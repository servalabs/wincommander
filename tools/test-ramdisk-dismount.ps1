# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\..\src-tauri\commander-free\scripts\modules\vault\ramdisks.ps1"

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Get-RamDiskStatus { return $script:fixtureStatus }
function Remove-RamDisk {
    param([string]$DriveLetter)
    $script:calls += $DriveLetter
    $outcome = $script:outcomes[$DriveLetter]
    if ($outcome -is [System.Exception]) { throw $outcome }
    if ($null -eq $outcome) { throw "Unexpected fixture drive: $DriveLetter" }
    return $outcome
}

function Reset-Fixture {
    $script:fixtureStatus = @{
        installed = $true
        disks = @(@{ letter = 'R:' }, @{ letter = 'S:' }, @{ letter = 'T:' })
    }
    $script:outcomes = @{
        'R:' = @{ status = 'dismounted'; drive = 'R:' }
        'S:' = @{ status = 'dismounted'; drive = 'S:' }
        'T:' = @{ status = 'dismounted'; drive = 'T:' }
    }
    $script:calls = @()
}

Reset-Fixture
$script:outcomes['S:'] = @{ status = 'error'; error = 'simulated driver refusal' }
$partial = Remove-AllRamDisks
Assert-True ($partial.status -eq 'error') 'A failed disk removal was falsely reported as all dismounted'
Assert-True (($partial.drives -join ',') -eq 'R:,T:') 'Partial removal did not preserve the successful drive list'
Assert-True ($partial.error -match 'S:' -and $partial.error -match 'simulated driver refusal') 'Partial removal omitted the failed drive or its reason'
Assert-True (($script:calls -join ',') -eq 'R:,S:,T:') 'A failed removal stopped attempts on later disks'

Reset-Fixture
$allSuccess = Remove-AllRamDisks
Assert-True ($allSuccess.status -eq 'all_dismounted' -and ($allSuccess.drives -join ',') -eq 'R:,S:,T:') 'Successful bulk removal changed its response contract'

Reset-Fixture
foreach ($letter in @('R:', 'S:', 'T:')) {
    $script:outcomes[$letter] = @{ status = 'error'; error = "simulated refusal for $letter" }
}
$allFailed = Remove-AllRamDisks
Assert-True ($allFailed.status -eq 'error' -and @($allFailed.drives).Count -eq 0) 'Complete failure claimed a dismounted disk'
foreach ($letter in @('R:', 'S:', 'T:')) {
    Assert-True ($allFailed.error.Contains($letter)) "Complete failure omitted $letter"
}
Assert-True ($script:calls.Count -eq 3) 'Complete failure did not attempt every listed disk'

Reset-Fixture
$script:outcomes['S:'] = [InvalidOperationException]::new('simulated removal exception')
$thrown = Remove-AllRamDisks
Assert-True ($thrown.status -eq 'error' -and ($thrown.drives -join ',') -eq 'R:,T:') 'A thrown removal lost the partial result'
Assert-True ($thrown.error -match 'S:' -and $thrown.error -match 'simulated removal exception') 'A thrown removal lost its drive or reason'
Assert-True (($script:calls -join ',') -eq 'R:,S:,T:') 'A thrown removal skipped later disks'

Reset-Fixture
$script:outcomes['S:'] = @{ status = 'unknown'; error = '' }
$unconfirmed = Remove-AllRamDisks
Assert-True ($unconfirmed.status -eq 'error' -and ($unconfirmed.drives -join ',') -eq 'R:,T:') 'An unconfirmed removal was treated as successful'
Assert-True ($unconfirmed.error -match 'S:' -and $unconfirmed.error -match 'Removal did not report success') 'An unconfirmed removal omitted an actionable failure reason'

Reset-Fixture
$script:fixtureStatus.disks = @()
$empty = Remove-AllRamDisks
Assert-True ($empty.status -eq 'all_dismounted' -and @($empty.drives).Count -eq 0 -and $script:calls.Count -eq 0) 'An empty inventory triggered a removal or reported failure'

Reset-Fixture
$script:fixtureStatus.installed = $false
$notInstalled = Remove-AllRamDisks
Assert-True ($notInstalled.status -eq 'error' -and $notInstalled.error -match 'not installed') 'An unavailable RAM disk engine was accepted'
Assert-True ($script:calls.Count -eq 0) 'An unavailable RAM disk engine triggered a removal'

Write-Output 'RAM disk bulk dismount checks passed.'
