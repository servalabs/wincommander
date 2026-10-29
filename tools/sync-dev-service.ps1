[CmdletBinding()]
param(
    # Internal only: this copy runs after the UAC prompt and is allowed to
    # stop/reconfigure the machine service.
    [switch]$Elevated,
    # The non-elevated parent has already built the current binary.  Avoid a
    # second Cargo invocation after UAC elevation, where developer PATH/tooling
    # differences could otherwise stop the old service and strand it.
    [switch]$UseExistingBuild,
    [switch]$SyncPro,
    [switch]$StageFleetDebug,
    # Replacing the SYSTEM service cleanly dismounts every Vault it owns. An
    # explicit opt-in keeps an ordinary frontend restart from unexpectedly
    # closing a mounted Vault during development.
    [switch]$AllowVaultDismount,
    [string]$ExpectedServiceSha256,
    [string]$ExpectedFreeSha256,
    [string]$ExpectedProSha256,
    [string]$DiagnosticPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repoRoot 'src-tauri'
$builtService = Join-Path $tauriRoot 'target\debug\wincommander-svc.exe'
$stagingDirectory = Join-Path $env:ProgramData 'WinCommander\dev-service'
$stagedService = Join-Path $stagingDirectory 'wincommander-svc.exe'
$fleetStaging = Join-Path $env:ProgramData 'WinCommander\dev-fleet'
$builtFree = Join-Path $tauriRoot 'target\debug\wincommander-free.exe'
$serviceName = 'WinCommanderSvc'
$builtPro = Join-Path $tauriRoot 'target\debug\wincommander-pro.exe'
$managedPro = 'C:\ProgramData\WinCommander\bin\wincommander-pro.dev.exe'
$driverServiceName = 'WinCommanderEncVol'
$driverPath = Join-Path $env:ProgramData 'WinCommander\bin\engine\EncVolKm.sys'
$driverNtPath = '\??\C:\ProgramData\WinCommander\bin\engine\EncVolKm.sys'
$driverSha256 = '1F0C6DB3559D1356C38A1486A967CD90DB5E6202E433FEA1DFE510DDB884FFB6'
$driverAccessRepair = Join-Path $PSScriptRoot 'repair-vault-driver-access.ps1'
$activeMountRegistry = Join-Path $env:ProgramData 'WinCommander\policy\vault-active-mounts-v1.json'

function Assert-NoActiveVaultsUnlessAllowed {
    if ($AllowVaultDismount -or -not (Test-Path -LiteralPath $activeMountRegistry -PathType Leaf)) {
        return
    }
    try {
        $registry = Get-Content -LiteralPath $activeMountRegistry -Raw | ConvertFrom-Json -ErrorAction Stop
        $mounts = @($registry.mounts.psobject.Properties | ForEach-Object { $_.Value })
    }
    catch {
        throw "WinCommander cannot verify whether a Vault is mounted. Stop the service only after resolving the Vault state, or rerun the explicit development synchronization with -AllowVaultDismount."
    }
    if ($mounts.Count -gt 0) {
        $letters = ($mounts | ForEach-Object { $_.drive_letter } | Where-Object { $_ } | Sort-Object -Unique) -join ', '
        throw "A Vault is mounted ($letters). Replacing the development service would dismount it. Dismount it in WinCommander first, or rerun this explicit test operation with -AllowVaultDismount."
    }
}

function Write-Diagnostic([string]$Message) {
    if ($DiagnosticPath) {
        Add-Content -LiteralPath $DiagnosticPath -Value $Message
    }
}

trap {
    Write-Host ("Development service synchronization failed: {0}" -f $_) -ForegroundColor Red
    if ($DiagnosticPath) {
        Add-Content -LiteralPath $DiagnosticPath -Value ($_ | Out-String)
        Add-Content -LiteralPath $DiagnosticPath -Value '--- WinCommanderSvc configuration ---'
        & sc.exe qc $serviceName 2>&1 | Add-Content -LiteralPath $DiagnosticPath
        Add-Content -LiteralPath $DiagnosticPath -Value '--- WinCommanderSvc status ---'
        & sc.exe queryex $serviceName 2>&1 | Add-Content -LiteralPath $DiagnosticPath
    }
    exit 1
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-Sha256([string]$Path) {
    # `Get-FileHash` lives in a PowerShell module that may be unavailable to
    # a PATH-shadowed development shell. Use the .NET implementation directly
    # so both the parent and the elevated sync process verify the same bytes.
    $hasher = [System.Security.Cryptography.SHA256]::Create()
    $stream = $null
    try {
        $stream = [System.IO.File]::OpenRead($Path)
        return -join ($hasher.ComputeHash($stream) | ForEach-Object { $_.ToString('X2') })
    }
    finally {
        if ($null -ne $stream) { $stream.Dispose() }
        $hasher.Dispose()
    }
}

function Assert-NoReparsePoint([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path -Force
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "A protected development path is a reparse point: $Path"
        }
    }
}

function Assert-ProtectedPath([string]$Path) {
    Assert-NoReparsePoint $Path
    $acl = Get-Acl -LiteralPath $Path
    $owner = ([Security.Principal.NTAccount]::new($acl.Owner)).Translate([Security.Principal.SecurityIdentifier]).Value
    if ($owner -notin @('S-1-5-18', 'S-1-5-32-544')) { throw "Development path has an untrusted owner: $Path" }
    $writeMask = [int64]([Security.AccessControl.FileSystemRights]::WriteData -bor [Security.AccessControl.FileSystemRights]::AppendData -bor [Security.AccessControl.FileSystemRights]::WriteExtendedAttributes -bor [Security.AccessControl.FileSystemRights]::WriteAttributes -bor [Security.AccessControl.FileSystemRights]::Delete -bor [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles -bor [Security.AccessControl.FileSystemRights]::ChangePermissions -bor [Security.AccessControl.FileSystemRights]::TakeOwnership) -bor 0x50000000
    foreach ($rule in $acl.Access) {
        if ($rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow) { continue }
        $sid = $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
        if ($sid -notin @('S-1-5-18', 'S-1-5-32-544') -and (([int64]$rule.FileSystemRights -band $writeMask) -ne 0)) {
            throw "Development path allows an untrusted writer: $Path"
        }
    }
}

function Ensure-ProtectedDevelopmentDirectory([string]$Path) {
    $parent = Split-Path -Parent $Path
    Assert-ProtectedPath $parent
    if (Test-Path -LiteralPath $Path) {
        Assert-ProtectedPath $Path
        return
    }
    New-Item -ItemType Directory -Path $Path | Out-Null
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    $inherit = [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit
    foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid), 'FullControl', $inherit, 'None', 'Allow'))
    }
    $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'), 'ReadAndExecute', $inherit, 'None', 'Allow'))
    Set-Acl -LiteralPath $Path -AclObject $acl
    Assert-ProtectedPath $Path
}

