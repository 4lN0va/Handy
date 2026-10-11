# Phase 3 — Test Report

Every result below was produced by a command run during this phase. Nothing is
carried over from Phase 2, including the things Phase 2 got right.

Status key: **PASS** · **FAIL** · **BLOCKED** · **NOT RUN** · **PARTIAL**

Environment: Windows 10.0.19045 (x86_64), VS Build Tools 2022 17.14.37710 (MSVC
14.44.35207), Windows SDK 10.0.26100.0, Vulkan SDK 1.4.363.0, CMake 4.4.4, Ninja
1.13.2, Bun 1.4.3, Rust 1.98.0.

---

## 0. Headline

Phase 3 was worth running. **Six defects were found, five of them by operating the
real application rather than by testing the code** — including three that made
features completely non-functional while every unit test passed.

| # | Defect | Severity | Status |
|---|---|---|---|
| 1 | Managed state double-wrapped in `Arc`, so **every `voice_api` command failed at runtime** | Critical | **fixed, verified** |
| 2 | `keyring` compiled with no backend → API key **silently stored nothing** | Critical (security) | **fixed, verified** |
| 3 | `reqwest::blocking` client built on the async runtime → **upload sent nothing** | Critical | **fixed, verified** |
| 4 | Single-flight permit leaked on early return → **uploads permanently disabled until restart** | High | **fixed, verified** |
| 5 | Late partial after final created a **phantom segment** | Medium | **fixed, verified** |
| 6 | Replaying a final **duplicated the transcript** | Medium | **fixed, verified** |
| 7 | Local duplicate-upload suppression never fires (client rebuilt per call) | Medium | **open — documented** |

Also fixed: two scripts referenced by the docs were missing from the repository,
and `fetch_model.ps1` misread the catalogue.

---

## 1. Starting state (re-verified)

```
$ git status --short --branch
## feature/voice-api-mvp                 <- clean, no uncommitted work

HEAD 20f92ed  feat(voice-api): wire live recognition into the transcript and build the app
4 commits ahead of upstream-baseline-v0.9.8;  main = 48a3d8e = origin/main, untouched
Handy process at start: none
```

Toolchain re-probed, not assumed: Rust 1.98.0 (msvc default, gnu also installed);
VS Build Tools with `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` present
(queried with `vswhere -requires`, not inferred from the file existing);
Windows SDK 10.0.26100.0; CMake 4.4.4; Ninja 1.13.2; Bun 1.4.3; Vulkan SDK
1.4.363.0 with `glslc`; `ORT_LIB_LOCATION` unset (`ort-sys` 2.0.0-rc.12 supplies
its own binaries).

`cl.exe`/`link.exe`/`ninja`/`bun`/`glslc` are **not** on the default `PATH`; the
environment scripts put them there. **PASS**

## 2. Regression suites

| Suite | Command | Result |
|---|---|---|
| Application crate | `cargo test --manifest-path src-tauri/Cargo.toml --lib` | **295 passed, 0 failed, 16 ignored** |
| `handy-voice-api` | `cargo +…-gnu test --all-features` | **182 unit + 11 HTTP + 1 doctest** |
| OS credential store | `cargo … test --features keyring --test os_keyring -- --ignored` | **8 passed** |
| Formatting / lints | `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings` | clean |
| Front end | `bun run build` | 2129 modules, `tsc` clean |

Test counts moved during this phase as fixes landed (178 → 182 in the crate,
294 → 295 in the app): the new tests lock in the defects found below.

## 3. Defect 1 — every command failed at runtime (CRITICAL, fixed)

**How it was found:** invoking `voice_api_get_transcript` from the running app.

```
state not managed for field `manager` on command `voice_api_get_transcript`.
You must call `.manage()` before using this command
```

All five state-needing `voice_api_*` commands failed, while an upstream
state-needing command (`get_available_microphones`) worked — so `manage()` itself
was fine.

