# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\..\src-tauri\commander-free\scripts\modules\vault\ramdisks.ps1"

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

$inherit = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
    [System.Security.AccessControl.InheritanceFlags]::ObjectInherit
$other = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-21-1-2-3-9999')
$authenticatedUsers = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-11')
$users = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-32-545')

function Get-AccessAces($Acl) {
    $sddl = $Acl.GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::Access)
    return @([regex]::Matches($sddl, '\([^)]*\)') | ForEach-Object { $_.Value })
}

function Reset-Fixture {
    # Resembles a freshly formatted NTFS root: other accounts can read but not save.
    $script:acl = New-Object System.Security.AccessControl.DirectorySecurity
    $script:acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
        $users, 'ReadAndExecute', $inherit, 'None', 'Allow')))
    $script:acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
        $other, 'ReadAndExecute', 'Deny')))
    $script:owner = 7
    $script:filesystem = 'NTFS'
    $script:writes = 0
    $script:changeOwnerOnRead = $false
    $script:ignoreWrite = $false
}
function _Get-RamDiskFilesystem { return $script:filesystem }
function _Get-ImDiskMountOwner { return $script:owner }
function Get-Acl {
    param([string]$LiteralPath, $ErrorAction)
    if ($script:changeOwnerOnRead) { $script:owner = 99 }
    $copy = New-Object System.Security.AccessControl.DirectorySecurity
    $copy.SetSecurityDescriptorSddlForm($script:acl.GetSecurityDescriptorSddlForm('All'))
    return $copy
}
function Set-Acl {
    param([string]$LiteralPath, $AclObject, $ErrorAction)
    Assert-True ($LiteralPath -eq 'R:\') 'ACL repair targeted a different path'
    $script:writes++
    if (-not $script:ignoreWrite) { $script:acl = $AclObject }
}

# Machine-wide grant builder: exact ACEs, inherited by every file and folder.
$built = New-Object System.Security.AccessControl.DirectorySecurity
_Add-RamDiskMachineAccess -Acl $built
$aces = Get-AccessAces $built
$expected = @('(A;OICI;FA;;;SY)', '(A;OICI;FA;;;BA)', '(A;OICI;0x1301bf;;;AU)')
Assert-True ($aces.Count -eq 3 -and @($expected | Where-Object { $aces -notcontains $_ }).Count -eq 0) "Machine-wide grant built unexpected ACEs: $($aces -join '')"
Assert-True (@($aces | Where-Object { $_ -match ';(WD|AN|S-1-1-0|S-1-5-7)\)$' }).Count -eq 0) 'Machine-wide grant included Everyone or Anonymous'
Assert-True (_Test-RamDiskMachineAccess -Acl $built) 'Built machine-wide grant did not satisfy its own check'

Reset-Fixture
Assert-True (-not (_Test-RamDiskMachineAccess -Acl $script:acl)) 'A root without Authenticated Users Modify was accepted'
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
Assert-True (_Test-RamDiskMachineAccess -Acl $script:acl) 'Other users still cannot save on the RAM disk after repair'
Assert-True ($script:writes -eq 1) 'Repair did not write exactly once'
$aces = Get-AccessAces $script:acl
Assert-True (@($aces | Where-Object { $_ -match '^\(D;' }).Count -eq 1) 'Repair removed an existing deny rule'
Assert-True (@($aces | Where-Object { $_ -match ';(WD|AN)\)$' }).Count -eq 0) 'Repair granted Everyone or Anonymous'
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
Assert-True ($script:writes -eq 1) 'Reuse rewrote an already machine-wide ACL'

# A disk made by an older build carries only the creator's Modify rule.
Reset-Fixture
$creator = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-21-1-2-3-1001')
$script:acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
    $creator, 'Modify', $inherit, 'None', 'Allow')))
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
Assert-True ($script:writes -eq 1 -and (_Test-RamDiskMachineAccess -Acl $script:acl)) 'Creator-only RAM disk was not opened to other users on reuse'

foreach ($case in @('folderOnly', 'readOnlyRights')) {
    Reset-Fixture
    $rights = if ($case -eq 'readOnlyRights') { 'ReadAndExecute' } else { 'Modify' }
    $flags = if ($case -eq 'folderOnly') { 'None' } else { $inherit }
    $script:acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
        $authenticatedUsers, $rights, $flags, 'None', 'Allow')))
    Assert-True (-not (_Test-RamDiskMachineAccess -Acl $script:acl)) "$case Authenticated Users rule was accepted"
    _Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
    Assert-True (_Test-RamDiskMachineAccess -Acl $script:acl) "$case Authenticated Users rule was not repaired"
}

Reset-Fixture
_Add-RamDiskMachineAccess -Acl $script:acl
$script:acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
    $authenticatedUsers, 'FullControl', $inherit, 'None', 'Allow')))
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
Assert-True ($script:writes -eq 0) 'Existing Authenticated Users FullControl was rewritten or downgraded'

foreach ($format in @('FAT32', 'exFAT')) {
    Reset-Fixture
    $script:filesystem = $format
    _Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
    Assert-True ($script:writes -eq 0) "$format was forced through NTFS ACL repair"
}
Reset-Fixture
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7 -ReadOnly $true
Assert-True ($script:writes -eq 0) 'Read-only RAM disk permissions were changed'

foreach ($case in @('occupied', 'changedDuringRead', 'correctAclChangedDuringRead', 'failedReadback')) {
    Reset-Fixture
    if ($case -eq 'occupied') { $script:owner = 99 }
    if ($case -eq 'changedDuringRead') { $script:changeOwnerOnRead = $true }
    if ($case -eq 'correctAclChangedDuringRead') {
        _Add-RamDiskMachineAccess -Acl $script:acl
        $script:changeOwnerOnRead = $true
    }
    if ($case -eq 'failedReadback') { $script:ignoreWrite = $true }
    $failed = $false
    try { _Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7 } catch { $failed = $true }
    Assert-True $failed "$case returned success"
    if ($case -ne 'failedReadback') {
        Assert-True ($script:writes -eq 0) "$case changed a different mount ACL"
    }
}

Write-Output 'RAM disk machine-wide write-access checks passed.'
