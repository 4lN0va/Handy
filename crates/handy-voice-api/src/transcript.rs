//! Transcript model and the partial/final bookkeeping that feeds it.
//!
//! # Why this module exists
//!
//! Upstream Handy already has a live streaming pipeline with an internal
//! committed + tentative split (see `src-tauri/src/managers/transcription.rs`),
//! but it surfaces a single final string to the main window and emits
//! `StreamTextEvent` only to the recording overlay. There is no stable,
//! versioned, segment-addressed transcript that a UI can render incrementally or
//! export.
//!
//! This module adds exactly that, without touching the ASR engine:
//!
//! * every recognition result is addressed by a **stable `segment_id`**, so a UI
//!   renders by upsert and can never double-append;
//! * every update carries a **monotonic `revision`**, so out-of-order or
//!   duplicated deliveries from a streaming engine are dropped rather than
//!   corrupting the text;
//! * a segment that has been finalized is **never reopened** by a late partial.
//!
//! The invariant the whole design protects is the one the product requires:
//! *partial and final text must never both be appended to the transcript.*

use crate::error::VoiceApiError;
use crate::time::Clock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The transcript schema version written by this build.
///
/// Bump only alongside a migration path in [`crate::schema`].
pub const SCHEMA_VERSION: &str = "1.0";

/// What a recognition result represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    /// In-progress (tentative) text that may still change.
    Partial,
    /// Confirmed text for this segment. Will not change further.
    Final,
    /// The engine or audio device failed for this segment.
    Error,
}

impl SegmentKind {
    /// Whether this kind is a confirmed result.
    pub fn is_final(self) -> bool {
        matches!(self, SegmentKind::Final)
    }
}

/// One recognition result delivered by a speech provider.
///
/// `segment_id` is the identity; `revision` orders successive updates to that
/// same identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptUpdate {
    /// Stable identifier for the segment. Repeats across revisions.
    pub segment_id: String,
    /// Monotonically increasing revision for `segment_id`, starting at 1.
    pub revision: u64,
    /// Whether this is tentative or confirmed text.
    pub kind: SegmentKind,
    /// The text for this revision.
    pub text: String,
    /// Optional start offset in milliseconds from the session start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<i64>,
    /// Optional end offset in milliseconds from the session start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<i64>,
}

impl TranscriptUpdate {
    /// Convenience constructor for a partial update.
    pub fn partial(segment_id: impl Into<String>, revision: u64, text: impl Into<String>) -> Self {
        Self {
            segment_id: segment_id.into(),
            revision,
            kind: SegmentKind::Partial,
            text: text.into(),
            start_ms: None,
            end_ms: None,
        }
    }

    /// Convenience constructor for a final update.
    pub fn final_(segment_id: impl Into<String>, revision: u64, text: impl Into<String>) -> Self {
        Self {
            segment_id: segment_id.into(),
            revision,
            kind: SegmentKind::Final,
            text: text.into(),
            start_ms: None,
            end_ms: None,
        }
    }
}

/// A segment as stored in the transcript.
///
/// Serialized shape follows the product specification: `id`, `text` and `final`
/// are always present; `kind` disambiguates `error`; timestamps are optional
/// because sub-segment timing is explicitly not a first-phase requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    /// Dense, 1-based position in the session. Assigned by the manager.
    pub id: u64,
    /// Stable identity, repeated across revisions of the same segment.
    pub segment_id: String,
    /// Revision of the text currently stored.
    pub revision: u64,
    /// The current text of this segment.
    pub text: String,
    /// Convenience mirror of `kind == Final`, kept because the export format
    /// specifies a `final` boolean.
    #[serde(rename = "final")]
    pub is_final: bool,
    /// The result kind.
    pub kind: SegmentKind,
    /// Optional start offset in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start_ms: Option<i64>,
    /// Optional end offset in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub end_ms: Option<i64>,
}

/// A failure worth recording against the session but not part of its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptError {
    /// Human-readable description, safe to display.
    pub message: String,
    /// When the error was recorded (RFC 3339 UTC).
    pub at: String,
    /// Optional segment this error relates to.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub segment_id: Option<String>,
}

/// Which engine produced a transcript. Optional metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfo {
    /// Engine identifier, e.g. `transcribe-cpp`.
    pub id: String,
    /// Model identifier, e.g. `handy-computer/Breeze-ASR-25-gguf`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_id: Option<String>,
}

