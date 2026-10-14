# Helpers for driving real OS input. Dot-source from other scripts or run: win.ps1 pos "<window title>"
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing @"
using System; using System.Runtime.InteropServices; using System.Drawing;
public static class W {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  public static void Down() { mouse_event(0x0002,0,0,0,UIntPtr.Zero); }
  public static void Up()   { mouse_event(0x0004,0,0,0,UIntPtr.Zero); }
  public static Point ClientOrigin(IntPtr h) { POINT p = new POINT(); ClientToScreen(h, ref p); return new Point(p.X, p.Y); }
}
"@
[void][W]::SetProcessDPIAware()

function Get-ClientOrigin([string]$title) {
  $h = [W]::FindWindow([NullString]::Value, $title)
  if ($h -eq [IntPtr]::Zero) { throw "window not found: $title" }
  [W]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 300
  $p = [W]::ClientOrigin($h)
  return $p
}
function Move-Cursor($x, $y) { [W]::SetCursorPos([int]$x, [int]$y) | Out-Null }
function Drag-Mouse($x1, $y1, $x2, $y2) {
  Move-Cursor $x1 $y1; Start-Sleep -Milliseconds 150
  [W]::Down(); Start-Sleep -Milliseconds 150
  $n = 25
  for ($i = 1; $i -le $n; $i++) {
    Move-Cursor ($x1 + ($x2 - $x1) * $i / $n) ($y1 + ($y2 - $y1) * $i / $n); Start-Sleep -Milliseconds 25
  }
  Start-Sleep -Milliseconds 200
  [W]::Up(); Start-Sleep -Milliseconds 300
}
if ($args[0] -eq 'pos') { $p = Get-ClientOrigin $args[1]; "$($p.X),$($p.Y)" }
if ($args[0] -eq 'drag') { Drag-Mouse $args[1] $args[2] $args[3] $args[4] }
