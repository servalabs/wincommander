# ============================================================================
# TWEAKS - GPU
# Vendor-specific GPU optimisations.
# Uses WMI Win32_VideoController to find GPU device-instance keys, then
# writes per-vendor registry values under
#   HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\NNNN
# All changes require a reboot to take effect.
# ============================================================================

# ── INTERNAL: enumerate GPU device-instance keys for a given vendor ─────
# Returns an array of HKLM paths.

function _Get-GpuDeviceKeys {
    param([string]$Vendor)  # "AMD" | "NVIDIA" | "Intel"

    $classRoot = "HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}"
    if (!(Test-Path $classRoot)) { return @() }

    $result = @()
    Get-ChildItem -Path $classRoot -ErrorAction SilentlyContinue | ForEach-Object {
        $key = $_
        $name = $key.PSChildName
        # Skip non-numeric subkeys (Configuration, Properties, etc.)
        if ($name -notmatch '^\d{4}$') { return }
        try {
            $providerName = (Get-ItemProperty -Path $key.PSPath -Name "ProviderName" -ErrorAction SilentlyContinue).ProviderName
            $driverDesc   = (Get-ItemProperty -Path $key.PSPath -Name "DriverDesc"   -ErrorAction SilentlyContinue).DriverDesc
            $merged = "$providerName $driverDesc"
            switch ($Vendor) {
                "AMD"    { if ($merged -match "(?i)AMD|ATI|Advanced Micro Devices|Radeon")  { $result += $key.PSPath } }
                "NVIDIA" { if ($merged -match "(?i)NVIDIA|GeForce|Quadro|Tesla")           { $result += $key.PSPath } }
                "Intel"  { if ($merged -match "(?i)Intel|HD Graphics|UHD|Iris|Arc")        { $result += $key.PSPath } }
            }
        } catch {}
    }
    return $result
}

function Get-GpuVendors {
    # Used by the UI to know which GPU vendor sections to enable.
    Assert-IsAdmin
    $vendors = @{
        amd    = ( _Get-GpuDeviceKeys -Vendor "AMD"    ).Count -gt 0
        nvidia = ( _Get-GpuDeviceKeys -Vendor "NVIDIA" ).Count -gt 0
        intel  = ( _Get-GpuDeviceKeys -Vendor "Intel"  ).Count -gt 0
    }
    @{ vendors = $vendors }
}

# ── HELPER: write a registry DWord across every device key for a vendor ─

function _Set-GpuValueForVendor {
    param(
        [string]$Vendor,
        [hashtable]$Values  # name -> int (DWord)
    )
    $keys = _Get-GpuDeviceKeys -Vendor $Vendor
    if (!$keys -or $keys.Count -eq 0) {
        return @{ status = "no-gpu"; vendor = $Vendor }
    }
    foreach ($k in $keys) {
        foreach ($name in $Values.Keys) {
            Set-ItemProperty -Path $k -Name $name -Value $Values[$name] -Type DWord -Force
        }
    }
    @{ status = "applied"; vendor = $Vendor; devices = $keys.Count; requiresRestart = $true }
}

function _Remove-GpuValueForVendor {
    param(
        [string]$Vendor,
        [string[]]$Names
    )
    $keys = _Get-GpuDeviceKeys -Vendor $Vendor
    if (!$keys -or $keys.Count -eq 0) {
        return @{ status = "no-gpu"; vendor = $Vendor }
    }
    foreach ($k in $keys) {
        foreach ($n in $Names) {
            Remove-ItemProperty -Path $k -Name $n -ErrorAction SilentlyContinue
        }
    }
    @{ status = "reverted"; vendor = $Vendor; requiresRestart = $true }
}

# ── AMD ─────────────────────────────────────────────────────────────────

function Disable-AmdUlps {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "AMD" -Values @{ EnableUlps = 0; EnableUlps_NA = 0 }
}
function Enable-AmdUlps {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "AMD" -Values @{ EnableUlps = 1 }
}

function Disable-AmdPowerGating {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "AMD" -Values @{
        DisablePowerGating         = 1
        PP_GPUPowerDownEnabled     = 0
        DisableDynamicPstate       = 1
    }
}
function Enable-AmdPowerGating {
    Assert-IsAdmin
    _Remove-GpuValueForVendor -Vendor "AMD" -Names @("DisablePowerGating","PP_GPUPowerDownEnabled","DisableDynamicPstate")
}

function Disable-AmdVideoClockGating {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "AMD" -Values @{
        DisableVCEPowerGating  = 1
        DisableVceClockGating  = 1
        EnableUvdClockGating   = 0
        EnableVceSwClockGating = 0
    }
}
function Enable-AmdVideoClockGating {
    Assert-IsAdmin
    _Remove-GpuValueForVendor -Vendor "AMD" -Names @("DisableVCEPowerGating","DisableVceClockGating","EnableUvdClockGating","EnableVceSwClockGating")
}

function Disable-AmdAspm {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "AMD" -Values @{ EnableAspmL0s = 0; EnableAspmL1 = 0 }
}
function Enable-AmdAspm {
    Assert-IsAdmin
    _Remove-GpuValueForVendor -Vendor "AMD" -Names @("EnableAspmL0s","EnableAspmL1")
}

# ── NVIDIA ──────────────────────────────────────────────────────────────

function Disable-NvidiaDynamicPstate {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "NVIDIA" -Values @{ DisableDynamicPstate = 1 }
}
function Enable-NvidiaDynamicPstate {
    Assert-IsAdmin
    _Remove-GpuValueForVendor -Vendor "NVIDIA" -Names @("DisableDynamicPstate")
}

function Disable-NvidiaAsyncPstates {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "NVIDIA" -Values @{ DisableASyncPstates = 1 }
}
function Enable-NvidiaAsyncPstates {
    Assert-IsAdmin
    _Remove-GpuValueForVendor -Vendor "NVIDIA" -Names @("DisableASyncPstates")
}

# ── INTEL ───────────────────────────────────────────────────────────────

function Disable-IntelAsyncFlips {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "Intel" -Values @{ Display1_DisableAsyncFlips = 1 }
}
function Enable-IntelAsyncFlips {
    Assert-IsAdmin
    _Remove-GpuValueForVendor -Vendor "Intel" -Names @("Display1_DisableAsyncFlips")
}

function Disable-IntelAdaptiveVsync {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "Intel" -Values @{ AdaptiveVsyncEnable = 0 }
}
function Enable-IntelAdaptiveVsync {
    Assert-IsAdmin
    _Set-GpuValueForVendor -Vendor "Intel" -Values @{ AdaptiveVsyncEnable = 1 }
}
