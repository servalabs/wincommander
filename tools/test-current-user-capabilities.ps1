# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1'
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$null, [ref]$errors)
if ($errors.Count) { throw 'Telemetry parser failure.' }
foreach ($name in @('Get-CapabilityRegistryValue', 'Set-CurrentUserCapabilityAccess', 'Set-AppCapabilityAccess')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }.GetNewClosure(), $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
foreach ($name in @('DeviceAccessGuids', 'AppPrivacyValueNames')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq ('$Script:' + $name) }.GetNewClosure(), $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
function Reset-Fixture {
    $script:registry = @{}
    $script:writeCount = 0
    $script:ignoreWrite = $false
    $script:denyRead = $false
    $script:denyWrite = $false
    $script:admin = $false
    $script:effectiveMismatch = $false
}
function Test-IsAdmin { $script:admin }
function Assert-IsAdmin { throw 'Elevated branch reached' }
function Test-Path { [CmdletBinding()]param([string]$LiteralPath) $script:registry.ContainsKey($LiteralPath) }
function New-Item {
    [CmdletBinding()]param([string]$Path, [switch]$Force)
    if (-not $Path.StartsWith('HKCU:\')) { throw 'Attempted machine write.' }
    $script:registry[$Path] = @{}
}
function Get-ChildItem { [CmdletBinding()]param([string]$LiteralPath, [switch]$Recurse) @() }
function Set-ItemProperty {
    [CmdletBinding()]param([string]$LiteralPath, [string]$Name, $Value, [string]$Type, [switch]$Force)
    if (-not $LiteralPath.StartsWith('HKCU:\')) { throw 'Attempted machine write.' }
    $script:writeCount++
    if ($script:denyWrite) { Write-Error 'Denied'; return }
    if (-not $script:ignoreWrite) { $script:registry[$LiteralPath][$Name] = $Value }
}
function Get-ItemProperty {
    [CmdletBinding()]param([string]$LiteralPath)
    if ($script:denyRead) { throw 'Denied read' }
    if (-not $script:registry.ContainsKey($LiteralPath)) { throw [Management.Automation.ItemNotFoundException]::new('Missing key') }
    [pscustomobject]$script:registry[$LiteralPath]
}
function Get-AppCapabilityAccessStatus {
    param([string]$Capability)
    $value = if ($script:effectiveMismatch) { 'Unknown' } else { $script:registry["HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\$Capability"].Value }
    @{ value = $value }
}
foreach ($cap in @('webcam', 'microphone', 'contacts', 'appointments', 'phoneCall', 'phoneCallHistory', 'chat', 'email', 'radios', 'userNotificationListener', 'documentsLibrary', 'picturesLibrary', 'videosLibrary', 'broadFileSystemAccess', 'gazeInput', 'appDiagnostics', 'userAccountInformation', 'bluetoothSync', 'location')) {
    Reset-Fixture
    foreach ($access in @('Deny', 'Allow')) {
        $result = Set-AppCapabilityAccess -Capability $cap -Access $access
        if ($result.error -or -not $result.verified -or $result.scope -ne 'user' -or $result.value -ne $access) { throw "Unverified current-user result for $cap/$access" }
    }
}
foreach ($mode in @('ignoreWrite', 'denyWrite', 'denyRead', 'effectiveMismatch')) {
    Reset-Fixture
    Set-Variable -Name $mode -Value $true -Scope Script
    $result = Set-AppCapabilityAccess -Capability webcam -Access Deny
    if (-not $result.error -or $result.verified) { throw "False success: $mode" }
}
foreach ($policy in @(1, 2)) {
    Reset-Fixture
    $script:registry['HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy'] = @{ LetAppsAccessCamera = $policy }
    $result = Set-AppCapabilityAccess -Capability webcam -Access Deny
    if (-not $result.error -or $script:writeCount) { throw 'Machine policy was ignored or written through.' }
}
foreach ($name in $Script:AppPrivacyValueNames.Values) {
    foreach ($suffix in @('', '_ForceAllowTheseApps', '_ForceDenyTheseApps')) {
        Reset-Fixture
        $value = if ($suffix) { @('managed.application') } else { 1 }
        $script:registry['HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy'] = @{ ($name + $suffix) = $value }
        $cap = @($Script:AppPrivacyValueNames.Keys | Where-Object { $Script:AppPrivacyValueNames[$_] -eq $name })[0]
        $result = Set-AppCapabilityAccess -Capability $cap -Access Deny
        if (-not $result.error -or $script:writeCount) { throw "Managed override was ignored: $name$suffix" }
    }
}
Reset-Fixture
$script:registry['HKLM:\SOFTWARE\Policies\Microsoft\Camera'] = @{ AllowCamera = 0 }
$result = Set-AppCapabilityAccess -Capability webcam -Access Allow
if (-not $result.error -or $script:writeCount) { throw 'User request bypassed Camera policy.' }
Reset-Fixture
$rejected = $false
try { Set-AppCapabilityAccess -Capability '..\outside' -Access Deny | Out-Null } catch { $rejected = $true }
if (-not $rejected -or $script:writeCount) { throw 'Unrecognized capability escaped validation.' }
$script:admin = $true
$rejected = $false
try { Set-AppCapabilityAccess -Capability webcam -Access Deny | Out-Null } catch { $rejected = $_.Exception.Message -eq 'Elevated branch reached' }
if (-not $rejected) { throw 'Elevated policy branch no longer has its administrator guard.' }
'PASS: 19 account capabilities allow/deny, failed read/write, policy overrides, input validation, and preserved administrator branch. All Windows writes mocked.'
