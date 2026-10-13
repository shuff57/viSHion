# Blind-critic native check (Windows), round 7: a dialog opened from the keyboard with a UI Automation client
# polling FocusedElement (what Narrator and NVDA do) must not kill the window. Fails today for Settings: the
# process exits with -1073740791 and stderr says
#   accesskit_consumer-0.16.1/src/tree.rs:35: assertion failed: self.nodes.contains_key(&self.focus)
# (the round 6 fix lets go of a focus that is not a node of the frame, but the dialog's first focus is not covered).
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-dialogs.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name W -Namespace N
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
function Foc { try { $c = [System.Windows.Automation.AutomationElement]::FocusedElement.Current; return ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { return 'ERR' } }
function TabTo($pat, $max = 60) {
  for ($i = 0; $i -lt $max; $i++) { if ((Foc) -like $pat) { return $true }; [System.Windows.Forms.SendKeys]::SendWait('{TAB}'); Start-Sleep -Milliseconds 350 }
  return $false
}
foreach ($case in @(@('Settings', 'Button:Settings'), @('Remove this page', 'Button:Remove this page'), @('Change student', 'Button:Change student'))) {
  $p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
  try {
    $win = $null
    for ($i = 0; $i -lt 100 -and -not $win; $i++) { Start-Sleep -Milliseconds 300; $p.Refresh(); if ($p.MainWindowHandle -ne 0) { try { $win = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle) } catch {} } }
    $all = @(); for ($i = 0; $i -lt 40 -and $all.Count -lt 30; $i++) { Start-Sleep -Milliseconds 500; $all = $win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition) }
    [N.W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 800
    Check (TabTo $case[1]) "Tab reaches $($case[0])"
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}'); Start-Sleep -Milliseconds 1500
    $null = Foc; Start-Sleep -Milliseconds 700
    $p.Refresh()
    Check (-not $p.HasExited) "the window is still there after Enter on $($case[0]) (exit code $(if ($p.HasExited) { $p.ExitCode } else { '-' }))"
    if (-not $p.HasExited) {
      $f = Foc
      Check ($f -ne 'ERR' -and $f -notlike 'Button:Settings*') "focus moved into the dialog: $f"
      [System.Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 800
      $p.Refresh(); Check (-not $p.HasExited) "the window is still there after Escape"
    }
  } finally { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
}
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative dialog checks passed"
