# Real OLE file drag: a WinForms source window drags a FileDrop onto screen coords (tx,ty).
# usage: filedrag.ps1 <file> <targetX> <targetY>
param([string]$File, [int]$TX, [int]$TY)
. "$PSScriptRoot\win.ps1" x
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$f = New-Object System.Windows.Forms.Form
$f.Text = 's6-drag-source'; $f.StartPosition = 'Manual'; $f.Location = New-Object System.Drawing.Point(20, 20)
$f.Size = New-Object System.Drawing.Size(200, 120); $f.TopMost = $true
$l = New-Object System.Windows.Forms.Label
$l.Text = 'DRAG ME'; $l.Dock = 'Fill'; $l.BackColor = 'Orange'
$f.Controls.Add($l)
$script:done = $false
$l.Add_MouseDown({
  $d = New-Object System.Windows.Forms.DataObject
  $c = New-Object System.Collections.Specialized.StringCollection
  [void]$c.Add($File)
  $d.SetFileDropList($c)
  [void]$l.DoDragDrop($d, [System.Windows.Forms.DragDropEffects]::Copy)
  $script:done = $true
  $f.Close()
})
$t = New-Object System.Windows.Forms.Timer
$t.Interval = 40
$state = 0; $i = 0; $n = 40
$sx = 0; $sy = 0
$t.Add_Tick({
  switch ($script:state) {
    0 { $o = $f.PointToScreen((New-Object System.Drawing.Point(100, 60))); $script:sx = $o.X; $script:sy = $o.Y
        Move-Cursor $script:sx $script:sy; $script:state = 1 }
    1 { [W]::Down(); $script:state = 2 }
    2 { $script:i++
        Move-Cursor ($script:sx + ($TX - $script:sx) * $script:i / $n) ($script:sy + ($TY - $script:sy) * $script:i / $n)
        if ($script:i -ge $n) { $script:state = 3; $script:i = 0 } }
    3 { $script:i++; if ($script:i -gt 10) { [W]::Up(); $script:state = 4; $script:i = 0 } }
    4 { $script:i++; if ($script:i -gt 25) { $f.Close() } }
  }
})
$script:state = 0; $script:i = 0
$f.Add_Shown({ $t.Start() })
$f.Add_FormClosed({ $t.Stop() })
[void][System.Windows.Forms.Application]::Run($f)
"done=$script:done"