/// A complete transcription session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    /// Schema version of this document.
    ///
    /// `schemaversion` matches the field name given in the product
    /// specification. `schema_version` is accepted as an alias on read so that
    /// either spelling can be ingested.
    #[serde(rename = "schemaversion", alias = "schema_version")]
    pub schema_version: String,
    /// Session identifier.
    #[serde(rename = "sessionid", alias = "session_id")]
    pub session_id: String,
    /// When the session was created (RFC 3339 UTC).
    pub created_at: String,
    /// When the transcript last changed (RFC 3339 UTC).
    pub updated_at: String,
    /// BCP-47-ish language tag, e.g. `zh-TW`.
    pub language: String,
    /// Where the audio came from, e.g. `microphone`.
    pub source: String,
    /// Optional engine metadata.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub engine: Option<EngineInfo>,
    /// Recognized text, in recognition order.
    pub segments: Vec<TranscriptSegment>,
    /// Errors recorded during the session.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<TranscriptError>,
}

impl Transcript {
    /// Concatenate the confirmed text.
    ///
    /// Segments are joined with a single space; CJK text does not use spaces,
    /// so callers that need exact CJK spacing should render segments
    /// individually. This is a convenience for plain-text copy, not the
    /// canonical representation.
    pub fn committed_text(&self) -> String {
        self.segments
            .iter()
            .filter(|s| s.is_final)
            .map(|s| s.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Concatenate every non-empty segment, confirmed or tentative.
    pub fn display_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Whether any confirmed text exists.
    pub fn has_final_text(&self) -> bool {
        self.segments
            .iter()
            .any(|s| s.is_final && !s.text.trim().is_empty())
    }
}

/// What happened when an update was applied.
///
/// Returned rather than logged so callers (and tests) can assert on the exact
/// deduplication behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ApplyOutcome {
    /// A new segment was created.
    Created {
        /// The segment that was created.
        segment_id: String,
    },
    /// An existing segment was updated in place.
    Updated {
        /// The segment that was updated.
        segment_id: String,
        /// The revision now stored.
        revision: u64,
    },
    /// The update was older than what is already stored and was dropped.
    IgnoredStaleRevision {
        /// The segment the update targeted.
        segment_id: String,
        /// The revision the update carried.
        incoming: u64,
        /// The revision already stored.
        current: u64,
    },
    /// A partial arrived for an already-finalized segment; it was dropped.
    IgnoredAfterFinal {
        /// The segment the update targeted.
        segment_id: String,
    },
    /// The update carried no usable text and was dropped.
    IgnoredEmpty {
        /// The segment the update targeted.
        segment_id: String,
    },
}

impl ApplyOutcome {
    /// Whether the transcript changed.
    pub fn changed(&self) -> bool {
        matches!(self, Self::Created { .. } | Self::Updated { .. })
    }
}

/// Owns one session's transcript and enforces the deduplication rules.
pub struct TranscriptManager {
    transcript: Transcript,
    index: HashMap<String, usize>,
    clock: Box<dyn Clock>,
    next_id: u64,
}

impl std::fmt::Debug for TranscriptManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptManager")
            .field("session_id", &self.transcript.session_id)
            .field("segments", &self.transcript.segments.len())
            .field("errors", &self.transcript.errors.len())
            .finish()
    }
}

impl TranscriptManager {
    /// Start a new session.
    ///
    /// `session_id` must be caller-supplied (and unique) so that upload
    /// idempotency can key off it — see [`crate::api`].
    pub fn new(
        clock: Box<dyn Clock>,
        session_id: impl Into<String>,
        language: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let now = clock.now_rfc3339();
        let transcript = Transcript {
            schema_version: SCHEMA_VERSION.to_string(),
            session_id: session_id.into(),
            created_at: now.clone(),
            updated_at: now,
            language: language.into(),
            source: source.into(),
            engine: None,
            segments: Vec::new(),
            errors: Vec::new(),
        };
        Self {
            transcript,
            index: HashMap::new(),
            clock,
            next_id: 1,
        }
    }

    /// Attach engine metadata. Does not change the timestamp.
    pub fn set_engine(&mut self, engine: EngineInfo) {
        self.transcript.engine = Some(engine);
    }