function Invoke-SafeServiceRecovery([bool]$HadPriorService, [bool]$WasRunning, [string]$VerifiedBackupHash, [string]$CurrentStatus, [scriptblock]$Restore) {
    if (-not $HadPriorService -or -not $VerifiedBackupHash -or $CurrentStatus -ne 'Stopped') { return $false }
    & $Restore $WasRunning
    return $true
}

function Get-PriorRecoveryPlan([bool]$HadService, [string]$PriorStatus, [bool]$HasBackup, [bool]$SameTarget, [bool]$ConfigRecoverable) {
    if (-not $HadService) { return 'none' }
    if (-not $ConfigRecoverable -or $PriorStatus -notin @('Running', 'Stopped')) { return 'abort' }
    if ($HasBackup) { return 'full' }
    if ($PriorStatus -eq 'Stopped' -and -not $SameTarget) { return 'config-only' }
    return 'abort'
}

function Test-SimpleServiceImagePath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or $Path.IndexOfAny([char[]]@('"', '%', "`r", "`n")) -ge 0) { return $false }
    if (-not [IO.Path]::IsPathRooted($Path) -or -not $Path.EndsWith('.exe', [StringComparison]::OrdinalIgnoreCase)) { return $false }
    try { return [IO.File]::Exists($Path) -and [IO.Path]::GetFullPath($Path).Equals($Path, [StringComparison]::OrdinalIgnoreCase) } catch { return $false }
}

function Get-VerifiedDriverHash {
    try {
        return Get-Sha256 $driverPath
    }
    catch [System.UnauthorizedAccessException] {
        # A prior installer or interrupted update can leave the pinned driver
        # readable only by SYSTEM. This elevated sync is the safe place to
        # repair that known, signed payload before any service starts it.
        Write-Diagnostic 'Repairing trusted encrypted-volume driver read access.'
        . $driverAccessRepair
        $repair = Repair-WcPinnedDriverAccess -Path $driverPath
        if (-not $repair.hashVerified -or -not $repair.signatureVerified) {
            throw 'Encrypted-volume driver access repair did not verify the pinned signed payload.'
        }
        return Get-Sha256 $driverPath
    }
}

