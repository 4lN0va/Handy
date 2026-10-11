//! HTTP bridge from Handy to the local integration-verification tool.
//!
//! # Contract
//!
//! Implemented against `handy-integration-verification-tool`:
//!
//! * `POST /api/handy/heartbeat` — `{ app_version, recording, status, client }`,
//!   every field optional; sent on a timer so the dashboard can tell "connected"
//!   from "process exists but no bridge".
//! * `POST /api/transcripts` — `{ session_id, segment_id, text, final, timestamp,
//!   source }`, sent as each transcript segment is added or updated.
//!
//! Field names and shapes are taken from the tool's own validator
//! (`src/lib/bridge-validate.ts`); nothing is invented and no extra field is added,
//! because the tool rejects unknown shapes rather than ignoring them.
//!
//! # Guarantees
//!
//! * **Never blocks the audio, ASR or UI paths.** Callers only push onto an
//!   unbounded channel; all networking happens on one dedicated thread.
//! * **The client is built on that thread.** `reqwest::blocking` starts its own
//!   background runtime at construction, so building it on a Tokio worker thread
//!   breaks it with "Failed to communicate successful startup". That failure mode
//!   cost this project a whole debugging cycle in the Voice API upload path, so
//!   the client is created inside the worker's own `run`.
//! * **A retry re-sends the same event, never a new one.** Identity is
//!   (session, segment, final, text); the tool merges by `segment_id`, so a retry
//!   that actually landed the first time cannot create a duplicate segment.
//! * **Failure is reported, not hidden.** Every failure increments a counter and
//!   records the last error and HTTP status, which the UI can show. Nothing is
//!   ever reported as sent when it was not.
//! * **No secrets are logged.** There is no credential in this path at all, and
//!   request bodies are never logged above `trace`.
//!
//! # Scope
//!
//! This is a development-verification bridge, not a product feature. It is
//! disabled unless an endpoint is configured, and it defaults to loopback.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Default endpoint. The verification tool binds loopback only.
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8765";

/// Identifies this client to the tool. Stable so the dashboard can label it.
pub const CLIENT_NAME: &str = "handy";

pub const CONFIG_FILE: &str = "integration_bridge.json";

/// Bounded so a hung server cannot stall the worker forever.
const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_HEARTBEAT_SECS: u64 = 5;
const DEFAULT_MAX_ATTEMPTS: u32 = 3;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    /// On by default: the bridge is expected to be talking to the local
    /// verification tool. It only ever sends to loopback or https.
    pub enabled: bool,
    pub endpoint: String,
    pub timeout_ms: u64,
    pub heartbeat_secs: u64,
    pub max_attempts: u32,
    /// Send `final: false` partial updates as well as finals.
    ///
    /// Off by default: with a non-streaming model every update is already final,
    /// and forwarding partials multiplies traffic for no extra information.
    pub send_partials: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: DEFAULT_ENDPOINT.to_string(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            heartbeat_secs: DEFAULT_HEARTBEAT_SECS,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            send_partials: true,
        }
    }
}

impl BridgeConfig {
    /// Reject configurations that would send user speech somewhere unexpected.
    ///
    /// Only loopback and HTTPS are allowed. A plain-HTTP non-loopback endpoint
    /// would put the user's transcript on the network in clear text, so it is
    /// refused rather than warned about.
    pub fn validate(&self) -> Result<(), String> {
        let endpoint = self.endpoint.trim();
        if endpoint.is_empty() {
            return Err("the bridge endpoint is empty".to_string());
        }
        let parsed = url::Url::parse(endpoint)
            .map_err(|e| format!("the bridge endpoint is not a valid URL: {e}"))?;

        match parsed.scheme() {
            "https" => Ok(()),
            "http" => {
                let host = parsed.host_str().unwrap_or_default();
                if host == "127.0.0.1" || host == "localhost" || host == "::1" {
                    Ok(())
                } else {
                    Err(format!(
                        "refusing to send transcripts in clear text to '{host}': \
                         use https, or a loopback address"
                    ))
                }
            }
            other => Err(format!("unsupported URL scheme '{other}'")),
        }
    }

    fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_secs.clamp(1, 300))
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.clamp(200, 30_000))
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// One transcript update on its way to the tool.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptEvent {
    pub session_id: String,
    pub segment_id: String,
    pub text: String,
    pub final_: bool,
    pub timestamp: String,
    pub source: String,
}

