# Phase 2 — Test Report

Every result below was produced by running the command shown. Nothing here is
inferred, and the "not verified" section is as important as the rest.

Environment: Windows 10.0.19045 (x86_64), VS Build Tools 2022 (MSVC 14.44.35207),
Windows SDK 10.0.26100.0, Vulkan SDK 1.4.363.0, CMake 4.4.4, Ninja 1.13.2,
Bun 1.4.3, Rust 1.98.0 (`stable-x86_64-pc-windows-msvc`).

---

## 1. Summary

| Area                                                       | Result                                                              |
| ---------------------------------------------------------- | ------------------------------------------------------------------- |
| Full Windows desktop build (MSVC)                          | ✅ **succeeds**                                                     |
| Desktop application launches                               | ✅ **runs**                                                         |
| `handy-voice-api` crate tests                              | ✅ **190 passed** (178 unit + 11 HTTP integration + 1 doctest)      |
| Application crate tests (incl. upstream's)                 | ✅ **294 passed, 0 failed, 16 ignored**                             |
| `cargo fmt --check`                                        | ✅ clean                                                            |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ clean                                                            |
| Front end type-check + production build                    | ✅ succeeds                                                         |
| Licence/schema tooling tests                               | ✅ 41 passed                                                        |
| Real recognition events wired into the transcript          | ✅ implemented; **not** exercised with live speech                  |
| React calls the real Tauri commands                        | ✅ type-checked against generated bindings; **not** clicked through |
| GUI JSON export                                            | ⚠️ logic covered by tests; **button not clicked**                   |
| Manual upload to a test backend                            | ✅ verified over real sockets                                       |
| Manual upload to a production third-party API              | ❌ **not attempted**                                                |
| Microphone capture                                         | ❌ **not tested**                                                   |
| Real ASR accuracy                                          | ❌ **not tested**                                                   |

---

## 2. Build environment

```
$ vswhere -all -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format value -property installationPath
C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools      # isComplete: true

$ where link        (inside vcvars64)   -> ...\14.44.35207\bin\HostX64\x64\link.exe
$ link /?                               -> Microsoft (R) Incremental Linker Version 14.44.35229.0
$ where cl          (inside vcvars64)   -> ...\14.44.35207\bin\HostX64\x64\cl.exe
Windows SDK Include/Lib                 -> 10.0.26100.0
$ where glslc                           -> C:\VulkanSDK\1.4.363.0\Bin\glslc.exe
```

Toolchain linking was proven before touching the app, with a crate that pulls
`serde` + `serde_json` (so build scripts and proc-macros are exercised):

```
$ cargo run --quiet
{"text":"你好，我想測試語音辨識。"}
run exit=0
```

Three environment defects were found and fixed; all three are recorded in
`tools/msvc_env.ps1` because they will recur on any non-English Windows:

1. **`VSLANG=1033`** — otherwise Ninja cannot parse `cl.exe`'s localised
   `/showIncludes` prefix and the build dies with a bare
   `ninja: build stopped: subcommand failed`.
2. **`-DCMAKE_C_FLAGS=/utf-8 -DCMAKE_CXX_FLAGS=/utf-8`** — `transcribe-cpp-sys`'s
   `canary/model.cpp` is UTF-8 without a BOM and contains a literal `▁`. Without
   `/utf-8` MSVC decodes it as GBK/936, the comment swallows real code, and the
   compiler reports `'sp_space_id': undeclared identifier` in valid code.
   Upstream CI is on English-locale runners (CP1252) and never sees this.
3. **Vulkan SDK** was missing; `ggml-vulkan` needs `glslc`.

A stale CMake cache at `%LOCALAPPDATA%\tcs\<hash>\build` also had to be cleared
once, because CMake refuses to switch generator.

### 2.1 Application build

```
$ cargo build --manifest-path src-tauri/Cargo.toml
warning: unused import: `Emitter`          <- pre-existing upstream warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 42.26s
BUILD_EXIT=0
```

Artifact: `handy.exe`, 86.6 MB, with 13 ggml/transcribe runtime DLLs staged next
to it and the Tauri resource tree (`resources/models/silero_vad_v4.onnx`,
tray icons, sounds) in place.

> The only remaining warning is `unused import: Emitter` in `src/secure_input.rs`.
> That file is **not** modified by this fork, so the warning is upstream's. (A
> `with_memory_secret_store is never used` warning existed too and is now gone,
> because the method is used by tests.)

