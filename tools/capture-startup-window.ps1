# SPDX-License-Identifier: AGPL-3.0-or-later
# Read-only diagnosis: no screenshots, window titles, command lines or settings contents.
[CmdletBinding()]
param(
    [int[]]$ProcessId,
    [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
if (-not ('WinCommanderWindowCapture' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class WinCommanderWindowCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    public sealed class Snapshot {
        public string Hwnd, ClassName, Style, ExtendedStyle;
        public uint ProcessId, ThreadId;
        public bool Visible, Minimized, Maximized, WindowsReportsHung, Responded;
        public long ResponseElapsedMs;
        public int ResponseError;
        public Rect Bounds;
    }
    delegate bool EnumProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsZoomed(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsHungAppWindow(IntPtr window);
    [DllImport("user32.dll", EntryPoint="GetWindowLongW")] static extern int GetWindowLong(IntPtr window, int index);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr SendMessageTimeout(IntPtr window, uint message, IntPtr wp, IntPtr lp, uint flags, uint timeout, out UIntPtr result);
    public static Snapshot[] Capture(int[] processIds) {
        var targets = new HashSet<int>(processIds);
        var windows = new List<Snapshot>();
        EnumWindows((window, parameter) => {
            uint pid;
            uint tid = GetWindowThreadProcessId(window, out pid);
            if (!targets.Contains((int)pid)) return true;
            var name = new StringBuilder(256);
            GetClassName(window, name, name.Capacity);
            Rect rect;
            GetWindowRect(window, out rect);
            var snapshot = new Snapshot {
                Hwnd = "0x" + window.ToInt64().ToString("X"), ProcessId = pid, ThreadId = tid,
                ClassName = name.ToString(), Bounds = rect,
                Visible = IsWindowVisible(window), Minimized = IsIconic(window), Maximized = IsZoomed(window),
                WindowsReportsHung = IsHungAppWindow(window),
                Style = "0x" + GetWindowLong(window, -16).ToString("X8"),
                ExtendedStyle = "0x" + GetWindowLong(window, -20).ToString("X8")
            };
            var watch = Stopwatch.StartNew();
            UIntPtr ignored;
            // WM_NULL only probes the message loop. Never activates or changes the window.
            snapshot.Responded = SendMessageTimeout(window, 0, IntPtr.Zero, IntPtr.Zero, 3, 200, out ignored) != IntPtr.Zero;
            snapshot.ResponseError = snapshot.Responded ? 0 : Marshal.GetLastWin32Error();
            snapshot.ResponseElapsedMs = watch.ElapsedMilliseconds;
            windows.Add(snapshot);
            return windows.Count < 32;
        }, IntPtr.Zero);
        return windows.ToArray();
    }
}
'@
}

if (-not $ProcessId) {
    $ProcessId = @(Get-Process -Name 'wincommander-free', 'WinCommander' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
}
if ($ProcessId.Count -gt 8) { throw 'Capture accepts at most eight WinCommander processes.' }
$capturedProcesses = @()
foreach ($targetId in @($ProcessId | Select-Object -Unique)) {
    if ($targetId -le 0) { throw 'Process IDs must be positive.' }
    $process = Get-Process -Id $targetId -ErrorAction SilentlyContinue
    if (-not $process) { continue }
    if ($process.ProcessName -notin @('wincommander-free', 'WinCommander')) {
        throw "Process $targetId is not a WinCommander desktop process."
    }
    $path = $null
    $hash = $null
    $version = $null
    $executableReadError = $null
    try {
        $path = $process.Path
        if ($path) {
            $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
            $version = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($path).FileVersion
        } else { $executableReadError = 'Executable path unavailable for this token.' }
    } catch { $executableReadError = 'Executable metadata unavailable; retry capture from an administrator PowerShell if the app is elevated.' }
    $capturedProcesses += [ordered]@{
        processId = $process.Id
        name = $process.ProcessName
        executablePath = $path
        executableSha256 = $hash
        fileVersion = $version
        executableReadError = $executableReadError
    }
}
$targetIds = [int[]]@($capturedProcesses | ForEach-Object { $_.processId })
$windows = if ($targetIds.Count) { @([WinCommanderWindowCapture]::Capture($targetIds)) } else { @() }
$report = [ordered]@{
    capturedUtc = [DateTime]::UtcNow.ToString('o')
    processes = @($capturedProcesses)
    windows = @($windows)
    windowLimitReached = $windows.Count -eq 32
    notes = @(
        'This read-only snapshot sends only WM_NULL. It does not show, hide, focus, close or repair any window.'
        'A nonresponsive HWND or Ghost class supports a native-hang diagnosis. A responsive HWND does not prove that WebView content is healthy.'
        'ResponseError 5 means access was denied; it does not prove a hang. WindowsReportsHung is a Windows heuristic.'
        'Executable hash describes the file currently on disk; a running process may still use an older image after an update.'
        'No window titles, screenshots, command lines, credentials or saved settings were collected.'
    )
}
$json = $report | ConvertTo-Json -Depth 6
if ($OutputPath) {
    $resolvedOutput = [System.IO.Path]::GetFullPath($OutputPath)
    # CreateNew avoids replacing an existing file or following an existing output link.
    $stream = [System.IO.File]::Open($resolvedOutput, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
    try {
        $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json)
        $stream.Write($bytes, 0, $bytes.Length)
    } finally { $stream.Dispose() }
    Write-Output "Saved native window snapshot: $resolvedOutput"
} else { $json }
