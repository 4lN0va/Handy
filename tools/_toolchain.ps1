# _toolchain.ps1 — locates the per-user portable toolchain, for every tool script.
#
# WHY THIS EXISTS
#   The other scripts in this directory were written against one developer's
#   layout (`%USERPROFILE%\toolchain`, `%USERPROFILE%\t`). Anyone cloning this
#   and building elsewhere has a toolchain in a different place — or a different
#   kind of toolchain entirely — so a hardcoded path is not a personal detail to
#   scrub, it is a deployment blocker.
#
#   Every location is therefore resolved here, once, with an environment-variable
#   override, and reported clearly when it is missing instead of failing later
#   with a confusing error.
#
# OVERRIDES
#   HANDY_TOOLCHAIN   Root holding the portable tools. Default: %USERPROFILE%\toolchain
#   HANDY_TARGET_DIR  Cargo target directory.          Default: %USERPROFILE%\t
#
# LAYOUT EXPECTED UNDER HANDY_TOOLCHAIN
#   w64devkit\w64devkit\bin\  MinGW-w64 gcc   (GNU target linking)
#   cmake\**\cmake.exe                        (if you want CMake on PATH)
#   ninja\**\ninja.exe
#   bun\**\bun.exe
#   Searched recursively, so the exact extraction folder does not matter.
#
# WHY A SHORT TARGET DIR
#   Windows' 260-character MAX_PATH is overflowed by nested CMake builds under
#   `target\<triple>\build\<crate>-<hash>\out\...`. BUILD.md recommends a short
#   CARGO_TARGET_DIR for the same reason.
#
# USAGE
#   . "$PSScriptRoot\_toolchain.ps1"

$ErrorActionPreference = 'Stop'

$HandyToolchainRoot = if ($env:HANDY_TOOLCHAIN) { $env:HANDY_TOOLCHAIN }
                      else { Join-Path $env:USERPROFILE 'toolchain' }

$HandyTargetDir = if ($env:HANDY_TARGET_DIR) { $env:HANDY_TARGET_DIR }
                  else { Join-Path $env:USERPROFILE 't' }

# First match under a toolchain subdirectory, or $null. Recursive because the
# extraction folder name depends on the version archive that was unpacked.
function Get-HandyTool {
    param([Parameter(Mandatory)][string]$Subdir, [Parameter(Mandatory)][string]$Exe)
    $root = Join-Path $HandyToolchainRoot $Subdir
    if (-not (Test-Path $root)) { return $null }
    Get-ChildItem $root -Recurse -Filter $Exe -ErrorAction SilentlyContinue |
        Select-Object -First 1
}

# Explain what is missing and how to fix it, rather than letting a later command
# fail with "not found" and no context.
function Assert-HandyToolchain {
    param([string[]]$Requires = @('w64devkit'))

    if (Test-Path $HandyToolchainRoot) { return }

    $hint = @(
        "Portable toolchain not found at: $HandyToolchainRoot"
        ""
        "Either run tools/setup_portable_toolchain.ps1 to fetch one, or point at an"
        "existing installation with an environment variable:"
        ""
        "    `$env:HANDY_TOOLCHAIN = 'D:\somewhere\toolchain'"
        ""
        "Expected layout (searched recursively, so folder names may vary):"
    ) + ($Requires | ForEach-Object { "    $_" }) -join "`n"

    throw $hint
}

Write-Verbose "toolchain root : $HandyToolchainRoot"
Write-Verbose "target dir     : $HandyTargetDir"
