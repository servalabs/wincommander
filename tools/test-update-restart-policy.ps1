$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/tweaks/system.ps1'
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$null, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Get-UpdateRestartPolicyState', 'Get-UpdateRestartPolicyStatus', 'Set-UpdateRestartPolicy')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }.GetNewClosure(), $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:policyKey = 'SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'
$script:markerKey = 'SOFTWARE\WinCommander\UpdateRestartPolicy'
function Reset-Fixture {
    $script:registry = @{}
    $script:registry["$script:policyKey|AUOptions"] = @{ exists = $true; kind = 'DWord'; value = 4 }
    $script:admin = $true
    $script:ignoreWrite = $false
    $script:ignoreRemove = $false
    $script:failPolicyWrite = $false
    $script:failFinalMarker = $false
}
function Test-IsAdmin { $script:admin }
function Assert-IsAdmin { if (-not $script:admin) { throw 'Administrator privileges required.' } }
function Invoke-WithUpdateRestartMutex { param([scriptblock]$Action) & $Action }
function Get-UpdateRestartRegistryValue {
    param([string]$Key, [string]$Name)
    $item = $script:registry["$Key|$Name"]
    if ($null -eq $item) { return @{ exists = $false; kind = $null; value = $null } }
    return $item
}
function Set-UpdateRestartRegistryValue {
    param([string]$Key, [string]$Name, [int]$Value)
    if ($script:failPolicyWrite -and $Name -eq 'NoAutoRebootWithLoggedOnUsers') { throw 'Simulated policy write failure.' }
    if ($script:failFinalMarker -and $Name -eq 'CreatedPolicy' -and $Value -eq 2) { throw 'Simulated ownership write failure.' }
    if (-not $script:ignoreWrite) { $script:registry["$Key|$Name"] = @{ exists = $true; kind = 'DWord'; value = $Value } }
}
function Remove-UpdateRestartRegistryValue {
    param([string]$Key, [string]$Name)
    if (-not $script:ignoreRemove) { $script:registry.Remove("$Key|$Name") }
}
function Expect-Failure([scriptblock]$Action) {
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    if (-not $failed) { throw 'Expected operation to fail.' }
}

Reset-Fixture
$result = Set-UpdateRestartPolicy apply
if (-not ($result.owned -and $result.configured -and $result.scheduledMode)) { throw 'Apply was not verified.' }
$result = Set-UpdateRestartPolicy revert
if ($result.owned -or $result.policyExists -or $result.reserved) { throw 'Revert was not verified.' }
if ($script:registry["$script:policyKey|AUOptions"].value -ne 4) { throw 'Changed update scheduling.' }

foreach ($value in @(0, 1, 2)) {
    Reset-Fixture
    $script:registry["$script:policyKey|NoAutoRebootWithLoggedOnUsers"] = @{ exists = $true; kind = 'DWord'; value = $value }
    Expect-Failure { Set-UpdateRestartPolicy apply }
    Expect-Failure { Set-UpdateRestartPolicy revert }
    if ($script:registry["$script:policyKey|NoAutoRebootWithLoggedOnUsers"].value -ne $value) { throw 'Changed an external policy.' }
}
Reset-Fixture
$script:registry["$script:policyKey|AUOptions"].value = 3
Expect-Failure { Set-UpdateRestartPolicy apply }
if ($script:registry.ContainsKey("$script:markerKey|CreatedPolicy")) { throw 'Wrote marker without required schedule.' }
Reset-Fixture
$script:registry["$script:policyKey|NoAutoUpdate"] = @{ exists = $true; kind = 'DWord'; value = 1 }
Expect-Failure { Set-UpdateRestartPolicy apply }
Reset-Fixture
$script:admin = $false
Expect-Failure { Set-UpdateRestartPolicy apply }
Reset-Fixture
$script:ignoreWrite = $true
Expect-Failure { Set-UpdateRestartPolicy apply }
Reset-Fixture
$script:failPolicyWrite = $true
Expect-Failure { Set-UpdateRestartPolicy apply }
$pending = Get-UpdateRestartPolicyStatus
if (-not $pending.reserved -or $pending.policyExists -or $pending.conflict) { throw 'Failed write did not leave a safe reservation.' }
$script:failPolicyWrite = $false
if (-not (Set-UpdateRestartPolicy apply).configured) { throw 'Could not resume an interrupted policy write.' }
Reset-Fixture
$script:failFinalMarker = $true
Expect-Failure { Set-UpdateRestartPolicy apply }
if (-not (Get-UpdateRestartPolicyStatus).conflict) { throw 'Uncertain ownership was not reported.' }
Expect-Failure { Set-UpdateRestartPolicy revert }
Reset-Fixture
Set-UpdateRestartPolicy apply | Out-Null
$script:registry["$script:policyKey|NoAutoRebootWithLoggedOnUsers"].value = 0
Expect-Failure { Set-UpdateRestartPolicy revert }
if ($script:registry["$script:policyKey|NoAutoRebootWithLoggedOnUsers"].value -ne 0) { throw 'Overwrote a later external change.' }
Reset-Fixture
$script:registry["$script:markerKey|CreatedPolicy"] = @{ exists = $true; kind = 'DWord'; value = 1 }
$script:registry["$script:policyKey|NoAutoRebootWithLoggedOnUsers"] = @{ exists = $true; kind = 'DWord'; value = 1 }
Expect-Failure { Set-UpdateRestartPolicy revert }
Reset-Fixture
Set-UpdateRestartPolicy apply | Out-Null
$script:ignoreRemove = $true
Expect-Failure { Set-UpdateRestartPolicy revert }
if (-not $script:registry.ContainsKey("$script:markerKey|CreatedPolicy")) { throw 'Lost ownership after failed removal.' }
Write-Output 'Update restart policy fixture tests passed.'
