# ============================================================================
# APPS - UNINSTALLER MODULE
# Specialized uninstallation routines for integrated Windows apps
# ============================================================================

# Remove Microsoft Edge using the official installer (force)
function Invoke-RemoveEdge {
    Assert-IsAdmin
    Write-Host "Unlocking The Official Edge Uninstaller And Removing Microsoft Edge..."
    try {
        $installer = Get-ChildItem "C:\Program Files (x86)\Microsoft\Edge\Application\*\Installer\setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($installer) {
            $Path = $installer.FullName
            # Dummy file to trick Windows (Edge sometimes checks for this)
            New-Item "C:\Windows\SystemApps\Microsoft.MicrosoftEdge_8wekyb3d8bbwe\MicrosoftEdge.exe" -Force -ErrorAction SilentlyContinue | Out-Null
            
            Start-Process $Path -ArgumentList '--uninstall --system-level --force-uninstall --delete-profile' -Wait -NoNewWindow
            return @{ status = 'removed' }
        }
        else {
            return @{ error = $true; message = "Edge installer not found." }
        }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}

# Surgical Removal of Microsoft OneDrive
function Invoke-RemoveOneDrive {
    Assert-IsAdmin
    try {
        # 1. icacls trick to ensure we can delete it
        if (Test-Path $Env:OneDrive) {
            icacls $Env:OneDrive /deny "Administrators:(D,DC)" | Out-Null
        }

        Write-Host "Uninstalling OneDrive..."
        $oneDriveSetup = @(
            "$env:SystemRoot\System32\OneDriveSetup.exe",
            "$env:SystemRoot\SysWOW64\OneDriveSetup.exe"
        ) | Where-Object { Test-Path $_ } | Select-Object -First 1
        
        if ($oneDriveSetup) {
            Start-Process $oneDriveSetup -ArgumentList '/uninstall' -Wait
        }
        
        # 2. Kill associated processes
        Write-Host "Stopping residual processes..."
        Stop-Process -Name "FileCoAuth", "OneDrive" -ErrorAction SilentlyContinue
        Stop-Process -Name "Explorer" -Force -ErrorAction SilentlyContinue
        
        # 3. Cleanup residual folders
        $folders = @(
            "$env:LocalAppData\Microsoft\OneDrive",
            "C:\ProgramData\Microsoft OneDrive",
            "C:\OneDriveTemp"
        )
        foreach ($folder in $folders) {
            if (Test-Path $folder) { Invoke-7Erase -Path $folder -Type File }
        }
        
        # 4. Remove sidebar icon (CLSID)
        $clsidPath = "HKCU:\Software\Classes\CLSID\{018D5C66-4533-4307-9B53-224DE2ED1FE6}"
        if (Test-Path $clsidPath) { Invoke-7Erase -Path $clsidPath -Type Registry }
        
        # 5. Restore permissions if folder still exists
        if (Test-Path $Env:OneDrive) {
            icacls $Env:OneDrive /grant "Administrators:(D,DC)" | Out-Null
        }

        Start-Process "explorer.exe"
        @{ status = 'removed' }
    }
    catch {
        Start-Process "explorer.exe"
        @{ error = $true; message = "Failed to remove OneDrive: $($_.Exception.Message)" }
    }
}

# Check if Microsoft Edge is installed
function Test-EdgeInstalled {
    $installer = Get-ChildItem "C:\Program Files (x86)\Microsoft\Edge\Application\*\Installer\setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    return @{ installed = $null -ne $installer }
}

