//! End-to-end HTTP tests against a real local server.
//!
//! # What these prove, and what they do not
//!
//! These tests drive the **real** `reqwest` transport over a real TCP socket to a
//! throwaway server bound to `127.0.0.1`. Nothing is mocked at the HTTP layer, so
//! they verify the actual request bytes, headers, retry behaviour and duplicate
//! suppression.
//!
//! They do **not** prove anything about a third-party production API: no external
//! service is contacted, no real endpoint is exercised, and no TLS handshake
//! happens (the loopback server speaks plain HTTP, which is why the config has to
//! opt in to insecure HTTP). Treat a green run here as "our client behaves
//! correctly against a controlled server", never as "the integration works
//! against provider X".
#![cfg(feature = "http")]

use handy_voice_api::api::reqwest_transport::ReqwestTransport;
use handy_voice_api::api::{
    ApiClient, ApiConfig, AuthScheme, HttpMethod, Idempotency, RetryPolicy, UploadOutcome,
};
use handy_voice_api::prelude::*;
use handy_voice_api::secrets::SecretString;
use handy_voice_api::time::FixedClock;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One request as the test server saw it on the wire.
#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RecordedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A minimal single-purpose HTTP/1.1 server.
///
/// Deliberately hand-rolled: pulling a web framework in as a dev-dependency to
/// test an HTTP client would add a large dependency tree for a few dozen lines of
/// parsing.
struct TestServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// A canned response the server will send for a given request index.
type Script = Vec<u16>;

impl TestServer {
    /// Start a server that answers with the given status codes in order, then 200.
    fn start(script: Script) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        // A read timeout lets the accept loop notice shutdown without a wakeup pipe.
        listener.set_nonblocking(false).expect("blocking listener");

        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let requests_for_thread = Arc::clone(&requests);
        let shutdown_for_thread = Arc::clone(&shutdown);

        let handle = std::thread::spawn(move || {
            let mut index = 0usize;
            loop {
                if shutdown_for_thread.load(Ordering::Acquire) {
                    return;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let status = script.get(index).copied().unwrap_or(200);
                        index += 1;
                        if let Some(req) = handle_connection(stream, status) {
                            requests_for_thread.lock().unwrap().push(req);
                        }
                    }
                    Err(_) => return,
                }
            }
        });

        Self {
            addr,
            requests,
            shutdown,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// Wait until at least `n` requests have been recorded, or time out.
    fn wait_for_requests(&self, n: usize, timeout: Duration) -> Vec<RecordedRequest> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let seen = self.requests();
            if seen.len() >= n || std::time::Instant::now() > deadline {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // Unblock the accept loop by connecting once.
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_connection(mut stream: TcpStream, status: u16) -> Option<RecordedRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    // --- read headers ---
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => buf.push(byte[0]),
            Err(_) => return None,
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
    }

    let head = String::from_utf8_lossy(&buf).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    // --- read body ---
    let content_length = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body).ok()?;
    }

    // --- respond ---
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        reason = if status < 300 { "OK" } else { "Error" }
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();

    Some(RecordedRequest {
        method,
        path,
        headers,
        body,
    })
}

fn transcript() -> Transcript {
    let mut m = TranscriptManager::new(
        Box::new(FixedClock::reference()),
        "session-integration",
        "zh-TW",
        "microphone",
    );
    m.apply(TranscriptUpdate::final_(
        "seg-1",
        1,
        "你好，我想測試語音辨識。",
    ));
    m.into_transcript()
}

/// A config pointed at the loopback server.
///
/// `allow_insecure_http` is required precisely because this is plain HTTP — the
/// same opt-in a user would need for a local backend.
fn local_config(server: &TestServer) -> ApiConfig {
    let mut config = ApiConfig::new(server.base_url());
    config.endpoint = "transcripts".to_string();
    config.allow_insecure_http = true;
    config.timeout_ms = 5_000;
    config.retry = RetryPolicy {
        max_attempts: 3,
        base_delay_ms: 10,
        max_delay_ms: 50,
    };
    config
}

const API_KEY: &str = "sk-live-e2e-must-not-leak";

/// Build a client with the shared test credential and no retry backoff.
fn client(config: ApiConfig) -> ApiClient<ReqwestTransport> {
    let transport = ReqwestTransport::new(true).expect("transport");
    ApiClient::new(
        config,
        AuthScheme::Bearer(SecretString::new(API_KEY)),
        transport,
    )
    // Retry backoff would otherwise make the suite slow.
    .with_sleeper(|_| {})
}

