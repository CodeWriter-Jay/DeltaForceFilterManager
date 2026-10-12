$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force artifacts/native | Out-Null
$report = Join-Path (Get-Location) 'artifacts/native/smoke.json'
$process = Start-Process artifacts/portable/DeltaForceFilterManager.exe -ArgumentList '--smoke-test', $report -PassThru
$deadline = [DateTime]::UtcNow.AddSeconds(40)
while (-not (Test-Path $report)) {
  if ($process.HasExited) { throw "Native app exited before smoke report: $($process.ExitCode)" }
  if ([DateTime]::UtcNow -gt $deadline) { $process.Kill(); throw 'Native IPC smoke test timed out.' }
  Start-Sleep -Milliseconds 300
}
$result = Get-Content $report -Raw | ConvertFrom-Json
if (-not $result.passed) { Get-Content $report; $process.Kill(); throw 'Native IPC smoke failed.' }
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class WindowCapture {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hgt, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect rect);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
}
'@
$process.Refresh()
[WindowCapture]::SetWindowPos($process.MainWindowHandle, [IntPtr]::Zero, 10, 10, 980, 720, 0) | Out-Null
[WindowCapture]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
Start-Sleep -Seconds 2
$rect = New-Object WindowCapture+Rect
if (-not [WindowCapture]::GetWindowRect($process.MainWindowHandle, [ref]$rect)) { throw 'Cannot locate native window.' }
$bitmap = New-Object System.Drawing.Bitmap(($rect.Right-$rect.Left), ($rect.Bottom-$rect.Top))
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
$bitmap.Save((Join-Path (Get-Location) 'artifacts/native/workspace.png'))
$graphics.Dispose(); $bitmap.Dispose()
$trackedIds = [System.Collections.Generic.HashSet[int]]::new()
$trackedIds.Add($process.Id) | Out-Null
function Get-AppTree {
  $all = @(Get-CimInstance Win32_Process)
  $ids = $trackedIds
  do {
    $added = $false
    foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true } }
  } while ($added)
  return @($all | Where-Object { $ids.Contains([int]$_.ProcessId) })
}
function Measure-Idle($phase) {
  $before = Get-AppTree
  $cpuBefore = ($before | ForEach-Object { [double]$_.KernelModeTime + [double]$_.UserModeTime } | Measure-Object -Sum).Sum
  $timer = [Diagnostics.Stopwatch]::StartNew()
  Start-Sleep -Seconds 3
  $after = Get-AppTree
  $timer.Stop()
  $cpuAfter = ($after | ForEach-Object { [double]$_.KernelModeTime + [double]$_.UserModeTime } | Measure-Object -Sum).Sum
  return [ordered]@{
    phase=$phase; elapsedSeconds=$timer.Elapsed.TotalSeconds
    workingSetMiB=(($after | Measure-Object -Property WorkingSetSize -Sum).Sum / 1MB)
    cpuSeconds=[Math]::Max(0, ($cpuAfter-$cpuBefore)/10000000)
    processes=@($after | ForEach-Object { [ordered]@{name=$_.Name; pid=$_.ProcessId; workingSetMiB=([double]$_.WorkingSetSize/1MB)} })
  }
}
$gui = Measure-Idle 'GUI with WebView2'
# The explicit smoke mode destroys the WebView twenty seconds after IPC verification.
$deadline = [DateTime]::UtcNow.AddSeconds(28)
do {
  Start-Sleep -Milliseconds 400
  $tree = Get-AppTree
  $webviews = @($tree | Where-Object { $_.Name -eq 'msedgewebview2.exe' })
} while ($webviews.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)
if ($webviews.Count -gt 0) { $process.Kill(); throw 'WebView2 processes were not released after tray transition.' }
$tray = Measure-Idle 'tray after WebView destruction'
[ordered]@{simulation=$true;hardwareColorTest=$false;manualMode=$true;polling=$false;gui=$gui;tray=$tray} | ConvertTo-Json -Depth 6 | Set-Content artifacts/native/idle.json -Encoding utf8
if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'Smoke app did not exit.' }
if ($process.ExitCode -ne 0) { throw "Smoke exit code $($process.ExitCode)" }