### 2.2 Application launch

```
$ handy.exe --debug          (working directory: src-tauri)
PID 9476, WorkingSet 41.7 MB, MainWindowTitle "Handy", still running after 12 s
```

Startup log (`%LOCALAPPDATA%\com.pais.handy\logs\handy.log`):

```
[INFO] Seeded 69 catalog model(s) into the registry
[INFO] Database migrated from version 0 to 4
[INFO] Recording overlay window created successfully (hidden)
[DEBUG] transcribe_cpp: ggml_vulkan: 0 = NVIDIA GeForce RTX 5060 Ti ...
[INFO] transcribe-cpp initialized with 2 compute device(s): [Vulkan0 (vulkan), CPU (cpu)]
```

One warning appears, and it is benign:

```
[WARN] Failed to apply autostart setting (enabled=false): 系統找不到指定的檔案。 (os error 2)
```

The application is being run as a loose executable rather than an installed one,
so there is no autostart entry to remove. It does not affect the transcript path.

**The application is left running** so it can be inspected.

---

## 3. Tests

### 3.1 `handy-voice-api` crate

```
$ cargo +stable-x86_64-pc-windows-gnu test --all-features
running 178 tests -> ok. 178 passed; 0 failed
Running tests\http_integration.rs
running 11 tests  -> ok.  11 passed; 0 failed
Doc-tests         -> ok.   1 passed; 0 failed
```

New in this phase: **`stream_adapter` (18 tests)**. The six scenarios the spec
requires, each a named test:

| Required scenario         | Test                                                                                                   |
| ------------------------- | ------------------------------------------------------------------------------------------------------ |
| partial → partial → final | `partial_partial_final_produces_one_segment_with_only_the_final_text`                                  |
| replay of the same event  | `replaying_the_same_event_sequence_is_idempotent`, `replaying_an_already_consumed_event_emits_nothing` |
| out-of-order events       | `out_of_order_events_are_ignored`                                                                      |
| late partial after final  | `a_late_partial_after_final_does_not_reopen_the_segment`                                               |
| new session               | `a_new_session_starts_afresh_and_keeps_earlier_text_separate`                                          |
| clear then restart        | `clearing_and_restarting_leaves_no_stale_state_in_the_adapter`                                         |

### 3.2 Real-socket HTTP integration tests

`crates/handy-voice-api/tests/http_integration.rs` drives the **real `reqwest`
transport over real TCP sockets** against a throwaway server bound to
`127.0.0.1`. Nothing is mocked at the HTTP layer.

```
running 11 tests -> ok. 11 passed; 0 failed
```

| Test                                                                        | What it proves                                                                                                                         |
| --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `a_real_upload_reaches_the_server_with_the_expected_bytes`                  | method, path, `Content-Type`, `Authorization: Bearer …`, `Idempotency-Key`, and a **UTF-8 body carrying the Traditional Chinese text** |
| `a_500_is_retried_over_real_sockets_until_it_succeeds`                      | 500 → 502 → 200, three attempts, **identical idempotency key on every retry**                                                          |
| `a_401_is_not_retried_over_real_sockets`                                    | exactly one request reaches the server                                                                                                 |
| `a_429_is_retried_over_real_sockets`                                        | 429 → 200                                                                                                                              |
| `a_timeout_against_a_silent_server_is_reported_as_a_timeout`                | a server that accepts but never answers produces `Timeout`, not a hang                                                                 |
| `a_duplicate_upload_is_suppressed_before_reaching_the_server`               | the second upload never touches the network                                                                                            |
| `a_wrapped_body_shape_reaches_the_server_as_configured`                     | configurable body shape                                                                                                                |
| `a_custom_method_and_header_reach_the_server`                               | `PUT` + `X-Tenant`, idempotency key absent when disabled                                                                               |
| `a_connection_test_against_a_real_server_reports_success_without_uploading` | the diagnostic works and is not counted as an upload                                                                                   |
| `a_connection_test_against_a_closed_port_fails_cleanly`                     | refused connection: no status, no key in the message, retryable                                                                        |
| `plain_http_is_refused_unless_explicitly_allowed_even_for_loopback`         | **nothing is sent over unencrypted HTTP without opt-in**                                                                               |

