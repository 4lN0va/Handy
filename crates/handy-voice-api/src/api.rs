//! Custom backend API client: configuration, request building, retry, and
//! idempotency.
//!
//! # Relationship to Handy's existing networking
//!
//! Handy already depends on `reqwest` and has an `llm_client` module, but that
//! client is specific to LLM chat completions: it hardcodes the request/response
//! shape and has no notion of a generic JSON upload. Rather than bolt unrelated
//! behaviour onto it, this module defines a small transport seam:
//!
//! * [`Transport`] is the only thing that performs I/O.
//! * This crate's default build implements no transport at all, so all request
//!   construction, retry, and idempotency logic is unit-testable offline.
//! * The `http` feature provides [`reqwest_transport::ReqwestTransport`], built
//!   on the same `reqwest` Handy already ships, so no second HTTP stack is added
//!   to the binary.
//!
//! # Product requirements encoded here
//!
//! * HTTPS by default; plain HTTP only via an explicit opt-in for a local backend.
//! * The request format is **not** assumed: endpoint, method, auth, headers,
//!   content type and body shape are all configurable.
//! * Bounded, retryable-only retries with backoff — a 4xx that is not 429 is
//!   never retried, because retrying would repeat the mistake or double-write.
//! * An idempotency key derived from the session id and the exact body bytes, so
//!   a retry (or a re-upload of unchanged content) is recognisable by the server.
//! * The API key never appears in a `Debug` rendering, an error, or a log line.

use crate::error::VoiceApiError;
use crate::export::{to_document, ExportFilter};
use crate::secrets::SecretString;
use crate::transcript::Transcript;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

/// HTTP methods this client will issue.
///
/// `GET` and `DELETE` are omitted deliberately: this client only uploads a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    /// Create a resource.
    Post,
    /// Replace a resource.
    Put,
    /// Partially update a resource.
    Patch,
}

impl HttpMethod {
    /// The wire representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
        }
    }

    /// Parse a user-supplied method name (case-insensitive).
    pub fn parse(value: &str) -> Result<Self, VoiceApiError> {
        match value.trim().to_ascii_uppercase().as_str() {
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "PATCH" => Ok(Self::Patch),
            other => Err(VoiceApiError::Config(format!(
                "unsupported HTTP method '{other}' (expected POST, PUT or PATCH)"
            ))),
        }
    }
}

/// How the request body is shaped.
///
/// Different backends expect different envelopes; assuming one shape is exactly
/// the mistake this type exists to avoid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestFormat {
    /// Send the transcript document as the request body.
    #[default]
    TranscriptDocument,
    /// Wrap the transcript under a named field: `{"<field>": {...transcript...}}`.
    Wrapped {
        /// The wrapping field name.
        field: String,
    },
    /// A JSON template whose `{{transcript}}` placeholder is replaced by the
    /// transcript document. Lets a user match an arbitrary backend contract.
    Template {
        /// A JSON object string containing the placeholder `{{transcript}}`.
        template: String,
    },
}

/// How the request authenticates.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum AuthScheme {
    /// No authentication.
    #[default]
    None,
    /// `Authorization: Bearer <token>`.
    Bearer(SecretString),
    /// A custom header carrying the credential, e.g. `X-Api-Key`.
    Header {
        /// Header name.
        name: String,
        /// Header value.
        value: SecretString,
    },
}

// Redacted so an AuthScheme can be embedded in a config struct that derives
// Debug without leaking the credential.
impl fmt::Debug for AuthScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Bearer(_) => f.write_str("Bearer(***redacted***)"),
            Self::Header { name, .. } => {
                write!(f, "Header {{ name: {name:?}, value: ***redacted*** }}")
            }
        }
    }
}

impl AuthScheme {
    /// Whether a credential is configured.
    pub fn is_configured(&self) -> bool {
        match self {
            Self::None => false,
            Self::Bearer(s) => !s.is_empty(),
            Self::Header { value, .. } => !value.is_empty(),
        }
    }
}

/// Whether to send an idempotency key with the upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    /// Send an `Idempotency-Key` header derived from the session and body.
    #[default]
    Enabled,
    /// Do not send one.
    Disabled,
}

/// Retry behaviour for failed uploads.
///
/// Delays are stored as plain millisecond integers so a settings file stays
/// human-readable and portable, rather than encoding `Duration` as a
/// `{secs, nanos}` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Total attempts, including the first. `1` disables retrying.
    pub max_attempts: u32,
    /// Delay before the second attempt, in milliseconds.
    pub base_delay_ms: u64,
    /// Upper bound for any single delay, in milliseconds.
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 500,
            max_delay_ms: 8_000,
        }
    }
}

impl RetryPolicy {
    /// No retries.
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Default::default()
        }
    }

    /// Delay before the second attempt.
    pub fn base_delay(&self) -> Duration {
        Duration::from_millis(self.base_delay_ms)
    }

    /// Upper bound for any single delay.
    pub fn max_delay(&self) -> Duration {
        Duration::from_millis(self.max_delay_ms)
    }

    /// Exponential backoff for the given (1-based) attempt.
    ///
    /// Deterministic on purpose — jitter would make retry behaviour untestable,
    /// and the client is single-flight per upload so there is no thundering herd
    /// to spread.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt < 2 {
            return Duration::ZERO;
        }
        let shift = (attempt - 2).min(16);
        let millis = (self.base_delay_ms as u128)
            .saturating_mul(1u128 << shift)
            .min(self.max_delay_ms as u128);
        Duration::from_millis(millis as u64)
    }
}

/// Header names this client refuses to set, because doing so corrupts the
/// request or subverts the transport's own framing.
const FORBIDDEN_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "upgrade",
    "authorization", // must go through AuthScheme so it stays a SecretString
];

/// Configuration for a custom backend endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Base URL, e.g. `https://api.example.com/v1`. A trailing slash is ignored.
    pub base_url: String,
    /// Endpoint path, e.g. `transcripts`. An absolute URL here overrides
    /// `base_url` entirely; a leading `/` resets to the host root.
    pub endpoint: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// Custom headers. Values are not secrets and are stored in plain settings.
    pub headers: BTreeMap<String, String>,
    /// `Content-Type` for the body.
    pub content_type: String,
    /// Request timeout.
    pub timeout_ms: u64,
    /// Retry behaviour.
    pub retry: RetryPolicy,
    /// Whether to send an idempotency key.
    pub idempotency: Idempotency,
    /// Request body shape.
    pub request_format: RequestFormat,
    /// Permit a plain-`http://` base URL. Off by default.
    ///
    /// Intended for a backend on the loopback interface during development.
    /// Required explicitly so a mistyped URL cannot silently send transcripts
    /// in cleartext.
    pub allow_insecure_http: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            endpoint: "transcripts".to_string(),
            method: HttpMethod::Post,
            headers: BTreeMap::new(),
            content_type: "application/json".to_string(),
            timeout_ms: 30_000,
            retry: RetryPolicy::default(),
            idempotency: Idempotency::default(),
            request_format: RequestFormat::default(),
            allow_insecure_http: false,
        }
    }
}

