[CmdletBinding()]
param([Parameter(Mandatory)][string]$DataRoot)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# PowerShell 7 ships filesystem ACL extensions in a separate assembly that may
# not be loaded in a clean runner process. Windows PowerShell 5.1 uses the
# FileInfo/DirectoryInfo instance methods below instead.
try { Add-Type -AssemblyName System.IO.FileSystem.AccessControl -ErrorAction Stop } catch { }

# Installer-only repair. Never recurse through private service state, Vaults,
# user profiles, or linked directories, and never replace encrypted contents.
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Shared settings repair requires an elevated installer.'
    }
    $root = [IO.Path]::GetFullPath($DataRoot).TrimEnd('\')
    if ([IO.Path]::GetFileName($root) -ne 'WinCommander') {
        throw 'Expected a product-specific WinCommander data directory.'
    }
    function Assert-NoLinks([string]$Path) {
        $cursor = $Path
        while ($cursor) {
            if (Test-Path -LiteralPath $cursor) {
                $item = Get-Item -LiteralPath $cursor -Force
                if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (-not $item.PSIsContainer -and $item.LinkType -eq 'HardLink')) {
                    throw 'Shared settings repair refuses linked paths.'
                }
            }
            $cursor = [IO.Path]::GetDirectoryName($cursor)
        }
    }
    # Do not use Get-Acl or Set-Acl here.  NSIS runs this helper from its
    # temporary directory and some stripped-down Windows runner images cannot
    # load Microsoft.PowerShell.Security, even though PowerShell can discover
    # the cmdlet.  The .NET APIs are available in Windows PowerShell itself.
    function Get-EntrySecurity(
        [string]$Path,
        [bool]$Directory,
        [Security.AccessControl.AccessControlSections]$Sections = [Security.AccessControl.AccessControlSections]::Access
    ) {
        $entry = if ($Directory) { [IO.DirectoryInfo]::new($Path) } else { [IO.FileInfo]::new($Path) }
        $extensions = 'System.IO.FileSystemAclExtensions' -as [type]
        if ($extensions) {
            return [IO.FileSystemAclExtensions]::GetAccessControl($entry, $Sections)
        }
        return $entry.GetAccessControl($Sections)
    }
    function Set-EntrySecurity([string]$Path, [bool]$Directory, [Security.AccessControl.FileSystemSecurity]$Acl) {
        $entry = if ($Directory) { [IO.DirectoryInfo]::new($Path) } else { [IO.FileInfo]::new($Path) }
        $extensions = 'System.IO.FileSystemAclExtensions' -as [type]
        if ($Directory) {
            if ($extensions) { [IO.FileSystemAclExtensions]::SetAccessControl($entry, [Security.AccessControl.DirectorySecurity]$Acl) }
            else { $entry.SetAccessControl([Security.AccessControl.DirectorySecurity]$Acl) }
        }
        else {
            if ($extensions) { [IO.FileSystemAclExtensions]::SetAccessControl($entry, [Security.AccessControl.FileSecurity]$Acl) }
            else { $entry.SetAccessControl([Security.AccessControl.FileSecurity]$Acl) }
        }
    }
    function Repair-Entry([string]$Path, [bool]$Directory) {
        Assert-NoLinks $Path
        $item = Get-Item -LiteralPath $Path -Force
        if ($item.PSIsContainer -ne $Directory) { throw 'Unexpected shared settings entry type.' }
        $output = & "$env:SystemRoot\System32\takeown.exe" /F $Path /A 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Could not reclaim shared settings ownership: $output" }
        # Start from the existing descriptor and modify only its DACL. Creating
        # a blank descriptor and assigning its owner makes SetAccessControl
        # attempt a separate WRITE_OWNER operation, which can be denied even
        # after takeown has successfully transferred ownership to Administrators.
        $acl = Get-EntrySecurity $Path $Directory
        $acl.SetAccessRuleProtection($true, $false)
        foreach ($existingRule in @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))) {
            [void]$acl.RemoveAccessRuleSpecific($existingRule)
        }
        $inheritance = if ($Directory) { [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit' } else { [Security.AccessControl.InheritanceFlags]::None }
        foreach ($sid in @('S-1-5-18', 'S-1-5-32-544', 'S-1-5-32-545')) {
            $rights = if ($sid -eq 'S-1-5-32-545') { [Security.AccessControl.FileSystemRights]::ReadAndExecute } else { [Security.AccessControl.FileSystemRights]::FullControl }
            $rule = New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]$sid), $rights, $inheritance, ([Security.AccessControl.PropagationFlags]::None), ([Security.AccessControl.AccessControlType]::Allow)
            $acl.AddAccessRule($rule)
        }
        Set-EntrySecurity $Path $Directory $acl
        # Read the actual descriptor back, rather than trusting a /C exit code.
        $sections = [Security.AccessControl.AccessControlSections]::Access -bor [Security.AccessControl.AccessControlSections]::Owner
        $actual = Get-EntrySecurity $Path $Directory $sections
        if ($actual.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne 'S-1-5-32-544') {
            throw 'Shared settings ownership verification failed.'
        }
        # Windows may add the auto-inherited control flag when canonicalizing
        # a descriptor. Compare the effective rules, not that bookkeeping flag.
        $ruleKey = { '{0}:{1}:{2}:{3}:{4}:{5}' -f $_.IdentityReference.Value, [int]$_.FileSystemRights, $_.AccessControlType, $_.InheritanceFlags, $_.PropagationFlags, $_.IsInherited }
        $expectedRules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) | ForEach-Object $ruleKey | Sort-Object)
        $actualRules = @($actual.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) | ForEach-Object $ruleKey | Sort-Object)
        if (-not $actual.AreAccessRulesProtected -or ($expectedRules -join '|') -ne ($actualRules -join '|')) {
            throw 'Shared settings permission verification failed.'
        }
    }
    Assert-NoLinks $root
    if (-not (Test-Path -LiteralPath $root)) { New-Item -ItemType Directory -Path $root | Out-Null }
    Repair-Entry $root $true
    $store = Join-Path $root 'store'
    Assert-NoLinks $store
    if (-not (Test-Path -LiteralPath $store)) { New-Item -ItemType Directory -Path $store | Out-Null }
    Repair-Entry $store $true
    foreach ($relative in @('.install.material', 'store\settings.dat')) {
        $path = Join-Path $root $relative
        Assert-NoLinks $path
        if (Test-Path -LiteralPath $path) { Repair-Entry $path $false }
    }
    Write-Output 'Shared settings permissions verified; encrypted contents preserved.'
}
catch {
    Write-Error $_
    exit 1
}
