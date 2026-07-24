# ============================================================================
# TWEAKS - POWER
# Power-management tweaks.
# ============================================================================

# ── USB SELECTIVE SUSPEND ───────────────────────────────────────────────
# WMI MSPower_DeviceEnable controls per-device "Allow the computer to turn
# off this device to save power". We disable it for every USB root hub
# (and apply the user-level powercfg toggle for completeness).

function Disable-UsbSelectiveSuspend {
    Assert-IsAdmin
    try {
        # Global selective-suspend setting in the active power scheme
        # USB SETTINGS subgroup GUID: 2a737441-1930-4402-8d77-b2bebba308a3
        # USB SELECTIVE SUSPEND      48e6b7a6-50f5-4782-a5d4-53bb8f07e226
        $null = & powercfg /SETACVALUEINDEX SCHEME_CURRENT 2a737441-1930-4402-8d77-b2bebba308a3 48e6b7a6-50f5-4782-a5d4-53bb8f07e226 0 2>$null
        $null = & powercfg /SETDCVALUEINDEX SCHEME_CURRENT 2a737441-1930-4402-8d77-b2bebba308a3 48e6b7a6-50f5-4782-a5d4-53bb8f07e226 0 2>$null
        $null = & powercfg /SETACTIVE SCHEME_CURRENT 2>$null

        # Disable per-device "allow turning off to save power" on USB root hubs
        $devices = Get-CimInstance -Namespace root\WMI -ClassName MSPower_DeviceEnable -ErrorAction SilentlyContinue
        $count = 0
        foreach ($d in $devices) {
            try {
                if ($d.InstanceName -match "USB") {
                    $d.Enable = $false
                    $null = $d | Set-CimInstance -ErrorAction SilentlyContinue
                    $count++
                }
            } catch {}
        }
        @{ status = "disabled"; devices = $count }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-UsbSelectiveSuspend {
    Assert-IsAdmin
    try {
        $null = & powercfg /SETACVALUEINDEX SCHEME_CURRENT 2a737441-1930-4402-8d77-b2bebba308a3 48e6b7a6-50f5-4782-a5d4-53bb8f07e226 1 2>$null
        $null = & powercfg /SETDCVALUEINDEX SCHEME_CURRENT 2a737441-1930-4402-8d77-b2bebba308a3 48e6b7a6-50f5-4782-a5d4-53bb8f07e226 1 2>$null
        $null = & powercfg /SETACTIVE SCHEME_CURRENT 2>$null

        $devices = Get-CimInstance -Namespace root\WMI -ClassName MSPower_DeviceEnable -ErrorAction SilentlyContinue
        foreach ($d in $devices) {
            try {
                if ($d.InstanceName -match "USB") {
                    $d.Enable = $true
                    $null = $d | Set-CimInstance -ErrorAction SilentlyContinue
                }
            } catch {}
        }
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── CPU POWER THROTTLING ────────────────────────────────────────────────

function Disable-CpuThrottling {
    Assert-IsAdmin
    try {
        $p = "HKLM:\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling"
        if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
        Set-ItemProperty -Path $p -Name "PowerThrottlingOff" -Value 1 -Type DWord -Force
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-CpuThrottling {
    Assert-IsAdmin
    try {
        $p = "HKLM:\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling"
        if (Test-Path $p) {
            Remove-ItemProperty -Path $p -Name "PowerThrottlingOff" -ErrorAction SilentlyContinue
        }
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# Ultimate Performance is now exposed via the unified Set-PowerPlan
# command in maintenance.ps1 (Mode='ultimate') — see PowerPlanCard.tsx.