function Test-DevelopmentProCurrent {
    if (-not (Test-Path -LiteralPath $builtPro -PathType Leaf) -or -not (Test-Path -LiteralPath $managedPro -PathType Leaf)) { return $false }
    try {
        Assert-ProtectedPath (Split-Path -Parent $managedPro)
        Assert-ProtectedPath $managedPro
        return (Get-Sha256 $builtPro) -eq (Get-Sha256 $managedPro)
    } catch { return $false }
}

function Test-ServiceProcessCurrent([switch]$AllowUnverifiable) {
    try {
        $serviceProcess = Get-CimInstance Win32_Service -Filter "Name='WinCommanderSvc'" -ErrorAction Stop
        if ($null -eq $serviceProcess -or $serviceProcess.ProcessId -le 0) { return $false }
        $runningProcess = Get-CimInstance Win32_Process -Filter "ProcessId=$($serviceProcess.ProcessId)" -ErrorAction Stop
        if ($null -eq $runningProcess) { return $false }
        if ([string]::IsNullOrWhiteSpace($runningProcess.ExecutablePath)) { return [bool]$AllowUnverifiable }
        return $runningProcess.ExecutablePath.Equals($stagedService, [StringComparison]::OrdinalIgnoreCase)
    } catch { return [bool]$AllowUnverifiable }
}

function Test-ServiceAcceptingConnections([int]$Attempts = 3) {
    for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
        # Connect only: send no Hello, request or credentials to an unverified peer.
        $pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
            '.', 'wincmd-svc', [System.IO.Pipes.PipeDirection]::InOut,
            [System.IO.Pipes.PipeOptions]::Asynchronous,
            [Security.Principal.TokenImpersonationLevel]::Anonymous)
        try {
            $pipe.Connect(1000)
            return $true
        }
        catch [System.TimeoutException] { }
        catch [System.IO.IOException] { }
        catch [System.UnauthorizedAccessException] { }
        finally { $pipe.Dispose() }
        if ($attempt + 1 -lt $Attempts) { Start-Sleep -Milliseconds 250 }
    }
    Write-Diagnostic 'The development service pipe is not accepting connections.'
    return $false
}

