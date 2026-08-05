# ============================================================================
# IDENTITY - BRANDING MODULE
# OEM Information and Backend App Cleanup
# ============================================================================

# Set OEM Information
function Set-OEMInformation {
    param(
        [string]$Manufacturer = "ServaLabs",
        [string]$Model = "SovereignOS",
        [string]$SupportURL = "https://servalabs.com",
        [string]$SupportProvider = "ServaLabs Support",
        [string]$Logo = ""
    )
    
    Assert-IsAdmin
    try {
        $oemPath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OEMInformation'
        if (!(Test-Path $oemPath)) { New-Item -Path $oemPath -Force | Out-Null }
        
        Set-ItemProperty -Path $oemPath -Name 'Manufacturer' -Value $Manufacturer -Force
        Set-ItemProperty -Path $oemPath -Name 'Model' -Value $Model -Force
        Set-ItemProperty -Path $oemPath -Name 'SupportURL' -Value $SupportURL -Force
        Set-ItemProperty -Path $oemPath -Name 'SupportProvider' -Value $SupportProvider -Force
        
        if ($Logo -and (Test-Path $Logo)) {
            Set-ItemProperty -Path $oemPath -Name 'Logo' -Value $Logo -Force
        }
        
        @{ status = 'success'; manufacturer = $Manufacturer; model = $Model; logo = $Logo }
    }
    catch {
        @{ error = $true; message = "Failed to set OEM information: $($_.Exception.Message)" }
    }
}

# Rename the actual Windows device name. OEMInformation.Model is only cosmetic
# and does not change the name used by Windows, networking, or ActivityWatch.
function Rename-ComputerName {
    param(
        [string]$NewName
    )

    Assert-IsAdmin
    try {
        $name = $NewName.Trim()
        if ($name.Length -lt 1 -or $name.Length -gt 15) {
            return @{ status = 'error'; error = 'PC name must be between 1 and 15 characters.' }
        }
        if ($name -notmatch '^[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?$') {
            return @{ status = 'error'; error = 'PC name may contain only letters, numbers, and hyphens, and cannot start or end with a hyphen.' }
        }

        $currentName = [System.Environment]::MachineName
        if ($name.Equals($currentName, [System.StringComparison]::OrdinalIgnoreCase)) {
            return @{ status = 'success'; computerName = $currentName; restartRequired = $false; message = 'This PC already has that name.' }
        }

        Rename-Computer -NewName $name -Force -ErrorAction Stop
        @{ status = 'success'; computerName = $name; restartRequired = $true; message = 'PC name updated. Restart Windows to finish applying the change.' }
    }
    catch {
        @{ status = 'error'; error = "Failed to rename PC: $($_.Exception.Message)" }
    }
}



# Hide Backend Apps (Tailscale, VeraCrypt, UniGetUI)
function Hide-BackendApps {
    # Delegated to centralized dependencies module (per-app hide functions)
    if (Get-Command "Hide-AllBackendApps" -ErrorAction SilentlyContinue) {
        return Hide-AllBackendApps
    }
    return @{ error = $true; message = "Dependencies module not loaded. Cannot hide backend apps." }
}


# --- APP WHITE-LABELING ---

function Set-AppBranding {
    param(
        [string]$CompanyName = "ServaLabs",
        [string]$ProductName = "WinCommander"
    )
    
    try {
        $brandingPath = 'HKCU:\Software\servalabs\WinCommander\Branding'
        if (!(Test-Path $brandingPath)) { New-Item -Path $brandingPath -Force | Out-Null }
        
        Set-ItemProperty -Path $brandingPath -Name 'CompanyName' -Value $CompanyName -Force
        Set-ItemProperty -Path $brandingPath -Name 'ProductName' -Value $ProductName -Force
        
        @{ status = 'success'; companyName = $CompanyName; productName = $ProductName }
    }
    catch {
        @{ error = $true; message = "Failed to set app branding: $($_.Exception.Message)" }
    }
}

function Get-AppBranding {
    try {
        $brandingPath = 'HKCU:\Software\servalabs\WinCommander\Branding'
        $companyName = "ServaLabs"
        $productName = "WinCommander"
        
        if (Test-Path $brandingPath) {
            $props = Get-ItemProperty -Path $brandingPath -ErrorAction SilentlyContinue
            if ($props.CompanyName) { $companyName = $props.CompanyName }
            if ($props.ProductName) { $productName = $props.ProductName }
        }
        
        @{ 
            companyName = $companyName
            productName = $productName
        }
    }
    catch {
        @{ 
            companyName = "ServaLabs"
            productName = "WinCommander"
            error       = $true
            message     = $_.Exception.Message
        }
    }
}