# Check if Microsoft OneDrive is installed
function Test-OneDriveInstalled {
    $oneDriveSetup = @(
        "$env:SystemRoot\System32\OneDriveSetup.exe",
        "$env:SystemRoot\SysWOW64\OneDriveSetup.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    return @{ installed = $null -ne $oneDriveSetup }
}

# Returns native, locally installed icons for the integrated Windows-extra
# actions. These are deliberately resolved from the executable/AppX package,
# not downloaded or guessed from a public icon service.
function Get-DebloatWindowsIconData {
    try {
        $icons = @{}
        $edgeExe = @(
            "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
            "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
        ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf -ErrorAction SilentlyContinue) } | Select-Object -First 1
        if ($edgeExe) { $icons.edge = Get-WcExecutableIconData -SourcePath $edgeExe -CacheKey 'debloat-edge' }

        $oneDriveExe = @(
            "$env:LOCALAPPDATA\Microsoft\OneDrive\OneDrive.exe",
            "$env:ProgramFiles\Microsoft OneDrive\OneDrive.exe"
        ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf -ErrorAction SilentlyContinue) } | Select-Object -First 1
        if ($oneDriveExe) { $icons.onedrive = Get-WcExecutableIconData -SourcePath $oneDriveExe -CacheKey 'debloat-onedrive' }

        $teamsPackage = @(
            Get-AppxPackage -Name 'MSTeams' -ErrorAction SilentlyContinue
            Get-AppxPackage -Name 'MicrosoftTeams' -ErrorAction SilentlyContinue
            Get-AppxPackage -Name 'MSTeams' -AllUsers -ErrorAction SilentlyContinue
            Get-AppxPackage -Name 'MicrosoftTeams' -AllUsers -ErrorAction SilentlyContinue
        ) | Where-Object { $_ -and $_.InstallLocation } | Select-Object -First 1
        if ($teamsPackage) { $icons.teams = Get-WcAppxIconData -Package $teamsPackage }

        $copilotPackage = @(
            Get-AppxPackage -Name 'Microsoft.Copilot' -ErrorAction SilentlyContinue
            Get-AppxPackage -Name 'Microsoft.Windows.Copilot' -ErrorAction SilentlyContinue
            Get-AppxPackage -Name '*Copilot*' -ErrorAction SilentlyContinue
        ) | Where-Object { $_ -and $_.InstallLocation } | Select-Object -First 1
        if ($copilotPackage) { $icons.'copilot-ai' = Get-WcAppxIconData -Package $copilotPackage }

        return @{ icons = $icons }
    }
    catch { return @{ icons = @{} } }
}

# ============================================================================
# NEW: Microsoft Teams Removal
# ============================================================================

function Remove-MicrosoftTeams {
    Assert-IsAdmin
    try {
        $removed = $false

        # 1. Remove new Teams (MSIX / AppX)
        Get-AppxPackage -Name "MSTeams" -AllUsers -ErrorAction SilentlyContinue | ForEach-Object {
            Remove-AppxPackage -Package $_.PackageFullName -AllUsers -ErrorAction SilentlyContinue
            $removed = $true
        }
        Get-AppxPackage -Name "MicrosoftTeams" -AllUsers -ErrorAction SilentlyContinue | ForEach-Object {
            Remove-AppxPackage -Package $_.PackageFullName -AllUsers -ErrorAction SilentlyContinue
            $removed = $true
        }

        # 2. Deprovision to block reinstall
        Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue |
            Where-Object { $_.PackageName -like "MSTeams*" -or $_.PackageName -like "MicrosoftTeams*" } |
            ForEach-Object {
                Remove-AppxProvisionedPackage -Online -PackageName $_.PackageName -ErrorAction SilentlyContinue | Out-Null
                $removed = $true
            }

        # 3. Remove classic Teams (machine-wide installer)
        $teamsPath = "${env:ProgramFiles(x86)}\Microsoft\Teams\Update.exe"
        if (Test-Path $teamsPath) {
            Start-Process $teamsPath -ArgumentList '--uninstall -s' -Wait -NoNewWindow -ErrorAction SilentlyContinue
            $removed = $true
        }

        # 4. Remove per-user classic Teams
        $localTeams = "$env:LOCALAPPDATA\Microsoft\Teams\Update.exe"
        if (Test-Path $localTeams) {
            Start-Process $localTeams -ArgumentList '--uninstall -s' -Wait -NoNewWindow -ErrorAction SilentlyContinue
            $removed = $true
        }

        # 5. Prevent Teams from reinstalling via Chat icon policy
        $chatPath = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Chat"
        if (!(Test-Path $chatPath)) { New-Item -Path $chatPath -Force | Out-Null }
        Set-ItemProperty -Path $chatPath -Name "ChatIcon" -Value 3 -Type DWord -Force

        # 6. Add deprovisioning registry keys
        $deprovBase = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned"
        @("MSTeams_8wekyb3d8bbwe", "MicrosoftTeams_8wekyb3d8bbwe") | ForEach-Object {
            $dp = "$deprovBase\$_"
            if (!(Test-Path $dp)) { New-Item -Path $dp -Force | Out-Null }
        }

        @{ status = if ($removed) { 'removed' } else { 'not_found' } }
    }
    catch {
        @{ error = $true; message = "Failed to remove Teams: $($_.Exception.Message)" }
    }
}

# ============================================================================
# APPX DEBLOAT
# Lists, removes, restores, and deprovisions Store/MSIX apps locally in Free.
# ============================================================================

function Test-AppxPackageHasInstalledUser {
    param([Parameter(Mandatory = $true)]$Package)
    foreach ($info in @($Package.PackageUserInformation)) {
        $state = $null
        try { $state = [string]$info.InstallState } catch {}
        if ($state -eq "Installed") { return $true }
        $text = [string]$info
        if ($text -match ":\s*Installed\s*$") { return $true }
    }
    return $false
}

