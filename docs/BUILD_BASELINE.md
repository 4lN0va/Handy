# Build Baseline

Record of what was actually executed, on what machine, with what result.

**Status summary: the upstream application cannot be linked on this machine.** The
reason is environmental (no MSVC toolchain, no administrator rights), not a code
defect. Everything that *can* be verified without that toolchain has been, and
the evidence is below.

---

## 1. Machine and environment

| Item | Value |
|---|---|
| OS | Windows 10.0.19045 (Windows 10 22H2) |
| CPU architecture | AMD64 (x86_64), 16 logical cores |
| Administrator | **No** — `IsInRole(Administrator)` is `false` |
| Free disk at start | ~21 GB on `C:` |
| Shell | PowerShell (`pwsh`) |

### Toolchain present before any setup

```
git      2.54.0.windows.1
rustc    1.98.0 (default toolchain stable-x86_64-pc-windows-msvc)
cargo    1.98.0
rustup   1.29.0
node     v24.17.0
pnpm / npm present
python   3.14.6
winget   present
tar      (Windows bsdtar)
ffmpeg   (Gyan winget build)

cmake            NOT FOUND
ninja            NOT FOUND
cl               NOT FOUND
link             NOT FOUND      <-- see blocker, section 3
vswhere          NOT FOUND
bun              NOT FOUND
Windows Kits\10  NOT FOUND      (no Windows SDK)
Visual Studio    NOT FOUND      (no VS / Build Tools installation)
```

## 2. Upstream baseline

```
repository   https://github.com/cjpais/Handy
cloned into  handy-fork/             (full history, not a shallow clone)
HEAD         48a3d8e9ee456c1c6a3b7223779271c457890b4d
HEAD date    2026-10-10
latest tag   v0.9.8
commits      883
working tree C:\Users\Nova\Project Brief\handy-fork
```

An immutable baseline tag was created before any edits, so the fork's divergence
is always measurable:

```bash
git tag upstream-baseline-v0.9.8 v0.9.8
git checkout -b feature/voice-api-mvp
```