    /// Read the current transcript.
    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    /// Take ownership of the transcript, ending this manager's session.
    pub fn into_transcript(self) -> Transcript {
        self.transcript
    }

    /// Apply a recognition update.
    ///
    /// This is the single entry point for all provider output, which is what
    /// guarantees partial and final text cannot both be appended: an update is
    /// addressed to a `segment_id` and *replaces* that segment's text.
    pub fn apply(&mut self, update: TranscriptUpdate) -> ApplyOutcome {
        let trimmed_empty = update.text.trim().is_empty();

        match self.index.get(&update.segment_id).copied() {
            Some(idx) => {
                let existing = &self.transcript.segments[idx];

                // A finalized segment is immutable: a late partial must not
                // reopen it, or the transcript would grow duplicate text.
                if existing.is_final && update.kind == SegmentKind::Partial {
                    return ApplyOutcome::IgnoredAfterFinal {
                        segment_id: update.segment_id,
                    };
                }

                if update.revision <= existing.revision {
                    return ApplyOutcome::IgnoredStaleRevision {
                        segment_id: update.segment_id,
                        incoming: update.revision,
                        current: existing.revision,
                    };
                }

                // A final arriving with no text keeps the text already shown
                // (the engine confirmed a segment but returned it blank) and
                // only flips the flag.
                let new_text = if trimmed_empty && update.kind == SegmentKind::Final {
                    existing.text.clone()
                } else if trimmed_empty {
                    return ApplyOutcome::IgnoredEmpty {
                        segment_id: update.segment_id,
                    };
                } else {
                    update.text
                };

                let segment = &mut self.transcript.segments[idx];
                segment.text = new_text;
                segment.revision = update.revision;
                segment.kind = update.kind;
                segment.is_final = update.kind.is_final();
                if update.start_ms.is_some() {
                    segment.start_ms = update.start_ms;
                }
                if update.end_ms.is_some() {
                    segment.end_ms = update.end_ms;
                }
                self.touch();
                ApplyOutcome::Updated {
                    segment_id: update.segment_id,
                    revision: update.revision,
                }
            }
            None => {
                // An empty result carries no information; creating a segment for
                // it would leave blank rows in the transcript and in exports.
                if trimmed_empty {
                    return ApplyOutcome::IgnoredEmpty {
                        segment_id: update.segment_id,
                    };
                }

                let id = self.next_id;
                self.next_id += 1;
                let segment = TranscriptSegment {
                    id,
                    segment_id: update.segment_id.clone(),
                    revision: update.revision,
                    text: update.text,
                    is_final: update.kind.is_final(),
                    kind: update.kind,
                    start_ms: update.start_ms,
                    end_ms: update.end_ms,
                };
                self.index
                    .insert(update.segment_id.clone(), self.transcript.segments.len());
                self.transcript.segments.push(segment);
                self.touch();
                ApplyOutcome::Created {
                    segment_id: update.segment_id,
                }
            }
        }
    }

    /// Replace a segment's text with a user edit.
    ///
    /// Editing always finalizes the segment: once a human has corrected the
    /// text, a later engine partial must not overwrite it.
    pub fn edit_segment(
        &mut self,
        segment_id: &str,
        text: impl Into<String>,
    ) -> Result<(), VoiceApiError> {
        let idx = *self
            .index
            .get(segment_id)
            .ok_or_else(|| VoiceApiError::NotFound {
                kind: "segment",
                id: segment_id.to_string(),
            })?;
        let segment = &mut self.transcript.segments[idx];
        segment.text = text.into();
        segment.is_final = true;
        segment.kind = SegmentKind::Final;
        segment.revision += 1;
        self.touch();
        Ok(())
    }

    /// Remove every segment and error, keeping the session identity.
    ///
    /// Also resets segment numbering so a cleared session re-exports cleanly.
    pub fn clear(&mut self) {
        self.transcript.segments.clear();
        self.transcript.errors.clear();
        self.index.clear();
        self.next_id = 1;
        self.touch();
    }

    /// Record a non-fatal failure against the session.
    pub fn record_error(&mut self, message: impl Into<String>, segment_id: Option<String>) {
        let at = self.clock.now_rfc3339();
        self.transcript.errors.push(TranscriptError {
            message: message.into(),
            at,
            segment_id,
        });
        self.touch();
    }

