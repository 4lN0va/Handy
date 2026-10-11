//! Automatic continuous transcription with natural, pause-based segmentation.
//!
//! # What this does
//!
//! The user starts once. From then on the microphone keeps recording, and the
//! transcript is cut into segments at the speaker's natural pauses instead of at
//! a fixed interval or at a keypress. Each completed segment becomes a finalized
//! transcript segment; the next one begins by itself.
//!
//! # How it decides where a sentence ends
//!
//! It does **not** re-implement voice activity detection. It reuses two things
//! that already exist:
//!
//! 1. The recorder's VAD, which is already deciding speech-vs-noise and is
//!    already producing the level buckets that drive the on-screen meter. Those
//!    buckets are observed passively by [`super::speech_activity`]; the audio path
//!    is untouched.
//! 2. The ordinary record → stop → transcribe path, unchanged: `stop_recording`
//!    returns the captured samples exactly as it does for a manual recording, and
//!    the samples go to the same `TranscriptionManager::transcribe`.
//!
//! A segment is closed when the level has stayed below the noise floor for
//! `pause_ms` after speech was heard. Because the recorder's VAD holds a
//! post-speech hangover tail, closing on sustained silence means the tail has
//! already been flushed, so the end of the sentence is not clipped.
//!
//! # Why the recording is restarted before transcribing
//!
//! The new recording starts the instant the samples are taken, *before* the
//! transcription runs. Transcription takes a few hundred milliseconds at least,
//! and any audio captured during it belongs to the next segment. Restarting first
//! means it is captured rather than dropped.
//!
//! # Interaction with the manual flow
//!
//! Nothing here modifies the shortcut path. Continuous mode uses its own binding
//! id and its own stop/start calls; the existing record, transcribe, paste and
//! clipboard behaviour is untouched. Only one of the two can be recording at a
//! time, which the recorder's state machine already enforces.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::audio_toolkit::VadPolicy;
use crate::commands::voice_api::VoiceApiManager;
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::speech_activity::{monotonic_ms, SpeechActivity};
use crate::managers::transcription::TranscriptionManager;

/// Binding id used for continuous recordings.
///
/// Distinct from every shortcut binding so the recorder's state machine can tell
/// the two apart and refuse to start one while the other is active.
const BINDING_ID: &str = "continuous";

const CONFIG_FILE: &str = "continuous_config.json";
const MONITOR_TICK: Duration = Duration::from_millis(100);

/// Tunables for pause detection. Exposed so the behaviour can be adjusted without
/// a rebuild, and so the values are visible rather than buried in the code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContinuousSettings {
    /// Silence required to close a segment. Long enough to survive a breath or a
    /// comma, short enough that a speaker does not have to wait.
    pub pause_ms: u64,
    /// Ignore a segment shorter than this: coughs, clicks, a single syllable.
    pub min_segment_ms: u64,
    /// Force a cut for a speaker who never pauses, so a monologue still appears
    /// as a sequence of segments rather than one unbounded block.
    pub max_segment_ms: u64,
    /// Level (0.0-1.0) below which audio counts as silence.
    ///
    /// Raised above typical background-music peaks so a continuous BGM is read
    /// as silence; the cost is that very quiet speech may be cut slightly early.
    pub silence_level: f32,
    /// Feature gate: when off, the transcription shortcut does nothing.
    /// Recording is never started directly from the UI toggle.
    pub enabled: bool,
}

impl Default for ContinuousSettings {
    fn default() -> Self {
        Self {
            pause_ms: 800,
            min_segment_ms: 400,
            max_segment_ms: 30_000,
            silence_level: crate::managers::speech_activity::DEFAULT_SILENCE_LEVEL,
            enabled: true,
        }
    }
}

