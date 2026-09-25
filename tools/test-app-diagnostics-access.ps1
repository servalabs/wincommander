$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$telemetryModule = Join-Path $repoRoot 'src-tauri\commander-free\scripts\modules\privacy\telemetry.ps1'
. $telemetryModule

$script:permissionValues = @{}
$script:lockedPolicyPath = $null
$script:failPermissionProbe = $false

function Assert-IsAdmin {}
function Add-Type {}

function Set-RegistryValueSafe {
    [CmdletBinding()]
    param([string]$Path, [string]$Name, [object]$Value, [string]$Type)
    $script:permissionValues["$Path|$Name"] = $Value
}

function Remove-ItemProperty {
    [CmdletBinding()]
    param([string]$Path, [string]$Name)
    if ($Path -eq $script:lockedPolicyPath -and $Name -eq 'LetAppsGetDiagnosticInfo') { return }
    [void]$script:permissionValues.Remove("$Path|$Name")
}

function Test-Path {
    [CmdletBinding()]
    param([Parameter(Position = 0)][string]$Path, [string]$LiteralPath)
    $target = if ($LiteralPath) { $LiteralPath } else { $Path }
    @($script:permissionValues.Keys | Where-Object { $_.StartsWith("$target|", [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
}

function Get-ItemProperty {
    [CmdletBinding()]
    param([string]$Path, [string]$LiteralPath, [string]$Name)
    if ($script:failPermissionProbe) { throw 'Mocked registry probe access denied.' }
    $target = if ($LiteralPath) { $LiteralPath } else { $Path }
    $values = [ordered]@{}
    foreach ($property in @('Value', 'LetAppsGetDiagnosticInfo')) {
        $key = "$target|$property"
        if ($script:permissionValues.ContainsKey($key)) { $values[$property] = $script:permissionValues[$key] }
    }
    [pscustomobject]$values
}

function Get-ChildItem {
    [CmdletBinding()]
    param([string]$Path, [string]$LiteralPath, [switch]$Recurse)
    @()
}

function Reset-PermissionValues {
    $script:permissionValues.Clear()
    $script:lockedPolicyPath = $null
    $script:failPermissionProbe = $false
    foreach ($hive in @('HKCU:', 'HKLM:')) {
        $policyPath = "$hive\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy"
        $consentPath = "$hive\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\appDiagnostics"
        $script:permissionValues["$policyPath|LetAppsGetDiagnosticInfo"] = 2
        $script:permissionValues["$consentPath|Value"] = 'Deny'
    }
}

Reset-PermissionValues
$allowed = Set-AppCapabilityAccess -Capability 'appDiagnostics' -Access 'Allow'
if ($allowed.error -or $allowed.value -ne 'Allow') { throw 'App Diagnostics Allow did not clear the force-deny policy and verify effective state.' }
foreach ($hive in @('HKCU:', 'HKLM:')) {
    $policyPath = "$hive\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy"
    $consentPath = "$hive\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\appDiagnostics"
    if ($script:permissionValues.ContainsKey("$policyPath|LetAppsGetDiagnosticInfo")) { throw 'Allow left a force-deny AppPrivacy policy in place.' }
    if ($script:permissionValues["$consentPath|Value"] -ne 'Allow') { throw 'Allow did not update the app diagnostics consent values.' }
}

$denied = Set-AppCapabilityAccess -Capability 'appDiagnostics' -Access 'Deny'
if ($denied.error -or $denied.value -ne 'Deny') { throw 'App Diagnostics Deny did not write and verify its force-deny policy.' }
foreach ($hive in @('HKCU:', 'HKLM:')) {
    $policyPath = "$hive\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy"
    $consentPath = "$hive\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\appDiagnostics"
    if ($script:permissionValues["$policyPath|LetAppsGetDiagnosticInfo"] -ne 2) { throw 'Deny did not set the AppPrivacy force-deny value.' }
    if ($script:permissionValues["$consentPath|Value"] -ne 'Deny') { throw 'Deny did not update the app diagnostics consent values.' }
}

Reset-PermissionValues
$script:lockedPolicyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy'
$policyBlockedAllow = Set-AppCapabilityAccess -Capability 'appDiagnostics' -Access 'Allow'
if (-not $policyBlockedAllow.error -or $policyBlockedAllow.value -ne 'Deny') {
    throw 'Allow was reported successful despite an enforced force-deny policy.'
}

Reset-PermissionValues
$script:failPermissionProbe = $true
$probeFailure = Set-AppCapabilityAccess -Capability 'appDiagnostics' -Access 'Allow'
if (-not $probeFailure.error -or $null -ne $probeFailure.value) {
    throw 'A failed access-state probe was reported as Allow instead of unknown/error.'
}

Write-Output 'App Diagnostics access contract passed: Allow, Deny, enforced policy, and probe failure were verified with mocks.'