> **Scope limit.** These are a _controlled test backend on loopback over plain
> HTTP_. They say nothing about any production third-party API: no external
> service was contacted, no TLS handshake occurred, and no real provider
> contract was exercised.

### 3.3 Application crate

```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib
running 310 tests
test result: ok. 294 passed; 0 failed; 0 ignored; 16 ignored; finished in 2.01s
```

This runs **upstream Handy's whole library test suite** for the first time in
this project — it could not even be compiled before this phase. 12 of the 294 are
new bridge tests:

| Test                                                                  | What it proves                                                              |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `recognition_events_create_a_session_lazily`                          | a global-shortcut recording that starts before the GUI opens is not dropped |
| `a_streaming_utterance_becomes_one_final_segment`                     | the in-situ adapter plus manager: one segment, not one per event            |
| `a_non_streaming_model_still_produces_a_final_segment`                | batch models still produce a transcript                                     |
| `replayed_events_report_no_change_the_second_time`                    | no event emitted, no revision consumed                                      |
| `a_stale_event_reports_no_change`                                     | an older prefix is ignored                                                  |
| `recognition_errors_are_recorded_but_never_become_text`               | errors go to `errors[]`, never to `segments[]`                              |
| `starting_a_new_session_discards_the_previous_utterance`              | session reset                                                               |
| `the_single_flight_guard_admits_one_network_operation_at_a_time`      | a double click is refused, and the permit is released                       |
| `the_upload_preview_describes_what_would_be_sent`                     | correct counts for the disclosure panel                                     |
| `configuration_round_trips_through_the_file_and_never_holds_a_secret` | config persists; the key is absent from the file and from `Debug`           |
| `a_corrupt_config_file_does_not_prevent_startup`                      | a malformed config cannot brick the app                                     |
| `the_config_json_the_gui_sends_deserializes_into_api_config`          | the frontend/Rust contract holds                                            |

One of these failed on its first run and was a genuine defect **in the test**: each
manager was given its own in-memory credential store, so the reloaded manager
could not see the key. In the real app the store is the process-wide OS store.
Fixed by sharing the store, which also makes the test assert the intended
property.

### 3.4 Secret-handling tests

| Test                                                                  | Asserts                                            |
| --------------------------------------------------------------------- | -------------------------------------------------- |
| `debug_formatting_never_reveals_the_secret`                           | `{:?}` on a key is redacted                        |
| `a_struct_deriving_debug_does_not_leak_a_wrapped_secret`              | a derived `Debug` on a config struct does not leak |
| `request_debug_output_never_contains_the_api_key`                     | a built request does not leak                      |
| `config_and_auth_debug_output_never_contains_the_api_key`             | client and auth config do not leak                 |
| `an_error_message_never_contains_the_api_key`                         | an echoing server cannot cause a leak              |
| `a_connection_test_failure_never_contains_the_api_key`                | the diagnostic path does not leak                  |
| `exported_document_never_contains_api_key_material`                   | exports and copied text do not leak                |
| `configuration_round_trips_through_the_file_and_never_holds_a_secret` | the config file does not leak                      |
| `configuration_and_schema_errors_are_not_retryable`                   | error classification                               |

### 3.5 Front end

```
$ bun run build          # tsc && vite build
✓ 2129 modules transformed
✓ built in 11.45s
```

`tsc` type-checks the two new React components **against the real generated
command signatures**: `tauri_specta` regenerated `src/bindings.ts` with all 20
`voiceApi*` accessors (231 added lines), so a mismatch between the UI and the Rust
command layer is a compile error, not a runtime surprise.

