# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\..\src-tauri\commander-free\scripts\modules\vault\ramdisks.ps1"

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

$user = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
Assert-True (-not $user.IsWellKnown([System.Security.Principal.WellKnownSidType]::LocalSystemSid)) 'Run these fixture checks as a user, not SYSTEM'
$inherit = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
    [System.Security.AccessControl.InheritanceFlags]::ObjectInherit
$other = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-21-1-2-3-9999')

function Reset-Fixture {
    $script:acl = New-Object System.Security.AccessControl.DirectorySecurity
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
    $copy.SetSecurityDescriptorSddlForm($script:acl.Sddl)
    return $copy
}
function Set-Acl {
    param([string]$LiteralPath, $AclObject, $ErrorAction)
    Assert-True ($LiteralPath -eq 'R:\') 'ACL repair targeted a different path'
    $script:writes++
    if (-not $script:ignoreWrite) { $script:acl = $AclObject }
}

Reset-Fixture
Assert-True (-not (_Test-RamDiskUserModifyAccess -Acl $script:acl -User $user)) 'A missing user rule was accepted'
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
Assert-True (_Test-RamDiskUserModifyAccess -Acl $script:acl -User $user) 'User Modify permission was not inherited by files and folders'
Assert-True ($script:writes -eq 1) 'Repair did not write exactly once'
$rules = @($script:acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
Assert-True ($rules.Count -eq 2 -and @($rules | Where-Object { $_.IdentityReference -eq $other -and $_.AccessControlType -eq 'Deny' }).Count -eq 1) 'Repair broadened other users access or removed a deny rule'
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
Assert-True ($script:writes -eq 1) 'Reuse rewrote an already-correct ACL'

Reset-Fixture
$script:acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
    $user, 'FullControl', $inherit, 'None', 'Allow')))
_Ensure-RamDiskWritableAccess -Letter R -DeviceNumber 7
Assert-True ($script:writes -eq 0) 'Existing user FullControl was rewritten or downgraded'

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
        $script:acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
            $user, 'Modify', $inherit, 'None', 'Allow')))
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

Write-Output 'RAM disk user write-access checks passed.'
