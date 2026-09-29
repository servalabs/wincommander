[CmdletBinding()]
param([switch]$ReadOnlyMachineCheck)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/migrate-legacy-user-launches.ps1'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Migration PowerShell syntax is invalid.' }
foreach ($name in @('Test-ProfileHiveUnavailable', 'Invoke-ProfileHive')) {
    $definition = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
    }.GetNewClosure(), $true)
    if ($null -eq $definition) { throw "Missing migration function: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}

if ($ReadOnlyMachineCheck) {
    # Only the hive-selection function runs; the cleanup action is read-only.
    $mountsBefore = @(Get-ChildItem Registry::HKEY_USERS | ForEach-Object PSChildName | Sort-Object)
    $profiles = @(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\*' |
        Where-Object { $_.PSChildName -match '^S-1-5-21-' } |
        ForEach-Object { [pscustomobject]@{ Sid = $_.PSChildName; Path = $_.ProfileImagePath } })
    $loaded = 0
    $deferred = 0
    # A regression must never mount/unmount a real profile during this check.
    function reg.exe { throw 'Read-only check forbids registry load/unload.' }
    foreach ($profile in $profiles) {
        $result = Invoke-ProfileHive $profile {
            param($root)
            if (-not (Test-Path -LiteralPath $root)) { throw 'Selected hive disappeared.' }
        }
        if ($result) { $loaded++ } else { $deferred++ }
    }
    $mountsAfter = @(Get-ChildItem Registry::HKEY_USERS | ForEach-Object PSChildName | Sort-Object)
    if (@(Compare-Object $mountsBefore $mountsAfter).Count) { throw 'Loaded hive inventory changed.' }
    $stale = @($mountsBefore | Where-Object { $_ -like 'WinCommanderInstallerCleanup_*' }).Count
    Write-Output "PASS: machine read-only selection loaded=$loaded deferred=$deferred existingTemporaryHives=$stale; registry unchanged"
    exit 0
}

$profile = [pscustomobject]@{ Sid = 'S-1-5-21-111-222-333-1001'; Path = 'C:\Users\Fixture' }
$loadedRoot = "Registry::HKEY_USERS\$($profile.Sid)"
$staleRoot = 'Registry::HKEY_USERS\WinCommanderInstallerCleanup_S_1_5_21_111_222_333_1001'
$script:exists = @{}
$script:visited = [Collections.Generic.List[string]]::new()
$script:probeFailure = $null
function Test-Path {
    [CmdletBinding()]
    param([string]$LiteralPath, [string]$PathType)
    if ($null -ne $script:probeFailure) { throw $script:probeFailure }
    return $script:exists.ContainsKey($LiteralPath)
}
function reg.exe { throw 'Migration must not load or unload offline registry hives.' }
$action = { param($root) $script:visited.Add($root) }

# Reproduces the persisted state left behind by interrupted/failed setup.
$script:exists[$staleRoot] = $true
$script:exists[(Join-Path $profile.Path 'NTUSER.DAT')] = $true
foreach ($attempt in 1..2) {
    if (Invoke-ProfileHive $profile $action) { throw 'Offline profile was reported as cleaned.' }
    if ($script:visited.Count) { throw 'Offline or stale hive was edited.' }
}
Write-Output 'PASS: repeated setup defers offline profiles with leftover temporary hives'

$script:exists[$loadedRoot] = $true
foreach ($attempt in 1..2) {
    if (-not (Invoke-ProfileHive $profile $action)) { throw 'Loaded profile was not cleaned.' }
}
if ($script:visited.Count -ne 2 -or $script:visited[0] -ne $loadedRoot -or $script:visited[1] -ne $loadedRoot) {
    throw 'Cleanup did not use exactly the canonical loaded user hive.'
}
Write-Output 'PASS: loaded profile takes precedence over leftover temporary hive'

$script:probeFailure = [UnauthorizedAccessException]::new('fixture access denied')
if (Invoke-ProfileHive $profile $action) { throw 'Inaccessible profile was reported as cleaned.' }
$script:probeFailure = $null
if (Invoke-ProfileHive $profile { throw [UnauthorizedAccessException]::new('fixture denied') }) {
    throw 'Denied cleanup was reported as complete.'
}
$failed = $false
try { $null = Invoke-ProfileHive $profile { throw 'fixture genuine cleanup failure' } }
catch {
    if ($_.Exception.Message -ne 'fixture genuine cleanup failure') { throw }
    $failed = $true
}
if (-not $failed) { throw 'Genuine cleanup failure was hidden.' }
Write-Output 'PASS: denied access is deferred; genuine cleanup failures remain errors'

$runtime = Get-Content (Join-Path $PSScriptRoot '../src-tauri/commander-free/src/autostart.rs') -Raw
$userRunFunction = [regex]::Match($runtime, '(?ms)^function Get-UserRunPaths \{.*?^\}')
if (-not $userRunFunction.Success) { throw 'Missing per-user startup cleanup path selector.' }
. ([scriptblock]::Create($userRunFunction.Value))
$expected = @(
    'Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run',
    'Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\RunOnce',
    'Registry::HKEY_CURRENT_USER\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run',
    'Registry::HKEY_CURRENT_USER\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce'
)
$actual = @(Get-UserRunPaths -RegistryRoot 'Registry::HKEY_CURRENT_USER' | ForEach-Object { $_ -replace '\\\\', '\' })
if (@(Compare-Object $expected $actual).Count) { throw 'Runtime deferred cleanup does not cover every migrated Run location.' }
Write-Output 'PASS: runtime deferred cleanup covers native and 32-bit Run/RunOnce paths'
