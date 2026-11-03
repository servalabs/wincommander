# Tauri calls its --runner as `runner run [Cargo flags] -- [app args]`.
# Build first, then launch only the service-pinned copy of the debug image.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$CargoSubcommand = if ($args.Count) { [string]$args[0] } else { '' }
$RunnerArgs = @($args | Select-Object -Skip 1)
if ($CargoSubcommand -cne 'run') {
    throw 'The protected development runner only accepts Tauri development runs.'
}

$delimiter = [Array]::IndexOf($RunnerArgs, '--')
if ($delimiter -lt 0) {
    throw 'The protected development runner requires the Cargo/app argument delimiter.'
}
$buildArgs = @($RunnerArgs | Select-Object -First $delimiter)
$appArgs = @($RunnerArgs | Select-Object -Skip ($delimiter + 1))
if ($buildArgs -contains '--release' -or $buildArgs -contains '--profile' -or
    $buildArgs -contains '--target' -or $buildArgs -contains '--bin') {
    throw 'The protected development runner only accepts the default Free debug binary.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$expected = [IO.Path]::GetFullPath((Join-Path $repoRoot 'src-tauri\target\debug\wincommander-free.exe'))
$cargo = Get-Command cargo.exe -CommandType Application -ErrorAction Stop |
    Select-Object -First 1 -ExpandProperty Path
& $cargo build -p commander-free --bin wincommander-free @buildArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if (-not (Test-Path -LiteralPath $expected -PathType Leaf)) {
    throw 'The protected development build did not produce the Free executable.'
}
if ([string]::IsNullOrWhiteSpace($env:ProgramData)) {
    throw 'The protected development staging directory is unavailable.'
}

$builtHash = (Get-FileHash -LiteralPath $expected -Algorithm SHA256).Hash
& (Join-Path $PSScriptRoot 'sync-dev-service.ps1') -SyncPro -StageFleetDebug
if (-not $?) { throw 'Protected development staging failed.' }

$staged = Join-Path $env:ProgramData 'WinCommander\dev-fleet\wincommander-free.exe'
$pin = Join-Path $env:ProgramData 'WinCommander\dev-fleet\wincommander-free.sha256'
if (-not (Test-Path -LiteralPath $staged -PathType Leaf) -or
    -not (Test-Path -LiteralPath $pin -PathType Leaf) -or
    (Get-FileHash -LiteralPath $staged -Algorithm SHA256).Hash -ne $builtHash -or
    (Get-Content -LiteralPath $pin -Raw).Trim() -ne $builtHash) {
    throw 'The protected development image or service pin does not match the Tauri build.'
}

function Get-ProcessImagePath {
    param([int]$ProcessId)
    if (-not ('WcDevProcessImage' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class WcDevProcessImage {
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenProcess(uint access, bool inherit, int processId);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool QueryFullProcessImageName(IntPtr process, int flags, StringBuilder path, ref int size);
    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr process);
    public static string Read(int processId) {
        IntPtr process = OpenProcess(0x1000, false, processId);
        if (process == IntPtr.Zero) return null;
        try {
            var path = new StringBuilder(32768);
            int size = path.Capacity;
            return QueryFullProcessImageName(process, 0, path, ref size) ? path.ToString() : null;
        } finally { CloseHandle(process); }
    }
}
'@
    }
    [WcDevProcessImage]::Read($ProcessId)
}

function Get-NewStagedProcess {
    param([string]$Path, [int]$SessionId, [int[]]$KnownIds)
    foreach ($candidate in @(Get-Process -Name 'wincommander-free' -ErrorAction SilentlyContinue)) {
        if ($KnownIds -contains $candidate.Id) { continue }
        try {
            if ($candidate.SessionId -ne $SessionId) { continue }
            $image = Get-ProcessImagePath -ProcessId $candidate.Id
            if ($image -and $image.Equals($Path, [StringComparison]::OrdinalIgnoreCase)) {
                return $candidate
            }
        } catch [System.InvalidOperationException] {
            # The process exited between enumeration and inspection.
        }
    }
    return $null
}

$sessionId = (Get-Process -Id $PID).SessionId
$knownIds = @()
foreach ($process in @(Get-Process -Name 'wincommander-free' -ErrorAction SilentlyContinue)) {
    try {
        $image = Get-ProcessImagePath -ProcessId $process.Id
        if ($process.SessionId -eq $sessionId -and $image -and
            $image.Equals($staged, [StringComparison]::OrdinalIgnoreCase)) {
            $knownIds += $process.Id
        }
    } catch [System.InvalidOperationException] {
        # A process that exited cannot affect the handoff.
    }
}

& $staged @appArgs
$appExitCode = $LASTEXITCODE
# The app may hand off to a new elevated process; Tauri must remain alive until that image exits.
$handoffDeadline = [DateTime]::UtcNow.AddSeconds(8)
while ([DateTime]::UtcNow -lt $handoffDeadline) {
    $active = Get-NewStagedProcess -Path $staged -SessionId $sessionId -KnownIds $knownIds
    if ($null -eq $active) {
        Start-Sleep -Milliseconds 250
        continue
    }
    $knownIds += $active.Id
    do {
        Start-Sleep -Milliseconds 250
        $active.Refresh()
    } while (-not $active.HasExited)
    $handoffDeadline = [DateTime]::UtcNow.AddSeconds(8)
}
exit $appExitCode
