[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Repair-WcPinnedDriverAccess {
    param([Parameter(Mandatory)][string]$Path)

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Driver access repair requires administrator approval.'
    }
    $expectedHash = '1F0C6DB3559D1356C38A1486A967CD90DB5E6202E433FEA1DFE510DDB884FFB6'
    $cursor = [IO.Path]::GetFullPath($Path)
    while ($cursor) {
        $item = Get-Item -LiteralPath $cursor -Force
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.LinkType -eq 'HardLink') {
            throw 'Driver access repair refuses linked paths.'
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or $item.Length -ne 667840) { throw 'Unexpected driver payload size.' }
    $original = Get-Acl -LiteralPath $Path
    $owner = $original.GetOwner([Security.Principal.SecurityIdentifier]).Value
    if ($owner -notin @('S-1-5-18', 'S-1-5-32-544')) { throw 'Unexpected driver owner.' }
    $rules = @($original.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    # Also finish an interrupted repair's exact trusted-only intermediate descriptor.
    $trustedOnly = $rules.Count -eq 2
    foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
        $matching = @($rules | Where-Object { $_.IdentityReference.Value -eq $sid -and $_.AccessControlType -eq 'Allow' -and $_.FileSystemRights -eq 'FullControl' -and -not $_.IsInherited })
        $trustedOnly = $trustedOnly -and $matching.Count -eq 1
    }
    $repair = $original.AreAccessRulesProtected -and ($rules.Count -eq 0 -or $trustedOnly)
    $changed = $false
    try {
        if ($repair) {
            $acl = New-Object Security.AccessControl.FileSecurity
            $acl.SetAccessRuleProtection($true, $false)
            $acl.SetOwner($original.GetOwner([Security.Principal.SecurityIdentifier]))
            foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
                $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]$sid), 'FullControl', 'Allow'))
            }
            # Grant only trusted principals long enough to validate; roll back on any failure.
            Set-Acl -LiteralPath $Path -AclObject $acl
            $changed = $true
        }
        if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Driver hash does not match the pinned payload.' }
        if ((Get-AuthenticodeSignature -LiteralPath $Path).Status -ne 'Valid') { throw 'Driver signature is not valid.' }
        if ($repair) {
            # Standard-user Pro processes must be able to compare the embedded driver bytes.
            $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule ([Security.Principal.SecurityIdentifier]'S-1-5-32-545'), 'ReadAndExecute', 'Allow'))
            Set-Acl -LiteralPath $Path -AclObject $acl
        }
        $actual = Get-Acl -LiteralPath $Path
        $actualRules = @($actual.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
        $write = [Security.AccessControl.FileSystemRights]'WriteData,AppendData,WriteExtendedAttributes,WriteAttributes,Delete,ChangePermissions,TakeOwnership'
        foreach ($rule in $actualRules) {
            if ($rule.AccessControlType -eq 'Allow' -and ($rule.FileSystemRights -band $write) -ne 0 -and $rule.IdentityReference.Value -notin @('S-1-5-18', 'S-1-5-32-544')) {
                throw 'Driver permissions allow an untrusted writer; automatic repair refused.'
            }
        }
        if ($repair) {
            if (-not $actual.AreAccessRulesProtected -or $actualRules.Count -ne 3) { throw 'Driver access readback failed.' }
            foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
                $matching = @($actualRules | Where-Object { $_.IdentityReference.Value -eq $sid -and $_.AccessControlType -eq 'Allow' -and $_.FileSystemRights -eq 'FullControl' -and -not $_.IsInherited })
                if ($matching.Count -ne 1) { throw 'Driver access readback failed.' }
            }
            $readRights = [Security.AccessControl.FileSystemRights]::ReadAndExecute -bor [Security.AccessControl.FileSystemRights]::Synchronize
            $users = @($actualRules | Where-Object { $_.IdentityReference.Value -eq 'S-1-5-32-545' -and $_.AccessControlType -eq 'Allow' -and $_.FileSystemRights -eq $readRights -and -not $_.IsInherited })
            if ($users.Count -ne 1) { throw 'Standard-user driver readback failed.' }
        }
        [pscustomobject]@{ repaired = $repair; hashVerified = $true; signatureVerified = $true }
    }
    catch {
        if ($changed) { Set-Acl -LiteralPath $Path -AclObject $original }
        throw
    }
}

# Dot sourcing exposes the same implementation to isolated Windows fixture tests.
if ($MyInvocation.InvocationName -ne '.') {
    Repair-WcPinnedDriverAccess -Path 'C:\ProgramData\WinCommander\bin\engine\EncVolKm.sys' | ConvertTo-Json -Compress
}
