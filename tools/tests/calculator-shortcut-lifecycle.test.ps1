$ErrorActionPreference = 'Stop'
$fixture = Join-Path $env:TEMP ('wincommander-calculator-shortcuts-' + [guid]::NewGuid().ToString('N'))
$savedAppData = $env:APPDATA
$savedProgramData = $env:ProgramData
$env:APPDATA = Join-Path $fixture 'profile'
$env:ProgramData = Join-Path $fixture 'machine'
$script:programs = Join-Path $fixture 'Programs'
$script:desktop = Join-Path $fixture 'Desktop'
$script:quickLaunch = Join-Path $fixture 'Quick Launch'
$script:shared = Join-Path $fixture 'Shared\WinCommander.lnk'
$script:target = Join-Path $env:SystemRoot 'System32\notepad.exe'
$script:otherTarget = Join-Path $env:SystemRoot 'explorer.exe'

function Assert-True($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function New-TestLink([string]$Path, [string]$Target) {
    $parent = Split-Path $Path -Parent
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($Path)
    $link.TargetPath = $Target
    $link.Arguments = '--test-argument'
    $link.Description = 'Original user shortcut'
    $link.Hotkey = 'CTRL+ALT+F11'
    $link.WindowStyle = 3
    $link.Save()
}
function Read-Bytes([string]$Path) { [Convert]::ToBase64String([IO.File]::ReadAllBytes($Path)) }
function Get-TestReplacement([string]$Root) {
    Join-Path $Root 'calculator..lnk'
}

try {
    New-Item -ItemType Directory -Path $env:APPDATA,$env:ProgramData,$script:programs,$script:desktop,$script:quickLaunch -Force | Out-Null
    $module = Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1'
    Invoke-Expression ([IO.File]::ReadAllText((Resolve-Path $module), [Text.Encoding]::UTF8))
    $savedOneDrive = $env:OneDrive
    try {
        $env:OneDrive = Join-Path $fixture 'OneDrive'
        $discoveredRoots = @(Get-WinCommanderCalculatorUserRoots)
        Assert-True ($discoveredRoots -contains (Join-Path $env:OneDrive 'Desktop')) 'Redirected OneDrive Desktop was not discovered'
        Assert-True ($discoveredRoots -contains [Environment]::GetFolderPath('Desktop')) 'The current account Desktop was not discovered'
    } finally { $env:OneDrive = $savedOneDrive }
    function Get-WinCommanderCalculatorProgramsPath { $script:programs }
    function Get-WinCommanderCalculatorDesktopPath { $script:desktop }
    function Get-WinCommanderCalculatorUserRoots { @($script:programs,$script:desktop,$script:quickLaunch) }
    function Test-WinCommanderCalculatorSharedReplacement {
        param([string]$Path, [bool]$SharedDesktop, [bool]$SharedStart)
        return ($SharedDesktop -and $Path -ieq (Get-TestReplacement $script:desktop)) -or
            ($SharedStart -and $Path -ieq (Get-TestReplacement $script:programs))
    }
    function Get-WinCommanderExePath { $script:target }
    function Get-WinCommanderStartMenuTargets { @($script:shared) }
    function Invoke-ShellChangeNotify {}

    $originals = @((Join-Path $script:programs 'Vendor\Custom name.lnk'), (Join-Path $script:desktop 'WinCommander.lnk'), (Join-Path $script:quickLaunch 'My launcher.lnk'), (Join-Path $script:programs 'ServaLabs\WinCommander\Launcher.lnk'))
    $snapshots = @{}
    foreach ($path in $originals) { New-TestLink $path $script:target; $snapshots[$path] = Read-Bytes $path }
    New-TestLink $script:shared $script:target
    $sharedBytes = Read-Bytes $script:shared
    $pinned = Join-Path $script:quickLaunch 'User Pinned\TaskBar\WinCommander.lnk'
    New-TestLink $pinned $script:target
    $pinnedBytes = Read-Bytes $pinned
    $unrelated = Join-Path $script:desktop 'Other.lnk'
    New-TestLink $unrelated $script:otherTarget
    $unrelatedBytes = Read-Bytes $unrelated
    $hidden = Set-WinCommanderCalculatorShortcuts $true
    Assert-True ($hidden.status -eq 'hidden') ("Enable failed: " + ($hidden | ConvertTo-Json -Depth 5 -Compress))
    foreach ($path in $originals) { Assert-True (-not (Test-Path -LiteralPath $path)) 'Original user shortcut was not replaced' }
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:programs 'ServaLabs'))) 'Empty owned product folders remained visible'
    Assert-True (Test-Path -LiteralPath (Join-Path $script:programs 'Vendor')) 'An unrelated shortcut folder was removed'
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) { Assert-True (Test-Path -LiteralPath (Get-TestReplacement $root)) 'Calculator shortcut is missing' }
    Assert-True ((Read-Bytes $script:shared) -ceq $sharedBytes) 'Shared shortcut changed'
    Assert-True ((Read-Bytes $pinned) -ceq $pinnedBytes) 'Pinned shortcut changed'
    Assert-True ((Read-Bytes $unrelated) -ceq $unrelatedBytes) 'Unrelated shortcut changed'
    $again = Set-WinCommanderCalculatorShortcuts $true
    Assert-True ($again.status -eq 'hidden') 'Repeated enable failed'
    $restored = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ($restored.status -eq 'restored') 'Disable failed'
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Original shortcut bytes were not restored exactly' }
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) { Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $root))) 'Owned Calculator shortcut was not removed' }

    # Migrate only the previous release's exact generated links, preserving
    # original snapshots and unrelated Windows Calculator entries.
    $null = Set-WinCommanderCalculatorShortcuts $true
    $migrationStatePath = Join-Path $env:APPDATA 'WinCommander\calc_shortcut_state.json'
    $migration = Get-Content -LiteralPath $migrationStatePath -Raw | ConvertFrom-Json
    foreach ($entry in $migration.generatedCalculatorLinks) {
        $old = Join-Path (Split-Path $entry.path -Parent) $(if ((Split-Path $entry.path -Parent) -ieq $script:desktop) { 'Calculator+.lnk' } else { 'Calculator.lnk' })
        Move-Item -LiteralPath $entry.path -Destination $old
        $entry.path = $old
    }
    $migration.createdCalculatorLinks = @($migration.generatedCalculatorLinks | ForEach-Object { $_.path })
    $migration | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $migrationStatePath -Encoding UTF8
    $null = Set-WinCommanderCalculatorShortcuts $true
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) {
        $oldName = if ($root -ieq $script:desktop) { 'Calculator+.lnk' } else { 'Calculator.lnk' }
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $root $oldName))) 'Old generated shortcut was not migrated'
        Assert-True (Test-Path -LiteralPath (Get-TestReplacement $root)) 'Replacement shortcut was not created'
    }
    $null = Set-WinCommanderCalculatorShortcuts $false
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Migration lost an original shortcut' }
    $null = Set-WinCommanderCalculatorShortcuts -Hidden $true -SharedDesktop $true -SharedStart $false
    Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $script:desktop))) 'Verified shared Desktop created a duplicate account alias'
    Assert-True (Test-Path -LiteralPath (Get-TestReplacement $script:programs)) 'Unmanaged Start surface did not get an account fallback'
    $null = Set-WinCommanderCalculatorShortcuts $false
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'One-surface shared replacement lost an original shortcut' }
    $null = Set-WinCommanderCalculatorShortcuts -Hidden $true -SharedDesktop $false -SharedStart $true
    Assert-True (Test-Path -LiteralPath (Get-TestReplacement $script:desktop)) 'Unmanaged Desktop did not get an account fallback'
    Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $script:programs))) 'Verified shared Start created a duplicate account alias'
    $null = Set-WinCommanderCalculatorShortcuts $false
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Other-surface shared replacement lost an original shortcut' }
    function Test-WinCommanderCalculatorSharedReplacement { param([string]$Path, [bool]$SharedDesktop, [bool]$SharedStart) return $false }
    $unverified = Set-WinCommanderCalculatorShortcuts -Hidden $true -SharedDesktop $true -SharedStart $true
    Assert-True (Test-Path -LiteralPath (Get-TestReplacement $script:desktop)) 'Unverified shared Desktop suppressed the account fallback'
    Assert-True (Test-Path -LiteralPath (Get-TestReplacement $script:programs)) 'Unverified shared Start suppressed the account fallback'
    Assert-True (@($unverified.warnings).Count -ge 2) 'Unverified shared replacements did not report warnings'
    $null = Set-WinCommanderCalculatorShortcuts $false
    function Test-WinCommanderCalculatorSharedReplacement {
        param([string]$Path, [bool]$SharedDesktop, [bool]$SharedStart)
        return ($SharedDesktop -and $Path -ieq (Get-TestReplacement $script:desktop)) -or
            ($SharedStart -and $Path -ieq (Get-TestReplacement $script:programs))
    }
    $null = Set-WinCommanderCalculatorShortcuts $true
    $migration = Get-Content -LiteralPath $migrationStatePath -Raw | ConvertFrom-Json
    foreach ($entry in $migration.generatedCalculatorLinks) {
        $old = Join-Path (Split-Path $entry.path -Parent) 'Calculator+.lnk'
        Move-Item -LiteralPath $entry.path -Destination $old
        $entry.path = $old
    }
    $migration.createdCalculatorLinks = @($migration.generatedCalculatorLinks | ForEach-Object { $_.path })
    $migration | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $migrationStatePath -Encoding UTF8
    $null = Set-WinCommanderCalculatorShortcuts -Hidden $true -SharedDesktop $true -SharedStart $true
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) {
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $root 'Calculator+.lnk'))) 'Legacy plus shortcut remained visible'
    }
    Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $script:desktop))) 'Managed Desktop duplicated the shared replacement'
    Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $script:programs))) 'Managed Start duplicated the shared replacement'
    Assert-True (Test-Path -LiteralPath (Get-TestReplacement $script:quickLaunch)) 'Unmanaged Quick Launch fallback is missing'
    $null = Set-WinCommanderCalculatorShortcuts $false
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Managed migration lost an original shortcut' }
    $null = Set-WinCommanderCalculatorShortcuts $true
    $bothFlag = Get-WinCommanderHideFlagPath
    New-Item -ItemType Directory -Path (Split-Path $bothFlag -Parent) -Force | Out-Null
    [IO.File]::WriteAllText($bothFlag, '')
    $null = Set-WinCommanderCalculatorShortcuts $true
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) { Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $root))) 'Both modes left a calculator alias visible' }
    Remove-Item -LiteralPath $bothFlag
    $null = Set-WinCommanderCalculatorShortcuts $true
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) { Assert-True (Test-Path -LiteralPath (Get-TestReplacement $root)) 'Turning Hide off did not restore the calculator alias' }
    $null = Set-WinCommanderCalculatorShortcuts $false
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Both-mode transition lost an original shortcut' }
    $windowsCalculatorLink = Join-Path $script:programs 'Calculator.lnk'
    New-TestLink $windowsCalculatorLink $script:otherTarget
    $windowsCalculatorBytes = Read-Bytes $windowsCalculatorLink
    $null = Set-WinCommanderCalculatorShortcuts $true
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $windowsCalculatorLink) -ceq $windowsCalculatorBytes) 'An unrelated Calculator entry was changed'
    Remove-Item -LiteralPath $windowsCalculatorLink

    # Disabling the lock while Hide WinCommander remains active must remove
    # Calculator aliases without exposing the saved WinCommander shortcuts.
    $null = Set-WinCommanderCalculatorShortcuts $true
    $hideFlag = Get-WinCommanderHideFlagPath
    New-Item -ItemType Directory -Path (Split-Path $hideFlag -Parent) -Force | Out-Null
    [IO.File]::WriteAllText($hideFlag, '')
    $deferred = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ($deferred.status -eq 'deferred') 'Hide mode did not report deferred original restoration'
    Assert-True (-not $deferred.warnings) 'Deferred restoration reported a false shortcut failure'
    foreach ($path in $originals) { Assert-True (-not (Test-Path -LiteralPath $path)) 'Lock off exposed a shortcut while Hide mode remained active' }
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) { Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $root))) 'Calculator shortcut remained after lock off' }
    $deferredState = Join-Path $env:APPDATA 'WinCommander\calc_shortcut_state.json'
    Assert-True (Test-Path -LiteralPath $deferredState) 'Deferred original recovery state was discarded'
    Remove-Item -LiteralPath $hideFlag
    $finishedRestore = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ($finishedRestore.status -eq 'restored') 'Original shortcuts did not report restored after Hide mode ended'
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Original shortcut was not restored after Hide mode ended' }
    Assert-True (-not (Test-Path -LiteralPath $deferredState)) 'Deferred recovery state remained after successful restore'

    # Hide WinCommander can remove every original shortcut first. Calculator
    # Lock still needs account Start and Desktop entries without restoring them.
    foreach ($path in $originals) { Remove-Item -LiteralPath $path }
    $null = Set-WinCommanderCalculatorShortcuts $true
    Assert-True (Test-Path -LiteralPath (Join-Path $script:programs 'calculator..lnk')) 'Calculator Start shortcut was not created without originals'
    Assert-True (Test-Path -LiteralPath (Join-Path $script:desktop 'calculator..lnk')) 'Calculator Desktop shortcut was not created without an original'
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:programs 'calculator..lnk'))) 'Calculator Start shortcut remained after disabling'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:desktop 'calculator..lnk'))) 'Calculator Desktop shortcut remained after disabling'
    foreach ($path in $originals) {
        Assert-True (-not (Test-Path -LiteralPath $path)) 'Hide mode original returned when Calculator Lock was disabled'
        New-Item -ItemType Directory -Path (Split-Path $path -Parent) -Force | Out-Null
        [IO.File]::WriteAllBytes($path, [Convert]::FromBase64String($snapshots[$path]))
    }

    # Historical installer and Pro links can point to other executable names
    # inside the same installation folder.
    $script:fixtureExe = Join-Path $fixture 'Install\wincommander-free.exe'
    $legacyTarget = Join-Path (Split-Path $script:fixtureExe -Parent) 'WinCommander.exe'
    New-Item -ItemType Directory -Path (Split-Path $script:fixtureExe -Parent) -Force | Out-Null
    [IO.File]::WriteAllBytes($script:fixtureExe, [byte[]]@(1))
    [IO.File]::WriteAllBytes($legacyTarget, [byte[]]@(1))
    $legacyLink = Join-Path $script:programs 'Legacy\WinCommander alias.lnk'
    New-TestLink $legacyLink $legacyTarget
    $legacyBytes = Read-Bytes $legacyLink
    function Get-WinCommanderExePath { $script:fixtureExe }
    $hiddenAlias = Set-WinCommanderCalculatorShortcuts $true
    Assert-True (-not (Test-Path -LiteralPath $legacyLink)) 'A historical WinCommander alias stayed visible'
    $aliasState = Get-Content -LiteralPath (Join-Path $env:APPDATA 'WinCommander\calc_shortcut_state.json') -Raw | ConvertFrom-Json
    Assert-True (@($aliasState.hiddenShortcuts | Where-Object { $_.path -eq $legacyLink }).Count -eq 1) 'Historical alias was not recorded for restoration'
    $restoredAlias = Set-WinCommanderCalculatorShortcuts $false
    Assert-True (Test-Path -LiteralPath $legacyLink) ("Historical alias was not restored: " + ($hiddenAlias | ConvertTo-Json -Compress) + " / " + ($restoredAlias | ConvertTo-Json -Compress))
    Assert-True ((Read-Bytes $legacyLink) -ceq $legacyBytes) 'Historical alias bytes were not restored'
    function Get-WinCommanderExePath { $script:target }
    Remove-Item -LiteralPath $legacyLink

    $collision = Join-Path $script:desktop 'calculator..lnk'
    New-TestLink $collision $script:otherTarget
    $collisionBytes = Read-Bytes $collision
    $null = Set-WinCommanderCalculatorShortcuts $true
    Assert-True (Test-Path -LiteralPath $originals[1]) 'An original was hidden despite a replacement collision'
    Assert-True ((Read-Bytes $collision) -ceq $collisionBytes) 'Existing Calculator shortcut was overwritten'
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $collision) -ceq $collisionBytes) 'Existing Calculator shortcut was removed on disable'
    Remove-Item -LiteralPath $collision

    $null = Set-WinCommanderCalculatorShortcuts $true
    New-TestLink $collision $script:target
    $changed = (New-Object -ComObject WScript.Shell).CreateShortcut($collision)
    $changed.Arguments = '--external-arguments'
    $changed.IconLocation = "$env:SystemRoot\System32\shell32.dll,1"
    $changed.Save()
    $externalBytes = Read-Bytes $collision
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $collision) -ceq $externalBytes) 'Externally changed replacement was removed'
    Assert-True ((Read-Bytes $originals[1]) -ceq $snapshots[$originals[1]]) 'Original shortcut was not restored after an external edit'
    $statePath = Join-Path $env:APPDATA 'WinCommander\calc_shortcut_state.json'
    Assert-True (Test-Path -LiteralPath $statePath) 'External replacement recovery state was lost'
    Remove-Item -LiteralPath $collision
    $null = Set-WinCommanderCalculatorShortcuts $false

    $null = Set-WinCommanderCalculatorShortcuts $true
    New-TestLink $originals[1] $script:otherTarget
    $occupiedBytes = Read-Bytes $originals[1]
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $originals[1]) -ceq $occupiedBytes) 'An externally occupied original path was overwritten'
    $retained = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    $savedOriginal = @($retained.hiddenShortcuts | Where-Object { $_.path -eq $originals[1] })[0]
    Assert-True ($savedOriginal.originalBytes -ceq $snapshots[$originals[1]]) 'Original backup was discarded after a restore collision'
    Remove-Item -LiteralPath $originals[1]
    $null = Set-WinCommanderCalculatorShortcuts $false
    Assert-True ((Read-Bytes $originals[1]) -ceq $snapshots[$originals[1]]) 'Retry did not restore the original after removing the collision'

    $canonical = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'WinCommander\wincommander-free.exe'
    Assert-True ((Get-WinCommanderCalculatorLauncherPath $canonical) -ieq (Join-Path (Split-Path $canonical -Parent) 'Calculator.exe')) 'Canonical installation did not select its protected launcher'
    Assert-True (-not (Get-WinCommanderCalculatorLauncherPath $script:target)) 'A custom executable was allowed to select a privileged launcher'
    $null = Set-WinCommanderCalculatorShortcuts $true
    function Get-WinCommanderCalculatorLauncherPath { param([string]$Executable) if ($Executable -ieq $script:target) { $script:otherTarget } }
    $null = Set-WinCommanderCalculatorShortcuts $true
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) {
        $replacement = (New-Object -ComObject WScript.Shell).CreateShortcut((Get-TestReplacement $root))
        Assert-True ($replacement.TargetPath -ieq $script:otherTarget) 'A previously generated shortcut was not migrated to the real launcher'
        Assert-True ($replacement.IconLocation -ieq "$script:otherTarget,0") 'The launcher icon was not selected'
    }
    $null = Set-WinCommanderCalculatorShortcuts $false
    foreach ($path in $originals) { Assert-True ((Read-Bytes $path) -ceq $snapshots[$path]) 'Launcher migration lost an original shortcut snapshot' }
    foreach ($root in @($script:programs,$script:desktop,$script:quickLaunch)) {
        Assert-True (-not (Test-Path -LiteralPath (Get-TestReplacement $root))) 'Migrated launcher shortcut was not removed on disable'
    }
    function Get-WinCommanderCalculatorLauncherPath { param([string]$Executable) return $null }

    New-TestLink $originals[1] $script:otherTarget
    $legacyExternalBytes = Read-Bytes $originals[1]
    @{ hiddenShortcuts = @(@{ path = $originals[1]; targetPath = $script:target }); calculatorShortcutCreated = $false } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $statePath
    $null = Set-WinCommanderCalculatorShortcuts $true
    Assert-True ((Read-Bytes $originals[1]) -ceq $legacyExternalBytes) 'Legacy recovery entry deleted an externally retargeted shortcut'

    $boundedRoot = Join-Path $fixture 'bounded-scan'
    foreach ($index in 1..257) { [void][IO.Directory]::CreateDirectory((Join-Path $boundedRoot "directory-$index")) }
    $capped = $false
    $null = Get-WinCommanderCalculatorShortcutFiles @($boundedRoot) (New-Object -ComObject WScript.Shell) $script:target ([ref]$capped)
    Assert-True $capped 'Shortcut traversal did not enforce its directory bound'
    Write-Output 'PASS: account scope, exact restoration, launcher migration, empty owned folders, collisions, external edits, legacy recovery, and bounded scan'
} finally {
    $env:APPDATA = $savedAppData
    $env:ProgramData = $savedProgramData
    $resolved = [IO.Path]::GetFullPath($fixture)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf) -like 'wincommander-calculator-shortcuts-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}