**Cause:** `voice_api::init` already returns `Arc<VoiceApiManager>`, but `lib.rs`
wrapped it in `Arc::new(...)` again, managing `Arc<Arc<VoiceApiManager>>`. Tauri
resolves managed state by `TypeId`, so the lookup missed. It compiles cleanly, and
the 12 bridge tests could not catch it because they exercise the manager directly
rather than through Tauri state.

**Impact:** the transcript, export, upload and settings commands were all dead. The
Transcript screen rendered "No speech recognized yet" — which looks like an empty
state, not an error.

**Fix:** drop the extra `Arc::new`, plus an explicit type annotation so the double
wrap becomes a compile error rather than a runtime one.

**Verified after fix:**
```
voice_api_get_config      OK {"base_url":"","endpoint":"transcripts",...
voice_api_has_api_key     OK false
voice_api_secret_storage  OK operating system credential store
```

## 4. Defect 2 — the API key was silently not stored (CRITICAL, fixed)

`Cargo.toml` declared `keyring = { version = "3", optional = true }` with no
features. `keyring` v3 has **no default backend**; each platform backend is
opt-in. Built that way it compiles and `set_password` returns `Ok(())` while
storing nothing.

**Evidence, before the fix** — the real Credential Manager tests failed 5 of 8,
with `set` reporting success and `get` returning `None`:

```
test a_secret_written_to_the_os_store_can_be_read_back ... FAILED
test a_secret_survives_a_fresh_store_handle ... FAILED
test updating_a_secret_replaces_the_stored_value ... FAILED
test deleting_a_secret_removes_it_and_is_idempotent ... FAILED
test entries_are_isolated_by_account_name ... FAILED
test result: FAILED. 3 passed; 5 failed
```

The in-memory mock store had passed all along and could not have caught this —
which is exactly why mock results are not OS-backend results.

**Fix, two parts:**
1. Enable `windows-native`, `apple-native`, `sync-secret-service`, `crypto-rust`.
   All three are safe together: `keyring` declares their crates under
   `[target.'cfg(target_os = …)'.dependencies]`, so only the relevant one builds.
2. `OsSecretStore::set` now **reads the value back** and fails loudly on mismatch
   or a missing entry, so this can never again be mistaken for success.

**Verified after fix:** 8/8 pass, including a fresh-handle read (process-restart
persistence), update, delete, idempotent delete, and account isolation. The tests
use a dedicated `handy-voice-api-selftest` service, so they cannot read or destroy
the application's real key; one test asserts that isolation directly.

**Confirmed in the running app** — the log shows the real Windows credential store:
```
[keyring][DEBUG] creating entry with service handy-voice-api, user handy-voice-api:custom
[keyring][DEBUG] created entry WinCredential { username: "handy-voice-api:custom", ... }
[keyring][DEBUG] get password from entry WinCredential { ... }
```

## 5. Defect 3 — uploads never reached the network (CRITICAL, fixed)

**How it was found:** the GUI upload returned nothing, and the loopback backend
recorded **0 requests**. The app log showed, repeatedly:

```
[reqwest::blocking::client][TRACE] start runtime::block_on
[reqwest::blocking::client][ERROR] Failed to communicate successful startup: Ok(())
```

**Cause:** `manager.client()` — which builds the `reqwest::blocking::Client` — was
called on the async command's thread. The blocking client starts its own background
runtime at construction, and doing that on a Tokio worker thread breaks its startup
handshake. Every request then died before opening a socket.

This is invisible to the crate's own tests: `tests/http_integration.rs` drives the
**same** `ReqwestTransport` over real sockets and passes 11/11, because it runs on
ordinary threads with no ambient async runtime.

**Fix:** construct the client *inside* the `spawn_blocking` closure.