#[test]
fn a_real_upload_reaches_the_server_with_the_expected_bytes() {
    let server = TestServer::start(vec![201]);
    let c = client(local_config(&server));

    let outcome = c.upload(&transcript()).expect("upload should succeed");
    match outcome {
        UploadOutcome::Uploaded {
            status, attempts, ..
        } => {
            assert_eq!(status, 201);
            assert_eq!(attempts, 1);
        }
        other => panic!("expected Uploaded, got {other:?}"),
    }

    let requests = server.wait_for_requests(1, Duration::from_secs(5));
    assert_eq!(requests.len(), 1);
    let req = &requests[0];

    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/transcripts");
    assert_eq!(req.header("content-type"), Some("application/json"));
    assert_eq!(
        req.header("authorization"),
        Some(format!("Bearer {API_KEY}").as_str())
    );
    assert!(req.header("idempotency-key").is_some());

    // The body is real UTF-8 JSON carrying the Traditional Chinese text.
    let text = String::from_utf8(req.body.clone()).expect("body must be UTF-8");
    assert!(
        text.contains("你好，我想測試語音辨識。"),
        "body did not carry the CJK text: {text}"
    );
    let parsed: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(parsed["schemaversion"], "1.0");
    assert_eq!(parsed["sessionid"], "session-integration");
    assert_eq!(parsed["segments"][0]["final"], serde_json::json!(true));
}

#[test]
fn a_500_is_retried_over_real_sockets_until_it_succeeds() {
    let server = TestServer::start(vec![500, 502, 200]);
    let c = client(local_config(&server));

    let outcome = c.upload(&transcript()).expect("should succeed on retry");
    match outcome {
        UploadOutcome::Uploaded {
            status, attempts, ..
        } => {
            assert_eq!(status, 200);
            assert_eq!(attempts, 3, "two failures then success");
        }
        other => panic!("expected Uploaded, got {other:?}"),
    }

    let requests = server.wait_for_requests(3, Duration::from_secs(5));
    assert_eq!(
        requests.len(),
        3,
        "the server must have seen three attempts"
    );
    // Every retry carries the same idempotency key, so the server can dedupe.
    let keys: Vec<_> = requests
        .iter()
        .map(|r| r.header("idempotency-key").unwrap_or_default().to_string())
        .collect();
    assert_eq!(keys[0], keys[1]);
    assert_eq!(keys[1], keys[2]);
}

#[test]
fn a_401_is_not_retried_over_real_sockets() {
    let server = TestServer::start(vec![401, 200]);
    let c = client(local_config(&server));

    let err = c.upload(&transcript()).unwrap_err();
    assert!(
        matches!(err, VoiceApiError::HttpStatus { status: 401, .. }),
        "got: {err}"
    );

    // Give any erroneous retry a chance to arrive before asserting.
    let requests = server.wait_for_requests(2, Duration::from_millis(400));
    assert_eq!(requests.len(), 1, "a 401 must not be retried");
}

#[test]
fn a_429_is_retried_over_real_sockets() {
    let server = TestServer::start(vec![429, 200]);
    let c = client(local_config(&server));

    let outcome = c.upload(&transcript()).expect("429 then success");
    assert!(matches!(
        outcome,
        UploadOutcome::Uploaded { attempts: 2, .. }
    ));
    assert_eq!(server.wait_for_requests(2, Duration::from_secs(5)).len(), 2);
}

#[test]
fn a_timeout_against_a_silent_server_is_reported_as_a_timeout() {
    // A listener that accepts but never responds.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_thread = Arc::clone(&stop);
    let handle = std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            if stop_for_thread.load(Ordering::Acquire) {
                break;
            }
            // Hold the connection open without answering.
            if let Ok(s) = stream {
                held.push(s);
            }
        }
        drop(held);
    });

    let mut config = ApiConfig::new(format!("http://{addr}"));
    config.allow_insecure_http = true;
    config.timeout_ms = 300; // short, so the test is quick
    config.retry = RetryPolicy {
        max_attempts: 1,
        base_delay_ms: 10,
        max_delay_ms: 10,
    };
    let transport = ReqwestTransport::new(true).unwrap();
    let c = ApiClient::new(config, AuthScheme::None, transport).with_sleeper(|_| {});

    let err = c.upload(&transcript()).unwrap_err();
    assert!(
        matches!(err, VoiceApiError::Timeout { .. }),
        "expected a timeout, got: {err}"
    );

    stop.store(true, Ordering::Release);
    let _ = TcpStream::connect(addr);
    let _ = handle.join();
}

