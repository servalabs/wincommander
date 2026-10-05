[CmdletBinding()]
param(
    [ValidateSet('Seed', 'Inspect', 'Migrate', 'Cleanup')][string]$RegistryViewFixtureAction,
    [guid]$RegistryFixtureId,
    [string]$ResultPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Read-ScriptAst([string]$RelativePath) {
    $tokens = $null; $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $RelativePath), [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw "Invalid script: $RelativePath" }
    return $ast
}

if ($RegistryViewFixtureAction) {
    # CLSID is redirected per architecture even in HKCU, allowing a real
    # two-view test without administrator rights or any live startup key.
    if ($RegistryFixtureId -eq [guid]::Empty) { throw 'A disposable fixture ID is required.' }
    $subkey = "Software\Classes\CLSID\{$RegistryFixtureId}\WinCommanderInstallerTests"
    $target = 'C:\Program Files\WinCommander\wincommander-free.exe'
    $foreign = '"C:\Foreign\other.exe" --minimized'
    if ($RegistryViewFixtureAction -eq 'Migrate') {
        $config = Read-ScriptAst '../src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'
        foreach ($definition in $config.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.FunctionDefinitionAst] }) {
            . ([scriptblock]::Create($definition.Extent.Text))
        }
        $canonicalRunValueNames = @('WinCommander', 'WinCommander Free', 'WinCommander Pro')
        $runValueNames = @($canonicalRunValueNames) + @($canonicalRunValueNames | ForEach-Object { "${_}__SystemCache"; "${_}__WC_Hidden" })
        $paths = @("Registry::HKEY_CURRENT_USER\$subkey", "Registry::HKEY_CURRENT_USER\$($subkey.Replace('Classes\', 'Classes\WOW6432Node\'))")
        $removed = Remove-OwnedRunValues $paths @($target)
        @{ bits = [IntPtr]::Size * 8; removed = $removed } | ConvertTo-Json -Compress | Set-Content -LiteralPath $ResultPath
        exit 0
    }
    $observations = @()
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, $view)
        $key = $null
        try {
            if ($RegistryViewFixtureAction -eq 'Cleanup') {
                $base.DeleteSubKeyTree("Software\Classes\CLSID\{$RegistryFixtureId}", $false)
                continue
            }
            if ($RegistryViewFixtureAction -eq 'Seed') {
                $key = $base.CreateSubKey($subkey)
                $key.SetValue('WinCommander__SystemCache', ('"' + $target + '" --minimized'))
                $key.SetValue('WinCommander Pro__WC_Hidden', ('"' + $target + '" --autostart'))
                $key.SetValue('WinCommander Free__SystemCache', $foreign)
            } else {
                $key = $base.OpenSubKey($subkey)
                if ($null -eq $key) { throw 'Fixture registry key disappeared.' }
                $observations += @{ view = $view.ToString(); current = $key.GetValue('WinCommander__SystemCache', $null); legacy = $key.GetValue('WinCommander Pro__WC_Hidden', $null); foreign = $key.GetValue('WinCommander Free__SystemCache', $null) }
            }
        } finally {
            if ($null -ne $key) { $key.Dispose() }
            $base.Dispose()
        }
    }
    if ($RegistryViewFixtureAction -eq 'Inspect') { ConvertTo-Json -InputObject $observations -Compress }
    exit 0
}

