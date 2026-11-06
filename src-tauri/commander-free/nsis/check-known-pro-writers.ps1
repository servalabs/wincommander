[CmdletBinding()]
param([Parameter(Mandatory)][string]$InstallDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# A copied or renamed old executable is outside this check. Do not terminate
# processes here: a backup or user operation could be active.
function Find-KnownProWriters([object[]]$Processes, [string[]]$KnownPaths) {
    $matches = [System.Collections.Generic.List[int]]::new()
    foreach ($process in $Processes) {
        if ([string]::IsNullOrWhiteSpace($process.ExecutablePath)) {
            throw 'Cannot determine the location of a running Pro process.'
        }
        $path = [IO.Path]::GetFullPath($process.ExecutablePath)
        foreach ($known in $KnownPaths) {
            if ([string]::Equals($path, $known, [StringComparison]::OrdinalIgnoreCase)) {
                $matches.Add([int]$process.ProcessId)
                break
            }
        }
    }
    return @($matches | Sort-Object -Unique)
}
if ($MyInvocation.InvocationName -eq '.') { return }
try {
    $machine = [Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)
    if ([string]::IsNullOrWhiteSpace($machine)) { throw 'Machine data directory is unavailable.' }
    $paths = @(
        [IO.Path]::GetFullPath((Join-Path $machine 'WinCommander\bin\wincommander-pro.exe')),
        [IO.Path]::GetFullPath((Join-Path $InstallDirectory 'wincommander-pro.exe'))
    )
    $processes = @(Get-CimInstance -ClassName Win32_Process -Filter "Name='wincommander-pro.exe'")
    $matches = @(Find-KnownProWriters $processes $paths)
    if ($matches.Count -gt 0) {
        [Console]::Error.WriteLine('WinCommander Pro is still running. Close Pro operations in all Windows sessions and retry the update.')
        exit 2
    }
    exit 0
} catch {
    [Console]::Error.WriteLine('Cannot verify that WinCommander Pro has stopped; retry the update after closing it.')
    exit 1
}