    /// Drop a segment entirely (used when a segment is retracted).
    pub fn remove_segment(&mut self, segment_id: &str) -> Result<(), VoiceApiError> {
        let idx = *self
            .index
            .get(segment_id)
            .ok_or_else(|| VoiceApiError::NotFound {
                kind: "segment",
                id: segment_id.to_string(),
            })?;
        self.transcript.segments.remove(idx);
        self.reindex();
        self.touch();
        Ok(())
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (i, segment) in self.transcript.segments.iter().enumerate() {
            self.index.insert(segment.segment_id.clone(), i);
        }
        // Re-number densely so `id` stays a stable display order after a removal.
        for (i, segment) in self.transcript.segments.iter_mut().enumerate() {
            segment.id = (i + 1) as u64;
        }
        self.next_id = (self.transcript.segments.len() + 1) as u64;
    }

    fn touch(&mut self) {
        self.transcript.updated_at = self.clock.now_rfc3339();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::FixedClock;

    fn manager() -> TranscriptManager {
        TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        )
    }

    #[test]
    fn new_session_stamps_created_and_updated_from_the_clock() {
        let m = manager();
        let t = m.transcript();
        assert_eq!(t.schema_version, SCHEMA_VERSION);
        assert_eq!(t.session_id, "session-001");
        assert_eq!(t.created_at, "2026-10-10T13:00:00Z");
        assert_eq!(t.updated_at, "2026-10-10T13:00:00Z");
        assert_eq!(t.language, "zh-TW");
        assert_eq!(t.source, "microphone");
        assert!(t.segments.is_empty());
    }

    // --- The core requirement: partial must not be re-appended as final. ---

    #[test]
    fn partial_then_final_updates_in_place_instead_of_appending_a_duplicate() {
        let mut m = manager();
        assert_eq!(
            m.apply(TranscriptUpdate::partial("s1", 1, "你好")),
            ApplyOutcome::Created {
                segment_id: "s1".into()
            }
        );
        assert_eq!(
            m.apply(TranscriptUpdate::partial("s1", 2, "你好，我想")),
            ApplyOutcome::Updated {
                segment_id: "s1".into(),
                revision: 2
            }
        );
        assert_eq!(
            m.apply(TranscriptUpdate::final_(
                "s1",
                3,
                "你好，我想測試語音辨識。"
            )),
            ApplyOutcome::Updated {
                segment_id: "s1".into(),
                revision: 3
            }
        );

        // One segment, holding only the final text — this is the regression the
        // whole design exists to prevent.
        let t = m.transcript();
        assert_eq!(t.segments.len(), 1, "must not append a second segment");
        assert_eq!(t.segments[0].text, "你好，我想測試語音辨識。");
        assert!(t.segments[0].is_final);
        assert_eq!(t.committed_text(), "你好，我想測試語音辨識。");
    }

