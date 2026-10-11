# dev_env.ps1 — reproducible local build environment for the Handy fork.
#
# WHY THIS EXISTS
#   The official Windows build path (BUILD.md) requires Microsoft C++ Build
#   Tools (link.exe + Windows SDK), which needs administrator rights. This
#   machine has neither MSVC nor admin. This script wires up a *per-user,
#   portable* toolchain instead so that portable Rust crates can be compiled and
#   tested for real.
#
# WHAT IT PROVIDES
#   - x86_64-pc-windows-gnu linker (MinGW-w64 gcc from w64devkit)
#   - CMake + Ninja (portable) and Bun (portable), for when a native build
#     becomes possible
#
# WHEN TO USE IT INSTEAD OF msvc_env.ps1
#   The full application links with MSVC (see msvc_env.ps1). This script is for
#   the portable crates, which build without a C++ toolchain - useful on a
#   machine that cannot install one, and for a fast test loop that skips the
#   native build entirely.
#
# USAGE
#   . .\tools\dev_env.ps1          # dot-source into the current shell
#   cargo test -p handy-voice-api --target x86_64-pc-windows-gnu

$ErrorActionPreference = 'Stop'

. "$PSScriptRoot\_toolchain.ps1"
$MinGwBin = Join-Path $HandyToolchainRoot 'w64devkit\w64devkit\bin'

Assert-HandyToolchain -Requires @('w64devkit\w64devkit\bin\x86_64-w64-mingw32-gcc.exe')
if (-not (Test-Path $MinGwBin)) {
    throw "MinGW not found under $HandyToolchainRoot. Run tools/setup_portable_toolchain.ps1, or set `$env:HANDY_TOOLCHAIN."
}

$env:PATH = "$MinGwBin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $MinGwBin 'x86_64-w64-mingw32-gcc.exe'

# Short target dir: Windows' 260-char MAX_PATH is overflowed by deep
# `target\<triple>\build\<crate>-<hash>\out\...` trees once native CMake builds
# are involved. BUILD.md recommends a short CARGO_TARGET_DIR for the same
# reason. This is per-shell only and does NOT touch the user's environment.
if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = $HandyTargetDir
}

# Portable CMake/Ninja/Bun, if they were extracted.
$cmakeBin = Get-ChildItem (Join-Path $HandyToolchainRoot 'cmake') -Recurse -Filter 'cmake.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($cmakeBin) { $env:PATH = "$($cmakeBin.DirectoryName);$env:PATH" }
$ninjaExe = Get-ChildItem (Join-Path $HandyToolchainRoot 'ninja') -Recurse -Filter 'ninja.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($ninjaExe) { $env:PATH = "$($ninjaExe.DirectoryName);$env:PATH" }
$bunExe = Get-ChildItem (Join-Path $HandyToolchainRoot 'bun') -Recurse -Filter 'bun.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($bunExe) { $env:PATH = "$($bunExe.DirectoryName);$env:PATH" }

# CMake must use Ninja, NOT the default "MSYS Makefiles" generator.
# w64devkit's make.exe runs through its own sh.exe, which does not translate the
# Windows paths CMake writes into the generated Makefiles, so the compiler probe
# dies with "/C/Users/.../cmake.exe: not found". Ninja invokes the compiler
# directly and has no such problem. These are the names the `cmake` build-script
# crate reads.
if ($ninjaExe) {
    $env:CMAKE_GENERATOR = 'Ninja'
    $env:CMAKE_MAKE_PROGRAM = $ninjaExe.FullName
}

Write-Host "dev_env: MinGW  = $MinGwBin" -ForegroundColor Green
Write-Host "dev_env: CARGOT = $env:CARGO_TARGET_DIR" -ForegroundColor Green
Write-Host ("dev_env: cmake  = " + $(if ($cmakeBin) { $cmakeBin.FullName } else { 'not found' })) -ForegroundColor Green
Write-Host ("dev_env: bun    = " + $(if ($bunExe) { $bunExe.FullName } else { 'not found' })) -ForegroundColor Green
