# Blind-critic native check, round 10 (Windows): dialogs that follow a dialog, a long dialog's tab order, and focus
# after a dialog's action. Fails today:
#  1. Settings > Clear remembered fixes (with one fix remembered): the confirmation opens and focus is on the Window
#     element, not on Cancel (every other confirmation puts focus on Cancel).
#  2. The same confirmation, cancelled with Escape or Cancel: Settings is gone and focus is on the gear. js stacks the
#     confirmation on Settings (modal.js), so Cancel returns to Settings with focus on "Clear remembered fixes".
#  3. Settings, Tab through one lap: "Done" is visited again after every scrolled section (it sorts by its place on the
#     screen against controls that scroll past it), 8 stops on Done in one lap; the footer button must be one stop, last.
#  4. Review again, accepted on a page that has choices: focus drops to the Window element (the control that opened
#     the dialog is gone); a screen reader says the window title and the next Tab starts at the top.
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-dialogs-stack.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name W -Namespace N
$UA = [System.Windows.Automation.AutomationElement]
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
function Foc { try { $c = $UA::FocusedElement.Current; return ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { return 'ERR' } }
function Key($k, $ms = 500) { [System.Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $ms }
function Btn($pat) { foreach ($e in $script:win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition)) { try { if ($e.Current.Name -like $pat -and $e.Current.ControlType.ProgrammaticName -match 'Button|Edit|CheckBox') { return $e } } catch {} } }
function Inv($pat) { $e = Btn $pat; if ($e) { try { $e.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke() } catch {}; Start-Sleep -Milliseconds 700 } }
function Go($pat) { $e = Btn $pat; if ($e) { try { $e.SetFocus() } catch {} }; Start-Sleep -Milliseconds 300 }
function TabTo($pat, $max = 60) { for ($i = 0; $i -lt $max; $i++) { if ((Foc) -like $pat) { return $true }; Key '{TAB}' 200 }; return $false }
function Boot { $script:p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
  $script:win = $null
  for ($i = 0; $i -lt 100 -and -not $script:win; $i++) { Start-Sleep -Milliseconds 300; $script:p.Refresh(); if ($script:p.MainWindowHandle -ne 0) { try { $script:win = $UA::FromHandle($script:p.MainWindowHandle) } catch {} } }
  Start-Sleep 4; [N.W]::SetForegroundWindow($script:p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800 }
try {
  Boot
  # (3) one lap of Settings
  Go 'Settings'; Key '{ENTER}' 700
  $dones = 0; for ($i = 0; $i -lt 34; $i++) { if ((Foc) -eq 'Button:Done') { $dones++ }; Key '{TAB}' 200 }
  Check ($dones -le 1) "Settings: one lap of Tab visits Done at most once, visited $dones times"
  Key '{ESC}' 600
  # remember one fix so "Clear remembered fixes" is enabled
  Inv 'Word that could not be read*'
  $ed = Btn 'Type what the word says'; if ($ed) { $ed.SetFocus(); Key 'integral' 300; Inv 'Use this' }
  # (1)(2)
  Go 'Settings'; Key '{ENTER}' 700
  if (TabTo 'Button:Clear remembered fixes') {
    Key '{ENTER}' 800
    Check ((Foc) -eq 'Button:Cancel') "Clear remembered fixes: the confirmation puts focus on Cancel, got '$(Foc)'"
    Key '{ESC}' 700
    Check ((Foc) -eq 'Button:Clear remembered fixes') "Cancel returns to Settings on Clear remembered fixes, got '$(Foc)'"
  } else { Check $false "Settings: Tab reaches Clear remembered fixes (it is disabled after Use this?)" }
  Key '{ESC}' 600; Key '{ESC}' 300
} finally { if ($script:p -and -not $script:p.HasExited) { Stop-Process -Id $script:p.Id -Force } }
try {
  Boot
  # (4)
  Inv 'Use all 3 suggestions'
  foreach ($n in 'Page 1*', 'Page 2*') { $e = Btn $n; if ($e) { try { $e.SetFocus(); Key '+{ENTER}' 600 } catch {} } }
  Go 'Review again*'; Key '{ENTER}' 800
  Check ((Foc) -eq 'Button:Cancel') "Review again: confirmation puts focus on Cancel, got '$(Foc)'"
  Inv 'Review again'; Start-Sleep -Milliseconds 800
  $p = $script:p; $p.Refresh()
  Check ((-not $p.HasExited) -and (Foc) -notlike 'Window:*' -and (Foc) -ne 'ERR') "after Review again is accepted focus is on a control, got '$(Foc)'"
} finally { if ($script:p -and -not $script:p.HasExited) { Stop-Process -Id $script:p.Id -Force } }
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative dialog stack checks passed"
