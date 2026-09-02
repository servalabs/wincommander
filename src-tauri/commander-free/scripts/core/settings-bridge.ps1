# settings-bridge.ps1
# ─────────────────────────────────────────────────────────────────────
# PowerShell registry probes used by backend command modules.
#
# Settings are encrypted and split between ProgramData and LocalAppData by the
# Rust datastore. PowerShell must not read or mutate a made-up JSON shadow copy:
# it only reports observed Windows state to Rust/the renderer.
# ─────────────────────────────────────────────────────────────────────

function Get-WCMigrationData {
    <#
    .SYNOPSIS
        Collects current state from all registry sources for initial migration.
        Returns a hashtable matching the settings.json schema with current values.
        Called by the frontend during first-run migration via Invoke-BackendCommand.
    #>

    $migration = @{
        privacy  = @{
            telemetry  = @{
                windowsDisabled            = $null
                officeDisabled             = $null
                copilotDisabled            = $null
                activityHistoryDisabled    = $null
                locationTrackingDisabled   = $null
                windowsSuggestionsDisabled = $null
            }
            clipboard  = @{
                historyDisabled   = $null
                cloudSyncDisabled = $null
            }
            tracking   = @{
                recentFilesDisabled = $null
                jumpListsDisabled   = $null
            }
            lockscreen = @{
                privacyDisabled = $null
            }
        }
        tweaks   = @{
            security = @{
                defenderDisabled      = $null
                windowsUpdateDisabled = $null
                uacDisabled           = $null
                usbWriteProtect       = $null
                usbStorageLockdown    = $null
            }
            ui       = @{
                classicContextMenu     = $null
                fileExtensionsVisible  = $null
                hiddenFilesVisible     = $null
                bingSearchDisabled     = $null
                backgroundAppsDisabled = $null
                notificationsDisabled  = $null
            }
            os       = @{
                hibernationDisabled = $null
                fastStartupDisabled = $null
            }
        }
        identity = @{
            branding = @{
                companyName = $null
                productName = $null
            }
            hideWinCommander = $false
        }
    }

    # ── Privacy: Telemetry ──
    try {
        $tel = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection" -Name "AllowTelemetry" -EA SilentlyContinue
        if ($tel -and $tel.AllowTelemetry -eq 0) { $migration.privacy.telemetry.windowsDisabled = $true }
    }
    catch {}

    # ── Privacy: Copilot ──
    try {
        $cop = Get-ItemProperty -Path "HKCU:\Software\Policies\Microsoft\Windows\WindowsCopilot" -Name "TurnOffWindowsCopilot" -EA SilentlyContinue
        if ($cop -and $cop.TurnOffWindowsCopilot -eq 1) { $migration.privacy.telemetry.copilotDisabled = $true }
    }
    catch {}

    # ── Privacy: Activity History ──
    try {
        $ah = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "EnableActivityFeed" -EA SilentlyContinue
        if ($ah -and $ah.EnableActivityFeed -eq 0) { $migration.privacy.telemetry.activityHistoryDisabled = $true }
    }
    catch {}

    # ── Privacy: Location ──
    try {
        # Primary: check CapabilityAccessManager consent store (what Disable-LocationTracking writes)
        $locCam = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location" -Name "Value" -EA SilentlyContinue
        # Fallback: check Group Policy key
        $locGP = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors" -Name "DisableLocation" -EA SilentlyContinue
        if (($locCam -and $locCam.Value -eq "Deny") -or ($locGP -and $locGP.DisableLocation -eq 1)) {
            $migration.privacy.telemetry.locationTrackingDisabled = $true
        }
    }
    catch {}

    # ── Privacy: Windows Suggestions ──
    try {
        $sug = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" -Name "SoftLandingEnabled" -EA SilentlyContinue
        if ($sug -and $sug.SoftLandingEnabled -eq 0) { $migration.privacy.telemetry.windowsSuggestionsDisabled = $true }
    }
    catch {}

    # ── Privacy: Clipboard History ──
    try {
        $ch = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Clipboard" -Name "EnableClipboardHistory" -EA SilentlyContinue
        if ($ch -and $ch.EnableClipboardHistory -eq 0) { $migration.privacy.clipboard.historyDisabled = $true }
    }
    catch {}

    # ── Privacy: Cloud Clipboard ──
    try {
        $cc = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Clipboard" -Name "EnableCloudClipboard" -EA SilentlyContinue
        if ($cc -and $cc.EnableCloudClipboard -eq 0) { $migration.privacy.clipboard.cloudSyncDisabled = $true }
    }
    catch {}

    # ── Tweaks: UAC ──
    try {
        $uac = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "EnableLUA" -EA SilentlyContinue
        if ($uac -and $uac.EnableLUA -eq 0) { $migration.tweaks.security.uacDisabled = $true }
    }
    catch {}

    # ── Tweaks: USB Storage Lockdown ──
    try {
        $usb = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Services\USBSTOR" -Name "Start" -EA SilentlyContinue
        if ($usb -and $usb.Start -eq 4) { $migration.tweaks.security.usbStorageLockdown = $true }
    }
    catch {}

    # ── Tweaks: Classic Context Menu ──
    try {
        $ctx = Get-ItemProperty -Path "HKCU:\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32" -Name "(Default)" -EA SilentlyContinue
        if ($null -ne $ctx) { $migration.tweaks.ui.classicContextMenu = $true }
    }
    catch {}

    # ── Tweaks: File Extensions ──
    try {
        $ext = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "HideFileExt" -EA SilentlyContinue
        if ($ext -and $ext.HideFileExt -eq 0) { $migration.tweaks.ui.fileExtensionsVisible = $true }
    }
    catch {}

    # ── Tweaks: Hidden Files ──
    try {
        $hid = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "Hidden" -EA SilentlyContinue
        if ($hid -and $hid.Hidden -eq 1) { $migration.tweaks.ui.hiddenFilesVisible = $true }
    }
    catch {}

    # ── Tweaks: Bing Search ──
    try {
        $bing = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search" -Name "BingSearchEnabled" -EA SilentlyContinue
        if ($bing -and $bing.BingSearchEnabled -eq 0) { $migration.tweaks.ui.bingSearchDisabled = $true }
    }
    catch {}

    # ── Tweaks: Background Apps ──
    try {
        $bg = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications" -Name "GlobalUserDisabled" -EA SilentlyContinue
        if ($bg -and $bg.GlobalUserDisabled -eq 1) { $migration.tweaks.ui.backgroundAppsDisabled = $true }
    }
    catch {}

    # ── Identity: Branding ──
    try {
        $brand = Get-ItemProperty -Path "HKCU:\Software\servalabs\WinCommander\Branding" -EA SilentlyContinue
        if ($brand) {
            if ($brand.CompanyName) { $migration.identity.branding.companyName = $brand.CompanyName }
            if ($brand.ProductName) { $migration.identity.branding.productName = $brand.ProductName }
        }
    }
    catch {}

    $migration
}