function Get-ServiceImagePath {
    $line = @(& sc.exe qc $serviceName 2>$null | Where-Object { $_ -match 'BINARY_PATH_NAME' }) |
        Select-Object -First 1
    if (-not $line) { return $null }
    return (($line -replace '^.*BINARY_PATH_NAME\s*:\s*', '').Trim().Trim('"').Trim('\'))
}

function Get-DriverImagePath {
    $registryPath = "HKLM:\SYSTEM\CurrentControlSet\Services\$driverServiceName"
    try {
        return (Get-ItemProperty -LiteralPath $registryPath -Name ImagePath -ErrorAction Stop).ImagePath
    }
    catch {
        return $null
    }
}

function Test-CompatibleVeraCryptDriverReady {
    # VeraCrypt and WinCommander's signed EncVol driver intentionally expose
    # the same machine-wide control device. If the official VeraCrypt driver
    # already owns it, starting a duplicate service fails with error 87.
    return (@(& sc.exe query VeraCrypt 2>$null) -match 'STATE\s*:\s*4\s+RUNNING').Count -gt 0
}

function Assert-StagedDevelopmentService {
    Assert-ProtectedPath (Split-Path -Parent $stagingDirectory)
    Assert-ProtectedPath $stagingDirectory
    Assert-ProtectedPath $stagedService
    $configuredPath = Get-ServiceImagePath
    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    $serviceState = if ($service) { $service.Status } else { 'Missing' }
    if (-not $configuredPath -or
        -not $configuredPath.Equals($stagedService, [StringComparison]::OrdinalIgnoreCase) -or
        # The elevated child verifies the SYSTEM process path strictly. Its
        # successful parent may lack permission to read that same process path.
        $serviceState -ne 'Running' -or -not (Test-ServiceProcessCurrent -AllowUnverifiable:(-not $Elevated))) {
        throw "WinCommander development-service synchronization did not take effect. Expected running service path: $stagedService. Actual path: $configuredPath. Actual state: $serviceState."
    }
    if ((Get-Sha256 $builtService) -ne (Get-Sha256 $stagedService)) {
        throw 'The staged development service no longer matches the current build.'
    }
    if ($SyncPro -and -not (Test-DevelopmentProCurrent)) {
        throw 'The service Pro helper does not match the current development build.'
    }
    if (-not (Test-ServiceAcceptingConnections -Attempts 24)) {
        throw 'The development service is running but its pipe is unavailable. Startup readiness was not verified; no preference save was attempted.'
    }
}

function Ensure-EncryptedVolumeDriver {
    if (Test-CompatibleVeraCryptDriverReady) {
        Write-Diagnostic 'Using the already-running compatible VeraCrypt driver.'
        return
    }
    # The sidecar extracts this Microsoft-signed, pinned payload into the
    # fixed ProgramData location. A fresh checkout may not have reached that
    # extraction step yet; leave its first mount to the service bootstrap.
    if (-not (Test-Path -LiteralPath $driverPath -PathType Leaf)) {
        Write-Diagnostic "Encrypted-volume driver payload is not present yet: $driverPath"
        return
    }
    if ((Get-VerifiedDriverHash) -ne $driverSha256) {
        throw "The encrypted-volume driver does not match WinCommander's pinned payload: $driverPath"
    }

    & sc.exe qc $driverServiceName 2>$null
    $driverExists = $LASTEXITCODE -eq 0
    if ($driverExists) {
        Write-Diagnostic "Configuring the fixed encrypted-volume driver service."
        & sc.exe config $driverServiceName 'type=' 'kernel' 'start=' 'system' 'binPath=' $driverNtPath
    }
    else {
        Write-Diagnostic "Creating the fixed encrypted-volume driver service."
        & sc.exe create $driverServiceName 'type=' 'kernel' 'start=' 'system' 'binPath=' $driverNtPath
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Windows could not configure $driverServiceName."
    }

    # sc.exe accepts its own command-line grammar.  Some older development
    # synchronizers passed an escaped path through PowerShell and persisted
    # two leading slashes (\\??\\...) instead of the NT driver path
    # (\??\...). Windows reports that malformed legacy configuration as
    # ERROR_INVALID_PARAMETER (87) when starting the driver. Write the owned
    # ImagePath value directly and verify it before attempting the start.
    $driverRegistryPath = "HKLM\SYSTEM\CurrentControlSet\Services\$driverServiceName"
    & reg.exe add $driverRegistryPath '/v' 'ImagePath' '/t' 'REG_EXPAND_SZ' '/d' $driverNtPath '/f'
    if ($LASTEXITCODE -ne 0) {
        throw "Windows could not set the fixed driver path for $driverServiceName."
    }
    $configuredDriverPath = Get-DriverImagePath
    if ($configuredDriverPath -cne $driverNtPath) {
        throw "$driverServiceName did not retain the fixed driver path. Expected: $driverNtPath. Actual: $configuredDriverPath"
    }

    & sc.exe start $driverServiceName 2>$null
    if ($LASTEXITCODE -notin @(0, 1056)) {
        throw "Windows could not start $driverServiceName (exit code $LASTEXITCODE)."
    }
    $driverConfig = @(& sc.exe qc $driverServiceName 2>&1)
    $driverRunning = @(& sc.exe query $driverServiceName 2>&1) -match 'STATE\s*:\s*4\s+RUNNING'
    if (-not ($driverConfig -match [regex]::Escape($driverNtPath)) -or -not $driverRunning) {
        throw "$driverServiceName did not retain the fixed driver path or running state."
    }
    Write-Diagnostic 'Encrypted-volume driver service is running.'
}

function Test-EncryptedVolumeDriverReady {
    # There is nothing to repair until Pro has extracted its fixed payload.
    # Once it exists, however, a missing/stopped/misdirected service must force
    # the elevated sync rather than letting an otherwise-current broker return
    # early and leave the next mount to fail.
    if (Test-CompatibleVeraCryptDriverReady) {
        return $true
    }
    if (-not (Test-Path -LiteralPath $driverPath -PathType Leaf)) {
        return $true
    }
    try {
        if ((Get-Sha256 $driverPath) -ne $driverSha256) {
            return $false
        }
    }
    catch {
        # A non-elevated parent cannot repair an installed driver ACL. Ask the
        # elevated child to do the verified repair rather than failing startup.
        return $false
    }
    $config = @(& sc.exe qc $driverServiceName 2>$null)
    if ($LASTEXITCODE -ne 0 -or
        (Get-DriverImagePath) -cne $driverNtPath -or
        -not ($config -match [regex]::Escape($driverNtPath))) {
        return $false
    }
    return (@(& sc.exe query $driverServiceName 2>$null) -match 'STATE\s*:\s*4\s+RUNNING').Count -gt 0
}

function Build-Service {
    Push-Location $tauriRoot
    try {
        & cargo build -p commander-svc
        if ($LASTEXITCODE -ne 0) { throw "commander-svc build failed with exit code $LASTEXITCODE." }
    }
    finally {
        Pop-Location
    }
    if (-not (Test-Path -LiteralPath $builtService -PathType Leaf)) {
        throw "The development service build did not produce $builtService."
    }
}

function Start-ElevatedSync([switch]$UseExistingBuild) {
    $diagnostic = Join-Path $env:TEMP ("wincommander-dev-service-{0}.log" -f [guid]::NewGuid().ToString('N'))
    Set-Content -LiteralPath $diagnostic -Value 'Starting elevated WinCommander development-service synchronization.'
    # Start-Process does not preserve a -File value containing spaces when it
    # receives an argument array. Encode the tiny trampoline instead, so a
    # checkout such as E:\E drive\Company\wincommander reliably starts this
    # exact script in the elevated process.
    $escapedScriptPath = $PSCommandPath.Replace("'", "''")
    $escapedDiagnostic = $diagnostic.Replace("'", "''")
    # A PowerShell script can fail with a terminating error without changing
    # LASTEXITCODE (Cargo may have left it at zero). Propagate PowerShell's
    # success flag instead, otherwise the parent would report success after
    # the elevated child stopped the service but failed before restarting it.
    $existingBuildArgument = if ($UseExistingBuild) { ' -UseExistingBuild' } else { '' }
    $proArgument = if ($SyncPro) { ' -SyncPro' } else { '' }
    $fleetArgument = if ($StageFleetDebug) { ' -StageFleetDebug' } else { '' }
    if (-not $UseExistingBuild) { throw 'Build the development service without elevation before synchronizing it.' }
    $serviceHashArgument = " -ExpectedServiceSha256 '$(Get-Sha256 $builtService)'"
    $freeHashArgument = if ($StageFleetDebug) { " -ExpectedFreeSha256 '$(Get-Sha256 $builtFree)'" } else { '' }
    $proHashArgument = if ($SyncPro -or $StageFleetDebug) { " -ExpectedProSha256 '$(Get-Sha256 $builtPro)'" } else { '' }
    $vaultArgument = if ($AllowVaultDismount) { ' -AllowVaultDismount' } else { '' }
    $childCommand = "try { & '$escapedScriptPath' -Elevated$existingBuildArgument$proArgument$fleetArgument$vaultArgument$serviceHashArgument$freeHashArgument$proHashArgument -DiagnosticPath '$escapedDiagnostic'; if (`$?) { exit 0 }; exit 1 } catch { exit 1 }"
    $encodedCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childCommand))
    # Pass one argument string to the Windows process launcher.  The encoded
    # payload contains no spaces, avoiding another quoting boundary on paths
    # such as E:\E drive\Company\wincommander.
    $processArguments = "-NoProfile -ExecutionPolicy Bypass -EncodedCommand $encodedCommand"
    # Do not resolve `powershell.exe` through PATH here. Development runtimes
    # can prepend a PowerShell-compatible shim that lacks Windows utility
    # cmdlets used by this script (such as Get-FileHash).
    $windowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $process = Start-Process -FilePath $windowsPowerShell -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList $processArguments
    if ($process.ExitCode -ne 0) {
        $details = if (Test-Path -LiteralPath $diagnostic) {
            Get-Content -LiteralPath $diagnostic -Raw
        } else {
            'No diagnostic was produced.'
        }
        throw "The elevated development-service synchronization failed with exit code $($process.ExitCode).`n$details"
    }
    try {
        Assert-StagedDevelopmentService
    }
    finally {
        Remove-Item -LiteralPath $diagnostic -Force -ErrorAction SilentlyContinue
    }
}

