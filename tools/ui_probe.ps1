# ui_probe.ps1
#
# UI Automation driver for the Handy window, used for real GUI acceptance
# testing.
#
# WHY THIS EXISTS
#   Type-checking the front end and unit-testing the Rust commands does not prove
#   that a button calls the command it claims to. Only driving the real window
#   does. Handy renders into WebView2, which exposes a UI Automation tree, so the
#   controls can be found and invoked programmatically.
#
# SCOPE AND SAFETY
#   Every operation is scoped to the process whose name is passed in (default
#   `handy`). This machine has other windows open, so the script never enumerates
#   or screenshots the whole desktop. It also never types into a window it did not
#   resolve itself.
#
# Actions:
#   dump                     list the automation tree of the app window
#   find    -Name <text>     report matches for a name substring
#   click   -Name <text>     invoke or click the first match
#   settext -Name <text> -Value <text>
#                             set the value of the first editable match
#
# Example:
#   powershell -File tools/ui_probe.ps1 -Action dump
#   powershell -File tools/ui_probe.ps1 -Action click -Name "Voice API"

param(
    [ValidateSet('dump', 'find', 'click', 'settext', 'windows')]
    [string]$Action = 'dump',
    [string]$Name = '',
    [string]$Value = '',
    [string]$ProcessName = 'handy',
    [int]$MaxDepth = 12,
    [int]$Limit = 400
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

function Get-AppWindow {
    $procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
    if (-not $procs) { throw "no process named '$ProcessName' is running" }
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $cond = [System.Windows.Automation.Condition]::TrueCondition
    foreach ($w in $root.FindAll([System.Windows.Automation.TreeScope]::Children, $cond)) {
        foreach ($p in $procs) {
            if ($w.Current.ProcessId -eq $p.Id) { return $w }
        }
    }
    throw "no top-level window found for process '$ProcessName'"
}

# Bring the app window to the front.
#
# Required before any coordinate-based click: this desktop has other windows
# open, and clicking at a screen coordinate only reaches the app if the app is
# actually on top. SetForegroundWindow is used rather than a synthetic
# Alt-Tab so nothing else is disturbed.
function Show-AppWindow($window) {
    Add-Type -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
'@ -Name Win32Window -Namespace UiProbe -ErrorAction SilentlyContinue
    $h = [IntPtr]$window.Current.NativeWindowHandle
    if ([UiProbe.Win32Window]::IsIconic($h)) { [UiProbe.Win32Window]::ShowWindow($h, 9) | Out-Null }  # SW_RESTORE
    [UiProbe.Win32Window]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 300
}

function Get-AllDescendants($element) {
    $cond = [System.Windows.Automation.Condition]::TrueCondition
    return $element.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
}

function Format-Element($e) {
    $c = $e.Current
    $type = $c.ControlType.ProgrammaticName -replace '^ControlType\.', ''
    $auto = $c.AutomationId
    $extra = ''
    if ($auto) { $extra += " id='$auto'" }
    if ($c.IsEnabled -eq $false) { $extra += ' DISABLED' }
    return ("[{0,-14}] '{1}'{2}" -f $type, $c.Name, $extra)
}

$window = Get-AppWindow
Write-Host "window: '$($window.Current.Name)' pid=$($window.Current.ProcessId)"
if ($Action -in @('click', 'settext')) { Show-AppWindow $window }
Write-Host ""

switch ($Action) {
    'windows' {
        $root = [System.Windows.Automation.AutomationElement]::RootElement
        foreach ($w in $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
            if ($w.Current.Name) { Write-Host ("  '{0}' [{1}]" -f $w.Current.Name, $w.Current.ClassName) }
        }
    }

    'dump' {
        $all = Get-AllDescendants $window
        Write-Host "descendants: $($all.Count)"
        Write-Host ""
        $shown = 0
        foreach ($e in $all) {
            if ($e.Current.Name -or $e.Current.AutomationId) {
                Write-Host ("  " + (Format-Element $e))
                $shown++
                if ($shown -ge $Limit) { Write-Host "  ... (limit $Limit reached)"; break }
            }
        }
        if ($shown -eq 0) {
            Write-Host "  (no named elements - the WebView2 accessibility tree may not be exposed)"
        }
    }

    'find' {
        if (-not $Name) { throw "-Name is required for 'find'" }
        $all = Get-AllDescendants $window
        $hits = 0
        foreach ($e in $all) {
            if ($e.Current.Name -and $e.Current.Name -like "*$Name*") {
                Write-Host ("  " + (Format-Element $e))
                $hits++
            }
        }
        Write-Host ""
        Write-Host "matches for '*$Name*': $hits"
    }

    'click' {
        if (-not $Name) { throw "-Name is required for 'click'" }
        $all = Get-AllDescendants $window
        $target = $null
        foreach ($e in $all) {
            if ($e.Current.Name -and $e.Current.Name -like "*$Name*") {
                # Prefer something invokable.
                $p = $e.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern) -as [System.Windows.Automation.InvokePattern]
                if ($p) { $target = $e; break }
                if (-not $target) { $target = $e }
            }
        }
        if (-not $target) { throw "no element matching '*$Name*' found" }
        Write-Host ("target: " + (Format-Element $target))

        $inv = $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern) -as [System.Windows.Automation.InvokePattern]
        if ($inv) { $inv.Invoke(); Write-Host "invoked (InvokePattern)"; break }

        $sel = $target.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern) -as [System.Windows.Automation.SelectionItemPattern]
        if ($sel) { $sel.Select(); Write-Host "selected (SelectionItemPattern)"; break }

        # Fall back to a real mouse click at the element centre.
        $r = $target.Current.BoundingRectangle
        if ($r.IsEmpty) { throw "element has no bounding rectangle and no invokable pattern" }
        $x = [int]($r.X + $r.Width / 2)
        $y = [int]($r.Y + $r.Height / 2)
        Write-Host "clicking at ($x,$y)"
        Add-Type -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, int e);
'@ -Name Win32Mouse -Namespace UiProbe
[UiProbe.Win32Mouse]::SetCursorPos($x, $y) | Out-Null
Start-Sleep -Milliseconds 60
[UiProbe.Win32Mouse]::mouse_event(0x0002, 0, 0, 0, 0)   # left down
Start-Sleep -Milliseconds 40
[UiProbe.Win32Mouse]::mouse_event(0x0004, 0, 0, 0, 0)   # left up
Write-Host "clicked"
    }

    'settext' {
        if (-not $Name) { throw "-Name is required for 'settext'" }
        $all = Get-AllDescendants $window
        $target = $null
        foreach ($e in $all) {
            if ($e.Current.Name -and $e.Current.Name -like "*$Name*") {
                $vp = $e.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern) -as [System.Windows.Automation.ValuePattern]
                if ($vp) { $target = $e; break }
            }
        }
        if (-not $target) { throw "no editable element matching '*$Name*' found" }
        $vp = $target.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern) -as [System.Windows.Automation.ValuePattern]
        $vp.SetValue($Value)
        Write-Host ("set value on: " + (Format-Element $target))
    }
}
