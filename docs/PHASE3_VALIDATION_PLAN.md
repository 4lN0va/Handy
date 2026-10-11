# Phase 3 — Validation Plan

This is the working plan and execution record for Phase 3: turning a version that
*compiles* into one that has been **operated**, with clear security boundaries and
reproducible evidence. The results live in
[`PHASE3_TEST_REPORT.md`](PHASE3_TEST_REPORT.md).

Every item is labelled with one of: **PASS**, **FAIL**, **BLOCKED**, **NOT RUN**,
**PARTIAL**. Nothing is carried over from Phase 2 — the environment and repository
were re-verified first, and every command in the test report was executed during
this phase.

---

## 1. Starting state (re-verified, not assumed)

Verified before any change, because Phase 2's report is history, not evidence.

| Item | Observed |
|---|---|
| Working directory | `C:\Users\Nova\Project Brief\handy-fork` |
| Branch | `feature/voice-api-mvp` |
| HEAD | `20f92ed` — *feat(voice-api): wire live recognition into the transcript and build the app* |
| Working tree | **clean** — no uncommitted changes, no untracked files |
| Commits ahead of `upstream-baseline-v0.9.8` | 4 |
| `main` | `48a3d8e` = `origin/main`, untouched |
| Handy process | **not running** at the start of this phase |

Toolchain, re-probed rather than trusted:

| Component | Result |
|---|---|
| Rust | 1.98.0, default `stable-x86_64-pc-windows-msvc`; gnu target also installed |
| VS Build Tools | 2022 17.14.37710, **VC++ x64 component present** (`vswhere -requires`) |
| MSVC toolset | 14.44.35207 — `cl.exe`/`link.exe` resolvable inside `vcvars64` |
| Windows SDK | 10.0.26100.0 |
| CMake | 4.4.4 |
| Ninja | 1.13.2 (portable) |
| Bun | 1.4.3 (portable) |
| Vulkan SDK | 1.4.363.0, `glslc` present |
| ONNX Runtime | not set via `ORT_LIB_LOCATION`; `ort-sys` supplies its own binaries |

## 2. Defects found in this phase

Phase 3 was worth running: it found four real problems, one of them a security
defect.

### 2.1 The GUI was never actually rendering (corrects a Phase 2 claim)

Phase 2 reported "the application launches". It does — but a **debug** Tauri build
loads `devUrl` (`http://localhost:1420`), not the bundled `dist/`. Launched
directly without a Vite server, the window opens and displays a **network error
page**. Phase 2 could not see this because it only checked that the process was
alive and the window title was set.

Discovered by reading the WebView2 accessibility tree, which showed
`localhost - 網路錯誤 - 網頁內容`.

*Fix:* either run a Vite dev server (`bun run dev`) alongside a debug build, or
build in **release** mode, which embeds `dist/` and needs no server. Phase 3 uses
the release build for GUI validation because it is self-contained and does not
depend on a watcher staying alive.

### 2.2 A security defect: the OS credential store silently stored nothing

`Cargo.toml` declared `keyring = { version = "3", optional = true }` with **no
features**. `keyring` v3 ships **no default backend** — each platform backend is
an opt-in feature. Built that way it compiles fine and `set_password` returns
`Ok(())` while writing nothing.

Effect: the Voice API screen would report *"API key saved to the OS credential
store"*, the key would not be stored, and the failure would only surface later as
an authentication error. This is precisely the "silent downgrade that reports
success" anti-pattern `SECURITY.md` warns about.

Found only by running the tests against the **real** Windows Credential Manager:
5 of 8 failed (`set` ok, `get` → `None`). The in-memory mock store had been
passing all along and could never have caught it — which is exactly why the brief
insists mock results are not OS-backend results.

*Fix (two parts):*
1. Enable the platform backends. All three are safe to enable together because
   `keyring` declares their crates under `[target.'cfg(target_os = …)'.dependencies]`,
   so only the relevant one is compiled.
2. `OsSecretStore::set` now **reads the value back** and fails loudly on a
   mismatch or a missing entry, so a silent no-op becomes an accurate error and
   can never be mistaken for success.

