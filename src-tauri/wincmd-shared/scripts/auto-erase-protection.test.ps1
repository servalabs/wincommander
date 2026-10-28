$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

function Assert-AutoEraseAdmin { throw 'A protected request reached privileged setup.' }
function Invoke-AutoEraseScheduleLock { param($CategoryId,$Operation) & $Operation }
function Register-ScheduledTask { throw 'A protected request attempted task registration.' }

$protected = @(
    'wlanProfiles', 'BROWSERFOOTPRINTS', 'notepadState', 'wslData',
    'dockerDesktopData', 'virtualMachineArtifacts', 'developerCaches',
    'credentialManager', 'sshState', 'passwordManagerCaches'
)
$advertised = (Get-AutoEraseSupportedCategories).categories
foreach ($id in $protected) {
    if ($id -in $advertised) { throw "Protected category was advertised: $id" }
    $single = Set-AutoEraseSchedule -CategoryId $id -IntervalMinutes 60
    $individual = Set-IndividualAutoEraseSchedule -CategoryId $id -IntervalMinutes 60
    $multi = Set-MultiUserAutoEraseSchedule -CategoryId $id -IntervalMinutes 60
    if (-not $single.error -or -not $individual.error -or -not $multi.error) { throw "Protected category was accepted: $id" }
    $bulk = Set-AutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -ManagedByAutoSet -AllowDefaultExcluded
    $multiBulk = Set-MultiUserAutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -ManagedByAutoSet -AllowDefaultExcluded
    if (-not $bulk.error -or -not $multiBulk.error) { throw "Bulk scheduling overrode a protected category: $id" }
}
foreach ($id in $script:AutoEraseUnsupportedCategories.Keys) {
    if (-not (Set-AutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -AllowDefaultExcluded).error) { throw "Unsupported request reached setup: $id" }
}
if ($script:AutoEraseManualOnlyCategories.Count -ne 31) { throw 'Expected 31 individually configured categories.' }
foreach ($id in $script:AutoEraseManualOnlyCategories) {
    if ($id -in $advertised) { throw "Individual-only category was advertised for Auto-set: $id" }
    $bulk = Set-AutoEraseSchedule -CategoryId ($id.ToUpperInvariant()) -IntervalMinutes 60 -ManagedByAutoSet -AllowDefaultExcluded
    $multiBulk = Set-MultiUserAutoEraseSchedule -CategoryId ($id.ToUpperInvariant()) -IntervalMinutes 60 -ManagedByAutoSet -AllowDefaultExcluded
    $dedicatedBulk = Set-ManagedAutoEraseSchedule -CategoryId ($id.ToUpperInvariant()) -IntervalMinutes 60
    if (-not $bulk.error -or -not $multiBulk.error -or -not $dedicatedBulk.error) { throw "Auto-set accepted an individual-only category: $id" }
}
Write-Output "Passed: $($protected.Count) default exclusions require manual authorization; bulk cannot override them; $($script:AutoEraseUnsupportedCategories.Count) unsupported cards cannot register."
