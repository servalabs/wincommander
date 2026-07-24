# ============================================================================
# SYSTEM - ACTIVATION MODULE
# Manages Windows and Office activation status and tools
# ============================================================================

# Helper: Get Microsoft Office activation status
function Get-OfficeStatus {
    try {
        $products = @()
        $installed = $false

        $clickToRun = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration' -ErrorAction SilentlyContinue
        if ($clickToRun -and $clickToRun.ProductReleaseIds) {
            $installed = $true
            $products += $clickToRun.ProductReleaseIds -split ','
        }

        $installRoots = @(
            'HKLM:\SOFTWARE\Microsoft\Office\16.0\Common\InstallRoot',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Office\16.0\Common\InstallRoot',
            'HKLM:\SOFTWARE\Microsoft\Office\15.0\Common\InstallRoot',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Office\15.0\Common\InstallRoot'
        )
        foreach ($root in $installRoots) {
            if (Test-Path $root) {
                $installed = $true
                break
            }
        }

        if (-not $installed) {
            $officePaths = @(
                "$env:ProgramFiles\Microsoft Office\root\Office16",
                "$env:ProgramFiles(x86)\Microsoft Office\root\Office16"
            )
            foreach ($path in $officePaths) {
                if (Test-Path $path) { $installed = $true; break }
            }
        }

        $activated = $false
        if ($installed) {
            try {
                $officeLicenses = Get-CimInstance -ClassName SoftwareLicensingProduct -Filter "Name like '%Office%'" -OperationTimeoutSec 5 -ErrorAction Stop
                $activeLicenses = $officeLicenses | Where-Object { $_.LicenseStatus -eq 1 }
                $activated = ($null -ne $activeLicenses)
                
                $licensedProducts = $activeLicenses | Select-Object -ExpandProperty Name -Unique -ErrorAction SilentlyContinue
                if ($licensedProducts) { $products += $licensedProducts }
            }
            catch {
                $activated = $false
            }
        }

        @{
            installed = $installed
            activated = $activated
            products  = ($products | Where-Object { $_ } | Select-Object -Unique)
        }
    }
    catch {
        @{ installed = $false; activated = $false; products = @() }
    }
}

# Get Windows and Office activation status
function Get-ActivationStatus {
    try {
        $windowsActivated = $false
        $edition = "Unknown"
        
        try {
            # Try CIM first
            $licStatus = (Get-CimInstance -ClassName SoftwareLicensingProduct -Filter "Name like '%Windows%' AND LicenseStatus = 1" -OperationTimeoutSec 3 -ErrorAction Stop | Select-Object -First 1)
            $windowsActivated = ($null -ne $licStatus)
        }
        catch {
            # Fallback to slmgr
            try {
                $slmgrOutput = cscript //nologo "$env:SystemRoot\System32\slmgr.vbs" /dli 2>&1 | Out-String
                if ($slmgrOutput -match 'License Status:\s*(Licensed|Qualified)') {
                    $windowsActivated = $true
                }
            }
            catch { $windowsActivated = $false }
        }

        try {
            $osReg = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue
            $edition = if ($osReg.ProductName) { $osReg.ProductName } else { 'Windows' }
            if ($osReg.DisplayVersion) { $edition = "$edition $($osReg.DisplayVersion)" }
        }
        catch {}

        $officeStatus = Get-OfficeStatus

        @{
            windows = @{
                activated = $windowsActivated
                edition   = $edition
            }
            office  = $officeStatus
        }
    }
    catch {
        @{ error = $true; message = "Failed to get activation status: $($_.Exception.Message)" }
    }
}

# Open Windows Activation settings
function Open-ActivationSettings {
    Start-Process "ms-settings:activation"
    @{ status = 'opened' }
}