impl ContinuousSettings {
    fn sanitized(mut self) -> Self {
        // Must outlast the recorder's own post-speech hangover
        // (VAD_OFFLINE_HANGOVER_MS, 450ms). Cutting earlier than that finds the
        // VAD still in its speech state, and `stop_recording` then withholds the
        // tail - the last syllable of the sentence - so the segment is
        // truncated and can transcribe to nothing at all. A short pause_ms looks
        // like a responsiveness win and is actually a correctness bug.
        let min_pause = crate::audio_toolkit::vad::VAD_OFFLINE_HANGOVER_MS + 150;
        self.pause_ms = self.pause_ms.clamp(min_pause, 10_000);
        self.min_segment_ms = self.min_segment_ms.clamp(100, 5_000);
        self.max_segment_ms = self.max_segment_ms.clamp(2_000, 600_000);
        // Never let a segment cap sit below the pause that closes a segment, or
        // the cap would fight the natural boundary.
        if self.max_segment_ms < self.pause_ms * 2 {
            self.max_segment_ms = self.pause_ms * 2;
        }
        self.silence_level = self.silence_level.clamp(0.0, 0.5);
        self
    }
}

/// Live state of the continuous session, for the GUI.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ContinuousState {
    pub running: bool,
    pub session_id: Option<String>,
    pub segments_completed: u64,
    pub running_ms: u64,
    pub pending_segments: u64,
}

struct Running {
    session_id: String,
    stop: Arc<AtomicBool>,
    monitor: Option<std::thread::JoinHandle<()>>,
    transcriber_tx: Option<Sender<Vec<f32>>>,
    transcriber: Option<std::thread::JoinHandle<()>>,
    started_at: Instant,
    segments: Arc<std::sync::atomic::AtomicU64>,
    pending: Arc<std::sync::atomic::AtomicU64>,
}

pub struct ContinuousManager {
    app: AppHandle,
    settings: Mutex<ContinuousSettings>,
    config_path: PathBuf,
    running: Mutex<Option<Running>>,
}

impl ContinuousManager {
    pub fn new(app: &AppHandle) -> Arc<Self> {
        let config_path = app
            .path()
            .app_data_dir()
            .map(|dir| dir.join(CONFIG_FILE))
            .unwrap_or_else(|_| PathBuf::from(CONFIG_FILE));
        let settings = load_settings(&config_path);
        Arc::new(Self {
            app: app.clone(),
            settings: Mutex::new(settings),
            config_path,
            running: Mutex::new(None),
        })
    }

    pub fn settings(&self) -> ContinuousSettings {
        self.settings.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Whether the continuous-listening feature is switched on.
    pub fn is_enabled(&self) -> bool {
        self.settings().enabled
    }

    /// Switch the feature on or off. The toggle never starts recording itself;
    /// it only arms the transcription shortcut.
    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let mut settings = self.settings();
        settings.enabled = enabled;
        self.set_settings(settings)
    }

