//! Tauri commands exposing the voice-API layer to the GUI.
//!
//! # Where this sits
//!
//! This module is a **thin adapter**. All logic lives in the `handy-voice-api`
//! crate, which is compiled and tested independently of the Tauri toolchain.
//! Nothing here touches the ASR engine, the audio pipeline, or Handy's existing
//! settings — the transcript manager is additive, so upstream syncs stay cheap.
//!
//! # Why the payloads are JSON strings
//!
//! `tauri_specta::collect_commands!` requires every argument and return type to
//! implement `specta::Type`. The `handy-voice-api` types deliberately do not
//! depend on a UI framework, so the boundary exchanges JSON strings instead of
//! duplicating each type as a `specta`-deriving DTO.
//!
//! The trade-off is real and worth stating: the frontend loses generated
//! TypeScript bindings for these payloads and must parse them itself. Adding
//! `specta` derives to the crate (behind a feature) is the follow-up once the
//! GUI build is verified on a machine with the native toolchain.
//!
//! # Secret handling
//!
//! The API key never enters the settings store, the transcript, or a log line:
//!
//! * it is written to the OS credential store via [`handy_voice_api::secrets`];
//! * the persisted endpoint configuration ([`ApiConfig`]) contains no secret;
//! * the key is read from the credential store only at the moment of a request.

use handy_voice_api::api::{ApiClient, ApiConfig, AuthScheme, ReqwestTransport};
use handy_voice_api::export;
use handy_voice_api::prelude::*;
use handy_voice_api::time::SystemClock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

/// Provider id used for the custom backend's credential.
const CUSTOM_PROVIDER_ID: &str = "custom";

/// File name for the persisted endpoint configuration inside the app data dir.
const CONFIG_FILE: &str = "voice_api_config.json";

/// Errors from this module are surfaced to the GUI as plain strings, matching
/// the convention used by Handy's other command modules.
fn err(message: impl std::fmt::Display) -> String {
    message.to_string()
}

/// State for the voice-API features.
pub struct VoiceApiManager {
    /// Current endpoint configuration. Never contains a credential.
    config: Mutex<ApiConfig>,
    /// The active recording session's transcript, if any.
    session: Mutex<Option<TranscriptManager>>,
    /// Credential storage. The OS store in the app; in-memory in tests.
    secret_store: Arc<dyn SecretStore>,
    /// Where the endpoint configuration is persisted.
    config_path: PathBuf,
    /// Monotonic counter used to build unique session ids.
    session_counter: AtomicU64,
}

impl VoiceApiManager {
    /// Build the manager, loading any persisted configuration.
    ///
    /// A malformed configuration file is reported and replaced with defaults
    /// rather than preventing the app from starting — a corrupt config must not
    /// brick the application.
    pub fn new(config_path: PathBuf, secret_store: Arc<dyn SecretStore>) -> Self {
        let config = match std::fs::read_to_string(&config_path) {
            Ok(raw) => serde_json::from_str::<ApiConfig>(&raw).unwrap_or_else(|e| {
                log::warn!("voice_api: ignoring invalid config at {config_path:?}: {e}");
                ApiConfig::default()
            }),
            Err(_) => ApiConfig::default(),
        };

        Self {
            config: Mutex::new(config),
            session: Mutex::new(None),
            secret_store,
            config_path,
            session_counter: AtomicU64::new(0),
        }
    }

    /// The OS-credential-store backed manager used by the running app.
    pub fn with_os_secret_store(config_path: PathBuf) -> Self {
        Self::new(
            config_path,
            Arc::new(handy_voice_api::secrets::os_store::OsSecretStore::default()),
        )
    }

    /// A manager backed by an in-memory secret store. Used by tests.
    pub fn with_memory_secret_store(config_path: PathBuf) -> Self {
        Self::new(config_path, Arc::new(MemorySecretStore::new()))
    }

    fn secret_account(&self) -> String {
        account_name(SECRET_SERVICE, CUSTOM_PROVIDER_ID)
    }

    /// Read the stored API key, if any.
    fn api_key(&self) -> Result<Option<SecretString>, VoiceApiError> {
        self.secret_store.get(&self.secret_account())
    }

    /// Build a client for the current configuration.
    ///
    /// The credential is attached here, at the last moment, so it is never held
    /// in long-lived state.
    fn client(&self) -> Result<ApiClient<ReqwestTransport>, VoiceApiError> {
        let config = self
            .config
            .lock()
            .map_err(|_| VoiceApiError::Config("configuration lock poisoned".into()))?
            .clone();
        config.validate()?;

        let transport = ReqwestTransport::new(config.allow_insecure_http)?;
        let auth = match self.api_key()? {
            Some(secret) if !secret.is_empty() => AuthScheme::Bearer(secret),
            _ => AuthScheme::None,
        };
        Ok(ApiClient::new(config, auth, transport))
    }

