# ============================================================================
# TWEAKS - SYSTEM MODULE
# Core Windows hardening, performance, and security tweaks
# ============================================================================

# --- WINDOWS DEFENDER ---
#
# Disable-WindowsDefender and Enable-WindowsDefender are paid (A-2 split).
# Their implementations live in commander-pro/src/handlers.rs; Free's
# dispatch routes them via dispatch_paid_command before any local module
# loads. Get-DefenderStatus (read-only probe) stays free below.
#
# Strings-grep CI gate (A-5) verifies the function names no longer appear
# in Free's encrypted .enc bundle after build.

function Get-DefenderStatus {
    # KT: realtimeEnabled used to read only the policy registry, so on systems
    # where Defender's binaries/service have been removed (no policy was ever
    # written by Microsoft, so the key is simply absent) it would return $true
    # — the dashboard then showed "DEFENDER: ON" while the cmdlet-based
    # exclusion auditor correctly said "disabled". Anchor the answer to
    # whether the engine actually exists and is running first; only then ask
    # whether policy has disabled realtime.
    $service     = Get-Service -Name 'WinDefend' -ErrorAction SilentlyContinue
    $serviceRunning = ($service -and $service.Status -eq 'Running')

    $rtDisabled  = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Real-Time Protection' -Name 'DisableRealtimeMonitoring' -ErrorAction SilentlyContinue
    $rtPolicyOk  = (-not $rtDisabled -or $rtDisabled.DisableRealtimeMonitoring -ne 1)

    @{
        serviceRunning  = $serviceRunning
        # Realtime cannot be enabled without a running engine — no service or
        # stopped service short-circuits this to false regardless of policy.
        realtimeEnabled = ($serviceRunning -and $rtPolicyOk)
    }
}

# --- WINDOWS UPDATE ---

# Disable-WindowsUpdate relocated to commander-pro (handlers.rs). Indefinitely
# pausing Windows Update (pause keys set to 2038 + service stops) suppresses
# security patching and is AV/EDR-flagged, so it runs in the Pro sidecar via
# dispatch_paid_command (tier=Paid). Enable-WindowsUpdate (revert) stays Free
# + local below.

function Enable-WindowsUpdate {
    Assert-IsAdmin
    $wuPath = 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings'
    $reservePath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\ReserveManager'
    $upgradePath = 'HKLM:\SYSTEM\Setup\UpgradeNotification'
    $devHomePath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Orchestrator\UScheduler\DevHomeUpdate'
    $outlookPath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Orchestrator\UScheduler\OutlookUpdate'
    $oobeSchedulerPath = 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\Orchestrator\UScheduler_Oobe'
    $doSettingsPath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DeliveryOptimization\Settings'

    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'FlightSettingsMaxPauseDays'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseUpdatesStartTime'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseFeatureUpdatesStartTime'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseFeatureUpdatesEndTime'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseQualityUpdatesStartTime'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseQualityUpdatesEndTime'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseUpdatesExpiryTime'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseState'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'PauseUpdateStatus'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'HideMCTLink'
    Invoke-7Erase -Path $wuPath -Type RegistryProperty -Name 'RestartNotificationsAllowed2'
    Invoke-7Erase -Path $reservePath -Type RegistryProperty -Name 'ShippedWithReserves'
    Invoke-7Erase -Path $upgradePath -Type RegistryProperty -Name 'UpgradeAvailable'
    Invoke-7Erase -Path $devHomePath -Type RegistryProperty -Name 'workCompleted'
    Invoke-7Erase -Path $outlookPath -Type RegistryProperty -Name 'workCompleted'
    Invoke-7Erase -Path $oobeSchedulerPath -Type RegistryProperty -Name 'BlockedOobeUpdaters'
    Invoke-7Erase -Path $doSettingsPath -Type RegistryProperty -Name 'DownloadMode'

    Set-Service -Name 'wuauserv' -StartupType Manual -ErrorAction SilentlyContinue
    Start-Service -Name 'UsoSvc' -ErrorAction SilentlyContinue
    Start-Service -Name 'wuauserv' -ErrorAction SilentlyContinue
    @{ status = 'enabled' }
}

function Get-UpdateStatus {
    $service = Get-Service -Name 'wuauserv' -ErrorAction SilentlyContinue
    $pauseTime = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings' -Name 'PauseUpdatesExpiryTime' -ErrorAction SilentlyContinue
    @{
        serviceRunning = ($service -and $service.Status -eq 'Running')
        paused         = ($null -ne $pauseTime)
        pausedUntil    = if ($pauseTime) { $pauseTime.PauseUpdatesExpiryTime } else { $null }
    }
}

# --- POWER & PERFORMANCE ---

function Disable-Hibernation {
    Assert-IsAdmin
    powercfg /hibernate off
    @{ status = 'disabled' }
}

function Enable-Hibernation {
    Assert-IsAdmin
    powercfg /hibernate on
    @{ status = 'enabled' }
}