#[test]
fn a_duplicate_upload_is_suppressed_before_reaching_the_server() {
    let server = TestServer::start(vec![200]);
    let c = client(local_config(&server));
    let t = transcript();

    assert!(matches!(
        c.upload(&t).unwrap(),
        UploadOutcome::Uploaded { .. }
    ));
    assert!(
        matches!(c.upload(&t).unwrap(), UploadOutcome::AlreadyUploaded { .. }),
        "an unchanged re-upload must be suppressed locally"
    );

    let requests = server.wait_for_requests(2, Duration::from_millis(400));
    assert_eq!(requests.len(), 1, "the server must have seen one request");
}

#[test]
fn a_wrapped_body_shape_reaches_the_server_as_configured() {
    let server = TestServer::start(vec![200]);
    let mut config = local_config(&server);
    config.request_format = handy_voice_api::api::RequestFormat::Wrapped {
        field: "transcript".to_string(),
    };
    let c = client(config);

    c.upload(&transcript()).unwrap();

    let requests = server.wait_for_requests(1, Duration::from_secs(5));
    let parsed: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(parsed["transcript"]["sessionid"], "session-integration");
}

#[test]
fn a_custom_method_and_header_reach_the_server() {
    let server = TestServer::start(vec![200]);
    let mut config = local_config(&server);
    config.method = HttpMethod::Put;
    config
        .headers
        .insert("X-Tenant".to_string(), "acme".to_string());
    config.idempotency = Idempotency::Disabled;
    let c = client(config);

    c.upload(&transcript()).unwrap();

    let requests = server.wait_for_requests(1, Duration::from_secs(5));
    let req = &requests[0];
    assert_eq!(req.method, "PUT");
    assert_eq!(req.header("x-tenant"), Some("acme"));
    assert!(req.header("idempotency-key").is_none());
}

#[test]
fn a_connection_test_against_a_real_server_reports_success_without_uploading() {
    let server = TestServer::start(vec![200]);
    let c = client(local_config(&server));

    let result = c.test_connection(&transcript());
    assert!(result.ok, "detail: {}", result.detail);
    assert_eq!(result.status, Some(200));
    assert_eq!(result.attempts, 1);
}

#[test]
fn a_connection_test_against_a_closed_port_fails_cleanly() {
    // Bind then drop, so the port is almost certainly free.
    let addr = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let mut config = ApiConfig::new(format!("http://{addr}"));
    config.allow_insecure_http = true;
    config.timeout_ms = 500;
    config.retry = RetryPolicy {
        max_attempts: 2,
        base_delay_ms: 10,
        max_delay_ms: 10,
    };
    let transport = ReqwestTransport::new(true).unwrap();
    let c = ApiClient::new(
        config,
        AuthScheme::Bearer(SecretString::new(API_KEY)),
        transport,
    )
    .with_sleeper(|_| {});

    let result = c.test_connection(&transcript());
    assert!(!result.ok);
    assert!(
        result.status.is_none(),
        "no HTTP status for a refused connection"
    );
    assert!(
        !result.detail.contains(API_KEY),
        "the diagnostic leaked the key: {}",
        result.detail
    );

    // And a real upload to the same dead endpoint surfaces the transport error.
    let err = c.upload(&transcript()).unwrap_err();
    assert!(err.is_retryable(), "a refused connection is retryable");
}

#[test]
fn plain_http_is_refused_unless_explicitly_allowed_even_for_loopback() {
    let server = TestServer::start(vec![200]);
    let mut config = ApiConfig::new(server.base_url());
    config.endpoint = "transcripts".into();
    // allow_insecure_http deliberately left false.
    let c = client(config);

    let err = c.upload(&transcript()).unwrap_err();
    assert!(err.to_string().contains("plain http"), "got: {err}");
    assert_eq!(
        server
            .wait_for_requests(1, Duration::from_millis(300))
            .len(),
        0,
        "nothing may be sent over unencrypted HTTP without opt-in"
    );
}
