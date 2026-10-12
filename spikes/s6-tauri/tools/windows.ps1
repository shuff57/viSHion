# List visible top-level windows of processes whose name matches the arg (default s6-desktop).
param([string]$Name = 's6-desktop')
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class EW {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(P cb, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  public static List<string> All(HashSet<int> pids) {
    var r = new List<string>();
    EnumWindows((h, l) => {
      int pid; GetWindowThreadProcessId(h, out pid);
      if (pids.Contains(pid) && IsWindowVisible(h)) {
        var t = new StringBuilder(256); var c = new StringBuilder(256);
        GetWindowText(h, t, 256); GetClassName(h, c, 256);
        r.Add(pid + "|" + c + "|" + t);
      }
      return true; }, IntPtr.Zero);
    return r;
  }
}
"@
$pids = New-Object 'System.Collections.Generic.HashSet[int]'
Get-Process | Where-Object { $_.ProcessName -like "*$Name*" } | ForEach-Object { [void]$pids.Add($_.Id) }
[EW]::All($pids)
