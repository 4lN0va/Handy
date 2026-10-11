# Build Baseline

Record of what was actually executed, on what machine, with what result.

**Status: the application builds and runs.** This document also keeps the
Phase 1 history, because the reason the build was blocked — and the two
locale-dependent defects that had to be fixed to unblock it — is the most useful
part for anyone reproducing this.

---

## 1. Machine and environment

| Item | Value |
|---|---|
| OS | Windows 10.0.19045 (Windows 10 22H2) |
| CPU architecture | AMD64 (x86_64), 16 logical cores |
| Administrator | **No** |
| GPU | NVIDIA GeForce RTX 5060 Ti, 1 Vulkan device |
| Shell | PowerShell (`pwsh`) |

### Toolchain

| Component | Version | Notes |
|---|---|---|
| Rust (default) | 1.98.0 `stable-x86_64-pc-windows-msvc` | used for the application build |
| Rust (secondary) | 1.99.0 `stable-x86_64-pc-windows-gnu` | per-user, used to test the portable crate |
| Visual Studio Build Tools | 2022 17.14.37710, `isComplete: true` | `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` present |
| MSVC toolset | 14.44.35207 | `cl.exe` / `link.exe` |
| Windows SDK | 10.0.26100.0 | `Include`, `Lib`, `bin` |
| CMake | 4.4.4 | for `ggml` / `transcribe-cpp` |
| Ninja | 1.13.2 | CMake generator |
| Vulkan SDK | 1.4.363.0 | provides `glslc` for `ggml-vulkan` |
| Bun | 1.4.3 | frontend |
| ONNX Runtime | downloaded by `ort-sys` | MSVC target only (see §5) |

Everything is per-user except the VS Build Tools and the Vulkan SDK, which
required elevation.

## 2. Upstream baseline

```
repository    https://github.com/cjpais/Handy
checked out   handy-fork/            (full history, not a shallow clone)
upstream HEAD 48a3d8e9ee456c1c6a3b7223779271c457890b4d   (2026-10-10)
latest tag    v0.9.8
baseline tag  upstream-baseline-v0.9.8
dev branch    feature/voice-api-mvp
```

Commit identity for this fork is set **locally** in `handy-fork/.git/config`; the
user's global git identity is untouched.

## 3. Build-environment problems, and how they were fixed

Three of these are **environment defects, not code defects**, and all three are
recorded in `tools/msvc_env.ps1` because they recur on any non-English Windows.
This is the part of the document worth reading.

### 3.1 Ninja cannot parse a localised `cl.exe` (`VSLANG`)

Symptom:

```
ninja: build stopped: subcommand failed.
```

accompanied by thousands of `注意: 包含檔案:` lines. CMake+Ninja drives `cl.exe`
with `/showIncludes` and parses the resulting prefix to discover header
dependencies. A Chinese-locale toolchain localises that prefix; Ninja stops
recognising it, and the failure is reported as a bare build stop that looks like a
compiler error but is a dependency-scanning failure.

**Fix: `VSLANG=1033`.**

### 3.2 A UTF-8 source file decoded as GBK (`/utf-8`)

With the above fixed, the real first error appeared:

```
transcribe-cpp-sys-0.3.1\src\arch\canary\model.cpp(670): error C2065: 'sp_space_id': undeclared identifier
transcribe-cpp-sys-0.3.1\src\arch\canary\model.cpp(673): error C3927: '->': trailing return type ...
```

The code at those lines is valid. The cause is line 667: the file is UTF-8
**without a BOM** and contains a literal `▁` (U+2581). Without `/utf-8`, MSVC
decodes source using the system ANSI codepage; under GBK/936 the UTF-8 bytes of
that character pair with the following bytes, the comment swallows real code, and
the compiler emits a cascade of nonsense.

Upstream CI builds on English-locale runners where the ANSI codepage is CP1252:
the same three bytes decode as three single-byte characters and consume nothing,
so the defect is invisible there.

**Fix: `-DCMAKE_C_FLAGS=/utf-8 -DCMAKE_CXX_FLAGS=/utf-8` via `CMAKE_ARGS`.**

### 3.3 Missing Vulkan SDK

```
CMake Error: Could NOT find Vulkan (missing: Vulkan_LIBRARY Vulkan_INCLUDE_DIR glslc)
```

**Fix: `winget install KhronosGroup.VulkanSDK`** (1.4.363.0). The installer
requests elevation; it completed successfully here.

### 3.4 Stale CMake cache when changing generator

After switching to Ninja, CMake refused with *"generator: Ninja does not match the
generator used previously"*. The transient cache lives at
`%LOCALAPPDATA%\tcs\<hash>\build` and had to be cleared once.

## 4. Proof the toolchain links

Before touching the application, a throwaway crate with `serde` + `serde_json`
(so build scripts and proc-macros are exercised):

```
$ cargo run --quiet
{"text":"你好，我想測試語音辨識。"}
run exit=0
```

## 5. History: the Phase 1 blocker

Kept because it explains the two fixes above, and because the second half is still
true.

* **MSVC route (was blocked, now resolved):** `link.exe` was absent, and
  installing Build Tools requires administrator rights.
  `cargo build` failed with `error: linker 'link.exe' not found`. Resolved by the
  user installing VS Build Tools 2022.
* **GNU route (blocked, permanently):**

  ```
  error: ort-sys: ort does not provide prebuilt binaries for the target
         `x86_64-pc-windows-gnu` with feature set (no features).
  ```

  Verified against ONNX Runtime's own release assets (`v1.31.0`):
  `onnxruntime-win-x64`, `-win-arm64`, `-win-arm64x`, and two CUDA variants — no
  `gnu`/`mingw` asset exists. The ONNX-backed engines are not optional, so the GNU
  target cannot build this application. This is an upstream packaging decision,
  not a local misconfiguration, and it will not change.

