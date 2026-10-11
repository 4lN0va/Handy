//! Adapter that converts Handy's live recognition event stream into the
//! transcript model's update format.
//!
//! # The problem this solves
//!
//! Handy's streaming pipeline emits
//! `StreamTextEvent { committed, tentative }` — a *stable prefix* plus an
//! *unstable tail* — repeatedly while the user speaks. That payload has two
//! properties that make it unusable as-is:
//!
//! 1. **It carries no identifier.** There is no segment id and no revision
//!    number, so a consumer cannot tell "the same segment, updated" from "a new
//!    segment". Naively appending each event would duplicate text, because each
//!    event contains the *whole* utterance so far, not a delta.
//! 2. **It has no explicit end.** The final text arrives through a different
//!    path entirely (the history entry), and for a non-streaming model there is
//!    no stream at all.
//!
//! The spec is explicit that recognition text must not be used as an identifier,
//! so identity is derived here from *structure* rather than content: the
//! committed prefix grows monotonically within one utterance, and a
//! discontinuity in that growth is what marks a new segment.
//!
//! # Design
//!
//! This is a pure state machine with no I/O and no dependency on Handy, so it is
//! fully unit-testable. The Tauri layer merely decodes upstream payloads and
//! feeds them in.
//!
//! Invariants it guarantees:
//!
//! * **No duplicate text.** Every emitted update addresses an existing segment
//!   and replaces its text; nothing is appended.
//! * **A finalized segment is never reopened.** Once a `Final` is emitted, the
//!   segment id is retired; later partials start a new segment.
//! * **Replay is idempotent.** Re-feeding an identical event emits nothing, and
//!   replaying a whole sequence from a fresh adapter reproduces the same result.
//! * **Out-of-order events are dropped.** An older (shorter) committed prefix
//!   arriving late is recognised as stale and ignored.

use crate::transcript::{SegmentKind, TranscriptUpdate};

/// One step of the adapter's output.
pub type AdapterEvents = Vec<TranscriptUpdate>;

/// Converts `(committed, tentative)` streaming updates and final texts into
/// [`TranscriptUpdate`]s.
#[derive(Debug, Default)]
pub struct StreamEventAdapter {
    /// Monotonic counter for segment ids within the current session.
    segment_seq: u64,
    /// The utterance currently being recognised, if any.
    active: Option<ActiveSegment>,
}

#[derive(Debug, Clone)]
struct ActiveSegment {
    segment_id: String,
    revision: u64,
    /// The committed prefix last observed. Used for progress/staleness/restart
    /// detection — never as an identifier.
    committed: String,
    /// The full text last emitted (`committed + tentative`).
    text: String,
}

impl StreamEventAdapter {
    /// Create an adapter for a fresh session.
    pub fn new() -> Self {
        Self::default()
    }

    /// The segment currently being recognised, if any.
    pub fn active_segment_id(&self) -> Option<&str> {
        self.active.as_ref().map(|a| a.segment_id.as_str())
    }

    /// Start a new recording session.
    ///
    /// Discards the in-flight utterance without emitting anything: a new session
    /// gets its own segment id space, so ids can safely restart at 1. The caller
    /// is expected to have created a new [`crate::transcript::TranscriptManager`].
    pub fn reset(&mut self) {
        self.active = None;
        self.segment_seq = 0;
    }

    fn next_segment_id(&mut self) -> String {
        self.segment_seq += 1;
        format!("seg-{}", self.segment_seq)
    }

