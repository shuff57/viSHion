# Blind-critic native check, round 8 (Windows): focus must stay inside an open dialog and come back to its opener.
# Fails today:
#  1. Settings (or Change student) opened from the keyboard, ArrowLeft / ArrowUp on the first control: UI Automation
#     focus goes to an unnamed Custom element that is not in the dialog (not named, not in the tree walk); Enter and
#     Space do nothing there and Tab then skips the first control.
#  2. A dialog opened with the mouse (Change student, Remove this page): Escape or Cancel leaves focus on the window,
#     not on the button that opened it (js modal returns focus to the opener).
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-dialogs-focus.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern void mouse_event(int f,int x,int y,int d,int e);' -Name W -Namespace N
$UA = [System.Windows.Automation.AutomationElement]
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
function Foc { try { $c = $UA::FocusedElement.Current; return ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { return 'ERR' } }
function Key($k) { [System.Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds 600 }
function Find($n) { foreach ($e in $script:win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition)) { if ($e.Current.Name -eq $n -and $e.Current.ControlType.ProgrammaticName -match 'Button') { return $e } } }
function Click($n) { $r = (Find $n).Current.BoundingRectangle; [N.W]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null; Start-Sleep -Milliseconds 150; [N.W]::mouse_event(2, 0, 0, 0, 0); [N.W]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 700 }
function Launch {
  $script:p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
  $script:win = $null
  for ($i = 0; $i -lt 100 -and -not $script:win; $i++) { Start-Sleep -Milliseconds 300; $script:p.Refresh(); if ($script:p.MainWindowHandle -ne 0) { try { $script:win = $UA::FromHandle($script:p.MainWindowHandle) } catch {} } }
  Start-Sleep 4; [N.W]::SetForegroundWindow($script:p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800
}
try {
  Launch
  $e = Find 'Settings'; $e.SetFocus(); Start-Sleep -Milliseconds 300; Key '{ENTER}'
  $first = Foc
  foreach ($k in '{LEFT}', '{UP}') {
    Key $k
    $f = Foc
    Check ($f -ne 'Custom:' -and $f -ne 'ERR') "Settings: $k from the first control ($first) keeps a named control focused, got '$f'"
    if ($f -eq 'Custom:') { Key '{HOME}' }
  }
  Key '{ESC}'
  Click 'Change student'
  Check ((Foc) -notlike 'Window:*') "Change student opened with the mouse: focus is in the dialog ($(Foc))"
  Key '{ESC}'
  Check ((Foc) -eq 'Button:Change student') "Escape after a mouse open returns focus to Change student, got '$(Foc)'"
  Click 'Remove this page'; Click 'Cancel'
  Check ((Foc) -eq 'Button:Remove this page') "Cancel after a mouse open returns focus to Remove this page, got '$(Foc)'"
} finally { if ($script:p -and -not $script:p.HasExited) { Stop-Process -Id $script:p.Id -Force } }
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative dialog focus checks passed"