**Verified after fix** — the real app uploaded and the loopback backend received it:
```
invoke voice_api_upload
{"outcome":"uploaded","status":200,"attempts":1,"idempotency_key":"abf422dade7b9039848267967e78d059"}

backend recorded:  POST /transcripts   303 bytes
  authorization:   ***redacted by the recorder***
  idempotency-key: abf422dade7b9039848267967e78d059
  body: {"created_at":"2026-10-11T04:54:03Z","language":"auto","schemaversion":"1.0",
         "segments":[{"final":true,"id":1,"kind":"final","revision":2,"segment_id":"seg-1",
         "text":"你好，我想測試語音辨識。"}],
         "sessionid":"session-1791694443105-0","source":"microphone",
         "updated_at":"2026-10-11T04:54:24Z"}
```

## 6. Defect 4 — single-flight permit leaked permanently (HIGH, fixed)

**How it was found:** a connection test answered `another network operation is
already running` on a freshly started app, and never recovered.

```
voice_api_is_busy      -> busy=true
voice_api_last_operation -> null
```

**Cause:** `voice_api_test_connection` acquired the permit, then
`.with_session(...).map_err(err)?` returned early **before**
`end_network_operation()`. The flag stayed `true` for the life of the process, so
uploads and connection tests were dead until restart.

**Fix:** the permit is now an RAII guard (`NetworkOperation`) released on `Drop`.
Early return, `?`, or panic can no longer strand it; the manual release calls are
gone. A regression test performs an operation that fails early through `?` and
asserts the permit is free afterwards.

**Verified after fix:** `busy=false` after a failing operation, and the subsequent
upload succeeded.

## 7. Defects 5 and 6 — phantom segment and duplicated transcript (MEDIUM, fixed)

**How they were found:** driving the running app's Transcript screen with real
recognition payloads.

Injected `partial → final → late partial → replay final` produced:
```
CONFIRMED   你好，我想測試語音辨識。今天天氣很好，適合出門走走。
RECOGNIZING 你好，我想測試                              <- phantom segment
```
and the model reported `segment_count: 2, final_segment_count: 2` with two
identical confirmed segments.

**Cause:** with no active segment, `on_stream_text` unconditionally created one, and
`on_final` unconditionally created one. A partial arriving after the final — which
is indistinguishable from new speech by payload alone — became a second segment,
and a repeated final was recorded twice.

**Fix:** the adapter remembers `last_finalized`. While there is no active segment, a
partial whose committed prefix rewinds into the just-finalized text is treated as an
in-flight straggler and ignored; an identical final with no utterance in between is
treated as a replay and ignored. The window closes by itself: any partial that is not
a prefix of the finalized text is accepted as new speech. `reset()` clears it.

The trade-off is stated in the code: if a user immediately repeats a phrase that is
exactly a prefix of the previous one, it is dropped. A silent omission is less
misleading than a duplicated transcript, and upstream stops the stream at finalize,
so this window is pathological rather than routine.

**Verified after fix in the running app:**
```
partial -> final -> late partial -> replay final
  voice_api_upload_preview -> segment_count: 1, final_segment_count: 1
  UI shows a single CONFIRMED block, no phantom RECOGNIZING
```
Four new unit tests cover it, including a negative control that the guard does not
swallow genuine new speech.

> Note: the pre-existing test named
> `a_late_partial_after_final_does_not_reopen_the_segment` did **not** cover this.
> Its "late partial" was text that was *not* a prefix of the finalized text, so it
> exercised the discontinuity path instead. The name was misleading; the real case
> is now covered.

## 8. Defect 7 — local duplicate suppression never fires (OPEN)

The second `voice_api_upload` of unchanged content returned `uploaded` again and
the backend recorded a **second** request.

**Cause:** `manager.client()` builds a fresh `ApiClient` on every call, so the
client's `last_uploaded` memory — which exists to suppress exactly this — is always
empty. The crate's own HTTP integration test covers the suppression and passes,
because there one client performs both uploads.

**Mitigating factor:** the idempotency key is derived from transcript content and is
**stable across both uploads** (`abf422da…`), so a server that honours
`Idempotency-Key` will not store a duplicate. The local check is an optimisation and
an offline safety net, not the only protection.

