# Downloads portable, non-admin toolchains into $env:USERPROFILE\toolchain
# Every item here is per-user and requires no elevation.
$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
$root = Join-Path $env:USERPROFILE 'toolchain'
$dl = Join-Path $root '_dl'
New-Item -ItemType Directory -Force -Path $root, $dl | Out-Null

$items = @(
  @{ name='cmake';  url='https://github.com/Kitware/CMake/releases/download/v4.4.4/cmake-4.4.4-windows-x86_64.zip';                                   file='cmake.zip';  kind='zip' },
  @{ name='ninja';  url='https://github.com/ninja-build/ninja/releases/download/v1.13.2/ninja-win.zip';                                             file='ninja.zip';  kind='zip' },
  @{ name='bun';    url='https://github.com/oven-sh/bun/releases/download/bun-v1.4.3/bun-windows-x64.zip';                                          file='bun.zip';    kind='zip' },
  @{ name='w64devkit'; url='https://github.com/skeeto/w64devkit/releases/download/v2.10.0/w64devkit-x64-2.10.0.7z.exe';                             file='w64.7z.exe'; kind='sfx' },
  @{ name='llvm';   url='https://github.com/llvm/llvm-project/releases/download/llvmorg-23.1.3/clang%2Bllvm-23.1.3-x86_64-pc-windows-msvc.tar.xz'; file='llvm.tar.xz'; kind='tarxz' }
)

foreach ($it in $items) {
  $dest = Join-Path $dl $it.file
  Write-Output "=========== $($it.name) ==========="
  if (Test-Path $dest) {
    Write-Output "already downloaded: $([math]::Round((Get-Item $dest).Length/1MB,1)) MB"
  } else {
    Write-Output "downloading $($it.url)"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    try {
      Invoke-WebRequest -Uri $it.url -OutFile $dest -TimeoutSec 1800 -UseBasicParsing
      $sw.Stop()
      Write-Output ("downloaded {0:N1} MB in {1:N0}s" -f ((Get-Item $dest).Length/1MB), $sw.Elapsed.TotalSeconds)
    } catch {
      Write-Output "DOWNLOAD FAILED: $($_.Exception.Message)"
      continue
    }
  }
}

Write-Output ""
Write-Output "=========== EXTRACT ==========="
foreach ($it in $items) {
  $dest = Join-Path $dl $it.file
  $out = Join-Path $root $it.name
  if (-not (Test-Path $dest)) { Write-Output "$($it.name): no archive, skipped"; continue }
  if (Test-Path $out) { Write-Output "$($it.name): already extracted"; continue }
  Write-Output "--- extracting $($it.name) ---"
  switch ($it.kind) {
    'zip'    { Expand-Archive -Path $dest -DestinationPath $out -Force }
    'tarxz'  { New-Item -ItemType Directory -Force -Path $out | Out-Null; tar -xf $dest -C $out }
    'sfx'    { New-Item -ItemType Directory -Force -Path $out | Out-Null; & $dest -y "-o$out" | Out-Null }
  }
  if (Test-Path $out) { Write-Output "OK -> $out" } else { Write-Output "EXTRACT FAILED -> $out" }
}

Write-Output ""
Write-Output "=========== LOCATE BINARIES ==========="
foreach ($pat in @('cmake.exe','ninja.exe','bun.exe','x86_64-w64-mingw32-gcc.exe','gcc.exe','clang-cl.exe','lld-link.exe')) {
  $hit = Get-ChildItem $root -Recurse -Filter $pat -ErrorAction SilentlyContinue | Select-Object -First 3 -ExpandProperty FullName
  if ($hit) { $hit | ForEach-Object { "  $_" } } else { Write-Output "  $pat : NOT FOUND" }
}
Write-Output "DONE"
