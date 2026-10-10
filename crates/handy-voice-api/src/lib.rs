//! # handy-voice-api
//!
//! The voice-API layer for the Handy fork: a versioned transcript model, JSON
//! export/import, OS-credential-backed secret storage, and a configurable custom
//! backend upload client.
//!
//! ## Why this is a separate crate
//!
//! Upstream Handy is a single Tauri crate that can only be linked with the
//! platform native toolchain (MSVC on Windows, Xcode on macOS). Putting this
//! logic in a dependency-light library means it can be **compiled and tested on
//! its own**, on any host, without the GUI toolchain — which is what makes the
//! test suite in this crate actually runnable rather than aspirational.
//!
//! It also keeps the diff against upstream small: `src-tauri` only needs to add
//! a path dependency and a handful of `#[tauri::command]` wrappers.
//!
//! ## Purity guarantee
//!
//! This crate is `#![forbid(unsafe_code)]`, matching upstream Handy's posture.
//!
//! ## Module map
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`transcript`] | Session model; the partial/final deduplication rules |
//! | [`schema`] | Versioned JSON Schema (embedded) and its validator |
//! | [`export`] | Atomic UTF-8 JSON export and validated import |
//! | [`api`] | Endpoint configuration, request building, retry, idempotency |
//! | [`secrets`] | API-key storage; redaction guarantees |
//! | [`time`] | Dependency-free RFC 3339 UTC clock |
//! | [`error`] | Error types and retryability |
//!
//! ## Design notes the reader should know
//!
//! * **No transport by default.** [`api::Transport`] is the only I/O seam. With
//!   default features nothing performs network I/O, so the whole request/retry
//!   path is unit-testable offline. The `http` feature adds a `reqwest`-based
//!   transport built on the HTTP stack Handy already ships.
//! * **No keyring by default.** The `keyring` feature adds OS credential-store
//!   storage. Without it, only the in-memory test store exists — and it reports
//!   itself as memory-only so the UI cannot claim a key was saved when it was
//!   not.
//! * **Read-lenient, write-strict.** The validator accepts the minimal document
//!   from the product specification while the exporter always writes the full
//!   field set.
//!
//! ## Example
//!
//! ```
//! use handy_voice_api::prelude::*;
//! use handy_voice_api::time::FixedClock;
//!
//! let mut session = TranscriptManager::new(
//!     Box::new(FixedClock::reference()),
//!     "session-001",
//!     "zh-TW",
//!     "microphone",
//! );
//!
//! // A streaming engine sends tentative then confirmed text for one segment.
//! session.apply(TranscriptUpdate::partial("seg-1", 1, "你好，我想"));
//! session.apply(TranscriptUpdate::final_("seg-1", 2, "你好，我想測試語音辨識。"));
//!
//! // One segment, holding only the final text — never both.
//! assert_eq!(session.transcript().segments.len(), 1);
//! assert_eq!(session.transcript().committed_text(), "你好，我想測試語音辨識。");
//!
//! // Export validates itself before returning.
//! let json = to_json(session.transcript()).unwrap();
//! assert!(json.contains("你好，我想測試語音辨識。"));
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod api;
pub mod error;
pub mod export;
pub mod schema;
pub mod secrets;
pub mod time;
pub mod transcript;

/// The most commonly used items, for ergonomic imports.
pub mod prelude {
    pub use crate::api::{
        ApiClient, ApiConfig, AuthScheme, ConnectionTestResult, HttpMethod, HttpRequest,
        HttpResponse, Idempotency, RequestFormat, RetryPolicy, Transport, UploadOutcome,
    };
    pub use crate::error::VoiceApiError;
    pub use crate::export::{export_to_file, import_from_file, to_json, ExportFilter};
    pub use crate::schema::{validate_transcript, SUPPORTED_SCHEMA_VERSIONS, TRANSCRIPT_SCHEMA_V1};
    pub use crate::secrets::{
        account_name, MemorySecretStore, SecretStore, SecretString, SECRET_SERVICE,
    };
    pub use crate::transcript::{
        ApplyOutcome, EngineInfo, SegmentKind, Transcript, TranscriptError, TranscriptManager,
        TranscriptSegment, TranscriptUpdate, SCHEMA_VERSION,
    };
}

#[cfg(test)]
mod integration_tests {
    //! Cross-module behaviour: the pieces have to work together, not just
    //! individually.

    use crate::api::{
        ApiClient, ApiConfig, AuthScheme, HttpRequest, HttpResponse, Transport, UploadOutcome,
    };
    use crate::export::{import_from_str, to_json};
    use crate::prelude::*;
    use crate::secrets::{MemorySecretStore, SecretStore};
    use crate::time::FixedClock;
    use std::sync::Mutex;