**Not fixed.** The correct fix is to cache the `ApiClient` in the manager and
invalidate it when the configuration or key changes. That is a change to the upload
path with invalidation and thread-safety consequences, and implementing it without
being able to verify it would trade a known, bounded defect for an unknown one. It
is recorded here as open, with the diagnosis and the intended fix.

## 9. Real ASR acceptance — PASS (synthetic audio)

No microphone is available, and no loopback or virtual capture device exists on
this machine. Rather than skip ASR entirely, a corpus was generated with Windows
SAPI's **Traditional Chinese (zh-TW)** voices, giving real audio with an exactly
known ground truth, and the model transcribed it through Handy's real headless
pipeline.

Model: `handy-computer/Breeze-ASR-25-gguf/Breeze-ASR-25-Q5_K_M.gguf`
(1,106.6 MB, SHA-256 verified against the catalogue digest
`c871fc81…139b` before use). Backend: **Vulkan0 — NVIDIA GeForce RTX 5060 Ti**.

| Sample | Ground truth | Recognition | CER |
|---|---|---|---|
| zh_tw_simple | 你好，我想測試語音辨識。 | 你好 我想測試語音辨識 | 0 |
| zh_tw_daily | 今天天氣很好，適合出門走走。 | 今天天氣很好,適合出門走走 | 0 |
| zh_tw_mixed | 這個系統的延遲很低，我們用開源的模型來做。 | 這個系統的延遲很低我們用開源的模型來做 | 0 |
| zh_tw_technical | 請幫我檢查這台電腦的記憶體跟顯示卡。 | 請幫我檢查這臺電腦的記憶體跟顯示卡 | 0.0588 |
| zh_tw_english_word | 我用 Open Source 的模型，這樣比較安全。 | 我用 open source 的模型這樣比較安全 | 0 |

**Overall CER 0.0127** (1 edit over 79 reference characters).

The single difference is `台` → `臺`, a character variant rather than a
misrecognition. Punctuation is omitted by the model and normalised out of the
score, since punctuation is a formatting choice, not a recognition error.
Code-switched English was recognised and differs only in case.

**Performance:** the first transcription took **24.8 s** for 3.25 s of audio
(RTF 0.13). That is Vulkan shader compilation, a one-time cost cached on disk:
warm runs took **345–498 ms** for 3.2–3.7 s of audio — roughly 7–10× faster than
real time. This is worth knowing for release: a user's very first transcription
after install will pause for ~25 s on this GPU.

> **This is synthetic TTS audio, not a human voice through a microphone.** Clean
> studio speech is markedly easier than a person in a room, so these figures are a
> **best case** and must not be quoted as real-world accuracy.

## 10. Real recognition events reaching the GUI — PASS

The microphone cannot be fed on this machine, but the **event bridge** was driven
end to end with the genuine upstream payload shapes, emitted through Tauri's own
IPC from inside the running page.

Verified live, in the running release-quality build:

| Step | Observed |
|---|---|
| `stream-text-event {committed: 你好，我想, tentative: 測試語音辨識}` | UI shows `RECOGNIZING` and `你好，我想測試語音辨識` |
| same utterance advances | the **same** segment grows in place — no duplicate |
| `history-update-payload` (final) | `RECOGNIZING` becomes `CONFIRMED`, full final text |
| late partial after final | **ignored** — no phantom segment |
| replay of the identical final | **ignored** — no duplication |
| upload preview panel | `Session: session-1791694443105-0`, `Language: auto` |

The text on screen came from the real event → Rust `app.listen` listener →
`StreamEventAdapter` → `TranscriptManager` → emitted event → React. Nothing was
mocked inside the application; the only synthetic part is that the *source* was an
injected event rather than the ASR engine, which emits the identical event with the
identical payload type.

**Not verified:** the upstream *emission* of those events under live speech, which
needs a microphone.

## 11. GUI acceptance

