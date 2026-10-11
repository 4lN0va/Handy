# run_asr_corpus.ps1
#
# Runs the real recognition pipeline over the TTS corpus and scores the output
# against the known ground truth.
#
# WHY THIS EXISTS
#   "Recognition works" is a claim that needs evidence. This drives Handy's real
#   headless transcription path (`--transcribe-file`) with a real model over real
#   audio, then compares the recognised text to the text the synthesiser was told
#   to speak.
#
# WHAT THE NUMBERS MEAN
#   The audio is SYNTHETIC TTS, not a human microphone recording. Clean synthetic
#   speech is markedly easier than a person in a room, so any error rate here is a
#   BEST CASE and must not be reported as real-world accuracy. It does prove the
#   pipeline performs genuine recognition of the audio content.
#
# Character error rate is computed after normalising away punctuation and
# whitespace, because the models legitimately differ on punctuation and scoring
# that would measure formatting rather than recognition.

param(
    [string]$Exe = (Join-Path $env:USERPROFILE 't\handy-msvc\debug\handy.exe'),
    [string]$ModelId = 'handy-computer/Breeze-ASR-25-gguf/Breeze-ASR-25-Q5_K_M.gguf',
    [string]$CorpusDir = (Join-Path (Split-Path -Parent $PSScriptRoot) '_phase3_corpus'),
    [string]$OutJson = (Join-Path (Split-Path -Parent $PSScriptRoot) '_phase3_corpus\asr_results.json'),
    [int]$Repeat = 1
)

$ErrorActionPreference = 'Stop'

function Get-Normalized([string]$s) {
    if ($null -eq $s) { return '' }
    # Drop everything that is not a CJK ideograph, a latin letter/digit, or an
    # apostrophe. This removes punctuation and whitespace, which are a formatting
    # disagreement rather than a recognition error.
    $sb = New-Object System.Text.StringBuilder
    foreach ($ch in $s.ToCharArray()) {
        $c = [int][char]$ch
        $isCjk = ($c -ge 0x3400 -and $c -le 0x4DBF) -or ($c -ge 0x4E00 -and $c -le 0x9FFF) -or ($c -ge 0xF900 -and $c -le 0xFAFF)
        $isLatin = ($ch -match '[A-Za-z0-9'']')
        if ($isCjk -or $isLatin) { [void]$sb.Append([char]::ToLowerInvariant($ch)) }
    }
    return $sb.ToString()
}

# Levenshtein distance over characters.
function Get-CharDistance([string]$a, [string]$b) {
    $n = $a.Length; $m = $b.Length
    if ($n -eq 0) { return $m }
    if ($m -eq 0) { return $n }
    $prev = New-Object 'int[]' ($m + 1)
    $curr = New-Object 'int[]' ($m + 1)
    for ($j = 0; $j -le $m; $j++) { $prev[$j] = $j }
    for ($i = 1; $i -le $n; $i++) {
        $curr[0] = $i
        for ($j = 1; $j -le $m; $j++) {
            $cost = if ($a[$i - 1] -eq $b[$j - 1]) { 0 } else { 1 }
            $del = $curr[$j - 1] + 1
            $ins = $prev[$j] + 1
            $sub = $prev[$j - 1] + $cost
            $curr[$j] = [Math]::Min([Math]::Min($del, $ins), $sub)
        }
        $tmp = $prev; $prev = $curr; $curr = $tmp
    }
    return $prev[$m]
}

$corpus = [System.IO.File]::ReadAllText((Join-Path $CorpusDir 'corpus.json'), [System.Text.Encoding]::UTF8) | ConvertFrom-Json

if (-not (Test-Path $Exe)) { throw "executable not found: $Exe" }

# A running GUI instance owns the single-instance lock and would swallow these
# arguments instead of transcribing.
$running = Get-Process -Name 'handy' -ErrorAction SilentlyContinue
if ($running) {
    throw "Handy is already running (PID $($running[0].Id)). Close it first: the app is single-instance, so a second launch forwards its arguments to the running instance instead of transcribing."
}

