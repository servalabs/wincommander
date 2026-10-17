# ============================================================================
# SYSTEM - MAINTENANCE MODULE
# Manages system cleanup, disk maintenance, and optimizations
# ============================================================================

# ============================================================================
# Modular Cleanup Components
# Each function is atomic — composable by Invoke-DiskCleanup, Invoke-DeepCleanup,
# lockdown, or any future orchestrator.
# ============================================================================

# --- Component: cleanmgr with configurable categories ---
function Invoke-CleanmgrCategories {
    param(
        [string[]]$Categories,
        [int]$SageRunId = 100
    )
    $volumeCachesPath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches'
    $flagName = "StateFlags$('{0:D4}' -f $SageRunId)"
    foreach ($cat in $Categories) {
        $keyPath = Join-Path $volumeCachesPath $cat
        if (Test-Path $keyPath) {
            Set-ItemProperty -Path $keyPath -Name $flagName -Value 2 -Type DWord -ErrorAction SilentlyContinue
        }
    }
    Start-Process -FilePath 'cleanmgr.exe' -ArgumentList "/sagerun:$SageRunId" -Wait -NoNewWindow -ErrorAction SilentlyContinue
}

# Clear-EventLogs moved to commander-pro/src/handlers.rs — privacy
# cleaners all run in the Pro sidecar now.