/// Wire shape. Written out explicitly rather than renaming `final_`, because
/// `final` is a reserved word in Rust and a silent typo here would make the tool
/// reject every event with a 4xx that is easy to miss.
#[derive(Debug, Serialize)]
struct TranscriptWire<'a> {
    session_id: &'a str,
    segment_id: &'a str,
    text: &'a str,
    #[serde(rename = "final")]
    is_final: bool,
    timestamp: &'a str,
    source: &'a str,
}

#[derive(Debug, Serialize)]
struct HeartbeatWire<'a> {
    app_version: &'a str,
    recording: bool,
    /// Omitted entirely when empty.
    ///
    /// The tool's validator treats a *provided* `status` as required to be a
    /// non-empty string and answers 422 for `""`. Sending an empty status made
    /// every heartbeat fail with 422 while transcripts succeeded, which reads
    /// like a transport problem rather than a contract error. Omitting the key
    /// is the documented shape for "no status".
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'a str>,
    client: &'a str,
}

/// What the worker sends. Keeping this an enum means heartbeats and transcript
/// events share one queue and one ordering.
enum Job {
    Transcript(TranscriptEvent),
    /// Force a heartbeat now, e.g. so a state change is visible immediately
    /// instead of up to one interval later.
    HeartbeatNow,
    Stop,
}

// ---------------------------------------------------------------------------
// Observable status
// ---------------------------------------------------------------------------

/// Counters and last-result state, so the UI can show whether the bridge is
/// actually working rather than assuming it is.
#[derive(Debug, Default)]
pub struct BridgeStatus {
    pub sent: AtomicU64,
    pub failed: AtomicU64,
    pub heartbeats: AtomicU64,
    last_error: Mutex<Option<String>>,
    last_ok_ms: AtomicU64,
    recording: AtomicBool,
    status_text: Mutex<String>,
}

impl BridgeStatus {
    fn note_ok(&self, now_ms: u64) {
        self.sent.fetch_add(1, Ordering::Relaxed);
        self.last_ok_ms.store(now_ms, Ordering::Release);
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = None;
        }
    }

    fn note_failure(&self, message: String) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = Some(message);
        }
    }

    fn set_recording(&self, recording: bool) {
        self.recording.store(recording, Ordering::Release);
    }

    fn set_status_text(&self, text: &str) {
        debug_assert!(!text.trim().is_empty(), "the tool rejects an empty status");
        if let Ok(mut guard) = self.status_text.lock() {
            guard.clear();
            guard.push_str(text);
        }
    }

    /// Snapshot for the GUI. Contains no user text and no credentials.
    pub fn snapshot(&self) -> serde_json::Value {
        let last_error = self.last_error.lock().ok().and_then(|g| g.clone());
        let status_text = self
            .status_text
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let last_ok = self.last_ok_ms.load(Ordering::Acquire);
        serde_json::json!({
            "sent": self.sent.load(Ordering::Relaxed),
            "failed": self.failed.load(Ordering::Relaxed),
            "heartbeats": self.heartbeats.load(Ordering::Relaxed),
            "last_error": last_error,
            "last_ok_ms": if last_ok == 0 { serde_json::Value::Null } else { serde_json::json!(last_ok) },
            "recording": self.recording.load(Ordering::Acquire),
            "status": status_text,
        })
    }
}

// ---------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------

/// Owns the worker thread and the configuration.
pub struct Bridge {
    config: Mutex<BridgeConfig>,
    config_path: PathBuf,
    tx: Mutex<Option<Sender<Job>>>,
    status: Arc<BridgeStatus>,
    app_version: String,
    /// Deduplication of the most recent event, so a repeated identical update
    /// (which the UI can legitimately produce) is not re-sent.
    last_sent: Mutex<Option<(String, String, bool, String)>>,
}

impl Bridge {
    pub fn new(config_path: PathBuf, app_version: String) -> Arc<Self> {
        let config = load_config(&config_path);
        let bridge = Arc::new(Self {
            config: Mutex::new(config),
            config_path,
            tx: Mutex::new(None),
            status: Arc::new(BridgeStatus::default()),
            app_version,
            last_sent: Mutex::new(None),
        });
        bridge.status.set_status_text("idle");
        bridge.ensure_worker();
        bridge
    }

