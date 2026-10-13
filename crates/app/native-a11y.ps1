# Native accessibility check (Windows): launch the demo and walk the UI Automation tree the way Narrator and
# NVDA do. Needs `cargo build --release -p vishion-app --bin vishion` first.
#   powershell -File crates/app/native-a11y.ps1 [-Exe target/release/vishion.exe]
# Fails (exit 1) when the window has no named buttons, no headings, no transcript words, or when invoking
# "Zoom in" through UI Automation does not reach the app.
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\vishion.exe'))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$fail = @()
function Check($ok, $msg) { if ($ok) { Write-Host "ok   $msg" } else { Write-Host "FAIL $msg"; $script:fail += $msg } }

$p = Start-Process -FilePath $Exe -ArgumentList '--demo' -PassThru
try {
  $win = $null
  for ($i = 0; $i -lt 100 -and -not $win; $i++) {
    Start-Sleep -Milliseconds 300
    $p.Refresh()
    if ($p.MainWindowHandle -ne 0) {
      try { $win = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle) } catch { $win = $null }
    }
  }
  Check ($null -ne $win) 'the window is in the UI Automation tree'
  if ($win) {
    # the tree is built when the first client asks: poll until the app has answered with its screen
    $all = @()
    for ($i = 0; $i -lt 40 -and $all.Count -lt 30; $i++) {
      Start-Sleep -Milliseconds 500
      $all = $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    }
    $named = @(); $buttons = @(); $unnamedCustom = 0
    foreach ($e in $all) {
      $c = $e.Current
      if ($c.Name) { $named += $c.Name }
      if ($c.ControlType.ProgrammaticName -eq 'ControlType.Button') { $buttons += $c.Name }
      if ($c.ControlType.ProgrammaticName -eq 'ControlType.Custom' -and -not $c.Name) { $unnamedCustom++ }
    }
    Write-Host ("{0} descendants, {1} named, {2} buttons, {3} unnamed custom" -f $all.Count, $named.Count, $buttons.Count, $unnamedCustom)
    Check ($buttons.Count -ge 15) "named buttons are exposed ($($buttons.Count))"
    Check ($buttons -contains 'Zoom in') 'a button named "Zoom in"'
    Check (($buttons | Where-Object { $_ -like 'Done*' }).Count -ge 1) 'a button named "Done, next student"'
    Check ($named -contains 'Maya R.' -or ($named | Where-Object { $_ -like 'Maya R.*' })) 'the student heading is there'
    Check (($named | Where-Object { $_ -like 'The 95%*' }).Count -ge 1) 'the transcript text is readable'
    Check (($named | Where-Object { $_ -like 'Spelling to check*' }).Count -ge 1) 'flagged words are named buttons'
    # UI Automation as AccessKit implements it has no landmark type or heading level property: the words are in
    # the name ("Maya R., heading level 1", "banner landmark"), which Narrator and NVDA read on arriving
    Check (($named | Where-Object { $_ -eq 'banner landmark' }).Count -ge 1) 'the banner landmark is named'
    Check (($named | Where-Object { $_ -eq 'main landmark' }).Count -ge 1) 'the main landmark is named'
    Check (($named | Where-Object { $_ -eq 'Word panel, complementary landmark' }).Count -ge 1) 'the word panel landmark is named'
    Check (($named | Where-Object { $_ -match '^Maya R\., heading level 1$' }).Count -ge 1) 'the student heading says its level'
    Check ($unnamedCustom -lt 10) "almost no unnamed custom nodes ($unnamedCustom)"
    # press Zoom in through UI Automation: the Fit button's name carries the zoom level
    $zin = $all | Where-Object { $_.Current.Name -eq 'Zoom in' -and $_.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' } | Select-Object -First 1
    $fit = { $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object { $_.Current.Name -like 'Fit the page*' } | ForEach-Object { $_.Current.Name } | Select-Object -First 1 }
    $before = & $fit
    if ($zin) {
      $pat = $null
      if ($zin.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pat)) { $pat.Invoke() }
      Start-Sleep -Milliseconds 800
      $after = & $fit
      Check ($before -ne $after) "Invoke on Zoom in reaches the app: '$before' -> '$after'"
    }
  }
} finally {
  if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
if ($fail.Count) { Write-Host "`n$($fail.Count) FAILED"; exit 1 }
Write-Host "`nnative accessibility checks passed"
