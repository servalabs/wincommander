[CmdletBinding()]
param(
    # Internal only: this copy runs after the UAC prompt and is allowed to
    # stop/reconfigure the machine service.
    [switch]$Elevated,
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

function Start-ElevatedSync {
    $diagnostic = Join-Path $env:TEMP ("wincommander-dev-service-{0}.log" -f [guid]::NewGuid().ToString('N'))
    Set-Content -LiteralPath $diagnostic -Value 'Starting elevated WinCommander development-service synchronization.'
    # Start-Process does not preserve a -File value containing spaces when it
    # receives an argument array. Encode the tiny trampoline instead, so a
    # checkout such as E:\E drive\Company\wincommander reliably starts this
    # exact script in the elevated process.
    $escapedScriptPath = $PSCommandPath.Replace("'", "''")
    $escapedDiagnostic = $diagnostic.Replace("'", "''")
    $childCommand = "& '$escapedScriptPath' -Elevated -DiagnosticPath '$escapedDiagnostic'; exit `$LASTEXITCODE"
    $encodedCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childCommand))
    $process = Start-Process -FilePath powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encodedCommand
    )
    if ($process.ExitCode -ne 0) {
        $details = if (Test-Path -LiteralPath $diagnostic) {
            Get-Content -LiteralPath $diagnostic -Raw
        } else {
            'No diagnostic was produced.'
        }
        throw "The elevated development-service synchronization failed with exit code $($process.ExitCode).`n$details"
    }
    Remove-Item -LiteralPath $diagnostic -Force -ErrorAction SilentlyContinue
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

    Start-ElevatedSync
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

Write-Diagnostic 'Building the development service.'
Build-Service
New-Item -ItemType Directory -Path $stagingDirectory -Force | Out-Null
Copy-Item -LiteralPath $builtService -Destination $stagedService -Force
if ((Get-Sha256 $builtService) -ne (Get-Sha256 $stagedService)) {
    throw 'The staged development service does not match the build output.'
}

# sc.exe receives an already-parsed argument list from PowerShell. Preserve
# literal quotes *inside* its binPath value so paths such as
# E:\E drive\Company\... are stored as one executable path by SCM.
$servicePathArgument = '\"' + $stagedService + '\"'
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
Write-Host 'WinCommander development service synchronized and running.'
