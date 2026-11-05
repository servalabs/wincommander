[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ExecutablePath,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$InstallDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
    # This helper is permitted only from the non-elevating current-user NSIS
    # mode. The expected profile comes from this process token and Windows, not
    # from the caller-supplied LOCALAPPDATA environment variable.
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if ($null -eq $identity.User) { throw 'Windows user identity unavailable.' }
    $sid = $identity.User.Value
    $local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if ([string]::IsNullOrWhiteSpace($local)) { throw 'Windows user profile unavailable.' }
    $expected = [IO.Path]::GetFullPath((Join-Path $local 'WinCommander')).TrimEnd('\')
    $actual = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
    if (-not [string]::Equals($actual, $expected, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'This installer is not running as the user who owns the target installation.'
    }
    $installed = Join-Path $actual 'wincommander-free.exe'
    $binaryMissing = $false
    try { [void][IO.File]::GetAttributes($installed) }
    catch [IO.FileNotFoundException] { $binaryMissing = $true }
    catch [IO.DirectoryNotFoundException] { $binaryMissing = $true }
    if ($binaryMissing) {
        $rootMissing = $false
        try { [void][IO.File]::GetAttributes($actual) }
        catch [IO.FileNotFoundException] { $rootMissing = $true }
        catch [IO.DirectoryNotFoundException] { $rootMissing = $true }
        if ($rootMissing) { return }
        # PREINSTALL already creates this diagnostic log on a fresh install.
        # No other file or directory may be treated as an empty profile.
        foreach ($entry in [IO.Directory]::EnumerateFileSystemEntries($actual)) {
            if (-not [string]::Equals([IO.Path]::GetFileName($entry), 'installer-lifecycle.log', [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Existing profile contents require recovery because the app binary is missing.'
            }
        }
        return
    }
    $helper = [IO.Path]::GetFullPath($ExecutablePath)
    if (-not [IO.File]::Exists($helper)) { throw 'Settings staging helper unavailable.' }
    $env:WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION = '1'
    & $helper --settings-generation-lab stage-user --offline-confirmed --expected-user-sid $sid
    if ($LASTEXITCODE -ne 0) { throw "User settings staging returned exit code $LASTEXITCODE." }
} catch {
    Write-Error "Original-user staging failed; existing settings remain active. $($_.Exception.Message)"
    exit 1
}