if (-not $Elevated) {
    try {
        # Once the service runs from the staging directory, Cargo can rebuild
        # its normal target without a running Windows service locking the EXE.
        Build-Service
    }
    catch {
        throw 'The development service could not be built without elevation. Resolve the build or locked executable before synchronization.'
    }

    $expectedHash = Get-Sha256 $builtService
    $stagedMatches = (Test-Path -LiteralPath $stagedService -PathType Leaf) -and
        ((Get-Sha256 $stagedService) -eq $expectedHash)
    $configuredPath = Get-ServiceImagePath
    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    $serviceRunning = $null -ne $service -and $service.Status -eq 'Running'
    if (-not $StageFleetDebug -and $stagedMatches -and $configuredPath -and
        $configuredPath.Equals($stagedService, [StringComparison]::OrdinalIgnoreCase) -and
        $serviceRunning -and (Test-ServiceProcessCurrent) -and
        (-not $SyncPro -or (Test-EncryptedVolumeDriverReady)) -and
        (-not $SyncPro -or (Test-DevelopmentProCurrent)) -and
        (Test-ServiceAcceptingConnections)) {
        Assert-StagedDevelopmentService
        Write-Host 'WinCommander development service is current.'
        return
    }

    Assert-NoActiveVaultsUnlessAllowed
    Start-ElevatedSync -UseExistingBuild
    Write-Host 'WinCommander development service synchronized and running.'
    return
}

