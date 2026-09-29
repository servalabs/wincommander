# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
$sourcePath = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/tweaks/system.ps1'
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($sourcePath, [ref]$null, [ref]$parseErrors)
if ($parseErrors.Count) { throw ('Hardening status module did not parse: ' + (($parseErrors | ForEach-Object { "line $($_.Extent.StartLineNumber): $($_.Message)" }) -join '; ')) }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-DashboardPrivacyPolicyStatus' }, $true)
. ([scriptblock]::Create($function.Extent.Text))
function Get-RecallSnapshotsStatus { $script:recall }
function Get-OfficeLoggingStatus { $script:office }
function Get-InternetCommunicationStatus { $script:internet }
function Get-BitLockerAutoEncryptPolicyStatus { $script:bitlocker }

foreach ($value in @($true, $false)) {
    $script:recall = @{ disabled = $value; verified = $true }
    $script:office = @{ disabled = $value; verified = $true }
    $script:internet = @{ restricted = $value; verified = $true }
    $script:bitlocker = @{ disabled = $value; verified = $true }
    $observed = Get-DashboardPrivacyPolicyStatus
    foreach ($field in @('recallSnapshotsDisabled', 'officeLoggingDisabled', 'internetCommRestricted', 'bitlockerAutoEncryptDisabled')) {
        if ($observed[$field] -ne $value) { throw "Verified value lost for $field" }
    }
}
foreach ($verified in @($false, $null)) {
    $script:recall = @{ disabled = $true; verified = $verified }
    $script:office = @{ disabled = $false; verified = $verified }
    $script:internet = @{ restricted = $true; verified = $verified }
    $script:bitlocker = @{ disabled = $false; verified = $verified }
    $observed = Get-DashboardPrivacyPolicyStatus
    foreach ($value in $observed.Values) {
        if ($null -ne $value) { throw 'Unverified observation became a boolean claim.' }
    }
}
$script:recall = @{ disabled = $true; verified = $true }
$observed = Get-DashboardPrivacyPolicyStatus
if ($observed.recallSnapshotsDisabled -ne $true -or $null -ne $observed.officeLoggingDisabled) { throw 'One unavailable probe discarded an independent verified setting.' }
Write-Output 'PASS: four observed policy fields preserve true, false and unknown independently; no Windows settings changed.'
