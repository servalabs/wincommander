[CmdletBinding()]
param([switch]$LibraryOnly, [switch]$ValidateCleanupTree, [string]$ExpectedMachineDataRoot)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-InstallerMachineDataRoot([string]$KnownFolderPath) {
    if ([string]::IsNullOrWhiteSpace($KnownFolderPath) -or $KnownFolderPath -match '[\r\n]' -or
        $KnownFolderPath -notmatch '^[A-Za-z]:\\') {
        throw 'A local Windows machine-data known folder is required.'
    }
    $path = [IO.Path]::GetFullPath($KnownFolderPath).TrimEnd('\')
    if ($path.Length -gt 900 -or $path -eq [IO.Path]::GetPathRoot($path).TrimEnd('\')) {
        throw 'The machine-data known folder is not a supported installer root.'
    }
    return $path
}

function Assert-InstallerMachineDataAgreement([string]$KnownFolderPath, [string]$ExpectedPath) {
    $actual = Resolve-InstallerMachineDataRoot $KnownFolderPath
    $expected = Resolve-InstallerMachineDataRoot $ExpectedPath
    if (-not [string]::Equals($actual, $expected, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'The installer and Windows machine-data locations disagree.'
    }
}

function Assert-InstallerEntryAttributes([IO.FileAttributes]$Attributes) {
    if ($Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'Installer machine-data paths cannot be reparse points.'
    }
}

function Assert-InstallerSecurityDescriptor($Descriptor, [switch]$Ancestor, [switch]$VolumeRoot) {
    $trusted = @('S-1-5-18', 'S-1-5-32-544',
        'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464')
    if ($Descriptor.GetOwner([Security.Principal.SecurityIdentifier]).Value -notin $trusted) {
        throw 'The installer data root must have a trusted Windows owner.'
    }
    $raw = [Security.AccessControl.RawSecurityDescriptor]::new($Descriptor.GetSecurityDescriptorBinaryForm(), 0)
    if ($null -eq $raw.DiscretionaryAcl) { throw 'A null data-root DACL is unsafe.' }
    $rights = [Security.AccessControl.FileSystemRights]
    $mask = [int64]($rights::DeleteSubdirectoriesAndFiles -bor $rights::ChangePermissions -bor $rights::TakeOwnership)
    # DELETE permits renaming/substituting a non-root ancestor even when its
    # parent denies DeleteChild. A volume root itself cannot be renamed this way.
    if (-not ($Ancestor -and $VolumeRoot)) { $mask = $mask -bor [int64]$rights::Delete }
    if (-not $Ancestor) { $mask = $mask -bor [int64]$rights::Write }
    $mask = $mask -bor 0x50000000 # Generic all/write access must not bypass the specific-rights check.
    foreach ($rule in $Descriptor.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or
            ($rule.PropagationFlags -band [Security.AccessControl.PropagationFlags]::InheritOnly)) { continue }
        if ($rule.IdentityReference.Value -notin $trusted -and ([int64]$rule.FileSystemRights -band $mask)) {
            throw 'The installer root can be substituted by an untrusted principal.'
        }
    }
}

function Get-InstallerDirectorySecurity([string]$Path) {
    $entry = [IO.DirectoryInfo]::new($Path)
    $sections = [Security.AccessControl.AccessControlSections]::Access -bor [Security.AccessControl.AccessControlSections]::Owner
    if ('System.IO.FileSystemAclExtensions' -as [type]) {
        return [IO.FileSystemAclExtensions]::GetAccessControl($entry, $sections)
    }
    return $entry.GetAccessControl($sections)
}

function Assert-InstallerRootPath([string]$Path, [switch]$Ancestor) {
    $entry = Get-Item -LiteralPath $Path -Force
    if (-not $entry.PSIsContainer) { throw 'Installer machine data requires directories.' }
    Assert-InstallerEntryAttributes $entry.Attributes
    $isVolumeRoot = [string]::Equals([IO.Path]::GetFullPath($Path).TrimEnd('\'),
        [IO.Path]::GetPathRoot($Path).TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    Assert-InstallerSecurityDescriptor (Get-InstallerDirectorySecurity $Path) -Ancestor:$Ancestor -VolumeRoot:$isVolumeRoot
}

function Assert-InstallerCleanupTree([string]$Root) {
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($Root)
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $count = 0
    while ($pending.Count -gt 0) {
        foreach ($entry in Get-ChildItem -LiteralPath $pending.Pop() -Force) {
            $count++
            if ($count -gt 100000 -or $timer.Elapsed.TotalSeconds -gt 30) {
                throw 'Machine-data inspection exceeded its safe bound; data is retained.'
            }
            Assert-InstallerEntryAttributes $entry.Attributes
            if ($entry.PSIsContainer) { $pending.Push($entry.FullName) }
        }
    }
}

if ($LibraryOnly) { return }
try {
    try { Add-Type -AssemblyName System.IO.FileSystem.AccessControl -ErrorAction Stop } catch { }
    $machineData = Resolve-InstallerMachineDataRoot ([Environment]::GetFolderPath('CommonApplicationData'))
    Assert-InstallerMachineDataAgreement $machineData $ExpectedMachineDataRoot
    $cursor = $machineData
    while ($cursor) {
        Assert-InstallerRootPath $cursor -Ancestor
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    $productRoot = Join-Path $machineData 'WinCommander'
    if (Test-Path -LiteralPath $productRoot) {
        Assert-InstallerRootPath $productRoot
        # Shared descendants can legitimately be writable. This inspection rejects
        # existing links, but does not retain identity across later NSIS cleanup.
        if ($ValidateCleanupTree) { Assert-InstallerCleanupTree $productRoot }
    }
    # Success is conveyed only by the exit code; paths never cross stdout encoding.
} catch {
    [Console]::Error.WriteLine('WinCommander machine-data validation failed; existing data was retained. Inspect the root owner, permissions and linked paths before retrying.')
    exit 1
}