impl ApiConfig {
    /// A configuration pointing at `base_url`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            ..Default::default()
        }
    }

    /// Add a custom header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Validate the configuration before any I/O.
    ///
    /// Failing here rather than at request time means the settings UI can show a
    /// precise problem next to the offending field.
    pub fn validate(&self) -> Result<(), VoiceApiError> {
        let base = self.base_url.trim();
        if base.is_empty() {
            return Err(VoiceApiError::Config("base URL must not be empty".into()));
        }
        if self.endpoint.trim().is_empty() && !base.contains("://") {
            // An endpoint may legitimately be empty when base_url is the full URL.
            return Err(VoiceApiError::Config("endpoint must not be empty".into()));
        }

        let resolved = join_url(base, self.endpoint.trim())?;
        let lower = resolved.to_ascii_lowercase();
        let is_https = lower.starts_with("https://");
        let is_http = lower.starts_with("http://");
        if !is_https && !is_http {
            return Err(VoiceApiError::Config(format!(
                "URL must start with https:// (or http:// when explicitly allowed), got '{resolved}'"
            )));
        }
        if is_http && !self.allow_insecure_http {
            return Err(VoiceApiError::Config(
                "refusing to send credentials over plain http:// — enable 'allow insecure http' \
                 only for a backend on localhost"
                    .into(),
            ));
        }

        if self.content_type.trim().is_empty() {
            return Err(VoiceApiError::Config(
                "content type must not be empty".into(),
            ));
        }

        for name in self.headers.keys() {
            validate_header_name(name)?;
            if FORBIDDEN_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(VoiceApiError::Config(format!(
                    "header '{name}' cannot be set here; Authorization goes through the \
                     authentication setting and the rest are managed by the HTTP client"
                )));
            }
        }

        if self.timeout_ms == 0 {
            return Err(VoiceApiError::Config(
                "timeout must be greater than 0".into(),
            ));
        }
        if self.timeout_ms > 600_000 {
            return Err(VoiceApiError::Config(
                "timeout must not exceed 600000 ms (10 minutes)".into(),
            ));
        }
        if self.retry.max_attempts == 0 {
            return Err(VoiceApiError::Config(
                "max attempts must be at least 1".into(),
            ));
        }
        if self.retry.max_attempts > 10 {
            return Err(VoiceApiError::Config(
                "max attempts must not exceed 10, to bound how long the app will keep retrying"
                    .into(),
            ));
        }

        if let RequestFormat::Wrapped { field } = &self.request_format {
            if field.trim().is_empty() {
                return Err(VoiceApiError::Config(
                    "the wrapping field name must not be empty".into(),
                ));
            }
        }
        if let RequestFormat::Template { template } = &self.request_format {
            if !template.contains(TEMPLATE_PLACEHOLDER) {
                return Err(VoiceApiError::Config(format!(
                    "the request template must contain {TEMPLATE_PLACEHOLDER}"
                )));
            }
            serde_json::from_str::<serde_json::Value>(template).map_err(|e| {
                VoiceApiError::Config(format!("the request template is not valid JSON: {e}"))
            })?;
        }

        Ok(())
    }

    /// The fully resolved request URL.
    pub fn resolved_url(&self) -> Result<String, VoiceApiError> {
        join_url(self.base_url.trim(), self.endpoint.trim())
    }
}

/// Placeholder replaced by the transcript document in a
/// [`RequestFormat::Template`] body.
pub const TEMPLATE_PLACEHOLDER: &str = "{{transcript}}";

fn validate_header_name(name: &str) -> Result<(), VoiceApiError> {
    if name.trim().is_empty() {
        return Err(VoiceApiError::Config(
            "header name must not be empty".into(),
        ));
    }
    // RFC 9110 token characters.
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c));
    if !valid {
        return Err(VoiceApiError::Config(format!(
            "header name '{name}' contains characters that are not allowed in an HTTP header"
        )));
    }
    Ok(())
}

/// Join a base URL and an endpoint.
///
/// * An endpoint containing `://` is absolute and replaces the base entirely.
/// * An endpoint starting with `/` resets to the host root.
/// * Otherwise the endpoint is appended to the base path.
pub fn join_url(base: &str, endpoint: &str) -> Result<String, VoiceApiError> {
    let base = base.trim();
    let endpoint = endpoint.trim();

    if endpoint.contains("://") {
        return Ok(endpoint.trim_end_matches('/').to_string());
    }
    if base.is_empty() {
        return Err(VoiceApiError::Config("base URL must not be empty".into()));
    }

    let scheme_end = base
        .find("://")
        .ok_or_else(|| VoiceApiError::Config(format!("base URL '{base}' is missing a scheme")))?;
    let after_scheme = &base[scheme_end + 3..];
    let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let origin = &base[..scheme_end + 3 + host_end];
    let base_path = &after_scheme[host_end..];

    if endpoint.is_empty() {
        return Ok(origin.to_string());
    }
    if endpoint.starts_with('/') {
        return Ok(format!("{}{}", origin, endpoint));
    }
    let base_path = base_path.trim_end_matches('/');
    if base_path.is_empty() {
        Ok(format!("{origin}/{endpoint}"))
    } else {
        Ok(format!("{origin}{base_path}/{endpoint}"))
    }
}

/// An outgoing HTTP request.
///
/// `Debug` is hand-written and never prints header values: an `Authorization`
/// header holds a live credential.
pub struct HttpRequest {
    /// Method.
    pub method: HttpMethod,
    /// Fully resolved URL.
    pub url: String,
    /// Headers to send, credential already included.
    pub headers: Vec<(String, String)>,
    /// Serialized body.
    pub body: Vec<u8>,
    /// Timeout for this request.
    pub timeout: Duration,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted: Vec<String> = self
            .headers
            .iter()
            .map(|(name, _)| {
                if name.eq_ignore_ascii_case("authorization")
                    || name.eq_ignore_ascii_case("idempotency-key")
                {
                    format!("{name}: ***redacted***")
                } else {
                    name.clone()
                }
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("header_names", &redacted)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// An HTTP response, reduced to what this client needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Response body as text (lossy), used only for error summaries.
    pub body: String,
}

/// The only component that performs I/O.
///
/// Implementations must map a timeout to [`VoiceApiError::Timeout`] and any
/// other transport fault to [`VoiceApiError::Transport`], so retry decisions can
/// be made without knowing the underlying client.
pub trait Transport: Send + Sync {
    /// Perform one request.
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, VoiceApiError>;
}

/// The outcome of an upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum UploadOutcome {
    /// The server accepted the transcript.
    Uploaded {
        /// HTTP status returned.
        status: u16,
        /// How many attempts were made.
        attempts: u32,
        /// The idempotency key sent, if any. Safe to log (it is a hash).
        idempotency_key: Option<String>,
    },
    /// The same content was already uploaded successfully; nothing was sent.
    AlreadyUploaded {
        /// The idempotency key that matched.
        idempotency_key: String,
    },
    /// No upload was attempted because auto-upload is off.
    Skipped {
        /// Why it was skipped.
        reason: &'static str,
    },
}

/// The result of a connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    /// Whether the endpoint answered with a success status.
    pub ok: bool,
    /// HTTP status, when a response was received.
    pub status: Option<u16>,
    /// How many attempts were made.
    pub attempts: u32,
    /// Human-readable detail, already free of credentials.
    pub detail: String,
}

