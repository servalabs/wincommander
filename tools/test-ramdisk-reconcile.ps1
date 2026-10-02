# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\..\src-tauri\commander-free\scripts\modules\vault\ramdisks.ps1"

function Assert-True([bool]$condition, [string]$message) {
    if (-not $condition) { throw $message }
}

function Get-ImDiskExe { return 'imdisk.exe' }
function Get-SystemRamInfo { return @{ totalMB = 16384 } }
function Test-Path { param([string]$Path) if ($Path -eq 'R:\') { return $null -ne $script:owner }; return $false }
function _Get-ImDiskMountOwner { return $script:owner }
function _Get-RamDiskVolumeLabel { return $script:label }
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
    param([int]$DeviceNumber)
    if ($script:failDetach) { throw 'simulated detach failure' }
    $script:units.Remove($DeviceNumber)
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
        return @{ ok = $true; output = (($script:units.Keys | Sort-Object) -join "`n"); exitCode = 0 }
    }
    if ($key -match '^-l -u (\d+)$') {
        $number = [int]$Matches[1]
        if (-not $script:units.ContainsKey($number)) { return @{ ok = $false; output = ''; exitCode = 1 } }
        return @{ ok = $true; output = $script:units[$number]; exitCode = 0 }
    }
    if ($Arguments[0] -eq '-a') {
        $script:owner = 10
        $script:label = 'TEMP'
        $script:units[10] = 'Drive letter: R:' + "`n" + 'No image file.' + "`n" + 'Size: 805306368 bytes (768 MB), Removable, Virtual Memory, HDD.'
        return @{ ok = -not $script:failAttach; output = 'Created device 10: R: -> Image in memory'; exitCode = $(if ($script:failAttach) { 1 } else { 0 }) }
    }
    if ($key -eq '-d -m R:') {
        $script:units.Remove(10)
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
    $script:calls = @()
    $script:failDetach = $false
    $script:failAttach = $false
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
$second = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($second.status -eq 'reused' -and $script:units.Count -eq 1) 'A repeat request allocated a disk'

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
$script:failDetach = $true
$failedCleanup = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($failedCleanup.status -eq 'error' -and $script:owner -eq 7) 'Failed cleanup did not stop safely'
Assert-True (-not ($script:calls | Where-Object { $_ -like '-a*' })) 'Failed cleanup triggered an attach'

Reset-Fixture
$script:owner = $null
$script:units = @{}
$fresh = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($fresh.status -eq 'created' -and $script:owner -eq 10 -and $script:units.Count -eq 1) 'Fresh attach did not create exactly one disk'
$again = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($again.status -eq 'reused' -and $script:units.Count -eq 1) 'Fresh attach was not idempotent'

Reset-Fixture
$script:owner = $null
$script:units = @{}
$script:failAttach = $true
$partial = New-RamDisk -SizeMB 768 -DriveLetter R -Label TEMP
Assert-True ($partial.status -eq 'error' -and $script:units.Count -eq 0 -and $null -eq $script:owner) 'Partial attach left an allocated disk'
Assert-True ($script:calls -contains '-d -m R:') 'Partial attach was not rolled back by its own mount point'

Write-Output 'RAM disk reconciliation checks passed.'
