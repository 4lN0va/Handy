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

use handy_voice_api::api::reqwest_transport::ReqwestTransport;
use handy_voice_api::api::{ApiClient, ApiConfig, AuthScheme};
use handy_voice_api::export;
use handy_voice_api::prelude::*;
use handy_voice_api::time::SystemClock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Listener, Manager, State};

/// Provider id used for the custom backend's credential.
const CUSTOM_PROVIDER_ID: &str = "custom";

/// File name for the persisted endpoint configuration inside the app data dir.
const CONFIG_FILE: &str = "voice_api_config.json";

/// Event emitted to the GUI whenever the transcript changes.
///
/// The payload is the transcript document as a JSON string, for the same reason
/// the commands exchange JSON: the `handy-voice-api` types deliberately do not
/// depend on a UI framework.
pub const TRANSCRIPT_UPDATED_EVENT: &str = "voice-api-transcript-updated";

/// Minimum gap between *partial* transcript events.
///
/// A streaming model can emit text many times per second. Coalescing keeps a
/// high-frequency recognition stream from forcing a React re-render per token.
/// Final updates always bypass the throttle, so the confirmed text is never
/// delayed.
const PARTIAL_EMIT_INTERVAL: Duration = Duration::from_millis(80);

/// Upstream event names this module subscribes to.
///
/// These are the strings tauri-specta generates for the `#[tauri_specta::Event]`
/// types; they match `src/bindings.ts` exactly. Using the real upstream payload
/// types (rather than re-declaring them) means an upstream shape change becomes a
/// compile error here instead of silently breaking at runtime.
const UPSTREAM_STREAM_TEXT_EVENT: &str = "stream-text-event";
const UPSTREAM_STREAM_PHASE_EVENT: &str = "stream-phase-event";
const UPSTREAM_HISTORY_UPDATE_EVENT: &str = "history-update-payload";

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
    /// Converts upstream streaming events into transcript updates.
    adapter: Mutex<StreamEventAdapter>,
    /// When the last throttled partial update was emitted.
    last_partial_emit: Mutex<Option<Instant>>,
    /// Set while a network operation is in flight, so repeated clicks on
    /// "Upload" or "Test connection" cannot fan out into unbounded concurrent
    /// requests.
    network_in_flight: Arc<std::sync::atomic::AtomicBool>,
    /// The most recent network result, as JSON, for the GUI to query.
    last_operation: Mutex<Option<String>>,
    /// Credential storage. The OS store in the app; in-memory in tests.
    secret_store: Arc<dyn SecretStore>,
    /// Where the endpoint configuration is persisted.
    config_path: PathBuf,
    /// Monotonic counter used to build unique session ids.
    session_counter: AtomicU64,
    /// Optional forwarder to the local integration-verification tool.
    ///
    /// Holds the bridge rather than owning it, so a run without a bridge costs
    /// one uncontended lock on the transcript path and nothing else.
    bridge: Mutex<Option<Arc<crate::bridge::Bridge>>>,
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
            adapter: Mutex::new(StreamEventAdapter::new()),
            last_partial_emit: Mutex::new(None),
            network_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_operation: Mutex::new(None),
            secret_store,
            config_path,
            session_counter: AtomicU64::new(0),
            bridge: Mutex::new(None),
        }
    }

    /// Record the outcome of a network operation for the GUI to query.
    fn remember_operation(&self, json: String) {
        if let Ok(mut guard) = self.last_operation.lock() {
            *guard = Some(json);
        }
    }

    /// Acquire the single-flight network permit, or report that one is running.
    ///
    /// This is a `compare_exchange` rather than a check-then-set so two threads
    /// racing from a double click cannot both win.
    ///
    /// The returned guard **releases the permit when dropped**. That is not
    /// stylistic: the first version released it with an explicit call on each
    /// exit path, and the `?` on the session lookup returned before that call.
    /// The permit was then held for the lifetime of the process, so every later
    /// upload and connection test failed with "another network operation is
    /// already running" until the app was restarted. Tying the release to `Drop`
    /// makes that class of leak impossible.
    fn begin_network_operation(&self) -> Result<NetworkOperation, VoiceApiError> {
        self.network_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| NetworkOperation {
                flag: Arc::clone(&self.network_in_flight),
            })
            .map_err(|_| {
                VoiceApiError::Config(
                    "another network operation is already running; wait for it to finish".into(),
                )
            })
    }

    /// The OS-credential-store backed manager used by the running app.
    pub fn with_os_secret_store(config_path: PathBuf) -> Self {
        Self::new(
            config_path,
            Arc::new(handy_voice_api::secrets::os_store::OsSecretStore::default()),
        )
    }

    /// A manager backed by an in-memory secret store. Used by tests.
    #[cfg(test)]
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

    /// Start a session if none is active, and return nothing.
    ///
    /// Recognition events can arrive before the GUI has called
    /// `voice_api_begin_session` (a global shortcut starts recording without any
    /// window interaction), so the bridge creates the session lazily rather than
    /// dropping the first utterance.
    fn ensure_session(&self) -> Result<(), VoiceApiError> {
        let mut guard = self
            .session
            .lock()
            .map_err(|_| VoiceApiError::Config("session lock poisoned".into()))?;
        if guard.is_none() {
            let seq = self.session_counter.fetch_add(1, Ordering::SeqCst);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            *guard = Some(TranscriptManager::new(
                Box::new(SystemClock),
                format!("session-{stamp}-{seq}"),
                "auto",
                "microphone",
            ));
        }
        Ok(())
    }

    /// Feed one live recognition update through the adapter.
    ///
    /// Returns whether the transcript actually changed. Emission is the caller's
    /// job, which keeps this method callable — and therefore testable — without a
    /// running Tauri application.
    fn ingest_stream_text(&self, committed: &str, tentative: &str) -> bool {
        if let Err(e) = self.ensure_session() {
            log::warn!("voice_api: could not start a session for streaming text: {e}");
            return false;
        }

        let events = match self.adapter.lock() {
            Ok(mut adapter) => adapter.on_stream_text(committed, tentative),
            Err(_) => {
                log::warn!("voice_api: stream adapter lock poisoned; dropping update");
                return false;
            }
        };
        if events.is_empty() {
            return false;
        }
        self.apply_events(events)
    }

    /// Finalize the current utterance with the authoritative final text.
    ///
    /// Also the entry point for **non-streaming** models, where no live event
    /// ever fires.
    fn ingest_final_text(&self, final_text: &str) -> bool {
        if let Err(e) = self.ensure_session() {
            log::warn!("voice_api: could not start a session for final text: {e}");
            return false;
        }

        let events = match self.adapter.lock() {
            Ok(mut adapter) => adapter.on_final(final_text),
            Err(_) => {
                log::warn!("voice_api: stream adapter lock poisoned; dropping final text");
                return false;
            }
        };
        if events.is_empty() {
            return false;
        }
        self.apply_events(events)
    }

    /// Record a non-fatal recognition failure against the active session.
    fn ingest_error(&self, message: &str) -> bool {
        if self.ensure_session().is_err() {
            return false;
        }
        match self.session.lock() {
            Ok(mut guard) => match guard.as_mut() {
                Some(session) => {
                    session.record_error(message.to_string(), None);
                    true
                }
                None => false,
            },
            Err(_) => false,
        }
    }

    /// Apply adapter output to the session. Returns whether anything changed.
    ///
    /// Every applied update is also offered to the integration bridge, so the
    /// verification tool sees the same per-segment stream the UI does. That is
    /// the single place transcript updates are produced, so there is no second
    /// event pipeline to keep in sync.
    fn apply_events(&self, events: Vec<TranscriptUpdate>) -> bool {
        let session_id = {
            match self.session.lock() {
                Ok(mut guard) => match guard.as_mut() {
                    Some(session) => {
                        let id = session.transcript().session_id.clone();
                        for update in &events {
                            session.apply(update.clone());
                        }
                        Some(id)
                    }
                    None => None,
                },
                Err(_) => None,
            }
        };
        let Some(session_id) = session_id else {
            return false;
        };

        // Forwarded after the session lock is released, so a slow bridge can
        // never hold up the transcript model.
        for update in &events {
            self.forward_to_bridge(&session_id, update);
        }
        true
    }

    /// Publish one applied update to the integration bridge, if one is attached.
    fn forward_to_bridge(&self, session_id: &str, update: &TranscriptUpdate) {
        let bridge = match self.bridge.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => None,
        };
        let Some(bridge) = bridge else {
            return;
        };
        bridge.send_transcript(crate::bridge::TranscriptEvent {
            session_id: session_id.to_string(),
            segment_id: update.segment_id.clone(),
            text: update.text.clone(),
            final_: update.kind == SegmentKind::Final,
            timestamp: crate::bridge::now_rfc3339(),
            source: "handy".to_string(),
        });
    }

    /// Attach (or detach) the integration bridge.
    ///
    /// Held as a weak-ish optional reference: the manager never owns the bridge,
    /// it only forwards to it, so a bridge that is not installed costs nothing.
    pub fn set_bridge(&self, bridge: Option<Arc<crate::bridge::Bridge>>) {
        if let Ok(mut guard) = self.bridge.lock() {
            *guard = bridge;
        }
    }

    /// Accept a finalized segment produced outside the streaming path.
    ///
    /// Used by automatic continuous transcription, which produces whole
    /// sentences from batch recognition rather than incremental updates. It goes
    /// through the same adapter and session as everything else, so segment
    /// identity, revision and the GUI event are identical to the manual flow.
    pub fn accept_final_segment(&self, app: &AppHandle, text: &str) -> bool {
        if self.ingest_final_text(text) {
            self.publish_transcript(app, true);
            true
        } else {
            false
        }
    }

    /// Record a recognition failure against the active session.
    pub fn record_recognition_error(&self, app: &AppHandle, message: &str) {
        if self.ingest_error(message) {
            self.publish_transcript(app, true);
        }
    }

    /// Drop the in-flight utterance without promoting it to final.
    ///
    /// The segment stays in the transcript exactly as last seen; a cancelled
    /// utterance must not be recorded as confirmed text.
    fn abandon_utterance(&self) {
        if let Ok(mut adapter) = self.adapter.lock() {
            adapter.abandon();
        }
    }

    /// Emit the current transcript to the GUI, coalescing rapid partials.
    fn publish_transcript(&self, app: &AppHandle, is_final: bool) {
        if !is_final {
            let mut last = match self.last_partial_emit.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            let now = Instant::now();
            if let Some(previous) = *last {
                if now.duration_since(previous) < PARTIAL_EMIT_INTERVAL {
                    // Skip this partial; the next one will carry the same text
                    // plus whatever accumulated in the meantime.
                    return;
                }
            }
            *last = Some(now);
        }

        let payload = match self.session.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(session) => match serde_json::to_string(session.transcript()) {
                    Ok(json) => json,
                    Err(e) => {
                        log::warn!("voice_api: could not serialize transcript: {e}");
                        return;
                    }
                },
                None => return,
            },
            Err(_) => return,
        };

        if let Err(e) = app.emit(TRANSCRIPT_UPDATED_EVENT, payload) {
            log::warn!("voice_api: could not emit transcript update: {e}");
        }
    }
}

