# Type a path into the foreground native file dialog and press Enter.
# usage: dialog.ps1 <text> [waitMs]
param([string]$Text, [int]$Wait = 2500)
Add-Type -AssemblyName System.Windows.Forms
Start-Sleep -Milliseconds $Wait
$esc = [regex]::Replace($Text, '[+^%~(){}\[\]]', { param($m) '{' + $m.Value + '}' })
[System.Windows.Forms.SendKeys]::SendWait($esc)
Start-Sleep -Milliseconds 400
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
"typed"
