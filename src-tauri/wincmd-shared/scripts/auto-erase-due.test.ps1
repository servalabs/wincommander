$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

$first = [DateTime]::Parse('2026-10-01T00:00:00Z').ToUniversalTime()
function Assert-Due($state, $interval, $expected) {
    $result = Get-AutoEraseNextDue -State $state -FirstDueUtc $first -IntervalMinutes $interval
    $expectedTime = [DateTime]::Parse($expected).ToUniversalTime()
    if ($result.nextDue -ne $expectedTime) { throw "Expected $expectedTime, got $($result.nextDue)" }
}

Assert-Due $null 60 '2026-10-01T00:00:00Z'
Assert-Due ([pscustomobject]@{lastSuccessfulUtc='2026-10-01T00:00:00Z'}) 60 '2026-10-01T01:00:00Z'
# A failed attempt keeps the prior success while postponing the next retry.
$failed = [pscustomobject]@{lastSuccessfulUtc='2026-10-01T00:00:00Z'; attemptedUtc='2026-10-05T00:00:00Z'; nextDueUtc='2026-10-05T01:00:00Z'; intervalMinutes=60; success=$false}
Assert-Due $failed 60 '2026-10-05T01:00:00Z'
# A newly chosen cadence uses the last successful clear rather than an old deadline.
Assert-Due $failed 5 '2026-10-01T00:05:00Z'
# A successful run makes duplicate triggers wait until the next interval.
$success = [pscustomobject]@{lastSuccessfulUtc='2026-10-05T00:00:00Z'; nextDueUtc='2026-10-05T01:00:00Z'; intervalMinutes=60; success=$true}
Assert-Due $success 60 '2026-10-05T01:00:00Z'
$noSuccess = [pscustomobject]@{attemptedUtc='2026-10-05T00:00:00Z'; nextDueUtc='2026-10-05T01:00:00Z'; intervalMinutes=60; success=$false}
Assert-Due $noSuccess 5 '2026-10-05T00:05:00Z'
$previous = [DateTime]::Parse('2026-10-01T00:00:00Z').ToUniversalTime()
$completed = [DateTime]::Parse('2026-10-05T00:00:00Z').ToUniversalTime()
foreach ($counts in @(@(1, 0), @(0, 1))) {
    $result = New-AutoEraseResult -CompletedUtc $completed -LastSuccess $previous -IntervalMinutes 60 -Removed 1 -Deferred $counts[0] -Failed $counts[1]
    if ($result.success -or [DateTime]::Parse($result.lastSuccessfulUtc).ToUniversalTime() -ne $previous) { throw 'A failed or deferred clear advanced the last success.' }
}
$result = New-AutoEraseResult -CompletedUtc $completed -LastSuccess $previous -IntervalMinutes 60 -Removed 1 -Deferred 0 -Failed 0
if (-not $result.success -or [DateTime]::Parse($result.lastSuccessfulUtc).ToUniversalTime() -ne $completed) { throw 'A successful clear did not advance the last success.' }
Write-Output 'Passed: initial, overdue, failed, interval-change, and duplicate-trigger timing.'
