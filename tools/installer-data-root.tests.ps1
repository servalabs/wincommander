param()
$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/installer-data-root.ps1'
. $helper -LibraryOnly
$count = 0
function Assert-Rejected([scriptblock]$Action) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (-not $rejected) { throw 'Expected unsafe installer state to be rejected.' }
    $script:count++
}
function New-Descriptor([string]$Owner = 'S-1-5-32-544') {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetOwner([Security.Principal.SecurityIdentifier]::new($Owner))
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($sid), 'FullControl', 'Allow'))
    }
    return $acl
}
foreach ($value in @('', 'relative', 'C:\', '\\example.invalid\share', "C:\data`nother")) {
    Assert-Rejected { Resolve-InstallerMachineDataRoot $value }
}
if ((Resolve-InstallerMachineDataRoot 'C:\ProgramData\') -cne 'C:\ProgramData') { throw 'Root normalization failed.' }
$count++
Assert-InstallerMachineDataAgreement 'C:\Données\共享' 'c:\Données\共享\'
$count++
Assert-Rejected { Assert-InstallerMachineDataAgreement 'C:\ProgramData' 'C:\untrusted' }
$acl = New-Descriptor
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'), 'ReadAndExecute', 'Allow'))
Assert-InstallerSecurityDescriptor $acl
$count++
$writable = New-Descriptor
$writable.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'), 'Modify', 'Allow'))
Assert-Rejected { Assert-InstallerSecurityDescriptor $writable }
Assert-Rejected { Assert-InstallerSecurityDescriptor (New-Descriptor 'S-1-5-21-1-2-3-1001') }
$parent = New-Descriptor
$parent.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-11'), 'CreateDirectories', 'Allow'))
Assert-InstallerSecurityDescriptor $parent -Ancestor
$count++
Assert-Rejected { Assert-InstallerSecurityDescriptor $parent }
$replaceableParent = New-Descriptor
$replaceableParent.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-11'), 'DeleteSubdirectoriesAndFiles', 'Allow'))
Assert-Rejected { Assert-InstallerSecurityDescriptor $replaceableParent -Ancestor }
$deletableAncestor = New-Descriptor
$deletableAncestor.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-11'), 'Delete', 'Allow'))
Assert-Rejected { Assert-InstallerSecurityDescriptor $deletableAncestor -Ancestor }
Assert-InstallerSecurityDescriptor $deletableAncestor -Ancestor -VolumeRoot
$count++ # DELETE alone cannot substitute a Windows volume root.
Assert-Rejected { Assert-InstallerSecurityDescriptor $replaceableParent -Ancestor -VolumeRoot }


Assert-Rejected { Assert-InstallerEntryAttributes ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint) }
Assert-InstallerEntryAttributes ([IO.FileAttributes]::Directory)
$count++
$nullDacl = [Security.AccessControl.DirectorySecurity]::new()
$nullDacl.SetSecurityDescriptorSddlForm('O:BAG:BAD:NO_ACCESS_CONTROL')
Assert-Rejected { Assert-InstallerSecurityDescriptor $nullDacl }
$script:tree = @{
    'C:\ProgramData\WinCommander' = @([pscustomobject]@{
        FullName = 'C:\ProgramData\WinCommander\store'; PSIsContainer = $true
        Attributes = [IO.FileAttributes]::Directory; SecurityDescriptor = $writable
    })
    'C:\ProgramData\WinCommander\store' = @([pscustomobject]@{
        FullName = 'C:\ProgramData\WinCommander\store\settings.dat'; PSIsContainer = $false
        Attributes = [IO.FileAttributes]::Archive; SecurityDescriptor = $writable
    })
}
function Get-ChildItem([string]$LiteralPath, [switch]$Force) {
    if (-not $script:tree.ContainsKey($LiteralPath)) { throw 'Unexpected traversal outside the fixture.' }
    return $script:tree[$LiteralPath]
}
try {
    Assert-InstallerSecurityDescriptor $acl
    Assert-InstallerCleanupTree 'C:\ProgramData\WinCommander'
    $count++ # Writable shared descendants do not invalidate an approved protected root.
    $script:tree['C:\ProgramData\WinCommander'][0].Attributes = [IO.FileAttributes]::ReparsePoint
    Assert-Rejected { Assert-InstallerCleanupTree 'C:\ProgramData\WinCommander' }
} finally {
    Remove-Item -LiteralPath Function:\Get-ChildItem
}
Write-Output "Installer data-root policy: $count checks passed; only in-memory paths/descriptors/attributes exercised."
