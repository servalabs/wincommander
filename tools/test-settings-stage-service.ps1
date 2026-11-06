[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/settings-stage-service.ps1')
function Test-Sequence([string]$Target, [int[]]$States, [string]$ExpectedAction) {
    $script:remainingStates = [Collections.Generic.Queue[int]]::new()
    foreach ($state in $States) { $script:remainingStates.Enqueue($state) }
    $script:actions = [Collections.Generic.List[string]]::new()
    Invoke-ServiceTransition $Target { $script:remainingStates.Dequeue() } { $script:actions.Add('Start') } { $script:actions.Add('Stop') } { } 5
    if (($script:actions -join ',') -ne $ExpectedAction) { throw "Unexpected actions for $Target." }
}
Test-Sequence 'Stop' @(4, 3, 3, 1) 'Stop'
Test-Sequence 'Restore' @(3, 3, 1, 2, 4) 'Start'
Test-Sequence 'Restore' @(4) ''
Test-Sequence 'Restore' @(2, 2, 4) ''
Test-Sequence 'Stop' @(0) ''
$denied = $false
try { Invoke-ServiceTransition 'Restore' { throw 'access denied' } { } { } { } 5 } catch { $denied = $true }
if (-not $denied) { throw 'Access denial was silently accepted.' }
$missing = $false
try { Invoke-ServiceTransition 'Restore' { 0 } { } { } { } 5 } catch { $missing = $true }
if (-not $missing) { throw 'Missing service was treated as restored.' }
$timeout = $false
try { Invoke-ServiceTransition 'Restore' { 3 } { } { } { } 0 } catch { $timeout = $true }
if (-not $timeout) { throw 'Pending service did not time out.' }
$startupFailure = $false
try { Invoke-ServiceTransition 'Restore' { 2 } { } { } { } 0 } catch { $startupFailure = $true }
if (-not $startupFailure) { throw 'Unfinished service startup was treated as ready.' }
Write-Output 'Settings staging service transition fixtures passed.'