Driver: Windows UI Automation (which found and invoked real controls), plus the
Chrome DevTools Protocol over WebView2 for reading rendered state and injecting
events. Chrome accessibility must be forced on —
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 --force-renderer-accessibility`
— otherwise WebView2 exposes no DOM tree at all and the window looks like an empty
pane.

| Action | Result |
|---|---|
| Complete onboarding (select Breeze-ASR-25) | PASS — it detected the pre-placed model, no re-download |
| Settings sidebar shows **Transcript** and **Voice API** | PASS |
| Open Transcript page | PASS — full UI renders |
| Display real transcript | PASS — from injected real events |
| Distinguish partial vs final | PASS — `RECOGNIZING` vs `CONFIRMED` |
| Clear / Copy / Export JSON / Upload buttons present and wired | PASS |
| Save non-sensitive config, store API key, connection test | PASS |
| Manual upload | PASS — reached the loopback backend |
| In-progress / success / failure status | PARTIAL — tested via command results and the busy flag; not visually confirmed mid-flight |
| Recover after an error | PASS — the permit is released and the next operation succeeds |
| **Genuine human mouse/keyboard clicking** | **NOT RUN** — automation drove the real DOM and real controls, but no human clicked |

> Note: WebView2 renders it, so the new screens are in **English** while the rest of
> the UI is Traditional Chinese. That is the documented fallback: the 79 new
> strings exist only in the `en` locale. Visible here in the real UI.

## 12. JSON export — PASS

Exported from the running app via the registered command:
`C:\Users\Nova\Project Brief\_phase3_export\transcript.json`

| Check | Result |
|---|---|
| File written | PASS — 393 bytes |
| UTF-8, no BOM | PASS |
| Traditional Chinese intact | PASS — `你好，我想測試語音辨識。` byte-exact |
| Valid against `schema/transcript-v1.0.json` | PASS |
| Contains schema version / session / segments | PASS — `schemaversion: 1.0`, `sessionid`, `segments[0].final: true` |
| Contains the API key | **PASS — absent** |
| Contains `Bearer` / any auth material | **PASS — absent** |
| Transcript unchanged by exporting | PASS |
| Saving through the native file dialog | **NOT RUN** — the command was invoked directly with a path; the dialog was not driven |
| Cancelling the dialog does not clear the transcript | **NOT RUN** |

## 13. Secret protection

| Check | Result |
|---|---|
| Real write to Windows Credential Manager | PASS — observed in the keyring log and by 8 real tests |
| Real read-back / survival across a fresh handle | PASS |
| Update, delete, idempotent delete | PASS |
| Test namespace cannot see the application namespace | PASS |
| Key absent from the exported JSON | PASS |
| Key absent from the persisted config file | PASS (unit test) |
| Key absent from `Debug`/`Display`/error text | PASS (9 unit tests) |
| GUI config response carries no key | PASS — `voice_api_get_config` returns config only |
| Silent downgrade to plaintext on store failure | PASS — there is no plaintext fallback path, and `set` now verifies the write |
| Real credentials exposed during testing | **None.** Only the synthetic value `sk-phase3-test-key-not-real-0001` was used, and the recorder redacts `Authorization` |

## 14. HTTPS / TLS

**NOT RUN against any real service.** No production endpoint or credential was
configured, and no authorisation to send data to a third party was given, so no
external request was made. Everything above is loopback HTTP.

TLS verification was **not** disabled anywhere: the code path only relaxes the
scheme check for plain HTTP, and a unit test asserts that `http://` is refused
unless explicitly opted in — including for loopback:
`plain_http_is_refused_unless_explicitly_allowed_even_for_loopback`.

## 15. Build and launch

```
cargo build --manifest-path src-tauri/Cargo.toml         -> Finished, exit 0
cargo build --release --manifest-path src-tauri/Cargo.toml -> Finished in 16m36s, exit 0
  debug   handy.exe  86.6 MB
  release handy.exe  48.4 MB (stripped)
```

The app was launched from the freshly built artifact and its commands were exercised
(§3–§8, §10–§12).

