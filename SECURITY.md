# Security Policy

This document describes how this fork handles sensitive data, what it does and
does not protect against, and the known limitations. It is written to be
**accurate rather than reassuring**: where a protection is partial or unverified,
that is stated.

## Reporting a vulnerability

Please open a private security advisory on this repository's GitHub page
("Security" → "Report a vulnerability") rather than a public issue.

Include: affected version or commit, platform, reproduction steps, and the impact
you believe it has. If the issue also affects upstream Handy, please report it
upstream as well — this fork tracks upstream and an upstream fix benefits
everyone.

**Do not** include real API keys, real transcripts, or recordings in a report.

## Threat model

The assets worth protecting:

1. **The user's API key** for their configured backend.
2. **The transcript** — what the user said.
3. **The audio** — only relevant if recordings are retained.

The realistic failure modes this project defends against:

- an API key appearing in a log line, an exported file, an error message, or a
  crash report;
- an API key being written to a world-readable settings file;
- a transcript being sent somewhere the user did not configure or agree to;
- a transcript being silently corrupted (dropped, duplicated, or mangled) so that
  the user does not know what was recorded.

## API key handling

Implemented in `crates/handy-voice-api/src/secrets.rs`.

- Keys are stored through the **operating system credential store** —
  macOS Keychain, Windows Credential Manager, Linux Secret Service — via the
  `keyring` crate. This fork adds that; **upstream Handy does not use a
  keyring** and keeps provider keys in its settings store.
- `SecretString` implements `Debug` and `Display` as `***redacted***`, so a key
  cannot escape through `{:?}` formatting. It deliberately does **not** implement
  `Serialize`, so it cannot be written into a settings file or a transcript by a
  `serde` derive.
- The error type never carries request headers, and the client strips the key
  from any text it returns — including a response body from a misbehaving server
  that echoes the request back.
- The persisted endpoint configuration (`voice_api_config.json`) contains **no
  secret**. Only the account name under which the key is stored is derivable.

Tests that enforce this (all passing):

| Test                                                      | Asserts                                            |
| --------------------------------------------------------- | -------------------------------------------------- |
| `debug_formatting_never_reveals_the_secret`               | `{:?}` on a key is redacted                        |
| `a_struct_deriving_debug_does_not_leak_a_wrapped_secret`  | a derived `Debug` on a config struct does not leak |
| `request_debug_output_never_contains_the_api_key`         | a built HTTP request does not leak                 |
| `config_and_auth_debug_output_never_contains_the_api_key` | client and auth config do not leak                 |
| `an_error_message_never_contains_the_api_key`             | an echoing server cannot cause a leak              |
| `a_connection_test_failure_never_contains_the_api_key`    | the diagnostic path does not leak                  |
| `exported_document_never_contains_api_key_material`       | exports and copied text do not leak                |

### Known limitation: no memory zeroization

`SecretString` does not scrub its heap buffer on drop. Doing that correctly
requires care (the allocator may already have copied the value, and `String` can
reallocate), and this crate is `#![forbid(unsafe_code)]`. If that property is
required, wrap the value with the `zeroize` crate rather than hand-rolling an
`unsafe` write. A process **memory dump** can therefore contain the key.

## Network behaviour

### What the application contacts

| Destination                                                                        | When                                          | Controlled by |
| ---------------------------------------------------------------------------------- | --------------------------------------------- | ------------- |
| Model catalogue sources (Hugging Face, and the mirror configured in the catalogue) | only when the user downloads a model          | user action   |
| The configured LLM post-processing provider                                        | only when post-processing is enabled and used | user settings |
| The user's configured upload endpoint                                              | only on an explicit upload action             | user settings |

There is **no telemetry, analytics, crash reporting, or first-party backend** in
this application. Verified by searching `src-tauri/src` for
`telemetry|analytics|sentry|mixpanel|amplitude|posthog`: no matches.

### Upload safety

- **HTTPS is required by default.** A plain `http://` endpoint is refused unless
  the user explicitly enables "allow insecure http", which exists for a backend
  on the local loopback interface.
