. "$PSScriptRoot/../src-tauri/commander-free/nsis/check-known-pro-writers.ps1" -InstallDirectory 'C:\Apps\WinCommander'
$known = @('C:\ProgramData\WinCommander\bin\wincommander-pro.exe', 'C:\Apps\WinCommander\wincommander-pro.exe')
$fixtures = @(
    [pscustomobject]@{ ProcessId = 11; ExecutablePath = 'c:\programdata\WINCOMMANDER\bin\wincommander-pro.exe' },
    [pscustomobject]@{ ProcessId = 12; ExecutablePath = 'C:\Other\wincommander-pro.exe' }
)
$found = @(Find-KnownProWriters $fixtures $known)
if ($found.Count -ne 1 -or $found[0] -ne 11) { throw 'Known path matching failed.' }
try {
    Find-KnownProWriters @([pscustomobject]@{ ProcessId = 13; ExecutablePath = $null }) $known
    throw 'An inaccessible process path was accepted.'
} catch {
    if ($_.Exception.Message -eq 'An inaccessible process path was accepted.') { throw }
}
Write-Host 'Known Pro process matching fixture passed.'