Commit identity used for this fork is set **locally** in `handy-fork/.git/config`
only (the user's global git identity was deliberately left untouched):

```
user.name  = Handy Voice-API Fork
user.email = dev@localhost
```

## 3. The blocker: no MSVC toolchain, and no rights to install one

Upstream `BUILD.md` states the Windows prerequisites:

> Microsoft C++ Build Tools: Visual Studio 2019/2022 with C++ development tools,
> or Visual Studio Build Tools 2019/2022

Both require an elevated (administrator) install. This account is not an
administrator, and approval prompts are disabled in this session, so the
installation cannot be performed.

This is not a vague "might not work" — it was executed and failed:

```
$ cargo build
   Compiling linkprobe v0.1.0
error: linker `link.exe` not found
  |
  = note: the msvc targets depend on the msvc linker but `link.exe` was not found
  = note: please ensure that Visual Studio 2017 or later, or Build Tools for
          Visual Studio were installed with the Visual C++ option
```

```
$ cargo install cargo-xwin --target x86_64-pc-windows-gnu
error: linker `link.exe` not found
error: failed to compile `cargo-xwin v0.23.1`
```

The second failure is the important one: **even cross-compiling needs a working
host linker**, because `build.rs` scripts and proc-macros are always compiled for
the *host*. On this machine the host ABI is `windows-msvc`.

**Consequence:** the Tauri application (`src-tauri`) cannot be compiled or run
here. No GUI launch, no microphone test, and no model inference could be
performed. Those acceptance criteria remain **unverified**, not "passing".

### 3.1 The GNU target was tried too, and fails for an independent reason

Rather than stop at "no MSVC", the portable MinGW toolchain from section 4 was
used to attempt the full app for `x86_64-pc-windows-gnu`. This got **much further
than expected** — Cargo resolved the whole graph, updated the lockfile, and began
building whisper.cpp through CMake — and then failed for a reason that cannot be
worked around:

```
error: ort-sys@2.0.0-rc.12: ort does not provide prebuilt binaries for the target
       `x86_64-pc-windows-gnu` with feature set (no features).
error: ort-sys@2.0.0-rc.12:  | You may have to compile ONNX Runtime from source
       and link `ort` to your custom build
```

Two independent facts make this terminal:

1. The Parakeet/Moonshine/SenseVoice/Canary/Cohere engines in this application run
   through ONNX Runtime, so `ort-sys` is not an optional dependency.
2. ONNX Runtime publishes **only MSVC-ABI Windows binaries**. Checked against the
   project's own release assets (`v1.31.0`): `onnxruntime-win-x64`,
   `onnxruntime-win-arm64`, `onnxruntime-win-arm64x`, and two CUDA variants —
   **no `gnu` or `mingw` asset exists**.

Building ONNX Runtime from source for the GNU ABI, or swapping in a different
inference backend, would mean changing which engines the product ships. That is
out of scope for a build-environment fix, so the attempt was stopped there.

Two intermediate failures were fixed along the way, and both fixes are recorded
in `tools/dev_env.ps1` because they would recur on any machine taking this route:

* **CMake must use Ninja, not "MSYS Makefiles".** w64devkit's `make.exe` runs
  through its own `sh.exe`, which does not translate the Windows paths CMake
  writes into the Makefiles, so the compiler probe fails with
  `/C/Users/.../cmake.exe: not found`. Setting `CMAKE_GENERATOR=Ninja` fixes it.
* **The transcribe-cpp CMake cache must be cleared when switching generators.**
  It lives at `%LOCALAPPDATA%\tcs\<hash>\build`; a stale `CMakeCache.txt` makes
  CMake refuse with *"generator: Ninja does not match the generator used
  previously"*.

So the two available routes are both closed, for different and independently
verified reasons:

| Route | Outcome | Root cause |
|---|---|---|
| MSVC (`windows-msvc`) | ❌ blocked | `link.exe` absent; installing Build Tools needs administrator rights |
| MinGW (`windows-gnu`) | ❌ blocked | ONNX Runtime ships no GNU-ABI binaries |

## 4. What was done instead: a per-user, portable toolchain

Everything below is installed **per-user with no elevation**, under
`C:\Users\Nova\toolchain`. Reproduce with:

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File tools/setup_portable_toolchain.ps1
```

| Component | Version | Purpose |
|---|---|---|
| w64devkit (MinGW-w64 GCC) | 2.10.0 / GCC 16.2.0 | GNU linker for the `x86_64-pc-windows-gnu` target |
| CMake | 4.4.4 | native dependency builds (whisper.cpp / ggml) |
| Ninja | 1.13.2 | CMake generator |
| Bun | 1.4.3 | frontend install and build |
| LLVM | 23.1.3 | `clang-cl` / `lld-link`, for a future MSVC-ABI path |

Plus a second Rust toolchain, **without changing the default**:

```bash
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
rustup component add --toolchain stable-x86_64-pc-windows-gnu rustfmt clippy
```

`rustup show active-toolchain` still reports
`stable-x86_64-pc-windows-msvc (default)` — the user's default is unchanged.

### 4.1 One non-obvious fix

w64devkit's GCC 16 ships only `libgcc.a`; the exception-handling routines are
folded into it, and there is no separate `libgcc_eh.a`. Rust's
`x86_64-pc-windows-gnu` target always passes `-lgcc_eh`, so linking fails:

```
ld.exe: cannot find -lgcc_eh: No such file or directory
```

Verified that the EH symbols really are present in `libgcc.a`
(`_Unwind_Resume`, `_Unwind_Resume_or_Rethrow`) and created an empty archive
next to it:

```bash
ar rcs libgcc_eh.a          # creates an empty archive
```

### 4.2 Proof the toolchain works

`tools/dev_env.ps1` sets up the environment; a trivial crate then builds, links
and runs:

```
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.53s
$ ./linkprobe.exe
Hello, world!
```

## 5. What was verified, and how

A full table of every claim made about this work.

| # | Claim | Command | Result |
|---|---|---|---|
| 1 | Portable toolchain builds and links a GNU-target binary | `cargo build` (probe crate) then run | ✅ `Hello, world!` |
| 2 | `handy-voice-api` compiles, default features | `cargo +stable-x86_64-pc-windows-gnu build` | ✅ |
| 3 | `handy-voice-api` compiles, all features (reqwest + keyring) | `cargo +stable-x86_64-pc-windows-gnu build --all-features` | ✅ |
| 4 | Unit + integration tests pass | `cargo +stable-x86_64-pc-windows-gnu test --all-features` | ✅ **160 passed, 0 failed** |
| 5 | Documentation example compiles and runs | same command (doctests) | ✅ **1 passed** |
| 6 | Formatting is clean | `cargo fmt --check` | ✅ exit 0 |
| 7 | Lints are clean at deny level | `cargo clippy --all-targets --all-features -- -D warnings` | ✅ exit 0 |
| 8 | Frontend type-checks and builds | `bun install && bun run build` (`tsc && vite build`) | ✅ 2123 modules, `dist/` produced |
| 9 | New Tauri command module parses | `rustfmt --edition 2021 --check src-tauri/src/commands/voice_api.rs` | ✅ exit 0 |
| 10 | Edited `lib.rs` / `commands/mod.rs` still parse | `rustfmt --edition 2021 --check <file>` | ✅ exit 0 |
| 11 | **Full application builds** | `cargo check --manifest-path src-tauri/Cargo.toml` (MSVC and GNU) | ❌ **blocked — see sections 3 and 3.1** |
| 12 | **`src-tauri` type-checks** | as above | ❌ **not verified** |
| 13 | **GUI launches** | — | ❌ **not verified** |
| 14 | **Microphone capture works** | — | ❌ **not verified** |
| 15 | **A real model transcribes Traditional Chinese** | — | ❌ **not verified** |
| 16 | **Upstream's own test suite passes** | `cargo test` in `src-tauri` | ❌ **not verified** |

Rows 11–16 are the honest gaps. **Nothing in this repository claims they work.**

### 5.1 A note on row 8

`bun install` reported one failure:

```
error: ENOENT extracting tarball from @typescript-eslint/eslint-plugin
```

That package is a **lint-time** dependency only. The production build
(`tsc && vite build`) completed successfully afterwards, so the frontend build is
verified while `bun run lint` is **not**. Re-running `bun install` once the
tarball cache settles is the fix.

### 5.2 A note on rows 9–10

`rustfmt` **parses** Rust but does not type-check it. Rows 9–10 therefore prove
the new Tauri code is syntactically valid and correctly formatted. They do **not**
prove it compiles. Type errors in `src-tauri/src/commands/voice_api.rs` remain
possible and unverified.

## 6. Reproducing this environment

```powershell
# 1. Per-user portable toolchain (no admin)
powershell -NoProfile -ExecutionPolicy Bypass -File tools/setup_portable_toolchain.ps1

# 2. GNU host toolchain, leaves the default toolchain untouched
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
rustup component add --toolchain stable-x86_64-pc-windows-gnu rustfmt clippy

# 3. Load the environment into the current shell
. .\tools\dev_env.ps1

# 4. Build and test the voice-API crate
cd handy-fork\crates\handy-voice-api
cargo +stable-x86_64-pc-windows-gnu test --all-features

# 5. Build the frontend
cd ..\..
bun install
bun run build
```

## 7. How to unblock the full build

Any one of these closes the gap. None of them can be done from a non-elevated
session, which is why they are recorded rather than performed.

1. **Install MSVC Build Tools** — this is the only route that can produce a real
   build of this application, because ONNX Runtime has no GNU-ABI Windows
   binaries (section 3.1). It requires administrator rights:

   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override `
     "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```

   Then, per `BUILD.md`: `winget install Kitware.CMake`, `winget install
   KhronosGroup.VulkanSDK`, set `ORT_LIB_LOCATION` to a prebuilt ONNX Runtime
   (`onnxruntime-win-x64-*.zip`), and `bun run tauri dev`.

2. **Build on a CI runner** with the native toolchain — see
   `.github/workflows/` and section 8.

3. **Build on another machine** that already satisfies `BUILD.md`.

> Do not expect the GNU target to become viable later: the ONNX Runtime
> limitation is an upstream packaging decision, not a local misconfiguration.

## 8. Cross-platform status

Nothing here has been built or run on any platform. The table records what the
upstream CI matrix covers, which is a statement about upstream, not about this
fork.

| Target | Upstream CI builds it | This fork verified |
|---|:---:|:---:|
| macOS Apple Silicon (aarch64) | ✅ | ❌ not verified |
| macOS Intel (x86_64) | ✅ | ❌ not verified |
| Windows x86_64 | ✅ | ❌ blocked (no MSVC) |
| Windows ARM64 | ✅ | ❌ not verified |
| Linux x86_64 | ✅ | ❌ not verified |
| Linux ARM64 | ✅ | ❌ not verified |

## 9. Environment deviations worth knowing

* The checkout path contains a space (`C:\Users\Nova\Project Brief`). Some native
  build systems mishandle this. `tools/dev_env.ps1` sets a short
  `CARGO_TARGET_DIR` (`C:\Users\Nova\t`) so build output avoids it, following the
  `CARGO_TARGET_DIR` recommendation in `BUILD.md`.
* Build artifacts are written **outside** the repository, to `C:\Users\Nova\t`,
  so `git status` stays clean and no multi-gigabyte `target/` tree is created
  inside the checkout.