function Disable-FastStartup {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name 'HiberbootEnabled' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-FastStartup {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name 'HiberbootEnabled' -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

function Disable-Superfetch {
    Assert-IsAdmin
    Stop-Service -Name 'SysMain' -Force -ErrorAction SilentlyContinue
    Set-Service -Name 'SysMain' -StartupType Disabled -ErrorAction SilentlyContinue
    $pPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    Set-ItemProperty -Path $pPath -Name 'EnablePrefetcher' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $pPath -Name 'EnableSuperfetch' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-Superfetch {
    Assert-IsAdmin
    Set-Service -Name 'SysMain' -StartupType Automatic -ErrorAction SilentlyContinue
    Start-Service -Name 'SysMain' -ErrorAction SilentlyContinue
    $pPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    Set-ItemProperty -Path $pPath -Name 'EnablePrefetcher' -Value 3 -Type DWord -Force
    Set-ItemProperty -Path $pPath -Name 'EnableSuperfetch' -Value 3 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- PREFETCH ---

function Disable-Prefetch {
    Assert-IsAdmin
    $pPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    if (!(Test-Path $pPath)) { New-Item -Path $pPath -Force | Out-Null }
    # 0 = Disabled, 1 = App Launch Prefetching, 2 = Boot Prefetching, 3 = App + Boot
    Set-ItemProperty -Path $pPath -Name 'EnablePrefetcher' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-Prefetch {
    Assert-IsAdmin
    $pPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    Set-ItemProperty -Path $pPath -Name 'EnablePrefetcher' -Value 3 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- UAC ---

# Disable-UAC relocated to commander-pro (handlers.rs). EnableLUA=0 turns off
# User Account Control machine-wide (reduces security, AV/EDR-flagged), so it
# runs in the Pro sidecar via dispatch_paid_command (tier=Paid). Enable-UAC
# (re-harden) stays Free + local below.

function Enable-UAC {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' -Name 'EnableLUA' -Value 1 -Type DWord -Force
    @{ status = 'enabled'; requiresRestart = $true }
}

# --- NTFS ---

function Enable-NTFSOptimizations {
    Assert-IsAdmin
    fsutil behavior set disablelastaccess 1
    fsutil behavior set disable8dot3 1
    fsutil behavior set memoryusage 2
    @{ status = 'enabled' }
}

function Disable-NTFSOptimizations {
    Assert-IsAdmin
    fsutil behavior set disablelastaccess 0
    fsutil behavior set disable8dot3 0
    fsutil behavior set memoryusage 1
    @{ status = 'disabled' }
}

# --- CONSUMER FEATURES ---

function Disable-ConsumerFeatures {
    Assert-IsAdmin
    # Using Policy key
    $cloudPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent'
    if (!(Test-Path $cloudPath)) { New-Item -Path $cloudPath -Force | Out-Null }
    Set-ItemProperty -Path $cloudPath -Name 'DisableWindowsConsumerFeatures' -Value 1 -Type DWord -Force
    
    # Keeping existing CDM keys for extra coverage
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager'
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'ContentDeliveryAllowed' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'OemPreInstalledAppsEnabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'PreInstalledAppsEnabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'PreInstalledAppsEverEnabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'SilentInstalledAppsEnabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'SubscribableContent-338387Enabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'SubscribableContent-338388Enabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'SubscribableContent-338389Enabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'SystemPaneSuggestionsEnabled' -Value 0 -Type DWord -Force
    
    @{ status = 'disabled' }
}

function Enable-ConsumerFeatures {
    Assert-IsAdmin
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager'
    if (Test-Path $path) {
        Set-ItemProperty -Path $path -Name 'ContentDeliveryAllowed' -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name 'SilentInstalledAppsEnabled' -Value 1 -Type DWord -Force
    }
    
    $cloudPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent'
    if (Test-Path $cloudPath) {
        Invoke-7Erase -Path $cloudPath -Type RegistryProperty -Name 'DisableWindowsConsumerFeatures'
    }
    
    @{ status = 'enabled' }
}

# --- DETAILED BSOD ---

function Enable-DetailedBSOD {
    Assert-IsAdmin
    $path = "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl"
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name "DisplayParameters" -Value 1 -Type DWord -Force
    Set-ItemProperty -Path $path -Name "DisableEmoticon" -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

function Disable-DetailedBSOD {
    Assert-IsAdmin
    $path = "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl"
    if (Test-Path $path) {
        Set-ItemProperty -Path $path -Name "DisplayParameters" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "DisableEmoticon" -Value 0 -Type DWord -Force
    }
    @{ status = 'disabled' }
}

# --- USB STORAGE LOCKDOWN + USB WRITE PROTECT (DLP) ---
#
# All five commands (Enable/Disable-USBStorageLockdown,
# Get-USBStorageLockdownStatus, Enable/Disable-USBWriteProtect)
# are paid (A-2 split) and moved to commander-pro/src/handlers.rs.
# Free's dispatch routes them via dispatch_paid_command before any
# local module loads. Strings-grep CI gate (A-5) verifies these
# function names no longer appear in Free's encrypted .enc bundle.

# --- RAM-SPILL CONTROL ---
# Combines hibernation-off + fast-startup-off + pagefile zero-on-shutdown.
# This is the DEFERRED-safe path (ClearPageFileAtShutdown); the immediate
# live-zero path (Invoke-PagefileZero) is a paid destruct step in Pro.

function Enable-RamSpillControl {
    Assert-IsAdmin
    Disable-Hibernation | Out-Null
    Disable-FastStartup | Out-Null
    $mmPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
    Set-ItemProperty -Path $mmPath -Name 'ClearPageFileAtShutdown' -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

function Disable-RamSpillControl {
    Assert-IsAdmin
    Enable-Hibernation | Out-Null
    Enable-FastStartup | Out-Null
    $mmPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
    Set-ItemProperty -Path $mmPath -Name 'ClearPageFileAtShutdown' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

# --- MASTER STATUS ---

# --- Windows maintenance and platform compatibility ---

function Disable-AutomaticMaintenance {
    Assert-IsAdmin
    try {
        $maintenancePath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\Maintenance'
        $diagnosticsPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\ScheduledDiagnostics'
        if (!(Test-Path $maintenancePath)) { New-Item -Path $maintenancePath -Force | Out-Null }
        if (!(Test-Path $diagnosticsPath)) { New-Item -Path $diagnosticsPath -Force | Out-Null }
        Set-ItemProperty -Path $maintenancePath -Name 'MaintenanceDisabled' -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $diagnosticsPath -Name 'EnabledExecution' -Value 0 -Type DWord -Force
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutomaticMaintenance {
    Assert-IsAdmin
    try {
        # Remove only values owned by this toggle so Windows returns to its default policy.
        Remove-ItemSecure -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\Maintenance' -Name 'MaintenanceDisabled' -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\ScheduledDiagnostics' -Name 'EnabledExecution' -ErrorAction SilentlyContinue
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-Win32LongPaths {
    Assert-IsAdmin
    try {
        Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name 'LongPathsEnabled' -Value 1 -Type DWord -Force
        @{ status = 'enabled'; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-Win32LongPaths {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name 'LongPathsEnabled' -ErrorAction SilentlyContinue
        @{ status = 'disabled'; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-SmbBandwidthThrottling {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanWorkstation\Parameters'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'DisableBandwidthThrottling' -Value 1 -Type DWord -Force
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-SmbBandwidthThrottling {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanWorkstation\Parameters' -Name 'DisableBandwidthThrottling' -ErrorAction SilentlyContinue
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-HardeningStatus {
    $defender = Get-DefenderStatus
    $updates = Get-UpdateStatus
    $uac = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' -Name 'EnableLUA' -ErrorAction SilentlyContinue
    $sysMain = Get-Service -Name 'SysMain' -ErrorAction SilentlyContinue
    $prefetch = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters' -ErrorAction SilentlyContinue
    $bgApps = Get-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications' -Name 'GlobalUserDisabled' -ErrorAction SilentlyContinue
    $bgToggle = Get-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Search' -Name 'BackgroundAppGlobalToggle' -ErrorAction SilentlyContinue
    $hiber = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Power' -Name 'HibernateEnabled' -ErrorAction SilentlyContinue
    $fastStartup = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name 'HiberbootEnabled' -ErrorAction SilentlyContinue
    $notifPolicy = Get-ItemProperty -Path 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer' -Name 'DisableNotificationCenter' -ErrorAction SilentlyContinue
    $notifToast = Get-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\PushNotifications' -Name 'ToastEnabled' -ErrorAction SilentlyContinue
    $classicContextMenu = (Test-Path 'HKCU:\SOFTWARE\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32')

    $ntfsOptimizations = $false
    try {
        function Get-NtfsVal { param([string]$S) $o = fsutil behavior query $S 2>$null; if ($o -match '[:=]\s*(\d+)') { return [int]$Matches[1] }; return $null }
        $ntfsOptimizations = ((Get-NtfsVal 'disablelastaccess') -eq 1 -and (Get-NtfsVal 'disable8dot3') -eq 1 -and (Get-NtfsVal 'memoryusage') -eq 2)
    }
    catch { $ntfsOptimizations = $false }

    $activity = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' -Name 'PublishUserActivities' -ErrorAction SilentlyContinue
    # KT: Delegate to the canonical probe in privacy/telemetry.ps1 (same file
    # that owns Disable/Enable-LocationTracking) so the two can never disagree.
    # A single ConsentStore key isn't enough — AppPrivacy force-deny, the
    # DeviceAccess Global master switch, and a disabled lfsvc service can each
    # independently keep location blocked even when that one key says "Allow".
    $locationStatus = Get-LocationTrackingStatus
    $posh = [Environment]::GetEnvironmentVariable('POWERSHELL_TELEMETRY_OPTOUT', 'Machine')
    $telemetry = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Name 'AllowTelemetry' -ErrorAction SilentlyContinue
    $endTask = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings' -Name 'TaskbarEndTask' -ErrorAction SilentlyContinue
    $bing = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search' -Name 'BingSearchEnabled' -ErrorAction SilentlyContinue
    $adv = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -ErrorAction SilentlyContinue
    $maintenance = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\Maintenance' -Name 'MaintenanceDisabled' -ErrorAction SilentlyContinue
    $longPaths = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name 'LongPathsEnabled' -ErrorAction SilentlyContinue
    $smb = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\LanmanWorkstation\Parameters' -Name 'DisableBandwidthThrottling' -ErrorAction SilentlyContinue
    $remoteAssistance = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Remote Assistance' -Name 'fAllowToGetHelp' -ErrorAction SilentlyContinue
    $remoteAssistancePolicy = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services' -Name 'fAllowToGetHelp' -ErrorAction SilentlyContinue
    $anonymousSam = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa' -Name 'RestrictAnonymousSAM' -ErrorAction SilentlyContinue

    $consumer = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent' -Name 'DisableWindowsConsumerFeatures' -ErrorAction SilentlyContinue
    $ipv4Pref = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters' -Name 'DisabledComponents' -ErrorAction SilentlyContinue
    # Gallery/Home check
    $hubMode = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer' -Name 'HubMode' -ErrorAction SilentlyContinue

    # Recall & Transparency & Typing
    $recall = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI" -Name "DisableAIDataAnalysis" -ErrorAction SilentlyContinue
    $urecall = Get-ItemProperty -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI" -Name "DisableAIDataAnalysis" -ErrorAction SilentlyContinue
    $trans = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name "EnableTransparency" -ErrorAction SilentlyContinue
    $typing = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Input\Settings" -Name "InsightsEnabled" -ErrorAction SilentlyContinue

    # USB Write Protect check
    $usbWp = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\StorageDevicePolicies' -Name 'WriteProtect' -ErrorAction SilentlyContinue
    # USB Storage Lockdown check
    $usbStorStart = (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\USBSTOR' -Name 'Start' -ErrorAction SilentlyContinue).Start

    # Anti-Acquisition Defenses check
    $acqBlocklist = $false
    try { $acqBlocklist = ((Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Config' -Name 'VulnerableDriverBlocklistEnable' -ErrorAction SilentlyContinue).VulnerableDriverBlocklistEnable -eq 1) } catch { $acqBlocklist = $false }
    $forensicToolBlock = $false
    try { $forensicToolBlock = -not [string]::IsNullOrEmpty((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\ftkimager.exe' -Name 'Debugger' -ErrorAction SilentlyContinue).Debugger) } catch { $forensicToolBlock = $false }

    # Exploit Protection (DEP / ASLR / CFG / SEHOP / Heap) check — probed
    # INLINE rather than via Get-ExploitProtectionStatus (security.ps1) since
    # this module cannot assume that module is loaded. Same defensive
    # ON/OFF/NOTSET matching as the security module's probe.
    $procMit = $null
    try { $procMit = Get-ProcessMitigation -System -ErrorAction SilentlyContinue } catch { $procMit = $null }
    $depEnabled = $false
    try { $depEnabled = ($null -ne $procMit.Dep.Enable) -and ("$($procMit.Dep.Enable)" -match '^(?i)on$') } catch { $depEnabled = $false }
    $aslrMandatory = $false
    try { $aslrMandatory = ($null -ne $procMit.Aslr.ForceRelocateImages) -and ("$($procMit.Aslr.ForceRelocateImages)" -match '^(?i)on$') } catch { $aslrMandatory = $false }
    $aslrBottomUp = $false
    try { $aslrBottomUp = ($null -ne $procMit.Aslr.BottomUp) -and ("$($procMit.Aslr.BottomUp)" -match '^(?i)on$') } catch { $aslrBottomUp = $false }
    $cfgEnabled = $false
    try { $cfgEnabled = ($null -ne $procMit.Cfg.Enable) -and ("$($procMit.Cfg.Enable)" -match '^(?i)on$') } catch { $cfgEnabled = $false }
    $heapIntegrity = $false
    try { $heapIntegrity = ($null -ne $procMit.Heap.TerminateOnError) -and ("$($procMit.Heap.TerminateOnError)" -match '^(?i)on$') } catch { $heapIntegrity = $false }
    $sehopEnabled = $false
    try { $sehopEnabled = ($null -ne $procMit.Sehop.Enable) -and ("$($procMit.Sehop.Enable)" -match '^(?i)on$') } catch { $sehopEnabled = $false }

    # Defender Exploit Guard (ASR / Controlled Folder Access / Network
    # Protection) check — probed INLINE via Get-MpPreference, fetched ONCE,
    # same defensive try/catch-default-$false pattern as the ProcessMitigation
    # probes above. EnableControlledFolderAccess/EnableNetworkProtection come
    # back as an enum whose Enabled value is 1 — stringify and match rather
    # than assume a bare -eq 1 always holds across OS builds.
    $mp = Get-MpPreference -ErrorAction SilentlyContinue
    $asrRulesEnabled = $false
    try { $asrRulesEnabled = [bool]($mp.AttackSurfaceReductionRules_Actions | Where-Object { "$_" -match '1|Enabled' }) } catch { $asrRulesEnabled = $false }
    $controlledFolderAccessEnabled = $false
    try { $controlledFolderAccessEnabled = "$($mp.EnableControlledFolderAccess)" -match '1|Enabled' } catch { $controlledFolderAccessEnabled = $false }
    $networkProtectionEnabled = $false
    try { $networkProtectionEnabled = "$($mp.EnableNetworkProtection)" -match '1|Enabled' } catch { $networkProtectionEnabled = $false }

    # Lid-close power-off (Anti-Acquisition) — probed INLINE via powercfg (a
    # native OS tool, not a Pro-only command) so this stays callable from the
    # Free bundle. SUB_BUTTONS=4f971e89-eebd-4455-a8de-9e59040e7347,
    # LIDACTION=5ca83367-6e45-459f-a27b-476b1d01c936; action 3 = Shut down.
    # A host with no lid subsystem (desktop/server) reports $false, not an error.
    $lidClosePowerOff = $false
    try {
        $lidQuery = (& powercfg /query SCHEME_CURRENT 4f971e89-eebd-4455-a8de-9e59040e7347 5ca83367-6e45-459f-a27b-476b1d01c936 2>$null | Out-String)
        if ($lidQuery -match 'Current AC Power Setting Index:\s*0x0+3\b') { $lidClosePowerOff = $true }
    } catch { $lidClosePowerOff = $false }

    # RDP stability state — probed INLINE from the registry/QoS here, not via
    # Get-Rdp*Status. Those status helpers live only in the paid handler
    # (commander-pro/handlers.rs) and are intentionally absent from this free
    # bundle, so calling them threw CommandNotFoundException and aborted the
    # whole hardening scan — leaving current.tweaks.rdp.* unset so the
    # Keep-Alive / No-Timeouts / QoS toggles never reflected real state.
    $tsCtrl   = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server'
    $kaEnable = (Get-ItemProperty -Path $tsCtrl -Name 'KeepAliveEnable'       -ErrorAction SilentlyContinue).KeepAliveEnable
    $kaSingle = (Get-ItemProperty -Path $tsCtrl -Name 'fSingleSessionPerUser' -ErrorAction SilentlyContinue).fSingleSessionPerUser
    $rdpKeepAlive = [bool]($kaEnable -eq 1 -and $kaSingle -eq 1)
    $tsPol    = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
    $polProps = Get-ItemProperty -Path $tsPol -ErrorAction SilentlyContinue
    $rdpNoTimeouts = [bool]($polProps -and $polProps.MaxDisconnectionTime -eq 0 -and $polProps.MaxIdleTime -eq 0 -and $polProps.MaxConnectionTime -eq 0 -and $polProps.fResetBroken -eq 0)
    $qosIn  = Get-NetQosPolicy -Name 'RDP Priority'     -ErrorAction SilentlyContinue
    $qosOut = Get-NetQosPolicy -Name 'RDP Priority Out' -ErrorAction SilentlyContinue
    $rdpQosPriority = [bool]($qosIn -and $qosOut)
    # KT: Delegate to the canonical probe in privacy/telemetry.ps1 so this and
    # Get-DiagnosticEventTracingStatus can never disagree. Two key correctness
    # rules live there: (a) the logger list is the single source of truth for
    # both probe and Apply (probing an untouched logger flipped the toggle off
    # after Windows re-enabled it), and (b) a missing `Start` value counts as
    # "off" — Windows treats absent autologger Start as "do not auto-start",
    # so `$null -ne 0` (which is $true in PowerShell) is the wrong test.
    $diagEventTracingDisabled = [bool](Get-DiagnosticEventTracingStatus).disabled
    # KT: Delegate to the canonical probe in privacy/telemetry.ps1 (same file
    # that owns Disable-InternetCommunication) so Apply and the radar finding
    # can never disagree about which keys count as "restricted".
    $internetCommRestricted = [bool](Get-InternetCommunicationStatus).restricted

    @{
        defenderDisabled         = (-not $defender.serviceRunning -or -not $defender.realtimeEnabled)
        updatesPaused            = ($updates.paused -or -not $updates.serviceRunning)
        uacDisabled              = ($uac.EnableLUA -eq 0)
        superfetchDisabled       = (($sysMain -and ($sysMain.StartType -eq 'Disabled' -or $sysMain.Status -ne 'Running')) -or ($prefetch.EnableSuperfetch -eq 0))
        prefetchDisabled         = ($prefetch.EnablePrefetcher -eq 0)
        backgroundAppsDisabled   = ($bgApps.GlobalUserDisabled -eq 1 -or $bgToggle.BackgroundAppGlobalToggle -eq 0)
        hibernationDisabled      = ($hiber.HibernateEnabled -eq 0)
        fastStartupDisabled      = ($fastStartup.HiberbootEnabled -eq 0)
        notificationsDisabled    = ($notifPolicy.DisableNotificationCenter -eq 1 -or $notifToast.ToastEnabled -eq 0)
        classicContextMenu       = $classicContextMenu
        ntfsOptimizations        = $ntfsOptimizations
        activityHistoryDisabled  = ($activity.PublishUserActivities -eq 0)
        locationTrackingDisabled = [bool]$locationStatus.disabled
        poshTelemetryDisabled    = ($posh -eq '1')
        telemetryDisabled        = ($telemetry.AllowTelemetry -eq 0)
        endTaskOnTaskbar         = ($endTask.TaskbarEndTask -eq 1)
        automaticMaintenanceDisabled = ($maintenance.MaintenanceDisabled -eq 1)
        win32LongPathsEnabled    = ($longPaths.LongPathsEnabled -eq 1)
        smbBandwidthThrottlingDisabled = ($smb.DisableBandwidthThrottling -eq 1)
        remoteAssistanceDisabled = (($remoteAssistance.fAllowToGetHelp -eq 0) -or ($remoteAssistancePolicy.fAllowToGetHelp -eq 0))
        anonymousSamEnumerationBlocked = ($anonymousSam.RestrictAnonymousSAM -eq 1)
        bingSearchDisabled       = ($bing.BingSearchEnabled -eq 0)
        fileExtensionsShown      = ($adv.HideFileExt -eq 0)
        hiddenFilesShown         = ($adv.Hidden -eq 1)
        explorerOpensThisPC      = ($adv.LaunchTo -eq 1)
        consumerFeaturesDisabled = ($consumer.DisableWindowsConsumerFeatures -eq 1)
        ipv4Preferred            = ($ipv4Pref.DisabledComponents -eq 32)
        galleryHomeRemoved       = ($hubMode.HubMode -eq 1)
        detailedBSOD             = ((Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl' -Name 'DisplayParameters' -ErrorAction SilentlyContinue).DisplayParameters -eq 1)
        usbWriteProtect          = ($usbWp.WriteProtect -eq 1)
        usbStorageLockdown       = ($usbStorStart -eq 4)
        recallSnapshotsDisabled  = ($recall.DisableAIDataAnalysis -eq 1 -or $urecall.DisableAIDataAnalysis -eq 1)
        transparencyDisabled     = ($trans.EnableTransparency -eq 0)
        typingInsightsDisabled   = ($typing.InsightsEnabled -eq 0)

        # Privacy
        advertisingIdDisabled    = ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo' -Name 'DisabledByGroupPolicy' -ErrorAction SilentlyContinue).DisabledByGroupPolicy -eq 1)
        tailoredExperiencesDisabled = ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent' -Name 'DisableTailoredExperiencesWithDiagnosticData' -ErrorAction SilentlyContinue).DisableTailoredExperiencesWithDiagnosticData -eq 1)
        officeLoggingDisabled    = (
            ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration' -Name 'DisableLogManagement' -ErrorAction SilentlyContinue).DisableLogManagement -eq 1) -or
            (((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Office\16.0\osm' -ErrorAction SilentlyContinue).Enablelogging -eq 0) -and
             ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Office\16.0\osm' -ErrorAction SilentlyContinue).EnableUpload -eq 0)) -or
            (((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Office\15.0\osm' -ErrorAction SilentlyContinue).Enablelogging -eq 0) -and
             ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Office\15.0\osm' -ErrorAction SilentlyContinue).EnableUpload -eq 0))
        )
        diagnosticEventTracingDisabled = $diagEventTracingDisabled
        internetCommRestricted   = $internetCommRestricted

        # Phase E — hide-recent MRU surfaces. Cheap HKCU reads delegated to the
        # cleanup module getters so probe + Apply share one source of truth.
        hideQuickAccessRecent    = [bool](Get-QuickAccessRecentStatus).disabled
        hideQuickAccessFrequent  = [bool](Get-QuickAccessFrequentStatus).disabled
        hideRunMRU               = [bool](Get-RunMRUStatus).disabled
        disableSearchHistory     = [bool](Get-SearchHistoryStatus).disabled
        terminalHistoryDisabled  = [bool](Get-TerminalHistoryStatus).disabled

        # RDP stability
        rdpKeepAlive    = [bool]$rdpKeepAlive
        rdpNoTimeouts   = [bool]$rdpNoTimeouts
        rdpQosPriority  = [bool]$rdpQosPriority

        # Host hardening (Feature 4)
        systemRestoreOff  = [bool]((Get-Service VSS -ErrorAction SilentlyContinue)?.StartType -eq 'Disabled')
        crashDumpsOff     = [bool]((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl' -Name CrashDumpEnabled -ErrorAction SilentlyContinue)?.CrashDumpEnabled -eq 0)
        clipboardHistoryOff = [bool](
            ((Get-ItemProperty 'HKCU:\Software\Microsoft\Clipboard' -Name EnableClipboardHistory -ErrorAction SilentlyContinue)?.EnableClipboardHistory -eq 0) -or
            ((Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' -Name AllowClipboardHistory -ErrorAction SilentlyContinue)?.AllowClipboardHistory -eq 0)
        )
        requirePwOnResume = [bool]((Get-ItemProperty 'HKCU:\Control Panel\Desktop' -Name ScreenSaverIsSecure -ErrorAction SilentlyContinue)?.ScreenSaverIsSecure -eq "1")
        kernelDmaProtect  = [bool]((Get-CimInstance -ClassName Win32_DeviceGuard -Namespace root\Microsoft\Windows\DeviceGuard -ErrorAction SilentlyContinue)?.KernelDMAProtection -eq 2)
        acquisitionDriverBlocklist = [bool]$acqBlocklist
        forensicToolBlock          = [bool]$forensicToolBlock
        depEnabled                 = [bool]$depEnabled
        aslrMandatory              = [bool]$aslrMandatory
        aslrBottomUp               = [bool]$aslrBottomUp
        cfgEnabled                 = [bool]$cfgEnabled
        heapIntegrity              = [bool]$heapIntegrity
        sehopEnabled               = [bool]$sehopEnabled
        asrRulesEnabled               = [bool]$asrRulesEnabled
        controlledFolderAccessEnabled = [bool]$controlledFolderAccessEnabled
        networkProtectionEnabled      = [bool]$networkProtectionEnabled
        lidClosePowerOff           = [bool]$lidClosePowerOff
        # RAM-spill control (Feature 3): all three conditions must hold.
        # ClearPageFileAtShutdown=1, hibernation off, fast-startup off.
        ramSpillControl = [bool](
            ((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' -Name ClearPageFileAtShutdown -ErrorAction SilentlyContinue)?.ClearPageFileAtShutdown -eq 1) -and
            ((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Power' -Name HibernateEnabled -ErrorAction SilentlyContinue)?.HibernateEnabled -eq 0) -and
            ((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled -ErrorAction SilentlyContinue)?.HiberbootEnabled -eq 0)
        )
    }
}

# ============================================================================
# NEW: Memory Compression (from ReviOS)
# ============================================================================

function Disable-MemoryCompression {
    Assert-IsAdmin
    try {
        Disable-MMAgent -MemoryCompression -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-MemoryCompression {
    Assert-IsAdmin
    try {
        Enable-MMAgent -MemoryCompression -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: Win32PrioritySeparation (from ReviOS win32ps.yml)
# ============================================================================

function Set-Win32PrioritySeparation {
    Assert-IsAdmin
    try {
        # 38 = Short Quantum, variable, 3x foreground boost (recommended for desktop/gaming)
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\PriorityControl" -Name "Win32PrioritySeparation" -Value 38 -Type DWord -Force
        @{ status = "optimized"; value = 38 }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Reset-Win32PrioritySeparation {
    Assert-IsAdmin
    try {
        # 2 = Windows default (Long Quantum, Fixed)
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\PriorityControl" -Name "Win32PrioritySeparation" -Value 2 -Type DWord -Force
        @{ status = "reset"; value = 2 }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Desktop shell priority (all interactive users, including Windows Server/RDS)
# ============================================================================

$script:ShellPriorityTaskName = 'WinCommanderShellPriorityLogon'
$script:ShellPriorityDirectory = Join-Path $env:ProgramData 'WinCommander\ShellPriority'
$script:ShellPriorityScriptPath = Join-Path $script:ShellPriorityDirectory 'Apply-ShellPriority.ps1'
$script:ShellPriorityBackupPath = 'HKLM:\SOFTWARE\WinCommander\ShellPriorityBackup'
$script:ShellPriorityTargets = @(
    'explorer.exe', 'dwm.exe', 'sihost.exe', 'StartMenuExperienceHost.exe',
    'ShellExperienceHost.exe', 'SystemSettings.exe', 'Taskmgr.exe',
    'SearchHost.exe', 'SearchApp.exe'
)

$script:ShellPriorityTaskScript = @'
param([switch]$AtLogon)

$ErrorActionPreference = 'SilentlyContinue'
if ($AtLogon) { Start-Sleep -Seconds 5 }

if (-not ('WinCommander.ShellPriorityNative' -as [type])) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;
namespace WinCommander {
    public static class ShellPriorityNative {
        [DllImport("ntdll.dll")]
        public static extern int NtSetInformationProcess(
            IntPtr processHandle, int processInformationClass,
            ref int processInformation, int processInformationLength);
    }
}
"@
}

$targets = @(
    'explorer', 'dwm', 'sihost', 'StartMenuExperienceHost',
    'ShellExperienceHost', 'SystemSettings', 'Taskmgr', 'SearchHost', 'SearchApp'
)
foreach ($process in @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
    $targets -contains "$($_.ProcessName)"
})) {
    try { $process.PriorityClass = [Diagnostics.ProcessPriorityClass]::High } catch {}
    try {
        $ioPriority = 3 # IoPriorityHigh
        [WinCommander.ShellPriorityNative]::NtSetInformationProcess(
            $process.Handle, 33, [ref]$ioPriority, [Runtime.InteropServices.Marshal]::SizeOf([int])
        ) | Out-Null
    }
    catch {}
}
'@

function Save-ShellPriorityValue {
    param([Parameter(Mandatory = $true)][string]$Target, [Parameter(Mandatory = $true)][string]$Name)

    $backupName = "$Target.$Name"
    if (Get-ItemProperty -Path $script:ShellPriorityBackupPath -Name $backupName -ErrorAction SilentlyContinue) { return }
    $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$Target\PerfOptions"
    $value = (Get-ItemProperty -Path $path -Name $Name -ErrorAction SilentlyContinue).$Name
    $saved = if ($null -eq $value) { 0 } else { [int]$value }
    New-ItemProperty -Path $script:ShellPriorityBackupPath -Name $backupName -Value $saved -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $script:ShellPriorityBackupPath -Name "$backupName.Present" -Value $([int]($null -ne $value)) -PropertyType DWord -Force | Out-Null
}

function Restore-ShellPriorityValue {
    param([Parameter(Mandatory = $true)][string]$Target, [Parameter(Mandatory = $true)][string]$Name)

    $backupName = "$Target.$Name"
    $value = (Get-ItemProperty -Path $script:ShellPriorityBackupPath -Name $backupName -ErrorAction SilentlyContinue).$backupName
    $wasPresent = (Get-ItemProperty -Path $script:ShellPriorityBackupPath -Name "$backupName.Present" -ErrorAction SilentlyContinue)."$backupName.Present"
    $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$Target\PerfOptions"
    if ($null -eq $value -or $null -eq $wasPresent) { return }
    if ($wasPresent -eq 0) {
        Remove-ItemProperty -Path $path -Name $Name -Force -ErrorAction SilentlyContinue
    }
    else {
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name $Name -Value $value -Type DWord -Force
    }
}

function Set-DesktopShellPriority {
    Assert-IsAdmin
    try {
        if (!(Test-Path $script:ShellPriorityDirectory)) {
            New-Item -Path $script:ShellPriorityDirectory -ItemType Directory -Force | Out-Null
        }
        & icacls.exe $script:ShellPriorityDirectory /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not secure the shell-priority helper directory.' }

        if (!(Test-Path $script:ShellPriorityBackupPath)) { New-Item -Path $script:ShellPriorityBackupPath -Force | Out-Null }
        foreach ($target in $script:ShellPriorityTargets) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$target\PerfOptions"
            Save-ShellPriorityValue -Target $target -Name 'CpuPriorityClass'
            Save-ShellPriorityValue -Target $target -Name 'IoPriority'
            if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
            Set-ItemProperty -Path $path -Name 'CpuPriorityClass' -Value 3 -Type DWord -Force
            Set-ItemProperty -Path $path -Name 'IoPriority' -Value 3 -Type DWord -Force
        }

        Set-Content -LiteralPath $script:ShellPriorityScriptPath -Value $script:ShellPriorityTaskScript -Encoding UTF8 -Force
        & icacls.exe $script:ShellPriorityScriptPath /inheritance:r /grant:r '*S-1-5-18:(F)' '*S-1-5-32-544:(F)' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not secure the shell-priority helper.' }

        $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$script:ShellPriorityScriptPath`" -AtLogon"
        $trigger = New-ScheduledTaskTrigger -AtLogOn
        $trigger.Delay = 'PT5S'
        $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
        $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
        Register-ScheduledTask -TaskName $script:ShellPriorityTaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null

        & $script:ShellPriorityScriptPath
        @{ status = 'enabled'; scope = 'all-users'; taskName = $script:ShellPriorityTaskName }
    }
    catch {
        $message = $_.Exception.Message
        Reset-DesktopShellPriority | Out-Null
        @{ error = $true; message = $message }
    }
}

function Reset-DesktopShellPriority {
    Assert-IsAdmin
    try {
        Unregister-ScheduledTask -TaskName $script:ShellPriorityTaskName -Confirm:$false -ErrorAction SilentlyContinue
        foreach ($target in $script:ShellPriorityTargets) {
            Restore-ShellPriorityValue -Target $target -Name 'CpuPriorityClass'
            Restore-ShellPriorityValue -Target $target -Name 'IoPriority'
        }
        Remove-Item -Path $script:ShellPriorityBackupPath -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $script:ShellPriorityScriptPath -Force -ErrorAction SilentlyContinue
        @{ status = 'disabled'; scope = 'windows-managed' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: Service Timeouts (from ReviOS explorer.yml WaitToKillServiceTimeout)
# ============================================================================

function Set-OptimizedTimeouts {
    Assert-IsAdmin
    try {
        # Speed up shutdown: WaitToKillServiceTimeout = 1500ms (default 5000ms)
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control" -Name "WaitToKillServiceTimeout" -Value "1500" -Type String -Force
        # Speed up hung app kill: HungAppTimeout = 1500ms
        $dtPath = "HKCU:\Control Panel\Desktop"
        Set-ItemProperty -Path $dtPath -Name "HungAppTimeout" -Value "1500" -Type String -Force
        Set-ItemProperty -Path $dtPath -Name "WaitToKillAppTimeout" -Value "2000" -Type String -Force
        @{ status = "optimized" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Reset-OptimizedTimeouts {
    Assert-IsAdmin
    try {
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control" -Name "WaitToKillServiceTimeout" -Value "5000" -Type String -Force
        $dtPath = "HKCU:\Control Panel\Desktop"
        Remove-ItemSecure -Path $dtPath -Name "HungAppTimeout" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path $dtPath -Name "WaitToKillAppTimeout" -ErrorAction SilentlyContinue
        @{ status = "reset" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: Reserved Storage
# ============================================================================

function Disable-ReservedStorage {
    Assert-IsAdmin
    try {
        & DISM.exe /Online /Set-ReservedStorageState /State:Disabled 2>$null
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-ReservedStorage {
    Assert-IsAdmin
    try {
        & DISM.exe /Online /Set-ReservedStorageState /State:Enabled 2>$null
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: Boot & Kernel Tweaks
# ============================================================================

# --- TSX (Intel Transactional Synchronization Extensions) ---
function Enable-TSX {
    Assert-IsAdmin
    try {
        $cpu = (Get-CimInstance Win32_Processor).Manufacturer
        if ($cpu -eq "GenuineIntel") {
            $path = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Kernel"
            if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
            Set-ItemProperty -Path $path -Name "DisableTsx" -Value 0 -Type DWord -Force
            @{ status = "enabled" }
        } else {
            @{ status = "skipped"; message = "TSX is Intel-only. Non-Intel CPU detected." }
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-TSX {
    Assert-IsAdmin
    try {
        $path = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Kernel"
        Remove-ItemSecure -Path $path -Name "DisableTsx" -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- First Logon Animation ---
function Disable-FirstLogonAnimation {
    Assert-IsAdmin
    try {
        $path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System"
        Set-ItemProperty -Path $path -Name "EnableFirstLogonAnimation" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-FirstLogonAnimation {
    Assert-IsAdmin
    try {
        $path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System"
        Set-ItemProperty -Path $path -Name "EnableFirstLogonAnimation" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Startup Sound ---
function Disable-StartupSound {
    Assert-IsAdmin
    try {
        $path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System"
        Set-ItemProperty -Path $path -Name "DisableStartupSound" -Value 1 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-StartupSound {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System" -Name "DisableStartupSound" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Auto-Restart Sign-On ---
function Disable-AutoRestartSignon {
    Assert-IsAdmin
    try {
        $path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System"
        Set-ItemProperty -Path $path -Name "DisableAutomaticRestartSignOn" -Value 1 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutoRestartSignon {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System" -Name "DisableAutomaticRestartSignOn" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Auto-Reboot on BSOD ---
function Disable-AutoRebootOnBSOD {
    Assert-IsAdmin
    try {
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl" -Name "AutoReboot" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutoRebootOnBSOD {
    Assert-IsAdmin
    try {
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl" -Name "AutoReboot" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Small Memory Dump (crash dump type) ---
function Set-SmallMemoryDump {
    Assert-IsAdmin
    try {
        # 3 = Small memory dump (64KB) - saves disk space
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl" -Name "CrashDumpEnabled" -Value 3 -Type DWord -Force
        @{ status = "small" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Reset-SmallMemoryDump {
    Assert-IsAdmin
    try {
        # 7 = Automatic memory dump (Windows default)
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl" -Name "CrashDumpEnabled" -Value 7 -Type DWord -Force
        @{ status = "automatic" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Content Delivery Manager (CDM) — Full Nuke
# ============================================================================

function Disable-ContentDeliveryManager {
    Assert-IsAdmin
    try {
        $cdmPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"
        $keys = @(
            "ContentDeliveryAllowed", "OemPreInstalledAppsEnabled",
            "PreInstalledAppsEnabled", "PreInstalledAppsEverEnabled",
            "SilentInstalledAppsEnabled", "SoftLandingEnabled",
            "SystemPaneSuggestionsEnabled", "RotatingLockScreenEnabled",
            "RotatingLockScreenOverlayEnabled", "FeatureManagementEnabled",
            "SubscribedContent-310093Enabled", "SubscribedContent-314563Enabled",
            "SubscribedContent-338387Enabled", "SubscribedContent-338388Enabled",
            "SubscribedContent-338389Enabled", "SubscribedContent-338393Enabled",
            "SubscribedContent-353694Enabled", "SubscribedContent-353696Enabled",
            "SubscribedContent-353698Enabled"
        )
        foreach ($name in $keys) {
            Set-RegistryValueSafe -Path $cdmPath -Name $name -Value 0 -Type DWord
        }
        # Delete Subscription/SuggestedApps
        Remove-ItemSecure -Path "$cdmPath\Subscriptions" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "$cdmPath\SuggestedApps" -Recurse -Force -ErrorAction SilentlyContinue
        # Policy-level feature management kill
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableWindowsConsumerFeatures" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableCloudOptimizedContent" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableSoftLanding" -Value 1 -Type DWord
        @{ status = "nuked" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-ContentDeliveryManager {
    try {
        $cdmPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"
        $keys = @("ContentDeliveryAllowed", "SilentInstalledAppsEnabled", "SoftLandingEnabled", "SystemPaneSuggestionsEnabled", "FeatureManagementEnabled")
        foreach ($name in $keys) {
            Set-RegistryValueSafe -Path $cdmPath -Name $name -Value 1 -Type DWord
        }
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableWindowsConsumerFeatures" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableCloudOptimizedContent" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableSoftLanding" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# IFEO CPU Priority Tuning (SearchIndexer, fontdrvhost, lsass)
# ============================================================================

function Enable-IFEOPriorityTuning {
    Assert-IsAdmin
    try {
        $targets = @(
            @{ Exe = "SearchIndexer.exe"; Priority = 1 },   # Below Normal = 1 (IDLE_PRIORITY_CLASS)
            @{ Exe = "fontdrvhost.exe"; Priority = 1 },
            @{ Exe = "SearchProtocolHost.exe"; Priority = 1 },
            @{ Exe = "SearchFilterHost.exe"; Priority = 1 }
        )
        foreach ($t in $targets) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$($t.Exe)\PerfOptions"
            if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
            Set-ItemProperty -Path $path -Name "CpuPriorityClass" -Value $t.Priority -Type DWord -Force
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-IFEOPriorityTuning {
    Assert-IsAdmin
    try {
        $exes = @("SearchIndexer.exe", "fontdrvhost.exe", "SearchProtocolHost.exe", "SearchFilterHost.exe")
        foreach ($exe in $exes) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$exe\PerfOptions"
            Remove-ItemSecure -Path $path -Recurse -Force -ErrorAction SilentlyContinue
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Disable Auto Disk Check on Boot
# ============================================================================

function Disable-AutoDiskCheck {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager" -Name "BootExecute" -Value "autocheck autochk /p \??\C:" -Type String
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutoDiskCheck {
    Assert-IsAdmin
    try {
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager" -Name "BootExecute" -Value "autocheck autochk *" -Type MultiString -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# SvcHost Split Optimization (isolate services into own svchost.exe)
# ============================================================================

function Enable-SvcHostSplit {
    Assert-IsAdmin
    try {
        $ram = (Get-CimInstance Win32_PhysicalMemory | Measure-Object -Property Capacity -Sum).Sum
        $ramKB = [math]::Round($ram / 1KB)
        Set-RegistryValueSafe -Path "HKLM:\SYSTEM\CurrentControlSet\Control" -Name "SvcHostSplitThresholdInKB" -Value $ramKB -Type DWord
        @{ status = "enabled"; thresholdKB = $ramKB }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-SvcHostSplit {
    Assert-IsAdmin
    try {
        # Default: 380000 (KB) ≈ ~380MB
        Set-RegistryValueSafe -Path "HKLM:\SYSTEM\CurrentControlSet\Control" -Name "SvcHostSplitThresholdInKB" -Value 380000 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NGen Pre-compilation of .NET Assemblies
# ============================================================================

function Invoke-NGenPrecompile {
    Assert-IsAdmin
    try {
        $ngenPaths = @(
            "$env:SystemRoot\Microsoft.NET\Framework\v4.0.30319\ngen.exe",
            "$env:SystemRoot\Microsoft.NET\Framework64\v4.0.30319\ngen.exe"
        )
        foreach ($ngen in $ngenPaths) {
            if (Test-Path $ngen) {
                Start-Process -FilePath $ngen -ArgumentList "executeQueuedItems" -Wait -NoNewWindow -ErrorAction SilentlyContinue
            }
        }
        @{ status = "completed" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# DISM Optional Feature Management
# ============================================================================

function Enable-DirectPlay {
    Assert-IsAdmin
    try {
        dism /Online /Enable-Feature /FeatureName:DirectPlay /All /NoRestart 2>$null
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-PowerShellV2 {
    Assert-IsAdmin
    try {
        dism /Online /Disable-Feature /FeatureName:MicrosoftWindowsPowerShellV2Root /NoRestart 2>$null
        dism /Online /Disable-Feature /FeatureName:MicrosoftWindowsPowerShellV2 /NoRestart 2>$null
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-PowerShellV2 {
    Assert-IsAdmin
    try {
        dism /Online /Enable-Feature /FeatureName:MicrosoftWindowsPowerShellV2Root /NoRestart 2>$null
        dism /Online /Enable-Feature /FeatureName:MicrosoftWindowsPowerShellV2 /NoRestart 2>$null
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-PrintingSubsystem {
    Assert-IsAdmin
    try {
        dism /Online /Disable-Feature /FeatureName:Printing-Foundation-Features /NoRestart 2>$null
        Stop-Service -Name "Spooler" -Force -ErrorAction SilentlyContinue
        Set-Service -Name "Spooler" -StartupType Disabled -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-PrintingSubsystem {
    Assert-IsAdmin
    try {
        dism /Online /Enable-Feature /FeatureName:Printing-Foundation-Features /NoRestart 2>$null
        Set-Service -Name "Spooler" -StartupType Automatic -ErrorAction SilentlyContinue
        Start-Service -Name "Spooler" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-WorkFoldersClient {
    Assert-IsAdmin
    try {
        dism /Online /Disable-Feature /FeatureName:WorkFolders-Client /NoRestart 2>$null
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-WorkFoldersClient {
    Assert-IsAdmin
    try {
        dism /Online /Enable-Feature /FeatureName:WorkFolders-Client /NoRestart 2>$null
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Windows Update Control (Extended)
# ============================================================================

function Disable-DeliveryOptimization {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization" -Name "DODownloadMode" -Value 0 -Type DWord
        Stop-Service -Name "DoSvc" -Force -ErrorAction SilentlyContinue
        Set-Service -Name "DoSvc" -StartupType Disabled -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-DeliveryOptimization {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization" -Name "DODownloadMode" -ErrorAction SilentlyContinue
        Set-Service -Name "DoSvc" -StartupType Automatic -ErrorAction SilentlyContinue
        Start-Service -Name "DoSvc" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-DriverAutoUpdate {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate" -Name "ExcludeWUDriversInQualityUpdate" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DriverSearching" -Name "SearchOrderConfig" -Value 0 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-DriverAutoUpdate {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate" -Name "ExcludeWUDriversInQualityUpdate" -ErrorAction SilentlyContinue
        Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DriverSearching" -Name "SearchOrderConfig" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-StoreAutoDownload {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore" -Name "AutoDownload" -Value 2 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore" -Name "DisableOSUpgrade" -Value 1 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-StoreAutoDownload {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore" -Name "AutoDownload" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\WindowsStore" -Name "DisableOSUpgrade" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-UpdateNotifications {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate" -Name "SetAutoRestartNotificationDisable" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings" -Name "RestartNotificationsAllowed2" -Value 0 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-UpdateNotifications {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate" -Name "SetAutoRestartNotificationDisable" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings" -Name "RestartNotificationsAllowed2" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Block-DevHomeOutlookAutoInstall {
    Assert-IsAdmin
    try {
        # Block via IFEO
        $apps = @("DevHome.exe", "OutlookInstaller.exe", "BingChatInstaller.exe")
        foreach ($app in $apps) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$app"
            if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
            Set-ItemProperty -Path $path -Name "Debugger" -Value "%SystemRoot%\System32\systray.exe" -Type String -Force
        }
        @{ status = "blocked" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Unblock-DevHomeOutlookAutoInstall {
    Assert-IsAdmin
    try {
        $apps = @("DevHome.exe", "OutlookInstaller.exe", "BingChatInstaller.exe")
        foreach ($app in $apps) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$app"
            Remove-ItemSecure -Path $path -Recurse -Force -ErrorAction SilentlyContinue
        }
        @{ status = "unblocked" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Miscellaneous (ReviOS parity)
# ============================================================================

# --- Classic Windows Photo Viewer ---
function Enable-ClassicPhotoViewer {
    Assert-IsAdmin
    try {
        $types = @(".bmp", ".gif", ".ico", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".wdp")
        foreach ($ext in $types) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows Photo Viewer\Capabilities\FileAssociations"
            if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
            Set-ItemProperty -Path $path -Name $ext -Value "PhotoViewer.FileAssoc.Tiff" -Type String -Force
        }
        # Register PhotoViewer DLL
        $dllPath = "%SystemRoot%\System32\rundll32.exe `"%ProgramFiles%\Windows Photo Viewer\PhotoViewer.dll`", ImageView_Fullscreen %1"
        $shellPath = "HKLM:\SOFTWARE\Classes\PhotoViewer.FileAssoc.Tiff\shell\open\command"
        if (!(Test-Path $shellPath)) { New-Item -Path $shellPath -Force | Out-Null }
        Set-ItemProperty -Path $shellPath -Name "(Default)" -Value $dllPath -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-ClassicPhotoViewer {
    try {
        $types = @(".bmp", ".gif", ".ico", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".wdp")
        foreach ($ext in $types) {
            Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\Windows Photo Viewer\Capabilities\FileAssociations" -Name $ext -ErrorAction SilentlyContinue
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Audio Ducking (Do Nothing for Communications) ---
function Disable-AudioDucking {
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Multimedia\Audio" -Name "UserDuckingPreference" -Value 3 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AudioDucking {
    try {
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Multimedia\Audio" -Name "UserDuckingPreference" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- UTC Time (RealTimeIsUniversal for dual-boot) ---
function Enable-UTCTime {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SYSTEM\CurrentControlSet\Control\TimeZoneInformation" -Name "RealTimeIsUniversal" -Value 1 -Type DWord
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-UTCTime {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SYSTEM\CurrentControlSet\Control\TimeZoneInformation" -Name "RealTimeIsUniversal" -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- EnableLinkedConnections (mapped drives work with UAC) ---
function Enable-LinkedConnections {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "EnableLinkedConnections" -Value 1 -Type DWord
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-LinkedConnections {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "EnableLinkedConnections" -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Unlimited Password Age ---
function Set-UnlimitedPasswordAge {
    Assert-IsAdmin
    try {
        net accounts /maxpwage:unlimited 2>$null
        @{ status = "unlimited" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Reset-PasswordAge {
    Assert-IsAdmin
    try {
        net accounts /maxpwage:42 2>$null
        @{ status = "42days" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- MSI Installer in Safe Mode ---
function Enable-MSISafeMode {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SYSTEM\CurrentControlSet\Control\SafeBoot\Minimal\MSIServer" -Name "(Default)" -Value "Service" -Type String
        Set-RegistryValueSafe -Path "HKLM:\SYSTEM\CurrentControlSet\Control\SafeBoot\Network\MSIServer" -Name "(Default)" -Value "Service" -Type String
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-MSISafeMode {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SYSTEM\CurrentControlSet\Control\SafeBoot\Minimal\MSIServer" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SYSTEM\CurrentControlSet\Control\SafeBoot\Network\MSIServer" -Recurse -Force -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy & Telemetry
# ============================================================================

# --- Advertising ID ---
function Disable-AdvertisingID {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo" -Name "Enabled" -Value 0 -Type DWord
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo" -Name "Id" -ErrorAction SilentlyContinue
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo" -Name "DisabledByGroupPolicy" -Value 1 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AdvertisingID {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo" -Name "Enabled" -Value 1 -Type DWord
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo" -Name "DisabledByGroupPolicy" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Tailored Experiences ---
function Disable-TailoredExperiences {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Privacy" -Name "TailoredExperiencesWithDiagnosticDataEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableTailoredExperiencesWithDiagnosticData" -Value 1 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-TailoredExperiences {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Privacy" -Name "TailoredExperiencesWithDiagnosticDataEnabled" -Value 1 -Type DWord
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableTailoredExperiencesWithDiagnosticData" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Office Logging ---
function Disable-OfficeLogging {
    Assert-IsAdmin
    try {
        foreach ($ver in @('15.0', '16.0')) {
            Set-RegistryValueSafe -Path "HKCU:\Software\Policies\Microsoft\office\$ver\osm" -Name "Enablelogging" -Value 0 -Type DWord
            Set-RegistryValueSafe -Path "HKCU:\Software\Policies\Microsoft\office\$ver\osm" -Name "EnableUpload" -Value 0 -Type DWord
        }
        Set-RegistryValueSafe -Path "HKCU:\Software\Policies\Microsoft\office\common\clienttelemetry" -Name "DisableTelemetry" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration" -Name "ClientTelemetry" -Value 0 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-OfficeLogging {
    Assert-IsAdmin
    try {
        foreach ($ver in @('15.0', '16.0')) {
            Remove-ItemSecure -Path "HKCU:\Software\Policies\Microsoft\office\$ver\osm" -Name "Enablelogging" -ErrorAction SilentlyContinue
            Remove-ItemSecure -Path "HKCU:\Software\Policies\Microsoft\office\$ver\osm" -Name "EnableUpload" -ErrorAction SilentlyContinue
        }
        Remove-ItemSecure -Path "HKCU:\Software\Policies\Microsoft\office\common\clienttelemetry" -Name "DisableTelemetry" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration" -Name "ClientTelemetry" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# KT: Removed dead Disable-DiagnosticTracing / Enable-DiagnosticTracing.
# These were never wired into the dispatcher (registry calls
# Disable-DiagnosticEventTracing in privacy/telemetry.ps1) and used a
# different, partially-bogus logger list (SleepStudyTraceSession, PowerMeter
# don't exist as autologger keys). Canonical list lives in
# privacy/telemetry.ps1's $Script:WC_DIAG_ETW_LOGGERS.

# ============================================================================
# System Restore Point
# ============================================================================

function Create-RestorePoint {
    Assert-IsAdmin
    try {
        # Enable System Protection on C: if it's not already on
        Enable-ComputerRestore -Drive "C:\" -ErrorAction SilentlyContinue
        # Bypass the 24-hour cooldown by touching the registry
        $srKey = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore'
        $prev  = (Get-ItemProperty -Path $srKey -Name 'SystemRestorePointCreationFrequency' -ErrorAction SilentlyContinue).SystemRestorePointCreationFrequency
        Set-ItemProperty -Path $srKey -Name 'SystemRestorePointCreationFrequency' -Value 0 -Type DWord -Force
        Checkpoint-Computer -Description 'WinCommander Pre-Setup' -RestorePointType 'MODIFY_SETTINGS'
        # Restore original cooldown value
        if ($null -ne $prev) {
            Set-ItemProperty -Path $srKey -Name 'SystemRestorePointCreationFrequency' -Value $prev -Type DWord -Force
        } else {
            Remove-ItemProperty -Path $srKey -Name 'SystemRestorePointCreationFrequency' -ErrorAction SilentlyContinue
        }
        @{ status = "created" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- RDP STABILITY ---
#
# The full RDP-stability triplet (keepalive / no-timeouts / QoS priority,
# 9 functions total) is paid (A-2 split) and moved to
# commander-pro/src/handlers.rs. Free's dispatch routes them via
# dispatch_paid_command before any local module loads. Strings-grep CI
# gate (A-5) verifies these function names no longer appear in Free's
# encrypted .enc bundle after build.

# ============================================================================
# RDP Incoming Session Idle Sign-Out (paid)
# Server-enforced Group Policy: signs out (not just disconnects) idle
# inbound RDP sessions. Applies to THIS machine acting as an RDP host.
#
# Registry: HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services
#   MaxIdleTime          (DWORD, ms)  idle threshold; 0 = no limit
#   fResetBroken         (DWORD)      1 = sign out at limit, 0 = disconnect only
#   MaxDisconnectionTime (DWORD, ms)  caps how long a disconnected session persists
#
# gpupdate /force is called after each change so the policy is active
# immediately without a reboot.
# ============================================================================

# Registry note: the 2-minute "you'll be signed out" warning that RDS shows at
# the idle limit is hardcoded and cannot be shortened. fEnableTimeoutWarning=0
# (WinStations\RDP-Tcp) disables it so sign-out is immediate. Canonical unit is
# SECONDS; sub-minute is best-effort (Windows idle resolution is ~1 minute).
$script:RdpWinStationPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp'

function Get-RdpIncomingIdleStatus {
    $tsPath  = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
    $maxIdle = (Get-ItemProperty -Path $tsPath -Name 'MaxIdleTime'   -ErrorAction SilentlyContinue).MaxIdleTime
    $maxDisc = (Get-ItemProperty -Path $tsPath -Name 'MaxDisconnectionTime' -ErrorAction SilentlyContinue).MaxDisconnectionTime
    $maxConn = (Get-ItemProperty -Path $tsPath -Name 'MaxConnectionTime' -ErrorAction SilentlyContinue).MaxConnectionTime
    $resetBr = (Get-ItemProperty -Path $tsPath -Name 'fResetBroken'  -ErrorAction SilentlyContinue).fResetBroken
    $denyRdp = (Get-ItemProperty -Path $tsPath -Name 'fDenyTSConnections' -ErrorAction SilentlyContinue).fDenyTSConnections
    $warn    = (Get-ItemProperty -Path $script:RdpWinStationPath -Name 'fEnableTimeoutWarning' -ErrorAction SilentlyContinue).fEnableTimeoutWarning
    if ($maxIdle -gt 0 -and $maxDisc -gt 0 -and $maxConn -gt 0 -and $resetBr -eq 1 -and $denyRdp -eq 0) {
        @{ enabled = $true;  seconds = [math]::Round($maxIdle / 1000); minutes = [math]::Round($maxIdle / 60000); maxIdleTimeMs = $maxIdle; maxDisconnectionTimeMs = $maxDisc; maxConnectionTimeMs = $maxConn; fResetBroken = $resetBr; fDenyTSConnections = $denyRdp; warningEnabled = ($warn -ne 0) }
    } else {
        @{ enabled = $false; seconds = $null; minutes = $null;        maxIdleTimeMs = $maxIdle; maxDisconnectionTimeMs = $maxDisc; maxConnectionTimeMs = $maxConn; fResetBroken = $resetBr; fDenyTSConnections = $denyRdp; warningEnabled = ($warn -ne 0) }
    }
}

function Enable-RdpIncomingIdleTimeout {
    param([int]$Seconds = 0, [int]$Minutes = 0)
    Assert-IsAdmin
    try {
        if ($Seconds -le 0) { $Seconds = if ($Minutes -gt 0) { $Minutes * 60 } else { 900 } }
        if ($Seconds -lt 10)    { $Seconds = 10    }
        if ($Seconds -gt 86400) { $Seconds = 86400 }
        $ms     = $Seconds * 1000
        $tsPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
        if (!(Test-Path $tsPath)) { New-Item -Path $tsPath -Force | Out-Null }
        Set-ItemProperty -Path $tsPath -Name 'fDenyTSConnections'    -Value 0  -Type DWord -Force
        Set-ItemProperty -Path $tsPath -Name 'MaxIdleTime'          -Value $ms -Type DWord -Force
        Set-ItemProperty -Path $tsPath -Name 'fResetBroken'         -Value 1  -Type DWord -Force
        Set-ItemProperty -Path $tsPath -Name 'MaxDisconnectionTime' -Value $ms -Type DWord -Force
        Set-ItemProperty -Path $tsPath -Name 'MaxConnectionTime'    -Value $ms -Type DWord -Force
        if (Test-Path $script:RdpWinStationPath) { Set-ItemProperty -Path $script:RdpWinStationPath -Name 'fEnableTimeoutWarning' -Value 0 -Type DWord -Force }
        & gpupdate /force /target:computer 2>&1 | Out-Null
        $observed = Get-RdpIncomingIdleStatus
        if (-not $observed.enabled -or $observed.seconds -ne $Seconds) { throw 'Windows read-back did not match the requested RDP Incoming policy' }
        $observed
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-RdpIncomingIdleTimeout {
    Assert-IsAdmin
    try {
        $tsPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
        if (Test-Path $tsPath) {
            Remove-ItemProperty -Path $tsPath -Name 'fDenyTSConnections'    -ErrorAction SilentlyContinue
            Remove-ItemProperty -Path $tsPath -Name 'MaxIdleTime'          -ErrorAction SilentlyContinue
            Remove-ItemProperty -Path $tsPath -Name 'fResetBroken'         -ErrorAction SilentlyContinue
            Remove-ItemProperty -Path $tsPath -Name 'MaxDisconnectionTime' -ErrorAction SilentlyContinue
            Remove-ItemProperty -Path $tsPath -Name 'MaxConnectionTime'    -ErrorAction SilentlyContinue
        }
        # Restore Windows' default (warning on) so removing the feature is a clean revert.
        if (Test-Path $script:RdpWinStationPath) { Set-ItemProperty -Path $script:RdpWinStationPath -Name 'fEnableTimeoutWarning' -Value 1 -Type DWord -Force }
        & gpupdate /force /target:computer 2>&1 | Out-Null
        $observed = Get-RdpIncomingIdleStatus
        if ($observed.enabled) { throw 'Windows read-back still reports an active RDP Incoming policy' }
        $observed
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── RDP Stability Toggles ─────────────────────────────────────────────────────
# Safe substring helper for fixed-width column parsing (quser / qwinsta output).
$script:SafeSliceRdp = {
    param([string]$s, [int]$start, [int]$end)
    if ($start -lt 0 -or $start -ge $s.Length) { return '' }
    $len = [Math]::Min($end - $start, $s.Length - $start)
    if ($len -le 0) { return '' }
    $s.Substring($start, $len).Trim()
}

function Enable-RdpKeepAlive {
    Assert-IsAdmin
    try {
        $tsCtrl = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server'
        Set-ItemProperty -Path $tsCtrl -Name 'KeepAliveEnable'       -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $tsCtrl -Name 'KeepAliveInterval'     -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $tsCtrl -Name 'fSingleSessionPerUser' -Value 1 -Type DWord -Force
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-RdpKeepAlive {
    Assert-IsAdmin
    try {
        $tsCtrl = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server'
        Set-ItemProperty -Path $tsCtrl -Name 'KeepAliveEnable'       -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $tsCtrl -Name 'fSingleSessionPerUser' -Value 0 -Type DWord -Force
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RdpKeepAliveStatus {
    $tsCtrl   = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server'
    $kaEnable = (Get-ItemProperty -Path $tsCtrl -Name 'KeepAliveEnable'       -ErrorAction SilentlyContinue).KeepAliveEnable
    $kaSingle = (Get-ItemProperty -Path $tsCtrl -Name 'fSingleSessionPerUser' -ErrorAction SilentlyContinue).fSingleSessionPerUser
    @{ enabled = [bool]($kaEnable -eq 1 -and $kaSingle -eq 1) }
}

function Enable-RdpNoTimeouts {
    Assert-IsAdmin
    try {
        $tsPol = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
        if (!(Test-Path $tsPol)) { New-Item -Path $tsPol -Force | Out-Null }
        Set-ItemProperty -Path $tsPol -Name 'MaxIdleTime'          -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $tsPol -Name 'MaxDisconnectionTime' -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $tsPol -Name 'MaxConnectionTime'    -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $tsPol -Name 'fResetBroken'         -Value 0 -Type DWord -Force
        & gpupdate /force /target:computer 2>&1 | Out-Null
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-RdpNoTimeouts {
    Assert-IsAdmin
    try {
        $tsPol = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
        if (Test-Path $tsPol) {
            Remove-ItemProperty -Path $tsPol -Name 'MaxIdleTime'          -ErrorAction SilentlyContinue
            Remove-ItemProperty -Path $tsPol -Name 'MaxDisconnectionTime' -ErrorAction SilentlyContinue
            Remove-ItemProperty -Path $tsPol -Name 'MaxConnectionTime'    -ErrorAction SilentlyContinue
            Remove-ItemProperty -Path $tsPol -Name 'fResetBroken'         -ErrorAction SilentlyContinue
        }
        & gpupdate /force /target:computer 2>&1 | Out-Null
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RdpNoTimeoutsStatus {
    $tsPol    = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
    $polProps = Get-ItemProperty -Path $tsPol -ErrorAction SilentlyContinue
    @{ enabled = [bool]($polProps -and $polProps.MaxDisconnectionTime -eq 0 -and $polProps.MaxIdleTime -eq 0 -and $polProps.MaxConnectionTime -eq 0 -and $polProps.fResetBroken -eq 0) }
}

function Enable-RdpQosPriority {
    Assert-IsAdmin
    try {
        if (-not (Get-NetQosPolicy -Name 'RDP Priority'     -ErrorAction SilentlyContinue)) {
            New-NetQosPolicy -Name 'RDP Priority'     -IPDstPortStart 3389 -IPDstPortEnd 3389 -IPProtocol TCP -DSCPAction 46 -ErrorAction Stop | Out-Null
        }
        if (-not (Get-NetQosPolicy -Name 'RDP Priority Out' -ErrorAction SilentlyContinue)) {
            New-NetQosPolicy -Name 'RDP Priority Out' -IPSrcPortStart 3389 -IPSrcPortEnd 3389 -IPProtocol TCP -DSCPAction 46 -ErrorAction Stop | Out-Null
        }
        & gpupdate /force 2>&1 | Out-Null
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-RdpQosPriority {
    Assert-IsAdmin
    try {
        Remove-NetQosPolicy -Name 'RDP Priority'     -Confirm:$false -ErrorAction SilentlyContinue
        Remove-NetQosPolicy -Name 'RDP Priority Out' -Confirm:$false -ErrorAction SilentlyContinue
        & gpupdate /force 2>&1 | Out-Null
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RdpQosPriorityStatus {
    $qosIn  = Get-NetQosPolicy -Name 'RDP Priority'     -ErrorAction SilentlyContinue
    $qosOut = Get-NetQosPolicy -Name 'RDP Priority Out' -ErrorAction SilentlyContinue
    @{ enabled = [bool]($qosIn -and $qosOut) }
}

# Parse quser idle string ("none", "0:05", "1:23", "2+03:00") → whole minutes.
function ConvertFrom-QuserIdleMinutesRdp {
    param([string]$s)
    if ([string]::IsNullOrWhiteSpace($s) -or $s -eq 'none' -or $s -eq '.') { return 0 }
    if ($s -match '^(\d+)\+(\d+):(\d+)$') { return [int]$Matches[1] * 1440 + [int]$Matches[2] * 60 + [int]$Matches[3] }
    if ($s -match '^(\d+):(\d+)$')         { return [int]$Matches[1] * 60  + [int]$Matches[2] }
    if ($s -match '^(\d+)$')               { return [int]$Matches[1] }
    return 0
}

function Initialize-RdpIncomingHostIdleApi {
    if (([System.Management.Automation.PSTypeName]'WC.RdpIncomingHostIdle').Type) { return }
    Add-Type @"
using System;
using System.Runtime.InteropServices;

namespace WC {
  public static class RdpIncomingHostIdle {
    [DllImport("user32.dll")]
    private static extern bool GetLastInputInfo(ref LASTINPUTINFO plii);

    [DllImport("kernel32.dll")]
    private static extern uint GetTickCount();

    [StructLayout(LayoutKind.Sequential)]
    private struct LASTINPUTINFO {
      public uint cbSize;
      public uint dwTime;
    }

    public static int GetIdleSeconds() {
      var info = new LASTINPUTINFO();
      info.cbSize = (uint)Marshal.SizeOf(typeof(LASTINPUTINFO));
      if (!GetLastInputInfo(ref info)) return -1;
      uint elapsedMs = GetTickCount() - info.dwTime;
      return (int)(elapsedMs / 1000);
    }
  }
}
"@
}

function Get-RdpIncomingHostIdleSeconds {
    try {
        Initialize-RdpIncomingHostIdleApi
        $seconds = [WC.RdpIncomingHostIdle]::GetIdleSeconds()
        if ($seconds -ge 0) { return [int]$seconds }
    } catch {}
    return 0
}

function Watch-RdpIncomingSessions {
    $rawLines   = [System.Collections.Generic.List[string]]::new()
    $sessions   = [System.Collections.Generic.List[hashtable]]::new()
    $queryError = ''
    try {
        $hostIdleSeconds = Get-RdpIncomingHostIdleSeconds
        $currentSessionId = -1
        try { $currentSessionId = [System.Diagnostics.Process]::GetCurrentProcess().SessionId } catch {}

        # quser → per-session-id idle/state/user. Regex-anchored (not fixed-width
        # column slicing) so it survives long usernames, locale spacing and the
        # leading '>' on the current session. Captures: user [sessname] id state idle logon.
        $quserById = @{}
        try {
            $quserRaw = & quser 2>&1
            foreach ($l in $quserRaw) { $rawLines.Add([string]$l) }
            foreach ($ql in $quserRaw) {
                $line = [string]$ql
                if (-not $line -or $line -match 'USERNAME') { continue }
                if ($line -match '^\s*>?\s*(\S.*?)(?:\s+(rdp-tcp#\d+|console|rdp-tcp))?\s+(\d+)\s+(\w+)\s+(\S+)\s+(.+)$') {
                    $qid = [int]$Matches[3]
                    $quserById[$qid] = @{
                        username    = $Matches[1].Trim()
                        sessionName = if ($Matches[2]) { $Matches[2] } else { '' }
                        state       = $Matches[4]
                        idleStr     = $Matches[5]
                    }
                }
            }
        } catch { $queryError = "quser: $($_.Exception.Message)" }

        # qwinsta is the AUTHORITATIVE incoming-session list: it shows the
        # rdp-tcp#N name for BOTH Active and Disconnected sessions (quser leaves
        # SESSIONNAME blank for Disc). It has no idle column, so idle is taken
        # from the quser map above by session id. Anchored on the literal
        # 'rdp-tcp#<n>' token and stops at STATE (qwinsta Active rows have no
        # TYPE field — requiring one silently drops every active session).
        $byId = @{}
        try {
            $qwRaw = & qwinsta 2>&1
            if ($rawLines.Count -eq 0) { foreach ($l in $qwRaw) { $rawLines.Add([string]$l) } }
            foreach ($wl in $qwRaw) {
                $line = [string]$wl
                if (-not $line -or $line -match 'SESSIONNAME') { continue }
                if ($line -match '(rdp-tcp#\d+)\s+(.+?)\s+(\d+)\s+(\w+)') {
                    $wid = [int]$Matches[3]
                    $q   = $quserById[$wid]
                    $byId[$wid] = @{
                        username    = if ($q) { $q.username } else { $Matches[2] }
                        sessionName = $Matches[1]
                        state       = $Matches[4]
                        idleStr     = if ($q -and $q.idleStr) { $q.idleStr } else { 'none' }
                    }
                }
            }
        } catch {}

        # Fallback: if qwinsta yielded nothing, use any rdp-tcp# sessions quser saw.
        if ($byId.Count -eq 0) {
            foreach ($kv in $quserById.GetEnumerator()) {
                if ($kv.Value.sessionName -match '^rdp-tcp#') { $byId[$kv.Key] = $kv.Value }
            }
        }

        foreach ($kvp in $byId.GetEnumerator()) {
            $info    = $kvp.Value
            $idleStr = $info.idleStr
            $sid     = [int]$kvp.Key
            # Per-session idle from quser's IDLE TIME (whole minutes → seconds).
            # Coarse, but correctly attributed to THIS session.
            $quserIdleSeconds = (ConvertFrom-QuserIdleMinutesRdp $idleStr) * 60
            # GetLastInputInfo only measures the session THIS process runs in, so
            # use its precise per-second value for the current session (the live
            # countdown that must advance) and the quser value for the others —
            # never attribute host idle to a different user's session.
            $effectiveIdle = if ($sid -eq $currentSessionId) { [Math]::Max($hostIdleSeconds, $quserIdleSeconds) } else { $quserIdleSeconds }
            $sessions.Add(@{
                sessionId        = $sid
                username         = $info.username
                sessionName      = $info.sessionName
                state            = $info.state
                idleSeconds      = [int]$effectiveIdle
                idleMinutes      = [math]::Floor($effectiveIdle / 60)
                idleDisplay      = $idleStr
                hostIdleSeconds  = $hostIdleSeconds
                isCurrentSession = ($sid -eq $currentSessionId)
            })
        }
    } catch { $queryError = $_.Exception.Message }

    @{
        sessions   = if ($sessions.Count -gt 0) { $sessions.ToArray() } else { @() }
        rawLines   = $rawLines.ToArray()
        queryError = $queryError
    }
}

function Logoff-RdpIncomingSession {
    param([int]$SessionId)
    Assert-IsAdmin
    try {
        & logoff $SessionId 2>&1 | Out-Null
        @{ success = $true }
    } catch { @{ success = $false; error = $_.Exception.Message } }
}