## 6. Portable per-user toolchain

Still used, for the reason in §8: the `handy-voice-api` crate is ordinary portable
Rust and can be built and tested **without** the GUI toolchain, which makes it
verifiable on machines that cannot build the application at all.

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File tools/setup_portable_toolchain.ps1
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
rustup component add --toolchain stable-x86_64-pc-windows-gnu rustfmt clippy
```

One non-obvious fix is required: w64devkit's GCC 16 ships only `libgcc.a` with the
exception-handling routines folded in, and no separate `libgcc_eh.a`, which Rust's
`windows-gnu` target always requests. Creating an empty archive
(`ar rcs libgcc_eh.a`) resolves it.

## 7. Building and running the application

```powershell
. .\tools\msvc_env.ps1                       # imports vcvars64 + VSLANG + /utf-8
$env:PATH = 'C:\Users\Nova\toolchain\ninja;C:\Users\Nova\toolchain\bun\bun-windows-x64;' + $env:PATH
$env:CMAKE_GENERATOR = 'Ninja'
$env:CMAKE_MAKE_PROGRAM = 'C:\Users\Nova\toolchain\ninja\ninja.exe'
$env:CARGO_TARGET_DIR = 'C:\Users\Nova\t'
cd handy-fork
bun install
bun run build                                # produces dist/, embedded by tauri-build
cargo build --manifest-path src-tauri/Cargo.toml
C:\Users\Nova\t\debug\handy.exe --debug
```

A short `CARGO_TARGET_DIR` is recommended by upstream `BUILD.md` to stay clear of
Windows' 260-character path limit during the nested CMake builds.

`src-tauri` is standalone — there is no workspace — and the fork's crate is pulled
in as a path dependency, so `--manifest-path src-tauri/Cargo.toml` is the correct
invocation.

Run the app **with the working directory set to `src-tauri`** if you want
`tauri-specta` to regenerate `src/bindings.ts` into the repository: the export path
is `../src/bindings.ts`, resolved relative to the process working directory.

## 8. What is verified, and how

| # | Claim | Command | Result |
|---|---|---|---|
| 1 | MSVC toolchain resolves | `where cl` / `where link` inside `vcvars64` | ✅ |
| 2 | MSVC links real code | `cargo run` on a `serde` probe | ✅ |
| 3 | **Full application builds** | `cargo build --manifest-path src-tauri/Cargo.toml` | ✅ `Finished dev profile in 42.26s` |
| 4 | **Application launches** | `handy.exe --debug` | ✅ running, window titled "Handy" |
| 5 | Vulkan backend initialises | startup log | ✅ RTX 5060 Ti detected, `Vulkan0, CPU` |
| 6 | Model catalog loads | startup log | ✅ `Seeded 69 catalog model(s)` |
| 7 | History DB migrates | startup log | ✅ `Database migrated from version 0 to 4` |
| 8 | `handy-voice-api` tests | `cargo test --all-features` | ✅ 178 unit + 11 integration + 1 doctest |
| 9 | Application crate tests | `cargo test --manifest-path src-tauri/Cargo.toml --lib` | ✅ 294 passed, 0 failed, 16 ignored |
| 10 | Formatting | `cargo fmt --check` | ✅ |
| 11 | Lints | `cargo clippy --all-targets --all-features -- -D warnings` | ✅ |
| 12 | Front end | `bun run build` | ✅ 2129 modules |
| 13 | **Microphone capture** | — | ❌ **not tested** |
| 14 | **Real ASR accuracy / latency** | — | ❌ **not tested** |
| 15 | **GUI click-through** | — | ❌ **not tested** |
| 16 | Installer bundling (`tauri build`) | — | ❌ **not attempted** (needs the signing setup) |
| 17 | Any platform other than Windows x86_64 | — | ❌ **not tested** |

Rows 13–17 are the honest gaps. **Nothing in this repository claims they work.**

See `PHASE2_TEST_REPORT.md` for the full breakdown, including which results come
from a controlled loopback test backend rather than a production API.

## 9. Cross-platform status

Upstream's CI matrix is a statement about **upstream**, not about this fork. No
platform other than Windows x86_64 has been built or run here.

| Target | Upstream CI builds it | This fork verified |
|---|:---:|:---:|
| macOS Apple Silicon (aarch64) | ✅ | ❌ not tested |
| macOS Intel (x86_64) | ✅ | ❌ not tested |
| **Windows x86_64** | ✅ | ✅ **built, launched, tests pass** |
| Windows ARM64 | ✅ | ❌ not tested |
| Linux x86_64 | ✅ | ❌ not tested |
| Linux ARM64 | ✅ | ❌ not tested |

CI added by this fork (`.github/workflows/voice-api.yml`) runs the portable
crate's tests, clippy and formatting on **Linux, macOS and Windows** — real
cross-platform evidence for the voice-API layer, and the cheapest way to keep it
verified. It does not build the application.

## 10. Environment deviations worth knowing

* The checkout path contains a space (`C:\Users\Nova\Project Brief`). Build
  artefacts are written outside the repository to `C:\Users\Nova\t`.
* The Vulkan SDK's installer sets `VULKAN_SDK` at **machine** scope, so an
  already-running shell will not see it. `tools/msvc_env.ps1` recovers it
  explicitly rather than silently building without a GPU backend.
* `cl.exe` and `link.exe` are only available inside the developer environment;
  `tools/msvc_env.ps1` imports `vcvars64.bat` into the current session and does not
  modify any persistent user setting.
