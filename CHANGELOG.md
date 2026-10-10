# Changelog

All notable changes to this fork are documented here.

This project is a fork of [cjpais/Handy](https://github.com/cjpais/Handy). The
fork's own changes are listed below; upstream history is preserved in git and is
not duplicated here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this fork does not
currently publish versioned releases of its own.

## [Unreleased]

### Added

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

- **The application has not been built or run.** The full build requires MSVC
  Build Tools, which need administrator rights that this environment does not
  have. Nothing in this repository should be read as evidence that the GUI
  launches, that the microphone works, or that any model transcribes correctly.
  See `docs/BUILD_BASELINE.md` for the evidence and the unblocking steps.
- `src-tauri/src/commands/voice_api.rs` has been verified to **parse and format**
  (via `rustfmt`) but **not to type-check**, because that requires compiling
  `src-tauri`'s dependency graph.
- Front-end UI for the new features is not implemented yet; the command surface
  exists but nothing in the React app calls it.
- Auto-upload is not implemented. Uploads are only ever performed by an explicit
  user action.
- `/api` uploads are not wired to Handy's own transcription events yet; the
  frontend must forward results into `voice_api_apply_update`.

### Not yet addressed

- Vendoring the git dependencies (`tao`, `tao-macros`, `hf-hub`, `rdev`,
  `rodio`, `vad-rs`, `tauri-nspanel`) into a controlled location.
- `cjpais/vad-rs` ships **no LICENSE file**; only a `Cargo.toml` declaration.
- Frontend (JavaScript) dependency licence audit.
- Font and icon licence audit.
