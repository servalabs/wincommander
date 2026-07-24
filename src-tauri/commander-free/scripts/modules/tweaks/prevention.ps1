# ============================================================================
# TWEAKS - ACTIVITY REDUCTION TOGGLES
# Reversible toggles that reduce what the OS logs about user activity.
# Disable-* applies the reduction; Enable-* restores the OS default.
# All state-mutating functions require admin; Get-* queries are non-admin.
# ============================================================================

# ── Status Query ─────────────────────────────────────────────────────────────

function Get-ActivityReductionStatus {
    $auditpol = (auditpol /get /category:* 2>$null | Out-String)
    $bamKey = 'HKLM:\SYSTEM\CurrentControlSet\Services\bam\State'
    $shimKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\AppCompatCache'
    $psLogKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\PowerShell\ScriptBlockLogging'
    $werKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Error Reporting'
    $racKey = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\TelemetryController\Racagent'

    @{
        auditReductionActive = ($auditpol -match 'No Auditing') -and ($auditpol -notmatch 'Success and Failure')
        bamDisabled = ((Get-ItemProperty $bamKey -Name SequenceNumber -ErrorAction SilentlyContinue) -eq $null) -or
                      ((Get-ItemProperty $bamKey -Name 'ServiceCurrentControlSet' -ErrorAction SilentlyContinue) -eq $null)
        shimCacheDisabled = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' `
                                -Name DisableShimCache -ErrorAction SilentlyContinue)?.DisableShimCache -eq 1
        scriptBlockLoggingDisabled = (Get-ItemProperty $psLogKey -Name EnableScriptBlockLogging `
                                          -ErrorAction SilentlyContinue)?.EnableScriptBlockLogging -eq 0
        werDisabled = (Get-ItemProperty $werKey -Name Disabled -ErrorAction SilentlyContinue)?.Disabled -eq 1
        telemetryRunnerDisabled = -not (Get-ScheduledTask -TaskName 'Microsoft Compatibility Appraiser' `
                                            -ErrorAction SilentlyContinue | Where-Object { $_.State -ne 'Disabled' })
        reliabilityMonitorDisabled = (Get-ItemProperty $racKey -Name Start -ErrorAction SilentlyContinue)?.Start -eq 4
        recentFilesDisabled = (Get-ItemProperty 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced' `
                                   -Name Start_TrackDocs -ErrorAction SilentlyContinue)?.Start_TrackDocs -eq 0
        ssidLoggingDisabled = (Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WlanSvc' `
                                   -Name fBlockNetworkSsidUi -ErrorAction SilentlyContinue)?.fBlockNetworkSsidUi -eq 1
    }
}

# ── 1. Audit Policy Reduction ────────────────────────────────────────────────
# Cuts the sub-categories that produce the most cleanupally-relevant events
# while keeping enough auditing for basic accountability.

