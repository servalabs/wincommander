[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('quarantine','verify')][string]$Phase,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ExecutablePath,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$InstallDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if ($null -eq $identity.User) { throw 'Windows user identity unavailable.' }
    $local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if ([string]::IsNullOrWhiteSpace($local)) { throw 'Windows user profile unavailable.' }
    $expected = [IO.Path]::GetFullPath((Join-Path $local 'WinCommander')).TrimEnd('\')
    $actual = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
    if (-not [string]::Equals($actual, $expected, [StringComparison]::OrdinalIgnoreCase)) { throw 'The installer is not running as the target user.' }
    $helper = [IO.Path]::GetFullPath($ExecutablePath)
    if (-not [IO.File]::Exists($helper)) { throw 'Search migration helper unavailable.' }
    $env:WINCOMMANDER_EXPERIMENTAL_SEARCH_MIGRATION = '1'
    & $helper --search-migration-lab $Phase
    if ($LASTEXITCODE -ne 0) { throw "Search migration phase returned exit code $LASTEXITCODE." }
} catch {
    [Console]::Error.WriteLine("Search migration failed; existing data was preserved. $($_.Exception.Message)")
    exit 1
}
