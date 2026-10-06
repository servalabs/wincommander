# SPDX-License-Identifier: AGPL-3.0-or-later
# Read-only diagnosis: no screenshots, window titles, command lines or settings contents.
[CmdletBinding()]
param(
    [int[]]$ProcessId,
    [switch]$IncludeOtherVisibleWindows,
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
        public bool IsTargetProcess;
        public string OwnerHwnd;
        public uint OwnerProcessId;
        public int Cloaked, CloakedQueryStatus;
        public bool Visible, Minimized, Maximized, WindowsReportsHung, Responded;
        public bool ResponseProbeAttempted;
        public long ResponseElapsedMs;
        public int ResponseError;
        public Rect Bounds;
    }
    public static bool EnumerationCompleted { get; private set; }
    public static int EnumerationError { get; private set; }
    delegate bool EnumProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll", SetLastError=true)] static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr window, uint attribute, out int value, uint size);
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
    public static Snapshot[] Capture(int[] processIds, bool includeOtherVisibleWindows) {
        var targets = new HashSet<int>(processIds);
        var windows = new List<Snapshot>();
        EnumerationError = 0;
        EnumerationCompleted = EnumWindows((window, parameter) => {
            uint pid;
            uint tid = GetWindowThreadProcessId(window, out pid);
            bool isTarget = targets.Contains((int)pid);
            var name = new StringBuilder(256);
            GetClassName(window, name, name.Capacity);
            Rect rect;
            GetWindowRect(window, out rect);
            bool visible = IsWindowVisible(window);
            if (!isTarget && !(includeOtherVisibleWindows &&
                (name.ToString() == "Ghost" || (visible && rect.Right - rect.Left > 200 && rect.Bottom - rect.Top > 200)))) return true;
            IntPtr owner = GetWindow(window, 4);
            uint ownerPid = 0;
            if (owner != IntPtr.Zero) GetWindowThreadProcessId(owner, out ownerPid);
            int cloaked;
            int cloakStatus = DwmGetWindowAttribute(window, 14, out cloaked, 4);
            var snapshot = new Snapshot {
                Hwnd = "0x" + window.ToInt64().ToString("X"), ProcessId = pid, ThreadId = tid,
                ClassName = name.ToString(), Bounds = rect,
                IsTargetProcess = isTarget,
                OwnerHwnd = "0x" + owner.ToInt64().ToString("X"), OwnerProcessId = ownerPid,
                Cloaked = cloaked, CloakedQueryStatus = cloakStatus,
                Visible = visible, Minimized = IsIconic(window), Maximized = IsZoomed(window),
                WindowsReportsHung = isTarget && IsHungAppWindow(window),
                Style = "0x" + GetWindowLong(window, -16).ToString("X8"),
                ExtendedStyle = "0x" + GetWindowLong(window, -20).ToString("X8")
            };
            if (isTarget) {
                var watch = Stopwatch.StartNew();
                UIntPtr ignored;
                // WM_NULL only probes WinCommander's message loop; other apps are metadata only.
                snapshot.ResponseProbeAttempted = true;
                snapshot.Responded = SendMessageTimeout(window, 0, IntPtr.Zero, IntPtr.Zero, 3, 200, out ignored) != IntPtr.Zero;
                snapshot.ResponseError = snapshot.Responded ? 0 : Marshal.GetLastWin32Error();
                snapshot.ResponseElapsedMs = watch.ElapsedMilliseconds;
            }
            windows.Add(snapshot);
            return windows.Count < 32;
        }, IntPtr.Zero);
        if (!EnumerationCompleted && windows.Count < 32) EnumerationError = Marshal.GetLastWin32Error();
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
$collectorSessionId = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
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
        sessionId = $process.SessionId
        inCollectorSession = $process.SessionId -eq $collectorSessionId
        name = $process.ProcessName
        executablePath = $path
        executableSha256 = $hash
        fileVersion = $version
        executableReadError = $executableReadError
    }
}
$targetIds = [int[]]@($capturedProcesses | ForEach-Object { $_.processId })
$windows = if ($targetIds.Count -or $IncludeOtherVisibleWindows) { @([WinCommanderWindowCapture]::Capture($targetIds, $IncludeOtherVisibleWindows.IsPresent)) } else { @() }
$report = [ordered]@{
    capturedUtc = [DateTime]::UtcNow.ToString('o')
    collectorSessionId = $collectorSessionId
    includesOtherVisibleWindows = $IncludeOtherVisibleWindows.IsPresent
    crossSessionTargetsPresent = @($capturedProcesses | Where-Object { -not $_.inCollectorSession }).Count -gt 0
    processes = @($capturedProcesses)
    windows = @($windows)
    enumerationCompleted = ($targetIds.Count -or $IncludeOtherVisibleWindows) -and [WinCommanderWindowCapture]::EnumerationCompleted
    enumerationError = [WinCommanderWindowCapture]::EnumerationError
    windowLimitReached = $windows.Count -eq 32
    notes = @(
        'This read-only snapshot sends only WM_NULL. It does not show, hide, focus, close or repair any window.'
        'A nonresponsive HWND or Ghost class supports a native-hang diagnosis. A responsive HWND does not prove that WebView content is healthy.'
        'Window enumeration covers only the collector desktop. Missing windows from another session are unobserved, not hidden.'
        'Other visible windows are included only when requested; their presence does not attribute a white window to WinCommander.'
        'Response and hung fields are meaningful only when ResponseProbeAttempted is true; other applications receive no messages.'
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