/// Build the manager, register it in Tauri state, and subscribe to Handy's
/// recognition events.
///
/// Called from `lib.rs` setup, alongside the other managers.
///
/// # Why listeners instead of editing the pipeline
///
/// The recognition pipeline already emits everything needed:
/// `stream-text-event` carries `{committed, tentative}` and
/// `history-update-payload` carries the finalized text. Subscribing to those
/// means **no upstream file is modified** to wire up live transcription, which
/// keeps the fork's diff against upstream minimal — the whole point of doing this
/// as a bridge rather than by threading a callback through `actions.rs`.
pub fn init(app: &AppHandle) -> Result<Arc<VoiceApiManager>, Box<dyn std::error::Error>> {
    let dir = app.path().app_data_dir()?;
    let manager = Arc::new(VoiceApiManager::with_os_secret_store(dir.join(CONFIG_FILE)));

    // --- live streaming text ---
    {
        let manager = Arc::clone(&manager);
        let app_handle = app.clone();
        app.listen(UPSTREAM_STREAM_TEXT_EVENT, move |event| {
            match serde_json::from_str::<crate::managers::transcription::StreamTextEvent>(
                event.payload(),
            ) {
                Ok(payload) => {
                    if manager.ingest_stream_text(&payload.committed, &payload.tentative) {
                        manager.publish_transcript(&app_handle, false);
                    }
                }
                Err(e) => log::warn!("voice_api: unreadable {UPSTREAM_STREAM_TEXT_EVENT}: {e}"),
            }
        });
    }

    // --- finalization and cancellation ---
    {
        let manager = Arc::clone(&manager);
        app.listen(UPSTREAM_STREAM_PHASE_EVENT, move |event| {
            use crate::managers::transcription::StreamPhase;
            match serde_json::from_str::<crate::managers::transcription::StreamPhaseEvent>(
                event.payload(),
            ) {
                // The stream is being finalized or post-processed. Stop treating
                // the current utterance as live so a late partial cannot reopen
                // it; the authoritative text arrives via the history event.
                Ok(payload) if payload.phase == StreamPhase::Working => {
                    manager.abandon_utterance();
                }
                Ok(_) => {}
                Err(e) => log::warn!("voice_api: unreadable {UPSTREAM_STREAM_PHASE_EVENT}: {e}"),
            }
        });
    }

    // --- the authoritative final text ---
    {
        let manager = Arc::clone(&manager);
        let app_handle = app.clone();
        app.listen(UPSTREAM_HISTORY_UPDATE_EVENT, move |event| {
            use crate::managers::history::HistoryUpdatePayload;
            match serde_json::from_str::<HistoryUpdatePayload>(event.payload()) {
                Ok(HistoryUpdatePayload::Added { entry }) => {
                    // Prefer the post-processed text: that is what the user saw
                    // and what was pasted, so it is the text the transcript
                    // should record.
                    let text = entry
                        .post_processed_text
                        .clone()
                        .unwrap_or_else(|| entry.transcription_text.clone());
                    if text.trim().is_empty() {
                        // An empty entry is created so the user can retry a
                        // failed transcription; it is not recognized text.
                        return;
                    }
                    // A final update always bypasses the partial throttle.
                    if manager.ingest_final_text(&text) {
                        manager.publish_transcript(&app_handle, true);
                    }
                }
                Ok(_) => {}
                Err(e) => log::warn!("voice_api: unreadable {UPSTREAM_HISTORY_UPDATE_EVENT}: {e}"),
            }
        });
    }

    // --- recognition errors ---
    {
        let manager = Arc::clone(&manager);
        let app_handle = app.clone();
        app.listen("transcription-error", move |event| {
            let message = serde_json::from_str::<String>(event.payload())
                .unwrap_or_else(|_| event.payload().to_string());
            if manager.ingest_error(&message) {
                manager.publish_transcript(&app_handle, true);
            }
        });
    }

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
    // Single-flight: a double click must not launch two probes. The permit is
    // released when `_permit` drops, on every exit path including the `?`s
    // below.
    let _permit = manager.begin_network_operation().map_err(err)?;

    let sample = manager
        .with_session(|session| session.transcript().clone())
        .map_err(err)?;

    // Blocking I/O runs on a worker thread so the GUI and the audio thread are
    // never blocked by a slow endpoint.
    //
    // The client is constructed *inside* the closure, and that matters.
    // `reqwest::blocking::Client` starts its own background runtime when it is
    // built. Building it on a Tokio worker thread makes that handshake fail with
    // "Failed to communicate successful startup", after which every request dies
    // before reaching the network - no upload, no connection test, and nothing
    // in the server log to explain it. Constructing it on the blocking thread
    // keeps it clear of the async runtime.
    let worker = Arc::clone(&manager);
    let json = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let client = worker.client().map_err(err)?;
        let outcome = client.test_connection(&sample);
        serde_json::to_string(&outcome).map_err(err)
    })
    .await
    .map_err(|e| format!("the network worker thread failed: {e}"))??;

    manager.remember_operation(json.clone());
    Ok(json)
}