# --- Component: Purge temp / cache directories ---
function Clear-TempFiles {
    Assert-IsAdmin
    try {
        $paths = @(
            "$env:TEMP", "$env:SystemRoot\Temp",
            "$env:SystemRoot\Prefetch",
            "$env:SystemRoot\SoftwareDistribution\Download",
            "$env:SystemRoot\Logs\CBS",
            "$env:SystemRoot\System32\SleepStudy",
            "$env:SystemRoot\System32\sru"
        )
        foreach ($p in $paths) {
            if (Test-Path $p) {
                Get-ChildItem -Path $p -Recurse -Force -ErrorAction SilentlyContinue |
                    Remove-ItemSecure -Recurse -Force -ErrorAction SilentlyContinue
            }
        }
        @{ status = 'cleared' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Component: DISM component store cleanup ---
function Invoke-DismComponentCleanup {
    Assert-IsAdmin
    try {
        dism /Online /Cleanup-Image /StartComponentCleanup /ResetBase 2>$null
        @{ status = 'cleaned' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Helper: Measure free space delta around a script block ---
function Measure-CleanupDelta {
    param([scriptblock]$Action)
    $drive = Get-Volume -DriveLetter C
    [int64]$before = $drive.SizeRemaining
    & $Action
    $drive = Get-Volume -DriveLetter C
    [int64]$after = $drive.SizeRemaining
    $saved = [Math]::Max([int64]0, $after - $before)
    return @{
        status      = 'success'
        savedMB     = [Math]::Round($saved / 1MB, 2)
        freeSpaceMB = [Math]::Round($after / 1MB, 2)
    }
}

# ============================================================================
# Orchestrators — compose the modular components above
# ============================================================================
# Shared base categories used by orchestrators to avoid duplicate cleanmgr runs
$baseCategories = @(
    'Active Setup Temp Folders', 'Downloaded Program Files', 'Internet Cache Files',
    'Old ChkDsk Files', 'Previous Installations', 'Recycle Bin', 'Setup Log Files',
    'System error memory dump files', 'System error minidump files', 'Temporary Files',
    'Temporary Setup Files', 'Thumbnail Cache', 'Update Cleanup', 'Upgrade Discarded Files',
    'Windows Error Reporting Archive Files', 'Windows Error Reporting Queue Files', 'Windows Upgrade Log Files'
)

# Standard disk cleanup (uses the shared base categories)
function Invoke-DiskCleanup {
    Assert-IsAdmin
    try {
        return Measure-CleanupDelta { Invoke-CleanmgrCategories -Categories $baseCategories -SageRunId 100 }
    }
    catch {
        return @{ error = $true; message = "Disk cleanup failed: $($_.Exception.Message)" }
    }
}

function Set-PowerPlan {
    param([string]$Mode)
    Assert-IsAdmin
    # The Ultimate Performance scheme isn't on every SKU by default. We
    # treat it like any other built-in mode: if powercfg doesn't list its
    # GUID yet we duplicate-then-activate; subsequent calls find the
    # duplicate and just /setactive.
    $guids = @{
        'balanced'    = '381b4222-f694-41f0-9685-ff5bb260df2e'
        'powersaving' = 'a1841308-3541-4fab-bc81-f71556f20b4a'
        'performance' = '8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c'
        'ultimate'    = 'e9a42b02-d5df-448d-aa00-03f14749eb61'
    }
    $key = $Mode.ToLower()
    if (-not $guids.ContainsKey($key)) { throw "Invalid power mode: $Mode" }
    $targetGuid = $guids[$key]

    if ($key -eq 'ultimate') {
        $listed = (& powercfg /list) -match $targetGuid
        if (-not $listed) {
            $out = & powercfg /duplicatescheme $targetGuid 2>$null
            if ($LASTEXITCODE -ne 0) {
                throw "Ultimate Performance plan is not available on this SKU."
            }
            # /duplicatescheme prints the freshly-minted scheme GUID; prefer
            # that over the template GUID so /setactive succeeds.
            if ($out -match '([a-f0-9\-]{36})') { $targetGuid = $Matches[1] }
        }
    }

    powercfg /setactive $targetGuid
    return @{ status = 'success'; mode = $Mode; guid = $targetGuid }
}


# Optimize system services to manual (Safe list)
function Get-ServiceProfileTargets {
    # 1. Services to set to MANUAL (Start on demand)
    # KT: list deduped (case-insensitive); 'SNMPTRAP'/'SNMPTrap' was a single service.
    $manualServices = @(
        'ALG', 'AppMgmt', 'AppReadiness', 'Appinfo', 'AxInstSV', 'BDESVC', 'BTAGService', 'CDPSvc',
        'COMSysApp', 'CertPropSvc', 'CscService', 'DevQueryBroker', 'DeviceAssociationService',
        'DeviceInstall', 'DisplayEnhancementService', 'EFS', 'EapHost', 'FDResPub', 'FrameServer',
        'FrameServerMonitor', 'GraphicsPerfSvc', 'HvHost', 'IKEEXT', 'InstallService', 'InventorySvc',
        'IpxlatCfgSvc', 'KtmRm', 'LicenseManager', 'LxpSvc', 'MSDTC', 'MSiSCSI', 'McpManagementService',
        'MicrosoftEdgeElevationService', 'NaturalAuthentication', 'NcaSvc', 'NcbService',
        'NcdAutoSetup', 'NetSetupSvc', 'Netman', 'NlaSvc', 'PcaSvc', 'PeerDistSvc', 'PerfHost', 'PhoneSvc',
        'PlugPlay', 'PolicyAgent', 'PrintNotify', 'PushToInstall', 'QWAVE', 'RasAuto', 'RasMan',
        'RetailDemo', 'RmSvc', 'RpcLocator', 'SCPolicySvc', 'SCardSvr', 'SDRSVC', 'SEMgrSvc',
        'SNMPTRAP', 'SSDPSRV', 'ScDeviceEnum', 'SensorDataService', 'SensorService',
        'SensrSvc', 'SessionEnv', 'SharedAccess', 'SmsRouter', 'SstpSvc', 'StiSvc', 'StorSvc', 'TapiSrv',
        'TieringEngineService', 'TokenBroker', 'TroubleshootingSvc', 'TrustedInstaller',
        # Keep clock and automatic time-zone synchronization under Windows'
        # default automatic management. An optimization profile must not leave
        # a machine without time synchronization.
        'UmRdpService', 'UsoSvc', 'VSS', 'WEPHOSTSVC', 'WFDSConMgrSvc', 'WMPNetworkSvc',
        'WManSvc', 'WPDBusEnum', 'WalletService', 'WarpJITSvc', 'WbioSrvc', 'WdiServiceHost',
        'WdiSystemHost', 'WebClient', 'Wecsvc', 'WerSvc', 'WiaRpc', 'WinRM', 'WpcMonSvc', 'WpnService',
        'XblAuthManager', 'XblGameSave', 'XboxGipSvc', 'XboxNetApiSvc', 'bthserv',
        'camsvc', 'cloudidsvc', 'dcsvc', 'defragsvc', 'diagsvc', 'dmwappushservice', 'dot3svc',
        'edgeupdate', 'edgeupdatem', 'fdPHost', 'fhsvc', 'hidserv', 'icssvc', 'lfsvc', 'lltdsvc',
        'lmhosts', 'netprofm', 'perceptionsimulation', 'pla', 'seclogon', 'smphost', 'svsvc',
        'swprv', 'upnphost', 'vds', 'vmicguestinterface', 'vmicheartbeat', 'vmickvpexchange',
        'vmicrdv', 'vmicshutdown', 'vmictimesync', 'vmicvmsession', 'vmicvss', 'wbengine',
        'wcncsvc', 'webthreatdefsvc', 'wercplsupport', 'wisvc', 'wlidsvc', 'wlpasvc', 'wmiApSrv',
        'workfolderssvc', 'wuauserv'
    ) | Sort-Object -Unique

    # DiagTrack is controlled by Privacy. NetBT is intentionally aggressive:
    # only appropriate for modern networks that do not rely on legacy NetBIOS.
    $disableServices = @(
        'AppVClient', 'AssignedAccessManagerSvc', 'DialogBlockingService',
        'NetTcpPortSharing', 'RemoteAccess', 'RemoteRegistry',
        'UevAgentService', 'shpamsvc', 'ssh-agent', 'tzautoupdate',
        'dam',                   # Desktop Activity Moderator
        'GpuEnergyDrv',          # GPU Energy Driver
        'NetBT',                 # NetBIOS over TCP/IP
        'diagnosticshub.standardcollector.service', # Diagnostics Hub
        'DPS',                   # Diagnostic Policy Service
        'tcpipreg',              # TCP/IP Registry Compatibility
        'UCPD',                  # UCPD velocity
        'MapsBroker',            # Downloaded Maps Manager
        'Fax'                    # Fax
    ) | Sort-Object -Unique
    return @{ manual = $manualServices; disable = $disableServices }
}

function Get-ServiceProfileStatus {
    $targets = Get-ServiceProfileTargets
    $checked = 0
    $drift = 0
    # State, not the last button click, survives reinstalls and detects actual changes.
    foreach ($mode in @('manual', 'disable')) {
        foreach ($name in $targets[$mode]) {
            $service = Get-Service -Name $name -ErrorAction SilentlyContinue
            if (-not $service) { continue }
            if (([int]$service.ServiceType -band 0xF) -ne 0) { continue }
            $checked++
            $expected = if ($mode -eq 'manual') { 'Manual' } else { 'Disabled' }
            if ($service.StartType -ne $expected -or ($mode -eq 'disable' -and $service.Status -ne 'Stopped')) { $drift++ }
        }
    }
    return @{ applied = ($checked -gt 0 -and $drift -eq 0); checkedCount = $checked; driftCount = $drift }
}

function Set-ServicesManual {
    param([Parameter(Mandatory)][string]$PlanId)
    Invoke-ServiceChangePlan -PlanId $PlanId -ExpectedAction 'Profile'
}
function Invoke-SystemRepair {
    Assert-IsAdmin
    try {
        Write-Host "Step 1/4: Running CHKDSK Scan..."
        chkdsk.exe /scan /perf | Out-Null
        
        Write-Host "Step 2/4: Running SFC Scan (Initial)..."
        sfc.exe /scannow | Out-Null
        
        Write-Host "Step 3/4: Running DISM Restore Health..."
        DISM /Online /Cleanup-Image /RestoreHealth | Out-Null
        
        Write-Host "Step 4/4: Running SFC Scan (Verification)..."
        sfc.exe /scannow | Out-Null
        
        return @{ status = 'success'; message = 'System repair completed successfully.' }
    }
    catch {
        return @{ error = $true; message = "System repair failed: $($_.Exception.Message)" }
    }
}

function Invoke-WindowsUpdateRepair {
    Assert-IsAdmin
    try {
        $services = @('bits', 'wuauserv', 'appidsvc', 'cryptsvc', 'msiserver')
        foreach ($service in $services) { Stop-Service -Name $service -Force -ErrorAction SilentlyContinue }
        Remove-Item -LiteralPath (Join-Path $env:windir 'SoftwareDistribution') -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath (Join-Path $env:windir 'System32\catroot2') -Recurse -Force -ErrorAction SilentlyContinue
        netsh.exe winsock reset | Out-Null
        netsh.exe winhttp reset proxy | Out-Null
        DISM /Online /Cleanup-Image /RestoreHealth /NoRestart | Out-Null
        sfc.exe /scannow | Out-Null
        foreach ($service in $services) { Start-Service -Name $service -ErrorAction SilentlyContinue }
        return @{ status = 'repaired'; requiresReboot = $true }
    }
    catch {
        return @{ error = $true; message = "Windows Update repair failed: $($_.Exception.Message)" }
    }
}

function Invoke-Defrag {
    Assert-IsAdmin
    try {
        # Optimize-Volume automatically handles SSDs (retrim) and HDDs (defrag)
        Get-Volume | Where-Object DriveLetter | Where-Object DriveType -eq Fixed | Optimize-Volume | Out-Null
        return @{ status = 'success'; message = 'Drive optimization completed.' }
    }
    catch {
        return @{ error = $true; message = "Defrag failed: $($_.Exception.Message)" }
    }
}

# ============================================================================
# Deep Disk Cleanup — orchestrates all modular components
# ============================================================================

function Invoke-DeepCleanup {
    Assert-IsAdmin
    try {
        return Measure-CleanupDelta {
            # 1. Build combined categories: base + extras (unique)
            $extraCategories = @(
                'BranchCache', 'D3D Shader Cache', 'Delivery Optimization Files',
                'Device Driver Packages', 'Diagnostic Data Viewer database files', 'Language Pack',
                'RetailDemo Offline Content', 'Windows Defender', 'Windows ESD installation files'
            )
            $combined = ($baseCategories + $extraCategories) | Select-Object -Unique
            Invoke-CleanmgrCategories -Categories $combined -SageRunId 200

            # 2. Erase event logs
            Clear-EventLogs | Out-Null

            # 3. Purge temp/cache dirs
            Clear-TempFiles | Out-Null

            # 4. DISM component cleanup
            Invoke-DismComponentCleanup | Out-Null
        }
    }
    catch {
        return @{ error = $true; message = "Deep cleanup failed: $($_.Exception.Message)" }
    }
}
