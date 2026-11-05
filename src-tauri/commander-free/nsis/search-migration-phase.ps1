[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('stage','verify-staged')][string]$Phase,
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
    function Invoke-Helper([string]$Arguments, [string]$Prefix) {
        $info = [Diagnostics.ProcessStartInfo]::new()
        $info.FileName = $helper
        $info.Arguments = $Arguments
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
            if (-not $process.WaitForExit(540000)) {
                $process.Kill()
                $process.WaitForExit()
                throw 'Search migration helper timed out; retry setup to recover.'
            }
            if ($process.ExitCode -ne 0) { throw "Search migration helper returned exit code $($process.ExitCode)." }
            $output = $stdout.Result.Trim()
            if (-not $output.StartsWith($Prefix, [StringComparison]::Ordinal)) { throw 'Search migration outcome was not confirmed.' }
            return $output.Substring($Prefix.Length)
        } finally { $process.Dispose() }
    }
    $result = Invoke-Helper "--search-migration-lab $Phase" 'WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:'
    $pattern = if ($Phase -eq 'stage') {
        '^(fresh|staged reconciliation=pending old-cache=retained|already-quarantined)$'
    } else { '^(fresh|staged old-cache=retained|already-quarantined)$' }
    if ($result -cnotmatch $pattern) { throw 'Protected search staging outcome was not confirmed.' }
    [Console]::Out.WriteLine($result)

} catch {
    [Console]::Error.WriteLine("Search migration failed; existing data was preserved. $($_.Exception.Message)")
    exit 1
}