/// Upload the active transcript to the configured endpoint.
///
/// Only ever called from an explicit user action; there is no implicit upload
/// path, and a second concurrent upload is refused rather than queued.
#[tauri::command]
#[specta::specta]
pub async fn voice_api_upload(manager: State<'_, Arc<VoiceApiManager>>) -> Result<String, String> {
    // Released when `_permit` drops, including on the `?`s below.
    let _permit = manager.begin_network_operation().map_err(err)?;

    let transcript = manager
        .with_session(|session| session.transcript().clone())
        .map_err(err)?;

    // See the note in `voice_api_test_connection`: the blocking client must be
    // constructed on the worker thread, not on the async runtime.
    let worker = Arc::clone(&manager);
    let json = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let client = worker.client().map_err(err)?;
        let outcome = client.upload(&transcript).map_err(err)?;
        serde_json::to_string(&outcome).map_err(err)
    })
    .await
    .map_err(|e| format!("the network worker thread failed: {e}"))??;

    manager.remember_operation(json.clone());
    Ok(json)
}

/// Whether a network operation is currently running.
#[tauri::command]
#[specta::specta]
pub fn voice_api_is_busy(manager: State<'_, Arc<VoiceApiManager>>) -> bool {
    manager
        .network_in_flight
        .load(std::sync::atomic::Ordering::Acquire)
}

