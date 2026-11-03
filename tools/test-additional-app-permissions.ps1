$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1'
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$null, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Get-AdditionalPermissionState', 'Get-AdditionalAppPermissions', 'Set-AdditionalAppPermission')) {
    $node = $ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }.GetNewClosure(), $true)
    . ([scriptblock]::Create($node.Extent.Text))
}
$assignment = $ast.Find({ param($n) $n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$Script:AdditionalAppPermissions' }, $true)
. ([scriptblock]::Create($assignment.Extent.Text))
$script:root = 'SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore'
function Reset-Fixture {
    $script:values = @{}; $script:keys = @{}; $script:admin = $true
    $script:failDeny = $false; $script:failFinalize = $false; $script:ignoreRemove = $false
    foreach ($capability in $Script:AdditionalAppPermissions) {
        $key = "$script:root\$capability"
        $script:keys["Machine|$key"] = $true
        $script:values["Machine|$key|Value"] = @{ value = 'Allow'; kind = 'String' }
    }
}
function Test-IsAdmin { $script:admin }
function Assert-IsAdmin { if (-not $script:admin) { throw 'Administrator required.' } }
function Invoke-WithAdditionalPermissionMutex { param([scriptblock]$Action) & $Action }
function Get-AdditionalPermissionRegistryValue {
    param([string]$Scope,[string]$Key,[string]$Name)
    $id = "$Scope|$Key|$Name"; $found = $script:values[$id]
    if ($null -eq $found) { return @{ keyExists = [bool]$script:keys["$Scope|$Key"]; exists = $false; value = $null; kind = $null } }
    return @{ keyExists = $true; exists = $true; value = $found.value; kind = $found.kind }
}
function Set-AdditionalPermissionRegistryValue {
    param([string]$Scope,[string]$Key,[string]$Name,$Value,[string]$Kind)
    if ($script:failDeny -and $Name -eq 'Value' -and $Value -eq 'Deny') { throw 'Mock deny failure.' }
    if ($script:failFinalize -and $Name -eq 'Phase' -and $Value -eq 2) { throw 'Mock finalization failure.' }
    $script:keys["$Scope|$Key"] = $true
    $script:values["$Scope|$Key|$Name"] = @{ value = $Value; kind = $Kind }
}
function Remove-AdditionalPermissionRegistryValue {
    param([string]$Scope,[string]$Key,[string]$Name)
    if (-not $script:ignoreRemove) { $script:values.Remove("$Scope|$Key|$Name") }
}
function Expect-Failure([scriptblock]$Action) { $failed = $false; try { & $Action | Out-Null } catch { $failed = $true }; if (-not $failed) { throw 'Expected failure.' } }

foreach ($capability in $Script:AdditionalAppPermissions) {
    Reset-Fixture
    $result = Set-AdditionalAppPermission -Capability $capability -Scope User -Mode deny
    if (-not $result.verified -or -not $result.permission.owned -or $result.permission.value -ne 'Deny') { throw "User deny failed: $capability" }
    $result = Set-AdditionalAppPermission -Capability $capability -Scope User -Mode restore
    if (-not $result.verified -or $result.permission.value -ne 'Unset' -or $result.permission.conflict) { throw "User restore failed: $capability" }
    if ($script:values["Machine|$script:root\$capability|Value"].value -ne 'Allow') { throw 'User change modified machine default.' }
    $result = Set-AdditionalAppPermission -Capability $capability -Scope Machine -Mode deny
    $result = Set-AdditionalAppPermission -Capability $capability -Scope Machine -Mode restore
    if (-not $result.verified -or $result.permission.value -ne 'Allow') { throw "Machine restore failed: $capability" }
}
Reset-Fixture
$script:values["Machine|$script:root\bluetooth|Value"].value = 'Prompt'
Set-AdditionalAppPermission bluetooth Machine deny | Out-Null
if ((Set-AdditionalAppPermission bluetooth Machine restore).permission.value -ne 'Prompt') { throw 'Prior Prompt was not restored.' }
Reset-Fixture
$script:admin = $false
Expect-Failure { Set-AdditionalAppPermission bluetooth Machine deny }
if (-not (Set-AdditionalAppPermission bluetooth User deny).verified) { throw 'Standard account could not change its own default.' }
Reset-Fixture
$script:values["Machine|$script:root\bluetooth|Value"].value = 'Deny'
Expect-Failure { Set-AdditionalAppPermission bluetooth Machine deny }
Expect-Failure { Set-AdditionalAppPermission bluetooth Machine restore }
Reset-Fixture
$script:values["Machine|$script:root\bluetooth|Value"] = @{ value = 2; kind = 'DWord' }
Expect-Failure { Set-AdditionalAppPermission bluetooth Machine deny }
Reset-Fixture
$script:keys.Remove("Machine|$script:root\bluetooth")
$script:values.Remove("Machine|$script:root\bluetooth|Value")
Expect-Failure { Set-AdditionalAppPermission bluetooth User deny }
Reset-Fixture
$policyKey = 'SOFTWARE\Policies\Microsoft\Windows\AppPrivacy'
$script:keys["Machine|$policyKey"] = $true
$script:values["Machine|$policyKey|LetAppsAccessHumanPresence_ForceAllowTheseApps"] = @{value = @('example'); kind = 'MultiString'}
Expect-Failure { Set-AdditionalAppPermission humanPresence User deny }
Reset-Fixture
$script:failDeny = $true
Expect-Failure { Set-AdditionalAppPermission bluetooth User deny }
if (-not (Get-AdditionalPermissionState bluetooth User).pending) { throw 'Interrupted pre-write backup was not identified.' }
$script:failDeny = $false
if (-not (Set-AdditionalAppPermission bluetooth User deny).verified) { throw 'Could not retry safely.' }
Reset-Fixture
$script:failFinalize = $true
Expect-Failure { Set-AdditionalAppPermission bluetooth User deny }
if (-not (Get-AdditionalPermissionState bluetooth User).conflict) { throw 'Uncertain ownership was not reported.' }
Expect-Failure { Set-AdditionalAppPermission bluetooth User restore }
Reset-Fixture
$snapshotKey = 'SOFTWARE\WinCommander\AdditionalAppPermissions\bluetooth'
$script:keys["User|$snapshotKey"] = $true
$script:values["User|$snapshotKey|HadValue"] = @{value = 0; kind = 'DWord'}
if (-not (Get-AdditionalPermissionState bluetooth User).conflict) { throw 'Incomplete backup was not reported.' }
Expect-Failure { Set-AdditionalAppPermission bluetooth User deny }
Reset-Fixture
Set-AdditionalAppPermission bluetooth Machine deny | Out-Null
$script:values["Machine|$script:root\bluetooth|Value"].value = 'Prompt'
Expect-Failure { Set-AdditionalAppPermission bluetooth Machine restore }
if ($script:values["Machine|$script:root\bluetooth|Value"].value -ne 'Prompt') { throw 'Overwrote a later change.' }
Reset-Fixture
Set-AdditionalAppPermission bluetooth User deny | Out-Null
$script:ignoreRemove = $true
Expect-Failure { Set-AdditionalAppPermission bluetooth User restore }
if (-not (Get-AdditionalPermissionState bluetooth User).owned) { throw 'Failed restoration lost its backup.' }
Reset-Fixture
Expect-Failure { Set-AdditionalAppPermission '../other' User deny }
Write-Output 'Additional app permission fixture tests passed.'