# A changed development service is a security boundary update. Stop it before
# copying so Windows cannot run a half-replaced binary; the service itself
# dismounts any active Vault presentation during its normal shutdown path.
if ($SyncPro -and -not (Test-Path -LiteralPath $builtPro -PathType Leaf)) {
    throw 'Build the development Pro helper before synchronizing the service.'
}
if ($StageFleetDebug -and ((-not (Test-Path -LiteralPath $builtFree -PathType Leaf)) -or (-not (Test-Path -LiteralPath $builtPro -PathType Leaf)))) {
    throw 'Build both Free and Pro debug helpers before protected Fleet staging.'
}
if (-not $UseExistingBuild -or $ExpectedServiceSha256 -notmatch '^[0-9A-Fa-f]{64}$' -or (Get-Sha256 $builtService) -ne $ExpectedServiceSha256) { throw 'A hash-bound non-elevated service build is required before stopping the service.' }
if ($StageFleetDebug -and ($ExpectedFreeSha256 -notmatch '^[0-9A-Fa-f]{64}$' -or (Get-Sha256 $builtFree) -ne $ExpectedFreeSha256)) { throw 'The Free debug helper changed before staging.' }
if (($SyncPro -or $StageFleetDebug) -and ($ExpectedProSha256 -notmatch '^[0-9A-Fa-f]{64}$' -or (Get-Sha256 $builtPro) -ne $ExpectedProSha256)) { throw 'The Pro debug helper changed before staging.' }
Ensure-ProtectedDevelopmentDirectory $stagingDirectory
if ($StageFleetDebug) { Ensure-ProtectedDevelopmentDirectory $fleetStaging }
Assert-NoActiveVaultsUnlessAllowed
$priorService = Get-CimInstance Win32_Service -Filter "Name='WinCommanderSvc'" -ErrorAction SilentlyContinue
$priorPath = Get-ServiceImagePath
$rollbackPath = Join-Path $stagingDirectory 'wincommander-svc.rollback.exe'
$rollbackHash = $null
$rollbackStart = if ($priorService) { switch ($priorService.StartMode) { 'Auto' { 'auto' } 'Manual' { 'demand' } 'Disabled' { 'disabled' } default { $null } } } else { $null }
$priorWasRunning = $null -ne $priorService -and $priorService.State -eq 'Running'
if ($priorService -and $priorPath -and $priorService.StartName -in @('LocalSystem', 'NT AUTHORITY\SYSTEM')) {
    try {
        Assert-ProtectedPath (Split-Path -Parent $priorPath)
        Assert-ProtectedPath $priorPath
        if (Test-Path -LiteralPath $rollbackPath) { Assert-ProtectedPath $rollbackPath }
        Copy-Item -LiteralPath $priorPath -Destination $rollbackPath -Force
        Assert-ProtectedPath $rollbackPath
        $rollbackHash = Get-Sha256 $priorPath
        if ((Get-Sha256 $rollbackPath) -ne $rollbackHash) { throw 'The previous service backup differs from its source.' }
        if (-not $rollbackStart) { $rollbackHash = $null }
    } catch {
        $rollbackHash = $null
        Write-Diagnostic 'The previous service cannot be safely backed up; automatic restart will not be attempted on failure.'
    }
}
$priorStatus = if ($priorService) { $priorService.State } else { 'Missing' }
$recoverableConfig = $null -ne $priorService -and (Test-SimpleServiceImagePath $priorPath) -and [bool]$rollbackStart -and $priorService.StartName -in @('LocalSystem', 'NT AUTHORITY\SYSTEM')
$sameTarget = $priorPath -and $priorPath.Equals($stagedService, [StringComparison]::OrdinalIgnoreCase)
$recoveryPlan = Get-PriorRecoveryPlan ($null -ne $priorService) $priorStatus ([bool]$rollbackHash) ([bool]$sameTarget) $recoverableConfig
if ($recoveryPlan -eq 'abort') { throw 'The prior service cannot be safely restored after a failed synchronization; it was not stopped or changed.' }
if ($recoveryPlan -eq 'config-only') { $rollbackHash = 'config-only' }
try {
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {
    Write-Diagnostic 'Stopping the existing WinCommander service.'
    Stop-Service -Name $serviceName -Force -NoWait -ErrorAction Stop
    # The service advertises 135 seconds for Vault cleanup; allow a scheduling margin.
    (Get-Service -Name $serviceName).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(150))
}