function Get-InstalledAppxInventory {
    try {
        $provisioned = @{}
        try {
            Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue | ForEach-Object {
                if ($_.DisplayName) { $provisioned[$_.DisplayName] = $_ }
            }
        } catch {}

        $installed = @{}
        Get-AppxPackage -ErrorAction SilentlyContinue | ForEach-Object {
            if ($_.Name) { $installed[$_.Name] = $_ }
        }
        try {
            Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue | ForEach-Object {
                if (-not $_.Name -or $installed.ContainsKey($_.Name)) { return }
                if ((Test-AppxPackageHasInstalledUser -Package $_) -or $provisioned.ContainsKey($_.Name)) {
                    $installed[$_.Name] = $_
                }
            }
        } catch {}

        $apps = @($installed.Values | Sort-Object Name | ForEach-Object {
            @{
                name            = $_.Name
                packageFullName = $_.PackageFullName
                version         = [string]$_.Version
                publisher       = $_.Publisher
                isProvisioned   = $provisioned.ContainsKey($_.Name)
                iconData        = Get-WcAppxIconData -Package $_
            }
        })

        foreach ($name in ($provisioned.Keys | Sort-Object)) {
            if ($installed.ContainsKey($name)) { continue }
            $pkg = $provisioned[$name]
            $apps += @{
                name            = $name
                packageFullName = $pkg.PackageName
                version         = [string]$pkg.Version
                publisher       = $pkg.PublisherId
                isProvisioned   = $true
                iconData        = $null
            }
        }
        return @{ apps = $apps }
    }
    catch {
        return @{ error = $true; message = "Get-InstalledAppxInventory failed: $($_.Exception.Message)" }
    }
}

function Remove-AppxByName {
    param([Parameter(Mandatory = $true)][string]$Name)
    if ([string]::IsNullOrWhiteSpace($Name)) {
        return @{ error = $true; status = "invalid_name"; message = "Remove-AppxByName requires a non-empty package name." }
    }
    Assert-IsAdmin
    try {
        $attempted = $false
        $errors = New-Object System.Collections.Generic.List[string]
        $seenPackages = @{}

        Get-AppxPackage -Name $Name -ErrorAction SilentlyContinue | ForEach-Object {
            if ($_.PackageFullName) { $seenPackages[$_.PackageFullName] = $_ }
            try {
                Remove-AppxPackage -Package $_.PackageFullName -ErrorAction Stop
                $attempted = $true
            }
            catch {
                $errors.Add("current-user:$($_.PackageFullName): $($_.Exception.Message)")
            }
        }

        Get-AppxPackage -AllUsers -Name $Name -ErrorAction SilentlyContinue | ForEach-Object {
            if ($_.PackageFullName) { $seenPackages[$_.PackageFullName] = $_ }
            try {
                Remove-AppxPackage -Package $_.PackageFullName -AllUsers -ErrorAction Stop
                $attempted = $true
            }
            catch {
                $errors.Add("all-users:$($_.PackageFullName): $($_.Exception.Message)")
            }
        }

        Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue |
            Where-Object { $_.DisplayName -eq $Name -or $_.PackageName -like "$Name*" } |
            ForEach-Object {
                try {
                    Remove-AppxProvisionedPackage -Online -PackageName $_.PackageName -ErrorAction Stop | Out-Null
                    $attempted = $true
                }
                catch {
                    $errors.Add("provisioned:$($_.PackageName): $($_.Exception.Message)")
            }
        }

        # KT: Some inbox apps remain in an all-users "Staged" state under
        # S-1-5-18 even after normal removal. Retry exact PackageFullName
        # values gathered before deprovisioning so we do not report success
        # while Get-AppxPackage -AllUsers still sees the staged package.
        foreach ($pkg in $seenPackages.Keys) {
            if ([string]::IsNullOrWhiteSpace($pkg)) { continue }
            try {
                Remove-AppxPackage -Package $pkg -AllUsers -ErrorAction Stop
                $attempted = $true
            }
            catch {
                $errors.Add("staged:${pkg}: $($_.Exception.Message)")
            }
        }

        $remainingInstalled = @()
        Get-AppxPackage -Name $Name -ErrorAction SilentlyContinue | ForEach-Object {
            $remainingInstalled += $_.PackageFullName
        }
        try {
            Get-AppxPackage -AllUsers -Name $Name -ErrorAction SilentlyContinue | ForEach-Object {
                if (
                    (Test-AppxPackageHasInstalledUser -Package $_) -and
                    $remainingInstalled -notcontains $_.PackageFullName
                ) {
                    $remainingInstalled += $_.PackageFullName
                }
            }
        } catch {}

        $remainingProvisioned = @()
        Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue |
            Where-Object { $_.DisplayName -eq $Name -or $_.PackageName -like "$Name*" } |
            ForEach-Object { $remainingProvisioned += $_.PackageName }

        if ($remainingInstalled.Count -gt 0 -or $remainingProvisioned.Count -gt 0) {
            return @{
                error                = $true
                status               = "failed"
                name                 = $Name
                message              = "AppX package '$Name' is still present after removal."
                attempted            = $attempted
                errors               = @($errors)
                remainingInstalled   = @($remainingInstalled)
                remainingProvisioned = @($remainingProvisioned)
            }
        }

        @{ status = if ($attempted) { "removed" } else { "not_found" }; name = $Name; errors = @($errors) }
    }
    catch {
        @{ error = $true; message = "Remove-AppxByName failed for '$Name': $($_.Exception.Message)" }
    }
}

