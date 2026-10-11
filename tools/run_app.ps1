# run_app.ps1
#
# Builds (only if needed) and launches the Handy fork with its front end.
#
# WHY A SCRIPT IS NEEDED
#   A debug Tauri build loads `devUrl` (http://localhost:1420), NOT the bundled
#   `dist/`. Launching handy.exe on its own therefore opens a window that shows
#   "localhost refused to connect" - the process looks alive but the UI never
#   renders. The Vite dev server has to be running first. This script starts it,
#   waits for it to answer, then launches the app.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tools/run_app.ps1
#   powershell -ExecutionPolicy Bypass -File tools/run_app.ps1 -DebugUi   # CDP + accessibility
#   powershell -ExecutionPolicy Bypass -File tools/run_app.ps1 -NoBuild
#   powershell -ExecutionPolicy Bypass -File tools/run_app.ps1 -Release

param(
    [switch]$Release,      # launch the release binary instead of the debug one
    [switch]$NoBuild,      # skip the cargo build step
    [switch]$DebugUi,      # enable WebView2 DevTools + accessibility tree
    [int]$Port = 1420
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$TargetDir = 'C:\Users\<you>\t\handy-msvc'
$Profile = if ($Release) { 'release' } else { 'debug' }
$Exe = Join-Path $TargetDir "$Profile\handy.exe"
$ViteUrl = "http://localhost:$Port"

Write-Host "Handy fork launcher" -ForegroundColor Cyan
Write-Host "  repository : $RepoRoot"
Write-Host "  profile    : $Profile"
Write-Host "  executable : $Exe"
Write-Host ""

# --- 0. refuse to double-launch: the app is single-instance and a second
#        launch would just forward arguments to the running one -------------
$existing = Get-Process -Name 'handy' -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "Handy is already running (PID $($existing[0].Id))." -ForegroundColor Yellow
    Write-Host "Close it first, or use tools/ui_probe.ps1 to drive the running window."
    exit 0
}

# --- 1. bring the MSVC environment in ---------------------------------------
if (-not $NoBuild) {
    Write-Host "[1/4] loading the MSVC environment..." -ForegroundColor Cyan
    . (Join-Path $PSScriptRoot 'msvc_env.ps1')
    $tc = 'C:\Users\<you>\toolchain'
    $env:PATH = "$tc\ninja;$tc\bun\bun-windows-x64;$env:PATH"
    $env:CMAKE_GENERATOR = 'Ninja'
    $env:CMAKE_MAKE_PROGRAM = "$tc\ninja\ninja.exe"
    $env:CARGO_TARGET_DIR = $TargetDir
} else {
    # Even when skipping the build we still need bun on PATH.
    $env:PATH = "C:\Users\<you>\toolchain\bun\bun-windows-x64;$env:PATH"
}

# --- 2. make sure the front end is built ------------------------------------
Push-Location $RepoRoot
try {
    if (-not (Test-Path (Join-Path $RepoRoot 'node_modules'))) {
        Write-Host "[2/4] installing front-end dependencies (first run)..." -ForegroundColor Cyan
        bun install
    } else {
        Write-Host "[2/4] front-end dependencies present" -ForegroundColor Cyan
    }

    if (-not $NoBuild) {
        Write-Host "[3/4] building the application (incremental)..." -ForegroundColor Cyan
        if ($Release) {
            cargo build --release --manifest-path src-tauri/Cargo.toml
        } else {
            cargo build --manifest-path src-tauri/Cargo.toml
        }
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    } else {
        Write-Host "[3/4] build skipped (-NoBuild)" -ForegroundColor Cyan
    }
} finally {
    Pop-Location
}

if (-not (Test-Path $Exe)) { throw "executable not found: $Exe" }

# --- 3. start the Vite dev server, unless it is already answering -----------
$viteAlreadyUp = $false
try {
    $r = Invoke-WebRequest -Uri $ViteUrl -TimeoutSec 3 -UseBasicParsing
    $viteAlreadyUp = $r.StatusCode -eq 200
} catch { $viteAlreadyUp = $false }

if ($viteAlreadyUp) {
    Write-Host "[4/4] Vite dev server already running on $ViteUrl" -ForegroundColor Cyan
} else {
    Write-Host "[4/4] starting the Vite dev server on $ViteUrl ..." -ForegroundColor Cyan
    Start-Process -FilePath 'bun' -ArgumentList 'run', 'dev' -WorkingDirectory $RepoRoot -WindowStyle Hidden

    $deadline = (Get-Date).AddSeconds(60)
    $up = $false
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 700
        try {
            $r = Invoke-WebRequest -Uri $ViteUrl -TimeoutSec 2 -UseBasicParsing
            if ($r.StatusCode -eq 200) { $up = $true; break }
        } catch { }
    }
    if (-not $up) {
        throw "the Vite dev server did not come up on $ViteUrl within 60s. The app would show a network error page without it."
    }
    Write-Host "      Vite is answering." -ForegroundColor Green
}

# --- 4. launch --------------------------------------------------------------
if ($DebugUi) {
    # WebView2 exposes no DOM to accessibility or DevTools unless this is set.
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222 --force-renderer-accessibility'
    Write-Host "      WebView2 DevTools on http://127.0.0.1:9222 (tools/cdp_drive.mjs)" -ForegroundColor DarkGray
}

Write-Host ""
Write-Host "launching $Exe ..." -ForegroundColor Cyan
# Working directory = src-tauri so tauri-specta can regenerate ../src/bindings.ts
$proc = Start-Process -FilePath $Exe -ArgumentList '--debug' -PassThru -WorkingDirectory (Join-Path $RepoRoot 'src-tauri')

Start-Sleep -Seconds 12
$alive = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
if ($alive) {
    Write-Host "running: PID $($proc.Id)  window '$($alive.MainWindowTitle)'  $([math]::Round($alive.WorkingSet64/1MB,1)) MB" -ForegroundColor Green
    Write-Host ""
    Write-Host "The model may take ~25s on the first transcription (Vulkan shader compilation)." -ForegroundColor DarkGray
} else {
    Write-Host "the process exited immediately (exit code $($proc.ExitCode))" -ForegroundColor Red
    Write-Host "check $env:LOCALAPPDATA\com.pais.handy\logs\handy.log"
    exit 1
}
