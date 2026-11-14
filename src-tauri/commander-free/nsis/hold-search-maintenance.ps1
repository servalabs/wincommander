[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ExecutablePath,
    [Parameter(Mandatory)][ValidateRange(1, 2147483647)][int]$InstallerProcessId
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
    $helper = [IO.Path]::GetFullPath($ExecutablePath)
    if (-not [IO.File]::Exists($helper)) { throw 'Search maintenance helper unavailable.' }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $helper
    $info.Arguments = "--hold-search-maintenance-fence $InstallerProcessId"
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    # Do not let the long-lived child inherit nsExec's stderr pipe.
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    if ($null -eq $process) { throw 'Search maintenance helper failed to start.' }
    try {
        $ready = $process.StandardOutput.ReadLineAsync()
        if (-not $ready.Wait(60000)) { throw 'Search maintenance ownership timed out.' }
        if ($ready.Result -cnotmatch '^WINCOMMANDER_SEARCH_FENCE_HANDLE_V1:([0-9]+)$') { throw 'Search maintenance handle was not confirmed.' }
        [Console]::Out.WriteLine($Matches[1])
    } finally { $process.Dispose() }
} catch {
    [Console]::Error.WriteLine("Search maintenance could not be established. Existing data remains active. $($_.Exception.Message)")
    exit 1
}
