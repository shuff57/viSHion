# Blind-critic native check, round 11 (Windows): focus after a plain action (no dialog) on the page. Fails today, on a
# fresh `--demo` with a UI Automation client polling FocusedElement:
#  1. "Show plain text" (Space/Enter): the button turns into "Show formatted text" and focus drops to the Window
#     element; the next Tab starts at the top ("viSHion", "Back to ...").
#  2. "Transcribe this page again" on an untouched page (no confirmation): same, focus is on the Window element and
#     stays there after the page is read.
# The harness test native_focus_survives_transcribe_again_on_an_untouched_page passes because it never presents the
# frame to accesskit/egui the way the real window does; only the real exe shows it.
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-actions.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name W -Namespace N
$UA = [System.Windows.Automation.AutomationElement]
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
function Foc { try { $c = $UA::FocusedElement.Current; return ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { return 'ERR' } }
function Key($k, $ms = 500) { [System.Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $ms }
function Go($pat) { foreach ($e in $script:win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition)) { try { if ($e.Current.Name -like $pat -and $e.Current.ControlType.ProgrammaticName -match 'Button') { $e.SetFocus(); break } } catch {} }; Start-Sleep -Milliseconds 300 }
foreach ($case in @('Show plain text', 'Transcribe this page again')) {
  $p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
  try {
    $win = $null
    for ($i = 0; $i -lt 100 -and -not $win; $i++) { Start-Sleep -Milliseconds 300; $p.Refresh(); if ($p.MainWindowHandle -ne 0) { try { $win = $UA::FromHandle($p.MainWindowHandle) } catch {} } }
    $script:win = $win
    Start-Sleep 4; [N.W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800
    Go $case
    Check ((Foc) -eq "Button:$case") "focus is on '$case' before the key"
    Key '{ENTER}' 1500
    $f = Foc
    Check ($f -ne 'ERR' -and $f -notlike 'Window:*' -and $f -ne 'Pane:') "after Enter on '$case' focus is on a control, got '$f'"
  } finally { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
}
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative action focus checks passed"
