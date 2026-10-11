# gen_tts_corpus.ps1
#
# Generates a Traditional-Chinese (Taiwan) speech corpus with a KNOWN ground
# truth, using the Windows SAPI voices installed on this machine.
#
# WHY THIS EXISTS
#   Phase 3 asks for a real ASR acceptance test. A microphone needs a human
#   speaker; text-to-speech does not, and it produces a WAV whose exact spoken
#   content is known in advance. That makes it possible to verify that the
#   recognition pipeline produces real text derived from real audio, and to
#   compare that text against a reference.
#
# WHAT THIS IS NOT
#   Synthetic TTS audio is clean, studio-quality speech. It is NOT a substitute
#   for testing a human voice through a real microphone in a real room. Any
#   accuracy figure derived from it is an upper bound and must be labelled as
#   TTS-derived.
#
# This script is deliberately ASCII-only: the Chinese ground truth lives in
# tools/tts_corpus.json and is read with an explicit UTF-8 decode, because
# Windows PowerShell 5.1 reads .ps1 files using the system ANSI codepage and
# would corrupt CJK literals embedded in the script.
#
# Output: <outdir>/<name>.wav  (16 kHz, mono, 16-bit PCM - what ASR expects)
#         <outdir>/corpus.json (name -> ground-truth text, copied from source)

param(
    [string]$OutDir = 'C:\Users\Nova\Project Brief\_phase3_corpus',
    [string]$CorpusJson = (Join-Path $PSScriptRoot 'tts_corpus.json')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech

if (-not (Test-Path $CorpusJson)) { throw "corpus definition not found: $CorpusJson" }

# Explicit UTF-8 decode: this is the whole reason the data lives in a file.
$corpus = [System.IO.File]::ReadAllText($CorpusJson, [System.Text.Encoding]::UTF8) |
    ConvertFrom-Json

$sampleRate = [int]$corpus.format.sample_rate
$bits = [int]$corpus.format.bits

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    $sampleRate,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono
)

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$synth.SelectVoice($corpus.voice)
$synth.Rate = 0

$items = [ordered]@{}
foreach ($item in $corpus.items) {
    $path = Join-Path $OutDir "$($item.name).wav"
    if (Test-Path $path) { Remove-Item $path -Force }
    $synth.SetOutputToWaveFile($path, $format)
    $synth.Speak($item.text)
    $synth.SetOutputToNull()

    # Verify the file really is RIFF/WAVE before anything downstream trusts it.
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $riff = [System.Text.Encoding]::ASCII.GetString($bytes[0..3])
    $wave = [System.Text.Encoding]::ASCII.GetString($bytes[8..11])
    if ($riff -ne 'RIFF' -or $wave -ne 'WAVE') { throw "$path is not a WAV file" }

    Write-Host ("generated {0,-22} {1,8:N0} bytes  header={2}/{3}" -f $item.name, $bytes.Length, $riff, $wave)
    $items[$item.name] = [ordered]@{
        text    = $item.text
        purpose = $item.purpose
        wav     = $path
        bytes   = $bytes.Length
    }
}
$synth.Dispose()

$out = [ordered]@{
    voice        = $corpus.voice
    sample_rate  = $sampleRate
    channels     = 1
    bits         = $bits
    generated_at = (Get-Date).ToString('o')
    note         = $corpus.note
    items        = $items
}
$json = $out | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText((Join-Path $OutDir 'corpus.json'), $json, (New-Object System.Text.UTF8Encoding($false)))

Write-Host ""
Write-Host "wrote $(Join-Path $OutDir 'corpus.json')"
Write-Host "items: $($items.Count)"