if ($UseExistingBuild) {
    if (-not (Test-Path -LiteralPath $builtService -PathType Leaf)) {
        throw "The verified development service build is missing: $builtService."
    }
    if ($ExpectedServiceSha256 -notmatch '^[0-9A-Fa-f]{64}$' -or (Get-Sha256 $builtService) -ne $ExpectedServiceSha256) { throw 'The development service changed after the non-elevated build.' }
    if ($StageFleetDebug -and ($ExpectedFreeSha256 -notmatch '^[0-9A-Fa-f]{64}$' -or (Get-Sha256 $builtFree) -ne $ExpectedFreeSha256)) { throw 'The Free debug helper changed before staging.' }
    if (($SyncPro -or $StageFleetDebug) -and ($ExpectedProSha256 -notmatch '^[0-9A-Fa-f]{64}$' -or (Get-Sha256 $builtPro) -ne $ExpectedProSha256)) { throw 'The Pro debug helper changed before staging.' }
    Write-Diagnostic 'Using the development service built by the non-elevated parent.'
} else {
    throw 'An elevated synchronization must use a previously built, hash-bound service.'
}
if (Test-Path -LiteralPath $stagedService) { Assert-ProtectedPath $stagedService }
Copy-Item -LiteralPath $builtService -Destination $stagedService -Force
Assert-ProtectedPath $stagedService
if ((Get-Sha256 $stagedService) -ne $ExpectedServiceSha256) {
    throw 'The staged development service does not match the build output.'
}
if ($StageFleetDebug) {
    foreach ($entry in @(@($builtFree, 'wincommander-free.exe'), @($builtPro, 'wincommander-pro.exe'))) {
        $destination = Join-Path $fleetStaging $entry[1]
        if (Test-Path -LiteralPath $destination) { Assert-ProtectedPath $destination }
        Copy-Item -LiteralPath $entry[0] -Destination $destination -Force
        Assert-ProtectedPath $destination
        $expected = if ($entry[1] -eq 'wincommander-free.exe') { $ExpectedFreeSha256 } else { $ExpectedProSha256 }
        if ((Get-Sha256 $destination) -ne $expected) { throw 'A staged Fleet helper differs from its approved build.' }
    }
    # Debug Free embeds its backend scripts at build time. Avoid
    # copying a separate mutable resource tree across the elevation boundary.
    $pin = Join-Path $fleetStaging 'wincommander-free.sha256'
    if (Test-Path -LiteralPath $pin) { Assert-ProtectedPath $pin }
    [IO.File]::WriteAllText($pin, (Get-Sha256 (Join-Path $fleetStaging 'wincommander-free.exe')))
    Assert-ProtectedPath $pin
}
if ($SyncPro -and -not (Test-DevelopmentProCurrent)) {
    Ensure-ProtectedDevelopmentDirectory (Split-Path -Parent $managedPro)
    $temporaryPro = "$managedPro.dev-sync"
    if (Test-Path -LiteralPath $temporaryPro) { Assert-ProtectedPath $temporaryPro }
    if (Test-Path -LiteralPath $managedPro) { Assert-ProtectedPath $managedPro }
    Copy-Item -LiteralPath $builtPro -Destination $temporaryPro -Force
    Assert-ProtectedPath $temporaryPro
    if ((Get-Sha256 $temporaryPro) -ne $ExpectedProSha256) {
        throw 'The temporary development Pro helper does not match the build.'
    }
    Move-Item -LiteralPath $temporaryPro -Destination $managedPro -Force
    Assert-ProtectedPath $managedPro
}