Four type errors were found and fixed this way, including an incorrect relative
import depth and a wrong `Select` prop shape.

### 3.6 Licence and schema tooling

```
node --test tools/audit_rust_licenses.test.mjs   -> 23 passed
node --test tools/gen_model_licenses.test.mjs    -> 18 passed
node tools/apply_license_overrides.mjs --check   -> OK
node tools/gen_model_licenses.mjs --check        -> OK (69 models, no restricted licence is a default)
node tools/gen_third_party_notices.mjs --check   -> OK (up to date)
node tools/add_voice_api_translations.mjs --check-> OK (79 strings present)
```

---

## 4. Real recognition events: connected or not?

**Connected in code, not exercised with live speech.**

Verified:

- The bridge subscribes to `stream-text-event`, `stream-phase-event`,
  `history-update-payload` and `transcription-error`. Payload types are the real
  upstream structs (`managers::transcription::StreamTextEvent`, …), so an upstream
  shape change is a compile error rather than a silent runtime failure. Event name
  strings were read from the generated `src/bindings.ts`, not guessed.
- **No upstream `.rs` file was modified to make this work** — the bridge is
  `app.listen(...)` inside `commands/voice_api.rs`. Confirmed by diff: the only
  upstream non-test file this phase changes is `src-tauri/src/lib.rs`, and only by
  adding command registrations to the `collect_commands!` list.
- The adapter converts `{committed, tentative}` into segment-addressed updates,
  deriving identity from the shape of the committed prefix because the payload
  carries no id. Using the text as an id was rejected as the spec requires.
- The bridge logic is covered by the 12 tests in §3.3, driven through the real
  `TranscriptManager` and `StreamEventAdapter`.

**Not verified:** no utterance has actually been spoken into the microphone, so
the path from a live model through these listeners has never run. Everything above
is verified at the boundary of the listener's own logic; the upstream emission
frequency and payload evolution under real speech were not observed.

## 5. React → Tauri commands: connected or not?

**Connected and type-checked; not clicked through.**

- `src/components/settings/transcript/TranscriptSettings.tsx` and
  `.../voice-api/VoiceApiSettings.tsx` call `commands.voiceApi*` directly.
- `tsc` validates every call against the generated signature.
- Both are registered in `SECTIONS_CONFIG`, and 79 English strings were added for
  them (verified present by a checker script).
- The settings sidebar therefore has two new sections: **Transcript** and
  **Voice API**.

**Not verified:** the buttons have not been pressed. No UI automation was
available in this environment, and the app runs in a session that cannot be
driven programmatically. Button wiring is verified only to the extent that it
type-checks and that the command behind it has its own tests.

## 6. GUI JSON export

**Partially verified.**

- The serialization, atomic write, UTF-8/BOM handling, schema validation and
  round-trip are covered by ~25 crate tests (`export::tests`), including
  Traditional Chinese, mixed script, emoji, quotes, backslashes and newlines.
- The command `voice_api_export_to_file` adds only a file-dialog path and a
  `fs::write` of already-validated bytes.

**Not verified:** the save dialog has not been opened and no export has been
performed through the GUI.

## 7. Manual upload

**Verified against a real local test backend over real sockets** (§3.2);
**not verified against any production third-party API.**

## 8. Explicitly not done

| Item                               | Why                                                                                                                                                  |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Microphone capture                 | Requires a human to speak and grant OS permission. Not performed.                                                                                    |
| Real ASR accuracy / CER / latency  | Requires audio and a downloaded model. **No measurement claim is made.**                                                                             |
| Production API verification        | Would require a real account and key. The spec requires keeping test-backend results distinct from production ones, so no third-party call was made. |
| GUI click-through                  | No UI automation available.                                                                                                                          |
| Upstream `bun run lint`            | One lint-only package failed to extract during `bun install`; the production build is unaffected.                                                    |
| 16 ignored upstream tests          | Ignored by upstream; not investigated.                                                                                                               |
| Certificate signing / notarisation | No certificates configured; the build is unsigned, as upstream's local builds are.                                                                   |