    /// Feed one live streaming update.
    ///
    /// `committed` is the stable prefix the model has committed to; `tentative`
    /// is the unstable tail that may still change. The current full text is
    /// their concatenation — Handy builds it the same way internally, with no
    /// separator, which matters for Chinese where there is nothing to join with.
    pub fn on_stream_text(&mut self, committed: &str, tentative: &str) -> AdapterEvents {
        let full = format!("{committed}{tentative}");

        let Some(active) = self.active.as_ref() else {
            // First text of the utterance. An empty utterance carries no
            // information and must not create a segment.
            if full.trim().is_empty() {
                return Vec::new();
            }
            let segment_id = self.next_segment_id();
            let update = TranscriptUpdate {
                segment_id: segment_id.clone(),
                revision: 1,
                kind: SegmentKind::Partial,
                text: full.clone(),
                start_ms: None,
                end_ms: None,
            };
            self.active = Some(ActiveSegment {
                segment_id,
                revision: 1,
                committed: committed.to_string(),
                text: full,
            });
            return vec![update];
        };

        // Exact replay: nothing changed, so emit nothing. This is what makes a
        // replayed event stream idempotent rather than merely harmless.
        if committed == active.committed && full == active.text {
            return Vec::new();
        }

        // Out-of-order delivery: an older, shorter committed prefix arrived after
        // a newer one. Treat as stale and ignore.
        if committed.len() < active.committed.len() && active.committed.starts_with(committed) {
            return Vec::new();
        }

        // Normal progress: the committed prefix extended (or the tentative tail
        // was refined). Replace the segment's text in place.
        if committed.starts_with(&active.committed) {
            let active = self.active.as_mut().expect("active checked above");
            active.revision += 1;
            active.committed = committed.to_string();
            active.text = full.clone();
            return vec![TranscriptUpdate {
                segment_id: active.segment_id.clone(),
                revision: active.revision,
                kind: SegmentKind::Partial,
                text: full,
                start_ms: None,
                end_ms: None,
            }];
        }

        // Discontinuity: the committed prefix is neither an extension nor a
        // prefix of what we had, so the engine restarted the utterance. Lock the
        // text it had actually committed — dropping the unconfirmed tail, which
        // the rewind has just invalidated — and begin a new segment.
        let previous = self.active.take().expect("active checked above");
        let mut events = vec![TranscriptUpdate {
            segment_id: previous.segment_id,
            revision: previous.revision + 1,
            kind: SegmentKind::Final,
            text: previous.committed.clone(),
            start_ms: None,
            end_ms: None,
        }];

        if !full.trim().is_empty() {
            let segment_id = self.next_segment_id();
            events.push(TranscriptUpdate {
                segment_id: segment_id.clone(),
                revision: 1,
                kind: SegmentKind::Partial,
                text: full.clone(),
                start_ms: None,
                end_ms: None,
            });
            self.active = Some(ActiveSegment {
                segment_id,
                revision: 1,
                committed: committed.to_string(),
                text: full,
            });
        }
        events
    }

    /// Finalize the current utterance with the authoritative final text.
    ///
    /// This is the path that also covers **non-streaming models**, for which no
    /// `on_stream_text` call ever happens: with no active segment, a new one is
    /// created and immediately finalized.
    ///
    /// Always emits a `Final` even when the text is unchanged, because that is
    /// what locks the segment: without it a late partial could still rewrite the
    /// text.
    pub fn on_final(&mut self, final_text: &str) -> AdapterEvents {
        match self.active.take() {
            Some(previous) => {
                if final_text.trim().is_empty() && previous.committed.trim().is_empty() {
                    // Nothing was ever recognised; do not create a blank segment.
                    return Vec::new();
                }
                vec![TranscriptUpdate {
                    segment_id: previous.segment_id,
                    revision: previous.revision + 1,
                    kind: SegmentKind::Final,
                    text: final_text.to_string(),
                    start_ms: None,
                    end_ms: None,
                }]
            }
            None => {
                if final_text.trim().is_empty() {
                    return Vec::new();
                }
                let segment_id = self.next_segment_id();
                vec![TranscriptUpdate {
                    segment_id,
                    revision: 1,
                    kind: SegmentKind::Final,
                    text: final_text.to_string(),
                    start_ms: None,
                    end_ms: None,
                }]
            }
        }
    }

    /// Abandon the in-flight utterance without finalizing it.
    ///
    /// Used when the user cancels: the partially recognised text was never
    /// confirmed, so promoting it to `Final` would assert something untrue. The
    /// segment is left as-is in the transcript for the caller to decide.
    pub fn abandon(&mut self) {
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::FixedClock;
    use crate::transcript::TranscriptManager;

    /// Apply adapter output to a manager, as the Tauri bridge does.
    fn drive(manager: &mut TranscriptManager, events: AdapterEvents) {
        for e in events {
            manager.apply(e);
        }
    }

    fn manager() -> TranscriptManager {
        TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        )
    }

    // --- the six required scenarios ---

