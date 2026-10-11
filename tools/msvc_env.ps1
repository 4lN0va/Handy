# msvc_env.ps1 — import the Visual Studio Build Tools developer environment.
#
# WHY THIS EXISTS
#   `cl.exe` and `link.exe` are deliberately NOT on the system PATH; they only
#   exist inside a developer command prompt (vcvars64.bat), which also sets
#   INCLUDE, LIB, and the Windows SDK paths. Running cargo without this produces
#   the classic "linker `link.exe` not found" failure.
#
#   This script imports that environment into the *current* PowerShell session so
#   `cargo build` works for the default x86_64-pc-windows-msvc toolchain.
#
# USAGE
#   . .\tools\msvc_env.ps1
#   cargo build
#
# It does NOT modify the user's persistent environment, and it does not change
# the default Rust toolchain.

$ErrorActionPreference = 'Stop'

$vsRoot = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools'
$vcvars = Join-Path $vsRoot 'VC\Auxiliary\Build\vcvars64.bat'

if (-not (Test-Path $vcvars)) {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $found = & $vswhere -latest -products '*' `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -format value -property installationPath 2>$null
        if ($found) {
            $vsRoot = ($found | Select-Object -First 1).Trim()
            $vcvars = Join-Path $vsRoot 'VC\Auxiliary\Build\vcvars64.bat'
        }
    }
}

if (-not (Test-Path $vcvars)) {
    throw "vcvars64.bat not found. Install the 'Desktop development with C++' workload (Microsoft.VisualStudio.Component.VC.Tools.x86.x64) in Visual Studio Build Tools."
}

# vcvars64 prints a banner and sets many variables. `set` dumps the resulting
# environment, which we parse back into this session.
$dump = cmd /c "call `"$vcvars`" >nul 2>&1 && set"
if ($LASTEXITCODE -ne 0) { throw "vcvars64.bat failed (exit $LASTEXITCODE)" }

# Force the MSVC toolchain to emit ENGLISH diagnostics.
#
# This is not cosmetic. CMake+Ninja drives cl.exe with `/showIncludes` and parses
# the resulting lines to discover header dependencies. cl.exe localises that
# prefix ("注意: 包含檔案:" in a Chinese locale), Ninja does not recognise it, and
# the build dies with a bare "ninja: build stopped: subcommand failed" that looks
# like a compiler error but is a dependency-scanning failure. VSLANG=1033 pins the
# toolchain's messages to en-US regardless of the Windows display language.
$env:VSLANG = '1033'
$dump += "VSLANG=1033"

# Tell cl.exe that source files are UTF-8.
#
# Also not cosmetic. Without `/utf-8`, MSVC decodes source files using the system
# ANSI codepage. transcribe-cpp's `src/arch/canary/model.cpp` is UTF-8 *without a
# BOM* and contains a literal U+2581 in a comment
# (`// ... = ▁`). Under a double-byte codepage such as GBK/936 the UTF-8 bytes
# of that character pair with the following characters, the comment swallows
# real code, and the compiler reports a bewildering cascade of syntax errors
# ("'sp_space_id': undeclared identifier", "return type must be preceded by
# '->'") in code that is in fact valid.
#
# Upstream CI builds on English-locale runners, where the ANSI codepage is
# CP1252: the same three bytes decode as three single-byte characters and happen
# not to consume anything, so the bug is invisible there. Making the source
# encoding explicit fixes it everywhere.
$env:CMAKE_ARGS = "-DCMAKE_C_FLAGS=/utf-8 -DCMAKE_CXX_FLAGS=/utf-8"

foreach ($line in $dump) {
    $idx = $line.IndexOf('=')
    if ($idx -lt 1) { continue }
    $name = $line.Substring(0, $idx)
    $value = $line.Substring($idx + 1)
    # Skip cmd-internal pseudo-variables that are not real environment entries.
    if ($name -match '^(=C:|PROMPT|_)$') { continue }
    Set-Item -Path "Env:$name" -Value $value -ErrorAction SilentlyContinue
}

# The Vulkan SDK is required to build ggml's Vulkan backend, which Handy enables
# for Windows x86_64. Its installer sets VULKAN_SDK at MACHINE scope, so an
# already-running shell will not see it; recover it explicitly rather than
# silently building without a GPU backend.
if (-not $env:VULKAN_SDK) {
    $env:VULKAN_SDK = [Environment]::GetEnvironmentVariable('VULKAN_SDK', 'Machine')
}
if (-not $env:VULKAN_SDK) {
    $candidate = Get-ChildItem 'C:\VulkanSDK' -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending | Select-Object -First 1
    if ($candidate) { $env:VULKAN_SDK = $candidate.FullName }
}
if ($env:VULKAN_SDK -and (Test-Path (Join-Path $env:VULKAN_SDK 'Bin'))) {
    $env:PATH = "$(Join-Path $env:VULKAN_SDK 'Bin');$env:PATH"
}

$link = Get-Command link.exe -ErrorAction SilentlyContinue
$cl = Get-Command cl.exe -ErrorAction SilentlyContinue
if (-not $link -or -not $cl) {
    throw "vcvars64 ran but cl.exe/link.exe are still not resolvable; the C++ toolset is incomplete."
}

Write-Host "msvc_env: cl   = $($cl.Source)" -ForegroundColor Green
Write-Host "msvc_env: link = $($link.Source)" -ForegroundColor Green
if ($env:VCToolsVersion) { Write-Host "msvc_env: VCToolsVersion = $env:VCToolsVersion" -ForegroundColor Green }
if ($env:WindowsSdkVersion) { Write-Host "msvc_env: WindowsSdkVersion = $env:WindowsSdkVersion" -ForegroundColor Green }
if ($env:VULKAN_SDK) {
    $glslc = Join-Path $env:VULKAN_SDK 'Bin\glslc.exe'
    if (Test-Path $glslc) { Write-Host "msvc_env: VULKAN_SDK = $env:VULKAN_SDK (glslc ok)" -ForegroundColor Green }
    else { Write-Host "msvc_env: VULKAN_SDK = $env:VULKAN_SDK (glslc MISSING!)" -ForegroundColor Yellow }
} else {
    Write-Host "msvc_env: VULKAN_SDK = (not set) - the Vulkan backend will fail to configure" -ForegroundColor Yellow
}