### 2.3 Two scripts referenced by the docs were not in the repository

`README.md` and `docs/BUILD_BASELINE.md` told the reader to use
`tools/dev_env.ps1` and `tools/setup_portable_toolchain.ps1`. Both existed only in
the parent workspace, not in the fork. The documented workflow was broken.

*Fix:* both scripts are now shipped in the fork.

### 2.4 `fetch_model.ps1` misread the catalogue

The script looked for `source.repo_id`; in `catalog.json` the model **`id` is the
HuggingFace repo id** and `revision` is a top-level field. The Rust `ModelSource`
enum is derived from those at load time and is not stored in the JSON.

*Fix:* read `id` and `revision` directly.

### 2.5 A fragile dev-server interaction

The Vite dev server watches the project root. The file-editing tool writes a
temporary directory next to the target file; Vite tried to watch it, hit
`EBUSY`, and **exited**, taking the running UI's backend with it.

*Mitigation:* GUI validation uses the release build, which has no watcher. This is
recorded because anyone developing this fork with `bun run dev` will hit it.

## 3. How each Phase 3 goal was approached

| Goal | Approach | Why |
|---|---|---|
| Regression tests | Run the full crate and application suites | Establishes a baseline before any change |
| **Real ASR acceptance** | Windows SAPI **TTS** generates Traditional-Chinese WAVs with a known ground truth; the model transcribes them headlessly; output is scored against the reference | A microphone needs a human. TTS gives real audio and real inference with an exact reference. Synthetic speech is **easier** than a human, so the resulting error rate is a best case and is labelled as such |
| GUI acceptance | Windows **UI Automation** to find and invoke real controls, plus **CDP** (WebView2 remote debugging) to read rendered state and drive the page | UIA alone cannot read arbitrary rendered text or emit an application event; CDP alone would bypass the accessibility layer. Together they cover both |
| Event-bridge acceptance | Emit the genuine `stream-text-event` / `history-update-payload` payloads into the running app through Tauri's own IPC, then observe the UI | The microphone path cannot be fed on this machine, but the *bridge* can be exercised with the real event names and payload shapes |
| OS credential store | Dedicated `#[ignore]`d tests against the real Credential Manager, scoped to a test namespace | Machine-state mutation belongs behind an explicit flag; a separate service name makes it impossible to touch the application's real key |
| JSON export | Drive the export and validate the file against the published schema; check UTF-8, CJK integrity, and absence of secrets | Also verify the transcript is unchanged by exporting |
| API upload | A loopback HTTP server records the exact bytes; the app uploads to it | Controlled reference. **Explicitly not** a third-party API |
| Release readiness | Release build (embeds `dist/`), launch, re-verify | A debug build is not what a user runs |

## 4. What this phase deliberately does **not** claim

* **Microphone capture is not tested.** No loopback or virtual audio device exists
  on this machine; installing a virtual cable driver would need administrator
  rights and would alter the user's audio configuration. A human speaking into a
  real microphone is required. This is recorded as **BLOCKED**, not as passing.
* **No production third-party API is contacted.** Without a configured endpoint
  and an explicit authorisation to send data to it, no external request is made.
* **TTS is not a human voice.** Synthetic, studio-clean speech. Any accuracy
  figure is an upper bound.
* **CDP-driven interaction is not a human clicking.** It dispatches real events
  into the real running application, which invokes real registered commands; but
  it is automation, and it is described as such.

## 5. Order of work

1. Re-verify repository, process state and toolchain.
2. Run both regression suites.
3. Generate the TTS corpus with a known ground truth.
4. Fetch and SHA-256-verify the Breeze-ASR-25 model from the catalogue.
5. Run real ASR over the corpus; score against the reference.
6. Verify the real OS credential store; fix the backend defect and add read-back
   verification.
7. Build in release mode; launch the release artifact.
8. Drive the GUI: navigate, configure the endpoint, set a key, test the
   connection, view the transcript, export JSON, upload.
9. Inject the real recognition events and confirm the transcript updates.
10. Re-run every suite; write the test report.
