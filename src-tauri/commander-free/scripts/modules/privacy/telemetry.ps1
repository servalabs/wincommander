# ============================================================================
# PRIVACY - TELEMETRY MODULE
# Windows telemetry and tracking controls
# ============================================================================

# Set-RegistryValueSafe moved to core/utils.ps1 so callers in
# tweaks/system.ps1, tweaks/security.ps1 and privacy/cleanup.ps1 can
# reach it. (When only the telemetry module was loaded for a command
# from one of those other modules, the lookup failed with
# "The term 'Set-RegistryValueSafe' is not recognized...".)

function Remove-RegistryValueSafe {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Name
    )
    if (Test-Path $Path) {
        Invoke-7Erase -Path $Path -Type RegistryProperty -Name $Name
    }
}

function Disable-Telemetry {
    Assert-IsAdmin
    try {
        # 1. Registry Keys - Deep Privacy Hardening
        $telemetryKeys = @(
            # System-wide Telemetry
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection"; Name = "AllowTelemetry"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "AllowTelemetry"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "AllowTelemetry"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "DoNotShowFeedbackNotifications"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "LimitDiagnosticLogCollection"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "LimitDiagnosticLogCollection"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "DisableOneSettingsDownloads"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "DisableOneSettingsDownloads"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Privacy"; Name = "TailoredExperiencesWithDiagnosticDataEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Privacy"; Name = "TailoredExperiencesWithDiagnosticDataEnabled"; Value = 0; Type = "DWord" },
            
            # Advertising and Personalization
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo"; Name = "DisabledByGroupPolicy"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo"; Name = "Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo"; Name = "Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Microsoft\SQMClient\Windows"; Name = "CEIPEnable"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\SQMClient\Windows"; Name = "CEIPEnable"; Value = 0; Type = "DWord" },
            
            # Error Reporting and Diag
            @{ Path = "HKCU:\SOFTWARE\Microsoft\Windows\Windows Error Reporting"; Name = "Disabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting"; Name = "Disabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting"; Name = "DontSendAdditionalData"; Value = 1; Type = "DWord" },
            
            # App and Shell Telemetry
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"; Name = "Start_TrackProgs"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "ContentDeliveryAllowed"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "OemPreInstalledAppsEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "PreInstalledAppsEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SilentInstalledAppsEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SystemPaneSuggestionsEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Windows\CloudContent"; Name = "DisableTailoredExperiencesWithDiagnosticData"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "AITEnable"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "AITEnable"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "DisableInventory"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "DisableInventory"; Value = 1; Type = "DWord" },

            # Defender / MRT telemetry reporting
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SpyNetReporting"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SpyNetReporting"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SubmitSamplesConsent"; Value = 2; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SubmitSamplesConsent"; Value = 2; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\MRT"; Name = "DontReportInfectionInformation"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\MRT"; Name = "DontReportInfectionInformation"; Value = 1; Type = "DWord" }
        )
        
        foreach ($key in $telemetryKeys) {
            Set-RegistryValueSafe -Path $key.Path -Name $key.Name -Value $key.Value -Type $key.Type
        }

        # 2. Services - Core Tracking
        $services = @(
            "DiagTrack",          # Connected User Experiences and Telemetry (CRITICAL)
            "dmwappushservice",   # WAP Push Message Routing Service
            "WerSvc",             # Windows Error Reporting Service
            "WMPNetworkSvc",      # Windows Media Player Network Sharing Service
            "diagsvc",            # Diagnostic Service Host
            "diagnosticshub.standardcollector.service" # Microsoft (R) Diagnostics Hub Standard Collector Service
        )
        foreach ($svc in $services) {
            if (Get-Service -Name $svc -ErrorAction SilentlyContinue) {
                Stop-Service -Name $svc -Force -ErrorAction SilentlyContinue
                Set-Service -Name $svc -StartupType Disabled -ErrorAction SilentlyContinue
            }
        }

        # 3. Scheduled Tasks - Comprehensive Cleanup
        $tasks = @(
            '\Microsoft\Windows\Autochk\Proxy',
            '\Microsoft\Windows\Customer Experience Improvement Program\Consolidator',
            '\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip',
            '\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector',
            '\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser',
            '\Microsoft\Windows\Application Experience\ProgramDataUpdater',
            '\Microsoft\Windows\Application Experience\StartupAppScan',
            '\Microsoft\Windows\Application Experience\PcaPatchDbTask',
            '\Microsoft\Windows\Windows Error Reporting\QueueReporting'
        )
        foreach ($task in $tasks) {
            Disable-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue | Out-Null
        }
        
        # ── PowerShell 7 telemetry opt-out ────────────────────────────
        [Environment]::SetEnvironmentVariable('POWERSHELL_TELEMETRY_OPTOUT', '1', 'Machine')

        # ── Office telemetry ───────────────────────────────────────────
        $officeKeys = @(
            @{ Path = "HKCU:\Software\Microsoft\Office\Common\ClientTelemetry"; Name = "DisableTelemetry"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Office\Common\ClientTelemetry"; Name = "DisableTelemetry"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\Common\ClientTelemetry"; Name = "SendTelemetry"; Value = 3; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\Common\ClientTelemetry"; Name = "SendTelemetry"; Value = 3; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "QMEnable"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "QMEnable"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "UpdateReliabilityData"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "UpdateReliabilityData"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "Enablelogging"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "Enablelogging"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "EnableUpload"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "EnableUpload"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common\Feedback"; Name = "Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common\Feedback"; Name = "Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "ControllerConnectedServicesEnabled"; Value = 2; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "ControllerConnectedServicesEnabled"; Value = 2; Type = "DWord" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "DownloadContentDisabled"; Value = 2; Type = "DWord" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "DownloadContentDisabled"; Value = 2; Type = "DWord" }
        )
        foreach ($k in $officeKeys) {
            Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type $k.Type
        }

        return @{ status = 'disabled' }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-Telemetry {
    Assert-IsAdmin
    try {
        # Reverse the telemetry-SPECIFIC registry values Disable-Telemetry set.
        # Deliberately excludes keys OWNED by other toggles — AdvertisingInfo
        # (advertisingId), Start_TrackProgs (jumpLists), ContentDeliveryManager
        # (suggestions), Tailored-Experiences/CloudContent (tailoredExp) — so a
        # telemetry re-enable never silently clears a still-active sibling
        # toggle. Removing a value returns Windows to its default (telemetry on).
        $telemetrySpecificRemovals = @(
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection"; Name = "AllowTelemetry" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "AllowTelemetry" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "AllowTelemetry" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "DoNotShowFeedbackNotifications" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "LimitDiagnosticLogCollection" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "LimitDiagnosticLogCollection" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "DisableOneSettingsDownloads" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection"; Name = "DisableOneSettingsDownloads" },
            @{ Path = "HKCU:\SOFTWARE\Microsoft\SQMClient\Windows"; Name = "CEIPEnable" },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\SQMClient\Windows"; Name = "CEIPEnable" },
            @{ Path = "HKCU:\SOFTWARE\Microsoft\Windows\Windows Error Reporting"; Name = "Disabled" },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting"; Name = "Disabled" },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting"; Name = "DontSendAdditionalData" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "AITEnable" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "AITEnable" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "DisableInventory" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppCompat"; Name = "DisableInventory" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SpyNetReporting" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SpyNetReporting" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SubmitSamplesConsent" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet"; Name = "SubmitSamplesConsent" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\MRT"; Name = "DontReportInfectionInformation" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\MRT"; Name = "DontReportInfectionInformation" }
        )
        foreach ($r in $telemetrySpecificRemovals) {
            Remove-RegistryValueSafe -Path $r.Path -Name $r.Name
        }

        # Restore the 6 services Disable-Telemetry stopped+disabled to their
        # Windows defaults (only DiagTrack is Automatic; the rest are Manual/
        # trigger-start), and start DiagTrack — the primary telemetry service.
        $serviceDefaults = @{
            "DiagTrack"                                 = "Automatic"
            "dmwappushservice"                          = "Manual"
            "WerSvc"                                    = "Manual"
            "WMPNetworkSvc"                             = "Manual"
            "diagsvc"                                   = "Manual"
            "diagnosticshub.standardcollector.service"  = "Manual"
        }
        foreach ($svc in $serviceDefaults.Keys) {
            if (Get-Service -Name $svc -ErrorAction SilentlyContinue) {
                Set-Service -Name $svc -StartupType $serviceDefaults[$svc] -ErrorAction SilentlyContinue
            }
        }
        Start-Service -Name "DiagTrack" -ErrorAction SilentlyContinue

        # Re-enable the 9 scheduled tasks Disable-Telemetry disabled.
        $tasks = @(
            '\Microsoft\Windows\Autochk\Proxy',
            '\Microsoft\Windows\Customer Experience Improvement Program\Consolidator',
            '\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip',
            '\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector',
            '\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser',
            '\Microsoft\Windows\Application Experience\ProgramDataUpdater',
            '\Microsoft\Windows\Application Experience\StartupAppScan',
            '\Microsoft\Windows\Application Experience\PcaPatchDbTask',
            '\Microsoft\Windows\Windows Error Reporting\QueueReporting'
        )
        foreach ($task in $tasks) {
            Enable-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue | Out-Null
        }

        # ── Re-enable PowerShell 7 telemetry ─────────────────────────
        [Environment]::SetEnvironmentVariable('POWERSHELL_TELEMETRY_OPTOUT', $null, 'Machine')

        # ── Re-enable Office telemetry ───────────────────────────────
        $officeRemovals = @(
            @{ Path = "HKCU:\Software\Microsoft\Office\Common\ClientTelemetry"; Name = "DisableTelemetry" },
            @{ Path = "HKLM:\Software\Microsoft\Office\Common\ClientTelemetry"; Name = "DisableTelemetry" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\Common\ClientTelemetry"; Name = "SendTelemetry" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\Common\ClientTelemetry"; Name = "SendTelemetry" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "QMEnable" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "QMEnable" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "UpdateReliabilityData" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common"; Name = "UpdateReliabilityData" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "Enablelogging" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "Enablelogging" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "EnableUpload" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\OSM"; Name = "EnableUpload" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common\Feedback"; Name = "Enabled" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common\Feedback"; Name = "Enabled" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "ControllerConnectedServicesEnabled" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "ControllerConnectedServicesEnabled" },
            @{ Path = "HKCU:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "DownloadContentDisabled" },
            @{ Path = "HKLM:\Software\Policies\Microsoft\Office\16.0\Common\Privacy"; Name = "DownloadContentDisabled" }
        )
        foreach ($r in $officeRemovals) {
            Remove-RegistryValueSafe -Path $r.Path -Name $r.Name
        }

        return @{ status = 'enabled' }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}

function Disable-WindowsSuggestions {
    try {
        $targets = @(
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-353698Enabled"; Value = 0; Type = "DWord" }, # Timeline suggestions
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338388Enabled"; Value = 0; Type = "DWord" }, # Start suggestions
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338389Enabled"; Value = 0; Type = "DWord" }, # Tips and tricks
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338393Enabled"; Value = 0; Type = "DWord" }, # Settings suggestions
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-353694Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-353696Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-310093Enabled"; Value = 0; Type = "DWord" }, # Finish setup nag
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; Name = "ScoobeSystemSettingEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SoftLandingEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SystemPaneSuggestionsEnabled"; Value = 0; Type = "DWord" }
        )
        foreach ($t in $targets) {
            Set-RegistryValueSafe -Path $t.Path -Name $t.Name -Value $t.Value -Type $t.Type
        }
        @{ status = "disabled" }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-WindowsSuggestions {
    try {
        $targets = @(
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-353698Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338388Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338389Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338393Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-353694Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-353696Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-310093Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; Name = "ScoobeSystemSettingEnabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SoftLandingEnabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SystemPaneSuggestionsEnabled"; Value = 1; Type = "DWord" }
        )
        foreach ($t in $targets) {
            Set-RegistryValueSafe -Path $t.Path -Name $t.Name -Value $t.Value -Type $t.Type
        }
        @{ status = "enabled" }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Get-WindowsSuggestionsStatus {
    try {
        $cdm = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" -ErrorAction SilentlyContinue
        $upe = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement" -ErrorAction SilentlyContinue

        $checks = @(
            ($cdm."SubscribedContent-353698Enabled" -eq 0),
            ($cdm."SubscribedContent-338388Enabled" -eq 0),
            ($cdm."SubscribedContent-338389Enabled" -eq 0),
            ($cdm."SubscribedContent-338393Enabled" -eq 0),
            ($cdm."SubscribedContent-353694Enabled" -eq 0),
            ($cdm."SubscribedContent-353696Enabled" -eq 0),
            ($cdm."SubscribedContent-310093Enabled" -eq 0),
            ($upe.ScoobeSystemSettingEnabled -eq 0),
            ($cdm.SoftLandingEnabled -eq 0),
            ($cdm.SystemPaneSuggestionsEnabled -eq 0)
        )
        $disabledCount = ($checks | Where-Object { $_ -eq $true } | Measure-Object).Count

        @{
            disabled = ($disabledCount -ge 6)
        }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message; disabled = $false }
    }
}

function Disable-Copilot {
    try {
        $policyPath = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot"
        Set-RegistryValueSafe -Path $policyPath -Name "TurnOffWindowsCopilot" -Value 1 -Type DWord

        $removed = $false
        $app = Get-AppxPackage -Name "Microsoft.Copilot" -ErrorAction SilentlyContinue
        if ($app) {
            Remove-AppxPackage -Package $app.PackageFullName -ErrorAction SilentlyContinue
            $removed = $true
        }

        return @{ status = "disabled"; appRemoved = $removed }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-Copilot {
    try {
        Remove-RegistryValueSafe -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot" -Name "TurnOffWindowsCopilot"
        return @{ status = "enabled" }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}

# GUIDs Windows uses to identify camera & microphone at the DeviceAccess
# layer. The CapabilityAccessManager actually reads `DeviceAccess\Global\
# {GUID}\Value` BEFORE consulting ConsentStore — that's the real master
# switch the Settings UI flips. Writing only ConsentStore left this at
# "Allow" on a lot of devices, which is why the toggle "worked on some
# PCs and not others" — driver-version-dependent fallback behavior.
$Script:DeviceAccessGuids = @{
    'webcam'     = '{E5323777-F976-4f5b-9B55-B94699C46E44}'
    'microphone' = '{2EEF81BE-33FA-4800-9670-1CD474972C3F}'
    'location'   = '{BFA794E4-F964-4FDB-90F6-51056BFE4B44}'
}

# AppPrivacy GP values used by the policy layer above ConsentStore.
# 0 = user in control, 1 = force allow, 2 = force deny. Force Deny here
# overrides any per-app permission grant — closes the MDM/Intune bypass
# loophole and the "browser ignores ConsentStore" path on Win10 pre-2004.
$Script:AppPrivacyValueNames = @{
    'webcam'     = 'LetAppsAccessCamera'
    'microphone' = 'LetAppsAccessMicrophone'
    'location'   = 'LetAppsAccessLocation'
}

function Set-AppCapabilityAccess {
    param(
        [Parameter(Mandatory = $true)][string]$Capability,
        [Parameter(Mandatory = $true)][ValidateSet("Allow", "Deny")][string]$Access
    )

    # FOUR enforcement layers — Windows resolves access in this order:
    #
    #   1. AppPrivacy GP policy   (HKLM\...\Policies\...\AppPrivacy)
    #      Force Deny (2) or Force Allow (1) overrides everything else.
    #      Mandatory when MDM/Intune is involved.
    #
    #   2. DeviceAccess\Global\{GUID}\Value
    #      The master switch the Settings UI actually toggles.
    #      Windows 24H2 writes this atomically with ConsentStore; older
    #      builds left it alone, which is why our ConsentStore-only
    #      writes worked on some boxes and not others.
    #
    #   3. ConsentStore\<cap>\Value + NonPackaged\Value + per-pkg\Value
    #      The per-app permission tree. Required for the Settings UI
    #      to reflect the change and for UWP apps that consult it.
    #
    #   4. PnP device disable (optional, kernel-level — handled by a
    #      separate `Set-CapabilityHardwareDisable` toggle if the user
    #      wants the nuclear option).
    Assert-IsAdmin
    try {
        $touched = 0
        $isDeny = ($Access -eq 'Deny')

        # ── Layer 0: Capability-specific OS policy (HKCU + HKLM) ──
        # Camera.admx AllowCamera locks the top-level "Camera access" toggle.
        # Written to both hives — same dual-write pattern that makes clipboard's
        # "Managed by your organization" banner work reliably in Windows Settings.
        if ($Capability -eq 'webcam') {
            foreach ($hive in @('HKCU:\SOFTWARE\Policies\Microsoft\Camera', 'HKLM:\SOFTWARE\Policies\Microsoft\Camera')) {
                try {
                    if ($isDeny) {
                        Set-RegistryValueSafe -Path $hive -Name 'AllowCamera' -Value 0 -Type DWord
                    } else {
                        Remove-ItemProperty -Path $hive -Name 'AllowCamera' -ErrorAction SilentlyContinue
                    }
                    $touched++
                } catch {}
            }
        }

        # ── Layer 1: AppPrivacy Group Policy (HKCU + HKLM) ──
        # Locks "Let apps access your camera/microphone" sub-toggle.
        # Dual HKCU+HKLM write mirrors the clipboard approach that reliably
        # shows the "Managed by your organization" banner in Windows Settings.
        if ($Script:AppPrivacyValueNames.ContainsKey($Capability)) {
            $valueName = $Script:AppPrivacyValueNames[$Capability]
            foreach ($hive in @('HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy', 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy')) {
                try {
                    if ($isDeny) {
                        Set-RegistryValueSafe -Path $hive -Name $valueName -Value 2 -Type DWord
                    } else {
                        Remove-ItemProperty -Path $hive -Name $valueName -ErrorAction SilentlyContinue
                    }
                    $touched++
                } catch {}
            }
        }

        # ── Layer 2: DeviceAccess Global master switch ──
        if ($Script:DeviceAccessGuids.ContainsKey($Capability)) {
            $guid = $Script:DeviceAccessGuids[$Capability]
            $deviceAccessPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DeviceAccess\Global\$guid"
            try {
                Set-RegistryValueSafe -Path $deviceAccessPath -Name "Value" -Value $Access -Type String
                $touched++
            } catch {}
        }

        # ── Layer 3: ConsentStore (the existing per-app tree) ──
        $consentRoots = @(
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore",
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore"
        )
        foreach ($root in $consentRoots) {
            $capPath = Join-Path $root $Capability
            try {
                Set-RegistryValueSafe -Path $capPath -Name "Value" -Value $Access -Type String
                $touched++
            } catch {}
            if (Test-Path -LiteralPath $capPath) {
                Get-ChildItem -LiteralPath $capPath -ErrorAction SilentlyContinue | ForEach-Object {
                    try {
                        Set-RegistryValueSafe -Path $_.PSPath -Name "Value" -Value $Access -Type String
                        $touched++
                    } catch {
                        # Per-package subkeys owned by TrustedInstaller can
                        # reject elevated-but-not-SYSTEM writes; skip silently.
                    }
                }
            }
        }

        # ── Policy refresh ──
        # Force Windows Settings to re-read the policy state immediately.
        # Without this the banner and toggle-lock only appear after Settings is reopened.
        try {
            if (-not ([System.Management.Automation.PSTypeName]'WC_PolicyRefresh').Type) {
                Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public class WC_PolicyRefresh {
    // RP_FORCE = 1
    [DllImport("userenv.dll", SetLastError=true)]
    public static extern bool RefreshPolicyEx(bool bMachine, uint dwOptions);
}
'@ -ErrorAction SilentlyContinue
            }
            [WC_PolicyRefresh]::RefreshPolicyEx($true, 1) | Out-Null
        } catch {}

        # ── Location-only: kick the Geolocation Service ──
        # location consent is cached by lfsvc and does NOT take effect until the
        # service re-reads it (unlike mic/camera which apps read live). Flip the
        # service master + restart so a Deny on the location capability actually
        # turns location off now.
        if ($Capability -eq 'location') {
            try { Set-LocationServiceMaster -Access $Access; $touched++ } catch {}
        }

        @{ status = "updated"; capability = $Capability; value = $Access; entriesTouched = $touched }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Get-AppCapabilityAccessStatus {
    param(
        [Parameter(Mandatory = $true)][string]$Capability
    )

    try {
        $path = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\$Capability"
        $value = (Get-ItemProperty -Path $path -Name "Value" -ErrorAction SilentlyContinue).Value
        if ([string]::IsNullOrWhiteSpace($value)) { $value = "Allow" }
        @{
            capability = $Capability
            value      = $value
            disabled   = ($value -eq "Deny")
        }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message; capability = $Capability; value = "Allow"; disabled = $false }
    }
}

function Get-AppPrivacyCapabilitiesStatus {
    try {
        $capabilities = @(
            "webcam",
            "microphone",
            "contacts",
            "appointments",
            "phoneCall",
            "phoneCallHistory",
            "chat",
            "userNotificationListener",
            "documentsLibrary",
            "picturesLibrary",
            "videosLibrary",
            "broadFileSystemAccess",
            "gazeInput",
            "appDiagnostics",
            "userAccountInformation",
            "bluetoothSync"
        )

        $results = @{}
        foreach ($cap in $capabilities) {
            $state = Get-AppCapabilityAccessStatus -Capability $cap
            $results[$cap] = [bool]$state.disabled
        }

        $results
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}



function Disable-LockScreenPrivacy {
    try {
        $keys = @(
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\CloudContent"; Name = "DisableWindowsSpotlightFeatures"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent"; Name = "DisableWindowsSpotlightFeatures"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338387Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338387Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings"; Name = "NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings"; Name = "NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\Personalization"; Name = "NoLockScreenCamera"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization"; Name = "NoLockScreenCamera"; Value = 1; Type = "DWord" }
        )
        foreach ($k in $keys) {
            Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type $k.Type
        }
        @{ status = "disabled" }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-LockScreenPrivacy {
    try {
        $keys = @(
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\CloudContent"; Name = "DisableWindowsSpotlightFeatures"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent"; Name = "DisableWindowsSpotlightFeatures"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338387Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-338387Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings"; Name = "NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings"; Name = "NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\Personalization"; Name = "NoLockScreenCamera"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization"; Name = "NoLockScreenCamera"; Value = 0; Type = "DWord" }
        )
        foreach ($k in $keys) {
            Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type $k.Type
        }
        @{ status = "enabled" }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Get-LockScreenPrivacyStatus {
    try {
        $cloud = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableWindowsSpotlightFeatures" -ErrorAction SilentlyContinue).DisableWindowsSpotlightFeatures
        $toast = (Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings" -Name "NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK" -ErrorAction SilentlyContinue).NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK
        $camera = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\Personalization" -Name "NoLockScreenCamera" -ErrorAction SilentlyContinue).NoLockScreenCamera
        @{ disabled = ($cloud -eq 1 -and $toast -eq 0 -and $camera -eq 1) }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message; disabled = $false }
    }
}

function Disable-SetupCompletionNags {
    try {
        $keys = @(
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-310093Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-310093Enabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; Name = "ScoobeSystemSettingEnabled"; Value = 0; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; Name = "ScoobeSystemSettingEnabled"; Value = 0; Type = "DWord" }
        )
        foreach ($k in $keys) {
            Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type $k.Type
        }
        @{ status = "disabled" }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-SetupCompletionNags {
    try {
        $keys = @(
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-310093Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"; Name = "SubscribedContent-310093Enabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; Name = "ScoobeSystemSettingEnabled"; Value = 1; Type = "DWord" },
            @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement"; Name = "ScoobeSystemSettingEnabled"; Value = 1; Type = "DWord" }
        )
        foreach ($k in $keys) {
            Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type $k.Type
        }
        @{ status = "enabled" }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Get-SetupCompletionNagsStatus {
    try {
        $cdm = (Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" -Name "SubscribedContent-310093Enabled" -ErrorAction SilentlyContinue)."SubscribedContent-310093Enabled"
        $scoobe = (Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement" -Name "ScoobeSystemSettingEnabled" -ErrorAction SilentlyContinue).ScoobeSystemSettingEnabled
        @{ disabled = ($cdm -eq 0 -and $scoobe -eq 0) }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message; disabled = $false }
    }
}

# --- TRACKING ---

function Disable-ActivityHistory {
    Assert-IsAdmin
    foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
        if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
        Set-ItemProperty -Path $p -Name "EnableActivityFeed" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $p -Name "PublishUserActivities" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $p -Name "UploadUserActivities" -Value 0 -Type DWord -Force
    }
    return @{ status = 'disabled' }
}

function Enable-ActivityHistory {
    Assert-IsAdmin
    foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
        Remove-ItemProperty -Path $p -Name "EnableActivityFeed" -ErrorAction SilentlyContinue
        Remove-ItemProperty -Path $p -Name "PublishUserActivities" -ErrorAction SilentlyContinue
        Remove-ItemProperty -Path $p -Name "UploadUserActivities" -ErrorAction SilentlyContinue
    }
    return @{ status = 'enabled' }
}

# The Win11 "Location services" MASTER toggle is backed by the lfsvc
# (Geolocation Service) configuration, NOT just the ConsentStore. Unlike
# mic/camera — whose consent apps read live — location consent is cached by
# lfsvc and only re-read when the service restarts. That's why writing
# ConsentStore\location\Value=Deny turned mic off but left location on: the
# service kept serving the old state. `Set-LocationServiceMaster` writes the
# service master value AND restarts lfsvc so the change takes effect now.
function Set-LocationServiceMaster {
    param([Parameter(Mandatory = $true)][ValidateSet("Deny", "Allow")][string]$Access)
    $status = "HKLM:\SYSTEM\CurrentControlSet\Services\lfsvc\Service\Configuration\Status"
    if (!(Test-Path $status)) { New-Item -Path $status -Force | Out-Null }
    # Configuration\Status\Value: 1 = location on, 0 = off (the value the
    # Settings "Location services" switch itself writes).
    Set-ItemProperty -Path $status -Name "Value" -Value ($(if ($Access -eq 'Deny') { 0 } else { 1 })) -Type DWord -Force

    # The Service Manager tweak's "Recommended profile" (service-manager.ps1,
    # $Global:WincmdServiceRecommendations) sets lfsvc's startup type to
    # Disabled. A Disabled service can never be started, so if that tweak ran
    # first, Restart-Service/Start-Service below would silently no-op forever
    # (the failure is swallowed) and location would stay dead even though every
    # consent/policy registry value correctly says "Allow". Restore a startable
    # mode before attempting to start it.
    if ($Access -eq 'Allow') {
        try {
            $svc = Get-Service -Name lfsvc -ErrorAction Stop
            if ($svc.StartType -eq 'Disabled') {
                Set-Service -Name lfsvc -StartupType Manual -ErrorAction SilentlyContinue
            }
        } catch {}
    }

    # Restart so lfsvc re-reads the master + ConsentStore immediately instead of
    # after the next reboot. Best-effort: if the service is disabled/locked the
    # registry state still applies on next start.
    try { Restart-Service -Name lfsvc -Force -ErrorAction Stop } catch {
        try { Stop-Service -Name lfsvc -Force -ErrorAction SilentlyContinue } catch {}
        try { Start-Service -Name lfsvc -ErrorAction SilentlyContinue } catch {}
    }
}

function Disable-LocationTracking {
    Assert-IsAdmin
    $path1 = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location"
    if (!(Test-Path $path1)) { New-Item -Path $path1 -Force | Out-Null }
    Set-ItemProperty -Path $path1 -Name "Value" -Value "Deny" -Type String -Force

    # HKCU+HKLM policy — triggers "Managed by your organization" in Settings > Location
    foreach ($gp in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors")) {
        if (!(Test-Path $gp)) { New-Item -Path $gp -Force | Out-Null }
        Set-ItemProperty -Path $gp -Name "DisableLocation" -Value 1 -Type DWord -Force
    }

    $path2 = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Sensor\Overrides\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}"
    if (!(Test-Path $path2)) { New-Item -Path $path2 -Force | Out-Null }
    Set-ItemProperty -Path $path2 -Name "SensorPermissionState" -Value 0 -Type DWord -Force

    $path4 = "HKLM:\SYSTEM\Maps"
    if (!(Test-Path $path4)) { New-Item -Path $path4 -Force | Out-Null }
    Set-ItemProperty -Path $path4 -Name "AutoUpdateEnabled" -Value 0 -Type DWord -Force

    # Also assert the App-Capability enforcement layers for "location"
    # (AppPrivacy GP force-deny, DeviceAccess Global master switch, HKCU +
    # per-package ConsentStore) via the shared capability function — this is
    # the one authoritative "block location" switch, so it must cover every
    # layer Windows can gate location through, not just the ones this
    # function historically knew about.
    Set-AppCapabilityAccess -Capability 'location' -Access 'Deny' | Out-Null

    # The load-bearing step for location specifically: flip + restart lfsvc.
    Set-LocationServiceMaster -Access 'Deny'
    return @{ status = 'disabled'; disabled = (Get-LocationTrackingStatus).disabled }
}

# Comprehensive location-access check across every layer Windows (and this
# app's own Set-AppCapabilityAccess) can independently gate location through.
# A single-key ConsentStore read disagrees with reality whenever a
# higher-priority layer — AppPrivacy force-deny, the DeviceAccess Global
# master switch, LocationAndSensors GP, or a disabled lfsvc startup type —
# is still blocking access; this is the one place both the toggle's status
# probe and its post-apply verification read from, so they can't disagree.
function Get-LocationTrackingStatus {
    try {
        $appPrivacyHKLM = (Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy" -Name "LetAppsAccessLocation" -ErrorAction SilentlyContinue).LetAppsAccessLocation
        $appPrivacyHKCU = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy" -Name "LetAppsAccessLocation" -ErrorAction SilentlyContinue).LetAppsAccessLocation
        $deviceAccess = (Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DeviceAccess\Global\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}" -Name "Value" -ErrorAction SilentlyContinue).Value
        $gpHKLM = (Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors" -Name "DisableLocation" -ErrorAction SilentlyContinue).DisableLocation
        $gpHKCU = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors" -Name "DisableLocation" -ErrorAction SilentlyContinue).DisableLocation
        $consentHKLM = (Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location" -Name "Value" -ErrorAction SilentlyContinue).Value
        $consentHKCU = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location" -Name "Value" -ErrorAction SilentlyContinue).Value
        $svc = Get-Service -Name lfsvc -ErrorAction SilentlyContinue

        $blocked = (
            $appPrivacyHKLM -eq 2 -or $appPrivacyHKCU -eq 2 -or
            $deviceAccess -eq 'Deny' -or
            $gpHKLM -eq 1 -or $gpHKCU -eq 1 -or
            $consentHKLM -eq 'Deny' -or $consentHKCU -eq 'Deny' -or
            ($svc -and $svc.StartType -eq 'Disabled')
        )
        @{ disabled = [bool]$blocked }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message; disabled = $false }
    }
}

function Enable-LocationTracking {
    Assert-IsAdmin
    $path1 = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location"
    if (Test-Path $path1) { Set-ItemProperty -Path $path1 -Name "Value" -Value "Allow" -Type String -Force }

    # Remove policy keys from both hives so Settings banner disappears
    foreach ($gp in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors")) {
        Remove-ItemProperty -Path $gp -Name "DisableLocation" -Force -ErrorAction SilentlyContinue
    }

    $path2 = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Sensor\Overrides\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}"
    if (Test-Path $path2) { Set-ItemProperty -Path $path2 -Name "SensorPermissionState" -Value 1 -Type DWord -Force }

    # Also clear the App-Capability enforcement layers for "location"
    # (AppPrivacy GP force-deny, DeviceAccess Global master switch, HKCU +
    # per-package ConsentStore). Those are a HIGHER-PRIORITY layer than the
    # ConsentStore write above and this function never touched them before —
    # if location was ever denied through Set-AppCapabilityAccess (e.g. a
    # past app-capability toggle), this toggle alone could report "enabled"
    # while access stayed dead, because the force-deny above it was untouched.
    Set-AppCapabilityAccess -Capability 'location' -Access 'Allow' | Out-Null

    Set-LocationServiceMaster -Access 'Allow'
    return @{ status = 'enabled'; disabled = (Get-LocationTrackingStatus).disabled }
}

function Get-TelemetryStatus {
    $diagTrack = Get-Service -Name 'DiagTrack' -ErrorAction SilentlyContinue
    $telemetryValue = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Name 'AllowTelemetry' -ErrorAction SilentlyContinue
    
    $serviceRunning = ($diagTrack -and $diagTrack.Status -eq 'Running')
    $telemetryLevel = if ($telemetryValue) { [int]$telemetryValue.AllowTelemetry } else { $null }
    @{
        serviceRunning = $serviceRunning
        telemetryLevel = $telemetryLevel
        blocked        = (-not $serviceRunning -and $telemetryLevel -eq 0)
    }
}

# NEW: Recall Snapshots (Windows 11 24H2+)
# ============================================================================

function Disable-RecallSnapshots {
    Assert-IsAdmin
    try {
        # Disable Windows Recall (AI Snapshots)
        $path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "DisableAIDataAnalysis" -Value 1 -Type DWord -Force

        # Also disable for current user
        $upath = "HKCU:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI"
        if (!(Test-Path $upath)) { New-Item -Path $upath -Force | Out-Null }
        Set-ItemProperty -Path $upath -Name "DisableAIDataAnalysis" -Value 1 -Type DWord -Force

        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-RecallSnapshots {
    try {
        Remove-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI" -Name "DisableAIDataAnalysis"
        Remove-RegistryValueSafe -Path "HKCU:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI" -Name "DisableAIDataAnalysis"
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: Typing Insights
# ============================================================================

function Disable-TypingInsights {
    try {
        $path = "HKCU:\Software\Microsoft\Input\Settings"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "InsightsEnabled" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-TypingInsights {
    try {
        $path = "HKCU:\Software\Microsoft\Input\Settings"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "InsightsEnabled" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-TypingInsightsStatus {
    try {
        $path = "HKCU:\Software\Microsoft\Input\Settings"
        $val = (Get-ItemProperty -Path $path -Name "InsightsEnabled" -ErrorAction SilentlyContinue).InsightsEnabled
        @{ disabled = ($val -eq 0) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; disabled = $false } }
}

# ============================================================================
# NEW: Internet Communication Restrictions (from ReviOS privacy.yml)
# ============================================================================

# Single source of truth for the "restricted" registry state, shared by
# Disable-InternetCommunication (Apply) and Get-InternetCommunicationStatus
# (probe) so the two can never disagree — same precedent as WC_DIAG_ETW_LOGGERS.
$Script:WC_INTERNET_COMM_KEYS = @(
    # HKCU restrictions
    @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoPublishingWizard"; Value = 1 },
    @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoWebServices"; Value = 1 },
    @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoOnlinePrintsWizard"; Value = 1 },
    @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoInternetOpenWith"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Windows NT\Printers"; Name = "DisableHTTPPrinting"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Windows NT\Printers"; Name = "DisableWebPnPDownload"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Windows\HandwritingErrorReports"; Name = "PreventHandwritingErrorReports"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Windows\TabletPC"; Name = "PreventHandwritingDataSharing"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Assistance\Client\1.0"; Name = "NoOnlineAssist"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Assistance\Client\1.0"; Name = "NoExplicitFeedback"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Assistance\Client\1.0"; Name = "NoImplicitFeedback"; Value = 1 },
    # HKLM restrictions
    @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoPublishingWizard"; Value = 1 },
    @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoWebServices"; Value = 1 },
    @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoOnlinePrintsWizard"; Value = 1 },
    @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"; Name = "NoInternetOpenWith"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\PCHealth\HelpSvc"; Name = "Headlines"; Value = 0 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\PCHealth\HelpSvc"; Name = "MicrosoftKBSearch"; Value = 0 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\PCHealth\ErrorReporting"; Name = "DoReport"; Value = 0 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows\Windows Error Reporting"; Name = "Disabled"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows\Internet Connection Wizard"; Name = "ExitOnMSICW"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\EventViewer"; Name = "MicrosoftEventVwrDisableLinks"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows\Registration Wizard Control"; Name = "NoRegistration"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\SearchCompanion"; Name = "DisableContentFileUpdates"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows NT\Printers"; Name = "DisableHTTPPrinting"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows NT\Printers"; Name = "DisableWebPnPDownload"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows\HandwritingErrorReports"; Name = "PreventHandwritingErrorReports"; Value = 1 },
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows\TabletPC"; Name = "PreventHandwritingDataSharing"; Value = 1 },
    # Messaging
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows\Messaging"; Name = "AllowMessageSync"; Value = 0 },
    # Edge UI help tips + tracking
    @{ Path = "HKLM:\Software\Policies\Microsoft\Windows\EdgeUI"; Name = "DisableHelpSticker"; Value = 1 },
    @{ Path = "HKCU:\Software\Policies\Microsoft\Windows\EdgeUI"; Name = "DisableMFUTracking"; Value = 1 }
)

function Disable-InternetCommunication {
    Assert-IsAdmin
    try {
        foreach ($k in $Script:WC_INTERNET_COMM_KEYS) {
            Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type DWord
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-InternetCommunication {
    Assert-IsAdmin
    try {
        # Reverse EVERY key Disable-InternetCommunication set, from the same
        # single source of truth — the old hand-picked 11-entry removal list
        # left ~19 of the 30 restrictions (PCHealth, Error Reporting, Internet
        # Connection Wizard, EventViewer, Registration/Search, Printers,
        # Handwriting/TabletPC, Assistance-Client) permanently applied.
        foreach ($k in $Script:WC_INTERNET_COMM_KEYS) {
            Remove-RegistryValueSafe -Path $k.Path -Name $k.Name
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-InternetCommunicationStatus {
    $matched = 0
    foreach ($k in $Script:WC_INTERNET_COMM_KEYS) {
        $val = (Get-ItemProperty -Path $k.Path -Name $k.Name -ErrorAction SilentlyContinue).($k.Name)
        if ($val -eq $k.Value) { $matched++ }
    }
    @{
        restricted     = ($matched -eq $Script:WC_INTERNET_COMM_KEYS.Count)
        keysMatched    = $matched
        keysTotal      = $Script:WC_INTERNET_COMM_KEYS.Count
    }
}

# ============================================================================
# NVIDIA Telemetry Opt-Out
# ============================================================================

function Disable-NvidiaTelemetry {
    Assert-IsAdmin
    try {
        $keys = @(
            @{ Path = "HKLM:\SOFTWARE\NVIDIA Corporation\NvControlPanel2\Client"; Name = "OptInOrOutPreference"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\NVIDIA Corporation\Global\FTS"; Name = "EnableRID44231"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\NVIDIA Corporation\Global\FTS"; Name = "EnableRID64640"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\NVIDIA Corporation\Global\FTS"; Name = "EnableRID66610"; Value = 0 },
            @{ Path = "HKLM:\SYSTEM\CurrentControlSet\Services\NvTelemetryContainer"; Name = "Start"; Value = 4 }
        )
        foreach ($k in $keys) {
            Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type DWord
        }
        # Stop NVIDIA telemetry service
        Stop-Service -Name "NvTelemetryContainer" -Force -ErrorAction SilentlyContinue
        # Disable NVIDIA telemetry tasks
        Get-ScheduledTask -TaskPath "\NvTelemetryContainer\*" -ErrorAction SilentlyContinue | Disable-ScheduledTask -ErrorAction SilentlyContinue | Out-Null
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-NvidiaTelemetry {
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\NVIDIA Corporation\NvControlPanel2\Client" -Name "OptInOrOutPreference" -ErrorAction SilentlyContinue
        $svcPath = "HKLM:\SYSTEM\CurrentControlSet\Services\NvTelemetryContainer"
        if (Test-Path $svcPath) { Set-ItemProperty -Path $svcPath -Name "Start" -Value 2 -Type DWord -Force }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# DOTNET CLI Telemetry Opt-Out (env var)
# ============================================================================

function Disable-DotnetTelemetry {
    Assert-IsAdmin
    try {
        [Environment]::SetEnvironmentVariable('DOTNET_CLI_TELEMETRY_OPTOUT', '1', 'Machine')
        [Environment]::SetEnvironmentVariable('POWERSHELL_TELEMETRY_OPTOUT', '1', 'Machine')
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-DotnetTelemetry {
    Assert-IsAdmin
    try {
        [Environment]::SetEnvironmentVariable('DOTNET_CLI_TELEMETRY_OPTOUT', $null, 'Machine')
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# WMI Autologger Disabling (DiagTrack, SQM, SetupPlatformTel)
# ============================================================================

function Disable-WMIAutologgers {
    Assert-IsAdmin
    try {
        $loggers = @("Diagtrack-Listener", "SQMLogger", "SetupPlatformTel", "CloudExperienceHostOobe", "NtfsLog")
        foreach ($logger in $loggers) {
            $path = "HKLM:\SYSTEM\CurrentControlSet\Control\WMI\Autologger\$logger"
            if (Test-Path $path) {
                Set-ItemProperty -Path $path -Name "Start" -Value 0 -Type DWord -Force
            }
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-WMIAutologgers {
    Assert-IsAdmin
    try {
        $loggers = @("Diagtrack-Listener", "SQMLogger", "SetupPlatformTel", "CloudExperienceHostOobe", "NtfsLog")
        foreach ($logger in $loggers) {
            $path = "HKLM:\SYSTEM\CurrentControlSet\Control\WMI\Autologger\$logger"
            if (Test-Path $path) {
                Set-ItemProperty -Path $path -Name "Start" -Value 1 -Type DWord -Force
            }
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# RSoP (Group Policy) Logging disable (improves boot time)
# ============================================================================

function Disable-RSoPLogging {
    Assert-IsAdmin
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "DisableBkGndGroupPolicy" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "DisableLogonBackgroundImage" -Value 0 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-RSoPLogging {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "DisableBkGndGroupPolicy" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Firewall Outbound Rules for Telemetry (DiagTrack, WerSvc)
# ============================================================================

function Enable-TelemetryFirewallRules {
    Assert-IsAdmin
    try {
        $rules = @(
            @{ Name = "WC_Block_DiagTrack"; DisplayName = "WinCommander: Block DiagTrack"; Program = "%SystemRoot%\System32\svchost.exe"; Service = "DiagTrack" },
            @{ Name = "WC_Block_WerSvc"; DisplayName = "WinCommander: Block WerSvc"; Program = "%SystemRoot%\System32\svchost.exe"; Service = "WerSvc" },
            @{ Name = "WC_Block_dmwappush"; DisplayName = "WinCommander: Block dmwappushservice"; Program = "%SystemRoot%\System32\svchost.exe"; Service = "dmwappushservice" }
        )
        foreach ($r in $rules) {
            Remove-NetFirewallRule -Name $r.Name -ErrorAction SilentlyContinue
            New-NetFirewallRule -Name $r.Name -DisplayName $r.DisplayName -Direction Outbound -Action Block -Program $r.Program -Service $r.Service -Enabled True -ErrorAction SilentlyContinue | Out-Null
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-TelemetryFirewallRules {
    Assert-IsAdmin
    try {
        $rules = @("WC_Block_DiagTrack", "WC_Block_WerSvc", "WC_Block_dmwappush")
        foreach ($r in $rules) {
            Remove-NetFirewallRule -Name $r -ErrorAction SilentlyContinue
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Online Speech Recognition
# ============================================================================

function Disable-OnlineSpeechRecognition {
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Speech_OneCore\Settings\OnlineSpeechPrivacy" -Name "HasAccepted" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\InputPersonalization" -Name "AllowInputPersonalization" -Value 0 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-OnlineSpeechRecognition {
    try {
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Speech_OneCore\Settings\OnlineSpeechPrivacy" -Name "HasAccepted" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\InputPersonalization" -Name "AllowInputPersonalization" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Inking & Typing Personalization
# ============================================================================

function Disable-InkingTypingPersonalization {
    try {
        $keys = @(
            @{ Path = "HKCU:\Software\Microsoft\InputPersonalization"; Name = "RestrictImplicitInkCollection"; Value = 1 },
            @{ Path = "HKCU:\Software\Microsoft\InputPersonalization"; Name = "RestrictImplicitTextCollection"; Value = 1 },
            @{ Path = "HKCU:\Software\Microsoft\InputPersonalization\TrainedDataStore"; Name = "HarvestContacts"; Value = 0 },
            @{ Path = "HKCU:\Software\Microsoft\Personalization\Settings"; Name = "AcceptedPrivacyPolicy"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\InputPersonalization"; Name = "RestrictImplicitInkCollection"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\InputPersonalization"; Name = "RestrictImplicitTextCollection"; Value = 1 }
        )
        foreach ($k in $keys) { Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type DWord }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-InkingTypingPersonalization {
    try {
        $removals = @(
            @{ Path = "HKCU:\Software\Microsoft\InputPersonalization"; Name = "RestrictImplicitInkCollection" },
            @{ Path = "HKCU:\Software\Microsoft\InputPersonalization"; Name = "RestrictImplicitTextCollection" },
            @{ Path = "HKCU:\Software\Microsoft\InputPersonalization\TrainedDataStore"; Name = "HarvestContacts" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\InputPersonalization"; Name = "RestrictImplicitInkCollection" },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\InputPersonalization"; Name = "RestrictImplicitTextCollection" }
        )
        foreach ($r in $removals) { Remove-ItemSecure -Path $r.Path -Name $r.Name -ErrorAction SilentlyContinue }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: HTTP Accept Language Opt-Out
# ============================================================================

function Disable-HttpAcceptLanguageOptOut {
    try {
        Set-RegistryValueSafe -Path "HKCU:\Control Panel\International\User Profile" -Name "HttpAcceptLanguageOptOut" -Value 1 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-HttpAcceptLanguageOptOut {
    try {
        Remove-ItemSecure -Path "HKCU:\Control Panel\International\User Profile" -Name "HttpAcceptLanguageOptOut" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Diagnostic Data Viewer + Feedback Frequency
# ============================================================================

function Disable-DiagnosticDataViewer {
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection" -Name "DisableDiagnosticDataViewer" -Value 1 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-DiagnosticDataViewer {
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection" -Name "DisableDiagnosticDataViewer" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Set-FeedbackFrequencyNever {
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Siuf\Rules" -Name "NumberOfSIUFInPeriod" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Siuf\Rules" -Name "PeriodInNanoSeconds" -Value 0 -Type DWord
        @{ status = "never" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Reset-FeedbackFrequency {
    try {
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Siuf\Rules" -Name "NumberOfSIUFInPeriod" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Siuf\Rules" -Name "PeriodInNanoSeconds" -ErrorAction SilentlyContinue
        @{ status = "reset" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Wi-Fi Sense
# ============================================================================

function Disable-WiFiSense {
    try {
        $keys = @(
            @{ Path = "HKLM:\SOFTWARE\Microsoft\PolicyManager\default\WiFi\AllowWiFiHotSpotReporting"; Name = "Value"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\PolicyManager\default\WiFi\AllowAutoConnectToWiFiSenseHotspots"; Name = "Value"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\WcmSvc\wifinetworkmanager\config"; Name = "AutoConnectAllowedOEM"; Value = 0 }
        )
        foreach ($k in $keys) { Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type DWord }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-WiFiSense {
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\PolicyManager\default\WiFi\AllowWiFiHotSpotReporting" -Name "Value" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\PolicyManager\default\WiFi\AllowAutoConnectToWiFiSenseHotspots" -Name "Value" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Cross-Device Resume (CDP)
# ============================================================================

function Disable-CrossDeviceResume {
    try {
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "EnableCdp" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\CDP" -Name "RomeSdkChannelUserAuthzPolicy" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\CDP" -Name "CdpSessionUserAuthzPolicy" -Value 0 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-CrossDeviceResume {
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "EnableCdp" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\CDP" -Name "RomeSdkChannelUserAuthzPolicy" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\CDP" -Name "CdpSessionUserAuthzPolicy" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Bulk Capability Permissions (all non-essential → Deny)
# ============================================================================

function Disable-BulkCapabilityPermissions {
    Assert-IsAdmin
    try {
        $capabilities = @(
            "location", "webcam", "microphone", "userNotificationListener",
            "activity", "userAccountInformation", "contacts", "appointments",
            "phoneCall", "phoneCallHistory", "email", "userDataTasks", "chat",
            "radios", "bluetoothSync", "appDiagnostics",
            "documentsLibrary", "picturesLibrary", "videosLibrary", "broadFileSystemAccess",
            "gazeInput"
        )
        $roots = @(
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore",
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore"
        )
        foreach ($root in $roots) {
            foreach ($cap in $capabilities) {
                $path = Join-Path $root $cap
                Set-RegistryValueSafe -Path $path -Name "Value" -Value "Deny" -Type String
            }
        }
        @{ status = "denied"; count = $capabilities.Count }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-BulkCapabilityPermissions {
    Assert-IsAdmin
    try {
        $capabilities = @(
            "location", "webcam", "microphone", "userNotificationListener",
            "activity", "userAccountInformation", "contacts", "appointments",
            "phoneCall", "phoneCallHistory", "email", "userDataTasks", "chat",
            "radios", "bluetoothSync", "appDiagnostics",
            "documentsLibrary", "picturesLibrary", "videosLibrary", "broadFileSystemAccess",
            "gazeInput"
        )
        $roots = @(
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore",
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore"
        )
        foreach ($root in $roots) {
            foreach ($cap in $capabilities) {
                $path = Join-Path $root $cap
                Set-RegistryValueSafe -Path $path -Name "Value" -Value "Allow" -Type String
            }
        }
        @{ status = "allowed" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Sync Settings (all off)
# ============================================================================

function Disable-SyncSettings {
    try {
        $keys = @(
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync"; Name = "SyncPolicy"; Value = 5 },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\Accessibility"; Name = "Enabled"; Value = 0 },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\AppSync"; Name = "Enabled"; Value = 0 },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\BrowserSettings"; Name = "Enabled"; Value = 0 },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\Credentials"; Name = "Enabled"; Value = 0 },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\Language"; Name = "Enabled"; Value = 0 },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\Personalization"; Name = "Enabled"; Value = 0 },
            @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\Windows"; Name = "Enabled"; Value = 0 }
        )
        foreach ($k in $keys) { Set-RegistryValueSafe -Path $k.Path -Name $k.Name -Value $k.Value -Type DWord }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-SyncSettings {
    try {
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync" -Name "SyncPolicy" -ErrorAction SilentlyContinue
        $groups = @("Accessibility", "AppSync", "BrowserSettings", "Credentials", "Language", "Personalization", "Windows")
        foreach ($g in $groups) {
            Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\Groups\$g" -Name "Enabled" -ErrorAction SilentlyContinue
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Advertising ID Full Nuke
# ============================================================================

function Disable-AdvertisingID {
    try {
        # Disable the advertising ID entirely
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo" -Name "Enabled" -Value 0 -Type DWord
        # Delete the actual advertising ID value
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo" -Name "Id" -ErrorAction SilentlyContinue
        # HKCU+HKLM policy — triggers "Managed by your organization" in Settings > Privacy > General
        foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo")) {
            Set-RegistryValueSafe -Path $p -Name "DisabledByGroupPolicy" -Value 1 -Type DWord
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AdvertisingID {
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo" -Name "Enabled" -Value 1 -Type DWord
        # Remove policy keys from both hives so Settings banner disappears
        foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo")) {
            Remove-ItemSecure -Path $p -Name "DisabledByGroupPolicy" -ErrorAction SilentlyContinue
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Tailored Experiences (diagnostic data-based personalization)
# ============================================================================

function Disable-TailoredExperiences {
    try {
        Set-RegistryValueSafe -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Privacy" -Name "TailoredExperiencesWithDiagnosticDataEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "HKCU:\Software\Policies\Microsoft\Windows\CloudContent" -Name "DisableTailoredExperiencesWithDiagnosticData" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableTailoredExperiencesWithDiagnosticData" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CPSS\DevicePolicy\TailoredExperiencesWithDiagnosticDataEnabled" -Name "DefaultValue" -Value 0 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-TailoredExperiences {
    try {
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Privacy" -Name "TailoredExperiencesWithDiagnosticDataEnabled" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Software\Policies\Microsoft\Windows\CloudContent" -Name "DisableTailoredExperiencesWithDiagnosticData" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent" -Name "DisableTailoredExperiencesWithDiagnosticData" -ErrorAction SilentlyContinue
        # 4th key Disable-TailoredExperiences sets — was previously left applied.
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CPSS\DevicePolicy\TailoredExperiencesWithDiagnosticDataEnabled" -Name "DefaultValue" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Office Click-to-Run Logging Disable
# ============================================================================

function Disable-OfficeLogging {
    Assert-IsAdmin
    try {
        $officePaths = @(
            "HKLM:\SOFTWARE\Policies\Microsoft\Office\16.0\osm",
            "HKLM:\SOFTWARE\Policies\Microsoft\Office\15.0\osm"
        )
        foreach ($p in $officePaths) {
            Set-RegistryValueSafe -Path $p -Name "Enablelogging" -Value 0 -Type DWord
            Set-RegistryValueSafe -Path $p -Name "EnableUpload" -Value 0 -Type DWord
        }
        # Disable Office telemetry agent scheduled tasks
        Get-ScheduledTask -TaskPath "\Microsoft\Office\*" -ErrorAction SilentlyContinue |
            Where-Object { $_.TaskName -match 'OfficeTelemetry|Telemetry' } |
            Disable-ScheduledTask -ErrorAction SilentlyContinue | Out-Null
        # Click-to-Run logging management
        Set-RegistryValueSafe -Path "HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration" -Name "DisableLogManagement" -Value 1 -Type DWord
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-OfficeLogging {
    Assert-IsAdmin
    try {
        $officePaths = @(
            "HKLM:\SOFTWARE\Policies\Microsoft\Office\16.0\osm",
            "HKLM:\SOFTWARE\Policies\Microsoft\Office\15.0\osm"
        )
        foreach ($p in $officePaths) {
            Remove-ItemSecure -Path $p -Name "Enablelogging" -ErrorAction SilentlyContinue
            Remove-ItemSecure -Path $p -Name "EnableUpload" -ErrorAction SilentlyContinue
        }
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration" -Name "DisableLogManagement" -ErrorAction SilentlyContinue
        # Re-enable the Office telemetry scheduled tasks Disable-OfficeLogging
        # turned off (registry-only reversal previously left them disabled).
        Get-ScheduledTask -TaskPath "\Microsoft\Office\*" -ErrorAction SilentlyContinue |
            Where-Object { $_.TaskName -match 'OfficeTelemetry|Telemetry' } |
            Enable-ScheduledTask -ErrorAction SilentlyContinue | Out-Null
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Privacy: Diagnostic Event Tracing (ETW Sessions) Disable
# ============================================================================
#
# KT: Apply / Probe / dead-code lists used to disagree on which ETW autologgers
# count as "diagnostic tracing". The probe in tweaks/system.ps1 was checking
# WdiContextLog but Apply never touched it — Windows re-enables WdiContextLog
# on sleep/wake (it's tied to DPS), which made the toggle "go off after some
# time" even though everything Apply did touch stayed off. Keep this list as
# the single source of truth and mirror it in tweaks/system.ps1's probe.

$Script:WC_DIAG_ETW_LOGGERS = @(
    'SleepStudy',
    'WiFiSession',
    'WdiContextLog',
    'WdiAutologger',
    'Kernel-Processor-Power',
    'UserModePowerService',
    'NBIFace',
    'RadioMgr',
    'Diagtrack-Listener',
    'LwtNetLog'
)

$Script:WC_DIAG_ETW_TASKS = @(
    @{ Path = '\Microsoft\Windows\Power Efficiency Diagnostics'; Name = 'AnalyzeSystem' },
    @{ Path = '\Microsoft\Windows\DiskDiagnostic'; Name = 'Microsoft-Windows-DiskDiagnosticDataCollector' }
)

# ============================================================================
# KT: Most ETW Autologger keys under HKLM\SYSTEM\...\WMI\Autologger\<NAME> are
# owned by NT SERVICE\TrustedInstaller and grant Administrators only ReadKey.
# A plain `Set-ItemProperty -Force` from an admin shell fails with "Requested
# registry access is not allowed" — the previous implementation swallowed that
# error via -EA SilentlyContinue and reported success anyway, so the toggle
# would appear to work and then revert on the next probe.
#
# To actually write `Start=0`, we have to:
#   1. Enable SeTakeOwnership / SeRestore / SeBackup in our token (admins
#      have these privileges by default but Windows leaves them disabled).
#   2. Take ownership as Administrators.
#   3. Add an Administrators FullControl access rule.
#   4. Write Start.
#   5. Restore the original ACL (which puts the owner back to TrustedInstaller
#      and removes our temporary rule).
# We stop short of re-locking when we can't capture the original ACL — but
# in practice Get-Acl always succeeds when Test-Path does.
# ============================================================================

function _WC-EnsureTokenPrivileges {
    if ($Script:WC_TOKEN_PRIVILEGES_ENABLED) { return }

    if (-not ('WC.TokenPriv' -as [type])) {
        Add-Type -ErrorAction Stop -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
namespace WC {
    public static class TokenPriv {
        [StructLayout(LayoutKind.Sequential, Pack = 1)]
        public struct TokPriv1Luid { public int Count; public long Luid; public int Attr; }
        const int SE_PRIVILEGE_ENABLED    = 0x00000002;
        const int TOKEN_QUERY             = 0x00000008;
        const int TOKEN_ADJUST_PRIVILEGES = 0x00000020;
        [DllImport("kernel32.dll", ExactSpelling = true)]
        static extern IntPtr GetCurrentProcess();
        [DllImport("advapi32.dll", ExactSpelling = true, SetLastError = true)]
        static extern bool OpenProcessToken(IntPtr h, int acc, ref IntPtr phtok);
        [DllImport("advapi32.dll", SetLastError = true)]
        static extern bool LookupPrivilegeValue(string host, string name, ref long pluid);
        [DllImport("advapi32.dll", ExactSpelling = true, SetLastError = true)]
        static extern bool AdjustTokenPrivileges(IntPtr htok, bool disall, ref TokPriv1Luid newst, int len, IntPtr prev, IntPtr relen);
        public static bool Enable(string privilege) {
            IntPtr htok = IntPtr.Zero;
            if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, ref htok)) return false;
            TokPriv1Luid tp = new TokPriv1Luid { Count = 1, Luid = 0, Attr = SE_PRIVILEGE_ENABLED };
            if (!LookupPrivilegeValue(null, privilege, ref tp.Luid)) return false;
            return AdjustTokenPrivileges(htok, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
        }
    }
}
"@
    }

    [WC.TokenPriv]::Enable('SeTakeOwnershipPrivilege') | Out-Null
    [WC.TokenPriv]::Enable('SeRestorePrivilege')       | Out-Null
    [WC.TokenPriv]::Enable('SeBackupPrivilege')        | Out-Null

    $Script:WC_TOKEN_PRIVILEGES_ENABLED = $true
}

# Writes a DWord value to a registry key, taking ownership if needed and
# restoring the original ACL afterwards. Returns @{ ok; method; reason; error }
# so callers can report honest counts of what actually changed.
function _WC-SetAutologgerStart {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string]$Path,
        [Parameter(Mandatory)] [int]$Value
    )
    if (-not (Test-Path $Path)) { return @{ ok = $false; reason = 'missing-key' } }

    # Fast path: many keys allow admin writes outright; skip the ACL dance.
    try {
        Set-ItemProperty -Path $Path -Name 'Start' -Value $Value -Type DWord -Force -ErrorAction Stop
        return @{ ok = $true; method = 'direct' }
    } catch {
        # Access denied — fall through to ownership takeover.
    }

    _WC-EnsureTokenPrivileges

    $admins  = New-Object System.Security.Principal.SecurityIdentifier 'S-1-5-32-544'
    $origAcl = $null

    try {
        $origAcl = Get-Acl -Path $Path -ErrorAction Stop

        # 1. Take ownership as Administrators.
        $aclTake = Get-Acl -Path $Path
        $aclTake.SetOwner($admins)
        Set-Acl -Path $Path -AclObject $aclTake -ErrorAction Stop

        # 2. Grant Administrators FullControl temporarily.
        $aclGrant = Get-Acl -Path $Path
        $rule = New-Object System.Security.AccessControl.RegistryAccessRule(
            $admins, 'FullControl', 'Allow'
        )
        $aclGrant.AddAccessRule($rule)
        Set-Acl -Path $Path -AclObject $aclGrant -ErrorAction Stop

        # 3. Write the value.
        Set-ItemProperty -Path $Path -Name 'Start' -Value $Value -Type DWord -Force -ErrorAction Stop

        # 4. Restore original ACL — this puts the owner back to TrustedInstaller
        #    and removes our temporary FullControl rule. Needs SeRestorePrivilege.
        try { Set-Acl -Path $Path -AclObject $origAcl -ErrorAction Stop } catch { }

        return @{ ok = $true; method = 'ownership-takeover' }
    } catch {
        # Best-effort restore of the original ACL if we got partway through.
        if ($origAcl) {
            try { Set-Acl -Path $Path -AclObject $origAcl -ErrorAction SilentlyContinue } catch { }
        }
        return @{ ok = $false; reason = 'access-denied'; error = $_.Exception.Message }
    }
}

function Disable-DiagnosticEventTracing {
    Assert-IsAdmin
    try {
        $touched = 0
        $failed  = 0
        $missing = 0
        $errors  = New-Object System.Collections.Generic.List[hashtable]

        foreach ($logger in $Script:WC_DIAG_ETW_LOGGERS) {
            # 1. Stop the currently running ETW session. Without -ets the
            #    autologger Start=0 only takes effect at next boot, so traces
            #    keep accumulating until reboot. The 2>$null swallows the
            #    "session not found" noise for sessions that aren't active.
            & logman stop $logger -ets 2>$null | Out-Null

            # 2. Disable autologger so it doesn't restart at next boot.
            $path = "HKLM:\SYSTEM\CurrentControlSet\Control\WMI\Autologger\$logger"
            $r = _WC-SetAutologgerStart -Path $path -Value 0
            if     ($r.ok)                       { $touched++ }
            elseif ($r.reason -eq 'missing-key') { $missing++ }
            else                                 {
                $failed++
                $errors.Add(@{ logger = $logger; reason = $r.reason; error = $r.error })
            }
        }

        foreach ($t in $Script:WC_DIAG_ETW_TASKS) {
            Disable-ScheduledTask -TaskPath $t.Path -TaskName $t.Name -ErrorAction SilentlyContinue | Out-Null
        }

        @{
            status         = if ($failed -gt 0) { 'partial' } else { 'disabled' }
            loggersTouched = $touched
            loggersFailed  = $failed
            loggersMissing = $missing
            loggersTotal   = $Script:WC_DIAG_ETW_LOGGERS.Count
            errors         = @($errors)
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-DiagnosticEventTracing {
    Assert-IsAdmin
    try {
        $touched = 0
        $failed  = 0
        foreach ($logger in $Script:WC_DIAG_ETW_LOGGERS) {
            $path = "HKLM:\SYSTEM\CurrentControlSet\Control\WMI\Autologger\$logger"
            $r = _WC-SetAutologgerStart -Path $path -Value 1
            if     ($r.ok)                       { $touched++ }
            elseif ($r.reason -ne 'missing-key') { $failed++  }
        }
        foreach ($t in $Script:WC_DIAG_ETW_TASKS) {
            Enable-ScheduledTask -TaskPath $t.Path -TaskName $t.Name -ErrorAction SilentlyContinue | Out-Null
        }
        @{
            status         = if ($failed -gt 0) { 'partial' } else { 'enabled' }
            loggersTouched = $touched
            loggersFailed  = $failed
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# Read-only: returns the same boolean shape the Get-HardeningStatus probe
# computes, but expressed as a standalone function so external callers (and
# debugging) don't need to re-derive the logic.
function Get-DiagnosticEventTracingStatus {
    $offCount = 0
    $present = 0
    foreach ($logger in $Script:WC_DIAG_ETW_LOGGERS) {
        $path = "HKLM:\SYSTEM\CurrentControlSet\Control\WMI\Autologger\$logger"
        if (Test-Path $path) {
            $present++
            $start = (Get-ItemProperty -Path $path -Name 'Start' -ErrorAction SilentlyContinue).Start
            if ($null -eq $start -or $start -eq 0) { $offCount++ }
        } else {
            # Missing key == effectively disabled.
            $offCount++
        }
    }
    @{
        disabled       = ($offCount -eq $Script:WC_DIAG_ETW_LOGGERS.Count)
        loggersOff     = $offCount
        loggersPresent = $present
        loggersTotal   = $Script:WC_DIAG_ETW_LOGGERS.Count
    }
}