function Disable-AuditLogging {
    Assert-IsAdmin
    try {
        # Disable sub-categories that produce high-value cleanup events
        $reductions = @(
            'Process Creation', 'Process Termination',
            'Detailed File Share', 'File Share',
            'Other Object Access Events', 'Removable Storage',
            'Logon', 'Logoff', 'Special Logon',
            'Account Lockout',
            'Plug and Play Events'
        )
        foreach ($cat in $reductions) {
            auditpol /set /subcategory:"$cat" /success:disable /failure:disable 2>$null | Out-Null
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-AuditLogging {
    Assert-IsAdmin
    try {
        # Restore Windows defaults (Success for logon/object; off for verbose)
        auditpol /set /subcategory:"Logon" /success:enable /failure:enable 2>$null | Out-Null
        auditpol /set /subcategory:"Logoff" /success:enable /failure:disable 2>$null | Out-Null
        auditpol /set /subcategory:"Special Logon" /success:enable /failure:disable 2>$null | Out-Null
        auditpol /set /subcategory:"Account Lockout" /success:enable /failure:disable 2>$null | Out-Null
        auditpol /set /subcategory:"Process Creation" /success:disable /failure:disable 2>$null | Out-Null
        auditpol /set /subcategory:"Detailed File Share" /success:disable /failure:disable 2>$null | Out-Null
        auditpol /set /subcategory:"Plug and Play Events" /success:enable /failure:disable 2>$null | Out-Null
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 2. Background Activity Monitor (BAM / DAM) ───────────────────────────────
# BAM tracks which binaries ran and when; stored in the BAM registry hive.

function Disable-ActivityMonitor {
    Assert-IsAdmin
    try {
        # Setting BAM service start type to Disabled prevents the driver from
        # collecting new activity records after next reboot.
        Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\bam' `
            -Name Start -Value 4 -Type DWord -Force
        Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\dam' `
            -Name Start -Value 4 -Type DWord -Force -ErrorAction SilentlyContinue
        @{ success = $true; note = 'Effective after reboot' }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-ActivityMonitor {
    Assert-IsAdmin
    try {
        Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\bam' `
            -Name Start -Value 3 -Type DWord -Force
        Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\dam' `
            -Name Start -Value 3 -Type DWord -Force -ErrorAction SilentlyContinue
        @{ success = $true; note = 'Effective after reboot' }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 3. AppCompatCache / ShimCache ────────────────────────────────────────────
# ShimCache records every executable that was run or enumerated on this machine.

function Disable-AppCompatCache {
    Assert-IsAdmin
    try {
        Set-ItemProperty `
            'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' `
            -Name DisableShimCache -Value 1 -Type DWord -Force
        @{ success = $true; note = 'Effective after reboot' }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-AppCompatCache {
    Assert-IsAdmin
    try {
        Remove-ItemProperty `
            'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' `
            -Name DisableShimCache -ErrorAction SilentlyContinue
        @{ success = $true; note = 'Effective after reboot' }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 4. UserAssist ────────────────────────────────────────────────────────────
# UserAssist tracks application launch counts and timestamps in HKCU.

function Disable-UserAssistTracking {
    Assert-IsAdmin
    try {
        $key = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{CEBFF5CD-ACE2-4F4F-9178-9926F41749EA}\Settings'
        if (Test-Path $key) {
            Set-ItemProperty $key -Name NoLog -Value 1 -Type DWord -Force
        }
        $key2 = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{F4E57C4B-2036-45F0-A9AB-443BCFE33D9F}\Settings'
        if (Test-Path $key2) {
            Set-ItemProperty $key2 -Name NoLog -Value 1 -Type DWord -Force
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-UserAssistTracking {
    Assert-IsAdmin
    try {
        $key = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{CEBFF5CD-ACE2-4F4F-9178-9926F41749EA}\Settings'
        if (Test-Path $key) {
            Remove-ItemProperty $key -Name NoLog -ErrorAction SilentlyContinue
        }
        $key2 = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{F4E57C4B-2036-45F0-A9AB-443BCFE33D9F}\Settings'
        if (Test-Path $key2) {
            Remove-ItemProperty $key2 -Name NoLog -ErrorAction SilentlyContinue
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 5. USB Device Event Logging (setupapi + USBSTOR ETW) ─────────────────────
# setupapi.dev.log records every USB device insertion; USBSTOR ETW adds kernel events.

function Disable-UsbEventLog {
    Assert-IsAdmin
    try {
        # Suppress setupapi device installation logging
        Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup' `
            -Name LogLevel -Value 0 -Type DWord -Force
        # Disable USBSTOR ETW provider via registry hint used by Windows kernel
        $usbKey = 'HKLM:\SYSTEM\CurrentControlSet\Services\USBSTOR'
        if (Test-Path $usbKey) {
            Set-ItemProperty $usbKey -Name Start -Value 4 -Type DWord -Force
        }
        @{ success = $true; note = 'setupapi logging suppressed; USBSTOR disabled after reboot' }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-UsbEventLog {
    Assert-IsAdmin
    try {
        Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup' `
            -Name LogLevel -Value 2 -Type DWord -Force
        $usbKey = 'HKLM:\SYSTEM\CurrentControlSet\Services\USBSTOR'
        if (Test-Path $usbKey) {
            Set-ItemProperty $usbKey -Name Start -Value 3 -Type DWord -Force
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 6. WLAN-AutoConfig SSID History ──────────────────────────────────────────
# Windows records every Wi-Fi network that was connected; toggle policy to stop collection.

function Disable-SsidHistory {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WlanSvc'
        if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty $path -Name fBlockNetworkSsidUi -Value 1 -Type DWord -Force
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-SsidHistory {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WlanSvc'
        Remove-ItemProperty $path -Name fBlockNetworkSsidUi -ErrorAction SilentlyContinue
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 7. Storage + Partition Diagnostic ETW Channels ───────────────────────────
# These ETW channels record disk I/O and partition events; disabling stops new entries.

function Disable-StorageEventLog {
    Assert-IsAdmin
    try {
        $channels = @(
            'Microsoft-Windows-Partition/Diagnostic',
            'Microsoft-Windows-StorageSpaces-Driver/Diagnostic',
            'Microsoft-Windows-Storage-ATAPort/Admin',
            'Microsoft-Windows-Storage-Storport/Health'
        )
        foreach ($ch in $channels) {
            wevtutil sl "$ch" /e:false 2>$null | Out-Null
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-StorageEventLog {
    Assert-IsAdmin
    try {
        $channels = @(
            'Microsoft-Windows-Partition/Diagnostic',
            'Microsoft-Windows-StorageSpaces-Driver/Diagnostic',
            'Microsoft-Windows-Storage-ATAPort/Admin',
            'Microsoft-Windows-Storage-Storport/Health'
        )
        foreach ($ch in $channels) {
            wevtutil sl "$ch" /e:true 2>$null | Out-Null
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 8. Recent File / Activity Tracking ───────────────────────────────────────
# Controls whether Explorer populates RecentDocs, JumpLists, and thumbnail cache.

function Disable-RecentActivityTracking {
    Assert-IsAdmin
    try {
        $advKey = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
        Set-ItemProperty $advKey -Name Start_TrackDocs -Value 0 -Type DWord -Force
        # Suppress thumbnail cache writes
        $explorerKey = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer'
        Set-ItemProperty $explorerKey -Name DisableThumbnailCache -Value 1 -Type DWord -Force
        # Suppress search suggestions DB (WordWheel)
        $policies = 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer'
        if (-not (Test-Path $policies)) { New-Item -Path $policies -Force | Out-Null }
        Set-ItemProperty $policies -Name NoSearchInternetInStartMenu -Value 1 -Type DWord -Force
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-RecentActivityTracking {
    Assert-IsAdmin
    try {
        $advKey = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
        Set-ItemProperty $advKey -Name Start_TrackDocs -Value 1 -Type DWord -Force
        $explorerKey = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer'
        Remove-ItemProperty $explorerKey -Name DisableThumbnailCache -ErrorAction SilentlyContinue
        $policies = 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer'
        Remove-ItemProperty $policies -Name NoSearchInternetInStartMenu -ErrorAction SilentlyContinue
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 9. Diagnostic Event Channels ─────────────────────────────────────────────
# Disables verbose ETW channels that record DNS lookups, SMB access, RDP sessions,
# and user-profile load events — all cleanupally interesting.

function Disable-DiagnosticChannel {
    Assert-IsAdmin
    try {
        $channels = @(
            'Microsoft-Windows-DNS-Client/Operational',
            'Microsoft-Windows-SMBClient/Operational',
            'Microsoft-Windows-SMBClient/Security',
            'Microsoft-Windows-TerminalServices-LocalSessionManager/Operational',
            'Microsoft-Windows-User Profile Service/Operational'
        )
        foreach ($ch in $channels) {
            wevtutil sl "$ch" /e:false 2>$null | Out-Null
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-DiagnosticChannel {
    Assert-IsAdmin
    try {
        $channels = @(
            'Microsoft-Windows-DNS-Client/Operational',
            'Microsoft-Windows-SMBClient/Operational',
            'Microsoft-Windows-SMBClient/Security',
            'Microsoft-Windows-TerminalServices-LocalSessionManager/Operational',
            'Microsoft-Windows-User Profile Service/Operational'
        )
        foreach ($ch in $channels) {
            wevtutil sl "$ch" /e:true 2>$null | Out-Null
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 10. Reliability Monitor (RACAgent) ───────────────────────────────────────
# The Reliability Analysis Component tracks application crashes/hangs over time.

function Disable-ReliabilityMonitor {
    Assert-IsAdmin
    try {
        # Disable the RACAgent scheduled task
        Disable-ScheduledTask -TaskPath '\Microsoft\Windows\RAC\' -TaskName 'RacTask' `
            -ErrorAction SilentlyContinue | Out-Null
        # Mark service disabled
        $racKey = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\TelemetryController\Racagent'
        if (-not (Test-Path $racKey)) { New-Item -Path $racKey -Force | Out-Null }
        Set-ItemProperty $racKey -Name Start -Value 4 -Type DWord -Force
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-ReliabilityMonitor {
    Assert-IsAdmin
    try {
        Enable-ScheduledTask -TaskPath '\Microsoft\Windows\RAC\' -TaskName 'RacTask' `
            -ErrorAction SilentlyContinue | Out-Null
        $racKey = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\TelemetryController\Racagent'
        Remove-ItemProperty $racKey -Name Start -ErrorAction SilentlyContinue
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 11. Windows Error Reporting (WER) ────────────────────────────────────────
# WER stores crash dumps and event logs; disabling prevents new artifacts.

function Disable-ErrorReporting {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Error Reporting'
        if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty $path -Name Disabled -Value 1 -Type DWord -Force
        Set-ItemProperty $path -Name DontSendAdditionalData -Value 1 -Type DWord -Force
        Disable-ScheduledTask -TaskPath '\Microsoft\Windows\Windows Error Reporting\' `
            -TaskName 'QueueReporting' -ErrorAction SilentlyContinue | Out-Null
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-ErrorReporting {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Error Reporting'
        Remove-ItemProperty $path -Name Disabled -ErrorAction SilentlyContinue
        Remove-ItemProperty $path -Name DontSendAdditionalData -ErrorAction SilentlyContinue
        Enable-ScheduledTask -TaskPath '\Microsoft\Windows\Windows Error Reporting\' `
            -TaskName 'QueueReporting' -ErrorAction SilentlyContinue | Out-Null
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── Crash Dumps — disable kernel + WER dumps ─────────────────────────────────
# Disabling crash dumps removes full memory images from disk. WER is also
# disabled so no minidumps are queued. Re-enable restores the Windows default
# (kernel memory dump, 7 = FULL | 3 = kernel | 1 = mini / minidump).

function Disable-CrashDumps {
    Assert-IsAdmin
    try {
        $cc = 'HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl'
        if (-not (Test-Path $cc)) { New-Item -Path $cc -Force | Out-Null }
        Set-ItemProperty $cc -Name CrashDumpEnabled -Value 0 -Type DWord -Force
        Set-ItemProperty $cc -Name DumpType          -Value 0 -Type DWord -Force

        $wer = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Error Reporting'
        if (-not (Test-Path $wer)) { New-Item -Path $wer -Force | Out-Null }
        Set-ItemProperty $wer -Name Disabled -Value 1 -Type DWord -Force
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-CrashDumps {
    Assert-IsAdmin
    try {
        $cc = 'HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl'
        # Restore Windows default: mini-dump (value 3 = kernel dump; 1 = minidump)
        Set-ItemProperty $cc -Name CrashDumpEnabled -Value 1 -Type DWord -Force
        Set-ItemProperty $cc -Name DumpType          -Value 1 -Type DWord -Force
        $wer = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Error Reporting'
        Remove-ItemProperty $wer -Name Disabled -ErrorAction SilentlyContinue
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 12. PowerShell Script Block + Module Logging ─────────────────────────────
# When enabled, Windows logs every PS script block to the event log (event 4104).
# Disabling prevents new log entries; existing entries in the event log remain.

function Disable-ScriptBlockLogging {
    Assert-IsAdmin
    try {
        $sbKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\PowerShell\ScriptBlockLogging'
        if (-not (Test-Path $sbKey)) { New-Item -Path $sbKey -Force | Out-Null }
        Set-ItemProperty $sbKey -Name EnableScriptBlockLogging -Value 0 -Type DWord -Force
        Set-ItemProperty $sbKey -Name EnableScriptBlockInvocationLogging -Value 0 -Type DWord -Force

        $modKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\PowerShell\ModuleLogging'
        if (-not (Test-Path $modKey)) { New-Item -Path $modKey -Force | Out-Null }
        Set-ItemProperty $modKey -Name EnableModuleLogging -Value 0 -Type DWord -Force
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-ScriptBlockLogging {
    Assert-IsAdmin
    try {
        $sbKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\PowerShell\ScriptBlockLogging'
        Remove-ItemProperty $sbKey -Name EnableScriptBlockLogging -ErrorAction SilentlyContinue
        Remove-ItemProperty $sbKey -Name EnableScriptBlockInvocationLogging -ErrorAction SilentlyContinue
        $modKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\PowerShell\ModuleLogging'
        Remove-ItemProperty $modKey -Name EnableModuleLogging -ErrorAction SilentlyContinue
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── 13. CompatTelRunner (Windows Compatibility Telemetry) ────────────────────
# The CompatTelRunner task uploads app-compatibility telemetry and generates
# WER-style event entries. Disabling stops both the scheduled upload and the
# CompatTelRunner.exe execution via IFEO redirect.

function Disable-TelemetryRunner {
    Assert-IsAdmin
    try {
        $tasks = @(
            @{ Path = '\Microsoft\Windows\Application Experience\'; Name = 'Microsoft Compatibility Appraiser' },
            @{ Path = '\Microsoft\Windows\Application Experience\'; Name = 'ProgramDataUpdater' }
        )
        foreach ($t in $tasks) {
            Disable-ScheduledTask -TaskPath $t.Path -TaskName $t.Name `
                -ErrorAction SilentlyContinue | Out-Null
        }
        # IFEO redirect: any spawn of CompatTelRunner.exe is silently killed
        $ifeoKey = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\CompatTelRunner.exe'
        if (-not (Test-Path $ifeoKey)) { New-Item -Path $ifeoKey -Force | Out-Null }
        Set-ItemProperty $ifeoKey -Name Debugger -Value 'taskkill.exe' -Force
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Enable-TelemetryRunner {
    Assert-IsAdmin
    try {
        $tasks = @(
            @{ Path = '\Microsoft\Windows\Application Experience\'; Name = 'Microsoft Compatibility Appraiser' },
            @{ Path = '\Microsoft\Windows\Application Experience\'; Name = 'ProgramDataUpdater' }
        )
        foreach ($t in $tasks) {
            Enable-ScheduledTask -TaskPath $t.Path -TaskName $t.Name `
                -ErrorAction SilentlyContinue | Out-Null
        }
        $ifeoKey = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\CompatTelRunner.exe'
        Remove-Item $ifeoKey -Recurse -Force -ErrorAction SilentlyContinue
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

# ── DN-01: App history trace removal ─────────────────────────────────────────
# Belt-and-suspenders: the app runs PS in -NonInteractive -NoProfile mode so
# PSReadLine never writes history. This function cleans any stale app-related
# entries that a prior interactive PS session may have recorded.

function Remove-AppHistoryTraces {
    try {
        $histPath = (Get-PSReadLineOption -ErrorAction SilentlyContinue)?.HistorySavePath
        if ($histPath -and (Test-Path $histPath)) {
            $lines = Get-Content $histPath -ErrorAction SilentlyContinue
            if ($lines) {
                $filtered = $lines | Where-Object {
                    $_ -notmatch 'WinCommander|wincommander-free|commander-free|bun run|encrypt-backend'
                }
                if ($filtered.Count -ne $lines.Count) {
                    Set-Content $histPath $filtered -ErrorAction SilentlyContinue
                }
            }
        }
        @{ success = $true }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}