    pub fn settings_json(&self) -> String {
        serde_json::to_string(&self.settings()).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn set_settings(&self, settings: ContinuousSettings) -> Result<(), String> {
        let settings = settings.sanitized();
        {
            let mut guard = self
                .settings
                .lock()
                .map_err(|_| "continuous settings lock poisoned".to_string())?;
            *guard = settings.clone();
        }
        self.persist(&settings)?;
        // Apply the new silence threshold to the live detector immediately.
        if let Some(activity) = self.activity() {
            activity.set_threshold(settings.silence_level);
        }
        Ok(())
    }

    fn persist(&self, settings: &ContinuousSettings) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("could not serialize continuous settings: {e}"))?;
        std::fs::write(&self.config_path, json)
            .map_err(|e| format!("could not write {}: {e}", self.config_path.display()))
    }

    fn room(&self) -> Arc<AudioRecordingManager> {
        self.app
            .state::<Arc<AudioRecordingManager>>()
            .inner()
            .clone()
    }

    fn transcriber(&self) -> Arc<TranscriptionManager> {
        self.app
            .state::<Arc<TranscriptionManager>>()
            .inner()
            .clone()
    }

    fn activity(&self) -> Option<Arc<SpeechActivity>> {
        self.room().speech_activity()
    }

    pub fn state(&self) -> ContinuousState {
        let guard = match self.running.lock() {
            Ok(g) => g,
            Err(_) => return ContinuousState::default(),
        };
        match guard.as_ref() {
            Some(running) => ContinuousState {
                running: true,
                session_id: Some(running.session_id.clone()),
                segments_completed: running.segments.load(Ordering::Relaxed),
                running_ms: running.started_at.elapsed().as_millis() as u64,
                pending_segments: running.pending.load(Ordering::Relaxed),
            },
            None => ContinuousState::default(),
        }
    }

    /// Begin a continuous session.
    ///
    /// Idempotent in the sense that a second call while one is running is
    /// refused rather than starting a competing session.
    pub fn start(&self) -> Result<ContinuousState, String> {
        let settings = self.settings();
        if !settings.enabled {
            return Err("continuous transcription is disabled".to_string());
        }
        {
            let guard = self
                .running
                .lock()
                .map_err(|_| "continuous state lock poisoned".to_string())?;
            if guard.is_some() {
                return Err("continuous transcription is already running".to_string());
            }
        }

        let room = self.room();
        let transcriber = self.transcriber();

        // Kick off a model load so the first segment is not delayed by it. It
        // loads lazily anyway; this only warms it.
        transcriber.initiate_model_load();

        let first = room.try_start_recording(BINDING_ID, VadPolicy::Offline)?;
        // Wait briefly for the device to actually deliver samples, so the first
        // segment is not opened against a stream that has not started. A failure
        // here is not fatal: recording has begun either way.
        let _ = first.wait();

        // Reset the activity counters so the segment starts from a clean slate.
        if let Some(activity) = self.activity() {
            activity.reset();
            activity.set_threshold(settings.silence_level);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let segments = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let pending = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let session_id = format!("cont-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));

        // --- transcriber worker: one segment at a time, in order ---------------
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let transcriber_handle = spawn_transcriber(
            self.app.clone(),
            rx,
            Arc::clone(&stop),
            Arc::clone(&pending),
        );

        // --- monitor: watches silence and closes segments ----------------------
        let monitor_handle = spawn_monitor(
            self.app.clone(),
            Arc::clone(&stop),
            settings,
            Arc::clone(&segments),
            tx.clone(),
        );

        let mut guard = self
            .running
            .lock()
            .map_err(|_| "continuous state lock poisoned".to_string())?;
        *guard = Some(Running {
            session_id: session_id.clone(),
            stop,
            monitor: Some(monitor_handle),
            transcriber_tx: Some(tx),
            transcriber: Some(transcriber_handle),
            started_at: Instant::now(),
            segments: Arc::clone(&segments),
            pending,
        });

        // Tell the bridge we are recording, immediately rather than waiting for
        // the next heartbeat interval.
        let bridge = self
            .app
            .state::<Arc<crate::bridge::Bridge>>()
            .inner()
            .clone();
        bridge.set_recording(true);
        bridge.set_activity("recording");
        bridge.heartbeat_now();

        log::info!("continuous transcription started (session {session_id})");
        drop(guard);
        Ok(self.state())
    }

    /// Stop the session and flush whatever has been captured.
    ///
    /// Ordering matters: the monitor stops first so no further segments are
    /// opened, then the in-flight audio is taken and queued, then the transcriber
    /// is drained. That way the final words are transcribed rather than discarded.
    pub fn stop(&self) -> Result<ContinuousState, String> {
        let running = {
            let mut guard = self
                .running
                .lock()
                .map_err(|_| "continuous state lock poisoned".to_string())?;
            guard.take()
        };
        let Some(running) = running else {
            return Ok(ContinuousState::default());
        };

        running.stop.store(true, Ordering::Release);
        if let Some(handle) = running.monitor {
            let _ = handle.join();
        }

        // Take whatever audio is in flight and hand it to the transcriber.
        let room = self.room();
        let token = room.cancel_generation();
        let tail = room.stop_recording(BINDING_ID, token);
        if let Some(samples) = tail {
            if !samples.is_empty() {
                if let Some(tx) = running.transcriber_tx.as_ref() {
                    let _ = tx.send(samples);
                }
            }
        }
        // Dropping the sender closes the channel, so the worker finishes the
        // queue and exits.
        drop(running.transcriber_tx);
        if let Some(handle) = running.transcriber {
            let _ = handle.join();
        }

        if let Some(activity) = self.activity() {
            activity.reset();
        }

        let bridge = self
            .app
            .state::<Arc<crate::bridge::Bridge>>()
            .inner()
            .clone();
        bridge.set_recording(false);
        bridge.set_activity("idle");
        bridge.heartbeat_now();

        log::info!(
            "continuous transcription stopped (session {}, {} segment(s))",
            running.session_id,
            running.segments.load(Ordering::Relaxed)
        );
        Ok(self.state())
    }
}

/// Handle a transcription-shortcut event on behalf of continuous mode.
///
/// Returns `true` when the event was consumed, meaning the caller must **not**
/// route it to the transcription coordinator. Returns `false` when the feature
/// is disarmed, leaving the shortcut exactly as it was before this feature.
///
/// # Why this is intercepted here and not inside the action
///
/// The coordinator keeps its own state machine (idle / recording / stopping).
/// Ignoring a press further down, inside `TranscribeAction`, left the
/// coordinator believing a recording was in progress even though continuous mode
/// had taken over. The visible result was a key that could start a session but
/// never stop it, and could not restart one after a stop from the UI - the
/// coordinator answered every later press with `stop`, which was then ignored.
///
/// Consuming the event before the coordinator sees it keeps that state machine
/// untouched, so it stays idle for as long as continuous mode owns the key.
///
/// Only the press toggles; the release is consumed and ignored. That makes the
/// key a toggle regardless of the configured activation mode, which is what
/// continuous listening needs: a push-to-talk release must not end a session the
/// user expects to keep running.
pub fn handle_shortcut_event(app: &AppHandle, is_pressed: bool) -> bool {
    let Some(state) = app.try_state::<Arc<ContinuousManager>>() else {
        return false;
    };
    let manager = state.inner().clone();
    if !manager.is_enabled() {
        return false;
    }

    // Consume the release without acting on it.
    if !is_pressed {
        return true;
    }

    let running = manager.state().running;
    let outcome = if running {
        manager.stop()
    } else {
        manager.start()
    };

    match outcome {
        Ok(state) => log::info!(
            "continuous: shortcut {} the session (running={})",
            if running { "stopped" } else { "started" },
            state.running
        ),
        // Reported as consumed even on failure: falling through to the
        // coordinator as well would start a second, competing recording, which
        // is more confusing than a logged failure.
        Err(e) => log::warn!(
            "continuous: shortcut could not {} the session: {e}",
            if running { "stop" } else { "start" }
        ),
    }
    true
}
fn load_settings(path: &PathBuf) -> ContinuousSettings {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<ContinuousSettings>(&text)
            .unwrap_or_default()
            .sanitized(),
        Err(_) => ContinuousSettings::default(),
    }
}

