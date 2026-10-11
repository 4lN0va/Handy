# Phase 2 — Voice API Integration Plan (execution record)

This document records the actual state of the integration as it was performed: the
real file paths, the public interfaces that were reused, the gaps that had to be
bridged, the build-environment results, and the order of work.

It is a working record, not the deliverable. The deliverable is the code plus
[`PHASE2_TEST_REPORT.md`](PHASE2_TEST_REPORT.md).

---

## 1. Repository and environment (re-verified, not assumed)

Verified at the start of this phase rather than carried over from Phase 1.

### 1.1 Repository

| Item | Value |
|---|---|
| Working directory | `C:\Users\Nova\Project Brief\handy-fork` |
| Branch | `feature/voice-api-mvp` |
| HEAD at phase start | `bca7654` (`chore(tools): stop tracking the local crates.io audit cache`) |
| Upstream baseline tag | `upstream-baseline-v0.9.8` → `14f6f0d3` (Handy v0.9.8) |
| `main` | `48a3d8e` = `origin/main`, untouched |
| Uncommitted changes at start | one untracked directory, `crates/handy-voice-api/target/` |
| Commits ahead of upstream | 3 |

The untracked `target/` directory was a local build artifact and was added to
`.gitignore` rather than deleted.

### 1.2 The environment had changed

Phase 1 concluded that the application could not be built. **That conclusion no
longer holds**, so it was re-tested rather than repeated.

| Component | Phase 1 | Phase 2 (verified) |
|---|---|---|
| Visual Studio Build Tools | absent | **VS Build Tools 2022 17.14.37710, `isComplete: true`**, at `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` |
| MSVC C++ x64 toolset | absent | **14.44.35207** — verified by querying `vswhere -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64`, not merely by finding `vswhere.exe` |
| `link.exe` / `cl.exe` | not found | present, but **only inside the developer environment** (`vcvars64.bat`) |
| Windows SDK | absent | **10.0.26100.0**, with `Include`, `Lib` and `bin` |
| CMake | absent | 4.4.4 (winget) |
| Vulkan SDK | absent | **1.4.363.0**, installed during this phase |
| ONNX Runtime | unresolvable on GNU | resolved by `ort-sys` for the MSVC target |
| Ninja, Bun | absent | present in the portable toolchain from Phase 1 |

Evidence that the toolchain genuinely works, rather than merely being present: a
throwaway crate using `serde` + `serde_json` (so build scripts and proc-macros are
exercised) compiled, linked, ran, and round-tripped Traditional Chinese JSON.

Environment helper: [`tools/msvc_env.ps1`](../../tools/msvc_env.ps1).

---

## 2. Existing implementation inventory

### 2.1 `handy-voice-api` public surface (reused, not rewritten)

| Module | Public API used by the integration |
|---|---|
| `transcript` | `TranscriptManager`, `TranscriptUpdate`, `ApplyOutcome`, `SegmentKind`, `Transcript` |
| `stream_adapter` | `StreamEventAdapter` — **added in this phase** |
| `schema` | `validate_transcript` / `validate_transcript_str`, `TRANSCRIPT_SCHEMA_V1` |
| `export` | `to_json`, `to_json_filtered`, `import_from_file`, `export_to_file`, `ExportFilter`, `assert_no_secret_material` |
| `api` | `ApiConfig`, `ApiClient`, `AuthScheme`, `Transport`, `ReqwestTransport`, `RetryPolicy`, `Idempotency`, `RequestFormat`, `UploadOutcome`, `ConnectionTestResult` |
| `secrets` | `SecretString`, `SecretStore`, `OsSecretStore`, `account_name`, `SECRET_SERVICE` |
| `time` | `SystemClock`, `Clock` |

### 2.2 Handy's recognition event surface (the integration points found)

All three are already emitted; **no upstream Rust file needed to be modified.**

| Event name | Payload | Where emitted |
|---|---|---|
| `stream-text-event` | `managers::transcription::StreamTextEvent { committed, tentative }` | `src-tauri/src/managers/transcription.rs:1529` |
| `stream-phase-event` | `managers::transcription::StreamPhaseEvent { phase, kind }` | same module |
| `history-update-payload` | `managers::history::HistoryUpdatePayload::Added { entry: HistoryEntry }` | `src-tauri/src/managers/history.rs` |
| `transcription-error` | `String` | `src-tauri/src/actions.rs` |

Event name strings were taken from the generated `src/bindings.ts` rather than
guessed.

### 2.3 The Tauri command conventions reused

* `#[tauri::command]` + `#[specta::specta]`, registered in the
  `collect_commands![...]` list in `src-tauri/src/lib.rs`.
* Handlers take `State<'_, Arc<Manager>>` and return `Result<T, String>`.
* `tauri_specta` regenerates `src/bindings.ts` from a debug build, which is how
  the frontend gets typed `commands.*` accessors.
* Front end: settings sections live in `SECTIONS_CONFIG` in
  `src/components/Sidebar.tsx`, and UI primitives are in `src/components/ui/`.

---

## 3. Integration gaps and how each was closed

