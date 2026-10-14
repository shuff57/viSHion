# Blind-critic native check, round 12 (Windows): a plain action pressed with the MOUSE must leave focus on that
# button (a click focuses a button in js and in every native control; round 11 fixed Tab+Enter only). Fails today:
#  1. Click Copy text / Zoom in / Text only / Rotate page 90 degrees / Straighten this page / Transcribe this page
#     again on an untouched page: UI Automation FocusedElement is the Window element (or nothing), not the button
#     that was pressed. A screen reader reads the window title and the next Tab starts again at the top.
#  Settings / Change student / Remove this page opened with the mouse are fine (focus is in the dialog, Escape
#  returns it to the opener, 4 open/close laps with a toast or the word panel up, no crash).
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-dialogs-mouse.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern void mouse_event(int f,int x,int y,int d,int e);' -Name W -Namespace N
$UA = [System.Windows.Automation.AutomationElement]
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
function Foc { try { $c = $UA::FocusedElement.Current; return ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { return 'ERR' } }
function Find($pat) { foreach ($e in $script:win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition)) { try { if ($e.Current.Name -like $pat -and $e.Current.ControlType.ProgrammaticName -match 'Button') { return $e } } catch {} } }
function Click($pat) { [N.W]::SetForegroundWindow($script:p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 300
  $e = Find $pat; $r = $e.Current.BoundingRectangle; [N.W]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
  Start-Sleep -Milliseconds 150; [N.W]::mouse_event(2, 0, 0, 0, 0); [N.W]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 900; return $e.Current.Name }
try {
  $script:p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
  $script:win = $null
  for ($i = 0; $i -lt 100 -and -not $script:win; $i++) { Start-Sleep -Milliseconds 300; $script:p.Refresh(); if ($script:p.MainWindowHandle -ne 0) { try { $script:win = $UA::FromHandle($script:p.MainWindowHandle) } catch {} } }
  $all = @(); for ($i = 0; $i -lt 40 -and $all.Count -lt 30; $i++) { Start-Sleep -Milliseconds 500; $all = $script:win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition) }
  Start-Sleep 3
  foreach ($pat in 'Rotate page*', 'Straighten*', 'Transcribe this page again', 'Copy text', 'Zoom in', 'Text only') {
    if (-not (Find $pat)) { Write-Host "skip $pat (not on screen)"; continue }
    $name = Click $pat
    $script:p.Refresh()
    Check ((-not $script:p.HasExited) -and (Foc) -eq "Button:$name") "mouse press on '$name' leaves focus on it, got '$(Foc)'"
  }
} finally { if ($script:p -and -not $script:p.HasExited) { Stop-Process -Id $script:p.Id -Force } }
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative mouse action checks passed"
