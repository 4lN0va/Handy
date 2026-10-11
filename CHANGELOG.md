# Changelog

All notable changes to this fork are documented here.

This project is a fork of [cjpais/Handy](https://github.com/cjpais/Handy). The
fork's own changes are listed below; upstream history is preserved in git and is
not duplicated here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this fork does not
currently publish versioned releases of its own.

## [Unreleased]

### Phase 2 — integration

#### Added

- **Recognition events are now wired into the transcript**, with **no upstream
  file modified**: the bridge subscribes from Rust to the events Handy already
  emits (`stream-text-event`, `stream-phase-event`, `history-update-payload`,
  `transcription-error`) inside `commands/voice_api.rs`.
  - **`StreamEventAdapter`** (`crates/handy-voice-api/src/stream_adapter.rs`) — a
    pure state machine converting `{committed, tentative}` into segment-addressed
    transcript updates. The upstream payload carries **no segment id and no
    revision**, and each event contains the whole utterance, so identity is
    derived from the *shape* of the committed prefix (progress / stale /
    discontinuity) rather than from the text. Non-streaming models are handled by
    the same path: a final text with no active segment creates and finalizes one.
  - **Two new settings screens**: **Transcript** (live text with tentative text
    distinguished, inline edit, copy, clear, JSON export, upload with a
    disclosure panel) and **Voice API** (endpoint, method, headers, body shape,
    credentials, timeout/retry, connection test).
  - Commands: `voice_api_is_busy`, `voice_api_last_operation`,
    `voice_api_upload_preview`.
  - `tools/msvc_env.ps1` — imports the MSVC developer environment, with the two
    locale fixes below.
  - `tools/add_voice_api_translations.mjs` — injects the 79 new English UI
    strings, with `--check` for CI.
  - `docs/PHASE2_INTEGRATION_PLAN.md`, `docs/PHASE2_TEST_REPORT.md`.
  - 18 adapter tests, 11 real-socket HTTP integration tests, and 12 bridge tests in
    the application crate.

#### Fixed

- **`VSLANG=1033` is required in this environment.** Otherwise CMake+Ninja cannot
  parse the localised `/showIncludes` prefix from `cl.exe` and the native build
  dies with a bare `ninja: build stopped: subcommand failed`.
- **MSVC must be told the sources are UTF-8** (`-DCMAKE_C_FLAGS=/utf-8`).
  `transcribe-cpp-sys`'s `canary/model.cpp` is UTF-8 without a BOM and contains a
  literal `▁`; on a GBK/936 system the comment swallows real code and the compiler
  reports `'sp_space_id': undeclared identifier` in valid code. Invisible on
  upstream's English-locale CI.
- A genuine defect in a new test: two managers were given separate in-memory
  credential stores, so a reloaded manager could not see a stored key. Fixed by
  sharing the store, matching the process-wide OS store the app uses.

#### Changed

- `voice_api_upload` and `voice_api_test_connection` take a single-flight permit
  (`compare_exchange`), so a double click cannot fan out into concurrent
  requests. The bridge's state changes were split from event emission so the logic
  is testable without a running Tauri application.

#### Verified by building and running

- The application now **builds** (`cargo build --manifest-path src-tauri/Cargo.toml`)
  and **launches**, initialising the Vulkan backend on an RTX 5060 Ti.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: **294 passed, 0 failed,
  16 ignored** — upstream's whole library suite, which could not be compiled
  before this phase.
- `handy-voice-api`: **190 tests** pass, clean `fmt` and `clippy -D warnings`.
- Front end type-checks against the regenerated bindings; `bun run build` passes.

> **Still not verified:** microphone capture, speech accuracy, GUI click-through,
> and any production third-party API. See `docs/PHASE2_TEST_REPORT.md`.

### Phase 1 — voice-API layer

#### Added

- **`handy-voice-api` crate** (`crates/handy-voice-api/`) — the transcript,
  JSON-export and custom-upload layer, kept as a separate crate so it compiles
  and tests independently of the native GUI toolchain.
  - **Transcript model** with stable `segment_id` identity and monotonic
    `revision` numbers. An update *replaces* the addressed segment rather than
    appending, so tentative and confirmed text can never both appear. A
    finalized segment is never reopened by a late partial, and a stale or
    duplicate revision is dropped.
  - **Versioned JSON schema** (`schema/transcript-v1.0.json`), embedded into the
    binary with `include_str!` so the published schema and the compiled
    validator cannot drift. Validation is lenient on read (the minimal document
    from the product specification is accepted) and strict on write (the
    exporter always emits every field).
  - **Export/import**: atomic writes via a sibling temp file plus rename, UTF-8
    with no BOM written and a BOM tolerated on read, and strict decoding so
    invalid bytes are reported rather than silently mangling CJK text.
  - **Custom backend client**: configurable base URL, endpoint, HTTP method,
    custom headers, content type, auth scheme (bearer or custom header), and
    request body shape (raw document, wrapped in a field, or a JSON template).
    HTTPS is required unless plain HTTP is explicitly allowed for a local
    backend.
  - **Retry policy** with exponential backoff that retries only genuinely
    retryable faults. A 4xx other than 429 is never retried, because retrying
    would repeat the mistake and risk a duplicate write.
  - **Idempotency**: a key derived from the session id and the exact request
    bytes, sent as `Idempotency-Key`, plus in-process suppression of a repeated
    upload of unchanged content.
  - **Secret handling**: `SecretString` with redacted `Debug`/`Display`; API
    keys stored in the OS credential store behind the `keyring` feature. The
    persisted endpoint configuration contains no secret.
  - **160 unit/integration tests plus one doctest**, with `cargo fmt` and
    `clippy -D warnings` clean.
- **Tauri command surface** (`src-tauri/src/commands/voice_api.rs`): session
  lifecycle, transcript read/edit/clear, JSON export/import, credential
  management, connection test, and upload. Blocking I/O runs on a worker thread
  so neither the GUI nor the audio pipeline can be blocked by a slow endpoint.
- **Model licence tooling**
  - `tools/gen_model_licenses.mjs` generates `MODEL_LICENSES.md` from the model
    catalog, and `--check` fails CI if a restricted or unconfirmed licence is
    marked as a recommended default.
  - `tools/apply_license_overrides.mjs` applies reviewed licence overrides to the
    upstream-generated catalog. Overrides live in `model_license_overrides.json`
    and are applied as surgical text edits, because re-serializing the catalog
    would reformat thousands of lines.
- **Dependency licence tooling**: `tools/audit_rust_licenses.mjs` audits every
  resolved crate version against the crates.io registry (with rate-limit
  handling and a persistent cache), and `tools/gen_third_party_notices.mjs`
  generates `THIRD_PARTY_NOTICES.md` from that data.
- **Build documentation**: `docs/BUILD_BASELINE.md` records exactly what was
  built, what was verified, and what remains unverified on this machine.

### Changed

- **Model catalog**: `handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf` is no
  longer marked `recommended`. Its licence is declared as `other`, i.e.
  unconfirmed, so it must not be presented to new users as a default pick. See
  `MODEL_LICENSES.md`. This one-field change is applied by
  `tools/apply_license_overrides.mjs` so it survives catalog regeneration.
- `src-tauri/Cargo.toml` adds a path dependency on `handy-voice-api` with the
  `http` and `keyring` features. That crate's `reqwest` dependency uses
  `reqwest`'s default features, so it shares Handy's existing TLS backend rather
  than compiling a second TLS stack into the binary.

### Security

- API keys are stored in the operating system credential store instead of a
  settings file. Tests assert that a key cannot appear in an exported
  transcript, an error message, a connection-test result, or a `Debug`
  rendering of any configuration or request type.

### Known gaps

Superseded by Phase 2 above for the build and integration status; the remaining
gaps are:

- **No microphone, speech-accuracy or GUI click-through test has been performed.**
  Nothing in this repository should be read as evidence that recognition works on
  a given machine, or that any model reaches a particular accuracy or latency.
- Uploads are verified against a **local test backend over loopback**, not against
  any production third-party API.
- **Cancellation is not wired to a UI control.** An upload is bounded by the
  configured timeout and by refusing concurrent requests, but the retry loop has
  no cancel token.
- **Command payloads cross the boundary as JSON strings** rather than generated
  types, because `tauri-specta` needs `specta::Type` on every type and the crate
  deliberately does not depend on a UI framework. The frontend declares matching
  interfaces by hand.
- **Only English has the new UI strings.** i18next falls back to `en` for the
  other 25 locales, so the screens are usable everywhere, but they are untranslated.
- Auto-upload is not implemented. Uploads are only ever performed by an explicit
  user action.
- Only Windows x86_64 has been built and run.

### Not yet addressed

- Vendoring the git dependencies (`tao`, `tao-macros`, `hf-hub`, `rdev`,
  `rodio`, `vad-rs`, `tauri-nspanel`) into a controlled location.
- `cjpais/vad-rs` ships **no LICENSE file**; only a `Cargo.toml` declaration.
- Frontend (JavaScript) dependency licence audit.
- Font and icon licence audit.