| # | Gap | Decision |
|---|---|---|
| 1 | Upstream streaming events carry **no segment id and no revision** — each event contains the *whole* utterance so far, so appending would duplicate text | Added `StreamEventAdapter` as a pure state machine deriving identity from the *shape* of the `committed` prefix (progress / stale / discontinuity), never from the text itself. Spec forbids using text as an id. |
| 2 | There is no "final text" event on the streaming path | Subscribed to `history-update-payload`; `HistoryEntry` carries `transcription_text` and `post_processed_text`. Prefers the post-processed text, since that is what the user saw and what was pasted. |
| 3 | Non-streaming models emit **no** `stream-text-event` at all | `on_final` with no active segment creates and immediately finalizes a segment, so batch models still produce a transcript. |
| 4 | A cancelled utterance must not be recorded as confirmed | `StreamEventAdapter::abandon()` on `stream-phase-event` `Working`; the segment stays but is never promoted to `Final`. |
| 5 | A fast recognition stream would re-render the GUI per token | Partial transcript events are coalesced to one per 80 ms; final updates always bypass the throttle. |
| 6 | Double-clicking Upload could fan out into unbounded concurrent requests | `AtomicBool` single-flight permit using `compare_exchange`, so two threads racing cannot both win; the loser gets a clear error. |
| 7 | The spec asks to query operation status | `voice_api_is_busy`, `voice_api_last_operation`, `voice_api_upload_preview`. |
| 8 | The user must be told what is about to leave the machine | `voice_api_upload_preview` returns session id, language, counts and a 200-character preview; the Transcript screen renders it above the button. |

---

## 4. Build-environment problems found and fixed

Two of these were **real defects in the build environment**, not in the
application, and both are documented in `tools/msvc_env.ps1` because they would
recur on any non-English Windows.

### 4.1 Ninja could not parse `cl.exe` output (locale)

```
ninja: build stopped: subcommand failed.
```
with thousands of `注意: 包含檔案:` lines. CMake+Ninja drives `cl.exe` with
`/showIncludes` and parses the resulting prefix; a Chinese-locale toolchain
localises it and Ninja stops recognising it. **Fix: `VSLANG=1033`.**

### 4.2 A UTF-8 source file was decoded as GBK (locale)

After fixing the above, the real first error surfaced:

```
transcribe-cpp-sys-0.3.1\src\arch\canary\model.cpp(670): error C2065: 'sp_space_id': undeclared identifier
transcribe-cpp-sys-0.3.1\src\arch\canary\model.cpp(673): error C3927: '->': trailing return type ...
```

The code at those lines is valid. The cause is at line 667: the file is UTF-8
**without a BOM** and contains a literal `▁` (U+2581). Without `/utf-8`, MSVC
decodes source using the system ANSI codepage; under GBK/936 the UTF-8 bytes of
that character pair with following bytes, the comment swallows real code, and the
compiler emits a cascade of nonsense. Upstream CI builds on English-locale
runners (CP1252), where the same three bytes decode as three separate characters
and consume nothing — which is why the bug is invisible there.

**Fix: `-DCMAKE_C_FLAGS=/utf-8 -DCMAKE_CXX_FLAGS=/utf-8` via `CMAKE_ARGS`.**

### 4.3 Missing Vulkan SDK

`ggml-vulkan` needs `glslc`:
```
CMake Error: Could NOT find Vulkan (missing: Vulkan_LIBRARY Vulkan_INCLUDE_DIR glslc)
```
Fixed by `winget install KhronosGroup.VulkanSDK` (1.4.363.0).

### 4.4 Stale CMake cache when changing generator

Switching from the default MSYS/Makefiles generator to Ninja required clearing
`%LOCALAPPDATA%\tcs\<hash>\build`, otherwise CMake refuses with *"generator does
not match the generator used previously"*.

---

## 5. Work order actually followed

1. Re-verify repository state, toolchain, and the VS/SDK/Vulkan facts.
2. Prove the MSVC toolchain links (throwaway crate), before touching the app.
3. Inventory the upstream event surface and the existing crate API.
4. Write `StreamEventAdapter` with tests, in the standalone crate — independently
   verifiable without the GUI toolchain.
5. Add the Rust bridge (`app.listen` on the three upstream events) inside
   `commands/voice_api.rs`; **no upstream file edited**.
6. Add the remaining commands: concurrency guard, busy/status, upload preview.
7. Build the application with MSVC; fix the two locale defects, then the two type
   errors the compiler found in the new command layer.
8. Launch the built application and confirm it runs.
9. Regenerate `src/bindings.ts`, add the two React settings sections, and
   type-check them against the generated bindings.
10. Rebuild and relaunch with the new UI.
11. Update documentation and the test report.

---

## 6. Deliberate limitations

Recorded rather than hidden.

| Limitation | Reason |
|---|---|
| **Cancellation is not wired to a UI control.** | The upload path is bounded by the configured timeout and by refusing concurrent requests; the retry loop itself has no cancel token. Implementing one requires threading a token through `ApiClient`, which was out of scope for this phase. |
| **Command payloads cross the boundary as JSON strings.** | `tauri-specta` requires every type to implement `specta::Type`; the crate deliberately does not depend on a UI framework. The frontend therefore declares matching interfaces by hand. Adding feature-gated `specta` derives is the follow-up. |
| **Only the English locale has the new strings.** | i18next falls back to `en` for the other 25 locales, so the screens are usable everywhere; real translations are a community task. |
| **Auto-upload is not implemented.** | The spec requires de-duplication, privacy disclosure and failure recovery before auto-upload may default on; none of that was in scope, so there is no auto-upload at all rather than a half-built one. |
| **The script editor in the Voice API screen accepts `Name: Value` per line**, not structured rows. | Functional and unambiguous; a richer editor is a UI-only change. |
