# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\..\src-tauri\commander-free\scripts\modules\vault\ramdisks.ps1"

function Assert-True([bool]$condition, [string]$message) {
    if (-not $condition) { throw $message }
}

Assert-True (_Test-ImDiskDeviceListResult -ExitCode 1 -Stderr '') 'ImDisk list success code was rejected'
Assert-True (-not (_Test-ImDiskDeviceListResult -ExitCode 0 -Stderr 'driver unavailable')) 'ImDisk list error was accepted'

function Get-ImDiskExe { return 'imdisk.exe' }
function Get-SystemRamInfo { return @{ totalMB = 16384 } }
function _Ensure-RamDiskWritableAccess {
    param([string]$Letter, [int]$DeviceNumber, [bool]$ReadOnly)
    $script:calls += "access $DeviceNumber $ReadOnly"
    if ($script:failAccess) { throw 'simulated access repair failure' }
}
function Test-Path { param([string]$Path) if ($Path -eq 'R:\') { return $null -ne $script:owner }; return $false }
function _Get-ImDiskMountOwner { return $script:owner }
function _Get-ImDiskUnitOpenError {
    param([int]$DeviceNumber)
    $script:calls += "probe $DeviceNumber"
    if ($script:openErrors.ContainsKey($DeviceNumber)) { return $script:openErrors[$DeviceNumber] }
    if ($script:units.ContainsKey($DeviceNumber)) { return 0 }
    return 2
}
function _Get-RamDiskVolumeLabel { return $script:label }
function _Get-ImDiskUnitVolumeLabel {
    param([int]$DeviceNumber)
    if ($script:changeMountDuringRestore -and $null -eq $script:owner -and $script:units.Count -eq 1) {
        $script:owner = 99
    }
    return $script:unitLabels[$DeviceNumber]
}
function _Set-ImDiskMountPoint {
    param([string]$Letter, [int]$DeviceNumber, [bool]$Remove = $false)
    $script:calls += "$(if ($Remove) { 'unlink' } else { 'mount' }) $DeviceNumber"
    if ($Remove) {
        if ($script:owner -eq $DeviceNumber) { $script:owner = $null }
    } else {
        $script:owner = $DeviceNumber
    }
}
function _Dismount-ImDiskUnitWithoutMountPoint {
    param([int]$DeviceNumber, [string]$ExpectedLabel)
    if ($PSBoundParameters.ContainsKey('ExpectedLabel') -and $script:unitLabels[$DeviceNumber] -ine $ExpectedLabel) { throw 'unexpected volume label' }
    if ($script:failDetach) { throw 'simulated detach failure' }
    $script:units.Remove($DeviceNumber)
    $script:unitLabels.Remove($DeviceNumber)
    if ($script:dropMountOnDetach) { $script:owner = $null }
    if ($script:changeMountOnDetach) { $script:owner = 99 }
    $script:calls += "eject $DeviceNumber"
}
function _Invoke-ImDisk {
    param([string[]]$Arguments)
    $key = $Arguments -join ' '
    $script:calls += $key
    if ($key -eq '-l -m R:') {
        if ($null -eq $script:owner -or -not $script:units.ContainsKey($script:owner)) {
            return @{ ok = $false; output = ''; exitCode = 1 }
        }
        return @{ ok = $true; output = 'Mount point: R:'; exitCode = 0 }
    }
    if ($key -eq '-l -n') {
        if ($null -ne $script:listFailure) { return $script:listFailure }
        return @{ ok = $true; output = ((@($script:units.Keys) + $script:extraListedUnits | Sort-Object -Unique) -join "`n"); exitCode = 0 }
    }
    if ($key -match '^-l -u (\d+)$') {
        $number = [int]$Matches[1]
        if ($script:detailFailures.ContainsKey($number)) { return $script:detailFailures[$number] }
        if (-not $script:units.ContainsKey($number)) { return @{ ok = $false; output = ''; exitCode = 1 } }
        return @{ ok = $true; output = $script:units[$number]; exitCode = 0 }
    }
    if ($Arguments[0] -eq '-a') {
        $script:owner = if ($script:failAttachWithoutMount) { $null } else { 10 }
        $script:label = if ($script:failAttach) { $null } else { 'TEMP' }
        $script:unitLabels[10] = $script:label
        $script:units[10] = 'Drive letter: R:' + "`n" + 'No image file.' + "`n" + 'Size: 805306368 bytes (768 MB), Removable, Virtual Memory, HDD.'
        return @{ ok = -not $script:failAttach; output = 'Created device 10: R: -> Image in memory'; exitCode = $(if ($script:failAttach) { 1 } else { 0 }) }
    }
    if ($key -eq '-d -m R:') {
        $script:units.Remove(10)
        $script:unitLabels.Remove(10)
        $script:owner = $null
        return @{ ok = $true; output = 'Done.'; exitCode = 0 }
    }
    throw "unexpected ImDisk call: $key"
}

$realOutput = "Drive letter: R`nNo image file.`nSize: 805306368 bytes (768 MB), Removable, Virtual Memory, HDD, Modified."
$parsed = _Parse-ImDiskDeviceDetails -Text $realOutput
Assert-True ($parsed.letter -eq 'R:' -and $parsed.isRam -and $parsed.sizeBytes -eq 805306368 -and -not $parsed.imageFile) 'Real ImDisk detail format was not parsed'

function Reset-Fixture {
    $script:owner = 7
    $script:label = 'TEMP'
    $script:units = @{
        7 = $realOutput
        3 = $realOutput
    }
    $script:unitLabels = @{ 7 = 'TEMP'; 3 = 'TEMP' }
    $script:calls = @()
    $script:failDetach = $false
    $script:failAttach = $false
    $script:failAttachWithoutMount = $false
    $script:dropMountOnDetach = $false
    $script:changeMountOnDetach = $false
    $script:changeMountDuringRestore = $false
    $script:failAccess = $false
    $script:extraListedUnits = @()
    $script:detailFailures = @{}
    $script:openErrors = @{}
    $script:listFailure = $null
}

Reset-Fixture
$status = Get-RamDiskStatus
Assert-True ($status.disks.Count -eq 2 -and $script:calls -contains '-l -u 3' -and $script:calls -contains '-l -u 7') 'Status did not enumerate individual ImDisk units'
$script:calls = @()
$first = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($first.status -eq 'reused') 'Existing live R: was not reused'
Assert-True ($script:units.Count -eq 1 -and $script:units.ContainsKey(7) -and $script:owner -eq 7) 'Duplicate cleanup changed the live R: owner'
Assert-True (($script:calls -contains 'eject 3') -and -not ($script:calls | Where-Object { $_ -like '-a*' })) 'Duplicate cleanup did not use unit-only eject'
Assert-True ($script:calls[0] -eq '-l -m R:') 'Mount point was not queried first'
Assert-True ($script:calls -contains 'access 7 False') 'Reuse did not ensure normal user write access'
$second = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($second.status -eq 'reused' -and $script:units.Count -eq 1) 'A repeat request allocated a disk'

Reset-Fixture
$script:dropMountOnDetach = $true
$driverUnlinked = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($driverUnlinked.status -eq 'reused' -and $script:owner -eq 7 -and $script:units.Count -eq 1) 'Driver eject removed the live drive letter without restoring it'
Assert-True (($script:calls -contains 'mount 7') -and -not ($script:calls | Where-Object { $_ -like '-a*' })) 'Drive-letter restoration allocated another disk'

Reset-Fixture
$script:changeMountOnDetach = $true
$changedOwner = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($changedOwner.status -eq 'error' -and $script:owner -eq 99) 'Duplicate cleanup replaced a different live mount'
Assert-True (-not ($script:calls | Where-Object { $_ -like 'mount *' -or $_ -like '-a*' })) 'An occupied drive letter was replaced during recovery'

Reset-Fixture
$script:dropMountOnDetach = $true
$script:changeMountDuringRestore = $true
$claimedDuringRestore = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($claimedDuringRestore.status -eq 'error' -and $script:owner -eq 99) 'Restoration covered a drive claimed while checking the survivor'
Assert-True (-not ($script:calls | Where-Object { $_ -like 'mount *' -or $_ -like '-a*' })) 'Restoration did not recheck the free drive letter'

Reset-Fixture
$script:owner = $null
$orphanedMount = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($orphanedMount.status -eq 'reused' -and $script:units.Count -eq 1 -and $script:owner -eq 3) 'An attached unit was replaced instead of remounted'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' })) 'Recovery allocated another System-backed disk'

Reset-Fixture
$script:owner = 99
$staleLetter = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($staleLetter.status -eq 'reused' -and $script:owner -eq 3 -and $script:units.Count -eq 1) 'A stale session letter was not repaired'

Reset-Fixture
$script:label = 'OTHER'
$wrongLabel = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($wrongLabel.status -eq 'error' -and $script:units.Count -eq 2) 'A different label was treated as TEMP'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' })) 'An occupied letter triggered an attach'

Reset-Fixture
$script:unitLabels[3] = 'OTHER'
$wrongStaleLabel = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($wrongStaleLabel.status -eq 'error' -and $script:units.Count -eq 2 -and $script:owner -eq 7) 'A same-size disk with another label was detached'

Reset-Fixture
$script:failDetach = $true
$failedCleanup = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($failedCleanup.status -eq 'error' -and $script:owner -eq 7) 'Failed cleanup did not stop safely'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' })) 'Failed cleanup triggered an attach'

Reset-Fixture
$script:owner = $null
$script:units = @{}
$fresh = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($fresh.status -eq 'created' -and $script:owner -eq 10 -and $script:units.Count -eq 1) 'Fresh attach did not create exactly one disk'
Assert-True ($script:calls -contains 'access 10 False') 'Creation did not ensure normal user write access'
$again = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($again.status -eq 'reused' -and $script:units.Count -eq 1) 'Fresh attach was not idempotent'

Reset-Fixture
$script:failAccess = $true
$accessFailure = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($accessFailure.status -eq 'error' -and $script:owner -eq 7 -and $script:units.Count -eq 1) 'Access repair failure destroyed the reused disk or returned success'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' -or $_ -eq '-d -m R:' })) 'Access repair failure replaced the existing disk'

Reset-Fixture
$script:owner = $null
$script:units = @{}
$script:failAccess = $true
$newAccessFailure = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($newAccessFailure.status -eq 'error' -and $script:units.Count -eq 0) 'New disk access failure was not rolled back'

Reset-Fixture
$script:owner = $null
$script:units = @{}
$script:failAttach = $true
$partial = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($partial.status -eq 'error' -and $script:units.Count -eq 0 -and $null -eq $script:owner) 'Partial attach left an allocated disk'
Assert-True ($script:calls -contains '-d -m R:') 'Partial attach was not rolled back by its own mount point'

Reset-Fixture
$script:owner = $null
$script:units = @{}
$script:failAttach = $true
$script:failAttachWithoutMount = $true
$unmountedPartial = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($unmountedPartial.status -eq 'error' -and $script:units.Count -eq 0) 'Unformatted allocated unit was not rolled back'
Assert-True ($script:calls -contains 'eject 10') 'Unformatted allocated unit was not detached by unit number'

Reset-Fixture
$script:owner = $null
$script:units = @{}
$script:extraListedUnits = @(0)
$script:detailFailures[0] = @{ ok = $false; output = 'Error opening device: The device has been removed.'; exitCode = 2 }
$script:openErrors[0] = 1617
$removedOnly = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($removedOnly.status -eq 'created' -and $script:owner -eq 10 -and $script:units.Count -eq 1) 'A listed but removed unit blocked fresh creation'
$removedAgain = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($removedAgain.status -eq 'reused' -and $script:owner -eq 10 -and $script:units.Count -eq 1) 'A repeated request replaced the survivor beside a removed unit'
Assert-True (@($script:calls | Where-Object { $_ -like '-a*' }).Count -eq 1) 'Removed unit recovery allocated more than once'
Assert-True (-not ($script:calls | Where-Object { $_ -like 'eject *' })) 'An already removed unit was ejected'

Reset-Fixture
$script:owner = 0
$script:units = @{}
$script:extraListedUnits = @(0)
$script:detailFailures[0] = @{ ok = $false; output = ''; exitCode = 2 }
$script:openErrors[0] = 1617
$removedOwner = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($removedOwner.status -eq 'created' -and $script:owner -eq 10 -and $script:units.Count -eq 1) 'A removed unit drive-letter mapping blocked recovery'
Assert-True (($script:calls -contains 'unlink 0') -and -not ($script:calls | Where-Object { $_ -like 'eject *' })) 'Removed unit recovery did not unlink only its stale mapping'

Reset-Fixture
$script:units.Remove(3)
$script:extraListedUnits = @(0)
$script:detailFailures[0] = @{ ok = $false; output = 'Error opening device: The device has been removed.'; exitCode = 2 }
$script:openErrors[0] = 1617
$mixedRemoved = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($mixedRemoved.status -eq 'reused' -and $script:owner -eq 7 -and $script:units.Count -eq 1 -and $script:units[7] -eq $realOutput) 'Removed unit recovery changed the live surviving disk'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' -or $_ -like 'eject *' -or $_ -like 'mount *' })) 'Removed unit recovery allocated, ejected, or remounted a live survivor'

Reset-Fixture
$script:owner = 99
$script:openErrors[99] = 5
$uninspectableOwner = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($uninspectableOwner.status -eq 'error' -and $script:owner -eq 99 -and $script:units.Count -eq 2) 'An inaccessible stale owner was unlinked'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' -or $_ -like 'eject *' -or $_ -like 'mount *' -or $_ -like 'unlink *' })) 'An inaccessible mount owner allowed disk mutation'

Reset-Fixture
$script:openErrors[3] = 5
$unconfirmedEject = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($unconfirmedEject.status -eq 'error' -and $script:owner -eq 7 -and $script:units.ContainsKey(7)) 'A failed inspection after duplicate removal was reported as successful'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' -or $_ -like 'mount *' })) 'An unconfirmed eject replaced or remounted the survivor'

foreach ($openError in @(0, 5, 32, 50, 12345)) {
    Reset-Fixture
    $script:owner = $null
    $script:units = @{}
    $script:extraListedUnits = @(0)
    $script:detailFailures[0] = @{ ok = $false; output = 'Error opening device: The device has been removed.'; exitCode = 2 }
    $script:openErrors[0] = $openError
    $uninspectable = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
    Assert-True ($uninspectable.status -eq 'error' -and $script:units.Count -eq 0) "An uninspectable unit was treated as removed: Windows error $openError"
    Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' -or $_ -like 'eject *' -or $_ -like 'mount *' })) 'An inspection failure allowed disk mutation'
}

foreach ($openError in @(2, 3, 1617)) {
    Reset-Fixture
    $script:owner = $null
    $script:units = @{}
    $script:extraListedUnits = @(0)
    $script:detailFailures[0] = @{ ok = $false; output = ''; exitCode = 2 }
    $script:openErrors[0] = $openError
    $missingUnit = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
    Assert-True ($missingUnit.status -eq 'created' -and $script:units.Count -eq 1) "Windows error $openError did not allow recovery without localized text"
    Assert-True (@($script:calls | Where-Object { $_ -like '-a*' }).Count -eq 1) 'Missing device recovery allocated more than once'
}

Reset-Fixture
$script:owner = $null
$script:units = @{}
$script:listFailure = @{ ok = $false; output = 'driver unavailable'; exitCode = 0 }
$failedList = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($failedList.status -eq 'error' -and -not ($script:calls | Where-Object { $_ -like '-a*' })) 'A failed device list allowed allocation'

Write-Output 'RAM disk reconciliation checks passed.'