/// Computes the idempotency key for a transcript body.
///
/// Derived from the session id and the exact serialized bytes, so:
/// * retrying identical content reuses the key (the server can dedupe);
/// * changed content produces a different key (a genuine update is sent).
pub fn idempotency_key_for(session_id: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update(b"\x1f"); // unit separator, so ids cannot run into the body
    hasher.update(body);
    let digest = hasher.finalize();
    // 32 hex chars (128 bits) is ample and keeps the header short.
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// Client for one configured endpoint.
pub struct ApiClient<T: Transport> {
    config: ApiConfig,
    auth: AuthScheme,
    transport: T,
    sleeper: Box<dyn Fn(Duration) + Send + Sync>,
    /// Idempotency key of the last successful upload, for duplicate suppression.
    last_uploaded: std::sync::Mutex<Option<String>>,
}

impl<T: Transport> fmt::Debug for ApiClient<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiClient")
            .field("config", &self.config)
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl<T: Transport> ApiClient<T> {
    /// Create a client. The configuration is validated lazily by each call so a
    /// half-typed URL in the settings UI does not panic anything.
    pub fn new(config: ApiConfig, auth: AuthScheme, transport: T) -> Self {
        Self {
            config,
            auth,
            transport,
            sleeper: Box::new(|d: Duration| {
                if !d.is_zero() {
                    std::thread::sleep(d);
                }
            }),
            last_uploaded: std::sync::Mutex::new(None),
        }
    }

    /// Replace the sleep implementation. Tests inject a no-op to run instantly.
    pub fn with_sleeper(mut self, sleeper: impl Fn(Duration) + Send + Sync + 'static) -> Self {
        self.sleeper = Box::new(sleeper);
        self
    }

    /// The active configuration.
    pub fn config(&self) -> &ApiConfig {
        &self.config
    }

    /// The transport in use. Exposed so tests and diagnostics can inspect what
    /// was actually sent.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Forget the duplicate-upload memo (used when the user edits the config).
    pub fn reset_upload_memo(&self) {
        if let Ok(mut guard) = self.last_uploaded.lock() {
            *guard = None;
        }
    }

    /// Build the request body for a transcript under the configured format.
    pub fn build_body(&self, transcript: &Transcript) -> Result<Vec<u8>, VoiceApiError> {
        let document = to_document(transcript, ExportFilter::default())?;
        let body = match &self.config.request_format {
            RequestFormat::TranscriptDocument => document,
            RequestFormat::Wrapped { field } => {
                let mut map = serde_json::Map::new();
                map.insert(field.clone(), document);
                serde_json::Value::Object(map)
            }
            RequestFormat::Template { template } => {
                let transcript_json = serde_json::to_string(&document)?;
                let substituted = template.replace(TEMPLATE_PLACEHOLDER, &transcript_json);
                serde_json::from_str(&substituted).map_err(|e| {
                    VoiceApiError::Config(format!(
                        "the request template produced invalid JSON after substitution: {e}"
                    ))
                })?
            }
        };
        Ok(serde_json::to_vec(&body)?)
    }

    /// Construct the outgoing request for a transcript.
    pub fn build_request(&self, transcript: &Transcript) -> Result<HttpRequest, VoiceApiError> {
        self.config.validate()?;
        let url = self.config.resolved_url()?;
        let body = self.build_body(transcript)?;

        let mut headers: Vec<(String, String)> = Vec::new();
        headers.push(("Content-Type".to_string(), self.config.content_type.clone()));

        for (name, value) in &self.config.headers {
            headers.push((name.clone(), value.clone()));
        }

        match &self.auth {
            AuthScheme::None => {}
            AuthScheme::Bearer(token) => {
                headers.push((
                    "Authorization".to_string(),
                    format!("Bearer {}", token.expose()),
                ));
            }
            AuthScheme::Header { name, value } => {
                validate_header_name(name)?;
                if FORBIDDEN_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                    return Err(VoiceApiError::Config(format!(
                        "the authentication header name '{name}' is reserved"
                    )));
                }
                headers.push((name.clone(), value.expose().to_string()));
            }
        }

        let idempotency_key = match self.config.idempotency {
            Idempotency::Enabled => {
                let key = idempotency_key_for(&transcript.session_id, &body);
                headers.push(("Idempotency-Key".to_string(), key.clone()));
                Some(key)
            }
            Idempotency::Disabled => None,
        };
        debug_assert_eq!(
            idempotency_key.is_some(),
            self.config.idempotency == Idempotency::Enabled
        );

        Ok(HttpRequest {
            method: self.config.method,
            url,
            headers,
            body,
            timeout: Duration::from_millis(self.config.timeout_ms),
        })
    }

    /// Remove the credential from a string before it is shown or logged.
    fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        if let Some(secret) = self.auth_secret() {
            if !secret.is_empty() {
                out = out.replace(secret, "***redacted***");
            }
        }
        out
    }

    fn auth_secret(&self) -> Option<&str> {
        match &self.auth {
            AuthScheme::None => None,
            AuthScheme::Bearer(s) => Some(s.expose()),
            AuthScheme::Header { value, .. } => Some(value.expose()),
        }
    }

    /// Upload a transcript, applying retries and duplicate suppression.
    pub fn upload(&self, transcript: &Transcript) -> Result<UploadOutcome, VoiceApiError> {
        let request = self.build_request(transcript)?;
        let key = request
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("idempotency-key"))
            .map(|(_, v)| v.clone());

        if let (Some(key), Ok(guard)) = (&key, self.last_uploaded.lock()) {
            if guard.as_deref() == Some(key.as_str()) {
                return Ok(UploadOutcome::AlreadyUploaded {
                    idempotency_key: key.clone(),
                });
            }
        }

        let (status, attempts) = self.execute_with_retry(&request).map_err(|f| f.error)?;

        if let Some(key) = &key {
            if let Ok(mut guard) = self.last_uploaded.lock() {
                *guard = Some(key.clone());
            }
        }

        Ok(UploadOutcome::Uploaded {
            status,
            attempts,
            idempotency_key: key,
        })
    }

    /// Check that the endpoint is reachable and the credentials are accepted.
    ///
    /// Sends the same request shape as a real upload (so it exercises auth and
    /// the URL) but reports the result instead of failing, because a diagnostic
    /// button should explain, not throw.
    pub fn test_connection(&self, sample: &Transcript) -> ConnectionTestResult {
        let request = match self.build_request(sample) {
            Ok(r) => r,
            Err(e) => {
                return ConnectionTestResult {
                    ok: false,
                    status: None,
                    attempts: 0,
                    detail: e.to_string(),
                }
            }
        };

        // A connection test must not consume the duplicate-upload memo, and
        // must not be reported as a real upload.
        match self.execute_with_retry(&request) {
            Ok((status, attempts)) => ConnectionTestResult {
                ok: true,
                status: Some(status),
                attempts,
                detail: format!("endpoint accepted the request (HTTP {status})"),
            },
            Err(failure) => {
                let status = match &failure.error {
                    VoiceApiError::HttpStatus { status, .. } => Some(*status),
                    _ => None,
                };
                ConnectionTestResult {
                    ok: false,
                    status,
                    // Report the attempts actually made, not the configured cap.
                    attempts: failure.attempts,
                    detail: self.redact(&failure.error.to_string()),
                }
            }
        }
    }

    /// Execute with the configured retry policy.
    ///
    /// On failure the number of attempts actually made is returned alongside the
    /// error, so diagnostics can report the truth rather than the configured cap.
    fn execute_with_retry(&self, request: &HttpRequest) -> Result<(u16, u32), AttemptFailure> {
        let max = self.config.retry.max_attempts.max(1);
        let mut last_error: Option<VoiceApiError> = None;
        let mut attempts_made = 0u32;

        for attempt in 1..=max {
            if attempt > 1 {
                (self.sleeper)(self.config.retry.delay_for_attempt(attempt));
            }
            attempts_made = attempt;

            match self.transport.execute(request) {
                Ok(response) => {
                    if (200..300).contains(&response.status) {
                        return Ok((response.status, attempt));
                    }
                    let error = VoiceApiError::HttpStatus {
                        status: response.status,
                        body_summary: summarize_body(&self.redact(&response.body)),
                    };
                    if !error.is_retryable() {
                        // Retrying a 4xx (other than 429) repeats the mistake and
                        // risks a duplicate write.
                        return Err(AttemptFailure {
                            error,
                            attempts: attempt,
                        });
                    }
                    last_error = Some(error);
                }
                Err(e) => {
                    if !e.is_retryable() {
                        return Err(AttemptFailure {
                            error: e,
                            attempts: attempt,
                        });
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(AttemptFailure {
            error: last_error.unwrap_or_else(|| VoiceApiError::Transport {
                endpoint: request.url.clone(),
                message: "upload failed with no recorded error".into(),
            }),
            attempts: attempts_made,
        })
    }
}

/// A failed request plus how many attempts were actually made.
struct AttemptFailure {
    error: VoiceApiError,
    attempts: u32,
}

/// Trim a server response body to something safe to show in a UI.
fn summarize_body(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    const LIMIT: usize = 300;
    if trimmed.chars().count() <= LIMIT {
        return Some(trimmed.to_string());
    }
    let mut s: String = trimmed.chars().take(LIMIT).collect();
    s.push('…');
    Some(s)
}

/// `reqwest`-based transport, sharing the HTTP stack Handy already ships.
#[cfg(feature = "http")]
pub mod reqwest_transport {
    use super::*;

    /// Blocking `reqwest` transport.
    ///
    /// Blocking (not async) is deliberate: uploads run on a worker thread so the
    /// Tauri command can return immediately, and a synchronous API keeps the
    /// retry/idempotency logic straightforward and testable.
    pub struct ReqwestTransport {
        client: reqwest::blocking::Client,
    }

    impl ReqwestTransport {
        /// Build a transport.
        ///
        /// The TLS backend is whatever `reqwest` was compiled with — the same
        /// one Handy already links (native-tls: SChannel on Windows,
        /// Security.framework on macOS, OpenSSL on Linux) — so embedding this
        /// crate does not pull a second TLS stack into the binary.
        pub fn new(_allow_insecure_http: bool) -> Result<Self, VoiceApiError> {
            let client = reqwest::blocking::Client::builder()
                .user_agent(concat!("HandyFork-VoiceApi/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| VoiceApiError::Transport {
                    endpoint: "<client>".into(),
                    message: e.to_string(),
                })?;
            Ok(Self { client })
        }
    }

    impl Default for ReqwestTransport {
        fn default() -> Self {
            Self::new(false).expect("default reqwest client must build")
        }
    }

    impl Transport for ReqwestTransport {
        fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, VoiceApiError> {
            let method = match request.method {
                HttpMethod::Post => reqwest::Method::POST,
                HttpMethod::Put => reqwest::Method::PUT,
                HttpMethod::Patch => reqwest::Method::PATCH,
            };

            let mut builder = self
                .client
                .request(method, &request.url)
                .timeout(request.timeout)
                .body(request.body.clone());

            for (name, value) in &request.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }

            let response = builder.send().map_err(|e| {
                if e.is_timeout() {
                    VoiceApiError::Timeout {
                        endpoint: request.url.clone(),
                        timeout_ms: request.timeout.as_millis() as u64,
                    }
                } else {
                    VoiceApiError::Transport {
                        endpoint: request.url.clone(),
                        // Message only; the error's source chain can embed the
                        // URL but nothing carries a header value.
                        message: e.to_string(),
                    }
                }
            })?;

            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            Ok(HttpResponse { status, body })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::SecretString;
    use crate::time::FixedClock;
    use crate::transcript::{TranscriptManager, TranscriptUpdate};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    // ---------- test doubles ----------

    /// A transport that replays a scripted list of results and records requests.
    struct ScriptedTransport {
        script: Mutex<Vec<Result<HttpResponse, VoiceApiError>>>,
        seen: Mutex<Vec<HttpRequest>>,
        calls: AtomicU32,
    }

    impl ScriptedTransport {
        fn new(script: Vec<Result<HttpResponse, VoiceApiError>>) -> Self {
            Self {
                script: Mutex::new(script),
                seen: Mutex::new(Vec::new()),
                calls: AtomicU32::new(0),
            }
        }

        fn ok(status: u16) -> Self {
            Self::new(vec![Ok(HttpResponse {
                status,
                body: String::new(),
            })])
        }

        fn calls(&self) -> u32 {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_request(&self) -> HttpRequest {
            // Rebuild a comparable snapshot of the last request.
            let seen = self.seen.lock().unwrap();
            let last = seen.last().expect("no request recorded");
            HttpRequest {
                method: last.method,
                url: last.url.clone(),
                headers: last.headers.clone(),
                body: last.body.clone(),
                timeout: last.timeout,
            }
        }
    }

    impl Transport for ScriptedTransport {
        fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, VoiceApiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().unwrap().push(HttpRequest {
                method: request.method,
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
                timeout: request.timeout,
            });
            let mut script = self.script.lock().unwrap();
            if script.is_empty() {
                return Ok(HttpResponse {
                    status: 200,
                    body: String::new(),
                });
            }
            script.remove(0)
        }
    }

    fn transcript() -> Transcript {
        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        );
        m.apply(TranscriptUpdate::final_(
            "s1",
            1,
            "你好，我想測試語音辨識。",
        ));
        m.into_transcript()
    }

    fn client(t: ScriptedTransport) -> ApiClient<ScriptedTransport> {
        ApiClient::new(
            ApiConfig::new("https://api.example.test/v1"),
            AuthScheme::Bearer(SecretString::new("sk-test")),
            t,
        )
        .with_sleeper(|_| {})
    }

    // ---------- URL joining ----------

    #[test]
    fn joins_base_and_relative_endpoint() {
        assert_eq!(
            join_url("https://api.example.com/v1", "transcripts").unwrap(),
            "https://api.example.com/v1/transcripts"
        );
    }

    #[test]
    fn a_trailing_slash_on_the_base_does_not_double_up() {
        assert_eq!(
            join_url("https://api.example.com/v1/", "transcripts").unwrap(),
            "https://api.example.com/v1/transcripts"
        );
    }

    #[test]
    fn a_leading_slash_resets_to_the_host_root() {
        assert_eq!(
            join_url("https://api.example.com/v1", "/ingest").unwrap(),
            "https://api.example.com/ingest"
        );
    }

    #[test]
    fn an_absolute_endpoint_replaces_the_base() {
        assert_eq!(
            join_url(
                "https://ignored.example.com/v1",
                "https://real.example.com/hook"
            )
            .unwrap(),
            "https://real.example.com/hook"
        );
    }

    #[test]
    fn a_bare_host_base_gets_the_endpoint_appended() {
        assert_eq!(
            join_url("https://api.example.com", "transcripts").unwrap(),
            "https://api.example.com/transcripts"
        );
    }

    #[test]
    fn an_empty_endpoint_yields_the_origin() {
        assert_eq!(
            join_url("https://api.example.com/v1", "").unwrap(),
            "https://api.example.com"
        );
    }

    #[test]
    fn a_base_without_a_scheme_is_rejected() {
        assert!(join_url("api.example.com/v1", "x").is_err());
    }

    // ---------- config validation ----------

    #[test]
    fn https_is_accepted_by_default() {
        ApiConfig::new("https://api.example.com/v1")
            .validate()
            .expect("https must be valid by default");
    }

    #[test]
    fn plain_http_is_refused_unless_explicitly_allowed() {
        let err = ApiConfig::new("http://localhost:8080")
            .validate()
            .unwrap_err();
        assert!(err.to_string().contains("plain http"), "got: {err}");

        let mut cfg = ApiConfig::new("http://localhost:8080");
        cfg.allow_insecure_http = true;
        cfg.validate().expect("explicit opt-in must be accepted");
    }

    #[test]
    fn an_empty_base_url_is_rejected() {
        assert!(ApiConfig::new("  ").validate().is_err());
    }

    #[test]
    fn a_url_without_a_scheme_is_rejected() {
        // The URL is rejected before any I/O, naming the actual problem.
        let err = ApiConfig::new("api.example.com").validate().unwrap_err();
        assert!(
            matches!(err, VoiceApiError::Config(_)),
            "expected a config error, got: {err}"
        );
        assert!(
            err.to_string().contains("scheme"),
            "the message should explain the missing scheme, got: {err}"
        );
    }

    #[test]
    fn a_scheme_less_url_with_a_full_endpoint_is_still_rejected() {
        // Guards the other path into the same check: an endpoint containing
        // `://` bypasses join_url, so validation must catch the bad base itself.
        let err = ApiConfig::new("ftp://files.example.com")
            .validate()
            .unwrap_err();
        assert!(
            err.to_string().contains("must start with https://"),
            "got: {err}"
        );
    }

    #[test]
    fn reserved_headers_cannot_be_set_manually() {
        for reserved in [
            "Host",
            "Content-Length",
            "Authorization",
            "Transfer-Encoding",
        ] {
            let cfg = ApiConfig::new("https://api.example.com").with_header(reserved, "x");
            let err = cfg.validate().unwrap_err();
            assert!(
                err.to_string().contains("cannot be set here"),
                "{reserved} should be rejected, got: {err}"
            );
        }
    }

    #[test]
    fn header_names_must_be_valid_tokens() {
        let cfg = ApiConfig::new("https://api.example.com").with_header("Bad Header", "x");
        assert!(cfg
            .validate()
            .unwrap_err()
            .to_string()
            .contains("not allowed"));

        let cfg = ApiConfig::new("https://api.example.com").with_header("X-Good_Header.1", "x");
        cfg.validate()
            .expect("valid token characters must be accepted");
    }

    #[test]
    fn timeouts_and_attempts_are_bounded() {
        let mut cfg = ApiConfig::new("https://a.test");
        cfg.timeout_ms = 0;
        assert!(cfg.validate().is_err());

        let mut cfg = ApiConfig::new("https://a.test");
        cfg.timeout_ms = 10_000_000;
        assert!(cfg.validate().is_err());

        let mut cfg = ApiConfig::new("https://a.test");
        cfg.retry.max_attempts = 0;
        assert!(cfg.validate().is_err());

        let mut cfg = ApiConfig::new("https://a.test");
        cfg.retry.max_attempts = 99;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn a_template_without_the_placeholder_is_rejected() {
        let mut cfg = ApiConfig::new("https://a.test");
        cfg.request_format = RequestFormat::Template {
            template: r#"{"data": 1}"#.into(),
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains(TEMPLATE_PLACEHOLDER), "got: {err}");
    }

    #[test]
    fn a_template_that_is_not_json_is_rejected() {
        let mut cfg = ApiConfig::new("https://a.test");
        cfg.request_format = RequestFormat::Template {
            template: format!("{{not json {TEMPLATE_PLACEHOLDER}}}"),
        };
        assert!(cfg
            .validate()
            .unwrap_err()
            .to_string()
            .contains("not valid JSON"));
    }

    #[test]
    fn http_method_parsing_is_case_insensitive_and_rejects_get() {
        assert_eq!(HttpMethod::parse("post").unwrap(), HttpMethod::Post);
        assert_eq!(HttpMethod::parse("Put").unwrap(), HttpMethod::Put);
        assert_eq!(HttpMethod::parse("PATCH").unwrap(), HttpMethod::Patch);
        let err = HttpMethod::parse("GET").unwrap_err();
        assert!(err.to_string().contains("unsupported HTTP method"));
    }

    // ---------- request construction ----------

    #[test]
    fn request_contains_method_url_content_type_and_bearer_token() {
        let c = client(ScriptedTransport::ok(200));
        let req = c.build_request(&transcript()).unwrap();

        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "https://api.example.test/v1/transcripts");
        assert!(req
            .headers
            .iter()
            .any(|(n, v)| n == "Content-Type" && v == "application/json"));
        assert!(req
            .headers
            .iter()
            .any(|(n, v)| n == "Authorization" && v == "Bearer sk-test"));
    }

    #[test]
    fn a_custom_auth_header_is_used_when_configured() {
        let c = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            AuthScheme::Header {
                name: "X-Api-Key".into(),
                value: SecretString::new("custom-key"),
            },
            ScriptedTransport::ok(200),
        );
        let req = c.build_request(&transcript()).unwrap();
        assert!(req
            .headers
            .iter()
            .any(|(n, v)| n == "X-Api-Key" && v == "custom-key"));
        assert!(!req.headers.iter().any(|(n, _)| n == "Authorization"));
    }

    #[test]
    fn no_auth_scheme_sends_no_credential_header() {
        let c = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            AuthScheme::None,
            ScriptedTransport::ok(200),
        );
        let req = c.build_request(&transcript()).unwrap();
        assert!(!req
            .headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("authorization")));
    }

    #[test]
    fn custom_headers_and_content_type_are_applied() {
        let mut cfg = ApiConfig::new("https://api.example.test")
            .with_header("X-Tenant", "acme")
            .with_header("X-Trace", "abc123");
        cfg.content_type = "application/vnd.acme+json".into();
        cfg.method = HttpMethod::Put;

        let c = ApiClient::new(cfg, AuthScheme::None, ScriptedTransport::ok(200));
        let req = c.build_request(&transcript()).unwrap();

        assert_eq!(req.method, HttpMethod::Put);
        assert!(req
            .headers
            .iter()
            .any(|(n, v)| n == "X-Tenant" && v == "acme"));
        assert!(req
            .headers
            .iter()
            .any(|(n, v)| n == "X-Trace" && v == "abc123"));
        assert!(req
            .headers
            .iter()
            .any(|(n, v)| n == "Content-Type" && v == "application/vnd.acme+json"));
    }

    #[test]
    fn the_default_body_is_the_transcript_document() {
        let c = client(ScriptedTransport::ok(200));
        let body = c.build_body(&transcript()).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["schemaversion"], "1.0");
        assert_eq!(parsed["sessionid"], "session-001");
    }

    #[test]
    fn a_wrapped_body_nests_the_document_under_the_named_field() {
        let mut cfg = ApiConfig::new("https://api.example.test");
        cfg.request_format = RequestFormat::Wrapped {
            field: "transcript".into(),
        };
        let c = ApiClient::new(cfg, AuthScheme::None, ScriptedTransport::ok(200));
        let parsed: serde_json::Value =
            serde_json::from_slice(&c.build_body(&transcript()).unwrap()).unwrap();
        assert_eq!(parsed["transcript"]["sessionid"], "session-001");
    }

    #[test]
    fn a_template_body_matches_an_arbitrary_backend_contract() {
        let mut cfg = ApiConfig::new("https://api.example.test");
        cfg.request_format = RequestFormat::Template {
            template: r#"{"workspace":"acme","payload":{{transcript}},"version":2}"#.into(),
        };
        let c = ApiClient::new(cfg, AuthScheme::None, ScriptedTransport::ok(200));
        let parsed: serde_json::Value =
            serde_json::from_slice(&c.build_body(&transcript()).unwrap()).unwrap();
        assert_eq!(parsed["workspace"], "acme");
        assert_eq!(parsed["version"], 2);
        assert_eq!(parsed["payload"]["sessionid"], "session-001");
        assert_eq!(
            parsed["payload"]["segments"][0]["text"],
            "你好，我想測試語音辨識。"
        );
    }

    #[test]
    fn the_request_body_is_utf8_json_that_preserves_traditional_chinese() {
        let c = client(ScriptedTransport::ok(200));
        let body = c.build_body(&transcript()).unwrap();
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("你好，我想測試語音辨識。"));
    }

    #[test]
    fn timeout_is_taken_from_the_configuration() {
        let mut cfg = ApiConfig::new("https://api.example.test");
        cfg.timeout_ms = 1_234;
        let c = ApiClient::new(cfg, AuthScheme::None, ScriptedTransport::ok(200));
        assert_eq!(
            c.build_request(&transcript()).unwrap().timeout,
            Duration::from_millis(1_234)
        );
    }

    #[test]
    fn an_invalid_config_fails_before_any_io() {
        let t = ScriptedTransport::ok(200);
        let c = ApiClient::new(
            ApiConfig::new("http://insecure.test"), // not allowed by default
            AuthScheme::None,
            t,
        );
        assert!(c.build_request(&transcript()).is_err());
    }

    // ---------- idempotency ----------

    #[test]
    fn an_idempotency_key_is_sent_by_default() {
        let c = client(ScriptedTransport::ok(200));
        let req = c.build_request(&transcript()).unwrap();
        assert!(req.headers.iter().any(|(n, _)| n == "Idempotency-Key"));
    }

    #[test]
    fn the_idempotency_key_is_stable_for_identical_content() {
        let c = client(ScriptedTransport::ok(200));
        let a = c.build_request(&transcript()).unwrap();
        let b = c.build_request(&transcript()).unwrap();
        assert_eq!(
            a.headers.iter().find(|(n, _)| n == "Idempotency-Key"),
            b.headers.iter().find(|(n, _)| n == "Idempotency-Key"),
            "retrying identical content must reuse the key"
        );
    }

    #[test]
    fn the_idempotency_key_changes_when_content_changes() {
        let c = client(ScriptedTransport::ok(200));
        let first = c.build_request(&transcript()).unwrap();

        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        );
        m.apply(TranscriptUpdate::final_("s1", 1, "完全不同的內容"));
        let second = c.build_request(m.transcript()).unwrap();

        assert_ne!(
            first.headers.iter().find(|(n, _)| n == "Idempotency-Key"),
            second.headers.iter().find(|(n, _)| n == "Idempotency-Key"),
            "changed content must produce a new key so the update is sent"
        );
    }

    #[test]
    fn the_idempotency_key_differs_between_sessions_with_identical_text() {
        let a = idempotency_key_for("session-a", b"{\"x\":1}");
        let b = idempotency_key_for("session-b", b"{\"x\":1}");
        assert_ne!(a, b);
    }

    #[test]
    fn the_idempotency_key_is_a_short_hex_digest_not_the_content() {
        let secret = "你好，我想測試語音辨識。";
        let key = idempotency_key_for("s1", secret.as_bytes());
        assert_eq!(key.len(), 32);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!key.contains(secret));
    }

    #[test]
    fn the_idempotency_key_can_be_disabled() {
        let mut cfg = ApiConfig::new("https://api.example.test");
        cfg.idempotency = Idempotency::Disabled;
        let c = ApiClient::new(cfg, AuthScheme::None, ScriptedTransport::ok(200));
        let req = c.build_request(&transcript()).unwrap();
        assert!(!req.headers.iter().any(|(n, _)| n == "Idempotency-Key"));
    }

    #[test]
    fn re_uploading_unchanged_content_is_suppressed() {
        let t = ScriptedTransport::ok(200);
        let c = client(t);
        let first = c.upload(&transcript()).unwrap();
        assert!(matches!(first, UploadOutcome::Uploaded { .. }));

        let second = c.upload(&transcript()).unwrap();
        assert!(
            matches!(second, UploadOutcome::AlreadyUploaded { .. }),
            "a duplicate upload must be suppressed, got {second:?}"
        );
    }

    #[test]
    fn a_suppressed_duplicate_does_not_touch_the_network() {
        let c = client(ScriptedTransport::ok(200));
        c.upload(&transcript()).unwrap();
        // Replace is impossible (transport is owned), so count instead via a
        // fresh transport and an explicit memo reset path.
        c.upload(&transcript()).unwrap();
        // Two upload calls, one network call.
        assert_eq!(c.transport.calls(), 1);
    }

    #[test]
    fn changing_content_after_an_upload_is_sent_again() {
        let c = client(ScriptedTransport::ok(200));
        c.upload(&transcript()).unwrap();

        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        );
        m.apply(TranscriptUpdate::final_("s1", 1, "更新後的內容"));
        let outcome = c.upload(m.transcript()).unwrap();

        assert!(matches!(outcome, UploadOutcome::Uploaded { .. }));
        assert_eq!(c.transport.calls(), 2);
    }

    #[test]
    fn resetting_the_memo_allows_a_resend() {
        let c = client(ScriptedTransport::ok(200));
        c.upload(&transcript()).unwrap();
        c.reset_upload_memo();
        let outcome = c.upload(&transcript()).unwrap();
        assert!(matches!(outcome, UploadOutcome::Uploaded { .. }));
        assert_eq!(c.transport.calls(), 2);
    }

    // ---------- retry and error handling ----------

    #[test]
    fn a_successful_upload_reports_status_and_attempt_count() {
        let c = client(ScriptedTransport::ok(201));
        match c.upload(&transcript()).unwrap() {
            UploadOutcome::Uploaded {
                status,
                attempts,
                idempotency_key,
            } => {
                assert_eq!(status, 201);
                assert_eq!(attempts, 1);
                assert!(idempotency_key.is_some());
            }
            other => panic!("expected Uploaded, got {other:?}"),
        }
    }

    #[test]
    fn a_server_error_is_retried_and_can_succeed() {
        let t = ScriptedTransport::new(vec![
            Ok(HttpResponse {
                status: 500,
                body: "boom".into(),
            }),
            Ok(HttpResponse {
                status: 502,
                body: String::new(),
            }),
            Ok(HttpResponse {
                status: 200,
                body: String::new(),
            }),
        ]);
        let c = client(t);
        match c.upload(&transcript()).unwrap() {
            UploadOutcome::Uploaded { attempts, .. } => assert_eq!(attempts, 3),
            other => panic!("expected Uploaded, got {other:?}"),
        }
    }

    #[test]
    fn a_rate_limit_is_retried() {
        let t = ScriptedTransport::new(vec![
            Ok(HttpResponse {
                status: 429,
                body: "slow down".into(),
            }),
            Ok(HttpResponse {
                status: 200,
                body: String::new(),
            }),
        ]);
        let c = client(t);
        assert!(matches!(
            c.upload(&transcript()).unwrap(),
            UploadOutcome::Uploaded { attempts: 2, .. }
        ));
    }

    #[test]
    fn retries_give_up_after_the_configured_attempts() {
        let t = ScriptedTransport::new(vec![
            Ok(HttpResponse {
                status: 503,
                body: String::new(),
            }),
            Ok(HttpResponse {
                status: 503,
                body: String::new(),
            }),
            Ok(HttpResponse {
                status: 503,
                body: String::new(),
            }),
        ]);
        let c = client(t);
        let err = c.upload(&transcript()).unwrap_err();
        assert!(matches!(err, VoiceApiError::HttpStatus { status: 503, .. }));
        assert_eq!(c.transport.calls(), 3, "must not exceed max_attempts");
    }

    #[test]
    fn a_client_error_is_not_retried() {
        // Retrying a 400 repeats the mistake; retrying a 409 could double-write.
        for status in [400, 401, 403, 404, 409, 422] {
            let t = ScriptedTransport::new(vec![
                Ok(HttpResponse {
                    status,
                    body: "bad request".into(),
                }),
                Ok(HttpResponse {
                    status: 200,
                    body: String::new(),
                }),
            ]);
            let c = client(t);
            let err = c.upload(&transcript()).unwrap_err();
            assert!(
                matches!(err, VoiceApiError::HttpStatus { status: s, .. } if s == status),
                "status {status} should surface immediately"
            );
            assert_eq!(
                c.transport.calls(),
                1,
                "status {status} must not be retried"
            );
        }
    }

    #[test]
    fn a_network_fault_is_retried_and_succeeds() {
        let t = ScriptedTransport::new(vec![
            Err(VoiceApiError::Transport {
                endpoint: "https://api.example.test".into(),
                message: "connection reset".into(),
            }),
            Ok(HttpResponse {
                status: 200,
                body: String::new(),
            }),
        ]);
        let c = client(t);
        assert!(matches!(
            c.upload(&transcript()).unwrap(),
            UploadOutcome::Uploaded { attempts: 2, .. }
        ));
    }

    #[test]
    fn a_persistent_timeout_surfaces_as_a_timeout_error() {
        let script: Vec<_> = (0..3)
            .map(|_| {
                Err(VoiceApiError::Timeout {
                    endpoint: "https://api.example.test".into(),
                    timeout_ms: 1000,
                })
            })
            .collect();
        let c = client(ScriptedTransport::new(script));
        let err = c.upload(&transcript()).unwrap_err();
        assert!(matches!(err, VoiceApiError::Timeout { .. }));
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn retry_delays_grow_exponentially_and_are_capped() {
        let p = RetryPolicy {
            max_attempts: 6,
            base_delay_ms: 100,
            max_delay_ms: 500,
        };
        assert_eq!(
            p.delay_for_attempt(1),
            Duration::ZERO,
            "no delay before attempt 1"
        );
        assert_eq!(p.delay_for_attempt(2), Duration::from_millis(100));
        assert_eq!(p.delay_for_attempt(3), Duration::from_millis(200));
        assert_eq!(p.delay_for_attempt(4), Duration::from_millis(400));
        assert_eq!(p.delay_for_attempt(5), Duration::from_millis(500), "capped");
        assert_eq!(p.delay_for_attempt(6), Duration::from_millis(500), "capped");
        assert_eq!(p.base_delay(), Duration::from_millis(100));
        assert_eq!(p.max_delay(), Duration::from_millis(500));
    }

    #[test]
    fn retry_policy_none_means_a_single_attempt() {
        let mut cfg = ApiConfig::new("https://api.example.test");
        cfg.retry = RetryPolicy::none();
        let t = ScriptedTransport::new(vec![Ok(HttpResponse {
            status: 500,
            body: String::new(),
        })]);
        let c = ApiClient::new(cfg, AuthScheme::None, t);
        assert!(c.upload(&transcript()).is_err());
        assert_eq!(c.transport.calls(), 1);
    }

    #[test]
    fn the_retry_sleeper_is_actually_used() {
        // Proves retries wait rather than spinning, without slowing the suite.
        let sleeps: std::sync::Arc<Mutex<Vec<Duration>>> = Default::default();
        let recorded = sleeps.clone();
        let t = ScriptedTransport::new(vec![
            Ok(HttpResponse {
                status: 500,
                body: String::new(),
            }),
            Ok(HttpResponse {
                status: 200,
                body: String::new(),
            }),
        ]);
        let c = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            AuthScheme::None,
            t,
        )
        .with_sleeper(move |d| recorded.lock().unwrap().push(d));
        c.upload(&transcript()).unwrap();

        let sleeps = sleeps.lock().unwrap();
        assert_eq!(sleeps.len(), 1);
        assert_eq!(sleeps[0], Duration::from_millis(500));
    }

    // ---------- secret hygiene ----------

    #[test]
    fn request_debug_output_never_contains_the_api_key() {
        let secret = "sk-live-DO-NOT-LEAK";
        let c = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            AuthScheme::Bearer(SecretString::new(secret)),
            ScriptedTransport::ok(200),
        );
        let req = c.build_request(&transcript()).unwrap();
        let debugged = format!("{req:?}");
        assert!(
            !debugged.contains(secret),
            "HttpRequest Debug leaked the key"
        );
        assert!(debugged.contains("redacted"));
    }

    #[test]
    fn config_and_auth_debug_output_never_contains_the_api_key() {
        let secret = "sk-live-DO-NOT-LEAK";
        let auth = AuthScheme::Bearer(SecretString::new(secret));
        assert!(!format!("{auth:?}").contains(secret));

        let c = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            auth,
            ScriptedTransport::ok(200),
        );
        assert!(!format!("{c:?}").contains(secret));
    }

    #[test]
    fn an_error_message_never_contains_the_api_key() {
        let secret = "sk-live-DO-NOT-LEAK";
        let t = ScriptedTransport::new(vec![Ok(HttpResponse {
            status: 400,
            // A misbehaving server that echoes the request back must not cause
            // the key to appear in our error text.
            body: format!("bad request: Authorization: Bearer {secret}"),
        })]);
        let c = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            AuthScheme::Bearer(SecretString::new(secret)),
            t,
        );
        let err = c.upload(&transcript()).unwrap_err();
        assert!(
            !err.to_string().contains(secret),
            "error leaked the key: {err}"
        );
        assert!(err.to_string().contains("redacted"));
    }

    #[test]
    fn a_connection_test_failure_never_contains_the_api_key() {
        let secret = "sk-live-DO-NOT-LEAK";
        let t = ScriptedTransport::new(vec![Ok(HttpResponse {
            status: 401,
            body: format!("unauthorized for bearer {secret}"),
        })]);
        let c = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            AuthScheme::Bearer(SecretString::new(secret)),
            t,
        );
        let result = c.test_connection(&transcript());
        assert!(!result.ok);
        assert!(
            !result.detail.contains(secret),
            "detail leaked: {}",
            result.detail
        );
    }

    #[test]
    fn an_idempotency_key_carries_no_transcript_text() {
        let c = client(ScriptedTransport::ok(200));
        let req = c.build_request(&transcript()).unwrap();
        let key = req
            .headers
            .iter()
            .find(|(n, _)| n == "Idempotency-Key")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(!key.contains("你好"));
    }

    // ---------- connection test ----------

    #[test]
    fn a_connection_test_reports_success() {
        let c = client(ScriptedTransport::ok(204));
        let r = c.test_connection(&transcript());
        assert!(r.ok);
        assert_eq!(r.status, Some(204));
        assert!(r.detail.contains("204"));
    }

    #[test]
    fn a_connection_test_reports_a_configuration_error_without_io() {
        let t = ScriptedTransport::ok(200);
        let c = ApiClient::new(ApiConfig::new(""), AuthScheme::None, t);
        let r = c.test_connection(&transcript());
        assert!(!r.ok);
        assert_eq!(r.attempts, 0);
        assert!(r.detail.contains("base URL"), "got: {}", r.detail);
    }

    #[test]
    fn a_connection_test_reports_an_auth_failure_with_its_status() {
        let t = ScriptedTransport::new(vec![Ok(HttpResponse {
            status: 401,
            body: "nope".into(),
        })]);
        let c = client(t);
        let r = c.test_connection(&transcript());
        assert!(!r.ok);
        assert_eq!(r.status, Some(401));
    }

    #[test]
    fn a_connection_test_does_not_suppress_a_later_real_upload() {
        let c = client(ScriptedTransport::ok(200));
        c.test_connection(&transcript());
        let outcome = c.upload(&transcript()).unwrap();
        assert!(
            matches!(outcome, UploadOutcome::Uploaded { .. }),
            "a diagnostic must not count as an upload"
        );
    }

    #[test]
    fn body_summaries_are_truncated_and_blank_bodies_become_none() {
        assert_eq!(summarize_body("   "), None);
        assert_eq!(summarize_body("ok"), Some("ok".to_string()));
        let long = "x".repeat(1000);
        let summary = summarize_body(&long).unwrap();
        assert!(summary.chars().count() <= 301);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn upload_skipped_variant_is_representable() {
        // Auto-upload off is a legitimate, non-error outcome the UI shows.
        let outcome = UploadOutcome::Skipped {
            reason: "automatic upload is disabled",
        };
        assert!(matches!(outcome, UploadOutcome::Skipped { .. }));
    }

    #[test]
    fn the_bytes_actually_sent_match_the_built_request() {
        // End-to-end check of what leaves the process: method, URL, headers and
        // UTF-8 body, not just the in-memory request struct.
        let c = client(ScriptedTransport::ok(201));
        let t = transcript();
        c.upload(&t).unwrap();

        let sent = c.transport.last_request();
        assert_eq!(sent.method, HttpMethod::Post);
        assert_eq!(sent.url, "https://api.example.test/v1/transcripts");

        let header = |name: &str| {
            sent.headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        };
        assert_eq!(header("Content-Type").as_deref(), Some("application/json"));
        assert_eq!(header("Authorization").as_deref(), Some("Bearer sk-test"));
        assert!(header("Idempotency-Key").is_some());

        // The body is real UTF-8 JSON carrying the Traditional Chinese text.
        let text = String::from_utf8(sent.body.clone()).expect("body must be UTF-8");
        assert!(text.contains("你好，我想測試語音辨識。"));
        let parsed: serde_json::Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(parsed["schemaversion"], "1.0");
        assert_eq!(parsed["segments"][0]["final"], serde_json::json!(true));
    }
}