# sc.exe receives an already-parsed argument list from PowerShell. Preserve
# literal quote characters *inside* its binPath value so paths such as
# E:\E drive\Company\... are stored as one executable path by SCM. Do not
# add a backslash here: it turns the quote literal and splits a spaced path.
$servicePathArgument = '"' + $stagedService + '"'
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {
    Write-Diagnostic "Configuring the service with staged path: $stagedService"
    & sc.exe config $serviceName 'binPath=' $servicePathArgument 'start=' 'auto' 'obj=' 'LocalSystem'
} else {
    Write-Diagnostic "Creating the service with staged path: $stagedService"
    & sc.exe create $serviceName 'binPath=' $servicePathArgument 'start=' 'auto' 'obj=' 'LocalSystem'
}
if ($LASTEXITCODE -ne 0) { throw "Windows could not configure $serviceName." }

$configuredPath = Get-ServiceImagePath
if (-not $configuredPath -or -not $configuredPath.Equals($stagedService, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Windows did not retain the expected development service path: $configuredPath"
}

# Verify and, if needed, repair the signed driver before starting the new
# service. Starting the service first can race its driver probing against this
# hash read and made fresh checkouts fail after the UAC prompt.
if ($SyncPro) { Ensure-EncryptedVolumeDriver }
Write-Diagnostic 'Starting the staged development service.'
Start-Service -Name $serviceName
(Get-Service -Name $serviceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
Assert-StagedDevelopmentService
Write-Host 'WinCommander development service synchronized and running.'
} catch {
    $failure = $_
    $current = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    $currentStatus = if ($current) { $current.Status.ToString() } else { 'Missing' }
    if ($priorService -and $rollbackHash -and $currentStatus -eq 'Stopped') {
        try {
            [void](Invoke-SafeServiceRecovery $true $priorWasRunning $rollbackHash $currentStatus {
            param([bool]$RestartPrior)
            if ($rollbackHash -ne 'config-only') {
                Assert-ProtectedPath $stagingDirectory
                Assert-ProtectedPath $rollbackPath
                if ((Get-Sha256 $rollbackPath) -ne $rollbackHash) { throw 'The previous service backup no longer matches.' }
                Assert-ProtectedPath (Split-Path -Parent $priorPath)
                if (Test-Path -LiteralPath $priorPath) { Assert-ProtectedPath $priorPath }
                Copy-Item -LiteralPath $rollbackPath -Destination $priorPath -Force
                Assert-ProtectedPath $priorPath
                if ((Get-Sha256 $priorPath) -ne $rollbackHash) { throw 'The restored service image does not match its backup.' }
            }
            $rollbackArgument = '"' + $priorPath + '"'
            & sc.exe config $serviceName 'binPath=' $rollbackArgument 'start=' $rollbackStart 'obj=' 'LocalSystem'
            if ($LASTEXITCODE -ne 0 -or (Get-ServiceImagePath) -ne $priorPath) { throw 'Windows did not restore the previous service configuration.' }
            if ($RestartPrior) {
                Start-Service -Name $serviceName
                (Get-Service -Name $serviceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
            }
            })
            Write-Diagnostic 'The protected previous service image, startup mode, and prior stopped/running state were restored.'
        } catch {
            Write-Diagnostic ("Automatic service recovery failed: {0}" -f $_)
            throw "Development service synchronization failed and automatic recovery also failed. Original failure: $failure; recovery: $_"
        }
    } elseif ($priorService) {
        Write-Diagnostic 'Automatic recovery skipped: no verified previous image, or service is not stopped. No additional stop was attempted.'
        throw "Development service synchronization failed. Automatic recovery was unsafe or unavailable; no additional service stop was attempted. Original failure: $failure"
    }
    throw $failure
}