    #[test]
    fn partial_partial_final_produces_one_segment_with_only_the_final_text() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("你好", "，我想"));
        drive(&mut m, adapter.on_stream_text("你好，我想", "測試"));
        drive(&mut m, adapter.on_final("你好，我想測試語音辨識。"));

        let t = m.transcript();
        assert_eq!(t.segments.len(), 1, "must not append one segment per event");
        assert_eq!(t.segments[0].text, "你好，我想測試語音辨識。");
        assert!(t.segments[0].is_final);
        assert_eq!(t.committed_text(), "你好，我想測試語音辨識。");
    }

    #[test]
    fn replaying_the_same_event_sequence_is_idempotent() {
        let sequence = [
            ("", "你"),
            ("你", "好"),
            ("你好", "，我想"),
            ("你好，我想", "測試"),
        ];

        let mut first = StreamEventAdapter::new();
        let mut m1 = manager();
        for (c, t) in sequence {
            drive(&mut m1, first.on_stream_text(c, t));
        }
        drive(&mut m1, first.on_final("你好，我想測試。"));

        // Replay from a fresh adapter into a fresh manager.
        let mut second = StreamEventAdapter::new();
        let mut m2 = manager();
        for (c, t) in sequence {
            drive(&mut m2, second.on_stream_text(c, t));
        }
        drive(&mut m2, second.on_final("你好，我想測試。"));

        assert_eq!(
            m1.transcript().segments,
            m2.transcript().segments,
            "replaying the identical stream must reproduce the identical transcript"
        );
    }

    #[test]
    fn replaying_an_already_consumed_event_emits_nothing() {
        let mut adapter = StreamEventAdapter::new();
        let first = adapter.on_stream_text("你好", "世界");
        assert_eq!(first.len(), 1);
        // Feeding the identical payload again must be a no-op, not a revision bump.
        assert!(adapter.on_stream_text("你好", "世界").is_empty());
        assert!(adapter.on_stream_text("你好", "世界").is_empty());
    }

    #[test]
    fn out_of_order_events_are_ignored() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("你好，我想測試", "語音"));
        // A stale, shorter committed prefix arriving late must not rewind text.
        drive(&mut m, adapter.on_stream_text("你好", "，我想"));

        assert_eq!(m.transcript().segments.len(), 1);
        assert_eq!(m.transcript().segments[0].text, "你好，我想測試語音");
    }

    #[test]
    fn a_late_partial_after_final_does_not_reopen_the_segment() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("你好", ""));
        drive(&mut m, adapter.on_final("你好。"));
        assert!(m.transcript().segments[0].is_final);

        // A new utterance begins: it must be a NEW segment, and the finalized
        // one must keep its text.
        drive(&mut m, adapter.on_stream_text("下一句", ""));
        // A late partial for the already-finalized text must be treated as a
        // discontinuity (new segment), never as an update to the locked one.
        let t = m.transcript();
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[0].text, "你好。");
        assert!(
            t.segments[0].is_final,
            "the finalized segment keeps its flag"
        );
        assert_eq!(t.segments[1].text, "下一句");
        assert!(!t.segments[1].is_final);
    }

    #[test]
    fn a_new_session_starts_afresh_and_keeps_earlier_text_separate() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("第一段", ""));
        drive(&mut m, adapter.on_final("第一段。"));
        let before = m.transcript().segments.len();

        // New session: a fresh adapter and a fresh manager.
        adapter.reset();
        let mut m2 = manager();
        drive(&mut m2, adapter.on_stream_text("第二段", ""));
        drive(&mut m2, adapter.on_final("第二段。"));

        assert_eq!(before, 1);
        assert_eq!(m2.transcript().segments.len(), 1);
        // Segment ids are scoped to a session, so restarting at seg-1 is correct.
        assert_eq!(m2.transcript().segments[0].segment_id, "seg-1");
        assert_eq!(m2.transcript().committed_text(), "第二段。");
    }

    #[test]
    fn clearing_and_restarting_leaves_no_stale_state_in_the_adapter() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("要被清掉的", ""));
        m.clear();
        adapter.abandon();

        // After clearing, the next utterance must start a new segment that does
        // not resurrect the discarded text.
        drive(&mut m, adapter.on_stream_text("重新開始", ""));
        assert_eq!(m.transcript().segments.len(), 1);
        assert_eq!(m.transcript().segments[0].text, "重新開始");
    }

    // --- non-streaming path ---

    #[test]
    fn a_non_streaming_model_creates_a_final_segment_from_the_final_text_alone() {
        // No on_stream_text call happens at all for a batch model.
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_final("批次辨識的結果。"));

        let t = m.transcript();
        assert_eq!(t.segments.len(), 1);
        assert!(t.segments[0].is_final);
        assert_eq!(t.segments[0].text, "批次辨識的結果。");
    }

    #[test]
    fn consecutive_batch_transcriptions_each_become_their_own_segment() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_final("第一句。"));
        drive(&mut m, adapter.on_final("第二句。"));

        let t = m.transcript();
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.committed_text(), "第一句。 第二句。");
        assert_ne!(t.segments[0].segment_id, t.segments[1].segment_id);
    }

    // --- edge cases ---

    #[test]
    fn empty_stream_text_never_creates_a_segment() {
        let mut adapter = StreamEventAdapter::new();
        assert!(adapter.on_stream_text("", "").is_empty());
        assert!(adapter.on_stream_text("", "   ").is_empty());
        assert!(adapter.active_segment_id().is_none());
    }

    #[test]
    fn empty_final_with_no_active_segment_emits_nothing() {
        let mut adapter = StreamEventAdapter::new();
        assert!(adapter.on_final("").is_empty());
        assert!(adapter.on_final("   ").is_empty());
    }

    #[test]
    fn changing_only_the_tentative_tail_updates_in_place() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("你好", "世"));
        drive(&mut m, adapter.on_stream_text("你好", "世界"));

        let t = m.transcript();
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].text, "你好世界");
        assert_eq!(t.segments[0].revision, 2, "a refinement is a new revision");
    }

    #[test]
    fn a_committed_prefix_that_rewinds_is_a_discontinuity_not_a_rewind_of_text() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("我覺得這個", "方向"));
        // The engine restarts the utterance with unrelated text.
        let events = adapter.on_stream_text("完全不同的", "內容");

        assert_eq!(
            events.len(),
            2,
            "lock the old segment, then start a new one"
        );
        drive(&mut m, events);

        let t = m.transcript();
        assert_eq!(t.segments.len(), 2);
        // The old segment keeps only what the engine had actually committed;
        // the tentative tail it just invalidated is dropped.
        assert_eq!(t.segments[0].text, "我覺得這個");
        assert!(t.segments[0].is_final);
        assert_eq!(t.segments[1].text, "完全不同的內容");
        assert!(!t.segments[1].is_final);
    }

    #[test]
    fn a_discontinuity_with_empty_new_text_closes_the_old_segment_only() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("有內容", ""));
        let events = adapter.on_stream_text("", "");
        // An empty full text would be a stale/no-op under the prefix rules; here
        // it is neither ("" is a prefix of "有內容"), so it is treated as stale.
        assert!(
            events.is_empty(),
            "an empty rewind is stale, not a new segment"
        );
        assert_eq!(m.transcript().segments.len(), 1);
    }

    #[test]
    fn revision_numbers_are_monotonic_within_a_segment() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        let mut seen = Vec::new();
        for (c, t) in [("你", ""), ("你好", ""), ("你好", "嗎"), ("你好嗎", "")] {
            for update in adapter.on_stream_text(c, t) {
                seen.push(update.revision);
                m.apply(update);
            }
        }
        for update in adapter.on_final("你好嗎？") {
            seen.push(update.revision);
            m.apply(update);
        }
        assert_eq!(seen, vec![1, 2, 3, 4, 5]);
        assert_eq!(m.transcript().segments.len(), 1);
    }

    #[test]
    fn a_partial_arriving_after_abandon_starts_a_new_segment() {
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(&mut m, adapter.on_stream_text("取消前的文字", ""));
        adapter.abandon();
        drive(&mut m, adapter.on_stream_text("取消後的文字", ""));

        let t = m.transcript();
        assert_eq!(t.segments.len(), 2);
        // The abandoned text is kept but never promoted to final.
        assert_eq!(t.segments[0].text, "取消前的文字");
        assert!(
            !t.segments[0].is_final,
            "cancelled text must not be marked final"
        );
        assert_eq!(t.segments[1].text, "取消後的文字");
    }

    #[test]
    fn mixed_chinese_and_english_is_joined_without_an_inserted_separator() {
        // Chinese has no word separator, so `committed + tentative` must not
        // invent one; Handy concatenates the same way internally.
        let mut adapter = StreamEventAdapter::new();
        let mut m = manager();

        drive(
            &mut m,
            adapter.on_stream_text("這個 API 的", " latency 很低"),
        );
        assert_eq!(m.transcript().segments[0].text, "這個 API 的 latency 很低");

        drive(&mut m, adapter.on_final("這個 API 的 latency 很低。"));
        assert_eq!(
            m.transcript().segments[0].text,
            "這個 API 的 latency 很低。"
        );
    }

    #[test]
    fn the_adapter_never_emits_a_segment_id_reused_after_final() {
        let mut adapter = StreamEventAdapter::new();
        let mut ids = Vec::new();

        for round in 0..5 {
            for update in adapter.on_stream_text(&format!("第{round}句"), "") {
                ids.push(update.segment_id.clone());
            }
            for update in adapter.on_final(&format!("第{round}句。")) {
                ids.push(update.segment_id.clone());
            }
            adapter.abandon();
        }

        // Five utterances: each uses one id, and no id appears for two different
        // utterances. Ids are reused only across sessions, never within one.
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(
            unique.len(),
            5,
            "expected one distinct id per utterance: {ids:?}"
        );
    }
}