    /// Persist the endpoint configuration (never a credential).
    fn persist_config(&self, config: &ApiConfig) -> Result<(), VoiceApiError> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                VoiceApiError::io("create directory for", parent.display().to_string(), e)
            })?;
        }
        let json = serde_json::to_string_pretty(config)?;
        std::fs::write(&self.config_path, json)
            .map_err(|e| VoiceApiError::io("write", self.config_path.display().to_string(), e))
    }

    /// Run `f` against the current session, or report that none is active.
    fn with_session<T>(
        &self,
        f: impl FnOnce(&mut TranscriptManager) -> T,
    ) -> Result<T, VoiceApiError> {
        let mut guard = self
            .session
            .lock()
            .map_err(|_| VoiceApiError::Config("session lock poisoned".into()))?;
        match guard.as_mut() {
            Some(session) => Ok(f(session)),
            None => Err(VoiceApiError::NotFound {
                kind: "session",
                id: "current".into(),
            }),
        }
    }
}

/// Build the manager and register it in Tauri state.
///
/// Called from `lib.rs` setup, alongside the other managers.
pub fn init(app: &AppHandle) -> Result<Arc<VoiceApiManager>, Box<dyn std::error::Error>> {
    let dir = app.path().app_data_dir()?;
    let manager = Arc::new(VoiceApiManager::with_os_secret_store(dir.join(CONFIG_FILE)));
    Ok(manager)
}

// ---------------------------------------------------------------------------
// Endpoint configuration
// ---------------------------------------------------------------------------

/// Return the current endpoint configuration as JSON. Contains no credential.
#[tauri::command]
#[specta::specta]
pub fn voice_api_get_config(manager: State<'_, Arc<VoiceApiManager>>) -> Result<String, String> {
    let config = manager
        .config
        .lock()
        .map_err(|_| "configuration lock poisoned".to_string())?;
    serde_json::to_string(&*config).map_err(err)
}

/// Replace the endpoint configuration, validating it before saving.
#[tauri::command]
#[specta::specta]
pub fn voice_api_set_config(
    manager: State<'_, Arc<VoiceApiManager>>,
    config_json: String,
) -> Result<(), String> {
    let config: ApiConfig = serde_json::from_str(&config_json).map_err(err)?;
    // Validate before persisting so an unusable URL cannot be saved silently.
    config.validate().map_err(err)?;
    manager.persist_config(&config).map_err(err)?;
    let mut guard = manager
        .config
        .lock()
        .map_err(|_| "configuration lock poisoned".to_string())?;
    *guard = config;
    Ok(())
}

/// Default endpoint configuration, for the settings UI's "reset" action.
#[tauri::command]
#[specta::specta]
pub fn voice_api_default_config() -> Result<String, String> {
    serde_json::to_string(&ApiConfig::default()).map_err(err)
}

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

/// Whether an API key is stored. Never returns the key itself.
#[tauri::command]
#[specta::specta]
pub fn voice_api_has_api_key(manager: State<'_, Arc<VoiceApiManager>>) -> Result<bool, String> {
    match manager.api_key() {
        Ok(Some(secret)) => Ok(!secret.is_empty()),
        Ok(None) => Ok(false),
        Err(e) => Err(err(e)),
    }
}

/// Store an API key in the operating system credential store.
///
/// The key is never written to the settings file, the transcript, or a log.
#[tauri::command]
#[specta::specta]
pub fn voice_api_set_api_key(
    manager: State<'_, Arc<VoiceApiManager>>,
    api_key: String,
) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("the API key must not be empty".to_string());
    }
    manager
        .secret_store
        .set(&manager.secret_account(), &SecretString::new(api_key))
        .map_err(err)
}

/// Delete the stored API key.
#[tauri::command]
#[specta::specta]
pub fn voice_api_clear_api_key(manager: State<'_, Arc<VoiceApiManager>>) -> Result<(), String> {
    manager
        .secret_store
        .delete(&manager.secret_account())
        .map_err(err)
}

/// Where credentials are actually stored, so the UI can be truthful about it.
#[tauri::command]
#[specta::specta]
pub fn voice_api_secret_storage(manager: State<'_, Arc<VoiceApiManager>>) -> String {
    manager.secret_store.kind().describe().to_string()
}

// ---------------------------------------------------------------------------
// Transcript session
// ---------------------------------------------------------------------------

/// Start a new transcription session and return its id.
#[tauri::command]
#[specta::specta]
pub fn voice_api_begin_session(
    manager: State<'_, Arc<VoiceApiManager>>,
    language: String,
    source: String,
) -> Result<String, String> {
    let seq = manager.session_counter.fetch_add(1, Ordering::SeqCst);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    // Session ids are the upload idempotency anchor, so they must be unique
    // across restarts: wall clock plus a per-process counter.
    let session_id = format!("session-{stamp}-{seq}");

    let session =
        TranscriptManager::new(Box::new(SystemClock), session_id.clone(), language, source);
    let mut guard = manager
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    *guard = Some(session);
    Ok(session_id)
}

/// Apply one recognition result to the active session.
///
/// Returns the [`ApplyOutcome`] as JSON so the frontend can distinguish a real
/// update from a dropped duplicate.
#[tauri::command]
#[specta::specta]
pub fn voice_api_apply_update(
    manager: State<'_, Arc<VoiceApiManager>>,
    update_json: String,
) -> Result<String, String> {
    let update: TranscriptUpdate = serde_json::from_str(&update_json).map_err(err)?;
    let outcome = manager
        .with_session(|session| session.apply(update))
        .map_err(err)?;
    serde_json::to_string(&outcome).map_err(err)
}

