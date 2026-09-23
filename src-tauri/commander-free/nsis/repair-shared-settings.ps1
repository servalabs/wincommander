[CmdletBinding()]
param([Parameter(Mandatory)][string]$DataRoot)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

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
    function Get-EntrySecurity([string]$Path, [bool]$Directory) {
        if ($Directory) {
            return [IO.Directory]::GetAccessControl($Path)
        }
        return [IO.File]::GetAccessControl($Path)
    }
    function Set-EntrySecurity([string]$Path, [bool]$Directory, [Security.AccessControl.FileSystemSecurity]$Acl) {
        if ($Directory) {
            [IO.Directory]::SetAccessControl($Path, [Security.AccessControl.DirectorySecurity]$Acl)
        }
        else {
            [IO.File]::SetAccessControl($Path, [Security.AccessControl.FileSecurity]$Acl)
        }
    }
    function Repair-Entry([string]$Path, [bool]$Directory) {
        Assert-NoLinks $Path
        $item = Get-Item -LiteralPath $Path -Force
        if ($item.PSIsContainer -ne $Directory) { throw 'Unexpected shared settings entry type.' }
        $output = & "$env:SystemRoot\System32\takeown.exe" /F $Path /A 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Could not reclaim shared settings ownership: $output" }
        $acl = if ($Directory) { New-Object Security.AccessControl.DirectorySecurity } else { New-Object Security.AccessControl.FileSecurity }
        $acl.SetAccessRuleProtection($true, $false)
        $acl.SetOwner([Security.Principal.SecurityIdentifier]'S-1-5-32-544')
        $inheritance = if ($Directory) { [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit' } else { [Security.AccessControl.InheritanceFlags]::None }
        foreach ($sid in @('S-1-5-18', 'S-1-5-32-544', 'S-1-5-32-545')) {
            $rights = if ($sid -eq 'S-1-5-32-545') { [Security.AccessControl.FileSystemRights]::ReadAndExecute } else { [Security.AccessControl.FileSystemRights]::FullControl }
            $rule = New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]$sid), $rights, $inheritance, ([Security.AccessControl.PropagationFlags]::None), ([Security.AccessControl.AccessControlType]::Allow)
            $acl.AddAccessRule($rule)
        }
        Set-EntrySecurity $Path $Directory $acl
        # Read the actual descriptor back, rather than trusting a /C exit code.
        $actual = Get-EntrySecurity $Path $Directory
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