function Get-WCSystemProbe {
    <#
    .SYNOPSIS
        Comprehensive system state probe. Reads ALL toggle-able settings from
        registry/OS and returns a SystemState object matching the settings.json schema.
        Used by the convergence engine to populate the "current" state.
    .OUTPUTS
        JSON matching the SystemState shape (privacy, tweaks, network, identity, apps, productivity).
    #>

    $state = @{
        device       = @{
            cpu                 = $null
            ramGb               = $null
            gpu                 = $null
            disks               = @()
            macAddresses        = @()
            serialNumber        = $null
            biosVersion         = $null
            osBuild             = $null
            domain              = $null
            timeZone            = $null
            systemLocale        = $null
            users               = @()
            runtimes            = $null
            windowsActivated    = $null
            bitlockerStatus     = @()
            lastUpdateAt        = $null
            pendingUpdatesCount = $null
        }
        privacy      = @{
            telemetry                   = @{
                windowsDisabled            = $false
                officeDisabled             = $false
                powershell7Disabled        = $false
                copilotDisabled            = $false
                activityHistoryDisabled    = $false
                locationTrackingDisabled   = $false
                windowsSuggestionsDisabled = $false
            }
            clipboard                   = @{
                historyDisabled   = $false
                cloudSyncDisabled = $false
                # autoEraseSchedule removed — Privacy Clean per-card scheduler
            }
            tracking                    = @{
                recentFilesDisabled    = $false
                jumpListsDisabled      = $false
                thumbnailCacheDisabled = $false
                pagefileDisabled       = $false
                # rdpAutoEraseSchedule + eventLogAutoEraseSchedule removed — Privacy Clean per-card scheduler
            }
            lockscreen                  = @{
                privacyDisabled = $false
            }
            privacyProtectionEnabled    = $false
            setupCompletionNagsDisabled = $false
            privacyShield               = @{
                gazeDetectionEnabled    = $true
                antiPeepingEnabled      = $false
                cameraHunterEnabled     = $false
                confidenceThreshold     = $null
                wakeDelaySeconds        = $null
                blurOpacity             = $null
                modelSize               = $null
                detectionBufferFrames   = $null
                captureOnDevice         = $false
                captureOnMultiFace      = $false
                captureSpeed            = $null
                deviceWakeMultiplier    = $null
                multiFaceWakeMultiplier = $null
            }
        }
        tweaks       = @{
            security = @{
                defenderDisabled         = $false
                windowsUpdateDisabled    = $false
                uacDisabled              = $false
                usbWriteProtect          = $false
                usbStorageLockdown       = $false
                consumerFeaturesDisabled = $false
                remoteAssistanceDisabled = $false
                anonymousSamEnumerationBlocked = $false
            }
            os       = @{
                superfetchDisabled       = $false
                prefetchDisabled         = $false
                hibernationDisabled      = $false
                fastStartupDisabled      = $false
                ntfsOptimizationsEnabled = $false
                detailedBsodEnabled      = $false
                automaticMaintenanceDisabled = $false
                win32LongPathsEnabled    = $false
                smbBandwidthThrottlingDisabled = $false
                desktopShellPriorityEnabled = $false
                powerPlan                = $null
            }
            ui       = @{
                classicContextMenu     = $false
                fileExtensionsVisible  = $false
                hiddenFilesVisible     = $false
                galleryHomeRemoved     = $false
                bingSearchDisabled     = $false
                backgroundAppsDisabled = $false
                notificationsDisabled  = $false
                endTaskOnTaskbar       = $false
            }
            server   = @{
                isServerSku                  = $false
                persistentRdpAnimations      = $false
                ctrlAltDelDisabled           = $false
                lastSignedInUserHidden       = $false
                consoleInactivityLock        = $false
                shutdownTrackerDisabled      = $false
                serverManagerAtLogonDisabled = $false
                ieEnhancedSecurityDisabled   = $false
                wdigestBlocked               = $false
                lsaProtectionEnabled         = $false
                legacyNtlmBlocked            = $false
                smbSigningRequired           = $false
                smb1Disabled                 = $false
                remoteRegistryDisabled       = $false
            }
            # ── Empty buckets so granular probe writes don't
            # need Add-Member trickery. Frontend reads the whole sub-
            # hashtable via data.tweaks.{performance,gpu,power}.* and
            # falls back to null when a field isn't present.
            performance = @{}
            gpu         = @{}
            power       = @{}
            bootKernel  = @{}
        }
        network      = @{
            dns      = @{
                ipv4Preference      = $false
                swissFirewallConfig = @{
                    dohId      = $null
                    deviceName = $null
                }
            }
            firewall = @{
                lockdownMode = $false
            }
        }
        identity     = @{
            branding = @{
                companyName  = $null
                productName  = $null
                pcName       = $null
                manufacturer = $null
                supportUrl   = $null
            }
            hideWinCommander = $false
        }
        apps         = @{
            edgeRemoved     = $false
            onedriveRemoved = $false
        }
        productivity = @{
            trackerEnabled                   = $false
            productivityEngineStealthEnabled = $false
            excludeAfk                       = $false
            defaultRange                     = $null
        }
    }

    # ════════════════════════════════════════════════════════════════════
    # DEVICE: Identifiers
    # ════════════════════════════════════════════════════════════════════
    try {
        $cpu = Get-WmiObject Win32_Processor -EA SilentlyContinue | Select-Object -First 1
        if ($cpu) { $state.device.cpu = $cpu.Name }
        
        $ram = Get-WmiObject Win32_PhysicalMemory -EA SilentlyContinue | Measure-Object -Property Capacity -Sum
        if ($ram) { $state.device.ramGb = [math]::Round($ram.Sum / 1GB, 2) }
        
        $gpu = Get-WmiObject Win32_VideoController -EA SilentlyContinue | Select-Object -First 1
        if ($gpu) { $state.device.gpu = $gpu.Name }
        
        $disks = Get-WmiObject Win32_DiskDrive -EA SilentlyContinue
        foreach ($d in $disks) {
            $state.device.disks += @{
                model    = $d.Model
                sizeGb   = [math]::Round($d.Size / 1GB, 2)
                diskType = if ($d.MediaType -match 'SSD' -or $d.Model -match 'SSD') { 'SSD' }else { 'HDD' }
            }
        }
        
        $state.device.macAddresses = Get-NetAdapter -EA SilentlyContinue | Where-Object { $_.Status -eq 'Up' } | Select-Object -ExpandProperty MacAddress
        
        $bios = Get-WmiObject Win32_BIOS -EA SilentlyContinue
        if ($bios) {
            $state.device.serialNumber = $bios.SerialNumber
            $state.device.biosVersion = $bios.SMBIOSBIOSVersion
        }
        
        $os = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion" -EA SilentlyContinue
        if ($os) { $state.device.osBuild = $os.CurrentBuild }
        
        $cs = Get-WmiObject Win32_ComputerSystem -EA SilentlyContinue
        if ($cs) { $state.device.domain = $cs.Domain }
        
        $state.device.timeZone = (Get-TimeZone -EA SilentlyContinue).DisplayName
        $state.device.systemLocale = (Get-WinSystemLocale -EA SilentlyContinue).Name
        
        $users = Get-LocalUser -EA SilentlyContinue
        foreach ($u in $users) {
            $state.device.users += @{
                username  = $u.Name
                enabled   = $u.Enabled
                isAdmin   = $false # Default, check below
                lastLogon = if ($u.LastLogon) { $u.LastLogon.ToString("yyyy-MM-ddTHH:mm:ssZ") } else { $null }
                sid       = $u.SID.Value
            }
        }
        
        # Check admin status
        $admins = Get-LocalGroupMember -Group "Administrators" -EA SilentlyContinue | Select-Object -ExpandProperty Name
        foreach ($user in $state.device.users) {
            if ($admins -contains $user.username) { $user.isAdmin = $true }
        }
        
        $wa = Get-CimInstance -ClassName SoftwareLicensingProduct -Filter "PartialProductKey IS NOT NULL" -EA SilentlyContinue | Where-Object { $_.LicenseStatus -eq 1 }
        if ($wa) { $state.device.windowsActivated = $true }
        
        if (Get-Command Get-BitLockerVolume -EA SilentlyContinue) {
            $bl = Get-BitLockerVolume -EA SilentlyContinue
            foreach ($v in $bl) {
                $state.device.bitlockerStatus += @{
                    drive     = $v.MountPoint
                    encrypted = ($v.VolumeStatus -eq 'FullyEncrypted')
                    status    = $v.ProtectionStatus.ToString()
                }
            }
        }
    }
    catch {}

    # ════════════════════════════════════════════════════════════════════
    # PRIVACY: Telemetry
    # ════════════════════════════════════════════════════════════════════
    try {
        $tel = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection" -Name "AllowTelemetry" -EA SilentlyContinue
        if ($tel -and $tel.AllowTelemetry -eq 0) { $state.privacy.telemetry.windowsDisabled = $true }
    }
    catch {}

    try {
        $cop = Get-ItemProperty -Path "HKCU:\Software\Policies\Microsoft\Windows\WindowsCopilot" -Name "TurnOffWindowsCopilot" -EA SilentlyContinue
        if ($cop -and $cop.TurnOffWindowsCopilot -eq 1) { $state.privacy.telemetry.copilotDisabled = $true }
    }
    catch {}

    try {
        $ah = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "EnableActivityFeed" -EA SilentlyContinue
        if ($ah -and $ah.EnableActivityFeed -eq 0) { $state.privacy.telemetry.activityHistoryDisabled = $true }
    }
    catch {}

    try {
        # Primary: check CapabilityAccessManager consent store (what Disable-LocationTracking writes)
        $locCam = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location" -Name "Value" -EA SilentlyContinue
        # Fallback: check Group Policy key
        $locGP = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors" -Name "DisableLocation" -EA SilentlyContinue
        if (($locCam -and $locCam.Value -eq "Deny") -or ($locGP -and $locGP.DisableLocation -eq 1)) {
            $state.privacy.telemetry.locationTrackingDisabled = $true
        }
    }
    catch {}

    try {
        $sug = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" -Name "SoftLandingEnabled" -EA SilentlyContinue
        if ($sug -and $sug.SoftLandingEnabled -eq 0) { $state.privacy.telemetry.windowsSuggestionsDisabled = $true }
    }
    catch {}

    # Office privacy pack (check one sentinel key)
    try {
        $ofc = Get-ItemProperty -Path "HKCU:\Software\Policies\Microsoft\Office\16.0\Common" -Name "QMEnable" -EA SilentlyContinue
        if ($ofc -and $ofc.QMEnable -eq 0) { $state.privacy.telemetry.officeDisabled = $true }
    }
    catch {}

    # PowerShell 7 telemetry
    try {
        $ps7 = [Environment]::GetEnvironmentVariable("POWERSHELL_TELEMETRY_OPTOUT", "Machine")
        if ($ps7 -eq "1" -or $ps7 -eq "true") { $state.privacy.telemetry.powershell7Disabled = $true }
    }
    catch {}

    # ════════════════════════════════════════════════════════════════════
    # PRIVACY: Clipboard
    # ════════════════════════════════════════════════════════════════════
    try {
        $ch = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Clipboard" -Name "EnableClipboardHistory" -EA SilentlyContinue
        if ($ch -and $ch.EnableClipboardHistory -eq 0) { $state.privacy.clipboard.historyDisabled = $true }
    }
    catch {}

    try {
        $ccPref = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Clipboard" -Name "EnableCloudClipboard" -EA SilentlyContinue
        $ccPol  = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "AllowCrossDeviceClipboard" -EA SilentlyContinue
        if (($ccPref -and $ccPref.EnableCloudClipboard -eq 0) -or ($ccPol -and $ccPol.AllowCrossDeviceClipboard -eq 0)) {
            $state.privacy.clipboard.cloudSyncDisabled = $true
        }
    }
    catch {}

    # Clipboard / RDP / Event Log auto-erase schedules used to be polled
    # here (legacy WinCommander_ClipboardErase / _RDPErase / _EventLogErase
    # tasks). Replaced by the per-card scheduler in Privacy Clean; the
    # Cleanup panel queries Get-AutoEraseSchedules directly when it
    # mounts. Legacy tasks are migrated/removed by Invoke-AutoEraseMigration.

    # ════════════════════════════════════════════════════════════════════
    # PRIVACY: Tracking
    # ════════════════════════════════════════════════════════════════════
    try {
        $rf = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "Start_TrackDocs" -EA SilentlyContinue
        if ($rf -and $rf.Start_TrackDocs -eq 0) { $state.privacy.tracking.recentFilesDisabled = $true }
    }
    catch {}

    try {
        $jl = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "Start_TrackProgs" -EA SilentlyContinue
        if ($jl -and $jl.Start_TrackProgs -eq 0) { $state.privacy.tracking.jumpListsDisabled = $true }
    }
    catch {}

    try {
        $tc = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "DisableThumbnailCache" -EA SilentlyContinue
        if ($tc -and $tc.DisableThumbnailCache -eq 1) { $state.privacy.tracking.thumbnailCacheDisabled = $true }
    }
    catch {}

    try {
        $pf = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management" -Name "PagingFiles" -EA SilentlyContinue
        if ($pf -and ($pf.PagingFiles -eq "" -or $pf.PagingFiles.Count -eq 0)) { $state.privacy.tracking.pagefileDisabled = $true }
    }
    catch {}

    # RDP + Event Log auto-erase schedule polling removed — see comment
    # above the Tracking section.

    # ════════════════════════════════════════════════════════════════════
    # PRIVACY: Lock Screen & Setup Nags
    # ════════════════════════════════════════════════════════════════════
    try {
        # KT: Keep the radar probe in sync with Get-LockScreenPrivacyStatus so
        # the finding disappears once the actual privacy bundle is applied.
        $cloud = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableWindowsSpotlightFeatures" -EA SilentlyContinue).DisableWindowsSpotlightFeatures
        $toast = (Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings" -Name "NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK" -EA SilentlyContinue).NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK
        $camera = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\Personalization" -Name "NoLockScreenCamera" -EA SilentlyContinue).NoLockScreenCamera
        if ($cloud -eq 1 -and $toast -eq 0 -and $camera -eq 1) { $state.privacy.lockscreen.privacyDisabled = $true }
    }
    catch {}

    try {
        $sn = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" -Name "SubscribedContent-310093Enabled" -EA SilentlyContinue
        if ($sn -and $sn.'SubscribedContent-310093Enabled' -eq 0) { $state.privacy.setupCompletionNagsDisabled = $true }
    }
    catch {}

    # ════════════════════════════════════════════════════════════════════
    # TWEAKS: Security
    # ════════════════════════════════════════════════════════════════════
    try {
        $def = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender" -Name "DisableAntiSpyware" -EA SilentlyContinue
        if ($def -and $def.DisableAntiSpyware -eq 1) { $state.tweaks.security.defenderDisabled = $true }
    }
    catch {}

    try {
        $wu = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings" -Name "PauseFeatureUpdatesStartTime" -EA SilentlyContinue
        if ($wu -and $wu.PauseFeatureUpdatesStartTime) { $state.tweaks.security.windowsUpdateDisabled = $true }
    }
    catch {}

    try {
        $uac = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "EnableLUA" -EA SilentlyContinue
        if ($uac -and $uac.EnableLUA -eq 0) { $state.tweaks.security.uacDisabled = $true }
    }
    catch {}

    try {
        $uwp = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\StorageDevicePolicies" -Name "WriteProtect" -EA SilentlyContinue
        if ($uwp -and $uwp.WriteProtect -eq 1) { $state.tweaks.security.usbWriteProtect = $true }
    }
    catch {}

    try {
        $usb = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Services\USBSTOR" -Name "Start" -EA SilentlyContinue
        if ($usb -and $usb.Start -eq 4) { $state.tweaks.security.usbStorageLockdown = $true }
    }
    catch {}

    try {
        $cf = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableWindowsConsumerFeatures" -EA SilentlyContinue
        if ($cf -and $cf.DisableWindowsConsumerFeatures -eq 1) { $state.tweaks.security.consumerFeaturesDisabled = $true }
    }
    catch {}

    # KT: copilotAiRemoved was missing from probe — toggle always showed OFF even after removal.
    # Check both HKCU and HKLM policy paths; either key set to 1 means removal is active.
    try {
        $copHcu = Get-ItemProperty -Path "HKCU:\Software\Policies\Microsoft\Windows\WindowsCopilot" -Name "TurnOffWindowsCopilot" -EA SilentlyContinue
        $copHlm = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot" -Name "TurnOffWindowsCopilot" -EA SilentlyContinue
        if (($copHcu -and $copHcu.TurnOffWindowsCopilot -eq 1) -or ($copHlm -and $copHlm.TurnOffWindowsCopilot -eq 1)) {
            $state.tweaks.security.copilotAiRemoved = $true
        }
    }
    catch {}

    try {
        $vbs = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DeviceGuard" -Name "EnableVirtualizationBasedSecurity" -EA SilentlyContinue
        if ($vbs -and $vbs.EnableVirtualizationBasedSecurity -eq 0) { $state.tweaks.security.vbsDisabled = $true }
    } catch {}
    try {
        $wpbt = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager" -Name "DisableWpbtExecution" -EA SilentlyContinue
        if ($wpbt -and $wpbt.DisableWpbtExecution -eq 1) { $state.tweaks.security.wpbtDisabled = $true }
    } catch {}
    try {
        $ss = Get-ItemProperty -Path "HKLM:\Software\Policies\Microsoft\Windows\System" -Name "EnableSmartScreen" -EA SilentlyContinue
        if ($null -ne $ss -and $ss.EnableSmartScreen -eq 0) { $state.tweaks.security.smartScreenDisabled = $true }
    } catch {}
    try {
        $oobe = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE" -Name "HideOnlineAccountScreens" -EA SilentlyContinue
        if ($oobe -and $oobe.HideOnlineAccountScreens -eq 1) { $state.tweaks.security.oobeBypassEnabled = $true }
    } catch {}
    try {
        $dvr = Get-ItemProperty -Path "HKCU:\System\GameConfigStore" -Name "GameDVR_Enabled" -EA SilentlyContinue
        if ($null -ne $dvr -and $dvr.GameDVR_Enabled -eq 0) { $state.tweaks.security.gameDvrDisabled = $true }
    } catch {}

    # ════════════════════════════════════════════════════════════════════
    # TWEAKS: OS
    # ════════════════════════════════════════════════════════════════════
    try {
        $sf = Get-Service -Name "SysMain" -EA SilentlyContinue
        if ($sf -and $sf.StartType -eq 'Disabled') { $state.tweaks.os.superfetchDisabled = $true }
    }
    catch {}

    try {
        $pf2 = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters" -Name "EnablePrefetcher" -EA SilentlyContinue
        if ($pf2 -and $pf2.EnablePrefetcher -eq 0) { $state.tweaks.os.prefetchDisabled = $true }
    }
    catch {}

    try {
        $hib = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Power" -Name "HibernateEnabled" -EA SilentlyContinue
        if ($hib -and $hib.HibernateEnabled -eq 0) { $state.tweaks.os.hibernationDisabled = $true }
    }
    catch {}

    try {
        $fs = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power" -Name "HiberbootEnabled" -EA SilentlyContinue
        if ($fs -and $fs.HiberbootEnabled -eq 0) { $state.tweaks.os.fastStartupDisabled = $true }
    }
    catch {}

    try {
        # KT: Don't substring-match "1" against the whole fsutil output —
        # locale text or format changes give false positives. Extract the
        # numeric value and treat 1 (User Managed Disabled) or 3 (System
        # Managed Disabled) as "optimization applied"; 0/2 means last-access
        # updates are still on, regardless of who manages the policy.
        $la = (fsutil behavior query disablelastaccess 2>$null) -join " "
        if ($la -match 'DisableLastAccess\s*=\s*(\d+)') {
            $val = [int]$Matches[1]
            if ($val -eq 1 -or $val -eq 3) { $state.tweaks.os.ntfsOptimizationsEnabled = $true }
        }
    }
    catch {}

    try {
        $bs = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl" -Name "DisplayParameters" -EA SilentlyContinue
        if ($bs -and $bs.DisplayParameters -eq 1) { $state.tweaks.os.detailedBsodEnabled = $true }
    }
    catch {}

    try {
        $mc = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management" -Name "MemoryCompression" -EA SilentlyContinue
        if ($null -ne $mc -and $mc.MemoryCompression -eq 0) { $state.tweaks.os.memoryCompressionDisabled = $true }
    } catch {}
    try {
        $wp = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\PriorityControl" -Name "Win32PrioritySeparation" -EA SilentlyContinue
        if ($wp -and $wp.Win32PrioritySeparation -eq 38) { $state.tweaks.os.win32PrioritySeparation = $true }
    } catch {}
    try {
        $shellPriorityTask = Get-ScheduledTask -TaskName 'WinCommanderShellPriorityLogon' -ErrorAction SilentlyContinue
        $shellPriorityHelper = Join-Path $env:ProgramData 'WinCommander\ShellPriority\Apply-ShellPriority.ps1'
        $shellPriorityTargets = @('explorer.exe', 'dwm.exe', 'sihost.exe', 'StartMenuExperienceHost.exe', 'ShellExperienceHost.exe', 'SystemSettings.exe', 'Taskmgr.exe', 'SearchHost.exe', 'SearchApp.exe')
        $shellPriorityRegistryOk = [bool](($shellPriorityTargets | Where-Object {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$_\PerfOptions"
            $options = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue
            $options -and $options.CpuPriorityClass -eq 3 -and $options.IoPriority -eq 3
        } | Measure-Object).Count -eq $shellPriorityTargets.Count)
        $shellPriorityTaskOk = [bool](($shellPriorityTask.Actions | Where-Object { $_.Arguments -like '*Apply-ShellPriority.ps1*' }) -and
            ($shellPriorityTask.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' }) -and
            $shellPriorityTask.Principal.UserId -in @('SYSTEM', 'S-1-5-18') -and $shellPriorityTask.State -ne 'Disabled')
        $usersCanWriteShellPriorityHelper = $false
        if (Test-Path -LiteralPath $shellPriorityHelper) {
            foreach ($rule in (Get-Acl -LiteralPath $shellPriorityHelper).Access) {
                try { $ruleSid = $rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value } catch { continue }
                if ($ruleSid -eq 'S-1-5-32-545' -and $rule.AccessControlType -eq 'Allow' -and
                    ($rule.FileSystemRights -band [System.Security.AccessControl.FileSystemRights]::Write) -ne 0) {
                    $usersCanWriteShellPriorityHelper = $true
                }
            }
        }
        $state.tweaks.os.desktopShellPriorityEnabled = [bool]($shellPriorityRegistryOk -and $shellPriorityTaskOk -and
            (Test-Path -LiteralPath $shellPriorityHelper) -and -not $usersCanWriteShellPriorityHelper)
    } catch {}
    try {
        $wkt = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control" -Name "WaitToKillServiceTimeout" -EA SilentlyContinue
        if ($wkt -and $wkt.WaitToKillServiceTimeout -eq "1500") { $state.tweaks.os.serviceTimeoutsOptimized = $true }
    } catch {}

    # ── Windows Server tweaks ────────────────────────────────────────────
    # Mirrors Get-ServerTweakStatus in tweaks/server.ps1. Kept in the same
    # boolean shape so the panel probe and this bridge cannot disagree.
    try {
        $pt = (Get-CimInstance -ClassName Win32_OperatingSystem -EA SilentlyContinue).ProductType
        $state.tweaks.server.isServerSku = ($pt -eq 2 -or $pt -eq 3)
    } catch {}
    try {
        $rdpAnimationTask = Get-ScheduledTask -TaskName 'Keep RDP Animation Effects' -EA SilentlyContinue
        $rdpAnimationPolicy = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DWM' -Name 'DisallowAnimations' -EA SilentlyContinue
        $rdpVisualEffects = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects' -Name 'VisualFXSetting' -EA SilentlyContinue
        $rdpWindowMetrics = Get-ItemProperty -Path 'HKCU:\Control Panel\Desktop\WindowMetrics' -Name 'MinAnimate' -EA SilentlyContinue
        $rdpExplorer = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'TaskbarAnimations' -EA SilentlyContinue
        $rdpAnimationHasLogon = [bool](@($rdpAnimationTask.Triggers) | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' })
        $rdpAnimationHasRemoteConnect = [bool](@($rdpAnimationTask.Triggers) | Where-Object {
            $_.CimClass.CimClassName -eq 'MSFT_TaskSessionStateChangeTrigger' -and $_.StateChange -eq 3
        })
        $rdpAnimationHasAction = [bool](@($rdpAnimationTask.Actions) | Where-Object { $_.Arguments -like '*Keep-RdpAnimationEffects.ps1*' })
        $state.tweaks.server.persistentRdpAnimations = [bool](
            $rdpAnimationTask -and $rdpAnimationTask.State -ne 'Disabled' -and
            $rdpAnimationHasLogon -and $rdpAnimationHasRemoteConnect -and $rdpAnimationHasAction -and
            (Test-Path -LiteralPath (Join-Path $env:ProgramData 'WinCommander\Keep-RdpAnimationEffects.ps1')) -and
            $rdpAnimationPolicy.DisallowAnimations -eq 0 -and
            $rdpVisualEffects.VisualFXSetting -eq 2 -and
            $rdpWindowMetrics.MinAnimate -eq '1' -and
            $rdpExplorer.TaskbarAnimations -eq 1
        )
    } catch {}
    try {
        $sp = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -EA SilentlyContinue
        if ($sp -and $sp.DisableCAD -eq 1) { $state.tweaks.server.ctrlAltDelDisabled = $true }
        if ($sp -and $sp.DontDisplayLastUserName -eq 1) { $state.tweaks.server.lastSignedInUserHidden = $true }
        if ($sp -and $sp.InactivityTimeoutSecs -gt 0) { $state.tweaks.server.consoleInactivityLock = $true }
    } catch {}
    try {
        $rel = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Reliability" -EA SilentlyContinue
        if ($rel -and $rel.ShutdownReasonOn -eq 0) { $state.tweaks.server.shutdownTrackerDisabled = $true }
    } catch {}
    try {
        $sm = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\ServerManager" -Name "DoNotOpenServerManagerAtLogon" -EA SilentlyContinue
        if ($sm -and $sm.DoNotOpenServerManagerAtLogon -eq 1) { $state.tweaks.server.serverManagerAtLogonDisabled = $true }
    } catch {}
    try {
        $esc = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Active Setup\Installed Components\{A509B1A7-37EF-4b3f-8CFC-4F3A74704073}" -Name "IsInstalled" -EA SilentlyContinue
        if ($esc -and $esc.IsInstalled -eq 0) { $state.tweaks.server.ieEnhancedSecurityDisabled = $true }
    } catch {}
    try {
        $wd = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\SecurityProviders\WDigest" -Name "UseLogonCredential" -EA SilentlyContinue
        if ($wd -and $wd.UseLogonCredential -eq 0) { $state.tweaks.server.wdigestBlocked = $true }
    } catch {}
    try {
        $lsa = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Lsa" -EA SilentlyContinue
        if ($lsa -and $lsa.RunAsPPL -eq 1) { $state.tweaks.server.lsaProtectionEnabled = $true }
        if ($lsa -and $lsa.LmCompatibilityLevel -ge 5) { $state.tweaks.server.legacyNtlmBlocked = $true }
    } catch {}
    try {
        $smb = Get-SmbServerConfiguration -EA SilentlyContinue
        if ($smb -and $smb.RequireSecuritySignature) { $state.tweaks.server.smbSigningRequired = $true }
        if ($smb -and -not $smb.EnableSMB1Protocol) { $state.tweaks.server.smb1Disabled = $true }
    } catch {}
    try {
        $rr = Get-Service -Name "RemoteRegistry" -EA SilentlyContinue
        if ($rr -and $rr.StartType -eq 'Disabled') { $state.tweaks.server.remoteRegistryDisabled = $true }
    } catch {}

    # Power Plan detection — writes to TweakSettings.powerPlan (the field
    # the UI actually reads). The previous tweaks.os.powerPlan field was
    # dead code; this probe used to write to it and nothing read it.
    try {
        $activeGuid = (powercfg /getactivescheme).Split(' ')[3]
        $guids = @{
            '381b4222-f694-41f0-9685-ff5bb260df2e' = 'balanced'
            'a1841308-3541-4fab-bc81-f71556f20b4a' = 'powersaving'
            '8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c' = 'performance'
            'e9a42b02-d5df-448d-aa00-03f14749eb61' = 'ultimate'
        }
        if ($guids.ContainsKey($activeGuid)) {
            $state.tweaks.powerPlan = $guids[$activeGuid]
        }
    }
    catch {}

    # ════════════════════════════════════════════════════════════════════
    # TWEAKS: BOOT & KERNEL
    # These toggles use HKLM paths; without probe entries the UI would
    # permanently show current.tweaks.bootKernel.* as null/false even
    # after the command applied the registry change.
    # ════════════════════════════════════════════════════════════════════
    try {
        $tsx = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Kernel" -Name "DisableTsx" -EA SilentlyContinue
        if ($null -ne $tsx -and $tsx.DisableTsx -eq 0) { $state.tweaks.bootKernel.tsxEnabled = $true }
    } catch {}
    try {
        $fla = Get-ItemProperty -Path "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System" -Name "EnableFirstLogonAnimation" -EA SilentlyContinue
        if ($null -ne $fla -and $fla.EnableFirstLogonAnimation -eq 0) { $state.tweaks.bootKernel.firstLogonAnimationDisabled = $true }
    } catch {}
    try {
        $ss2 = Get-ItemProperty -Path "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System" -Name "DisableStartupSound" -EA SilentlyContinue
        if ($ss2 -and $ss2.DisableStartupSound -eq 1) { $state.tweaks.bootKernel.startupSoundDisabled = $true }
    } catch {}
    try {
        $arso = Get-ItemProperty -Path "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System" -Name "DisableAutomaticRestartSignOn" -EA SilentlyContinue
        if ($arso -and $arso.DisableAutomaticRestartSignOn -eq 1) { $state.tweaks.bootKernel.autoRestartSignonDisabled = $true }
    } catch {}
    try {
        $arb = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl" -Name "AutoReboot" -EA SilentlyContinue
        if ($null -ne $arb -and $arb.AutoReboot -eq 0) { $state.tweaks.bootKernel.autoRebootOnBsodDisabled = $true }
    } catch {}
    try {
        $smd = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl" -Name "CrashDumpEnabled" -EA SilentlyContinue
        if ($smd -and $smd.CrashDumpEnabled -eq 3) { $state.tweaks.bootKernel.smallMemoryDumpEnabled = $true }
    } catch {}

    # ════════════════════════════════════════════════════════════════════
    # TWEAKS: PERFORMANCE
    # Without these probes, toggles in the Performance & Gaming section
    # never reflect their applied state — backend command succeeds, but
    # current.tweaks.performance.* stays null and the UI shows "off".
    # Buckets are pre-initialised on $state.tweaks above; assignments
    # below auto-extend the hashtable.
    # ════════════════════════════════════════════════════════════════════
    try {
        $mm = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile" -Name "SystemResponsiveness" -EA SilentlyContinue
        if ($mm -and $mm.SystemResponsiveness -le 10) { $state.tweaks.performance.mmcssGamingProfile = $true }
    } catch {}
    try {
        $kd = Get-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "KeyboardDelay" -EA SilentlyContinue
        if ($kd -and "$($kd.KeyboardDelay)" -eq "0") { $state.tweaks.performance.keyboardLatencyOptimised = $true }
    } catch {}
    try {
        $nl = Get-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "InitialKeyboardIndicators" -EA SilentlyContinue
        if ($nl) {
            $v = "$($nl.InitialKeyboardIndicators)"
            if ($v -eq "2" -or $v -eq "2147483650") { $state.tweaks.performance.numLockOnBoot = $true }
        }
    } catch {}
    try {
        $gs = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers" -Name "HwSchMode" -EA SilentlyContinue
        if ($gs -and $gs.HwSchMode -eq 2) { $state.tweaks.performance.gpuSchedulingEnabled = $true }
    } catch {}
    try {
        $svc = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control" -Name "SvcHostSplitThresholdInKB" -EA SilentlyContinue
        if ($svc -and $svc.SvcHostSplitThresholdInKB -gt 500000) { $state.tweaks.performance.svcHostSplitOptimised = $true }
    } catch {}
    try {
        $sk = Get-ItemProperty -Path "HKCU:\Control Panel\Accessibility\StickyKeys" -Name "Flags" -EA SilentlyContinue
        if ($sk -and "$($sk.Flags)" -eq "506") { $state.tweaks.performance.accessibilityShortcutsDisabled = $true }
    } catch {}
    try {
        $msd = Get-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "MenuShowDelay" -EA SilentlyContinue
        if ($msd -and "$($msd.MenuShowDelay)" -eq "0") { $state.tweaks.performance.instantMenuDelay = $true }
    } catch {}
    try {
        $ms = Get-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseSpeed" -EA SilentlyContinue
        if ($ms -and "$($ms.MouseSpeed)" -eq "0") { $state.tweaks.performance.mouseAccelerationDisabled = $true }
    } catch {}
    try {
        $ac = Get-ItemProperty -Path "HKCU:\Software\Microsoft\TabletTip\1.7" -Name "EnableAutocorrection" -EA SilentlyContinue
        if ($ac -and $ac.EnableAutocorrection -eq 0) { $state.tweaks.performance.autocorrectDisabled = $true }
    } catch {}
    try {
        $en = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager" -Name "EnthusiastMode" -EA SilentlyContinue
        if ($en -and $en.EnthusiastMode -eq 1) { $state.tweaks.performance.enthusiastModeEnabled = $true }
    } catch {}
    try {
        $wq = Get-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "JPEGImportQuality" -EA SilentlyContinue
        if ($wq -and $wq.JPEGImportQuality -eq 100) { $state.tweaks.performance.wallpaperFullQuality = $true }
    } catch {}

    # ════════════════════════════════════════════════════════════════════
    # TWEAKS: GPU (vendor-specific) — walk every GPU class device key and
    # report a vendor's tweak as ON if ANY device key has it applied.
    # ════════════════════════════════════════════════════════════════════
    try {
        $classRoot = "HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}"
        if (Test-Path $classRoot) {
            foreach ($entry in (Get-ChildItem -Path $classRoot -EA SilentlyContinue)) {
                if ($entry.PSChildName -notmatch '^\d{4}$') { continue }
                $kp = $entry.PSPath
                $pn = (Get-ItemProperty -Path $kp -Name "ProviderName" -EA SilentlyContinue).ProviderName
                $dd = (Get-ItemProperty -Path $kp -Name "DriverDesc"   -EA SilentlyContinue).DriverDesc
                $merged = "$pn $dd"
                if ($merged -match "(?i)AMD|ATI|Radeon") {
                    $u = (Get-ItemProperty -Path $kp -Name "EnableUlps" -EA SilentlyContinue).EnableUlps
                    if ($null -ne $u -and $u -eq 0) { $state.tweaks.gpu.amdUlpsDisabled = $true }
                    $pg = (Get-ItemProperty -Path $kp -Name "DisablePowerGating" -EA SilentlyContinue).DisablePowerGating
                    if ($pg -eq 1) { $state.tweaks.gpu.amdPowerGatingDisabled = $true }
                    $vcg = (Get-ItemProperty -Path $kp -Name "DisableVceClockGating" -EA SilentlyContinue).DisableVceClockGating
                    if ($vcg -eq 1) { $state.tweaks.gpu.amdVideoClockGatingDisabled = $true }
                    $a = (Get-ItemProperty -Path $kp -Name "EnableAspmL0s" -EA SilentlyContinue).EnableAspmL0s
                    if ($null -ne $a -and $a -eq 0) { $state.tweaks.gpu.amdAspmDisabled = $true }
                }
                if ($merged -match "(?i)NVIDIA|GeForce|Quadro") {
                    $d = (Get-ItemProperty -Path $kp -Name "DisableDynamicPstate" -EA SilentlyContinue).DisableDynamicPstate
                    if ($d -eq 1) { $state.tweaks.gpu.nvidiaDynamicPstateDisabled = $true }
                    $da = (Get-ItemProperty -Path $kp -Name "DisableASyncPstates" -EA SilentlyContinue).DisableASyncPstates
                    if ($da -eq 1) { $state.tweaks.gpu.nvidiaAsyncPstatesDisabled = $true }
                }
                if ($merged -match "(?i)Intel|HD Graphics|UHD|Iris|Arc") {
                    $f = (Get-ItemProperty -Path $kp -Name "Display1_DisableAsyncFlips" -EA SilentlyContinue).Display1_DisableAsyncFlips
                    if ($f -eq 1) { $state.tweaks.gpu.intelAsyncFlipsDisabled = $true }
                    $v = (Get-ItemProperty -Path $kp -Name "AdaptiveVsyncEnable" -EA SilentlyContinue).AdaptiveVsyncEnable
                    if ($null -ne $v -and $v -eq 0) { $state.tweaks.gpu.intelAdaptiveVsyncDisabled = $true }
                }
            }
        }
    } catch {}

    # ════════════════════════════════════════════════════════════════════
    # TWEAKS: POWER
    # ════════════════════════════════════════════════════════════════════
    try {
        $ct = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling" -Name "PowerThrottlingOff" -EA SilentlyContinue
        if ($ct -and $ct.PowerThrottlingOff -eq 1) { $state.tweaks.power.cpuThrottlingDisabled = $true }
    } catch {}
    try {
        $q = & powercfg /Q SCHEME_CURRENT 2a737441-1930-4402-8d77-b2bebba308a3 48e6b7a6-50f5-4782-a5d4-53bb8f07e226 2>$null
        $ac = ($q -join "`n") -split "`n" | Where-Object { $_ -match 'Current AC Power Setting Index:\s*0x' } | Select-Object -First 1
        if ($ac -and $ac -match '0x0+$') { $state.tweaks.power.usbSelectiveSuspendDisabled = $true }
    } catch {}

    # ════════════════════════════════════════════════════════════════════
    # TWEAKS: UI
    # ════════════════════════════════════════════════════════════════════
    try {
        # Test-Path is more reliable than reading (Default) for an empty-string reg value.
        if (Test-Path "HKCU:\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32") {
            $state.tweaks.ui.classicContextMenu = $true
        }
    }
    catch {}

    try {
        $ext = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "HideFileExt" -EA SilentlyContinue
        if ($ext -and $ext.HideFileExt -eq 0) { $state.tweaks.ui.fileExtensionsVisible = $true }
    }
    catch {}

    try {
        $hid = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "Hidden" -EA SilentlyContinue
        if ($hid -and $hid.Hidden -eq 1) { $state.tweaks.ui.hiddenFilesVisible = $true }
    }
    catch {}

    # `galleryHomeRemoved` — must match what Enable-RemoveGalleryHome
    # actually configures (see scripts/modules/tweaks/ui.ps1). That command
    # deletes the HKLM NameSpace entries for both Home and Gallery and
    # sets HubMode=1; it does NOT touch the HKCU Classes\CLSID key the
    # old probe was reading, which is why the radar kept reporting "not
    # removed" even after the change took effect in Explorer.
    #
    # Considered "removed" iff EITHER the HKLM NameSpace entries are gone
    # OR HubMode is 1 — any one of those signals our changes are in effect.
    try {
        $homeCLSID = "{f874310e-b6b7-47dc-bc84-b9e6b38f5903}"
        $galleryCLSID = "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}"
        $homeNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$homeCLSID"
        $galNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$galleryCLSID"
        $homeGone = -not (Test-Path $homeNS)
        $galGone = -not (Test-Path $galNS)
        $hubMode = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "HubMode" -EA SilentlyContinue
        $hubOn = ($hubMode -and $hubMode.HubMode -eq 1)
        if (($homeGone -and $galGone) -or $hubOn) { $state.tweaks.ui.galleryHomeRemoved = $true }
    }
    catch {}

    try {
        $bing = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search" -Name "BingSearchEnabled" -EA SilentlyContinue
        if ($bing -and $bing.BingSearchEnabled -eq 0) { $state.tweaks.ui.bingSearchDisabled = $true }
    }
    catch {}

    try {
        $bg = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications" -Name "GlobalUserDisabled" -EA SilentlyContinue
        if ($bg -and $bg.GlobalUserDisabled -eq 1) { $state.tweaks.ui.backgroundAppsDisabled = $true }
    }
    catch {}

    try {
        $notif = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\PushNotifications" -Name "ToastEnabled" -EA SilentlyContinue
        if ($notif -and $notif.ToastEnabled -eq 0) { $state.tweaks.ui.notificationsDisabled = $true }
    }
    catch {}

    try {
        $etk = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings" -Name "TaskbarEndTask" -EA SilentlyContinue
        if ($etk -and $etk.TaskbarEndTask -eq 1) { $state.tweaks.ui.endTaskOnTaskbar = $true }
    }
    catch {}

    try {
        $maintenance = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\Maintenance" -Name "MaintenanceDisabled" -EA SilentlyContinue
        if ($maintenance -and $maintenance.MaintenanceDisabled -eq 1) { $state.tweaks.os.automaticMaintenanceDisabled = $true }
    }
    catch {}

    try {
        $longPaths = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" -Name "LongPathsEnabled" -EA SilentlyContinue
        if ($longPaths -and $longPaths.LongPathsEnabled -eq 1) { $state.tweaks.os.win32LongPathsEnabled = $true }
    }
    catch {}

    try {
        $smb = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Services\LanmanWorkstation\Parameters" -Name "DisableBandwidthThrottling" -EA SilentlyContinue
        if ($smb -and $smb.DisableBandwidthThrottling -eq 1) { $state.tweaks.os.smbBandwidthThrottlingDisabled = $true }
    }
    catch {}

    try {
        $remoteAssistance = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Remote Assistance" -Name "fAllowToGetHelp" -EA SilentlyContinue
        $remoteAssistancePolicy = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services" -Name "fAllowToGetHelp" -EA SilentlyContinue
        if (($remoteAssistance -and $remoteAssistance.fAllowToGetHelp -eq 0) -or ($remoteAssistancePolicy -and $remoteAssistancePolicy.fAllowToGetHelp -eq 0)) { $state.tweaks.security.remoteAssistanceDisabled = $true }
    }
    catch {}

    try {
        $sam = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Lsa" -Name "RestrictAnonymousSAM" -EA SilentlyContinue
        if ($sam -and $sam.RestrictAnonymousSAM -eq 1) { $state.tweaks.security.anonymousSamEnumerationBlocked = $true }
    }
    catch {}

    # KT: explorerOpensThisPC was missing from probe — toggle always showed OFF even when set.
    # LaunchTo = 1 means "This PC"; LaunchTo = 2 (or missing) means Quick Access.
    try {
        $lt = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "LaunchTo" -EA SilentlyContinue
        if ($lt -and $lt.LaunchTo -eq 1) { $state.tweaks.ui.explorerOpensThisPC = $true }
    }
    catch {}

    # ── UI GRANULAR ────────────────────────────────────────────────────
    # Per-icon desktop visibility (0 = visible, 1 = hidden).
    $iconClsids = @{
        desktopIconThisPc       = '{20D04FE0-3AEA-1069-A2D8-08002B30309D}'
        desktopIconRecycleBin   = '{645FF040-5081-101B-9F08-00AA002F954E}'
        desktopIconUserFiles    = '{59031a47-3f72-44a7-89c5-5595fe6b30ee}'
        desktopIconNetwork      = '{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}'
        desktopIconControlPanel = '{5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0}'
    }
    foreach ($k in $iconClsids.Keys) {
        try {
            $v = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel" -Name $iconClsids[$k] -EA SilentlyContinue
            if ($v -and ($v.($iconClsids[$k])) -eq 0) { $state.tweaks.ui.$k = $true }
        } catch {}
    }
    try {
        $sa = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Icons" -Name "29" -EA SilentlyContinue
        if ($sa) { $state.tweaks.ui.shortcutArrowRemoved = $true }
    } catch {}
    try {
        $sf = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "EnableSnapAssistFlyout" -EA SilentlyContinue
        if ($sf -and $sf.EnableSnapAssistFlyout -eq 0) { $state.tweaks.ui.snapAssistFlyoutDisabled = $true }
    } catch {}
    try {
        $cm = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "UseCompactMode" -EA SilentlyContinue
        if ($cm -and $cm.UseCompactMode -eq 1) { $state.tweaks.ui.explorerCompactMode = $true }
    } catch {}
    try {
        $cb = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "AutoCheckSelect" -EA SilentlyContinue
        if ($cb -and $cb.AutoCheckSelect -eq 1) { $state.tweaks.ui.explorerCheckboxesEnabled = $true }
    } catch {}
    try {
        $sh = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "DisallowShaking" -EA SilentlyContinue
        if ($sh -and $sh.DisallowShaking -eq 1) { $state.tweaks.ui.windowShakeDisabled = $true }
    } catch {}
    try {
        $cs = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSecondsInSystemClock" -EA SilentlyContinue
        if ($cs -and $cs.ShowSecondsInSystemClock -eq 1) { $state.tweaks.ui.clockSecondsVisible = $true }
    } catch {}

    # ── UI TWEAKS: additional probes (previously missing — caused persistent DRIFT
    # even when the setting was applied by the user or another tool, because
    # current.tweaks.ui.* was never written by the probe so it stayed null/false)
    try {
        $ftype = Get-ItemProperty -Path "HKCU:\SOFTWARE\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags\AllFolders\Shell" -Name "FolderType" -EA SilentlyContinue
        if ($ftype -and $ftype.FolderType -eq "NotSpecified") { $state.tweaks.ui.folderTypeDiscoveryDisabled = $true }
    } catch {}
    try {
        $lnk = Get-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "link" -EA SilentlyContinue
        if ($lnk -and $lnk.link -is [byte[]] -and $lnk.link.Length -eq 4 -and (($lnk.link | Measure-Object -Sum).Sum -eq 0)) {
            $state.tweaks.ui.shortcutSuffixRemoved = $true
        }
    } catch {}
    try {
        $ap = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers" -Name "DisableAutoplay" -EA SilentlyContinue
        if ($ap -and $ap.DisableAutoplay -eq 1) { $state.tweaks.ui.autoPlayDisabled = $true }
    } catch {}
    try {
        $ld = Get-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer" -Name "NoLowDiskSpaceChecks" -EA SilentlyContinue
        if ($ld -and $ld.NoLowDiskSpaceChecks -eq 1) { $state.tweaks.ui.lowDiskCheckDisabled = $true }
    } catch {}
    try {
        $spn = Get-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSyncProviderNotifications" -EA SilentlyContinue
        if ($null -ne $spn -and $spn.ShowSyncProviderNotifications -eq 0) { $state.tweaks.ui.syncProviderNotificationsHidden = $true }
    } catch {}
    try {
        $tr = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name "EnableTransparency" -EA SilentlyContinue
        if ($null -ne $tr -and $tr.EnableTransparency -eq 0) { $state.tweaks.ui.transparencyDisabled = $true }
    } catch {}
    try {
        $fp = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState" -Name "FullPath" -EA SilentlyContinue
        if ($fp -and $fp.FullPath -eq 1) { $state.tweaks.ui.fullPathInTitleBar = $true }
    } catch {}
    try {
        $srch = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search" -Name "SearchboxTaskbarMode" -EA SilentlyContinue
        $tvb  = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowTaskViewButton" -EA SilentlyContinue
        if (($srch -and $srch.SearchboxTaskbarMode -eq 0) -and ($tvb -and $tvb.ShowTaskViewButton -eq 0)) {
            $state.tweaks.ui.taskbarDebloated = $true
        }
    } catch {}
    try {
        $sir = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "Start_IrisRecommendations" -EA SilentlyContinue
        if ($null -ne $sir -and $sir.Start_IrisRecommendations -eq 0) { $state.tweaks.ui.startRecommendationsDisabled = $true }
    } catch {}

    # ════════════════════════════════════════════════════════════════════
    # NETWORK
    # ════════════════════════════════════════════════════════════════════
    try {
        $ipv4 = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters" -Name "DisabledComponents" -EA SilentlyContinue
        if ($ipv4 -and $ipv4.DisabledComponents -eq 32) { $state.network.dns.ipv4Preference = $true }
    }
    catch {}

    # ════════════════════════════════════════════════════════════════════
    # IDENTITY: Branding
    # ════════════════════════════════════════════════════════════════════
    try {
        $brand = Get-ItemProperty -Path "HKCU:\Software\servalabs\WinCommander\Branding" -EA SilentlyContinue
        if ($brand) {
            if ($brand.CompanyName) { $state.identity.branding.companyName = $brand.CompanyName }
            if ($brand.ProductName) { $state.identity.branding.productName = $brand.ProductName }
        }
    }
    catch {}

    try {
        $oem = Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OEMInformation" -EA SilentlyContinue
        if ($oem) {
            if ($oem.Manufacturer) { $state.identity.branding.manufacturer = $oem.Manufacturer }
            if ($oem.SupportURL) { $state.identity.branding.supportUrl = $oem.SupportURL }
        }
        $state.identity.branding.pcName = $env:COMPUTERNAME
    }
    catch {}

    # ════════════════════════════════════════════════════════════════════
    # APPS: Edge & OneDrive removal status
    # ════════════════════════════════════════════════════════════════════
    try {
        $edgePkg = Get-AppxPackage -Name "Microsoft.MicrosoftEdge.Stable" -EA SilentlyContinue
        $edgeExe = Test-Path "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
        if (-not $edgePkg -and -not $edgeExe) { $state.apps.edgeRemoved = $true }
    }
    catch {}

    try {
        $od = Test-Path "$env:LOCALAPPDATA\Microsoft\OneDrive\OneDrive.exe"
        if (-not $od) { $state.apps.onedriveRemoved = $true }
    }
    catch {}

    # ════════════════════════════════════════════════════════════════════
    # PRODUCTIVITY: ActivityWatch tracker
    # ════════════════════════════════════════════════════════════════════
    try {
        $aw = Get-Process -Name "aw-*" -EA SilentlyContinue
        if ($aw) { $state.productivity.trackerEnabled = $true }
    }
    catch {}

    $state
}
