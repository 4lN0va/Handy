# Automatic Continuous Transcription + Verification-Tool Integration

## What was added

**Continuous mode.** The user starts once; the microphone keeps recording and the
transcript is cut into segments at natural pauses. No keypress per sentence.

**Integration bridge.** Heartbeat and per-segment transcript events are pushed to
the local verification tool (`handy-integration-verification-tool`).

## How segmentation works

It reuses two existing mechanisms instead of building a parallel audio path:

1. **Level observation.** The recorder already computes spectrum-level buckets for
   the on-screen meter. `managers::speech_activity` observes those buckets
   passively and records when the level was last above the noise floor. The audio
   path is unchanged — it cannot affect what gets recorded.
2. **The ordinary record → stop → transcribe path.** `stop_recording` returns
   captured samples exactly as it does for a manual recording, and they go to the
   same `TranscriptionManager::transcribe`.

A segment closes when the level has stayed below the threshold for `pause_ms`
(default 800 ms) *after* speech was heard. Because the recorder's VAD holds a
post-speech hangover tail, closing on sustained silence means the tail has already
been flushed and the end of the sentence is not clipped.

A `max_segment_ms` cap (default 30 s) forces a cut for a speaker who never pauses,
so a monologue still becomes several segments.

**The next recording starts before transcription runs.** Transcription takes
hundreds of milliseconds, and audio captured during it belongs to the next
segment; restarting first means it is captured rather than dropped.

## Identity and idempotency

Each segment goes through the **existing** `TranscriptManager`, so segment ids,
revision ordering and the GUI event are identical to the manual flow. The bridge
is fed from `VoiceApiManager::apply_events` — the single place transcript updates
are produced — so there is no second pipeline to keep in sync.

| Requirement | How it holds |
|---|---|
| partial updates reuse the segment id | one `segment_id` per segment; the adapter advances `revision` |
| final is marked correctly | `SegmentKind::Final` → `"final": true` |
| replayed final does not duplicate | adapter suppresses it; the bridge also drops an exact repeat |
| a late partial cannot overwrite a final | adapter treats a straggler into just-finalized text as stale |
| retries do not duplicate | a retry re-sends the *same* event with the same identity; verified no duplicates |
| sessions and segments are distinguishable | `session_id` per session, `segment_id` unique within it |

## Network behaviour

* One dedicated worker thread. Callers only push onto an unbounded channel, so
  audio, ASR and the UI are never blocked by the network.
* The `reqwest::blocking` client is built **on that worker thread**. Building it
  on a Tokio worker breaks it with "Failed to communicate successful startup" —
  the defect Phase 3 found in the upload path.
* 2 s timeout, bounded retries, and 4xx is not retried (a contract mismatch cannot
  be fixed by trying again).
* Failure is surfaced: counters and the last error are exposed via `bridge_status`
  and shown in the UI. Nothing is ever reported as sent when it was not.
* Endpoint validation refuses clear-text HTTP to anything but loopback.
* No credential exists in this path, and request bodies are never logged.

## Settings

| Setting | Default | Meaning |
|---|---|---|
| `pause_ms` | 800 | silence that closes a segment |
| `min_segment_ms` | 400 | ignore blips shorter than this |
| `max_segment_ms` | 30000 | force a cut for a speaker who never pauses |
| `silence_level` | 0.02 | level below which audio counts as silence |
| bridge `enabled` | **false** | nothing is sent unless explicitly turned on |
| bridge `endpoint` | `http://127.0.0.1:8765` | loopback or https only |
| bridge `send_partials` | false | forward partials as well as finals |

Configuration persists to `continuous_config.json` and `integration_bridge.json`
in the app data directory.

## Verified

| Item | Result |
|---|---|
| `cargo test --lib` | **323 passed, 0 failed** (27 new) |
| Heartbeat reaches the tool | **PASS** — `heartbeatsReceived: 11`, `failed: 0` |
| Transcript events reach the tool | **PASS** — correct schema, Traditional Chinese intact |
| partial/final share a segment id | **PASS** — `seg-1` partial then `seg-1` final |
| No duplicate segments | **PASS** — every (session, segment, final) appears once |
| `/api/status` reflects state | **PASS** — `app_version`, `recording`, `status`, `client` |

**Bug found and fixed by this test:** the first heartbeat implementation sent
`"status": ""` when no activity had been set. The tool rejects a provided-but-empty
status with **422**, so every heartbeat failed while transcripts succeeded —
which reads like a transport problem. An empty status is now omitted.

## Not verified

* **No microphone run was performed in this session.** Segmentation timing
  (`pause_ms`, `silence_level`) is therefore tuned by reasoning, not measured, and
  should be adjusted against a real voice.
* Platform other than Windows x86_64.
* The tool's `/bridge` dashboard page was not opened; verification used
  `GET /api/status` and the app's own counters.
* `check_handy.ps1` (process probe) was not started, so `handy_process` in
  `/api/status` is `null`.

## Known open issue (unchanged from Phase 3)

Local duplicate-upload suppression in the Voice API path still never fires,
because a fresh `ApiClient` is built per call. This is **unrelated** to the
continuous/transcript event path, which does its own deduplication and was
verified free of duplicates. The fix (cache the client, invalidate on config or
key change) remains diagnosed but unimplemented.
