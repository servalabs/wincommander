[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('quarantine','verify','restore')][string]$Phase,
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
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $helper
    $info.Arguments = "--search-migration-lab $Phase"
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.EnvironmentVariables['WINCOMMANDER_EXPERIMENTAL_SEARCH_MIGRATION'] = '1'
    $process = [Diagnostics.Process]::Start($info)
    if ($null -eq $process) { throw 'Search migration helper could not start.' }
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "Search migration phase returned exit code $($process.ExitCode)." }
        $outcome = $stdout.Result.Trim()
        $pattern = switch ($Phase) {
            'quarantine' { '^WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:(fresh|moved(?: indexed=[0-9]+ omitted=[0-9]+ links=[0-9]+(?: reconciliation=pending)?)?|already-quarantined)$' }
            'verify' { '^WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:(fresh|verified)$' }
            'restore' { '^WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:restored$' }
        }
        if ($outcome -cnotmatch $pattern) { throw 'Search migration outcome was not confirmed.' }
        [Console]::Out.WriteLine($outcome)
    } finally { $process.Dispose() }
} catch {
    [Console]::Error.WriteLine("Search migration failed; existing data was preserved. $($_.Exception.Message)")
    exit 1
}
