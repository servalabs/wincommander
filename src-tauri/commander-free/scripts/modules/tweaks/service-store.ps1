function Get-ServiceSafetyRoot {
    Assert-IsAdmin
    $root = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'WinCommanderServiceHistory'
    Initialize-ServiceSafetyRoot $root
    return $root
}

function Initialize-ServiceSafetyRoot {
    param([Parameter(Mandatory)][string]$Root)
    $ErrorActionPreference = 'Stop'
    $trusted = @('S-1-5-18', 'S-1-5-32-544', [Security.Principal.WindowsIdentity]::GetCurrent().User.Value)
    if (-not [IO.Directory]::Exists($root)) {
        [void][IO.Directory]::CreateDirectory($root)
        $owner = (Get-Acl -LiteralPath $root -ErrorAction Stop).GetOwner([Security.Principal.SecurityIdentifier]).Value
        if ($owner -notin $trusted) { throw 'Service history folder has an unexpected owner.' }
        $acl = New-Object Security.AccessControl.DirectorySecurity
        $acl.SetAccessRuleProtection($true, $false)
        $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
        foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
            $rule = [Security.AccessControl.FileSystemAccessRule]::new(
                [Security.Principal.SecurityIdentifier]::new($sid), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
            [void]$acl.AddAccessRule($rule)
        }
        Set-Acl -LiteralPath $root -AclObject $acl -ErrorAction Stop
    }
    Assert-ServiceSafetyFile $root
}

function Assert-ServiceSafetyFile {
    param([Parameter(Mandatory)][string]$Path)
    $ErrorActionPreference = 'Stop'
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Service history cannot use links or redirected folders.' }
    $acl = Get-Acl -LiteralPath $Path -ErrorAction Stop
    $trusted = @('S-1-5-18', 'S-1-5-32-544')
    if ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -notin $trusted) { throw 'Service history has an unexpected owner.' }
    $writeRights = [Security.AccessControl.FileSystemRights]'Write,Delete,DeleteSubdirectoriesAndFiles,ChangePermissions,TakeOwnership'
    foreach ($rule in $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -eq 'Allow' -and ($rule.FileSystemRights -band $writeRights) -and $rule.IdentityReference.Value -notin $trusted) {
            throw 'Service history permissions allow changes by another account.'
        }
    }
}

function Get-ServiceSafetyPath {
    param([string]$Id, [ValidateSet('plan', 'snapshot')][string]$Kind)
    if ($Id -cnotmatch '^[a-f0-9]{32}$') { throw 'Invalid service history identifier.' }
    return Join-Path (Get-ServiceSafetyRoot) "$Id.$Kind.json"
}

function Write-ServiceSafetyDocument {
    param([string]$Id, [ValidateSet('plan', 'snapshot')][string]$Kind, $Document)
    $ErrorActionPreference = 'Stop'
    $path = Get-ServiceSafetyPath $Id $Kind
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Document | ConvertTo-Json -Depth 15))
    $temporary = "$path.writing"
    $created = $false
    try {
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $created = $true
        try {
            Set-ServiceSafetyOwner $temporary
            $stream.Write($bytes, 0, $bytes.Length); $stream.Flush()
        } finally { $stream.Dispose() }
        [IO.File]::Move($temporary, $path)
    } finally {
        if ($created -and [IO.File]::Exists($temporary)) { Remove-Item -LiteralPath $temporary -ErrorAction SilentlyContinue }
    }
}

function Read-ServiceSafetyDocument {
    param([string]$Id, [ValidateSet('plan', 'snapshot')][string]$Kind)
    $ErrorActionPreference = 'Stop'
    $path = Get-ServiceSafetyPath $Id $Kind
    Assert-ServiceSafetyFile $path
    if ((Get-Item -LiteralPath $path).Length -gt 8MB) { throw 'Service history file is too large.' }
    $document = Get-Content -LiteralPath $path -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    if ($document.schemaVersion -ne 1 -or $document.id -cne $Id -or $document.machine -cne (Get-ServiceSafetyMachine)) {
        throw 'Service history does not belong to this PC or uses an unsupported format.'
    }
    return $document
}

function Set-ServiceSafetyOwner {
    param([string]$Path)
    $ErrorActionPreference = 'Stop'
    # The unelevated half of the same Windows account must not own an approval file.
    $acl = Get-Acl -LiteralPath $Path -ErrorAction Stop
    $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    Set-Acl -LiteralPath $Path -AclObject $acl -ErrorAction Stop
    Assert-ServiceSafetyFile $Path
}

function Get-ServiceSafetyMachine {
    $machine = [string][Microsoft.Win32.Registry]::GetValue('HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Cryptography', 'MachineGuid', $null)
    if (-not $machine) { throw 'This PC could not be identified. Service history is unavailable.' }
    return $machine
}

function Enter-ServiceSafetyLock {
    $ErrorActionPreference = 'Stop'
    $path = Join-Path (Get-ServiceSafetyRoot) 'operation.lock'
    if (Test-Path -LiteralPath $path) { Assert-ServiceSafetyFile $path }
    try { $stream = [IO.File]::Open($path, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'Another service change is in progress. Try again after it finishes.' }
    try { Set-ServiceSafetyOwner $path; return $stream } catch { $stream.Dispose(); throw }
}
