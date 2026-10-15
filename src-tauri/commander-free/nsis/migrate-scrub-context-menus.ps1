# SPDX-License-Identifier: AGPL-3.0-or-later
[CmdletBinding()]
param([Parameter(Mandatory)][string]$SharedExecutable)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Repair-ScrubClasses([Microsoft.Win32.RegistryKey]$Classes, [string]$Executable) {
    $result = @{ updated = 0; skipped = 0 }
    $replacement = '"' + $Executable + '" "--scrub" "%1"'
    foreach ($class in @('*', 'Directory', 'AllFilesystemObjects')) {
        $verb = $Classes.OpenSubKey("$class\shell\WinCommanderScrub")
        if ($null -eq $verb) { continue }
        $command = $null
        try {
            # Recognize our registry verb, never ownership of the old executable.
            # This predicate must not authorize task changes or file deletion.
            $names = @($verb.GetValueNames())
            $label = $verb.GetValue('')
            $model = $verb.GetValue('MultiSelectModel')
            $unsafe = @('LegacyDisable', 'ProgrammaticAccessOnly', 'ExplorerCommandHandler', 'SubCommands', 'ExtendedSubCommandsKey', 'MUIVerb')
            if ($label -notin @('Scrub metadata', 'Scrub with WinCommander') -or
                $model -notin @('Player', 'Document') -or
                @($unsafe | Where-Object { $_ -in $names }).Count -gt 0) {
                $result.skipped++; continue
            }
            $command = $verb.OpenSubKey('command', $true)
            if ($null -eq $command -or '' -notin $command.GetValueNames()) { $result.skipped++; continue }
            $kind = $command.GetValueKind('')
            $value = $command.GetValue('', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            if ($kind -notin @([Microsoft.Win32.RegistryValueKind]::String, [Microsoft.Win32.RegistryValueKind]::ExpandString) -or
                'DelegateExecute' -in $command.GetValueNames() -or
                $value -isnot [string] -or
                $value -notmatch '^"[A-Za-z]:\\[^"\r\n]+\\wincommander-free\.exe"\s+(?:"--scrub"|--scrub)\s+"%1"$') {
                $result.skipped++; continue
            }
            if ($value -ceq $replacement) { continue }
            if ($command.GetValue('', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) -cne $value -or
                $command.GetValueKind('') -ne $kind -or
                $verb.GetValue('') -cne $label -or $verb.GetValue('MultiSelectModel') -cne $model -or
                @($unsafe | Where-Object { $_ -in $verb.GetValueNames() }).Count -gt 0 -or
                'DelegateExecute' -in $command.GetValueNames()) { throw 'scrub_route_changed' }
            $command.SetValue('', $replacement, $kind)
            if ($command.GetValue('', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) -cne $replacement -or
                $command.GetValueKind('') -ne $kind) { throw 'scrub_route_readback_failed' }
            $result.updated++
        } finally {
            if ($null -ne $command) { $command.Dispose() }
            $verb.Dispose()
        }
    }
    return $result
}

$target = [IO.Path]::GetFullPath($SharedExecutable)
if ($target -match '["\r\n]' -or -not (Test-Path -LiteralPath $target -PathType Leaf) -or
    [IO.Path]::GetFileName($target) -ine 'wincommander-free.exe') {
    throw 'Installed scrub executable could not be verified.'
}
$users = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::Users, [Microsoft.Win32.RegistryView]::Registry64)
$counts = @{ updated = 0; skipped = 0; deferred = 0 }
try {
    $profiles = @(Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\*' -ErrorAction Stop)
    foreach ($profile in $profiles) {
        $sid = [string]$profile.PSChildName
        if ($sid -notmatch '^S-1-(?:\d+-)+\d+$' -or $sid -in @('S-1-5-18', 'S-1-5-19', 'S-1-5-20')) { continue }
        $classes = $null
        try {
            # Windows owns hive loading. Never mount an offline UsrClass.dat or
            # take ownership of another account's protected profile.
            $classes = $users.OpenSubKey("${sid}_Classes", $true)
            if ($null -eq $classes) { $classes = $users.OpenSubKey("$sid\Software\Classes", $true) }
            if ($null -eq $classes) { $counts.deferred++; continue }
            $result = Repair-ScrubClasses $classes $target
            $counts.updated += $result.updated
            $counts.skipped += $result.skipped
        } catch [UnauthorizedAccessException] {
            $counts.deferred++
        } finally { if ($null -ne $classes) { $classes.Dispose() } }
    }
} catch {
    Write-Output 'Scrub context-menu migration could not be verified. Start the installed app in the affected account to refresh its menu.'
    exit 1
} finally { $users.Dispose() }
Write-Output "Scrub context-menu migration: updated=$($counts.updated) skipped=$($counts.skipped) deferred=$($counts.deferred)"
if ($counts.deferred -gt 0 -or $counts.skipped -gt 0) {
    Write-Output 'Some context menus were preserved or deferred. Start the installed app in those accounts to refresh their WinCommander menu.'
    exit 3
}
