$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/tweaks/ui.ps1'
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$null, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Get-ShellVisibilitySpec','Test-ShellVisibilityValue','Get-ShellVisibilityState','Get-ShellVisibilityStatus','Set-ShellVisibility')) {
    $node = $ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }.GetNewClosure(), $true)
    . ([scriptblock]::Create($node.Extent.Text))
}
$script:startKey = 'SOFTWARE\Policies\Microsoft\Windows\Explorer'
$script:settingsKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer'
function Reset-Fixture { $script:values = @{}; $script:admin = $true; $script:eligible = $true; $script:failWrite = $false; $script:failFinalize = $false; $script:ignoreRemove = $false }
function Test-IsAdmin { $script:admin }
function Assert-IsAdmin { if (-not $script:admin) { throw 'Administrator required.' } }
function Get-ShellVisibilityPlatform { $script:eligible }
function Invoke-WithShellVisibilityMutex { param([scriptblock]$Action) & $Action }
function Get-ShellVisibilityValue {
    param([string]$Hive,[string]$Key,[string]$Name)
    $value = $script:values["$Hive|$Key|$Name"]
    if ($null -eq $value) { return @{ exists = $false; kind = $null; value = $null } }
    return @{ exists = $true; kind = $value.kind; value = $value.value }
}
function Set-ShellVisibilityValue {
    param([string]$Key,[string]$Name,$Value,[string]$Kind)
    if ($script:failWrite -and $Name -in @('HideRecommendedSection','SettingsPageVisibility')) { throw 'Mock policy write failure.' }
    if ($script:failFinalize -and $Name -eq 'Phase' -and $Value -eq 2) { throw 'Mock finalization failure.' }
    $script:values["LocalMachine|$Key|$Name"] = @{ value = $Value; kind = $Kind }
}
function Remove-ShellVisibilityValue {
    param([string]$Key,[string]$Name)
    if (-not $script:ignoreRemove) { $script:values.Remove("LocalMachine|$Key|$Name") }
}
function Expect-Failure([scriptblock]$Action) { $failed = $false; try { & $Action | Out-Null } catch { $failed = $true }; if (-not $failed) { throw 'Expected failure.' } }

foreach ($item in @('StartRecommended','SettingsHome')) {
    Reset-Fixture
    $result = Set-ShellVisibility $item hide
    if (-not $result.verified -or -not $result.item.owned -or $result.item.conflict) { throw "Hide failed: $item" }
    $result = Set-ShellVisibility $item restore
    if (-not $result.verified -or $result.item.owned -or $result.item.conflict) { throw "Restore failed: $item" }
}
Reset-Fixture
$script:values["LocalMachine|$script:settingsKey|SettingsPageVisibility"] = @{ kind='String'; value='hide:privacy;bluetooth;' }
$result = Set-ShellVisibility SettingsHome hide
if ($script:values["LocalMachine|$script:settingsKey|SettingsPageVisibility"].value -cne 'hide:privacy;bluetooth;home') { throw 'Existing hidden pages were not retained.' }
Set-ShellVisibility SettingsHome restore | Out-Null
if ($script:values["LocalMachine|$script:settingsKey|SettingsPageVisibility"].value -cne 'hide:privacy;bluetooth;') { throw 'Previous policy was not restored exactly.' }
foreach ($existing in @('showonly:privacy', 'hide:privacy;home', 'unexpected')) {
    Reset-Fixture
    $script:values["LocalMachine|$script:settingsKey|SettingsPageVisibility"] = @{ kind='String'; value=$existing }
    Expect-Failure { Set-ShellVisibility SettingsHome hide }
    if ($script:values["LocalMachine|$script:settingsKey|SettingsPageVisibility"].value -cne $existing) { throw 'An external policy was overwritten.' }
}
Reset-Fixture
$script:values["LocalMachine|$script:startKey|HideRecommendedSection"] = @{ kind='DWord'; value=0 }
Expect-Failure { Set-ShellVisibility StartRecommended hide }
Reset-Fixture
$script:values["CurrentUser|$script:startKey|HideRecommendedSection"] = @{ kind='DWord'; value=0 }
Expect-Failure { Set-ShellVisibility StartRecommended hide }
Reset-Fixture
$script:eligible = $false
Expect-Failure { Set-ShellVisibility StartRecommended hide }
Reset-Fixture
$script:admin = $false
Expect-Failure { Set-ShellVisibility StartRecommended hide }
Reset-Fixture
$script:failWrite = $true
Expect-Failure { Set-ShellVisibility StartRecommended hide }
if (-not (Get-ShellVisibilityState StartRecommended $true).pending) { throw 'Interrupted write was not identified.' }
$script:failWrite = $false
if (-not (Set-ShellVisibility StartRecommended hide).verified) { throw 'Retry failed.' }
Reset-Fixture
$script:failFinalize = $true
Expect-Failure { Set-ShellVisibility StartRecommended hide }
if (-not (Get-ShellVisibilityState StartRecommended $true).conflict) { throw 'Uncertain ownership was not reported.' }
Expect-Failure { Set-ShellVisibility StartRecommended restore }
Reset-Fixture
Set-ShellVisibility StartRecommended hide | Out-Null
$snapshot = 'SOFTWARE\WinCommander\ShellVisibility\StartRecommended'
$script:values["LocalMachine|$snapshot|AppliedValue"].value = '0'
if (-not (Get-ShellVisibilityState StartRecommended $true).conflict) { throw 'Invalid backup was not reported.' }
Expect-Failure { Set-ShellVisibility StartRecommended restore }
Reset-Fixture
$snapshot = 'SOFTWARE\WinCommander\ShellVisibility\SettingsHome'
$script:values["LocalMachine|$snapshot|BeforeKind"] = @{kind='String';value='Absent'}
if (-not (Get-ShellVisibilityState SettingsHome $true).conflict) { throw 'Incomplete backup was not reported.' }
Expect-Failure { Set-ShellVisibility SettingsHome hide }
Reset-Fixture
Set-ShellVisibility SettingsHome hide | Out-Null
$script:values["LocalMachine|$script:settingsKey|SettingsPageVisibility"].value = 'hide:home;apps'
Expect-Failure { Set-ShellVisibility SettingsHome restore }
if ($script:values["LocalMachine|$script:settingsKey|SettingsPageVisibility"].value -cne 'hide:home;apps') { throw 'A later external change was overwritten.' }
Reset-Fixture
Set-ShellVisibility StartRecommended hide | Out-Null
$script:ignoreRemove = $true
Expect-Failure { Set-ShellVisibility StartRecommended restore }
if (-not (Get-ShellVisibilityState StartRecommended $true).owned) { throw 'Failed restoration lost its backup.' }
Expect-Failure { Set-ShellVisibility '../other' hide }
Write-Output 'Shell visibility fixture tests passed.'