function Restore-AppxByName {
    param([Parameter(Mandatory = $true)][string]$Name)
    Assert-IsAdmin
    try {
        $restored = $false
        Get-AppxPackage -AllUsers -Name $Name -ErrorAction SilentlyContinue | ForEach-Object {
            $manifest = Join-Path $_.InstallLocation "AppXManifest.xml"
            if (Test-Path -LiteralPath $manifest) {
                Add-AppxPackage -DisableDevelopmentMode -Register $manifest -ErrorAction Continue
                $restored = $true
            }
        }
        @{ status = if ($restored) { "restored" } else { "not_found" }; name = $Name }
    }
    catch {
        @{ error = $true; message = "Restore-AppxByName failed for '$Name': $($_.Exception.Message)" }
    }
}

function Set-AppxDeprovisioned {
    param([Parameter(Mandatory = $true)][string]$Name)
    Assert-IsAdmin
    try {
        $removed = $false
        Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue |
            Where-Object { $_.DisplayName -eq $Name -or $_.PackageName -like "$Name*" } |
            ForEach-Object {
                Remove-AppxProvisionedPackage -Online -PackageName $_.PackageName -ErrorAction Continue | Out-Null
                $removed = $true
            }
        @{ status = if ($removed) { "deprovisioned" } else { "not_found" }; name = $Name }
    }
    catch {
        @{ error = $true; message = "Set-AppxDeprovisioned failed for '$Name': $($_.Exception.Message)" }
    }
}

function Get-TeamsStatus {
    try {
        # Check current user first (no elevation required)
        $msix = Get-AppxPackage -Name "MSTeams" -ErrorAction SilentlyContinue
        $legacy = Get-AppxPackage -Name "MicrosoftTeams" -ErrorAction SilentlyContinue
        # Also try -AllUsers if running elevated (fails silently otherwise)
        if (-not $msix) { $msix = Get-AppxPackage -Name "MSTeams" -AllUsers -ErrorAction SilentlyContinue }
        if (-not $legacy) { $legacy = Get-AppxPackage -Name "MicrosoftTeams" -AllUsers -ErrorAction SilentlyContinue }
        # Classic desktop installer
        $classicPath = "${env:ProgramFiles(x86)}\Microsoft\Teams\current\Teams.exe"
        $hasClassic = Test-Path $classicPath
        # Per-user install
        $hasLocalUser = Test-Path "$env:LOCALAPPDATA\Microsoft\Teams\Update.exe"
        # KT: Windows 11 pre-registers the "MicrosoftTeams"/"MSTeams" (Chat icon)
        # AppX package on essentially every machine by default, whether or not
        # the user ever opened it - a bare Get-AppxPackage match can't tell that
        # empty default shell apart from a real install (this is why Office-only
        # machines were reporting Teams as "installed" with nothing to remove).
        # A genuinely-used client has its app payload actually downloaded under
        # InstallLocation (100+ MB); the untouched default shell is only a
        # manifest + a small stub. Require a real-app-sized footprint before
        # counting the AppX match as an actual install.
        $hasRealMsix = (Test-AppxPackageHasRealPayload $msix) -or (Test-AppxPackageHasRealPayload $legacy)
        @{ installed = ($hasRealMsix -or $hasClassic -or $hasLocalUser) }
    }
    catch {
        @{ installed = $false }
    }
}

# Distinguishes a real, downloaded AppX install from Windows 11's default
# pre-registered shell package (manifest-only, a few hundred KB) by checking
# the on-disk footprint under InstallLocation. See Get-TeamsStatus above.
function Test-AppxPackageHasRealPayload {
    param($Packages)
    foreach ($pkg in @($Packages)) {
        if (-not $pkg -or -not $pkg.InstallLocation) { continue }
        if (-not (Test-Path $pkg.InstallLocation)) { continue }
        $size = (Get-ChildItem -LiteralPath $pkg.InstallLocation -Recurse -File -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
        if ($size -gt 20MB) { return $true }
    }
    return $false
}
