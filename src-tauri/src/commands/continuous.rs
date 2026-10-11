//! Tauri commands for automatic continuous transcription and the integration
//! bridge.
//!
//! Both are kept in one module because they are two halves of the same feature:
//! continuous mode produces the segments, and the bridge is what makes them
//! observable by the verification tool. Splitting them would obscure that the
//! bridge also carries the manually-produced segments.
//!
//! Commands return JSON strings rather than generated types. The
//! `handy-voice-api` crate deliberately does not depend on a UI framework, and
//! these settings structs live in modules that would otherwise need
//! `specta::Type` derives for no benefit. The front end declares matching
//! interfaces, as it already does for the Voice API payloads.

use std::sync::Arc;

use tauri::State;

use crate::bridge::{Bridge, BridgeConfig};
use crate::managers::continuous::{ContinuousManager, ContinuousSettings};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/* ------------------------------- continuous ------------------------------- */

/// Start a continuous session. Refused if one is already running.
#[tauri::command]
#[specta::specta]
pub fn continuous_start(manager: State<'_, Arc<ContinuousManager>>) -> Result<String, String> {
    let state = manager.start()?;
    serde_json::to_string(&state).map_err(err)
}

/// Stop the session, transcribing whatever has already been captured.
#[tauri::command]
#[specta::specta]
pub fn continuous_stop(manager: State<'_, Arc<ContinuousManager>>) -> Result<String, String> {
    let state = manager.stop()?;
    serde_json::to_string(&state).map_err(err)
}

/// Current session state. Cheap enough to poll.
#[tauri::command]
#[specta::specta]
pub fn continuous_state(manager: State<'_, Arc<ContinuousManager>>) -> String {
    serde_json::to_string(&manager.state()).unwrap_or_else(|_| "{}".to_string())
}

#[tauri::command]
#[specta::specta]
pub fn continuous_get_settings(manager: State<'_, Arc<ContinuousManager>>) -> String {
    manager.settings_json()
}

/// Replace the pause-detection settings. Values are clamped to sane ranges.
#[tauri::command]
#[specta::specta]
pub fn continuous_set_settings(
    manager: State<'_, Arc<ContinuousManager>>,
    settings_json: String,
) -> Result<(), String> {
    let settings: ContinuousSettings = serde_json::from_str(&settings_json).map_err(err)?;
    manager.set_settings(settings)
}

/* --------------------------------- bridge --------------------------------- */

#[tauri::command]
#[specta::specta]
pub fn bridge_get_config(bridge: State<'_, Arc<Bridge>>) -> String {
    bridge.config_json()
}

/// Replace the bridge configuration and persist it.
///
/// Validation refuses to send transcripts in clear text to a non-loopback host,
/// so a typo in the endpoint surfaces here rather than silently exposing speech.
#[tauri::command]
#[specta::specta]
pub fn bridge_set_config(
    bridge: State<'_, Arc<Bridge>>,
    config_json: String,
) -> Result<(), String> {
    let config: BridgeConfig = serde_json::from_str(&config_json).map_err(err)?;
    bridge.set_config(config)
}

/// Counters and the last error, so the UI can show whether events are actually
/// being delivered instead of assuming they are.
#[tauri::command]
#[specta::specta]
pub fn bridge_status(bridge: State<'_, Arc<Bridge>>) -> String {
    bridge.status().snapshot().to_string()
}

/// Send a heartbeat now, rather than waiting for the next interval.
#[tauri::command]
#[specta::specta]
pub fn bridge_heartbeat_now(bridge: State<'_, Arc<Bridge>>) {
    bridge.heartbeat_now();
}