/// The most recent network result as JSON, or `null` if none has run.
#[tauri::command]
#[specta::specta]
pub fn voice_api_last_operation(
    manager: State<'_, Arc<VoiceApiManager>>,
) -> Result<Option<String>, String> {
    manager
        .last_operation
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| "operation status lock poisoned".to_string())
}

/// RAII permit for the single-flight network guard.
///
/// Holds its own handle on the flag rather than borrowing the manager, so it can
/// be held across an `await` in an async command without lifetime trouble.
#[derive(Debug)]
pub struct NetworkOperation {
    flag: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for NetworkOperation {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

/// What the GUI is about to upload, without sending anything.
///
/// Lets the upload screen state plainly which data will leave the machine —
/// session id, language, segment count and a short preview — so the user does
/// not have to guess.
#[tauri::command]
#[specta::specta]
pub fn voice_api_upload_preview(manager: State<'_, Arc<VoiceApiManager>>) -> Result<String, String> {
    manager
        .with_session(|session| {
            let t = session.transcript();
            let preview: String = t.display_text().chars().take(200).collect();
            serde_json::json!({
                "session_id": t.session_id,
                "language": t.language,
                "source": t.source,
                "segment_count": t.segments.len(),
                "final_segment_count": t.segments.iter().filter(|s| s.is_final).count(),
                "error_count": t.errors.len(),
                "preview": preview,
            })
            .to_string()
        })
        .map_err(err)
}

#[cfg(test)]
mod tests {
    //! Tests for the bridge between Handy's recognition events and the
    //! transcript model.
    //!
    //! These exercise the manager's own logic — session lifecycle, the event
    //! adapter in situ, the single-flight guard, and configuration persistence.
    //! They deliberately avoid a running Tauri application by testing the
    //! `ingest_*` methods, which mutate state, rather than the `app.listen`
    //! closures, which only add emission on top.

    use super::*;
    use handy_voice_api::secrets::MemorySecretStore;

    fn manager_in(dir: &std::path::Path) -> VoiceApiManager {
        manager_with_store(dir, Arc::new(MemorySecretStore::new()))
    }

    /// Build a manager sharing `store`.
    ///
    /// Sharing matters when two managers must see the same credentials — in the
    /// real app the OS credential store is process-wide, so a fresh manager
    /// still finds a previously stored key.
    fn manager_with_store(
        dir: &std::path::Path,
        store: Arc<MemorySecretStore>,
    ) -> VoiceApiManager {
        VoiceApiManager::new(dir.join(CONFIG_FILE), store)
    }

    fn transcript_of(manager: &VoiceApiManager) -> Transcript {
        manager
            .with_session(|s| s.transcript().clone())
            .expect("a session should exist")
    }

    #[test]
    fn recognition_events_create_a_session_lazily() {
        // A global shortcut can start recording before the GUI has opened, so
        // events must not be dropped for want of a session.
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());
        assert!(m.session.lock().unwrap().is_none());

        assert!(m.ingest_stream_text("你好", "世界"));

        let t = transcript_of(&m);
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].text, "你好世界");
        assert_eq!(t.source, "microphone");
    }

    #[test]
    fn a_streaming_utterance_becomes_one_final_segment() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());

        assert!(m.ingest_stream_text("你好", "，我想"));
        assert!(m.ingest_stream_text("你好，我想", "測試"));
        assert!(m.ingest_final_text("你好，我想測試語音辨識。"));

        let t = transcript_of(&m);
        assert_eq!(t.segments.len(), 1, "partials must not be appended");
        assert_eq!(t.segments[0].text, "你好，我想測試語音辨識。");
        assert!(t.segments[0].is_final);
        assert_eq!(t.committed_text(), "你好，我想測試語音辨識。");
    }

    #[test]
    fn a_non_streaming_model_still_produces_a_final_segment() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());
        assert!(m.ingest_final_text("批次模型的結果。"));
        let t = transcript_of(&m);
        assert_eq!(t.segments.len(), 1);
        assert!(t.segments[0].is_final);
    }

    #[test]
    fn replayed_events_report_no_change_the_second_time() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());
        assert!(m.ingest_stream_text("你好", ""));
        // Identical replay must be a no-op, so no event is emitted and no
        // revision is consumed.
        assert!(!m.ingest_stream_text("你好", ""));
        assert_eq!(transcript_of(&m).segments[0].revision, 1);
    }

    #[test]
    fn a_stale_event_reports_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());
        assert!(m.ingest_stream_text("你好，我想測試", ""));
        assert!(!m.ingest_stream_text("你好", ""), "an older prefix is stale");
        assert_eq!(transcript_of(&m).segments[0].text, "你好，我想測試");
    }

    #[test]
    fn recognition_errors_are_recorded_but_never_become_text() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());
        assert!(m.ingest_error("麥克風裝置中斷"));

        let t = transcript_of(&m);
        assert_eq!(t.segments.len(), 0, "an error is not recognized text");
        assert_eq!(t.errors.len(), 1);
        assert_eq!(t.errors[0].message, "麥克風裝置中斷");
    }

    #[test]
    fn starting_a_new_session_discards_the_previous_utterance() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());
        m.ingest_stream_text("第一段的內容", "");

        // Simulate `voice_api_begin_session`.
        let seq = m.session_counter.fetch_add(1, Ordering::SeqCst);
        let mut guard = m.session.lock().unwrap();
        *guard = Some(TranscriptManager::new(
            Box::new(SystemClock),
            format!("session-test-{seq}"),
            "zh-TW",
            "microphone",
        ));
        drop(guard);
        m.adapter.lock().unwrap().reset();

        assert!(m.ingest_stream_text("第二段", ""));
        let t = transcript_of(&m);
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].text, "第二段");
    }

    #[test]
    fn the_single_flight_guard_admits_one_network_operation_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());

        let permit = m.begin_network_operation().expect("first should be admitted");
        let second = m.begin_network_operation();
        assert!(second.is_err(), "a concurrent operation must be refused");
        assert!(second.unwrap_err().to_string().contains("already running"));

        drop(permit);
        m.begin_network_operation()
            .expect("dropping the guard must release the permit");
    }

    #[test]
    fn the_permit_is_released_even_when_the_operation_returns_early() {
        // Regression: the permit used to be released by an explicit call on each
        // exit path, and an early `?` skipped it. The permit then stayed held for
        // the life of the process, permanently disabling upload and connection
        // tests. Dropping the guard must release it on an error path too.
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());

        fn operation_that_fails_early(m: &VoiceApiManager) -> Result<(), VoiceApiError> {
            let _permit = m.begin_network_operation()?;
            // Stands in for the session lookup that failed in the real bug.
            Err(VoiceApiError::Config("no session".into()))
        }

        assert!(operation_that_fails_early(&m).is_err());
        assert!(
            m.begin_network_operation().is_ok(),
            "the permit must be free after an early error return"
        );
    }

    #[test]
    fn the_upload_preview_describes_what_would_be_sent() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager_in(dir.path());
        m.ingest_stream_text("已確認", "");
        m.ingest_final_text("已確認的內容。");
        m.ingest_stream_text("還在辨識", "");

        let json = m
            .with_session(|s| {
                let t = s.transcript();
                serde_json::json!({
                    "session_id": t.session_id,
                    "segment_count": t.segments.len(),
                    "final_segment_count": t.segments.iter().filter(|x| x.is_final).count(),
                    "preview": t.display_text(),
                })
                .to_string()
            })
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["segment_count"], 2);
        assert_eq!(parsed["final_segment_count"], 1);
        assert!(parsed["preview"].as_str().unwrap().contains("已確認的內容。"));
    }

    #[test]
    fn configuration_round_trips_through_the_file_and_never_holds_a_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        // One shared credential store, as the real app has one OS store.
        let store = Arc::new(MemorySecretStore::new());
        let m = manager_with_store(dir.path(), Arc::clone(&store));

        let mut config = ApiConfig::new("https://api.example.test/v1");
        config.timeout_ms = 12_345;
        m.persist_config(&config).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("api.example.test"));
        assert!(raw.contains("12345"));

        // The secret lives in the credential store, so it is absent from the
        // file and from the in-memory config.
        let secret = "sk-live-should-not-appear";
        m.secret_store
            .set(&m.secret_account(), &SecretString::new(secret))
            .unwrap();
        let raw_after = std::fs::read_to_string(&path).unwrap();
        assert!(!raw_after.contains(secret), "config file leaked the key");
        assert!(!format!("{config:?}").contains(secret));

        // A fresh manager loads the persisted config back.
        let reloaded = manager_with_store(dir.path(), store);
        let loaded = reloaded.config.lock().unwrap().clone();
        assert_eq!(loaded.base_url, config.base_url);
        assert_eq!(loaded.timeout_ms, 12_345);
        // And the key is still readable from the store, not from the file.
        assert_eq!(
            reloaded
                .secret_store
                .get(&reloaded.secret_account())
                .unwrap()
                .expect("the shared store still holds the key")
                .expose(),
            secret
        );
    }

    #[test]
    fn a_corrupt_config_file_does_not_prevent_startup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CONFIG_FILE), "{ not valid json").unwrap();
        let m = manager_in(dir.path());
        // Falls back to defaults rather than panicking.
        assert_eq!(m.config.lock().unwrap().base_url, "");
    }

    #[test]
    fn the_config_json_the_gui_sends_deserializes_into_api_config() {
        // Guards the frontend/Rust contract: the UI builds this exact shape.
        let json = r#"{
            "base_url": "https://api.example.test/v1",
            "endpoint": "transcripts",
            "method": "POST",
            "headers": { "X-Tenant": "acme" },
            "content_type": "application/json",
            "timeout_ms": 30000,
            "retry": { "max_attempts": 3, "base_delay_ms": 500, "max_delay_ms": 8000 },
            "idempotency": "enabled",
            "request_format": { "kind": "wrapped", "field": "transcript" },
            "allow_insecure_http": false
        }"#;
        let config: ApiConfig = serde_json::from_str(json).expect("GUI payload must deserialize");
        assert_eq!(config.method, HttpMethod::Post);
        assert_eq!(config.headers.get("X-Tenant").map(String::as_str), Some("acme"));
        assert!(matches!(
            config.request_format,
            RequestFormat::Wrapped { .. }
        ));
        config.validate().expect("and must be valid");
    }
}