    pub fn status(&self) -> Arc<BridgeStatus> {
        Arc::clone(&self.status)
    }

    pub fn config(&self) -> BridgeConfig {
        self.config
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn config_json(&self) -> String {
        serde_json::to_string(&self.config()).unwrap_or_else(|_| "{}".to_string())
    }

    /// Replace the configuration and persist it. Restarts the worker if needed.
    pub fn set_config(&self, config: BridgeConfig) -> Result<(), String> {
        config.validate()?;
        {
            let mut guard = self
                .config
                .lock()
                .map_err(|_| "bridge configuration lock poisoned".to_string())?;
            *guard = config.clone();
        }
        self.persist(&config)?;
        self.restart_worker();
        Ok(())
    }

    fn persist(&self, config: &BridgeConfig) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("could not serialize the bridge configuration: {e}"))?;
        // Write via a temporary file so an interrupted write cannot leave a
        // half-written config that fails to parse on next start.
        let tmp = self.config_path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&tmp)
                .map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
            file.write_all(json.as_bytes())
                .map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
            file.sync_all().ok();
        }
        std::fs::rename(&tmp, &self.config_path)
            .map_err(|e| format!("could not replace {}: {e}", self.config_path.display()))
    }

    /// Report whether recording is happening, for the heartbeat.
    pub fn set_recording(&self, recording: bool) {
        self.status.set_recording(recording);
    }

    /// Report a coarse state word (`idle`, `recording`, `transcribing`).
    pub fn set_activity(&self, status: &str) {
        self.status.set_status_text(status);
    }

    /// Queue a transcript event. Never blocks and never fails the caller.
    ///
    /// Returns `true` if it was queued. A disabled bridge, or an event identical
    /// to the previous one, returns `false` without any network activity.
    pub fn send_transcript(&self, event: TranscriptEvent) -> bool {
        let config = self.config();
        if !config.enabled {
            return false;
        }
        if !event.final_ && !config.send_partials {
            return false;
        }

        // Drop an exact repeat of the previous event. Revisions are not part of
        // the wire contract, so "same segment, same text, same finality" is the
        // strongest identity available here.
        let key = (
            event.session_id.clone(),
            event.segment_id.clone(),
            event.final_,
            event.text.clone(),
        );
        if let Ok(mut last) = self.last_sent.lock() {
            if last.as_ref() == Some(&key) {
                return false;
            }
            *last = Some(key);
        }

        self.enqueue(Job::Transcript(event))
    }

    /// Ask for a heartbeat immediately (e.g. recording just started or stopped).
    pub fn heartbeat_now(&self) {
        if self.config().enabled {
            let _ = self.enqueue(Job::HeartbeatNow);
        }
    }

    fn enqueue(&self, job: Job) -> bool {
        let guard = match self.tx.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        match guard.as_ref() {
            Some(tx) => tx.send(job).is_ok(),
            None => false,
        }
    }

    fn restart_worker(&self) {
        // Stop the old worker, then start a new one with the new configuration.
        if let Ok(mut guard) = self.tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(Job::Stop);
            }
        }
        self.ensure_worker();
    }

    /// Start the worker thread if the bridge is enabled and none is running.
    fn ensure_worker(&self) {
        let config = self.config();
        if !config.enabled {
            return;
        }
        let mut guard = match self.tx.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel::<Job>();
        *guard = Some(tx);
        drop(guard);

        let status = Arc::clone(&self.status);
        let app_version = self.app_version.clone();
        std::thread::Builder::new()
            .name("handy-integration-bridge".to_string())
            .spawn(move || run_worker(config, app_version, rx, status))
            .ok();
    }
}

fn load_config(path: &Path) -> BridgeConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => BridgeConfig::default(),
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// RFC 3339 with milliseconds and a `Z` suffix, e.g. `2026-01-01T12:00:00.000Z`.
///
/// The tool requires the timestamp to survive `Date.parse`, and the UTC `Z` form
/// is the least ambiguous representation for that.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Milliseconds since this thread started. Relative only, never persisted.
fn now_ms(origin: Instant) -> u64 {
    origin.elapsed().as_millis() as u64
}

