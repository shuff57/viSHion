# Blind-critic native check, round 6 (Windows): work through the word panel from the keyboard with a UI Automation
# client attached (what Narrator or NVDA is). Press Enter on the first choice, then on the control that has focus,
# twice more. Fails today: the second or third Enter kills the process, panic in accesskit_consumer
# `tree.rs:35 assertion failed: self.nodes.contains_key(&self.focus)` (focus is on a choice that the next word
# does not have, the update names a focus node it does not contain).
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-choices.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name W -Namespace N
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
$psi = New-Object System.Diagnostics.ProcessStartInfo $Exe, '--demo'
$psi.RedirectStandardError = $true; $psi.UseShellExecute = $false
$p = [System.Diagnostics.Process]::Start($psi)
$UA = [System.Windows.Automation.AutomationElement]
function Foc() { try { $c = $UA::FocusedElement.Current; return $c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name } catch { return 'ERR' } }
try {
  $win = $null
  for ($i = 0; $i -lt 100 -and -not $win; $i++) { Start-Sleep -Milliseconds 300; $p.Refresh(); if ($p.MainWindowHandle -ne 0) { try { $win = $UA::FromHandle($p.MainWindowHandle) } catch {} } }
  Start-Sleep 4
  $all = $win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition)
  [N.W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; Start-Sleep 1
  foreach ($e in $all) { if ($e.Current.Name -like 'Spelling to check: confidance*') { $e.SetFocus(); break } }
  Start-Sleep -Milliseconds 500
  [System.Windows.Forms.SendKeys]::SendWait('{TAB}'); Start-Sleep -Milliseconds 400
  for ($i = 0; $i -lt 20 -and -not ((Foc) -like '*Course word*'); $i++) { [System.Windows.Forms.SendKeys]::SendWait('{TAB}'); Start-Sleep -Milliseconds 300 }
  Check ((Foc) -like '*Course word*') 'Tab reaches the first choice'
  $alive = $true
  for ($i = 1; $i -le 4 -and $alive; $i++) {
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}'); Start-Sleep -Milliseconds 1200
    $p.Refresh(); $alive = -not $p.HasExited
    Check $alive "the window is still there after Enter $i on a choice"
  }
  if (-not $alive) { Write-Host ($p.StandardError.ReadToEnd() -split "`n" | Select-Object -First 3) }
} finally { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative choices check passed"
