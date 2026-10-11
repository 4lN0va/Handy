//! Tracks whether speech is currently happening, from the audio level stream.
//!
//! # Why this exists
//!
//! Automatic continuous transcription needs to know when the speaker paused, so a
//! segment can be closed at a natural boundary instead of being cut at a fixed
//! interval or run on forever.
//!
//! The recorder's VAD already decides speech-vs-noise, but that decision is made
//! inside the capture worker and only affects which frames are kept; it is not
//! observable from outside. Rather than thread new state through that real-time
//! path, this reads the spectrum-level buckets that are *already* being computed
//! and pushed to the front end for the level meter. That is a passive observer of
//! data the audio path produces anyway, so it cannot alter what gets recorded.
//!
//! # Choosing the threshold
//!
//! The buckets are **not** raw amplitude. `visualizer.rs` maps a dB range
//! through a compressive curve:
//!
//! ```text
//! normalized = ((db + 68) / 38).clamp(0, 1)   // DB_MIN -68, DB_MAX -30
//! bucket     = (normalized * 1.3).powf(0.7)    // GAIN 1.3, CURVE_POWER 0.7
//! ```
//!
//! | bucket | dB    |
//! |--------|-------|
//! | 0.12   | -66.6 |
//! | 0.24   | -64.9 |
//! | 0.45   | -57.5 |
//! | 0.56   | -55   |
//! | 0.71   | -50   |
//! | 0.95   | -40   |
//!
//! So `0.12` sits barely above digital silence: ordinary room tone clears it,
//! `last_voice_ms` then stays fresh forever, the pause detector never observes a
//! gap, and segments only ever end at `max_segment_ms`. A threshold that *looks*
//! small can therefore disable pause segmentation entirely - which is exactly
//! what happened here.
//!
//! The default is a gate around -57 dB: above a typical quiet-room floor and
//! below normal speech. In a room with continuous background music, raise it
//! toward 0.7-0.8 (about -50 to -45 dB); that will eventually start cutting quiet
//! speech, so it is a real trade-off rather than a free win.
//!
//! # What it is not
//!
//! A speech detector. It answers "how long since the level was last above the
//! noise floor", which is the same signal a level meter shows. Mechanical noise,
//! music, or a fan loud enough to cross the threshold will all read as speech -
//! which is the correct failure mode here, since the consequence is only that a
//! segment boundary is deferred, never that audio is dropped.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Milliseconds since the process started observing audio.
///
/// Monotonic, and shared by the audio consumer thread and the continuous-mode
/// driver so their two readings are directly comparable. A wall clock would be
/// wrong here: an NTP correction mid-session could make the elapsed silence
/// negative or enormous and cut a segment at the wrong moment.
pub fn monotonic_ms() -> u64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Level (0.0-1.0) below which a chunk counts as silence.
///
/// Roughly -57 dB on the visualizer's curve - see the module docs for the
/// mapping. Deliberately not lower: anything near the bottom of that curve is
/// cleared by ordinary room tone, which silently disables pause detection.
/// Deliberately not much higher: that starts truncating quiet speech.
pub const DEFAULT_SILENCE_LEVEL: f32 = 0.45;

/// How speech activity is tracked across the audio callback and the driver.
///
/// All fields are atomics so the audio consumer thread only ever does a relaxed
/// store; nothing here allocates, locks, or blocks.
#[derive(Debug)]
pub struct SpeechActivity {
    /// Milliseconds (monotonic, from an arbitrary origin) of the last chunk whose
    /// level rose above the silence threshold. Zero until the first such chunk.
    last_voice_ms: AtomicU64,
    /// Count of chunks that were above the threshold. Used by the driver to tell
    /// "this segment contains speech" from "this segment is only room tone",
    /// without having to inspect the audio buffer.
    voiced_chunks: AtomicU64,
    /// Count of every chunk observed, voiced or not. Lets a caller wait for the
    /// detector to warm up rather than concluding "silence" from no data.
    total_chunks: AtomicU64,
    /// Threshold, stored as the bit pattern of an `f32` so it stays atomic.
    threshold_bits: AtomicU64,
}

impl Default for SpeechActivity {
    fn default() -> Self {
        Self::new(DEFAULT_SILENCE_LEVEL)
    }
}

impl SpeechActivity {
    pub fn new(threshold: f32) -> Self {
        Self {
            last_voice_ms: AtomicU64::new(0),
            voiced_chunks: AtomicU64::new(0),
            total_chunks: AtomicU64::new(0),
            threshold_bits: AtomicU64::new((threshold as f64).to_bits()),
        }
    }

