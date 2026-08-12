# ============================================================================
# SYSTEM - STARTUP MODULE
# Aggregates critical system status for fast application startup
# ============================================================================

function Get-StartupStatus {
    try {
        # System Info is intentionally excluded here.
        # Static hardware fields (CPU model, GPU, RAM, device type) are cached in
        # settings.json → current.device.* from the previous session and seeded into
        # the UI instantly via seedFromCachedSettings. A background Get-SystemInfo
        # probe fires 15s after startup to refresh the cache without blocking load.

        # 1. Hardening Status (Registry checks - Fast)
        $hardening = Get-HardeningStatus

        # 2. Privacy Status - Core
        $telemetry = Get-TelemetryStatus
        $privacyProtection = Get-PrivacyProtectionStatus

        # 3. Privacy Status - Extended (Consolidated here to avoid extra PS spawns)
        # Check if functions exist before calling (safe fallback)
        $clipboardHistory = if (Get-Command Get-ClipboardHistoryStatus -ErrorAction SilentlyContinue) { Get-ClipboardHistoryStatus } else { $null }
        $windowsSuggestions = if (Get-Command Get-WindowsSuggestionsStatus -ErrorAction SilentlyContinue) { Get-WindowsSuggestionsStatus } else { $null }
        $appPrivacyCapabilities = if (Get-Command Get-AppPrivacyCapabilitiesStatus -ErrorAction SilentlyContinue) { Get-AppPrivacyCapabilitiesStatus } else { $null }
        $lockScreenPrivacy = if (Get-Command Get-LockScreenPrivacyStatus -ErrorAction SilentlyContinue) { Get-LockScreenPrivacyStatus } else { $null }
        $setupNags = if (Get-Command Get-SetupCompletionNagsStatus -ErrorAction SilentlyContinue) { Get-SetupCompletionNagsStatus } else { $null }

        # 4. Productivity Status (if module is loaded)
        $awStatus = if (Get-Command Get-ProductivityStatus -ErrorAction SilentlyContinue) { Get-ProductivityStatus } else { $null }

        return @{
            systemInfo             = $null
            hardeningStatus        = $hardening
            telemetryStatus        = $telemetry
            privacyProtection      = $privacyProtection
            productivity           = $awStatus

            # Extended Privacy Data
            clipboardHistory       = $clipboardHistory
            windowsSuggestions     = $windowsSuggestions
            appPrivacyCapabilities = $appPrivacyCapabilities
            lockScreenPrivacy      = $lockScreenPrivacy
            setupNags              = $setupNags
        }
    }
    catch {
        return @{ 
            error      = $true 
            message    = $_.Exception.Message 
            stackTrace = $_.ScriptStackTrace
        }
    }
}



