[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('prepare','verify','commit')][string]$Phase,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ExecutablePath,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$InstallDirectory,
    [string]$Transaction = ''
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
    $txPrefix = 'WINCOMMANDER_SEARCH_TRANSACTION_OUTCOME:'
    if ($Phase -eq 'prepare') {
        $pending = Invoke-Helper '--search-migration-transaction inspect placeholder' $txPrefix
        if ($pending -ceq 'none') {
            $random = New-Object byte[] 16
            $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
            try { $rng.GetBytes($random) } finally { $rng.Dispose() }
            $token = -join ($random | ForEach-Object { $_.ToString('x2') })
            if ($token -ceq ('0' * 32)) { throw 'Could not generate search transaction identity.' }
        } elseif ($pending -cmatch '^(?:intent|replacement-pending) ([0-9a-f]{32})$') {
            $token = $Matches[1]
        } else { throw 'Pending search transaction is invalid; retry setup or recover it.' }
        $result = Invoke-Helper "--search-migration-transaction prepare $token" $txPrefix
        if ($result -ceq 'fresh' -or $result -ceq 'already-quarantined') {
            [Console]::Out.WriteLine('NONE')
        } elseif ($result -ceq 'replacement-pending reconciliation=pending') {
            [Console]::Out.WriteLine($token)
        } else { throw 'Search migration preparation needs recovery.' }
    } elseif ($Phase -eq 'verify') {
        $result = Invoke-Helper '--search-migration-lab verify' 'WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:'
        if ($result -cne 'fresh' -and $result -cne 'verified') { throw 'Search verification outcome was invalid.' }
        [Console]::Out.WriteLine($result)
    } else {
        if ($Transaction -cnotmatch '^[0-9a-f]{32}$') { throw 'Search transaction identity is invalid.' }
        $result = Invoke-Helper "--search-migration-transaction commit $Transaction" $txPrefix
        if ($result -cne 'committed') { throw 'Search transaction commit was not confirmed.' }
        [Console]::Out.WriteLine('committed')
    }

} catch {
    [Console]::Error.WriteLine("Search migration failed; existing data was preserved. $($_.Exception.Message)")
    exit 1
}