- **Nothing is uploaded implicitly.** There is no auto-upload path in the code;
  uploads happen only from an explicit user action.
- Blocking network I/O runs on a worker thread, so a slow or hanging endpoint
  cannot freeze the GUI or the audio capture thread.
- Retries use bounded exponential backoff and only retry genuinely retryable
  faults (transport errors, timeouts, HTTP 429 and 5xx). A 4xx other than 429 is
  never retried, because retrying would repeat the mistake or risk a duplicate
  write.
- Requests carry an `Idempotency-Key` derived from the session id and the exact
  request bytes, so a server _can_ deduplicate a retry. The key is a truncated
  SHA-256 digest and contains no transcript text.

> **The server side is outside our control.** Sending an idempotency key only
> helps if the receiving service honours it. A backend that ignores it may still
> create duplicates on retry. This is why a non-retryable 4xx is never retried.

## Data at rest

| Data                                 | Location                                                  | Protection                                              |
| ------------------------------------ | --------------------------------------------------------- | ------------------------------------------------------- |
| Transcript history                   | Handy's SQLite history database in the app data directory | **none** — not encrypted; relies on OS file permissions |
| Recordings (if retention is enabled) | app data directory                                        | **none**                                                |
| Endpoint configuration               | `voice_api_config.json` in the app data directory         | **none needed** — contains no secret                    |
| API key                              | OS credential store                                       | provided by the operating system                        |
| Application logs                     | log directory in the app data directory                   | see below                                               |

**Transcriptions and recordings are not encrypted at rest.** Anyone with access
to the user account's files can read them. Full-disk encryption is the practical
mitigation.

### Logging and transcripts — read this carefully

Transcription text is passed through `redact_text` in `src-tauri/src/utils.rs`
before being logged. That function is:

```rust
pub fn redact_text(text: &str) -> &str {
    if cfg!(debug_assertions) { text } else { "[REDACTED]" }
}
```

So:

- **Release builds**: transcript text is replaced with `[REDACTED]` in logs. ✅
- **Debug builds** (`cargo build` without `--release`): transcript text is logged
  **verbatim**. This includes the default developer build and any build made with
  `--debug`. ⚠️

This is inherited upstream behaviour, not something this fork introduced, and it
is why a debug build should never be used with real dictation.

## Dependencies

- The Rust dependency graph is inventoried in `THIRD_PARTY_NOTICES.md`, generated
  from a registry audit (`tools/audit_rust_licenses.mjs`). The fork's resolved
  graph contains **no GPL or AGPL** dependency.
- Speech **models** are downloaded separately; their licences are inventoried in
  `MODEL_LICENSES.md`. Models are **not** bundled with the application, and each
  download is verified against a SHA-256 recorded in the catalogue.
- Git dependencies pinned to individual forks must be vendored before release;
  see `CHANGELOG.md`.

## Unverified areas

Stated plainly so nobody mistakes absence of evidence for evidence of safety:

1. **The application has been built and launched, but not exercised.** It starts,
   initialises the Vulkan backend and creates its windows; no dictation has been
   performed, so no runtime behaviour of the credential-store integration, the
   transcript bridge or the upload path has been observed end to end. See
   `docs/PHASE2_TEST_REPORT.md`.
2. **The OS credential store is not covered by an automated test.** The crate
   tests use an in-memory store; `OsSecretStore` compiles and is wired up, but a
   real write/read against Keychain, Credential Manager or Secret Service has not
   been performed.
3. **Uploads were verified against a local loopback test server only** — plain
   HTTP, no TLS handshake, no production third-party API.
4. The front-end (JavaScript) dependency tree has **not** been licence- or
   vulnerability-audited.
5. No penetration testing, fuzzing, or independent review has been performed.
6. Upstream's own logging, IPC surface, and CSP configuration have not been
   audited as part of this work.

## Configuration hardening notes

- Upstream's Tauri configuration sets `csp: null` and an asset-protocol scope of
  `**`. Tightening both is advisable and is tracked in `CHANGELOG.md`.
- On Linux, GTK and WebKitGTK are LGPL-licensed C libraries that must remain
  **dynamically linked and replaceable**; do not statically link them.