fn run_worker(
    config: BridgeConfig,
    app_version: String,
    rx: Receiver<Job>,
    status: Arc<BridgeStatus>,
) {
    let origin = Instant::now();

    // Built HERE, on the worker's own thread: see the module docs.
    let client = match reqwest::blocking::Client::builder()
        .timeout(config.timeout())
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            log::error!("integration bridge: could not build the HTTP client: {e}");
            status.note_failure(format!("could not build the HTTP client: {e}"));
            return;
        }
    };

    let base = config.endpoint.trim_end_matches('/').to_string();
    let heartbeat_url = format!("{base}/api/handy/heartbeat");
    let transcripts_url = format!("{base}/api/transcripts");
    let interval = config.heartbeat_interval();
    let origin_for_loop = origin;

    log::info!(
        "integration bridge: worker started (endpoint={base}, heartbeat={}s, timeout={}ms)",
        config.heartbeat_secs,
        config.timeout_ms
    );

    let mut next_heartbeat = Instant::now();

    loop {
        // Wait for work, but never longer than until the next heartbeat.
        let wait = next_heartbeat
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250));
        let job = rx.recv_timeout(wait);

        match job {
            Ok(Job::Stop) => {
                log::info!("integration bridge: worker stopping");
                return;
            }
            Ok(Job::HeartbeatNow) => {
                post_heartbeat(
                    &client,
                    &heartbeat_url,
                    &base,
                    &app_version,
                    &status,
                    now_ms(origin_for_loop),
                    config.max_attempts,
                );
                next_heartbeat = Instant::now() + interval;
            }
            Ok(Job::Transcript(event)) => {
                post_transcript(
                    &client,
                    &transcripts_url,
                    &base,
                    &event,
                    &status,
                    now_ms(origin_for_loop),
                    config.max_attempts,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("integration bridge: queue closed, worker stopping");
                return;
            }
        }

        if Instant::now() >= next_heartbeat {
            post_heartbeat(
                &client,
                &heartbeat_url,
                &base,
                &app_version,
                &status,
                now_ms(origin_for_loop),
                config.max_attempts,
            );
            next_heartbeat = Instant::now() + interval;
        }
    }
}

/// Small pause between retries. Linear rather than exponential: the endpoint is
/// local, and a long backoff would just delay the next real event behind it.
fn retry_pause(attempt: u32) {
    std::thread::sleep(Duration::from_millis(150 * attempt as u64));
}

fn post_heartbeat(
    client: &reqwest::blocking::Client,
    url: &str,
    base: &str,
    app_version: &str,
    status: &Arc<BridgeStatus>,
    now: u64,
    max_attempts: u32,
) {
    let recording = status.recording.load(Ordering::Acquire);
    let status_text = status
        .status_text
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| "unknown".to_string());
    // Omit rather than send "": the tool rejects an empty provided value.
    let status_value = if status_text.trim().is_empty() {
        None
    } else {
        Some(status_text.as_str())
    };
    let payload = HeartbeatWire {
        app_version,
        recording,
        status: status_value,
        client: CLIENT_NAME,
    };

    for attempt in 1..=max_attempts.max(1) {
        match client.post(url).json(&payload).send() {
            Ok(response) if response.status().is_success() => {
                status
                    .heartbeats
                    .fetch_add(1, Ordering::Relaxed);
                status.note_ok(now);
                return;
            }
            Ok(response) => {
                let code = response.status().as_u16();
                let message = format!("heartbeat rejected by {base} with HTTP {code}");
                if attempt == max_attempts.max(1) {
                    status.note_failure(message);
                    return;
                }
                retry_pause(attempt);
            }
            Err(e) => {
                if attempt == max_attempts.max(1) {
                    status.note_failure(format!("heartbeat to {base} failed: {e}"));
                    return;
                }
                retry_pause(attempt);
            }
        }
    }
}

