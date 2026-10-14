# Blind-critic native check (Windows), round 7: a dialog opened from the keyboard with a UI Automation client
# polling FocusedElement (what Narrator and NVDA do) must not kill the window. Fails today for Settings: the
# process exits with -1073740791 and stderr says
#   accesskit_consumer-0.16.1/src/tree.rs:35: assertion failed: self.nodes.contains_key(&self.focus)
# (the round 6 fix lets go of a focus that is not a node of the frame, but the dialog's first focus is not covered).
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic-dialogs.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow(); [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);' -Name W -Namespace N
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
function Foc { try { $c = [System.Windows.Automation.AutomationElement]::FocusedElement.Current; return ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { return 'ERR' } }
# Root cause of the intermittent "Tab reaches X never found" (round 12): the harness, not the window. Windows refuses
# SetForegroundWindow from a process that is not itself foreground (the foreground lock), without an error; the 60 Tabs
# then went to whatever window had the keyboard (the terminal that launched the script, or a notification) and the app
# never saw one. Taking the foreground with the Alt-key trick and checking GetForegroundWindow before every key makes
# the run deterministic, and a run that cannot get the foreground says so instead of reporting a focus stall.
function Take-Foreground($h) {
  for ($i = 0; $i -lt 20; $i++) {
    if ([N.W]::GetForegroundWindow() -eq $h) { return $true }
    [N.W]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [N.W]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)   # a synthetic Alt lifts the lock
    [N.W]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 150
  }
  return ([N.W]::GetForegroundWindow() -eq $h)
}
# Send one Tab and wait (up to 2 s) for the focus to be somewhere else: UIA focus events lag a frame or two behind the key.
function Press-Tab($h) {
  if (-not (Take-Foreground $h)) { return $null }
  $before = Foc
  [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
  for ($k = 0; $k -lt 20; $k++) { Start-Sleep -Milliseconds 100; $f = Foc; if ($f -ne $before) { return $f } }
  return $f
}
$script:trail = @(); $script:nofg = $false
function TabTo($pat, $h, $max = 60) {
  $script:trail = @(); $script:nofg = $false
  for ($i = 0; $i -lt $max; $i++) {
    if ((Foc) -like $pat) { return $true }
    $f = Press-Tab $h
    if ($null -eq $f) { $script:nofg = $true; return $false }
    $script:trail += $f
  }
  return ((Foc) -like $pat)
}
foreach ($case in @(@('Settings', 'Button:Settings'), @('Remove this page', 'Button:Remove this page'), @('Change student', 'Button:Change student'))) {
  $p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
  try {
    $win = $null
    for ($i = 0; $i -lt 100 -and -not $win; $i++) { Start-Sleep -Milliseconds 300; $p.Refresh(); if ($p.MainWindowHandle -ne 0) { try { $win = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle) } catch {} } }
    $all = @(); for ($i = 0; $i -lt 40 -and $all.Count -lt 30; $i++) { Start-Sleep -Milliseconds 500; $all = $win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition) }
    $h = $p.MainWindowHandle
    Check (Take-Foreground $h) "the window took the keyboard foreground (harness precondition, not an app result)"
    Start-Sleep -Milliseconds 800
    $reached = TabTo $case[1] $h
    Check $reached "Tab reaches $($case[0])$(if (-not $reached) { if ($script:nofg) { ' (harness lost the foreground, not a focus stall)' } else { ' (trail: ' + ($script:trail -join ' > ') + ')' } })"
    Take-Foreground $h | Out-Null
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}'); Start-Sleep -Milliseconds 1500
    $null = Foc; Start-Sleep -Milliseconds 700
    $p.Refresh()
    Check (-not $p.HasExited) "the window is still there after Enter on $($case[0]) (exit code $(if ($p.HasExited) { $p.ExitCode } else { '-' }))"
    if (-not $p.HasExited) {
      $f = Foc
      Check ($f -ne 'ERR' -and $f -notlike 'Button:Settings*') "focus moved into the dialog: $f"
      Take-Foreground $h | Out-Null
      [System.Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 800
      $p.Refresh(); Check (-not $p.HasExited) "the window is still there after Escape"
    }
  } finally { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
}
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative dialog checks passed"