    #[test]
    fn many_partial_revisions_never_grow_the_segment_count() {
        let mut m = manager();
        for rev in 1..=50 {
            m.apply(TranscriptUpdate::partial(
                "s1",
                rev,
                format!("累積文字 {rev}"),
            ));
        }
        let t = m.transcript();
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].text, "累積文字 50");
        assert_eq!(t.segments[0].revision, 50);
    }

    #[test]
    fn late_partial_after_final_is_dropped() {
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("s1", 5, "確認文字"));
        let outcome = m.apply(TranscriptUpdate::partial("s1", 6, "確認文字 (late)"));

        assert_eq!(
            outcome,
            ApplyOutcome::IgnoredAfterFinal {
                segment_id: "s1".into()
            }
        );
        assert_eq!(m.transcript().segments[0].text, "確認文字");
        assert_eq!(m.transcript().segments.len(), 1);
    }

    #[test]
    fn stale_revision_is_ignored() {
        let mut m = manager();
        m.apply(TranscriptUpdate::partial("s1", 10, "新文字"));
        let outcome = m.apply(TranscriptUpdate::partial("s1", 9, "舊文字"));

        assert_eq!(
            outcome,
            ApplyOutcome::IgnoredStaleRevision {
                segment_id: "s1".into(),
                incoming: 9,
                current: 10
            }
        );
        assert_eq!(m.transcript().segments[0].text, "新文字");
    }

    #[test]
    fn duplicate_revision_is_ignored() {
        let mut m = manager();
        m.apply(TranscriptUpdate::partial("s1", 3, "第一次"));
        let outcome = m.apply(TranscriptUpdate::partial("s1", 3, "重複投遞"));

        assert!(matches!(outcome, ApplyOutcome::IgnoredStaleRevision { .. }));
        assert_eq!(m.transcript().segments[0].text, "第一次");
    }

    #[test]
    fn replaying_an_entire_update_stream_is_idempotent() {
        // Models the realistic failure: a provider reconnects and replays the
        // whole session. Replay must not duplicate anything.
        let mut m = manager();
        let stream = vec![
            TranscriptUpdate::partial("s1", 1, "第一段"),
            TranscriptUpdate::final_("s1", 2, "第一段完成。"),
            TranscriptUpdate::partial("s2", 1, "第二段"),
            TranscriptUpdate::final_("s2", 2, "第二段完成。"),
        ];
        for u in &stream {
            m.apply(u.clone());
        }
        let after_first = m.transcript().clone();

        for u in &stream {
            m.apply(u.clone());
        }
        let after_replay = m.transcript().clone();

        assert_eq!(after_first.segments, after_replay.segments);
        assert_eq!(after_replay.segments.len(), 2);
        assert_eq!(after_replay.committed_text(), "第一段完成。 第二段完成。");
    }

    #[test]
    fn interleaved_segments_keep_independent_revisions() {
        let mut m = manager();
        m.apply(TranscriptUpdate::partial("a", 1, "A1"));
        m.apply(TranscriptUpdate::partial("b", 1, "B1"));
        m.apply(TranscriptUpdate::partial("a", 2, "A2"));
        m.apply(TranscriptUpdate::final_("b", 2, "B2"));
        m.apply(TranscriptUpdate::final_("a", 3, "A3"));

        let t = m.transcript();
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[0].segment_id, "a");
        assert_eq!(t.segments[0].text, "A3");
        assert_eq!(t.segments[1].segment_id, "b");
        assert_eq!(t.segments[1].text, "B2");
        assert!(t.segments.iter().all(|s| s.is_final));
    }

    // --- Empty and whitespace handling ---

    #[test]
    fn empty_partial_does_not_create_a_segment() {
        let mut m = manager();
        assert_eq!(
            m.apply(TranscriptUpdate::partial("s1", 1, "")),
            ApplyOutcome::IgnoredEmpty {
                segment_id: "s1".into()
            }
        );
        assert_eq!(
            m.apply(TranscriptUpdate::partial("s1", 2, "   \n\t ")),
            ApplyOutcome::IgnoredEmpty {
                segment_id: "s1".into()
            }
        );
        assert!(m.transcript().segments.is_empty());
    }

    #[test]
    fn empty_final_for_an_existing_segment_keeps_text_and_finalizes() {
        let mut m = manager();
        m.apply(TranscriptUpdate::partial("s1", 1, "有文字"));
        m.apply(TranscriptUpdate::final_("s1", 2, ""));

        let t = m.transcript();
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].text, "有文字");
        assert!(t.segments[0].is_final);
    }

    #[test]
    fn empty_final_for_an_unknown_segment_is_ignored() {
        let mut m = manager();
        assert!(matches!(
            m.apply(TranscriptUpdate::final_("ghost", 1, "  ")),
            ApplyOutcome::IgnoredEmpty { .. }
        ));
        assert!(m.transcript().segments.is_empty());
    }

    #[test]
    fn whitespace_around_text_is_preserved_but_blank_text_is_not_a_segment() {
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("s1", 1, "  你好  "));
        assert_eq!(m.transcript().segments[0].text, "  你好  ");
        // committed_text trims for joining, without mutating stored text.
        assert_eq!(m.transcript().committed_text(), "你好");
    }

    // --- Unicode / CJK / special characters ---

    #[test]
    fn traditional_chinese_and_mixed_script_survive_round_trip_through_the_model() {
        let mut m = manager();
        let text = "你好，我想測試語音辨識。OpenAI 的 API 延遲大約 300ms。";
        m.apply(TranscriptUpdate::final_("s1", 1, text));
        assert_eq!(m.transcript().segments[0].text, text);
        assert_eq!(m.transcript().committed_text(), text);
    }

    #[test]
    fn special_characters_that_break_naive_json_building_are_preserved() {
        let mut m = manager();
        let text = "引號\"與\\反斜線、換行\n、tab\t、emoji 🎙️、<>{}[]";
        m.apply(TranscriptUpdate::final_("s1", 1, text));
        assert_eq!(m.transcript().segments[0].text, text);
    }

    // --- Editing ---

    #[test]
    fn editing_replaces_text_and_finalizes_the_segment() {
        let mut m = manager();
        m.apply(TranscriptUpdate::partial("s1", 1, "錯字"));
        m.edit_segment("s1", "正確的字").unwrap();

        let seg = &m.transcript().segments[0];
        assert_eq!(seg.text, "正確的字");
        assert!(seg.is_final);

        // A late engine partial must not undo the human edit.
        m.apply(TranscriptUpdate::partial("s1", 99, "引擎想覆蓋"));
        assert_eq!(m.transcript().segments[0].text, "正確的字");
    }

    #[test]
    fn editing_an_unknown_segment_is_an_error() {
        let mut m = manager();
        let err = m.edit_segment("nope", "x").unwrap_err();
        assert!(matches!(
            err,
            VoiceApiError::NotFound {
                kind: "segment",
                ..
            }
        ));
    }

    #[test]
    fn editing_can_clear_a_segment_to_empty_text() {
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("s1", 1, "要刪掉的內容"));
        m.edit_segment("s1", "").unwrap();
        assert_eq!(m.transcript().segments[0].text, "");
        assert_eq!(m.transcript().committed_text(), "");
    }

    // --- Clear / remove ---

    #[test]
    fn clear_empties_everything_and_resets_numbering() {
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("s1", 1, "一"));
        m.apply(TranscriptUpdate::final_("s2", 1, "二"));
        m.record_error("boom", None);
        assert_eq!(m.transcript().segments.len(), 2);

        m.clear();

        let t = m.transcript();
        assert!(t.segments.is_empty());
        assert!(t.errors.is_empty());
        assert_eq!(t.session_id, "session-001", "session identity is preserved");

        // Numbering restarts, so IDs stay dense for a fresh recording.
        m.apply(TranscriptUpdate::final_("s3", 1, "三"));
        assert_eq!(m.transcript().segments[0].id, 1);
    }

    #[test]
    fn remove_segment_reindexes_and_renumbers_densely() {
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("a", 1, "A"));
        m.apply(TranscriptUpdate::final_("b", 1, "B"));
        m.apply(TranscriptUpdate::final_("c", 1, "C"));

        m.remove_segment("b").unwrap();

        let t = m.transcript();
        assert_eq!(
            t.segments
                .iter()
                .map(|s| s.segment_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "c"]
        );
        assert_eq!(t.segments[0].id, 1);
        assert_eq!(t.segments[1].id, 2);
        assert_eq!(t.committed_text(), "A C");
    }

    #[test]
    fn removing_then_reprocessing_the_same_segment_works() {
        // Reindexing must not leave a stale index entry pointing at the wrong
        // slot, which would silently corrupt a later update.
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("a", 1, "A"));
        m.apply(TranscriptUpdate::partial("b", 1, "B"));
        m.remove_segment("a").unwrap();

        m.apply(TranscriptUpdate::final_("b", 2, "B updated"));
        assert_eq!(m.transcript().segments.len(), 1);
        assert_eq!(m.transcript().segments[0].text, "B updated");
    }

    #[test]
    fn a_finalized_segment_stays_protected_after_a_neighbour_is_removed() {
        // Removal must not accidentally make an already-finalized segment
        // writable again: that would let a late partial duplicate its text.
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("a", 1, "A"));
        m.apply(TranscriptUpdate::final_("b", 1, "B"));
        m.remove_segment("a").unwrap();

        let outcome = m.apply(TranscriptUpdate::partial("b", 2, "B (late partial)"));
        assert_eq!(
            outcome,
            ApplyOutcome::IgnoredAfterFinal {
                segment_id: "b".into()
            }
        );
        assert_eq!(m.transcript().segments[0].text, "B");
        assert_eq!(m.transcript().segments.len(), 1);
    }

    // --- Errors and text projection ---

    #[test]
    fn recorded_errors_do_not_become_transcript_segments() {
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("s1", 1, "正常文字"));
        m.record_error("麥克風裝置中斷", Some("s2".into()));

        let t = m.transcript();
        assert_eq!(
            t.segments.len(),
            1,
            "an error must not create a text segment"
        );
        assert_eq!(t.errors.len(), 1);
        assert_eq!(t.errors[0].message, "麥克風裝置中斷");
        assert_eq!(t.errors[0].at, "2026-10-10T13:00:00Z");
        assert_eq!(t.errors[0].segment_id.as_deref(), Some("s2"));
    }

    #[test]
    fn committed_text_excludes_tentative_segments() {
        let mut m = manager();
        m.apply(TranscriptUpdate::final_("a", 1, "已確認"));
        m.apply(TranscriptUpdate::partial("b", 1, "還在辨識"));

        let t = m.transcript();
        assert_eq!(t.committed_text(), "已確認");
        // display_text is the superset a live UI shows.
        assert_eq!(t.display_text(), "已確認 還在辨識");
        assert!(t.has_final_text());
    }

    #[test]
    fn engine_metadata_is_optional_and_settable() {
        let mut m = manager();
        assert!(m.transcript().engine.is_none());
        m.set_engine(EngineInfo {
            id: "transcribe-cpp".into(),
            model_id: Some("handy-computer/Breeze-ASR-25-gguf".into()),
        });
        assert_eq!(
            m.transcript().engine.as_ref().unwrap().model_id.as_deref(),
            Some("handy-computer/Breeze-ASR-25-gguf")
        );
    }

    #[test]
    fn timestamps_are_captured_when_a_provider_supplies_them() {
        let mut m = manager();
        let mut update = TranscriptUpdate::final_("s1", 1, "有時間戳");
        update.start_ms = Some(1200);
        update.end_ms = Some(3400);
        m.apply(update);
        let seg = &m.transcript().segments[0];
        assert_eq!(seg.start_ms, Some(1200));
        assert_eq!(seg.end_ms, Some(3400));
    }

    #[test]
    fn a_later_update_without_timestamps_does_not_erase_earlier_ones() {
        let mut m = manager();
        let mut first = TranscriptUpdate::partial("s1", 1, "有時間戳");
        first.start_ms = Some(100);
        m.apply(first);
        m.apply(TranscriptUpdate::partial("s1", 2, "沒有時間戳"));
        assert_eq!(m.transcript().segments[0].start_ms, Some(100));
    }

    #[test]
    fn segment_kind_and_final_flag_are_consistent() {
        let mut m = manager();
        m.apply(TranscriptUpdate::partial("a", 1, "p"));
        m.apply(TranscriptUpdate::final_("b", 1, "f"));
        for seg in &m.transcript().segments {
            assert_eq!(
                seg.is_final,
                seg.kind == SegmentKind::Final,
                "final flag must mirror kind for {}",
                seg.segment_id
            );
        }
    }

    // --- GUI bridge: the frontend forwards recognition results as JSON ---

    #[test]
    fn a_transcript_update_survives_a_json_round_trip() {
        // The command layer accepts updates as JSON from the frontend, so this
        // wire form is part of the contract.
        let update = TranscriptUpdate {
            segment_id: "seg-1".into(),
            revision: 3,
            kind: SegmentKind::Final,
            text: "你好，我想測試語音辨識。".into(),
            start_ms: Some(100),
            end_ms: Some(2400),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("\"kind\":\"final\""));
        assert_eq!(
            serde_json::from_str::<TranscriptUpdate>(&json).unwrap(),
            update
        );
    }

    #[test]
    fn an_update_without_timestamps_deserializes_from_a_minimal_document() {
        let json = r#"{"segment_id":"s","revision":1,"kind":"partial","text":"hi"}"#;
        let update: TranscriptUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.segment_id, "s");
        assert_eq!(update.kind, SegmentKind::Partial);
        assert_eq!(update.start_ms, None);
    }

    #[test]
    fn apply_outcome_survives_a_json_round_trip() {
        let outcomes = vec![
            ApplyOutcome::Created {
                segment_id: "a".into(),
            },
            ApplyOutcome::Updated {
                segment_id: "a".into(),
                revision: 2,
            },
            ApplyOutcome::IgnoredStaleRevision {
                segment_id: "a".into(),
                incoming: 1,
                current: 2,
            },
            ApplyOutcome::IgnoredAfterFinal {
                segment_id: "a".into(),
            },
            ApplyOutcome::IgnoredEmpty {
                segment_id: "a".into(),
            },
        ];
        for outcome in outcomes {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<ApplyOutcome>(&json).unwrap(),
                outcome
            );
        }
    }
}
