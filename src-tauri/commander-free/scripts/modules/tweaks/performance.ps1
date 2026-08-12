# ============================================================================
# TWEAKS - PERFORMANCE
# Gaming and responsiveness tweaks.
# All HKLM writes -> Assert-IsAdmin (executed by Tauri command via dispatch).
# Idempotent: each enable writes the desired values, each disable removes
# the override or restores the OS default.
# ============================================================================

# ── MMCSS GAMING PROFILE ────────────────────────────────────────────────
# Multimedia Class Scheduler Service tuning. Optimises Windows for low-latency
# foreground apps (gaming, audio). Disables network throttling that otherwise
# steals CPU from the foreground process every 10 ms for network-stack tasks.

function Enable-MMCSSGamingProfile {
    Assert-IsAdmin
    try {
        $sp = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile"
        if (!(Test-Path $sp)) { New-Item -Path $sp -Force | Out-Null }
        Set-ItemProperty -Path $sp -Name "SystemResponsiveness" -Value 10 -Type DWord -Force
        Set-ItemProperty -Path $sp -Name "NetworkThrottlingIndex" -Value 0xFFFFFFFF -Type DWord -Force
        Set-ItemProperty -Path $sp -Name "NoLazyMode" -Value 1 -Type DWord -Force

        $games = "$sp\Tasks\Games"
        if (!(Test-Path $games)) { New-Item -Path $games -Force | Out-Null }
        Set-ItemProperty -Path $games -Name "Affinity" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $games -Name "Background Only" -Value "False" -Type String -Force
        Set-ItemProperty -Path $games -Name "Clock Rate" -Value 10000 -Type DWord -Force
        Set-ItemProperty -Path $games -Name "GPU Priority" -Value 8 -Type DWord -Force
        Set-ItemProperty -Path $games -Name "Priority" -Value 6 -Type DWord -Force
        Set-ItemProperty -Path $games -Name "Scheduling Category" -Value "High" -Type String -Force
        Set-ItemProperty -Path $games -Name "SFIO Priority" -Value "High" -Type String -Force
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-MMCSSGamingProfile {
    Assert-IsAdmin
    try {
        $sp = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile"
        # Restore Windows defaults
        Set-ItemProperty -Path $sp -Name "SystemResponsiveness" -Value 20 -Type DWord -Force
        Set-ItemProperty -Path $sp -Name "NetworkThrottlingIndex" -Value 10 -Type DWord -Force
        Remove-ItemProperty -Path $sp -Name "NoLazyMode" -ErrorAction SilentlyContinue

        $games = "$sp\Tasks\Games"
        if (Test-Path $games) {
            Set-ItemProperty -Path $games -Name "GPU Priority" -Value 8 -Type DWord -Force
            Set-ItemProperty -Path $games -Name "Priority" -Value 2 -Type DWord -Force
            Set-ItemProperty -Path $games -Name "Scheduling Category" -Value "Medium" -Type String -Force
            Set-ItemProperty -Path $games -Name "SFIO Priority" -Value "Normal" -Type String -Force
        }
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── KEYBOARD LATENCY ────────────────────────────────────────────────────
# HKCU keyboard repeat delay/rate. Per-user, no admin required.

function Enable-KeyboardLatencyOptimised {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "KeyboardDelay" -Value "0" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "KeyboardSpeed" -Value "31" -Type String -Force
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-KeyboardLatencyOptimised {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "KeyboardDelay" -Value "1" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "KeyboardSpeed" -Value "31" -Type String -Force
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── NUM LOCK ON BOOT ────────────────────────────────────────────────────
# Both HKU\.DEFAULT (logon screen) and HKCU (interactive session).

function Enable-NumLockOnBoot {
    Assert-IsAdmin
    try {
        # HKU:\.DEFAULT may not be present unless loaded; use REG ADD via reg.exe which loads it on demand.
        $null = & reg.exe ADD "HKU\.DEFAULT\Control Panel\Keyboard" /v InitialKeyboardIndicators /t REG_SZ /d "2" /f 2>$null
        Set-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "InitialKeyboardIndicators" -Value "2147483650" -Type String -Force
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-NumLockOnBoot {
    Assert-IsAdmin
    try {
        $null = & reg.exe ADD "HKU\.DEFAULT\Control Panel\Keyboard" /v InitialKeyboardIndicators /t REG_SZ /d "0" /f 2>$null
        Set-ItemProperty -Path "HKCU:\Control Panel\Keyboard" -Name "InitialKeyboardIndicators" -Value "2147483648" -Type String -Force
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── HARDWARE-ACCELERATED GPU SCHEDULING ─────────────────────────────────
# HwSchMode=2 enables, =1 disables. Requires reboot.

function Enable-GpuScheduling {
    Assert-IsAdmin
    try {
        $p = "HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers"
        if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
        Set-ItemProperty -Path $p -Name "HwSchMode" -Value 2 -Type DWord -Force
        @{ status = "enabled"; requiresRestart = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-GpuScheduling {
    Assert-IsAdmin
    try {
        Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers" -Name "HwSchMode" -Value 1 -Type DWord -Force
        @{ status = "disabled"; requiresRestart = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}