/// Record a non-fatal failure against the active session.
#[tauri::command]
#[specta::specta]
pub fn voice_api_record_error(
    manager: State<'_, Arc<VoiceApiManager>>,
    message: String,
    segment_id: Option<String>,
) -> Result<(), String> {
    manager
        .with_session(|session| session.record_error(message, segment_id))
        .map_err(err)
}

/// Return the active session's transcript as JSON.
#[tauri::command]
#[specta::specta]
pub fn voice_api_get_transcript(
    manager: State<'_, Arc<VoiceApiManager>>,
) -> Result<String, String> {
    manager
        .with_session(|session| serde_json::to_string(session.transcript()))
        .map_err(err)?
        .map_err(err)
}

/// Replace a segment's text with a user edit.
#[tauri::command]
#[specta::specta]
pub fn voice_api_edit_segment(
    manager: State<'_, Arc<VoiceApiManager>>,
    segment_id: String,
    text: String,
) -> Result<(), String> {
    manager
        .with_session(|session| session.edit_segment(&segment_id, text))
        .map_err(err)?
        .map_err(err)
}

/// Discard the current transcript's contents, keeping the session.
#[tauri::command]
#[specta::specta]
pub fn voice_api_clear_transcript(manager: State<'_, Arc<VoiceApiManager>>) -> Result<(), String> {
    manager.with_session(|session| session.clear()).map_err(err)
}

// ---------------------------------------------------------------------------
// JSON export / import
// ---------------------------------------------------------------------------

/// Export the active transcript to `path`, returning the JSON that was written.
///
/// Passing `finals_only` exports only confirmed text.
#[tauri::command]
#[specta::specta]
pub fn voice_api_export_to_file(
    manager: State<'_, Arc<VoiceApiManager>>,
    path: String,
    finals_only: bool,
) -> Result<String, String> {
    let filter = if finals_only {
        ExportFilter::finals_only()
    } else {
        ExportFilter::default()
    };
    let json = manager
        .with_session(|session| export::to_json_filtered(session.transcript(), filter))
        .map_err(err)?
        .map_err(err)?;
    // Write the exact bytes that were validated, rather than re-serializing.
    std::fs::write(&path, &json).map_err(err)?;
    Ok(json)
}

/// Load a transcript document from disk into the active session's place.
///
/// Returns the imported document as JSON and makes it the active session, so
/// the user can keep editing or upload what they just opened.
#[tauri::command]
#[specta::specta]
pub fn voice_api_import_from_file(
    manager: State<'_, Arc<VoiceApiManager>>,
    path: String,
) -> Result<String, String> {
    let transcript = import_from_file(&path).map_err(err)?;
    let json = serde_json::to_string(&transcript).map_err(err)?;

    let mut session = TranscriptManager::new(
        Box::new(SystemClock),
        transcript.session_id.clone(),
        transcript.language.clone(),
        transcript.source.clone(),
    );
    // Replay stored segments so the manager's index and numbering match.
    for segment in &transcript.segments {
        session.apply(TranscriptUpdate {
            segment_id: segment.segment_id.clone(),
            revision: segment.revision,
            kind: segment.kind,
            text: segment.text.clone(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
        });
    }

    let mut guard = manager
        .session
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    *guard = Some(session);
    Ok(json)
}

// ---------------------------------------------------------------------------
// Upload
// ---------------------------------------------------------------------------

/// Test the configured endpoint without uploading anything.
///
/// The connection test is deliberately explicit: nothing is ever sent
/// automatically, and this is the only way to probe the endpoint from the UI.
#[tauri::command]
#[specta::specta]
pub async fn voice_api_test_connection(
    manager: State<'_, Arc<VoiceApiManager>>,
) -> Result<String, String> {
    let sample = manager
        .with_session(|session| session.transcript().clone())
        .map_err(err)?;
    let client = manager.client().map_err(err)?;

    // Blocking I/O runs on a worker thread so the GUI and the audio thread are
    // never blocked by a slow endpoint.
    let result = tauri::async_runtime::spawn_blocking(move || client.test_connection(&sample))
        .await
        .map_err(err)?;
    serde_json::to_string(&result).map_err(err)
}

/// Upload the active transcript to the configured endpoint.
///
/// Only ever called from an explicit user action or from auto-upload, which the
/// user must opt into; there is no implicit upload path.
#[tauri::command]
#[specta::specta]
pub async fn voice_api_upload(manager: State<'_, Arc<VoiceApiManager>>) -> Result<String, String> {
    let transcript = manager
        .with_session(|session| session.transcript().clone())
        .map_err(err)?;
    let client = manager.client().map_err(err)?;

    let outcome = tauri::async_runtime::spawn_blocking(move || client.upload(&transcript))
        .await
        .map_err(err)?
        .map_err(err)?;
    serde_json::to_string(&outcome).map_err(err)
}
