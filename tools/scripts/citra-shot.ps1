param(
  [Parameter(Mandatory)] [string]$Rom,
  [Parameter(Mandatory)] [string]$Out,
  [Parameter(Mandatory)] [string]$Citra,   # a *portable copy* of citra.exe (see CLAUDE.md), never the owner's install
  [int]$Wait = 8
)
# Starts Citra (SDL frontend) with a .3dsx, waits, captures its window into a PNG, stops only that process.
# PrintWindow(PW_RENDERFULLCONTENT) captures the GL/Vulkan surface; CopyFromScreen gives black.
# Citra's log goes to "$Out.err" (Debug.Emulated lines = the game's svcOutputDebugString / goat log).
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[W]::SetProcessDPIAware() | Out-Null
$p = Start-Process -FilePath $Citra -ArgumentList "`"$Rom`"" -PassThru -RedirectStandardOutput "$Out.log" -RedirectStandardError "$Out.err"
Write-Output "citra pid $($p.Id)"
try {
  Start-Sleep -Seconds $Wait
  $p.Refresh()
  if ($p.HasExited) { Write-Output "citra exited early (crash? see $Out.err and the Windows Application event log)"; exit 1 }
  $h = $p.MainWindowHandle
  if ($h -eq [IntPtr]::Zero) { Write-Output "no window"; exit 1 }
  $r = New-Object W+RECT
  [W]::GetWindowRect($h, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $ht = $r.B - $r.T
  Write-Output "window $w x $ht '$($p.MainWindowTitle)'"
  $bmp = New-Object System.Drawing.Bitmap $w, $ht
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  [W]::PrintWindow($h, $hdc, 2) | Out-Null
  $g.ReleaseHdc($hdc); $g.Dispose()
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  Write-Output "saved $Out"
} finally {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
}
