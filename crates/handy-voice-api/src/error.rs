//! Error types for the voice-API layer.
//!
//! Every error that can reach the user carries a human-readable message that is
//! safe to display. Errors deliberately do NOT carry request headers or bodies,
//! because those may contain an API key — see [`crate::api`] for how the key is
//! kept out of every error and log path.

use thiserror::Error;

/// Errors produced by transcript handling, JSON export, and API upload.
#[derive(Debug, Error)]
pub enum VoiceApiError {
    /// The transcript JSON could not be serialized.
    #[error("failed to serialize transcript: {0}")]
    Serialize(#[from] serde_json::Error),

    /// The on-disk export could not be written or read.
    #[error("failed to {action} transcript file '{path}': {source}")]
    Io {
        /// What was being attempted, e.g. `write`.
        action: &'static str,
        /// The file involved.
        path: String,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// A transcript document did not satisfy the versioned schema.
    #[error("transcript schema validation failed: {0}")]
    Schema(String),

    /// The transcript's schema version is not one this build can read.
    #[error("unsupported transcript schema version '{found}' (this build supports {supported})")]
    UnsupportedSchemaVersion {
        /// The version found in the document.
        found: String,
        /// The version this build writes.
        supported: &'static str,
    },

    /// The API endpoint configuration is invalid.
    #[error("invalid API configuration: {0}")]
    Config(String),

    /// The HTTP request failed before a response was received.
    #[error("request to '{endpoint}' failed: {message}")]
    Transport {
        /// The endpoint that was called.
        endpoint: String,
        /// A description of the fault, free of credentials.
        message: String,
    },

    /// The request timed out.
    #[error("request to '{endpoint}' timed out after {timeout_ms} ms")]
    Timeout {
        /// The endpoint that was called.
        endpoint: String,
        /// The configured timeout.
        timeout_ms: u64,
    },

    /// The server returned a non-success status.
    #[error("server returned HTTP {status}{}", .body_summary.as_deref().map(|b| format!(": {b}")).unwrap_or_default())]
    HttpStatus {
        /// The status code returned.
        status: u16,
        /// A truncated, credential-free summary of the response body.
        body_summary: Option<String>,
    },

    /// A secret could not be read from or written to the OS credential store.
    #[error("credential store error for '{account}': {message}")]
    SecretStore {
        /// The account the operation targeted.
        account: String,
        /// The message reported by the credential store.
        message: String,
    },

    /// The requested session or segment does not exist.
    #[error("unknown {kind} '{id}'")]
    NotFound {
        /// What kind of thing was looked up.
        kind: &'static str,
        /// The identifier that was looked up.
        id: String,
    },
}

impl VoiceApiError {
    /// Build an [`VoiceApiError::Io`] with context.
    pub fn io(action: &'static str, path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }

    /// Whether this error is worth retrying.
    ///
    /// Only transport faults, timeouts, and 5xx/429 responses are retryable.
    /// A 4xx other than 429 means the request itself is wrong and retrying
    /// would just repeat the mistake — and could double-write on the server.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { .. } | Self::Timeout { .. } => true,
            Self::HttpStatus { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_and_timeout_errors_are_retryable() {
        let t = VoiceApiError::Transport {
            endpoint: "https://example.test/v1".into(),
            message: "connection reset".into(),
        };
        let to = VoiceApiError::Timeout {
            endpoint: "https://example.test/v1".into(),
            timeout_ms: 5000,
        };
        assert!(t.is_retryable());
        assert!(to.is_retryable());
    }

    #[test]
    fn server_errors_and_rate_limits_are_retryable_but_other_4xx_are_not() {
        let five_hundred = VoiceApiError::HttpStatus {
            status: 503,
            body_summary: None,
        };
        let too_many = VoiceApiError::HttpStatus {
            status: 429,
            body_summary: None,
        };
        let bad_request = VoiceApiError::HttpStatus {
            status: 400,
            body_summary: Some("bad json".into()),
        };
        let unauthorized = VoiceApiError::HttpStatus {
            status: 401,
            body_summary: None,
        };
        assert!(five_hundred.is_retryable());
        assert!(too_many.is_retryable());
        // Retrying a 400/401 would either loop forever or duplicate a write.
        assert!(!bad_request.is_retryable());
        assert!(!unauthorized.is_retryable());
    }

    #[test]
    fn configuration_and_schema_errors_are_not_retryable() {
        assert!(!VoiceApiError::Config("missing endpoint".into()).is_retryable());
        assert!(!VoiceApiError::Schema("no segments".into()).is_retryable());
    }

    #[test]
    fn http_status_error_message_includes_body_summary_when_present() {
        let with_body = VoiceApiError::HttpStatus {
            status: 422,
            body_summary: Some("field 'segments' is required".into()),
        };
        assert!(with_body.to_string().contains("422"));
        assert!(with_body.to_string().contains("segments"));

        let without_body = VoiceApiError::HttpStatus {
            status: 500,
            body_summary: None,
        };
        assert_eq!(without_body.to_string(), "server returned HTTP 500");
    }
}
