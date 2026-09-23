[CmdletBinding()]
param([Parameter(Mandatory)][string]$FixtureParent)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try { Add-Type -AssemblyName System.IO.FileSystem.AccessControl -ErrorAction Stop } catch { }
$repair = Join-Path $PSScriptRoot '..\src-tauri\commander-free\nsis\repair-shared-settings.ps1'
$parent = [IO.Path]::GetFullPath($FixtureParent)
if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw 'Create an empty fixture parent first.' }
if (@(Get-ChildItem -LiteralPath $parent -Force).Count -ne 0) { throw 'Fixture parent must be empty.' }
$root = Join-Path $parent 'WinCommander'
$report = Join-Path $parent 'result.txt'
try {
    function Get-EntrySecurity(
        [string]$Path,
        [bool]$Directory,
        [Security.AccessControl.AccessControlSections]$Sections = [Security.AccessControl.AccessControlSections]::Access
    ) {
        $entry = if ($Directory) { [IO.DirectoryInfo]::new($Path) } else { [IO.FileInfo]::new($Path) }
        $extensions = 'System.IO.FileSystemAclExtensions' -as [type]
        if ($extensions) { return [IO.FileSystemAclExtensions]::GetAccessControl($entry, $Sections) }
        return $entry.GetAccessControl($Sections)
    }
    function Get-EntrySddl([string]$Path, [bool]$Directory) {
        $sections = [Security.AccessControl.AccessControlSections]::Access -bor [Security.AccessControl.AccessControlSections]::Owner -bor [Security.AccessControl.AccessControlSections]::Group
        $security = Get-EntrySecurity $Path $Directory $sections
        return $security.GetSecurityDescriptorSddlForm($sections)
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
    if ((Get-Content -LiteralPath $repair -Raw) -match '(?im)^\s*(Get|Set)-Acl\b') {
        throw 'Installer repair must not depend on Microsoft.PowerShell.Security ACL cmdlets.'
    }
    function Repair {
        $ErrorActionPreference = 'Continue'
        $output = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $repair -DataRoot $root 2>&1 | Out-String
        $failed = $LASTEXITCODE -ne 0
        $ErrorActionPreference = 'Stop'
        if ($failed) { throw "Repair helper failed: $output" }
    }
    Repair
    if (-not (Test-Path -LiteralPath (Join-Path $root 'store') -PathType Container)) { throw 'Fresh store not prepared.' }
    $settings = Join-Path $root 'store\settings.dat'
    $material = Join-Path $root '.install.material'
    # Deliberately not real secrets. Check byte preservation, not just exit status.
    [IO.File]::WriteAllText($settings, 'fixture-encrypted-settings')
    [IO.File]::WriteAllText($material, 'fixture-protected-material')
    $private = Join-Path $root 'private-service-state'
    New-Item -ItemType Directory -Path $private | Out-Null
    $privateAcl = New-Object Security.AccessControl.DirectorySecurity
    $privateAcl.SetAccessRuleProtection($true, $false)
    $privateAcl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]'S-1-5-32-544'), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    Set-EntrySecurity $private $true $privateAcl
    $privateBefore = Get-EntrySddl $private $true
    foreach ($path in @($settings, $material)) {
        $acl = Get-EntrySecurity $path $false
        $acl.SetAccessRuleProtection($true, $false)
        $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]'S-1-5-32-545'), 'ReadData', 'Deny'))
        Set-EntrySecurity $path $false $acl
    }
    Repair
    Repair
    if ([IO.File]::ReadAllText($settings) -ne 'fixture-encrypted-settings') { throw 'Settings contents changed.' }
    if ([IO.File]::ReadAllText($material) -ne 'fixture-protected-material') { throw 'Material contents changed.' }
    if ((Get-EntrySddl $private $true) -ne $privateBefore) { throw 'Unrelated protected state permissions changed.' }
    foreach ($path in @($settings, $material)) {
        $rules = @((Get-EntrySecurity $path $false).GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
        if ($rules.Count -ne 3) { throw 'Unexpected residual access rules.' }
        $users = @($rules | Where-Object { $_.IdentityReference.Value -eq 'S-1-5-32-545' })
        $readOnlyRights = [Security.AccessControl.FileSystemRights]::ReadAndExecute -bor [Security.AccessControl.FileSystemRights]::Synchronize
        if ($users.Count -ne 1 -or $users[0].FileSystemRights -ne $readOnlyRights -or $users[0].AccessControlType -ne 'Allow') { throw 'Users access is not read-only.' }
    }
    'PASS: clean store, legacy denied settings, read-only Users ACL, idempotence, byte preservation, protected sibling preservation.' | Set-Content -LiteralPath $report
}
catch {
    "FAIL: $($_.Exception.Message)" | Set-Content -LiteralPath $report
    throw
}
