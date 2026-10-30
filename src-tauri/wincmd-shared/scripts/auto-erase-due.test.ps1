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

# Completion a few seconds after the slot must not postpone the next scheduled trigger.
$slot = [DateTime]::Parse('2026-10-05T00:00:00Z').ToUniversalTime()
$normal = New-AutoEraseResult -CompletedUtc $slot.AddSeconds(2) -ScheduledDueUtc $slot -IntervalMinutes 60 -Removed 0 -Deferred 0 -Failed 0
if ([DateTime]::Parse($normal.nextDueUtc).ToUniversalTime() -ne $slot.AddHours(1)) { throw 'A short run missed the next Windows trigger.' }
Assert-Due ([pscustomobject]$normal) 60 '2026-10-05T01:00:00Z'
# A delayed catch-up advances past missed slots and does not repeat in the same slot.
$delayed = New-AutoEraseResult -CompletedUtc $slot.AddMinutes(125) -ScheduledDueUtc $slot -IntervalMinutes 60 -Removed 0 -Deferred 0 -Failed 0
if ([DateTime]::Parse($delayed.nextDueUtc).ToUniversalTime() -ne $slot.AddHours(3)) { throw 'Delayed cleanup did not advance to the next scheduled slot.' }
Assert-Due ([pscustomobject]$delayed) 60 '2026-10-05T03:00:00Z'
Write-Output 'Passed: fixed scheduled cadence and delayed catch-up.'