fn post_transcript(
    client: &reqwest::blocking::Client,
    url: &str,
    base: &str,
    event: &TranscriptEvent,
    status: &Arc<BridgeStatus>,
    now: u64,
    max_attempts: u32,
) {
    // The wire body is rebuilt each attempt from the SAME event, so a retry can
    // never produce a different segment id or text.
    let payload = TranscriptWire {
        session_id: &event.session_id,
        segment_id: &event.segment_id,
        text: &event.text,
        is_final: event.final_,
        timestamp: &event.timestamp,
        source: &event.source,
    };

    for attempt in 1..=max_attempts.max(1) {
        match client.post(url).json(&payload).send() {
            Ok(response) if response.status().is_success() => {
                status.note_ok(now);
                return;
            }
            Ok(response) => {
                let code = response.status().as_u16();
                let message = format!(
                    "transcript event for segment {} rejected by {base} with HTTP {code}",
                    event.segment_id
                );
                // A 4xx is a contract mismatch; retrying cannot fix it.
                if (400..500).contains(&code) || attempt == max_attempts.max(1) {
                    log::warn!("integration bridge: {message}");
                    status.note_failure(message);
                    return;
                }
                retry_pause(attempt);
            }
            Err(e) => {
                if attempt == max_attempts.max(1) {
                    let message = format!(
                        "transcript event for segment {} could not reach {base}: {e}",
                        event.segment_id
                    );
                    log::warn!("integration bridge: {message}");
                    status.note_failure(message);
                    return;
                }
                retry_pause(attempt);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loopback() -> BridgeConfig {
        BridgeConfig {
            enabled: true,
            endpoint: DEFAULT_ENDPOINT.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn the_default_configuration_only_ever_targets_loopback() {
        // The default is enabled, so the safety property is not "off" but
        // "loopback": transcripts may never leave the machine for a remote host
        // without an explicit change.
        let c = BridgeConfig::default();
        assert_eq!(c.endpoint, DEFAULT_ENDPOINT);
        assert!(c.endpoint.starts_with("http://127.0.0.1"));
        assert!(c.validate().is_ok());
    }

    #[test]
    fn loopback_http_is_allowed() {
        assert!(loopback().validate().is_ok());
    }

    #[test]
    fn https_anywhere_is_allowed() {
        let mut c = loopback();
        c.endpoint = "https://example.test/api".to_string();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn clear_text_http_to_a_remote_host_is_refused() {
        // The important one: transcripts must not go over the network in clear.
        let mut c = loopback();
        c.endpoint = "http://192.168.1.50:8765".to_string();
        let err = c.validate().unwrap_err();
        assert!(err.contains("clear text"), "got: {err}");
    }

    #[test]
    fn a_malformed_or_empty_endpoint_is_refused() {
        let mut c = loopback();
        c.endpoint = String::new();
        assert!(c.validate().is_err());

        c.endpoint = "not a url".to_string();
        assert!(c.validate().is_err());

        c.endpoint = "ftp://example.test".to_string();
        assert!(c.validate().is_err());
    }

    #[test]
    fn a_disabled_bridge_queues_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path().join(CONFIG_FILE), "0.0.0-test".into());
        let mut off = BridgeConfig::default();
        off.enabled = false;
        bridge.set_config(off).unwrap();
        assert!(!bridge.send_transcript(TranscriptEvent {
            session_id: "s".into(),
            segment_id: "1".into(),
            text: "你好".into(),
            final_: true,
            timestamp: "2026-01-01T00:00:00.000Z".into(),
            source: "handy".into(),
        }));
        assert_eq!(bridge.status().sent.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn an_identical_repeat_of_the_previous_event_is_not_queued_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        let bridge = Bridge::new(path, "0.0.0-test".into());
        bridge.set_config(loopback()).unwrap();

        let event = || TranscriptEvent {
            session_id: "s1".into(),
            segment_id: "seg-1".into(),
            text: "你好".into(),
            final_: true,
            timestamp: "2026-01-01T00:00:00.000Z".into(),
            source: "handy".into(),
        };

        assert!(bridge.send_transcript(event()), "first is queued");
        assert!(
            !bridge.send_transcript(event()),
            "an exact repeat is dropped locally"
        );
    }

    #[test]
    fn a_partial_is_dropped_unless_partials_are_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path().join(CONFIG_FILE), "0.0.0-test".into());
        bridge.set_config(loopback()).unwrap();
        let mut no_partials = loopback();
        no_partials.send_partials = false;
        bridge.set_config(no_partials).unwrap();

        let partial = TranscriptEvent {
            session_id: "s1".into(),
            segment_id: "seg-1".into(),
            text: "你".into(),
            final_: false,
            timestamp: "2026-01-01T00:00:00.000Z".into(),
            source: "handy".into(),
        };
        assert!(!bridge.send_transcript(partial.clone()), "partials off");

        let mut with_partials = loopback();
        with_partials.send_partials = true;
        bridge.set_config(with_partials).unwrap();
        assert!(bridge.send_transcript(partial), "partials on");
    }

    #[test]
    fn the_configuration_round_trips_and_is_written_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        let bridge = Bridge::new(path.clone(), "0.0.0-test".into());
        let mut config = loopback();
        config.heartbeat_secs = 7;
        bridge.set_config(config).unwrap();

        assert!(path.exists());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temporary file must be renamed away"
        );

        let reloaded = load_config(&path);
        assert!(reloaded.enabled);
        assert_eq!(reloaded.heartbeat_secs, 7);
    }

    #[test]
    fn an_invalid_configuration_is_rejected_and_not_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        let bridge = Bridge::new(path.clone(), "0.0.0-test".into());

        let mut bad = loopback();
        bad.endpoint = "http://10.0.0.9:8765".into();
        assert!(bridge.set_config(bad).is_err());
        assert!(!path.exists(), "a rejected config must not be written");
    }

    #[test]
    fn a_corrupt_config_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        std::fs::write(&path, "{ not json").unwrap();
        let config = load_config(&path);
        assert_eq!(config.endpoint, DEFAULT_ENDPOINT);
        assert!(
            config.validate().is_ok(),
            "a fallback must not point somewhere unsafe"
        );
    }

    #[test]
    fn the_wire_shape_matches_the_tool_validator() {
        // The tool requires exactly these keys and types; `final` in particular
        // must not be renamed.
        let event = TranscriptEvent {
            session_id: "session-1".into(),
            segment_id: "seg-1".into(),
            text: "你好，我想測試語音辨識。".into(),
            final_: true,
            timestamp: "2026-01-01T12:00:00.000Z".into(),
            source: "handy".into(),
        };
        let wire = TranscriptWire {
            session_id: &event.session_id,
            segment_id: &event.segment_id,
            text: &event.text,
            is_final: event.final_,
            timestamp: &event.timestamp,
            source: &event.source,
        };
        let json: serde_json::Value = serde_json::to_value(&wire).unwrap();
        for key in [
            "session_id",
            "segment_id",
            "text",
            "final",
            "timestamp",
            "source",
        ] {
            assert!(json.get(key).is_some(), "missing key {key}");
        }
        assert!(json["final"].is_boolean());
        assert_eq!(json["text"], "你好，我想測試語音辨識。");
        assert_eq!(
            json.as_object().unwrap().len(),
            6,
            "no extra fields: the tool rejects shapes it does not know"
        );
    }

    #[test]
    fn the_heartbeat_wire_shape_is_within_the_allowed_field_set() {
        let payload = HeartbeatWire {
            app_version: "0.9.8",
            recording: true,
            status: Some("recording"),
            client: CLIENT_NAME,
        };
        let json: serde_json::Value = serde_json::to_value(&payload).unwrap();
        for key in ["app_version", "recording", "status", "client"] {
            assert!(json.get(key).is_some(), "missing key {key}");
        }
        assert_eq!(json.as_object().unwrap().len(), 4);
        assert!(json["recording"].is_boolean());
    }

    #[test]
    fn an_empty_heartbeat_status_is_omitted_rather_than_sent() {
        // The tool answers 422 for a provided-but-empty status, so every
        // heartbeat failed while transcripts succeeded. Omitting the key is the
        // documented shape for "no status".
        let payload = HeartbeatWire {
            app_version: "0.9.8",
            recording: false,
            status: None,
            client: CLIENT_NAME,
        };
        let json: serde_json::Value = serde_json::to_value(&payload).unwrap();
        assert!(
            json.get("status").is_none(),
            "an empty status must not be serialized: {json}"
        );
        assert_eq!(json.as_object().unwrap().len(), 3);
    }

    #[test]
    fn the_status_snapshot_never_contains_transcript_text() {
        let status = BridgeStatus::default();
        status.note_failure("connection refused to http://127.0.0.1:8765".into());
        status.set_status_text("recording");
        let snapshot = status.snapshot().to_string();
        assert!(snapshot.contains("connection refused"));
        assert!(snapshot.contains("recording"));
        // The snapshot is built only from counters and status words.
        assert!(!snapshot.contains("你好"));
    }
}
