# fetch_model.ps1
#
# Downloads a speech-recognition model exactly as the catalogue specifies and
# verifies its SHA-256 before it is used.
#
# WHY A SCRIPT
#   The app's own downloader verifies the digest recorded in
#   src-tauri/src/catalog/catalog.json. Installing a model by hand (which upstream
#   README documents for restricted networks) bypasses that, so the same
#   guarantee has to be reproduced here. Placing an unverified multi-hundred-MB
#   binary where the recogniser will load it is exactly the kind of thing that
#   should not be done casually.
#
# The expected digest is read from the catalogue rather than hard-coded, so this
# cannot drift from what the application itself would accept.
#
# Usage:
#   powershell -File tools/fetch_model.ps1 -ModelId "handy-computer/Breeze-ASR-25-gguf" -Quant Q5_K_M

param(
    [string]$ModelId = 'handy-computer/Breeze-ASR-25-gguf',
    [string]$Quant = 'Q5_K_M',
    [string]$Catalog = (Join-Path $PSScriptRoot '..\src-tauri\src\catalog\catalog.json'),
    [string]$ModelsDir = (Join-Path $env:APPDATA 'com.pais.handy\models')
)

$ErrorActionPreference = 'Stop'

$catalogText = [System.IO.File]::ReadAllText($Catalog, [System.Text.Encoding]::UTF8)
$catalogJson = $catalogText | ConvertFrom-Json

$model = $catalogJson.models | Where-Object { $_.id -eq $ModelId }
if (-not $model) {
    $available = ($catalogJson.models | Where-Object { $_.id -like '*Breeze*' }).id -join ', '
    throw "model '$ModelId' not found in the catalogue. Breeze entries: $available"
}

$file = $model.files | Where-Object { $_.quant -eq $Quant }
if (-not $file) {
    $quants = ($model.files | ForEach-Object { $_.quant }) -join ', '
    throw "quant '$Quant' not found for $ModelId. Available: $quants"
}

# In catalog.json the model `id` IS the HuggingFace repo id, and `revision` is a
# commit SHA pinned at the top level. (The Rust `ModelSource` enum is derived from
# these at load time; it is not stored in the JSON.)
$repoId = $model.id
$revision = $model.revision
if (-not $repoId -or $repoId -notmatch '/') {
    throw "model $ModelId does not look like a HuggingFace repo id"
}
if (-not $revision) { throw "model $ModelId has no pinned revision in the catalogue" }

$expectedSha = $file.sha256
$expectedSize = $file.size_bytes

Write-Host "model      : $ModelId"
Write-Host "quant      : $Quant"
Write-Host "file       : $($file.filename)"
Write-Host "expected   : $([math]::Round($expectedSize/1MB,1)) MB"
Write-Host "sha256     : $expectedSha"
if ($model.license) { Write-Host "license    : $($model.license)" }
Write-Host ""

New-Item -ItemType Directory -Force -Path $ModelsDir | Out-Null
$target = Join-Path $ModelsDir $file.filename
$partial = "$target.partial"

function Get-Sha256([string]$Path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $hash = $sha.ComputeHash($stream)
        return ($hash | ForEach-Object { $_.ToString('x2') }) -join ''
    } finally {
        $stream.Dispose(); $sha.Dispose()
    }
}

# Already present and correct? Nothing to do.
if (Test-Path $target) {
    Write-Host "already present, verifying existing file..."
    $existing = Get-Sha256 $target
    if ($existing -eq $expectedSha) {
        Write-Host "OK: existing file matches the catalogue digest. Nothing to download."
        Write-Host "path: $target"
        exit 0
    }
    Write-Host "existing file does NOT match (got $existing); re-downloading."
    Remove-Item $target -Force
}

# Pinned revision: the catalogue records the exact commit, so a moved branch
# cannot silently change what we fetch.
$url = "https://huggingface.co/$repoId/resolve/$revision/$($file.filename)"
Write-Host "downloading $url"
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$ProgressPreference = 'SilentlyContinue'
Invoke-WebRequest -Uri $url -OutFile $partial -TimeoutSec 3600 -UseBasicParsing
$sw.Stop()
$gotSize = (Get-Item $partial).Length
Write-Host ("downloaded {0:N1} MB in {1:N0}s" -f ($gotSize/1MB), $sw.Elapsed.TotalSeconds)

if ($gotSize -ne $expectedSize) {
    Remove-Item $partial -Force -ErrorAction SilentlyContinue
    throw "size mismatch: expected $expectedSize bytes, got $gotSize. Partial file deleted."
}

$actualSha = Get-Sha256 $partial
if ($actualSha -ne $expectedSha) {
    Remove-Item $partial -Force -ErrorAction SilentlyContinue
    throw "SHA-256 mismatch: expected $expectedSha, got $actualSha. Partial file deleted and NOT installed."
}

Move-Item $partial $target -Force
Write-Host ""
Write-Host "OK: digest verified against the catalogue, model installed."
Write-Host "path: $target"