/// Watches for a pause and closes the current segment when one is found.
fn spawn_monitor(
    app: AppHandle,
    stop: Arc<AtomicBool>,
    settings: ContinuousSettings,
    segments: Arc<std::sync::atomic::AtomicU64>,
    tx: Sender<Vec<f32>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("handy-continuous-monitor".to_string())
        .spawn(move || {
            let room = app.state::<Arc<AudioRecordingManager>>().inner().clone();
            let activity = match room.speech_activity() {
                Some(a) => a,
                None => {
                    log::error!("continuous: speech activity is unavailable; stopping");
                    return;
                }
            };

            let mut segment_started = Instant::now();
            let mut restarts_failed = 0u32;

            let mut last_log_ms = 0u64;
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(MONITOR_TICK);
                if stop.load(Ordering::Acquire) {
                    break;
                }

                // No level reports yet: a freshly reset segment cannot tell a
                // quiet room from a microphone that has stopped delivering audio,
                // so do not act on the absence of data.
                if activity.total_chunks() == 0 {
                    continue;
                }

                // Diagnostic: log the decision inputs once per second so a stuck
                // segment is debuggable from the log instead of guessing.
                let now_diag = monotonic_ms();
                if now_diag.saturating_sub(last_log_ms) >= 1000 {
                    last_log_ms = now_diag;
                    let lv = activity.last_voice_ms();
                    let quiet_for = now_diag.saturating_sub(lv);
                    log::info!(
                        "continuous monitor: voiced_chunks={}, quiet_for={}ms, threshold={}, segment_age={}ms, pause_ms={}ms, max_segment_ms={}ms",
                        activity.voiced_chunks(),
                        quiet_for,
                        activity.threshold(),
                        segment_started.elapsed().as_millis(),
                        settings.pause_ms,
                        settings.max_segment_ms,
                    );
                }

                let now = monotonic_ms();
                let last_voice = activity.last_voice_ms();
                let has_speech = activity.voiced_chunks() > 0;

                // Silence only counts once speech has actually been heard in this
                // segment. Without that guard, the silence between segments would
                // immediately close the next one.
                let quiet_for = if has_speech && last_voice > 0 {
                    now.saturating_sub(last_voice)
                } else {
                    0
                };
                let segment_age = segment_started.elapsed().as_millis() as u64;

                let natural_pause = has_speech && quiet_for >= settings.pause_ms;
                let too_long = segment_age >= settings.max_segment_ms;
                let too_short = segment_age < settings.min_segment_ms;

                if !(natural_pause || too_long) {
                    continue;
                }
                if too_short && !too_long {
                    // A very short blip that went quiet; wait rather than emit
                    // a fragment, unless we are already at the ceiling.
                    if quiet_for < settings.pause_ms {
                        continue;
                    }
                }

                // --- close the segment -----------------------------------------
                let token = room.cancel_generation();
                let samples = room.stop_recording(BINDING_ID, token);

                // Restart IMMEDIATELY so audio captured while this segment is
                // being transcribed lands in the next one instead of being lost.
                let mut restarted = false;
                for attempt in 0..3 {
                    match room.try_start_recording(BINDING_ID, VadPolicy::Offline) {
                        Ok(_) => {
                            restarted = true;
                            break;
                        }
                        Err(e) => {
                            log::warn!(
                                "continuous: could not restart recording (attempt {}): {e}",
                                attempt + 1
                            );
                            std::thread::sleep(Duration::from_millis(60));
                        }
                    }
                }
                if !restarted {
                    restarts_failed += 1;
                    if restarts_failed >= 3 {
                        log::error!(
                            "continuous: recording could not be restarted; stopping the session"
                        );
                        break;
                    }
                } else {
                    restarts_failed = 0;
                }

                activity.reset();
                segment_started = Instant::now();

                if let Some(samples) = samples {
                    if !samples.is_empty() {
                        let cause = if natural_pause { "pause" } else { "length cap" };
                        log::debug!(
                            "continuous: closing segment after {}ms ({cause}), {} samples",
                            segment_age,
                            samples.len()
                        );
                        // Straight to the transcriber's queue: the monitor holds
                        // the sender, so no manager lookup is needed here.
                        if tx.send(samples).is_ok() {
                            segments.fetch_add(1, Ordering::Relaxed);
                        } else {
                            log::warn!("continuous: transcriber queue is closed; stopping");
                            break;
                        }
                    }
                }
            }

            log::debug!("continuous: monitor thread exiting");
        })
        .expect("spawning the continuous monitor thread")
}