    pub fn threshold(&self) -> f32 {
        f64::from_bits(self.threshold_bits.load(Ordering::Relaxed)) as f32
    }

    pub fn set_threshold(&self, threshold: f32) {
        self.threshold_bits
            .store((threshold as f64).to_bits(), Ordering::Relaxed);
    }

    /// Record one chunk of level buckets, as produced by the visualizer.
    ///
    /// Called from the audio consumer thread: no locks, no allocation, no I/O.
    pub fn note_levels(&self, levels: &[f32], now_ms: u64) {
        self.total_chunks.fetch_add(1, Ordering::Relaxed);

        let threshold = self.threshold();
        let peak = levels
            .iter()
            .copied()
            .fold(0.0f32, |acc, v| if v > acc { v } else { acc });

        if peak > threshold {
            self.voiced_chunks.fetch_add(1, Ordering::Relaxed);
            self.last_voice_ms.store(now_ms, Ordering::Release);
        }
    }

    pub fn last_voice_ms(&self) -> u64 {
        self.last_voice_ms.load(Ordering::Acquire)
    }

    pub fn voiced_chunks(&self) -> u64 {
        self.voiced_chunks.load(Ordering::Relaxed)
    }

    /// How many chunks have been observed since the last reset.
    ///
    /// Used as a warm-up guard: before the first report the detector cannot tell
    /// a silent room from a microphone that is not delivering audio at all.
    pub fn total_chunks(&self) -> u64 {
        self.total_chunks.load(Ordering::Relaxed)
    }

    /// Forget everything. Used when a new segment begins.
    pub fn reset(&self) {
        self.last_voice_ms.store(0, Ordering::Release);
        self.voiced_chunks.store(0, Ordering::Relaxed);
        self.total_chunks.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loud_chunk_records_voice_and_a_quiet_one_does_not() {
        let a = SpeechActivity::new(0.45);
        a.note_levels(&[0.0, 0.5, 0.1], 1_000);
        assert_eq!(a.last_voice_ms(), 1_000);
        assert_eq!(a.voiced_chunks(), 1);

        a.note_levels(&[0.0, 0.01, 0.0], 1_100);
        assert_eq!(a.last_voice_ms(), 1_000, "quiet audio must not advance it");
        assert_eq!(a.voiced_chunks(), 1);
        assert_eq!(a.total_chunks(), 2, "every chunk is counted");
    }

    #[test]
    fn the_peak_of_the_buckets_is_what_decides() {
        // A single loud bucket in an otherwise silent frame still counts as voice.
        let a = SpeechActivity::new(0.45);
        a.note_levels(&[0.0, 0.0, 0.0, 0.9], 500);
        assert_eq!(a.voiced_chunks(), 1);
    }

    #[test]
    fn an_empty_level_report_is_not_voice() {
        // The visualizer can legitimately return no buckets; treating that as
        // speech would make a mute microphone look like continuous talking.
        let a = SpeechActivity::new(0.45);
        a.note_levels(&[], 700);
        assert_eq!(a.voiced_chunks(), 0);
        assert_eq!(a.last_voice_ms(), 0);
        assert_eq!(a.total_chunks(), 1);
    }

    #[test]
    fn exactly_at_the_threshold_is_not_voice() {
        // The comparison is strict, so the threshold is the first *silent* level.
        let a = SpeechActivity::new(0.45);
        a.note_levels(&[0.45], 100);
        assert_eq!(a.voiced_chunks(), 0);
        a.note_levels(&[0.4501], 200);
        assert_eq!(a.voiced_chunks(), 1);
    }

    #[test]
    fn reset_clears_the_counters() {
        let a = SpeechActivity::new(0.45);
        a.note_levels(&[0.5], 100);
        a.reset();
        assert_eq!(a.last_voice_ms(), 0);
        assert_eq!(a.voiced_chunks(), 0);
        assert_eq!(a.total_chunks(), 0);
    }

    #[test]
    fn the_threshold_can_be_tuned_at_runtime() {
        let a = SpeechActivity::new(0.02);
        a.note_levels(&[0.1], 100);
        assert_eq!(a.voiced_chunks(), 1);

        a.set_threshold(0.5);
        a.note_levels(&[0.1], 200);
        assert_eq!(a.voiced_chunks(), 1, "now below the raised threshold");
        assert_eq!(a.last_voice_ms(), 100);
    }
}