Write-Host "exe   : $Exe"
Write-Host "model : $ModelId"
Write-Host "repeat: $Repeat"
Write-Host ""

$results = [ordered]@{}
$totalDist = 0; $totalRef = 0

foreach ($item in $corpus.items.PSObject.Properties) {
    $name = $item.Name
    $truth = $item.Value.text
    $wav = Join-Path $CorpusDir "$name.wav"
    Write-Host "--- $name ---"
    Write-Host "  truth : $truth"

    $args = @('--transcribe-file', $wav, '--model', $ModelId, '--json')
    if ($Repeat -gt 1) { $args += @('--repeat', $Repeat) }

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    # Handy logs to stderr. With ErrorActionPreference='Stop', PowerShell turns
    # any native-command stderr line into a terminating error, which would abort
    # the run on the very first log message. Relax it just for this call.
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & $Exe @args 2>&1
    } finally {
        $ErrorActionPreference = $prevEap
    }
    $sw.Stop()

    # The JSON result is the last line that parses as a JSON object.
    $jsonLine = $null
    foreach ($line in $output) {
        $t = "$line".Trim()
        if ($t.StartsWith('{') -and $t.EndsWith('}')) { $jsonLine = $t }
    }
    if (-not $jsonLine) {
        Write-Host "  NO JSON RESULT - raw tail:"
        $output | Select-Object -Last 5 | ForEach-Object { "    $_" }
        $results[$name] = [ordered]@{ truth = $truth; error = 'no json result' }
        continue
    }

    $r = $jsonLine | ConvertFrom-Json
    $hyp = $r.text
    $refN = Get-Normalized $truth
    $hypN = Get-Normalized $hyp
    $dist = Get-CharDistance $refN $hypN
    $cer = if ($refN.Length -gt 0) { [math]::Round($dist / $refN.Length, 4) } else { 0 }
    $totalDist += $dist; $totalRef += $refN.Length

    Write-Host "  output: $hyp"
    Write-Host ("  normalized ref/hyp: '{0}' / '{1}'" -f $refN, $hypN)
    Write-Host ("  CER={0}  edits={1}/{2}  rtf={3}  load_ms={4}  transcribe_ms={5}  backend={6}" -f `
        $cer, $dist, $refN.Length, [math]::Round($r.rtf, 3), $r.load_ms, ($r.transcribe_ms -join ','), $r.bound_backend)

    $results[$name] = [ordered]@{
        truth          = $truth
        hypothesis     = $hyp
        normalized_ref = $refN
        normalized_hyp = $hypN
        edits          = $dist
        ref_chars      = $refN.Length
        cer            = $cer
        audio_secs     = $r.audio_secs
        rtf            = $r.rtf
        load_ms        = $r.load_ms
        transcribe_ms  = $r.transcribe_ms
        bound_backend  = $r.bound_backend
        wall_secs      = [math]::Round($sw.Elapsed.TotalSeconds, 1)
    }
}

$overall = if ($totalRef -gt 0) { [math]::Round($totalDist / $totalRef, 4) } else { 0 }

Write-Host ""
Write-Host "==================================================="
Write-Host ("OVERALL CER (TTS audio): {0}   ({1} edits over {2} reference characters)" -f $overall, $totalDist, $totalRef)
Write-Host "==================================================="
Write-Host ""
Write-Host "REMINDER: this is SYNTHETIC TTS speech, not a human microphone recording."
Write-Host "These figures are a best case and are NOT real-world accuracy."

$out = [ordered]@{
    exe          = $Exe
    model        = $ModelId
    repeat       = $Repeat
    generated_at = (Get-Date).ToString('o')
    audio_source = 'Windows SAPI TTS (synthetic)'
    voice        = $corpus.voice
    overall_cer  = $overall
    edits        = $totalDist
    ref_chars    = $totalRef
    caveat       = 'Synthetic TTS audio. Best case, not real-world accuracy.'
    items        = $results
}
$json = $out | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($OutJson, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Host ""
Write-Host "wrote $OutJson"