/// Transcribes queued segments one at a time, in order.
///
/// Sequential on purpose: the model worker is a single resource, and serialising
/// here keeps segment order identical to speech order, which is what the
/// verification tool's merge expects.
fn spawn_transcriber(
    app: AppHandle,
    rx: Receiver<Vec<f32>>,
    stop: Arc<AtomicBool>,
    pending: Arc<std::sync::atomic::AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("handy-continuous-transcriber".to_string())
        .spawn(move || {
            let transcriber = app.state::<Arc<TranscriptionManager>>().inner().clone();
            let voice_api = app.state::<Arc<VoiceApiManager>>().inner().clone();
            let history = app.state::<Arc<HistoryManager>>().inner().clone();

            let mut seq = 0u64;
            while let Ok(samples) = rx.recv() {
                pending.fetch_add(1, Ordering::Relaxed);
                let sample_count = samples.len();

                // Keep the last few samples so the model has a clean tail; the
                // recorder's VAD has already trimmed the silence.
                match transcriber.transcribe(samples.clone()) {
                    Ok(text) => {
                        let trimmed = text.trim().to_string();
                        if trimmed.is_empty() {
                            log::debug!(
                                "continuous: segment {seq} produced no text ({sample_count} samples)"
                            );
                        } else {
                            // Hand it to the EXISTING transcript pipeline. This is
                            // the same entry point the manual flow uses, so the
                            // transcript model, the UI event and the
                            // verification-tool bridge all see it without a
                            // second pipeline being built.
                            voice_api.accept_final_segment(&app, &trimmed);
                            save_history(&history, &trimmed, &samples);
                            seq += 1;
                        }
                    }
                    Err(e) => {
                        log::warn!("continuous: transcription failed: {e}");
                        voice_api.record_recognition_error(&app, &format!("{e}"));
                    }
                }
                pending.fetch_sub(1, Ordering::Relaxed);
            }

            // A stop request before the queue drained is not an error: the
            // remaining items were transcribed above.
            let _ = stop.load(Ordering::Acquire);
            log::debug!("continuous: transcriber thread exiting after {seq} segment(s)");
        })
        .expect("spawning the continuous transcriber thread")
}

