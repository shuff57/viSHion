# Blind-critic native check (Windows): the native Tab ring must follow reading order (the transcript before the word
# panel, as on the web) and no Tab stop may be unnamed. Fails today: header > tools > stage > word panel > transcript,
# with 12 unnamed Custom stops (egui scroll areas, the splitter).
#   powershell -NoProfile -STA -File crates/app/native-a11y-critic.ps1 [-Exe target/release/vishion.exe]
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);' -Name W -Namespace N
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }
$p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
try {
  $win = $null
  for ($i = 0; $i -lt 100 -and -not $win; $i++) { Start-Sleep -Milliseconds 300; $p.Refresh(); if ($p.MainWindowHandle -ne 0) { try { $win = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle) } catch {} } }
  $all = @(); for ($i = 0; $i -lt 40 -and $all.Count -lt 30; $i++) { Start-Sleep -Milliseconds 500; $all = $win.FindAll('Descendants', [System.Windows.Automation.Condition]::TrueCondition) }
  [N.W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 1000
  $seen = @()
  for ($i = 0; $i -lt 70; $i++) {
    [System.Windows.Forms.SendKeys]::SendWait('{TAB}'); Start-Sleep -Milliseconds 450
    try { $c = [System.Windows.Automation.AutomationElement]::FocusedElement.Current; $seen += ($c.ControlType.ProgrammaticName.Replace('ControlType.', '') + ':' + $c.Name) } catch { $seen += 'ERR:' }
  }
  Write-Host ($seen -join " > ")
  Check (($seen | Where-Object { $_ -like 'Button:Spelling to check*' }).Count -ge 1) 'Tab reaches a flagged word'
  $iw = [array]::IndexOf($seen, [string]($seen | Where-Object { $_ -like 'Button:Spelling to check*' } | Select-Object -First 1)); $it = [array]::IndexOf($seen, 'Button:Use all 3 suggestions')
  Check ($iw -ge 0 -and $it -ge 0 -and $iw -lt $it) "the transcript is before the word panel in Tab order (word at $iw, panel at $it)"
  Check ($seen -contains 'Button:Change student') 'Tab reaches Change student'
  Check ($seen -contains 'Button:Transcribe this page again') 'Tab reaches Transcribe this page again'
  $unnamed = @($seen | Where-Object { $_ -match ':$' } | Select-Object -Unique)
  Check ($unnamed.Count -eq 0) ('every Tab stop has a name (unnamed: ' + (($seen | Where-Object { $_ -match ':$' }).Count) + ' stops)')
} finally { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative critic checks passed"
