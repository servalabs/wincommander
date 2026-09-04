[CmdletBinding()]
param(
    # Internal only: this copy runs after the UAC prompt and is allowed to
    # stop/reconfigure the machine service.
    [switch]$Elevated,
    # The non-elevated parent has already built the current binary.  Avoid a
    # second Cargo invocation after UAC elevation, where developer PATH/tooling
    # differences could otherwise stop the old service and strand it.
    [switch]$UseExistingBuild,
    [string]$DiagnosticPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repoRoot 'src-tauri'
$builtService = Join-Path $tauriRoot 'target\debug\wincommander-svc.exe'
$stagingDirectory = Join-Path $repoRoot '.dev\wincommander-service'
$stagedService = Join-Path $stagingDirectory 'wincommander-svc.exe'
$serviceName = 'WinCommanderSvc'
$driverServiceName = 'WinCommanderEncVol'
$driverPath = Join-Path $env:ProgramData 'WinCommander\bin\engine\EncVolKm.sys'
$driverNtPath = '\??\C:\ProgramData\WinCommander\bin\engine\EncVolKm.sys'
$driverSha256 = '1F0C6DB3559D1356C38A1486A967CD90DB5E6202E433FEA1DFE510DDB884FFB6'

function Write-Diagnostic([string]$Message) {
    if ($DiagnosticPath) {
        Add-Content -LiteralPath $DiagnosticPath -Value $Message
    }
}

trap {
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
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}

function Get-ServiceImagePath {
    $line = @(& sc.exe qc $serviceName 2>$null | Where-Object { $_ -match 'BINARY_PATH_NAME' }) |
        Select-Object -First 1
    if (-not $line) { return $null }
    return (($line -replace '^.*BINARY_PATH_NAME\s*:\s*', '').Trim().Trim('"').Trim('\'))
}

function Assert-StagedDevelopmentService {
    $configuredPath = Get-ServiceImagePath
    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    $serviceState = if ($service) { $service.Status } else { 'Missing' }
    if (-not $configuredPath -or
        -not $configuredPath.Equals($stagedService, [StringComparison]::OrdinalIgnoreCase) -or
        $serviceState -ne 'Running') {
        throw "WinCommander development-service synchronization did not take effect. Expected running service path: $stagedService. Actual path: $configuredPath. Actual state: $serviceState."
    }
}

function Ensure-EncryptedVolumeDriver {
    # The sidecar extracts this Microsoft-signed, pinned payload into the
    # fixed ProgramData location. A fresh checkout may not have reached that
    # extraction step yet; leave its first mount to the service bootstrap.
    if (-not (Test-Path -LiteralPath $driverPath -PathType Leaf)) {
        Write-Diagnostic "Encrypted-volume driver payload is not present yet: $driverPath"
        return
    }
    if ((Get-Sha256 $driverPath) -ne $driverSha256) {
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

    & sc.exe start $driverServiceName 2>$null
    if ($LASTEXITCODE -notin @(0, 1056)) {
        throw "Windows could not start $driverServiceName (exit code $LASTEXITCODE)."
    }
    $driverConfig = @(& sc.exe qc $driverServiceName 2>&1)
    $driverRunning = @(& sc.exe query $driverServiceName 2>&1) -match 'STATE\s*:\s*4\s+RUNNING'
    if (($driverConfig -notmatch [regex]::Escape($driverNtPath)) -or -not $driverRunning) {
        throw "$driverServiceName did not retain the fixed driver path or running state."
    }
    Write-Diagnostic 'Encrypted-volume driver service is running.'
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
    $childCommand = "try { & '$escapedScriptPath' -Elevated$existingBuildArgument -DiagnosticPath '$escapedDiagnostic'; if (`$?) { exit 0 }; exit 1 } catch { exit 1 }"
    $encodedCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childCommand))
    # Pass one argument string to the Windows process launcher.  The encoded
    # payload contains no spaces, avoiding another quoting boundary on paths
    # such as E:\E drive\Company\wincommander.
    $processArguments = "-NoProfile -ExecutionPolicy Bypass -EncodedCommand $encodedCommand"
    $process = Start-Process -FilePath powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList $processArguments
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
        # First migration from an older target\debug service can hold the build
        # output open. The elevated child stops it, rebuilds, and completes the
        # one-time move to the staging directory.
        Start-ElevatedSync
        Write-Host 'WinCommander development service synchronized and running.'
        return
    }

    $expectedHash = Get-Sha256 $builtService
    $stagedMatches = (Test-Path -LiteralPath $stagedService -PathType Leaf) -and
        ((Get-Sha256 $stagedService) -eq $expectedHash)
    $configuredPath = Get-ServiceImagePath
    $serviceRunning = (Get-Service -Name $serviceName -ErrorAction SilentlyContinue).Status -eq 'Running'
    if ($stagedMatches -and $configuredPath -and
        $configuredPath.Equals($stagedService, [StringComparison]::OrdinalIgnoreCase) -and
        $serviceRunning) {
        Write-Host 'WinCommander development service is current.'
        return
    }

    Start-ElevatedSync -UseExistingBuild
    Write-Host 'WinCommander development service synchronized and running.'
    return
}

# A changed development service is a security boundary update. Stop it before
# copying so Windows cannot run a half-replaced binary; the service itself
# dismounts any active Vault presentation during its normal shutdown path.
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {
    Write-Diagnostic 'Stopping the existing WinCommander service.'
    Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
    (Get-Service -Name $serviceName).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
}

if ($UseExistingBuild) {
    if (-not (Test-Path -LiteralPath $builtService -PathType Leaf)) {
        throw "The verified development service build is missing: $builtService."
    }
    Write-Diagnostic 'Using the development service built by the non-elevated parent.'
} else {
    Write-Diagnostic 'Building the development service.'
    Build-Service
}
New-Item -ItemType Directory -Path $stagingDirectory -Force | Out-Null
Copy-Item -LiteralPath $builtService -Destination $stagedService -Force
if ((Get-Sha256 $builtService) -ne (Get-Sha256 $stagedService)) {
    throw 'The staged development service does not match the build output.'
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

Write-Diagnostic 'Starting the staged development service.'
Start-Service -Name $serviceName
(Get-Service -Name $serviceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
Assert-StagedDevelopmentService
Ensure-EncryptedVolumeDriver
Write-Host 'WinCommander development service synchronized and running.'