/// Write the segment's audio and a history row, so continuous output is not lost
/// when the app closes.
///
/// Mirrors the manual flow, which also saves a WAV alongside the history entry.
fn save_history(history: &Arc<HistoryManager>, text: &str, samples: &[f32]) {
    let file_name = format!("continuous-{}.wav", chrono::Utc::now().timestamp_millis());
    let path = history.recordings_dir().join(&file_name);
    if let Err(e) = crate::audio_toolkit::save_wav_file(&path, samples) {
        log::warn!("continuous: could not save {}: {e}", path.display());
        // Still record the text: losing the audio is not a reason to lose the
        // transcription.
        if let Err(e) = history.save_entry(file_name, text.to_string(), false, None, None) {
            log::warn!("continuous: could not save history entry: {e}");
        }
        return;
    }
    if let Err(e) = history.save_entry(file_name, text.to_string(), false, None, None) {
        log::warn!("continuous: could not save history entry: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_describe_a_usable_pause_detector() {
        let s = ContinuousSettings::default();
        assert_eq!(s.pause_ms, 800);
        assert!(s.min_segment_ms < s.pause_ms);
        assert!(s.max_segment_ms > s.pause_ms);
        // Roughly -57 dB on the visualizer curve. The old < 0.1 bound encoded a
        // threshold so low that ordinary room tone cleared it, which disabled
        // pause detection entirely.
        assert!(s.silence_level > 0.3 && s.silence_level < 0.6);
        assert!(s.enabled, "continuous mode is armed by default");
    }

    #[test]
    fn sanitizing_clamps_nonsense_values() {
        let s = ContinuousSettings {
            pause_ms: 0,
            min_segment_ms: 0,
            max_segment_ms: 0,
            silence_level: 5.0,
            enabled: true,
        }
        .sanitized();

        assert!(s.pause_ms >= 200, "a zero pause would cut constantly");
        assert!(s.min_segment_ms >= 100);
        assert!(s.max_segment_ms >= 2_000);
        assert!(s.silence_level <= 0.5);
    }

    #[test]
    fn the_length_cap_can_never_sit_below_the_pause() {
        // Otherwise the cap would pre-empt the natural boundary on every segment.
        let s = ContinuousSettings {
            pause_ms: 2_000,
            max_segment_ms: 2_100,
            ..Default::default()
        }
        .sanitized();
        assert!(s.max_segment_ms >= s.pause_ms * 2);
    }

    #[test]
    fn a_missing_config_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = load_settings(&dir.path().join("nope.json"));
        assert_eq!(s.pause_ms, ContinuousSettings::default().pause_ms);
    }

    #[test]
    fn a_corrupt_config_file_yields_defaults_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("continuous.json");
        std::fs::write(&path, "{ not json").unwrap();
        let s = load_settings(&path);
        assert_eq!(s.pause_ms, ContinuousSettings::default().pause_ms);
    }

    #[test]
    fn settings_survive_a_write_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("continuous.json");
        let original = ContinuousSettings {
            pause_ms: 1_250,
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&original).unwrap()).unwrap();
        let reloaded = load_settings(&path);
        assert_eq!(reloaded.pause_ms, 1_250);
    }

    #[test]
    fn the_stopped_state_reports_not_running() {
        // Guards the GUI contract: a stopped session must not claim to be live.
        let state = ContinuousState::default();
        assert!(!state.running);
        assert!(state.session_id.is_none());
        assert_eq!(state.segments_completed, 0);
    }
}
