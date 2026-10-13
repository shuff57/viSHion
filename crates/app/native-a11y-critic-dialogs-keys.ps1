# Blind-critic native check, round 9 (Windows): keys must not throw focus away. Fails today:
#  1. Enter in the new-student name box of the Change student dialog: focus drops to the Window element while the
#     dialog is still open (egui's single-line text box gives focus up on Enter); a screen reader says the window
#     title and the next Tab lands on "Only this page", the name box is skipped. The js input keeps focus on Enter.
#  2. Escape on a plain control with nothing open (Zoom in, Text only): focus drops to the Window element. The js
#     page does nothing on Escape there, so the person keeps their place.
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-dialogs-keys.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name W -Namespace N
$UA = [System.Windows.Automation.AutomationElement]
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
function Foc { try { $c = $UA::FocusedElement.Current; return ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { return 'ERR' } }
function Key($k) { [System.Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds 600 }
function Find($pat) { foreach ($e in $script:win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition)) { if ($e.Current.Name -like $pat -and $e.Current.ControlType.ProgrammaticName -match 'Button|Edit') { return $e } } }
function Go($pat) { $e = Find $pat; $e.SetFocus(); Start-Sleep -Milliseconds 300 }
$script:p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
try {
  $script:win = $null
  for ($i = 0; $i -lt 100 -and -not $script:win; $i++) { Start-Sleep -Milliseconds 300; $script:p.Refresh(); if ($script:p.MainWindowHandle -ne 0) { try { $script:win = $UA::FromHandle($script:p.MainWindowHandle) } catch {} } }
  Start-Sleep 4; [N.W]::SetForegroundWindow($script:p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800
  Go 'Change student'; Key '{ENTER}'
  Go 'A new student*'; Key '{ENTER}'
  Check ((Foc) -eq 'Edit:New student name') "the name box takes focus: $(Foc)"
  Key 'Zed'; Key '{ENTER}'
  Check ((Foc) -notlike 'Window:*') "Enter in the name box keeps focus in the dialog, got '$(Foc)'"
  Key '{ESC}'
  foreach ($b in 'Zoom in', 'Text only') {
    Go $b; Key '{ESC}'
    Check ((Foc) -eq "Button:$b") "Escape on $b with nothing open keeps focus there, got '$(Foc)'"
  }
} finally { if ($script:p -and -not $script:p.HasExited) { Stop-Process -Id $script:p.Id -Force } }
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative key checks passed"