    /// A transport that records what it was asked to send and answers 200.
    struct RecordingTransport {
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl RecordingTransport {
        fn new() -> Self {
            Self {
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl Transport for RecordingTransport {
        fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, VoiceApiError> {
            self.seen.lock().unwrap().push(HttpRequest {
                method: request.method,
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
                timeout: request.timeout,
            });
            Ok(HttpResponse {
                status: 201,
                body: String::new(),
            })
        }
    }

    const API_KEY: &str = "sk-live-integration-DO-NOT-LEAK";

    #[test]
    fn full_flow_transcribe_export_upload_never_leaks_the_api_key() {
        // 1. A session with partial-then-final results, in Traditional Chinese.
        let mut session = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-integration",
            "zh-TW",
            "microphone",
        );
        session.set_engine(EngineInfo {
            id: "transcribe-cpp".into(),
            model_id: Some("handy-computer/Breeze-ASR-25-gguf".into()),
        });
        session.apply(TranscriptUpdate::partial("seg-1", 1, "你好，我想"));
        session.apply(TranscriptUpdate::final_(
            "seg-1",
            2,
            "你好，我想測試語音辨識。",
        ));
        session.apply(TranscriptUpdate::partial("seg-2", 1, "OpenAI 的 API"));

        // 2. The exported document is valid and round-trips.
        let json = to_json(session.transcript()).unwrap();
        let reimported = import_from_str(&json).unwrap();
        assert_eq!(&reimported, session.transcript());

        // 3. Upload it with a key held in a secret store.
        let store = MemorySecretStore::new();
        let account = account_name(SECRET_SERVICE, "custom");
        store.set(&account, &SecretString::new(API_KEY)).unwrap();
        let key = store.get(&account).unwrap().unwrap();

        let transport = RecordingTransport::new();
        let client = ApiClient::new(
            ApiConfig::new("https://api.example.test/v1"),
            AuthScheme::Bearer(key.clone()),
            transport,
        );

        match client.upload(&reimported).unwrap() {
            UploadOutcome::Uploaded {
                status, attempts, ..
            } => {
                assert_eq!(status, 201);
                assert_eq!(attempts, 1);
            }
            other => panic!("expected Uploaded, got {other:?}"),
        }

        // 4. The request carried the key (it must, to authenticate)...
        let seen = client.transport().seen.lock().unwrap();
        let request = seen.last().unwrap();
        assert!(request
            .headers
            .iter()
            .any(|(n, v)| n == "Authorization" && v.contains(API_KEY)));

        // 5. ...but nothing that gets logged, printed, or written leaks it.
        assert!(!format!("{request:?}").contains(API_KEY));
        assert!(!format!("{client:?}").contains(API_KEY));
        assert!(!format!("{key:?}").contains(API_KEY));
        assert!(!json.contains(API_KEY));
        assert!(!session.transcript().display_text().contains(API_KEY));
        assert!(!format!("{:?}", client.config()).contains(API_KEY));
    }

    #[test]
    fn export_filter_and_upload_agree_on_what_is_sent() {
        // The uploaded body must contain the same text the user sees exported.
        let mut session = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "s",
            "zh-TW",
            "microphone",
        );
        session.apply(TranscriptUpdate::final_("a", 1, "已確認的文字"));

        let client = ApiClient::new(
            ApiConfig::new("https://api.example.test"),
            AuthScheme::None,
            RecordingTransport::new(),
        );
        let body = client.build_body(session.transcript()).unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let exported: serde_json::Value =
            serde_json::from_str(&to_json(session.transcript()).unwrap()).unwrap();

        assert_eq!(body_json["segments"], exported["segments"]);
    }

    #[test]
    fn every_exported_transcript_satisfies_the_published_schema() {
        // Property-style check across a range of session shapes.
        let cases: Vec<Vec<TranscriptUpdate>> = vec![
            vec![],
            vec![TranscriptUpdate::partial("a", 1, "只有暫時文字")],
            vec![TranscriptUpdate::final_("a", 1, "只有最終文字")],
            vec![
                TranscriptUpdate::partial("a", 1, "暫"),
                TranscriptUpdate::final_("a", 2, "最終"),
                TranscriptUpdate::partial("b", 1, "第二段暫"),
            ],
        ];

        for (i, updates) in cases.into_iter().enumerate() {
            let mut session = TranscriptManager::new(
                Box::new(FixedClock::reference()),
                format!("case-{i}"),
                "zh-TW",
                "microphone",
            );
            for u in updates {
                session.apply(u);
            }
            let json = to_json(session.transcript())
                .unwrap_or_else(|e| panic!("case {i} failed to export: {e}"));
            crate::schema::validate_transcript_str(&json)
                .unwrap_or_else(|e| panic!("case {i} produced an invalid document: {e}"));
        }
    }
}