> **Correction to Phase 2.** Phase 2 reported "the application launches" on the
> strength of the process being alive with the window title set. In fact a **debug**
> Tauri build loads `devUrl` (`http://localhost:1420`), so with no Vite server the
> window displayed `localhost - 網路錯誤 - 網頁內容` — a network error page. It was
> found this phase with UI Automation. The GUI only ever rendered because a Vite dev
> server happened to be running. A debug build is not self-contained; this needs
> either `bun run dev` alongside it or a release build.

## 16. Not done, and why

| Item | Status | Reason |
|---|---|---|
| Microphone capture | **BLOCKED** | No loopback or virtual capture device exists; installing a virtual audio driver needs administrator rights and would alter the user's audio configuration. Requires a human speaking. |
| Real human speech accuracy | **BLOCKED** | Same. TTS results are a best case, not real-world accuracy. |
| Live streaming through the ASR engine | **BLOCKED** | Needs the microphone path. The bridge itself is verified with real payloads. |
| Production third-party API | **NOT RUN** | No endpoint, credential or authorisation. Loopback results must not be read as production validation. |
| TLS handshake / certificate validation | **NOT RUN** | No HTTPS endpoint was contacted. |
| Human click-through | **NOT RUN** | Automation drove the real DOM and UI Automation invoked real controls, but no human clicked. |
| Native save/open dialog | **NOT RUN** | The export command was invoked with a path directly. |
| macOS / Linux build and run | **NOT RUN** | Windows only. |
| Installer bundling (`tauri build`) | **NOT RUN** | Needs signing configuration. |
| Duplicate-upload suppression | **FAIL (open)** | §8. |

## 17. Files changed in this phase

| File | Change |
|---|---|
| `src-tauri/src/lib.rs` | **Fix**: removed the double `Arc` wrap that broke every command; added an explicit type annotation as a compile-time guard |
| `src-tauri/src/commands/voice_api.rs` | **Fix**: RAII single-flight permit; client built on the worker thread; 2 new tests |
| `crates/handy-voice-api/Cargo.toml` | **Fix**: enable the `keyring` platform backends |
| `crates/handy-voice-api/src/secrets.rs` | **Fix**: `set` verifies by reading back |
| `crates/handy-voice-api/src/stream_adapter.rs` | **Fix**: straggler and replay suppression; 4 new tests |
| `crates/handy-voice-api/tests/os_keyring.rs` | New: 8 real credential-store tests (ignored by default) |
| `tools/dev_env.ps1`, `tools/setup_portable_toolchain.ps1` | New in the fork: both were referenced by the docs but missing |
| `tools/fetch_model.ps1` | New; reads the repo id and revision correctly |
| `tools/gen_tts_corpus.ps1`, `tools/tts_corpus.json` | New: TTS corpus with known ground truth |
| `tools/run_asr_corpus.ps1` | New: runs real ASR and scores it |
| `tools/ui_probe.ps1` | New: UI Automation driver |
| `tools/cdp_drive.mjs` | New: WebView2 DevTools driver |
| `tools/test_backend.mjs` | New: loopback recording server |
| `docs/PHASE3_VALIDATION_PLAN.md`, `docs/PHASE3_TEST_REPORT.md` | New |

## 18. Next steps

1. **Fix local duplicate suppression** (§8): cache the `ApiClient` in the manager,
   invalidate on config/key change, and assert one network request for two identical
   uploads — in the app, not only in the crate.
2. **Run a human microphone test**: speak a known phrase, confirm `RECOGNIZING`
   appears and settles into `CONFIRMED`, and measure real end-to-end latency and
   accuracy. Everything up to the event bridge is verified; this is the last gap.
3. **Validate against a real HTTPS provider** once an endpoint and authorisation
   exist, and confirm certificate validation is active rather than merely enabled.
4. **Make a release build self-contained** — either verify `bun run tauri build`
   produces a binary that serves the embedded frontend without a dev server, or
   document that a debug build requires `bun run dev`.
5. Consider raising the transcript event throttle or batching, and re-check CPU use
   during long dictation now that the GUI actually receives live updates.