$ast = Read-ScriptAst '../src-tauri/commander-free/nsis/migrate-legacy-user-launches.ps1'
foreach ($definition in $ast.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.FunctionDefinitionAst] }) {
    . ([scriptblock]::Create($definition.Extent.Text))
}
$canonicalRunValueNames = @('WinCommander', 'WinCommander Free', 'WinCommander Pro')
$runValueNames = @($canonicalRunValueNames) + @($canonicalRunValueNames | ForEach-Object { "${_}__SystemCache"; "${_}__WC_Hidden" })
$root = "HKCU:\Software\ServaLabs\WinCommander\InstallerTests\$([guid]::NewGuid().ToString('N'))"
$shared = 'C:\Program Files\WinCommander\wincommander-free.exe'
New-Item -Path $root -Force | Out-Null
try {
    # A credential-elevated installer runs under another account's environment.
    # Check both admin and standard profile routes, including REG_EXPAND_SZ.
    foreach ($profilePath in @('C:\Users\FixtureAdmin', 'C:\Users\FixtureStandard')) {
        $owned = @($shared, (Join-Path $profilePath 'AppData\Local\WinCommander\wincommander-free.exe'), (Join-Path $profilePath 'AppData\Local\Programs\WinCommander\wincommander-free.exe'))
        foreach ($command in @('"%LOCALAPPDATA%\WinCommander\wincommander-free.exe" --autostart', '"%USERPROFILE%\AppData\Local\WinCommander\wincommander-free.exe" --minimized', '"%LOCALAPPDATA%\Programs\WinCommander\wincommander-free.exe" --autostart')) {
            foreach ($kind in @('String', 'ExpandString')) {
                New-ItemProperty -Path $root -Name 'WinCommander Pro__SystemCache' -PropertyType $kind -Value $command -Force | Out-Null
                New-ItemProperty -Path $root -Name 'WinCommander__WC_Hidden' -PropertyType $kind -Value $command -Force | Out-Null
                New-ItemProperty -Path $root -Name 'WinCommander Free' -PropertyType String -Value '"%LOCALAPPDATA%\Foreign\wincommander-free.exe" --autostart' -Force | Out-Null
                $removed = Remove-OwnedRunValues @($root) $owned $profilePath
                if ($removed -ne 2) { throw "Profile-relative hidden $kind startup survived migration for $profilePath" }
                if ($null -ne (Get-OptionalRegistryValue $root 'WinCommander Pro__SystemCache')) { throw 'Owned current hidden Run entry survived' }
                if ($null -ne (Get-OptionalRegistryValue $root 'WinCommander__WC_Hidden')) { throw 'Owned legacy hidden Run entry survived' }
                if ($null -eq (Get-OptionalRegistryValue $root 'WinCommander Free')) { throw 'Foreign Run entry was removed' }
                if ((Remove-OwnedRunValues @($root) $owned $profilePath) -ne 0) { throw 'Repeated migration was not idempotent' }
            }
        }
    }
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
Write-Output 'PASS: profile startup cleanup'

# Exercise the complete migration against disposable profiles and real .lnk
# files. Only ProfileList and hive selection are substituted; payload cleanup,
# command ownership, shortcut retargeting and raw registry reads are real.
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('wc-startup-migration-' + [guid]::NewGuid().ToString('N'))
$fixtureRegistry = "HKCU:\Software\ServaLabs\WinCommander\InstallerTests\$([guid]::NewGuid().ToString('N'))"
$script:fixtureProfiles = @()
$script:fixtureHives = @{}
$savedProgramData = $env:ProgramData
New-Item -Path $fixtureRoot -ItemType Directory | Out-Null
New-Item -Path $fixtureRegistry -Force | Out-Null
try {
    $SharedExecutable = Join-Path $fixtureRoot 'Installed\wincommander-free.exe'
    New-Item -Path (Split-Path $SharedExecutable) -ItemType Directory | Out-Null
    Set-Content -LiteralPath $SharedExecutable -Value 'fixture'
    $env:ProgramData = Join-Path $fixtureRoot 'ProgramData'
    $Uninstall = $false
    $fixtureShell = New-Object -ComObject WScript.Shell
    foreach ($account in @('Admin', 'Standard')) {
        $profilePath = Join-Path $fixtureRoot $account
        $sid = "S-1-5-21-111-222-333-$(if ($account -eq 'Admin') { 1001 } else { 1002 })"
        $startupRoot = Join-Path $profilePath 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup'
        $desktop = Join-Path $profilePath 'Desktop'
        foreach ($layout in @('WinCommander', 'Programs\WinCommander')) {
            $legacyRoot = Join-Path $profilePath "AppData\Local\$layout"
            $linkName = if ($layout -like 'Programs*') { 'WinCommanderPrograms.lnk' } else { 'WinCommander.lnk' }
            foreach ($directory in @($legacyRoot, $startupRoot, $desktop)) { New-Item -Path $directory -ItemType Directory -Force | Out-Null }
            $legacyExecutable = Join-Path $legacyRoot 'wincommander-free.exe'
            Set-Content -LiteralPath $legacyExecutable -Value 'old executable'
            Set-Content -LiteralPath (Join-Path $legacyRoot 'uninstall.exe') -Value 'old uninstaller'
            foreach ($payloadDirectory in @('resources', 'scripts')) {
                New-Item -Path (Join-Path $legacyRoot $payloadDirectory) -ItemType Directory | Out-Null
                Set-Content -LiteralPath (Join-Path (Join-Path $legacyRoot $payloadDirectory) 'fixture.bin') -Value 'old payload'
            }
            Set-Content -LiteralPath (Join-Path $legacyRoot 'settings.dat') -Value 'user data'
            foreach ($linkPath in @((Join-Path $startupRoot $linkName), (Join-Path $desktop $linkName))) {
                $link = $fixtureShell.CreateShortcut($linkPath); $link.TargetPath = $legacyExecutable; $link.Save()
            }
        }
        $foreignLink = $fixtureShell.CreateShortcut((Join-Path $startupRoot 'Other.lnk'))
        $foreignLink.TargetPath = Join-Path $fixtureRoot 'Foreign\wincommander-free.exe'; $foreignLink.Save()
        $hive = Join-Path $fixtureRegistry $account
        $script:fixtureHives[$sid] = $hive
        foreach ($suffix in @('Software\Microsoft\Windows\CurrentVersion\Run', 'Software\Microsoft\Windows\CurrentVersion\RunOnce', 'Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run', 'Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce')) {
            $runKey = Join-Path $hive $suffix
            New-Item -Path $runKey -Force | Out-Null
            $runLayout = if ($suffix -like '*WOW6432Node*') { 'Programs\WinCommander' } else { 'WinCommander' }
            New-ItemProperty -Path $runKey -Name 'WinCommander Pro' -PropertyType ExpandString -Value ('"%LOCALAPPDATA%\' + $runLayout + '\wincommander-free.exe" --autostart') | Out-Null
            New-ItemProperty -Path $runKey -Name 'WinCommander Free' -PropertyType String -Value '"C:\Other\other.exe" --autostart' | Out-Null
        }
        $script:fixtureProfiles += [pscustomobject]@{ PSChildName = $sid; ProfileImagePath = $profilePath }
    }
    function Get-ItemProperty {
        param($Path, $ErrorAction)
        if ($Path -ne 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\*') { throw "Unexpected profile lookup: $Path" }
        return $script:fixtureProfiles
    }
    function Invoke-ProfileHive($Profile, [scriptblock]$Action) {
        & $Action $script:fixtureHives[$Profile.Sid]
        return $true
    }
    foreach ($attempt in 1..2) {
        foreach ($statement in $ast.EndBlock.Statements) {
            if ($statement -is [Management.Automation.Language.FunctionDefinitionAst] -and $statement.Name -eq 'Invoke-ProfileHive') { continue }
            . ([scriptblock]::Create($statement.Extent.Text)) | Out-Null
        }
        if ($summary.failures -or $summary.registryHivesDeferred) { throw 'Fixture upgrade did not complete profile migration' }
    }
    foreach ($entry in $script:fixtureProfiles) {
        $profilePath = $entry.ProfileImagePath
        $startupRoot = Join-Path $profilePath 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup'
        foreach ($layout in @('WinCommander', 'Programs\WinCommander')) {
            $legacyRoot = Join-Path $profilePath "AppData\Local\$layout"
            $linkName = if ($layout -like 'Programs*') { 'WinCommanderPrograms.lnk' } else { 'WinCommander.lnk' }
            if (Test-Path (Join-Path $legacyRoot 'wincommander-free.exe')) { throw 'Old profile executable survived' }
            foreach ($payload in @('uninstall.exe', 'resources', 'scripts')) {
                if (Test-Path (Join-Path $legacyRoot $payload)) { throw "Old profile payload survived: $layout $payload" }
            }
            if ((Get-Content (Join-Path $legacyRoot 'settings.dat')) -ne 'user data') { throw 'User settings changed' }
            if (Test-Path (Join-Path $startupRoot $linkName)) { throw 'Old startup shortcut survived' }
            if (-not (Test-Path (Join-Path $startupRoot 'Other.lnk'))) { throw 'Foreign shortcut removed' }
            $desktopLink = $fixtureShell.CreateShortcut((Join-Path (Join-Path $profilePath 'Desktop') $linkName))
            if ($desktopLink.TargetPath -ne $SharedExecutable) { throw 'Manual shortcut retained the old executable' }
        }
        foreach ($key in Get-ChildItem -LiteralPath $script:fixtureHives[$entry.PSChildName] -Recurse | Where-Object { $_.PSChildName -in @('Run', 'RunOnce') }) {
            if ($null -ne (Get-OptionalRegistryValue $key.PSPath 'WinCommander Pro')) { throw 'Profile Pro-labelled Run route survived complete migration' }
            if ($null -eq (Get-OptionalRegistryValue $key.PSPath 'WinCommander Free')) { throw 'Foreign profile Run route removed' }
        }
    }
} finally {
    $env:ProgramData = $savedProgramData
    Remove-Item Function:\Get-ItemProperty -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $fixtureRegistry -Recurse -Force
    # fixtureRoot is a newly created absolute child of the OS temporary folder.
    $resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
    if (-not $resolvedFixture.StartsWith(([IO.Path]::GetFullPath([IO.Path]::GetTempPath())), [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture cleanup escaped temporary directory' }
    Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
}
Write-Output 'PASS: complete upgrade replaces admin and standard profile routes and preserves data'

& {
    $config = Read-ScriptAst '../src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'
    foreach ($statement in $config.EndBlock.Statements) {
        if ($statement -is [Management.Automation.Language.FunctionDefinitionAst] -or $statement -is [Management.Automation.Language.AssignmentStatementAst]) {
            . ([scriptblock]::Create($statement.Extent.Text))
        }
    }
    $targetPath = 'C:\Program Files\WinCommander\wincommander-free.exe'
    $script:fixturePreference = $null
    function Get-ItemProperty {
        param($Path, $ErrorAction)
        return @([pscustomobject]@{ ProfileImagePath = 'C:\Users\FixtureAdmin' }, [pscustomobject]@{ ProfileImagePath = 'C:\Users\FixtureStandard' })
    }
    function Get-AutostartPreferenceValue { return $script:fixturePreference }
    function Set-AutostartPreference($Enabled) { $script:fixturePreference = [int]$Enabled }
    function Get-ScheduledTask {
        param($TaskPath, $TaskName, $ErrorAction)
        if ($TaskName -eq 'WinCommander Autostart') { return $script:fixtureDisabledTask }
    }
    $paths = @(Get-OwnedExecutablePaths)
    foreach ($account in @('FixtureAdmin', 'FixtureStandard')) {
        foreach ($layout in @('WinCommander', 'Programs\WinCommander')) {
                $legacy = "C:\Users\$account\AppData\Local\$layout\wincommander-free.exe"
                $script:fixturePreference = $null
                $script:fixtureDisabledTask = [pscustomobject]@{ State = 'Disabled'; Actions = @([pscustomobject]@{ Execute = $legacy; Arguments = '--autostart' }) }
                if ($paths -notcontains $legacy) { throw "Task ownership missed legacy executable: $legacy" }
                if (Get-AutostartEnabled $true $paths) { throw "Upgrade reset explicit OFF for $legacy" }
                if ($script:fixturePreference -ne 0) { throw 'Upgrade did not persist legacy OFF' }
            }
    }
    $script:fixturePreference = $null
    $script:fixtureDisabledTask.Actions[0].Execute = 'C:\Users\FixtureStandard\AppData\Local\Programs\Foreign\wincommander-free.exe'
    if (-not (Get-AutostartEnabled $true $paths)) { throw 'Foreign task disabled WinCommander startup' }

    $ownedBareTask = [pscustomobject]@{ Actions = @([pscustomobject]@{ Execute = $targetPath; Arguments = '' }) }
    if (-not (Test-TaskActionOwnership $ownedBareTask '--autostart' $paths)) { throw 'Known owned no-argument autostart task was not recognized for normalization' }
    $foreignBareTask = [pscustomobject]@{ Actions = @([pscustomobject]@{ Execute = 'C:\Foreign\wincommander-free.exe'; Arguments = '' }) }
    if (Test-TaskActionOwnership $foreignBareTask '--autostart' $paths) { throw 'Foreign no-argument task was treated as owned' }
}
Write-Output 'PASS: both legacy install layouts preserve disabled tasks for every profile'

# The secondary task rename pass must never restore an obsolete launcher after
# the authoritative startup reconciliation removed it or respected OFF.
$maintenance = Read-ScriptAst '../src-tauri/commander-free/nsis/migrate-system-maintenance-tasks.ps1'
$script:moved = @()
function Ensure-TaskFolder { }
function Move-OwnedTask($OldName, $NewName) { $script:moved += $OldName; return 'fixture' }
function Get-ScheduledTask { param($TaskPath, $ErrorAction) return @() }
$body = $maintenance.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.TryStatementAst] }
& ([scriptblock]::Create($body.Extent.Text)) | Out-Null
if ($script:moved -contains 'SL-AS' -or $script:moved -contains 'SL-EL') { throw 'Secondary migration can recreate an obsolete automatic or manual startup task' }
if ($script:moved -notcontains 'WinCommander Session Guard') { throw 'Unrelated maintenance migration stopped working' }
Write-Output 'PASS: maintenance migration cannot resurrect startup'
