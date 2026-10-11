# Fork maintenance and building on GitHub

This repository is a personal fork of [`cjpais/Handy`](https://github.com/cjpais/Handy).
It is **not** intended to be contributed back upstream. This document records why
it is a fork rather than a fresh repository, how it is laid out, and what has to
change before GitHub Actions will build it cleanly on all three platforms.

---

## 1. Why a fork and not a clean repository

The deciding factor is **cross-platform building**.

Handy's native layer is not trivial: `transcribe-cpp` (ggml) with Vulkan, Metal
and CPU backends, plus ONNX Runtime for the `transcribe-rs` engines. Getting that
to compile and link on macOS, Linux **and** Windows is the single hardest part of
this project, and the upstream CI already does it:

| Target | Upstream CI |
|---|---|
| `aarch64-apple-darwin`, `x86_64-apple-darwin` | ✅ |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | ✅ |
| `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | ✅ |

A fresh repository would throw that away and require re-authoring a three-platform
native build from scratch. Keeping the fork means those workflows are inherited
and only need the changes listed in §4.

Supporting reasons:

* **The delta is small and stays reviewable.** 70 files differ from upstream, of
  which only 16 are *modifications*; nothing is deleted. `git diff upstream/main`
  is a legible summary of everything this fork does.
* **Upstream fixes come for free.** `git merge upstream/main` is a normal merge
  with a small, known conflict surface (§5).
* **Attribution stays unambiguous.** Handy is MIT. Keeping the upstream history and
  `LICENSE` in place means the licence obligations are satisfied structurally
  rather than by remembering to restate them.

The only thing a clean repository would buy is a tidier commit history, which is
not worth losing a working three-platform build for.

## 2. Repository layout

`origin` in a clone made from upstream points at **`cjpais/Handy`**, not at this
fork. That has to be corrected first, or work is never pushed anywhere.

```bash
# `origin` is really upstream; rename it to say so.
git remote rename origin upstream

# Then this fork becomes origin.
git remote add origin https://github.com/<your-account>/Handy.git

# The product branch.
git checkout main
git merge --no-ff feature/voice-api-mvp -m "Merge fork features into main"
git push -u origin main

# A read-only local branch for merges, so the fork's main never gets confused
# with upstream's.
git branch upstream-main upstream/main
```

Resulting shape:

| Ref | Purpose |
|---|---|
| `origin/main` | this fork's product branch — what you actually run |
| `upstream/main` | read-only mirror of `cjpais/Handy` |
| `upstream-main` | local convenience branch tracking `upstream/main` |

## 3. Optional: taking upstream fixes

Not required, since nothing goes back upstream, but useful when a bug is fixed
upstream that you also care about.

```bash
git fetch upstream
git checkout main
git merge upstream/main
```

Conflicts can only appear in the 16 modified files listed in §5. Everything else
this fork adds is new files, which cannot conflict.

## 4. Before GitHub Actions will be clean

Pushing to `main` currently triggers **six** workflows, three of which come from
upstream and were written for a repository that has signing secrets and Nix
infrastructure.

| Workflow | Trigger | Needs secrets | On this fork |
|---|---|---|---|
| `voice-api.yml` | push / PR / dispatch | no | ✅ runs; added by this fork |
| `test.yml` | push / PR / dispatch | no | ✅ runs |
| `code-quality.yml` | push / PR / dispatch | no | ✅ runs |
| `playwright.yml` | PR / dispatch | no | ⚠️ upstream's front-end E2E; may not match this fork's UI |
| `main-build.yml` | **push** | no | ⚠️ builds all three platforms unsigned — the useful one |
| `nix-check.yml` | push / PR / dispatch | **yes** | ❌ fails without Nix secrets |
| `build.yml` | dispatch / release | **yes** | ❌ fails without signing secrets |
| `release.yml` | dispatch only | no | ✅ will not publish by itself |
| `app-build.yml` | dispatch only | no | ⚠️ added by this fork, **never executed** |

Suggested cleanup:

1. **Delete or disable** `nix-check.yml` and `build.yml`. Neither can pass without
   secrets this fork does not have, and a permanently red Actions tab trains you
   to ignore it.
2. **Keep** `main-build.yml` as the three-platform build check. It needs no
   secrets, so it produces unsigned builds on `macos-latest`, `ubuntu-22.04` and
   `windows-latest`.
3. **Keep** `voice-api.yml`, which tests the portable `handy-voice-api` crate,
   formatting and clippy on all three platforms.
4. **Decide about** `app-build.yml`. It is this fork's own Windows-only build
   workflow and it has **never been run**. Either delete it in favour of
   `main-build.yml`, or exercise it once via *Actions → app build (manual) → Run
   workflow* and fix what breaks.

> **These workflows have not been executed by the author of this document.** The
> statements above come from reading their trigger blocks and `secrets` usage, not
> from a green run. Treat the first push as the real test.

## 5. The upstream conflict surface

These 16 files are modifications of upstream code. Any future upstream change to
one of them can conflict; everything else this fork adds is new and cannot.

| File | Change |
|---|---|
| `src-tauri/Cargo.toml` | `reqwest` `blocking` feature, `url`, the `handy-voice-api` path dependency |
| `src-tauri/Cargo.lock` | resolved from the above |
| `src-tauri/src/lib.rs` | module declarations, manager construction, command registration |
| `src-tauri/src/managers/mod.rs` | two new modules |
| `src-tauri/src/managers/audio.rs` | speech-activity observer + its accessor |
| `src-tauri/src/shortcut/handler.rs` | continuous-mode interception before the coordinator |
| `src-tauri/src/commands/mod.rs` | one new module |
| `src-tauri/src/catalog/catalog.json` | one model's `recommended` flag set to `false` (unconfirmed licence) |
| `src-tauri/src/secure_input.rs` | removed an unused import |
| `src/bindings.ts` | regenerated by `tauri-specta` |
| `src/components/Sidebar.tsx` | sidebar sections |
| `src/components/settings/index.ts` | component exports |
| `src/i18n/locales/en/translation.json` | new strings |
| `src/i18n/locales/zh-TW/translation.json` | new strings |
| `README.md`, `CONTRIBUTING.md`, `.gitignore` | fork notes, ignore rules |

`src-tauri/src/actions.rs` was modified during development and has since been
restored to upstream, so it is **not** part of the surface.

## 6. Portability of this fork's additions

Checked, not assumed. The three modules this fork adds contain **zero**
platform-conditional code — no `cfg(target_os)`, no shelling out, no Windows-only
paths:

| Module | Platform-specific lines |
|---|---|
| `src-tauri/src/bridge.rs` | 0 |
| `src-tauri/src/managers/continuous.rs` | 0 |
| `src-tauri/src/managers/speech_activity.rs` | 0 |
| `src-tauri/src/commands/continuous.rs` | 0 |

The one genuinely platform-sensitive addition is the OS credential store, and it
is deliberately set up for all three: `keyring` is declared with
`apple-native`, `windows-native` and `sync-secret-service` together. That is safe
because `keyring` places each backend's dependency under
`[target.'cfg(target_os = …)'.dependencies]`, so only the relevant one is
compiled.

**Not verified:** this fork has only ever been built and run on Windows x86_64.
Nothing in it *should* be platform-specific, but that is a reading of the code and
a check of the module contents, not a macOS or Linux build.

## 7. Building

### Windows, locally

Three environment problems must be handled first; see
[`BUILD_BASELINE.md`](BUILD_BASELINE.md) §3 for the full explanations.

```powershell
. .\tools\msvc_env.ps1                      # vcvars64 + VSLANG=1033 + /utf-8
$env:CMAKE_GENERATOR = 'Ninja'

bun install
bun run build                               # produces dist/
cargo build --manifest-path src-tauri/Cargo.toml
```

`VSLANG=1033` and `CMAKE_ARGS=-DCMAKE_C_FLAGS=/utf-8 -DCMAKE_CXX_FLAGS=/utf-8` are both **required on
a non-English Windows**; without them the native build fails with messages that do
not point at the real cause.

### Running it

```powershell
powershell -ExecutionPolicy Bypass -File tools\run_app.ps1 -NoBuild
```

`run_app.ps1` starts the Vite dev server first, because a **debug** Tauri build
loads `devUrl`, not the bundled `dist/`. Without the dev server the window opens
on a network-error page and Tauri then rejects every IPC call with
`Origin header is not a valid URL`, which looks like a broken command rather than
a missing front end.

### All three platforms

Use upstream's `main-build.yml` (push to `main`) or run it manually from the
Actions tab. It builds unsigned installers for macOS, Linux and Windows.

## 8. Documentation map

| Document | Contents |
|---|---|
| [`BUILD_BASELINE.md`](BUILD_BASELINE.md) | what was built, on what machine, and the two locale-dependent environment fixes |
| [`PHASE3_TEST_REPORT.md`](PHASE3_TEST_REPORT.md) | real-world validation: six runtime defects, with evidence |
| [`PHASE4_CONTINUOUS_INTEGRATION.md`](PHASE4_CONTINUOUS_INTEGRATION.md) | continuous mode and the verification-tool bridge |
| `PHASE2_*.md` | the earlier transcript / export / upload work |
| [`../SECURITY.md`](../SECURITY.md) | threat model and its limits |
| [`../MODEL_LICENSES.md`](../MODEL_LICENSES.md), [`../THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md) | generated licence data |
