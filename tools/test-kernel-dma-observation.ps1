# SPDX-License-Identifier: AGPL-3.0-or-later
# Unit-test the version-tolerant WMI interpretation without reading or changing
# any Windows setting on the host running the test.
$ErrorActionPreference = 'Stop'
$sourcePath = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/core/utils.ps1'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($sourcePath, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ('Core utilities did not parse: ' + (($parseErrors | ForEach-Object { "line $($_.Extent.StartLineNumber): $($_.Message)" }) -join '; ')) }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-KernelDMAProtectionObservation' }, $true)
if (-not $function) { throw 'Missing Get-KernelDMAProtectionObservation.' }
. ([scriptblock]::Create($function.Extent.Text))

$script:deviceGuardResult = $null
$script:deviceGuardThrows = $false
function Get-CimInstance {
    if ($script:deviceGuardThrows) { throw 'Injected provider failure.' }
    return $script:deviceGuardResult
}
function Assert-Equal($Actual, $Expected, [string]$Message) {
    if ($Actual -ne $Expected) { throw "$Message Expected '$Expected', got '$Actual'." }
}

$script:deviceGuardResult = [pscustomobject]@{ KernelDMAProtection = 2; AvailableSecurityProperties = @(1, 3) }
$active = Get-KernelDMAProtectionObservation
Assert-Equal $active.actuallyActive $true 'Reported active DMA protection must stay active.'
Assert-Equal $active.observable $true 'A present active-state property must be observable.'
Assert-Equal $active.firmwareCapable $true 'Availability code 3 must report firmware capability.'

$script:deviceGuardResult = [pscustomobject]@{ AvailableSecurityProperties = @(1, 3, 4) }
$unknown = Get-KernelDMAProtectionObservation
if ($null -ne $unknown.actuallyActive) { throw 'A Device Guard provider without KernelDMAProtection must remain unknown, not false.' }
Assert-Equal $unknown.observable $false 'A missing active-state property must be unobservable.'
Assert-Equal $unknown.firmwareCapable $true 'Capability remains useful even when active state is unavailable.'

$script:deviceGuardResult = [pscustomobject]@{ KernelDMAProtection = 0; AvailableSecurityProperties = @(1) }
$inactive = Get-KernelDMAProtectionObservation
Assert-Equal $inactive.actuallyActive $false 'A present inactive-state property must report false.'
Assert-Equal $inactive.observable $true 'A present inactive-state property remains observable.'
Assert-Equal $inactive.firmwareCapable $false 'Missing availability code 3 must not claim DMA capability.'

$script:deviceGuardThrows = $true
$failed = Get-KernelDMAProtectionObservation
if ($null -ne $failed.actuallyActive -or $failed.observable -ne $false -or $null -ne $failed.firmwareCapable) {
    throw 'An unreadable provider must remain an unknown observation.'
}

Write-Output 'PASS: Kernel DMA WMI observations distinguish active, inactive, unknown, and unreadable providers; no Windows settings changed.'
