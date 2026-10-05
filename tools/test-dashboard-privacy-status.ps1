# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
$sourcePath = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/tweaks/system.ps1'
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($sourcePath, [ref]$null, [ref]$parseErrors)
if ($parseErrors.Count) { throw ('Hardening status module did not parse: ' + (($parseErrors | ForEach-Object { "line $($_.Extent.StartLineNumber): $($_.Message)" }) -join '; ')) }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-DashboardPrivacyPolicyStatus' }, $true)
. ([scriptblock]::Create($function.Extent.Text))
$script:failingProbe = ''
function Get-RecallSnapshotsStatus { if ($script:failingProbe -eq 'recall') { throw 'Unavailable Recall provider' }; $script:recall }
function Get-OfficeLoggingStatus { if ($script:failingProbe -eq 'office') { throw 'Unavailable Office provider' }; $script:office }
function Get-InternetCommunicationStatus { if ($script:failingProbe -eq 'internet') { throw 'Unavailable Internet policy provider' }; $script:internet }
function Get-BitLockerAutoEncryptPolicyStatus { if ($script:failingProbe -eq 'bitlocker') { throw 'Unavailable BitLocker provider' }; $script:bitlocker }
function Get-DiagnosticEventTracingStatus { if ($script:failingProbe -eq 'diagnostic') { throw 'Unavailable ETW provider' }; $script:diagnostic }
$fields = @('recallSnapshotsDisabled', 'officeLoggingDisabled', 'internetCommRestricted', 'bitlockerAutoEncryptDisabled', 'diagnosticEventTracingDisabled')

foreach ($value in @($true, $false)) {
    $script:recall = @{ disabled = $value; verified = $true }
    $script:office = @{ disabled = $value; verified = $true }
    $script:internet = @{ restricted = $value; verified = $true }
    $script:bitlocker = @{ disabled = $value; verified = $true }
    $script:diagnostic = @{ disabled = $value }
    $observed = Get-DashboardPrivacyPolicyStatus
    foreach ($field in $fields) {
        if ($observed[$field] -ne $value) { throw "Verified value lost for $field" }
    }
}
foreach ($verified in @($false, $null)) {
    $script:recall = @{ disabled = $true; verified = $verified }
    $script:office = @{ disabled = $false; verified = $verified }
    $script:internet = @{ restricted = $true; verified = $verified }
    $script:bitlocker = @{ disabled = $false; verified = $verified }
    $script:diagnostic = @{ disabled = $false; error = $true }
    $observed = Get-DashboardPrivacyPolicyStatus
    foreach ($value in $observed.Values) {
        if ($null -ne $value) { throw 'Unverified observation became a boolean claim.' }
    }
}
$script:recall = @{ disabled = $true; verified = $true }
$observed = Get-DashboardPrivacyPolicyStatus
if ($observed.recallSnapshotsDisabled -ne $true -or $null -ne $observed.officeLoggingDisabled) { throw 'One unavailable probe discarded an independent verified setting.' }
$script:office = @{ disabled = $true; verified = $true }
$script:internet = @{ restricted = $true; verified = $true }
$script:bitlocker = @{ disabled = $true; verified = $true }
$script:diagnostic = @{ disabled = $true }
$names = @('recall', 'office', 'internet', 'bitlocker', 'diagnostic')
for ($index = 0; $index -lt $names.Count; $index++) {
    $script:failingProbe = $names[$index]
    $observed = Get-DashboardPrivacyPolicyStatus | ConvertTo-Json -Compress | ConvertFrom-Json
    foreach ($field in $fields) {
        if ($field -eq $fields[$index]) {
            if ($null -ne $observed.$field) { throw "Failed provider became a boolean: $field" }
        } elseif ($observed.$field -ne $true) { throw "Failed provider discarded unrelated field: $field" }
    }
}
$script:failingProbe = ''
$script:diagnostic = @{ disabled = 'false' }
$script:recall = @{ disabled = 1; verified = $true }
$observed = Get-DashboardPrivacyPolicyStatus
if ($null -ne $observed.diagnosticEventTracingDisabled -or $null -ne $observed.recallSnapshotsDisabled) {
    throw 'A malformed policy observation became a boolean claim.'
}
Write-Output 'PASS: five privacy fields retain true/false/unknown; failed or malformed providers cannot discard unrelated fields; no Windows settings changed.'
