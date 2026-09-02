# ============================================================================
# SYSTEM - SECURITY MODULE
# Sensitive security operations and emergency procedures
# ============================================================================

# ============================================================================
# NEW: VBS (Virtualization Based Security) - from ReviOS vbs.yml
# ============================================================================

# --- Remote Assistance and anonymous SAM enumeration ---

function Disable-RemoteAssistance {
    Assert-IsAdmin
    try {
        $runtimePath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Remote Assistance'
        $policyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
        if (!(Test-Path $runtimePath)) { New-Item -Path $runtimePath -Force | Out-Null }
        if (!(Test-Path $policyPath)) { New-Item -Path $policyPath -Force | Out-Null }
        Set-ItemProperty -Path $runtimePath -Name 'fAllowToGetHelp' -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $policyPath -Name 'fAllowToGetHelp' -Value 0 -Type DWord -Force
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-RemoteAssistance {
    Assert-IsAdmin
    try {
        # Clear this app's policy values instead of forcing a potentially non-default setting.
        Remove-ItemSecure -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Remote Assistance' -Name 'fAllowToGetHelp' -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services' -Name 'fAllowToGetHelp' -ErrorAction SilentlyContinue
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Block-AnonymousSamEnumeration {
    Assert-IsAdmin
    try {
        Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa' -Name 'RestrictAnonymousSAM' -Value 1 -Type DWord -Force
        @{ status = 'blocked' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Allow-AnonymousSamEnumeration {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa' -Name 'RestrictAnonymousSAM' -ErrorAction SilentlyContinue
        @{ status = 'allowed' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# Disable-VBS relocated to commander-pro (handlers.rs). The degrade direction
# reduces system security (HVCI/Credential Guard off) and is Defender/EDR-flagged,
# so it runs in the Pro sidecar via dispatch_paid_command (tier=Paid). Enable-VBS
# (re-harden) stays Free + local below.

function Enable-VBS {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DeviceGuard" -Name "EnableVirtualizationBasedSecurity" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SYSTEM\CurrentControlSet\Control\DeviceGuard" -Name "EnableVirtualizationBasedSecurity" -ErrorAction SilentlyContinue
        $hvciPath = "HKLM:\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity"
        if (Test-Path $hvciPath) {
            Set-ItemProperty -Path $hvciPath -Name "Enabled" -Value 1 -Type DWord -Force
        }
        @{ status = "enabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# BitLocker Auto-Encryption — moved to commander-pro (A-2)
# ============================================================================
# Disable-BitLockerAutoEncrypt and Enable-BitLockerAutoEncrypt are paid
# (tier=paid in backend.rs::get_command_tier). Their implementations
# live inline in commander-pro/src/handlers.rs; Free's dispatch routes
# them via dispatch_paid_command before any local module loads.
# Strings-grep CI gate (A-5) verifies the function names no longer
# appear in Free's encrypted .enc bundle after build.

# ============================================================================
# NEW: WPBT (Windows Platform Binary Table)
# ============================================================================

function Disable-WPBT {
    Assert-IsAdmin
    try {
        $path = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager"
        Set-ItemProperty -Path $path -Name "DisableWpbtExecution" -Value 1 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-WPBT {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager" -Name "DisableWpbtExecution" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# BitLocker TPM+PIN enforcement (Free, reversible — owner decision 2026-07-10)
# ============================================================================
# Adds a TPM+PIN protector to the target drive and removes the pure-TPM one.
# Reversible: disable path removes the PIN protector and re-adds TPM-only.
# Params: Enable [bool], Pin [string, 6-20 ASCII digits, required when Enable],
# Drive [string, defaults to "C:"].

function Set-BitLockerTpmPin {
    param(
        [bool]$Enable = $true,
        [string]$Pin,
        [string]$Drive = "C:"
    )
    Assert-IsAdmin
    try {
        # Drive whitelist: exactly one letter + colon, no separators/backticks/nulls.
        if ($Drive -notmatch '^[A-Za-z]:$') {
            return @{ error = $true; message = "Set-BitLockerTpmPin: invalid Drive" }
        }
        $drive = $Drive.ToUpper()

        if ($Enable) {
            if ([string]::IsNullOrEmpty($Pin) -or $Pin -notmatch '^[0-9]{6,20}$') {
                return @{ error = $true; message = "Set-BitLockerTpmPin: Pin must be 6-20 ASCII digits" }
            }
            $securePin = ConvertTo-SecureString $Pin -AsPlainText -Force
            Add-BitLockerKeyProtector -MountPoint $drive -TpmAndPinProtector -Pin $securePin -ErrorAction Stop | Out-Null
            $tpmOnly = (Get-BitLockerVolume -MountPoint $drive).KeyProtector |
                Where-Object { $_.KeyProtectorType -eq 'Tpm' }
            foreach ($kp in $tpmOnly) {
                Remove-BitLockerKeyProtector -MountPoint $drive -KeyProtectorId $kp.KeyProtectorId -ErrorAction SilentlyContinue | Out-Null
            }
            @{ status = "enabled"; drive = $drive }
        }
        else {
            $tpmPin = (Get-BitLockerVolume -MountPoint $drive).KeyProtector |
                Where-Object { $_.KeyProtectorType -eq 'TpmPin' }
            foreach ($kp in $tpmPin) {
                Remove-BitLockerKeyProtector -MountPoint $drive -KeyProtectorId $kp.KeyProtectorId -ErrorAction SilentlyContinue | Out-Null
            }
            Add-BitLockerKeyProtector -MountPoint $drive -TpmProtector -ErrorAction SilentlyContinue | Out-Null
            @{ status = "disabled"; drive = $drive }
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: SmartScreen (from ReviOS security.yml)
# ============================================================================

# Disable-SmartScreen relocated to commander-pro (handlers.rs). Turning off
# SmartScreen reputation/phishing protection reduces security and is AV-flagged,
# so it runs in the Pro sidecar via dispatch_paid_command (tier=Paid).
# Enable-SmartScreen (re-harden) stays Free + local below.

function Enable-SmartScreen {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\Software\Policies\Microsoft\Windows\System" -Name "EnableSmartScreen" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\Software\Policies\Microsoft\MicrosoftEdge\PhishingFilter" -Name "EnabledV9" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\Software\Policies\Microsoft\Windows Defender\SmartScreen" -Name "ConfigureAppInstallControlEnabled" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\Software\Policies\Microsoft\Windows Defender\SmartScreen" -Name "ConfigureAppInstallControl" -ErrorAction SilentlyContinue
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\AppHost" -Name "EnableWebContentEvaluation" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: OOBE Bypass (from ReviOS oobe.yml)
# ============================================================================

function Set-OOBEBypass {
    Assert-IsAdmin
    try {
        $keys = @(
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE"; Name = "HideOnlineAccountScreens"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE"; Name = "HideOnlineAccountScreens"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE"; Name = "HideEULAPage"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE"; Name = "HideEULAPage"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE"; Name = "DisablePrivacyExperience"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE"; Name = "DisablePrivacyExperience"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE"; Name = "HideWirelessSetupInOOBE"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE"; Name = "HideWirelessSetupInOOBE"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE"; Name = "ProtectYourPC"; Value = 3 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE"; Name = "ProtectYourPC"; Value = 3 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE"; Name = "EnableCortanaVoice"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE"; Name = "EnableCortanaVoice"; Value = 0 },
            @{ Path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE"; Name = "DisableVoice"; Value = 1 },
            @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE"; Name = "DisableVoice"; Value = 1 }
        )
        foreach ($k in $keys) {
            if (!(Test-Path $k.Path)) { New-Item -Path $k.Path -Force | Out-Null }
            Set-ItemProperty -Path $k.Path -Name $k.Name -Value $k.Value -Type DWord -Force
        }
        @{ status = "bypassed" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Clear-OOBEBypass {
    Assert-IsAdmin
    try {
        $names = @("HideOnlineAccountScreens", "HideEULAPage", "DisablePrivacyExperience", "HideWirelessSetupInOOBE", "ProtectYourPC", "EnableCortanaVoice", "DisableVoice")
        foreach ($n in $names) {
            Remove-ItemSecure -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE" -Name $n -ErrorAction SilentlyContinue
            Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE" -Name $n -ErrorAction SilentlyContinue
        }
        @{ status = "cleared" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# NEW: Game DVR
# ============================================================================

function Disable-GameDVR {
    try {
        $path = "HKCU:\System\GameConfigStore"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "GameDVR_Enabled" -Value 0 -Type DWord -Force

        $polPath = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\GameDVR"
        if (!(Test-Path $polPath)) { New-Item -Path $polPath -Force | Out-Null }
        Set-ItemProperty -Path $polPath -Name "AllowGameDVR" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-GameDVR {
    try {
        Set-ItemProperty -Path "HKCU:\System\GameConfigStore" -Name "GameDVR_Enabled" -Value 1 -Type DWord -Force -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\GameDVR" -Name "AllowGameDVR" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Windows 11 Requirements Bypass (TPM, SecureBoot, CPU, RAM, Storage)
# ============================================================================

# Enable-Win11RequirementsBypass relocated to commander-pro (handlers.rs).
# Bypassing Secure Boot / TPM enforcement weakens platform integrity and is
# AV-flagged, so it runs in the Pro sidecar via dispatch_paid_command
# (tier=Paid). Disable-Win11RequirementsBypass (revert) stays Free + local below.

function Disable-Win11RequirementsBypass {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SYSTEM\Setup\LabConfig" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SYSTEM\Setup\MoSetup" -Name "AllowUpgradesWithUnsupportedTPMOrCPU" -ErrorAction SilentlyContinue
        @{ status = "cleared" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# IFEO Process Blocking for Telemetry Executables
# ============================================================================

# Enable-IFEOTelemetryBlock relocated to commander-pro (handlers.rs). IFEO
# Debugger hijacking (T1546.012) is an AV/EDR-flagged technique regardless of
# intent, so it runs in the Pro sidecar via dispatch_paid_command (tier=Paid).
# Disable-IFEOTelemetryBlock (revert) stays Free + local below.

function Disable-IFEOTelemetryBlock {
    Assert-IsAdmin
    try {
        $targets = @("CompatTelRunner.exe","DeviceCensus.exe","AggregatorHost.exe","BingChatInstaller.exe","GameBarPresenceWriter.exe","MsCtfMonitor.exe")
        foreach ($exe in $targets) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$exe"
            Remove-ItemSecure -Path $path -Name "Debugger" -ErrorAction SilentlyContinue
        }
        @{ status = "unblocked" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Shared Extension Lists (single source of truth)
# ============================================================================

# Chromium extensions: full uBlock Origin for MV2-capable browsers, Lite for Chrome
$script:UBlockFull = @{ Id = "cjpalhdlnbpafiamejdnhcphjbkeiagm"; Name = "uBlock Origin" }
$script:UBlockLite = @{ Id = "ddkjiahejlhfcafbddmgiahcphecmpfh"; Name = "uBlock Origin Lite" }

# uBlock Origin managed-storage config (full uBO / MV2 only — Brave, Edge, Vivaldi, etc.).
# toOverwrite.filterLists enforces this exact set of enabled lists on each browser launch.
# Edit these two arrays to customize the deployment. Unknown tokens are ignored by uBO.
$script:UBlockFilterLists = @(
    # uBO defaults (keep these enabled)
    "user-filters", "ublock-filters", "ublock-badware", "ublock-privacy",
    "ublock-abuse", "ublock-unbreak", "ublock-quick-fixes", "easylist", "easyprivacy",
    # Extra privacy / malware / security hardening lists
    "adguard-spyware-url",   # AdGuard URL Tracking protection
    "urlhaus-1",             # abuse.ch malicious URL blocklist
    "plowe-0",               # Peter Lowe's ad + tracking servers
    "block-lan",             # block web pages from probing the LAN (does NOT block you visiting the router directly)
    "fanboy-cookiemonster"   # EasyList cookie-consent banners
)
# Custom "My filters" rules — array of filter strings. Add your own under the comment line.
$script:UBlockCustomFilters = @(
    "! Custom filters deployed by Commander"
)

# ToggleKey is the stable per-browser id a user can enable/disable via
# privacy.browserExtensions (see src/registry/browserExtensions.ts, which
# must list the same keys). Entries without a ToggleKey (uBlock) aren't
# individually toggleable — they're always force-installed. Chromium's
# "Volume Master" and Firefox's "SoundFixer" share one ToggleKey: same
# feature, different extension per browser engine.
$script:ChromiumExtensionsBase = @(
    @{ Id = "pkehgijcmpdhfbdbbnkijodmdjhbjlgp"; Name = "Privacy Badger"; ToggleKey = "privacy-badger" }
    @{ Id = "mnjggcdmjocbbbhaepdhchncahnbgone"; Name = "SponsorBlock"; ToggleKey = "sponsorblock" }
    @{ Id = "jghecgabfgfdldnmbfkhmffcabddioke"; Name = "Volume Master"; ToggleKey = "volume-boost" }
    @{ Id = "gebbhagfogifgggkldgodflihgfeippi"; Name = "Return YouTube Dislike"; ToggleKey = "return-youtube-dislike" }
    @{ Id = "lckanjgmijmafbedllaakclkaicjfmnk"; Name = "ClearURLs"; ToggleKey = "clearurls" }
    @{ Id = "cnojnbdhbhnkbcieeekonklommdnndci"; Name = "Search by Image"; ToggleKey = "search-by-image" }
)

$script:FirefoxExtensions = @(
    @{ Slug = "ublock-origin";            Name = "uBlock Origin";           Guid = "uBlock0@raymondhill.net" }
    @{ Slug = "privacy-badger17";         Name = "Privacy Badger";          Guid = "jid1-MnnxcxisBPnSXQ@jetpack"; ToggleKey = "privacy-badger" }
    @{ Slug = "sponsorblock";             Name = "SponsorBlock";            Guid = "sponsorBlocker@ajay.app"; ToggleKey = "sponsorblock" }
    @{ Slug = "soundfixer";               Name = "SoundFixer";              Guid = "soundfixer@unrelenting.technology"; ToggleKey = "volume-boost" }
    @{ Slug = "return-youtube-dislikes";  Name = "Return YouTube Dislike";  Guid = "{762f9885-5a13-4abd-9c77-433dcd38b8fd}"; ToggleKey = "return-youtube-dislike" }
    @{ Slug = "clearurls";                Name = "ClearURLs";               Guid = "{74145f27-f039-47ce-a470-a662b129930a}"; ToggleKey = "clearurls" }
    @{ Slug = "search_by_image";          Name = "Search by Image";         Guid = "{2e5ff8c8-32fe-46d0-9fc8-6b8986621f3c}"; ToggleKey = "search-by-image" }
)

# Filters a master extension list (ChromiumExtensionsBase / FirefoxExtensions)
# down to what the command is allowed to install. Per-user preferences are
# encrypted in Rust and are not readable from PowerShell; absent a typed policy
# passed by Rust, preserve the safe historical default of enabling every entry.
function Get-BrowserExtensionSettingKey {
    param(
        [string]$BrowserName,
        [string]$ToggleKey
    )
    return "${BrowserName}::${ToggleKey}"
}

function Get-EnabledBrowserExtensions {
    param(
        [array]$MasterList,
        [string]$BrowserName
    )
    $toggles = $null
    return @($MasterList | Where-Object {
        if (-not $_.ToggleKey) { return $true }
        if ($null -eq $toggles) { return $true }
        $browserKey = Get-BrowserExtensionSettingKey -BrowserName $BrowserName -ToggleKey $_.ToggleKey
        $browserProp = $toggles.PSObject.Properties[$browserKey]
        if ($null -ne $browserProp) { return [bool]$browserProp.Value }
        # Legacy slug-only preferences stay effective until this browser is
        # explicitly configured, preserving existing users' choices.
        $legacyProp = $toggles.PSObject.Properties[$_.ToggleKey]
        return ($null -eq $legacyProp) -or [bool]$legacyProp.Value
    })
}

function Get-DisabledBrowserExtensions {
    param(
        [array]$MasterList,
        [string]$BrowserName
    )
    $toggles = $null
    if ($null -eq $toggles) { return @() }
    return @($MasterList | Where-Object {
        if (-not $_.ToggleKey) { return $false }
        $browserKey = Get-BrowserExtensionSettingKey -BrowserName $BrowserName -ToggleKey $_.ToggleKey
        $browserProp = $toggles.PSObject.Properties[$browserKey]
        if ($null -ne $browserProp) { return -not [bool]$browserProp.Value }
        $legacyProp = $toggles.PSObject.Properties[$_.ToggleKey]
        return ($null -ne $legacyProp) -and (-not [bool]$legacyProp.Value)
    })
}

function Install-ChromiumExtensions {
    <#
    .SYNOPSIS Force-install all Chromium extensions via ExtensionSettings policy.
    .PARAMETER PolicyPath Registry path e.g. HKLM:\SOFTWARE\Policies\Google\Chrome
    .PARAMETER UseLiteUBlock Use uBlock Origin Lite (MV3) instead of full (MV2). Required for Chrome.
    #>
    param(
        [string]$PolicyPath,
        [string]$BrowserName,
        [switch]$UseLiteUBlock
    )
    $updateUrl = "https://clients2.google.com/service/update2/crx"
    $ublock    = if ($UseLiteUBlock) { $script:UBlockLite } else { $script:UBlockFull }
    $allExts   = @($ublock) + (Get-EnabledBrowserExtensions -MasterList $script:ChromiumExtensionsBase -BrowserName $BrowserName)
    $disabledExts = Get-DisabledBrowserExtensions -MasterList $script:ChromiumExtensionsBase -BrowserName $BrowserName

    # ExtensionSettings: force_installed per extension ID. incognito is NOT a valid policy
    # field — it is handled separately by patching each profile's Preferences file.
    $settings = [ordered]@{}
    foreach ($ext in $allExts) {
        $settings[$ext.Id] = [ordered]@{
            installation_mode = "force_installed"
            update_url        = $updateUrl
        }
    }
    if (!(Test-Path $PolicyPath)) { New-Item -Path $PolicyPath -Force | Out-Null }
    Set-ItemProperty -Path $PolicyPath -Name "ExtensionSettings" -Value ($settings | ConvertTo-Json -Compress -Depth 3) -Type String -Force
    Remove-ChromiumExtensionForcelistEntries -PolicyPath $PolicyPath -ExtensionIds @($disabledExts | ForEach-Object { $_.Id })
    Remove-ChromiumExtensionProfileDataById -BrowserName $BrowserName -ExtensionIds @($disabledExts | ForEach-Object { $_.Id })
    # NOTE: force_installed extensions auto-update on their own; there is no
    # ExtensionAutoUpdateEnabled policy (Chrome rejects it as unknown), so we don't set one.
}

function Get-ChromiumPolicyPathsForBrowser {
    param($Browser)
    if (-not $Browser -or -not $Browser.PolicyPath) { return @() }
    $paths = @($Browser.PolicyPath)
    if ($Browser.PolicyPath -like "HKLM:\*") {
        $paths += ($Browser.PolicyPath -replace "^HKLM:", "HKCU:")
    }
    return $paths | Where-Object { $_ } | Select-Object -Unique
}

function Test-WinCommanderChromiumExtensionPolicy {
    param($Props)
    if ($null -eq $Props -or $null -eq $Props.ExtensionSettings) { return $false }
    $raw = [string]$Props.ExtensionSettings
    $knownIds = @($script:UBlockFull.Id, $script:UBlockLite.Id) + ($script:ChromiumExtensionsBase | ForEach-Object { $_.Id })
    foreach ($id in $knownIds) {
        if ($raw -like "*$id*" -and $raw -like "*force_installed*") { return $true }
    }
    return $false
}

function Test-WinCommanderChromiumForcelistPolicy {
    param([string]$PolicyPath)
    $forcelistPath = "$PolicyPath\ExtensionInstallForcelist"
    if (!(Test-Path $forcelistPath)) { return $false }
    $knownIds = @($script:UBlockFull.Id, $script:UBlockLite.Id) + ($script:ChromiumExtensionsBase | ForEach-Object { $_.Id })
    try {
        $props = Get-ItemProperty -Path $forcelistPath -ErrorAction SilentlyContinue
        if ($null -eq $props) { return $false }
        foreach ($prop in $props.PSObject.Properties) {
            if ($prop.Name -match "^PS") { continue }
            $value = [string]$prop.Value
            foreach ($id in $knownIds) {
                if ($value -like "$id;*" -or $value -eq $id) { return $true }
            }
        }
    } catch { return $false }
    return $false
}

function Get-WinCommanderChromiumExtensionIds {
    $ids = @($script:UBlockFull.Id, $script:UBlockLite.Id) + ($script:ChromiumExtensionsBase | ForEach-Object { $_.Id })
    return $ids | Where-Object { $_ } | Select-Object -Unique
}

function Remove-ChromiumExtensionForcelistEntries {
    param(
        [string]$PolicyPath,
        [string[]]$ExtensionIds
    )
    if (-not $PolicyPath -or $ExtensionIds.Count -eq 0) { return }
    $forcelistPath = "$PolicyPath\ExtensionInstallForcelist"
    if (!(Test-Path $forcelistPath)) { return }
    try {
        $props = Get-ItemProperty -Path $forcelistPath -ErrorAction Stop
        foreach ($prop in $props.PSObject.Properties) {
            if ($prop.Name -match "^PS") { continue }
            $value = [string]$prop.Value
            if ($ExtensionIds | Where-Object { $value -like "$_;*" -or $value -eq $_ }) {
                Remove-ItemProperty -Path $forcelistPath -Name $prop.Name -Force -ErrorAction SilentlyContinue
            }
        }
    } catch {}
}

function Remove-BrowserPolicyPathIfPresent {
    param([string]$Path)
    if ($Path -and (Test-Path $Path)) {
        try { Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop }
        catch { Remove-ItemSecure -Path $Path -Recurse -Force -ErrorAction SilentlyContinue }
    }
}

function Remove-BrowserPolicyValueIfPresent {
    param(
        [string]$Path,
        [string]$Name
    )
    if ($Path -and $Name -and (Test-Path $Path)) {
        $props = Get-ItemProperty -Path $Path -Name $Name -ErrorAction SilentlyContinue
        if ($null -ne $props) {
            try { Remove-ItemProperty -Path $Path -Name $Name -Force -ErrorAction Stop }
            catch { Remove-ItemSecure -Path $Path -Name $Name -Force -ErrorAction SilentlyContinue }
        }
    }
}

function Remove-ChromiumExtensionProfileData {
    param($Browser)
    if (-not $Browser -or -not $Browser.UserDataDir -or -not (Test-Path $Browser.UserDataDir)) {
        return @{ removed = 0; errors = @() }
    }

    $removed = 0
    $errors = @()
    $ids = Get-WinCommanderChromiumExtensionIds

    $profileDirs = @()
    try {
        $profileDirs = @(Get-ChildItem -LiteralPath $Browser.UserDataDir -Directory -ErrorAction SilentlyContinue |
            Where-Object {
                $_.Name -in @("Default", "Guest Profile", "System Profile") -or
                $_.Name -like "Profile *" -or
                (Test-Path (Join-Path $_.FullName "Preferences"))
            })
    } catch {}

    foreach ($profile in $profileDirs) {
        foreach ($id in $ids) {
            $targets = @(
                (Join-Path $profile.FullName "Extensions\$id"),
                (Join-Path $profile.FullName "Local Extension Settings\$id"),
                (Join-Path $profile.FullName "Sync Extension Settings\$id"),
                (Join-Path $profile.FullName "Extension Rules\$id"),
                (Join-Path $profile.FullName "Extension Scripts\$id"),
                (Join-Path $profile.FullName "Extension State\$id")
            )

            $indexedDb = Join-Path $profile.FullName "IndexedDB"
            if (Test-Path $indexedDb) {
                try {
                    $targets += @(Get-ChildItem -LiteralPath $indexedDb -Directory -Filter "chrome-extension_${id}_*" -ErrorAction SilentlyContinue |
                        ForEach-Object { $_.FullName })
                } catch {}
            }

            foreach ($target in $targets) {
                if (-not $target -or -not (Test-Path $target)) { continue }
                try {
                    Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction Stop
                    $removed++
                } catch {
                    $errors += "${target}: $($_.Exception.Message)"
                }
            }
        }
    }

    @{ removed = $removed; errors = $errors }
}

function Remove-ChromiumExtensionProfileDataById {
    param(
        [string]$BrowserName,
        [string[]]$ExtensionIds
    )
    if (-not $BrowserName -or $ExtensionIds.Count -eq 0) { return @{ removed = 0; errors = @() } }
    $browser = @(Get-InstalledBrowsers | Where-Object { $_.Name -eq $BrowserName } | Select-Object -First 1)
    if ($browser.Count -eq 0 -or -not $browser[0].UserDataDir -or -not (Test-Path $browser[0].UserDataDir)) {
        return @{ removed = 0; errors = @() }
    }

    $removed = 0
    $errors = @()
    $profileDirs = @(Get-ChildItem -LiteralPath $browser[0].UserDataDir -Directory -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -in @("Default", "Guest Profile", "System Profile") -or $_.Name -like "Profile *" -or (Test-Path (Join-Path $_.FullName "Preferences"))
    })
    foreach ($profile in $profileDirs) {
        foreach ($id in $ExtensionIds) {
            $targets = @(
                (Join-Path $profile.FullName "Extensions\$id"),
                (Join-Path $profile.FullName "Local Extension Settings\$id"),
                (Join-Path $profile.FullName "Sync Extension Settings\$id"),
                (Join-Path $profile.FullName "Extension Rules\$id"),
                (Join-Path $profile.FullName "Extension Scripts\$id"),
                (Join-Path $profile.FullName "Extension State\$id")
            )
            $indexedDb = Join-Path $profile.FullName "IndexedDB"
            if (Test-Path $indexedDb) {
                $targets += @(Get-ChildItem -LiteralPath $indexedDb -Directory -Filter "chrome-extension_${id}_*" -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
            }
            foreach ($target in $targets) {
                if (-not $target -or -not (Test-Path $target)) { continue }
                try { Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction Stop; $removed++ }
                catch { $errors += "${target}: $($_.Exception.Message)" }
            }
        }
    }
    return @{ removed = $removed; errors = $errors }
}

function Remove-GeckoExtensionProfileData {
    param($Browser)
    if (-not $Browser -or -not $Browser.UserDataDir -or -not (Test-Path $Browser.UserDataDir)) {
        return @{ removed = 0; errors = @() }
    }

    $removed = 0
    $errors = @()
    $guids = $script:FirefoxExtensions | ForEach-Object { $_.Guid }
    $profileDirs = @()
    try {
        $profileDirs = @(Get-ChildItem -LiteralPath $Browser.UserDataDir -Directory -ErrorAction SilentlyContinue |
            Where-Object {
                (Test-Path (Join-Path $_.FullName "prefs.js")) -or
                (Test-Path (Join-Path $_.FullName "extensions"))
            })
    } catch {}

    foreach ($profile in $profileDirs) {
        foreach ($guid in $guids) {
            $safeGuid = $guid -replace '[\\/:*?"<>|]', '_'
            $targets = @(
                (Join-Path $profile.FullName "extensions\$guid.xpi"),
                (Join-Path $profile.FullName "extensions\$guid"),
                (Join-Path $profile.FullName "browser-extension-data\$guid"),
                (Join-Path $profile.FullName "storage\default\moz-extension+++${safeGuid}")
            )
            foreach ($target in $targets) {
                if (-not $target -or -not (Test-Path $target)) { continue }
                try {
                    Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction Stop
                    $removed++
                } catch {
                    $errors += "${target}: $($_.Exception.Message)"
                }
            }
        }
    }

    @{ removed = $removed; errors = $errors }
}

function Remove-GeckoExtensionProfileDataByGuid {
    param(
        [string]$BrowserName,
        [string[]]$ExtensionGuids
    )
    if (-not $BrowserName -or $ExtensionGuids.Count -eq 0) { return @{ removed = 0; errors = @() } }
    $browser = @(Get-InstalledBrowsers | Where-Object { $_.Name -eq $BrowserName } | Select-Object -First 1)
    if ($browser.Count -eq 0 -or -not $browser[0].UserDataDir -or -not (Test-Path $browser[0].UserDataDir)) {
        return @{ removed = 0; errors = @() }
    }

    $removed = 0
    $errors = @()
    $profileDirs = @(Get-ChildItem -LiteralPath $browser[0].UserDataDir -Directory -ErrorAction SilentlyContinue | Where-Object {
        (Test-Path (Join-Path $_.FullName "prefs.js")) -or (Test-Path (Join-Path $_.FullName "extensions"))
    })
    foreach ($profile in $profileDirs) {
        foreach ($guid in $ExtensionGuids) {
            $safeGuid = $guid -replace '[\\/:*?"<>|]', '_'
            $targets = @(
                (Join-Path $profile.FullName "extensions\$guid.xpi"),
                (Join-Path $profile.FullName "extensions\$guid"),
                (Join-Path $profile.FullName "browser-extension-data\$guid"),
                (Join-Path $profile.FullName "storage\default\moz-extension+++${safeGuid}")
            )
            foreach ($target in $targets) {
                if (-not $target -or -not (Test-Path $target)) { continue }
                try { Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction Stop; $removed++ }
                catch { $errors += "${target}: $($_.Exception.Message)" }
            }
        }
    }
    return @{ removed = $removed; errors = $errors }
}

function Reset-ChromiumBrowserPolicy {
    param($Browser)
    $profileCleanup = Remove-ChromiumExtensionProfileData -Browser $Browser
    foreach ($policyPath in (Get-ChromiumPolicyPathsForBrowser -Browser $Browser)) {
        if (!(Test-Path $policyPath)) { continue }
        Remove-BrowserPolicyValueIfPresent -Path $policyPath -Name "ExtensionSettings"
        Remove-BrowserPolicyPathIfPresent -Path "$policyPath\ExtensionInstallForcelist"

        # Remove only the WinCommander hardening keys so unrelated enterprise policies survive.
        $wcNames = @(
            "MetricsReportingEnabled", "SafeBrowsingProtectionLevel", "UrlKeyedAnonymizedDataCollectionEnabled",
            "SpellCheckServiceEnabled", "AlternateErrorPagesEnabled", "NetworkPredictionOptions",
            "SearchSuggestEnabled", "SyncDisabled", "BrowserSignin", "NTPCardsVisible",
            "PromotionalTabsEnabled", "PrivacySandboxPromptEnabled", "PrivacySandboxAdTopicsEnabled",
            "PrivacySandboxSiteEnabledAdsEnabled", "PrivacySandboxAdMeasurementEnabled",
            "BlockThirdPartyCookies", "DnsOverHttpsMode", "PasswordLeakDetectionEnabled",
            "PromotionsEnabled", "ShoppingListEnabled", "BraveRewardsDisabled", "BraveWalletDisabled",
            "BraveVPNDisabled", "BraveAIChatEnabled", "DiagnosticData", "PersonalizationReportingEnabled",
            "SendSiteInfoToImproveServices", "SmartScreenEnabled", "EdgeShoppingAssistantEnabled",
            "EdgeCollectionsEnabled", "HubsSidebarEnabled", "EdgeFollowEnabled",
            "MicrosoftEdgeInsiderPromotionEnabled", "ShowMicrosoftRewards", "ConfigureDoNotTrack",
            "CopilotCDPPageContext", "DiscoverPageContextEnabled", "HideInternetExplorerRedirectUXForIncompatibleSitesEnabled",
            "NewTabPageContentEnabled", "NewTabPageQuickLinksEnabled"
        )
        foreach ($name in $wcNames) {
            Remove-BrowserPolicyValueIfPresent -Path $policyPath -Name $name
        }

        try {
            if (Test-Path $policyPath) {
                $remaining = Get-ItemProperty -Path $policyPath -ErrorAction SilentlyContinue
                $realProps = @($remaining.PSObject.Properties | Where-Object { $_.Name -notmatch "^PS" })
                $childKeys = @(Get-ChildItem -Path $policyPath -ErrorAction SilentlyContinue)
                if ($realProps.Count -eq 0 -and $childKeys.Count -eq 0) {
                    Remove-ItemSecure -Path $policyPath -Recurse -Force -ErrorAction SilentlyContinue
                }
            }
        } catch {}
    }
    return $profileCleanup
}

function Get-OperaPolicyPaths {
    param([string]$Name)
    if ($Name -eq "Opera GX") {
        return @(
            "HKLM:\SOFTWARE\Policies\Opera Software\Opera GX",
            "HKLM:\SOFTWARE\Policies\Opera Software\Opera GX Stable"
        )
    }
    return @(
        "HKLM:\SOFTWARE\Policies\Opera Software\Opera",
        "HKLM:\SOFTWARE\Policies\Opera Software\Opera Stable"
    )
}

function Get-OperaProfileDirs {
    param($Browser)
    $dirs = @($Browser.UserDataDir)
    if ($Browser.Name -eq "Opera GX") {
        $dirs += @(
            "$env:APPDATA\Opera Software\Opera GX Stable",
            "$env:LOCALAPPDATA\Opera Software\Opera GX Stable"
        )
    }
    else {
        $dirs += @(
            "$env:APPDATA\Opera Software\Opera Stable",
            "$env:LOCALAPPDATA\Opera Software\Opera Stable"
        )
    }
    return $dirs | Where-Object { $_ -and (Test-Path $_) } | Select-Object -Unique
}

function Ensure-JsonObjectProperty {
    param($Object, [string]$Name)
    if (-not $Object.PSObject.Properties[$Name] -or $null -eq $Object.$Name) {
        $Object | Add-Member -NotePropertyName $Name -NotePropertyValue ([PSCustomObject]@{}) -Force
    }
    return $Object.$Name
}

function Set-JsonNestedValue {
    param($Object, [string[]]$Path, $Value)
    $current = $Object
    for ($i = 0; $i -lt $Path.Count - 1; $i++) {
        $current = Ensure-JsonObjectProperty -Object $current -Name $Path[$i]
    }
    $leaf = $Path[-1]
    if ($current.PSObject.Properties[$leaf]) { $current.$leaf = $Value }
    else { $current | Add-Member -NotePropertyName $leaf -NotePropertyValue $Value -Force }
}

function Remove-JsonNestedValue {
    param($Object, [string[]]$Path)
    $current = $Object
    for ($i = 0; $i -lt $Path.Count - 1; $i++) {
        if (-not $current.PSObject.Properties[$Path[$i]]) { return }
        $current = $current.$($Path[$i])
        if ($null -eq $current) { return }
    }
    $leaf = $Path[-1]
    if ($current.PSObject.Properties[$leaf]) {
        $current.PSObject.Properties.Remove($leaf)
    }
}

function Get-JsonNestedValueState {
    param($Object, [string[]]$Path)
    $current = $Object
    for ($i = 0; $i -lt $Path.Count - 1; $i++) {
        if (-not $current.PSObject.Properties[$Path[$i]]) {
            return [PSCustomObject]@{ exists = $false; value = $null }
        }
        $current = $current.$($Path[$i])
        if ($null -eq $current) {
            return [PSCustomObject]@{ exists = $false; value = $null }
        }
    }
    $leaf = $Path[-1]
    if (-not $current.PSObject.Properties[$leaf]) {
        return [PSCustomObject]@{ exists = $false; value = $null }
    }
    return [PSCustomObject]@{ exists = $true; value = $current.$leaf }
}

function Restore-JsonNestedValue {
    param($Object, [string[]]$Path, $State)
    if ($State -and $State.exists -eq $true) {
        Set-JsonNestedValue -Object $Object -Path $Path -Value $State.value
    } else {
        Remove-JsonNestedValue -Object $Object -Path $Path
    }
}

function Set-OperaProfileHardening {
    param($Browser)

    # Opera locks its Preferences file while running; writes fail silently. Detect
    # up front so the caller can return a meaningful "close Opera and retry" error.
    $procName = if ($Browser.Name -eq "Opera GX") { 'opera' } else { 'opera' }
    $running = @(Get-Process -Name $procName -ErrorAction SilentlyContinue)
    if ($running.Count -gt 0) {
        return @{ written = 0; profilesFound = 0; running = $true; errors = @("Opera is running - close it and retry, or the Preferences patch will be ignored.") }
    }

    $written = 0
    $profilesFound = 0
    $errors = @()
    foreach ($dir in (Get-OperaProfileDirs -Browser $Browser)) {
        $prefPaths = @((Join-Path $dir "Preferences"), (Join-Path $dir "Default\Preferences"))
        foreach ($prefPath in ($prefPaths | Select-Object -Unique)) {
            if (-not (Test-Path $prefPath)) { continue }
            $profilesFound++
            try {
                $prefs = Get-Content $prefPath -Raw -ErrorAction Stop | ConvertFrom-Json
                Set-JsonNestedValue -Object $prefs -Path @("winCommander", "hardened") -Value $true
                Set-JsonNestedValue -Object $prefs -Path @("browser", "enable_spellchecking") -Value $false
                Set-JsonNestedValue -Object $prefs -Path @("alternate_error_pages", "enabled") -Value $false
                Set-JsonNestedValue -Object $prefs -Path @("search", "suggest_enabled") -Value $false
                Set-JsonNestedValue -Object $prefs -Path @("net", "network_prediction_options") -Value 2
                Set-JsonNestedValue -Object $prefs -Path @("credentials_enable_service") -Value $false
                Set-JsonNestedValue -Object $prefs -Path @("profile", "password_manager_enabled") -Value $false
                Set-JsonNestedValue -Object $prefs -Path @("safebrowsing", "enabled") -Value $true
                Write-Utf8NoBom -Path $prefPath -Content ($prefs | ConvertTo-Json -Depth 30 -Compress)
                $written++
            } catch {
                $errors += "Failed to patch $prefPath : $($_.Exception.Message)"
            }
        }
    }
    return @{ written = $written; profilesFound = $profilesFound; running = $false; errors = $errors }
}

function Clear-OperaProfileHardening {
    param($Browser)
    foreach ($dir in (Get-OperaProfileDirs -Browser $Browser)) {
        $prefPaths = @((Join-Path $dir "Preferences"), (Join-Path $dir "Default\Preferences"))
        foreach ($prefPath in ($prefPaths | Select-Object -Unique)) {
            try {
                if (-not (Test-Path $prefPath)) { continue }
                $prefs = Get-Content $prefPath -Raw -ErrorAction Stop | ConvertFrom-Json
                Remove-JsonNestedValue -Object $prefs -Path @("winCommander", "hardened")
                Remove-JsonNestedValue -Object $prefs -Path @("browser", "enable_spellchecking")
                Remove-JsonNestedValue -Object $prefs -Path @("alternate_error_pages", "enabled")
                Remove-JsonNestedValue -Object $prefs -Path @("search", "suggest_enabled")
                Remove-JsonNestedValue -Object $prefs -Path @("net", "network_prediction_options")
                Remove-JsonNestedValue -Object $prefs -Path @("credentials_enable_service")
                Remove-JsonNestedValue -Object $prefs -Path @("profile", "password_manager_enabled")
                Write-Utf8NoBom -Path $prefPath -Content ($prefs | ConvertTo-Json -Depth 30 -Compress)
            } catch {}
        }
    }
}

function Test-OperaProfileHardening {
    param($Browser)
    foreach ($dir in (Get-OperaProfileDirs -Browser $Browser)) {
        $prefPaths = @((Join-Path $dir "Preferences"), (Join-Path $dir "Default\Preferences"))
        foreach ($prefPath in ($prefPaths | Select-Object -Unique)) {
            try {
                if (-not (Test-Path $prefPath)) { continue }
                $prefs = Get-Content $prefPath -Raw -ErrorAction Stop | ConvertFrom-Json
                if ($prefs.winCommander -and $prefs.winCommander.hardened -eq $true) { return $true }
            } catch {}
        }
    }
    return $false
}

function Get-BraveProfilePreferencePaths {
    param($Browser)
    if (-not $Browser -or -not $Browser.UserDataDir -or -not (Test-Path -LiteralPath $Browser.UserDataDir)) {
        return @()
    }
    return @(Get-ChildItem -LiteralPath $Browser.UserDataDir -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -in @("Default", "Guest Profile", "System Profile") -or $_.Name -like "Profile *" -or (Test-Path -LiteralPath (Join-Path $_.FullName "Preferences")) } |
        ForEach-Object { Join-Path $_.FullName "Preferences" } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -Unique)
}

function Set-BraveProfileHardening {
    param($Browser)

    # These are real per-profile Brave preferences, not managed policies. Brave
    # can overwrite Preferences on exit, so never patch it while the browser runs.
    if (@(Get-Process -Name "brave" -ErrorAction SilentlyContinue).Count -gt 0) {
        return @{ written = 0; profilesFound = 0; running = $true; errors = @("Brave is running - close it and retry, or the Preferences patch will be ignored.") }
    }

    $written = 0
    $prefPaths = @(Get-BraveProfilePreferencePaths -Browser $Browser)
    $errors = @()
    foreach ($prefPath in $prefPaths) {
        try {
            $prefs = Get-Content -LiteralPath $prefPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
            $settings = [ordered]@{
                p3aEnabled = @("brave", "p3a", "enabled")
                dailyUsagePing = @("brave", "stats", "reporting_enabled")
                diagnosticReports = @("user_experience_metrics", "reporting_enabled")
                aiToolbarButton = @("brave", "ai_chat", "show_toolbar_button")
                rewardsToolbarButton = @("brave", "rewards", "show_brave_rewards_button_in_location_bar")
                walletToolbarButton = @("brave", "wallet", "show_wallet_icon_on_toolbar")
                vpnToolbarButton = @("brave", "brave_vpn", "show_button")
                newTabVpn = @("brave", "new_tab_page", "show_brave_vpn")
                newTabRewards = @("brave", "new_tab_page", "show_rewards")
                newTabTogether = @("brave", "new_tab_page", "show_together")
            }
            if (-not $prefs.winCommander -or -not $prefs.winCommander.braveProfileBackup) {
                $backup = [ordered]@{ version = 1; settings = [ordered]@{} }
                foreach ($setting in $settings.GetEnumerator()) {
                    $backup.settings[$setting.Key] = Get-JsonNestedValueState -Object $prefs -Path $setting.Value
                }
                Set-JsonNestedValue -Object $prefs -Path @("winCommander", "braveProfileBackup") -Value $backup
            }
            # Telemetry settings exposed by brave://settings/privacy.
            Set-JsonNestedValue -Object $prefs -Path @("winCommander", "braveHardened") -Value $true
            Set-JsonNestedValue -Object $prefs -Path @("brave", "p3a", "enabled") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("brave", "stats", "reporting_enabled") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("user_experience_metrics", "reporting_enabled") -Value $false

            # The managed policies below disable these features. Hide their remaining
            # profile-level entry points as well so they are not promoted in the UI.
            Set-JsonNestedValue -Object $prefs -Path @("brave", "ai_chat", "show_toolbar_button") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("brave", "rewards", "show_brave_rewards_button_in_location_bar") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("brave", "wallet", "show_wallet_icon_on_toolbar") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("brave", "brave_vpn", "show_button") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("brave", "new_tab_page", "show_brave_vpn") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("brave", "new_tab_page", "show_rewards") -Value $false
            Set-JsonNestedValue -Object $prefs -Path @("brave", "new_tab_page", "show_together") -Value $false
            Write-Utf8NoBom -Path $prefPath -Content ($prefs | ConvertTo-Json -Depth 30 -Compress)
            $written++
        } catch {
            $errors += "Failed to patch $prefPath : $($_.Exception.Message)"
        }
    }
    return @{ written = $written; profilesFound = $prefPaths.Count; running = $false; errors = $errors }
}

function Clear-BraveProfileHardening {
    param($Browser)

    if (@(Get-Process -Name "brave" -ErrorAction SilentlyContinue).Count -gt 0) {
        return @{ cleared = 0; running = $true; errors = @("Brave is running - close it and retry before restoring its profile settings.") }
    }

    $cleared = 0
    $errors = @()
    foreach ($prefPath in @(Get-BraveProfilePreferencePaths -Browser $Browser)) {
        try {
            $prefs = Get-Content -LiteralPath $prefPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
            if (-not $prefs.winCommander -or $prefs.winCommander.braveHardened -ne $true) { continue }
            $settings = [ordered]@{
                p3aEnabled = @("brave", "p3a", "enabled")
                dailyUsagePing = @("brave", "stats", "reporting_enabled")
                diagnosticReports = @("user_experience_metrics", "reporting_enabled")
                aiToolbarButton = @("brave", "ai_chat", "show_toolbar_button")
                rewardsToolbarButton = @("brave", "rewards", "show_brave_rewards_button_in_location_bar")
                walletToolbarButton = @("brave", "wallet", "show_wallet_icon_on_toolbar")
                vpnToolbarButton = @("brave", "brave_vpn", "show_button")
                newTabVpn = @("brave", "new_tab_page", "show_brave_vpn")
                newTabRewards = @("brave", "new_tab_page", "show_rewards")
                newTabTogether = @("brave", "new_tab_page", "show_together")
            }
            $backup = $prefs.winCommander.braveProfileBackup
            foreach ($setting in $settings.GetEnumerator()) {
                $state = if ($backup -and $backup.settings -and $backup.settings.PSObject.Properties[$setting.Key]) { $backup.settings.PSObject.Properties[$setting.Key].Value } else { $null }
                Restore-JsonNestedValue -Object $prefs -Path $setting.Value -State $state
            }
            Remove-JsonNestedValue -Object $prefs -Path @("winCommander", "braveHardened")
            Remove-JsonNestedValue -Object $prefs -Path @("winCommander", "braveProfileBackup")
            Write-Utf8NoBom -Path $prefPath -Content ($prefs | ConvertTo-Json -Depth 30 -Compress)
            $cleared++
        } catch {
            $errors += "Failed to restore $prefPath : $($_.Exception.Message)"
        }
    }
    return @{ cleared = $cleared; running = $false; errors = $errors }
}

function Install-FirefoxExtensions {
    <#
    .SYNOPSIS Force-install all Firefox extensions and enable private browsing per extension.
    .DESCRIPTION Extensions\Install handles installation; ExtensionSettings per GUID enables
                 private browsing. The "*" wildcard does NOT support private_browsing in Firefox.
    .PARAMETER PolicyPath Registry path e.g. HKLM:\SOFTWARE\Policies\Mozilla\Firefox
    #>
    param(
        [string]$PolicyPath,
        [string]$BrowserName
    )
    $enabledExts = Get-EnabledBrowserExtensions -MasterList $script:FirefoxExtensions -BrowserName $BrowserName
    $disabledExts = Get-DisabledBrowserExtensions -MasterList $script:FirefoxExtensions -BrowserName $BrowserName
    # ── Force-install via Extensions\Install ──
    $extPath = "$PolicyPath\Extensions\Install"
    if (!(Test-Path $extPath)) { New-Item -Path $extPath -Force | Out-Null }
    $knownUrls = @($script:FirefoxExtensions | ForEach-Object { "https://addons.mozilla.org/firefox/downloads/latest/$($_.Slug)/latest.xpi" })
    try {
        $existing = Get-ItemProperty -Path $extPath -ErrorAction SilentlyContinue
        foreach ($prop in $existing.PSObject.Properties) {
            if ($prop.Name -match "^PS" -or $knownUrls -notcontains [string]$prop.Value) { continue }
            Remove-ItemProperty -Path $extPath -Name $prop.Name -Force -ErrorAction SilentlyContinue
        }
    } catch {}
    foreach ($ext in $enabledExts) {
        Set-ItemProperty -Path $extPath -Name "WinCommander-$($ext.Slug)" -Value "https://addons.mozilla.org/firefox/downloads/latest/$($ext.Slug)/latest.xpi" -Type String -Force
    }

    # ── Private browsing via per-GUID ExtensionSettings ──
    # Note: "*" wildcard does NOT support private_browsing — must be set per GUID.
    # Auto-updates are controlled by the ExtensionUpdate DWORD below.
    $settings = [ordered]@{}
    foreach ($ext in $enabledExts) {
        $settings[$ext.Guid] = [ordered]@{ private_browsing = $true }
    }
    if (!(Test-Path $PolicyPath)) { New-Item -Path $PolicyPath -Force | Out-Null }
    Set-ItemProperty -Path $PolicyPath -Name "ExtensionSettings" -Value ($settings | ConvertTo-Json -Compress -Depth 3) -Type String -Force
    # Enable extension auto-updates
    Set-ItemProperty -Path $PolicyPath -Name "ExtensionUpdate" -Value 1 -Type DWord -Force
    Remove-GeckoExtensionProfileDataByGuid -BrowserName $BrowserName -ExtensionGuids @($disabledExts | ForEach-Object { $_.Guid })
}

# ============================================================================
# Universal Browser Detection & Extension Deployment
# ============================================================================

# Map of known browsers: Engine (Chromium|Gecko), registry policy path, exe detection paths
# For Gecko forks without a known registry path, we fall back to distribution/policies.json
$script:BrowserMap = @(
    # ── Chromium-based ──
    # KT: Chromium browsers (Chrome, Edge, Brave) all support per-user installs
    # under %LOCALAPPDATA% — that's the default when the user doesn't have
    # admin rights, and the standard layout on machines like Windows Server
    # where the installer fell back to the user profile. Probing only
    # ProgramFiles{,(x86)} misses that case entirely.
    @{
        Name        = "Google Chrome"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\Google\Chrome"
        UserDataDir = "$env:LOCALAPPDATA\Google\Chrome\User Data"
        ExePaths    = @(
            "$env:ProgramFiles\Google\Chrome\Application\chrome.exe"
            "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe"
            "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe"
        )
    }
    @{
        Name        = "Microsoft Edge"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\Microsoft\Edge"
        UserDataDir = "$env:LOCALAPPDATA\Microsoft\Edge\User Data"
        ExePaths    = @(
            "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
            "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
            "$env:LOCALAPPDATA\Microsoft\Edge\Application\msedge.exe"
        )
    }
    @{
        Name        = "Brave"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\BraveSoftware\Brave"
        UserDataDir = "$env:LOCALAPPDATA\BraveSoftware\Brave-Browser\User Data"
        ExePaths    = @(
            "$env:ProgramFiles\BraveSoftware\Brave-Browser\Application\brave.exe"
            "${env:ProgramFiles(x86)}\BraveSoftware\Brave-Browser\Application\brave.exe"
            "$env:LOCALAPPDATA\BraveSoftware\Brave-Browser\Application\brave.exe"
        )
    }
    @{
        Name        = "Opera"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\Opera Software\Opera"
        UserDataDir = "$env:APPDATA\Opera Software\Opera Stable"
        ExePaths    = @(
            "$env:ProgramFiles\Opera\launcher.exe"
            "$env:ProgramFiles\Opera\opera.exe"
            "${env:ProgramFiles(x86)}\Opera\launcher.exe"
            "${env:ProgramFiles(x86)}\Opera\opera.exe"
            "$env:LOCALAPPDATA\Programs\Opera\launcher.exe"
            "$env:LOCALAPPDATA\Programs\Opera\opera.exe"
        )
    }
    @{
        Name        = "Opera GX"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\Opera Software\Opera GX"
        UserDataDir = "$env:APPDATA\Opera Software\Opera GX Stable"
        ExePaths    = @(
            "$env:ProgramFiles\Opera GX\launcher.exe"
            "$env:ProgramFiles\Opera GX\opera.exe"
            "${env:ProgramFiles(x86)}\Opera GX\launcher.exe"
            "${env:ProgramFiles(x86)}\Opera GX\opera.exe"
            "$env:LOCALAPPDATA\Programs\Opera GX\launcher.exe"
            "$env:LOCALAPPDATA\Programs\Opera GX\opera.exe"
        )
    }
    @{
        Name        = "Vivaldi"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\Vivaldi"
        UserDataDir = "$env:LOCALAPPDATA\Vivaldi\User Data"
        ExePaths    = @(
            "$env:ProgramFiles\Vivaldi\Application\vivaldi.exe"
            "$env:LOCALAPPDATA\Vivaldi\Application\vivaldi.exe"
        )
    }
    @{
        Name        = "Chromium"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\Chromium"
        UserDataDir = "$env:LOCALAPPDATA\Chromium\User Data"
        ExePaths    = @(
            "$env:ProgramFiles\Chromium\Application\chrome.exe"
            "${env:ProgramFiles(x86)}\Chromium\Application\chrome.exe"
        )
    }
    @{
        Name        = "Arc"
        Engine      = "Chromium"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\ArcBrowser\Arc"
        UserDataDir = "$env:LOCALAPPDATA\Arc\User Data"
        ExePaths    = @(
            "$env:LOCALAPPDATA\Arc\Application\arc.exe"
        )
    }
    # ── Gecko / Firefox-based ──
    @{
        Name        = "Firefox"
        Engine      = "Gecko"
        PolicyPath  = "HKLM:\SOFTWARE\Policies\Mozilla\Firefox"
        UserDataDir = "$env:APPDATA\Mozilla\Firefox"
        ExePaths    = @(
            "$env:ProgramFiles\Mozilla Firefox\firefox.exe"
            "${env:ProgramFiles(x86)}\Mozilla Firefox\firefox.exe"
        )
    }
    @{
        Name        = "LibreWolf"
        Engine      = "Gecko"
        PolicyPath  = $null   # uses distribution/policies.json
        UserDataDir = "$env:APPDATA\LibreWolf"
        ExePaths    = @(
            "$env:ProgramFiles\LibreWolf\librewolf.exe"
            "${env:ProgramFiles(x86)}\LibreWolf\librewolf.exe"
            "$env:LOCALAPPDATA\LibreWolf\librewolf.exe"
        )
    }
    @{
        Name        = "Floorp"
        Engine      = "Gecko"
        PolicyPath  = $null
        UserDataDir = "$env:APPDATA\Floorp"
        ExePaths    = @(
            "$env:ProgramFiles\Floorp\floorp.exe"
            "${env:ProgramFiles(x86)}\Floorp\floorp.exe"
        )
    }
    @{
        Name        = "Waterfox"
        Engine      = "Gecko"
        PolicyPath  = $null
        UserDataDir = "$env:APPDATA\Waterfox"
        ExePaths    = @(
            "$env:ProgramFiles\Waterfox\waterfox.exe"
            "${env:ProgramFiles(x86)}\Waterfox\waterfox.exe"
        )
    }
    @{
        Name        = "Zen"
        Engine      = "Gecko"
        PolicyPath  = $null
        UserDataDir = "$env:APPDATA\Zen Browser"
        ExePaths    = @(
            "$env:ProgramFiles\Zen Browser\zen.exe"
            "$env:LOCALAPPDATA\Zen Browser\zen.exe"
            "$env:APPDATA\Zen Browser\zen.exe"
        )
    }
    @{
        Name        = "Mullvad Browser"
        Engine      = "Gecko"
        PolicyPath  = $null
        UserDataDir = "$env:APPDATA\Mullvad Browser"
        ExePaths    = @(
            "$env:ProgramFiles\Mullvad Browser\mullvadbrowser.exe"
        )
    }
    @{
        Name        = "Tor Browser"
        Engine      = "Gecko"
        PolicyPath  = $null
        UserDataDir = "$env:APPDATA\Tor Browser"
        ExePaths    = @(
            "$env:ProgramFiles\Tor Browser\Browser\firefox.exe"
            "$env:USERPROFILE\Desktop\Tor Browser\Browser\firefox.exe"
        )
    }
)

function Get-InstalledBrowsers {
    <#
    .SYNOPSIS Detect all installed browsers on the system.
    .DESCRIPTION Returns browser map entries for browsers whose exe is found on disk.
    #>
    $detected = @()
    foreach ($browser in $script:BrowserMap) {
        foreach ($exePath in $browser.ExePaths) {
            if (Test-Path $exePath) {
                $detected += @{
                    Name        = $browser.Name
                    Engine      = $browser.Engine
                    PolicyPath  = $browser.PolicyPath
                    UserDataDir = $browser.UserDataDir
                    InstallDir  = Split-Path $exePath -Parent
                    ExePath     = $exePath
                }
                break  # found this browser, skip remaining paths
            }
        }
    }
    return $detected
}

function Write-Utf8NoBom {
    # Firefox/LibreWolf's policy engine parses policies.json via JSON.parse, which
    # throws on a leading BOM and aborts the entire policy load silently. PS 5.1's
    # `Set-Content -Encoding UTF8` always emits a BOM, so route every JSON write
    # for browser config through this helper.
    param([string]$Path, [string]$Content)
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Remove-WinCommanderGeckoExtensionPolicyEntries {
    param($Policies)
    if (-not $Policies) { return }
    if ($Policies.ExtensionSettings) {
        foreach ($ext in $script:FirefoxExtensions) {
            $Policies.ExtensionSettings.PSObject.Properties.Remove($ext.Guid)
        }
    }
    if ($Policies.Extensions -and $Policies.Extensions.PSObject.Properties["Install"]) {
        $knownUrls = @($script:FirefoxExtensions | ForEach-Object { "https://addons.mozilla.org/firefox/downloads/latest/$($_.Slug)/latest.xpi" })
        $Policies.Extensions.Install = @($Policies.Extensions.Install | Where-Object { $knownUrls -notcontains [string]$_ })
    }
}

function Write-GeckoPoliciesJson {
    <#
    .SYNOPSIS Write distribution/policies.json for a Gecko browser install dir.
    .DESCRIPTION Uses both Extensions.Install (auto-install prompt) and
                 ExtensionSettings force_installed (silent lock) simultaneously
                 for maximum compatibility across LibreWolf/Firefox fork versions.
    #>
    param(
        [string]$InstallDir,
        [string]$BrowserName
    )
    $distDir = Join-Path $InstallDir "distribution"
    if (!(Test-Path $distDir)) { New-Item -Path $distDir -ItemType Directory -Force | Out-Null }

    $enabledExts = Get-EnabledBrowserExtensions -MasterList $script:FirefoxExtensions -BrowserName $BrowserName
    $disabledExts = Get-DisabledBrowserExtensions -MasterList $script:FirefoxExtensions -BrowserName $BrowserName
    $installUrls = @()
    $extSettingsObj = [ordered]@{}
    foreach ($ext in $enabledExts) {
        $url = "https://addons.mozilla.org/firefox/downloads/latest/$($ext.Slug)/latest.xpi"
        $installUrls += $url
        $extSettingsObj[$ext.Guid] = [ordered]@{
            installation_mode = "force_installed"
            install_url       = $url
            private_browsing  = $true
        }
    }

    $policy = [ordered]@{
        __WinCommanderHardened__ = $true
        policies = [ordered]@{
            ExtensionUpdate   = $true
            Extensions        = [ordered]@{ Install = $installUrls }
            ExtensionSettings = $extSettingsObj
        }
    }

    $jsonPath = Join-Path $distDir "policies.json"
    if (Test-Path $jsonPath) {
        try {
            $existing = Get-Content $jsonPath -Raw | ConvertFrom-Json
            # Write sentinel marker
            if ($existing.PSObject.Properties["__WinCommanderHardened__"]) {
                $existing.__WinCommanderHardened__ = $true
            } else {
                $existing | Add-Member -NotePropertyName "__WinCommanderHardened__" -NotePropertyValue $true -Force
            }
            # Ensure policies object exists
            if (!$existing.policies) { $existing | Add-Member -NotePropertyName "policies" -NotePropertyValue ([PSCustomObject]@{}) -Force }
            # Remove every previous WinCommander extension entry before adding
            # this browser's current set. Otherwise a disabled extension remains
            # force-installed through its stale ExtensionSettings protocol.
            Remove-WinCommanderGeckoExtensionPolicyEntries -Policies $existing.policies

            # Merge Extensions.Install
            if ($existing.policies.PSObject.Properties["Extensions"]) {
                if ($existing.policies.Extensions.PSObject.Properties["Install"]) {
                    $existing.policies.Extensions.Install = @($existing.policies.Extensions.Install) + $installUrls
                } else {
                    $existing.policies.Extensions | Add-Member -NotePropertyName "Install" -NotePropertyValue $installUrls -Force
                }
            } else {
                $existing.policies | Add-Member -NotePropertyName "Extensions" -NotePropertyValue ([PSCustomObject]@{ Install = $installUrls }) -Force
            }

            # Merge force_installed ExtensionSettings per GUID
            $newExtSettings = [PSCustomObject]@{}
            foreach ($ext in $enabledExts) {
                $url = "https://addons.mozilla.org/firefox/downloads/latest/$($ext.Slug)/latest.xpi"
                $newExtSettings | Add-Member -NotePropertyName $ext.Guid -NotePropertyValue ([PSCustomObject]@{
                    installation_mode = "force_installed"
                    install_url       = $url
                    private_browsing  = $true
                }) -Force
            }
            if ($existing.policies.PSObject.Properties["ExtensionSettings"]) {
                foreach ($prop in $newExtSettings.PSObject.Properties) {
                    if ($existing.policies.ExtensionSettings.PSObject.Properties[$prop.Name]) {
                        $existing.policies.ExtensionSettings.$($prop.Name) = $prop.Value
                    } else {
                        $existing.policies.ExtensionSettings | Add-Member -NotePropertyName $prop.Name -NotePropertyValue $prop.Value -Force
                    }
                }
            } else {
                $existing.policies | Add-Member -NotePropertyName "ExtensionSettings" -NotePropertyValue $newExtSettings -Force
            }

            # Ensure ExtensionUpdate
            if ($existing.policies.PSObject.Properties["ExtensionUpdate"]) {
                $existing.policies.ExtensionUpdate = $true
            } else {
                $existing.policies | Add-Member -NotePropertyName "ExtensionUpdate" -NotePropertyValue $true -Force
            }
            Write-Utf8NoBom -Path $jsonPath -Content ($existing | ConvertTo-Json -Depth 10)
        }
        catch {
            # Merge failed (corrupt JSON, schema mismatch, etc). Preserve the original
            # for cleanup and overwrite with a fresh policy so hardening still applies.
            try { Copy-Item -Path $jsonPath -Destination "$jsonPath.bak" -Force -ErrorAction SilentlyContinue } catch {}
            Write-Utf8NoBom -Path $jsonPath -Content ($policy | ConvertTo-Json -Depth 10)
            Write-Warning "Write-GeckoPoliciesJson: merge failed for $jsonPath; restored from policy template (backup at $jsonPath.bak). $($_.Exception.Message)"
        }
    }
    else {
        Write-Utf8NoBom -Path $jsonPath -Content ($policy | ConvertTo-Json -Depth 10)
    }
    Remove-GeckoExtensionProfileDataByGuid -BrowserName $BrowserName -ExtensionGuids @($disabledExts | ForEach-Object { $_.Guid })
}

function Remove-GeckoPoliciesJson {
    <#
    .SYNOPSIS Remove extension entries from distribution/policies.json.
    #>
    param([string]$InstallDir)
    $jsonPath = Join-Path $InstallDir "distribution\policies.json"
    if (Test-Path $jsonPath) {
        try {
            $existing = Get-Content $jsonPath -Raw | ConvertFrom-Json
            if ($existing.policies) {
                # Remove our force_installed entries from ExtensionSettings
                if ($existing.policies.ExtensionSettings) {
                    foreach ($ext in $script:FirefoxExtensions) {
                        $existing.policies.ExtensionSettings.PSObject.Properties.Remove($ext.Guid)
                    }
                }
                # Remove Extensions.Install entries
                if ($existing.policies.Extensions) {
                    $existing.policies.Extensions.PSObject.Properties.Remove("Install")
                }
                # Remove our sentinel marker
                $existing.PSObject.Properties.Remove("__WinCommanderHardened__")
                Write-Utf8NoBom -Path $jsonPath -Content ($existing | ConvertTo-Json -Depth 10)
            }
        }
        catch {
            Remove-ItemSecure $jsonPath -Force -ErrorAction SilentlyContinue
        }
    }
}

function Install-UniversalBrowserExtensions {
    <#
    .SYNOPSIS Auto-detect all installed browsers and force-install the full extension suite.
    .DESCRIPTION Covers every Chromium fork (via registry policy) and every Gecko fork
                 (via registry where known + distribution/policies.json as universal fallback).
    #>
    Assert-IsAdmin
    try {
        $browsers = Get-InstalledBrowsers
        $results = @()

        foreach ($b in $browsers) {
            if ($b.Engine -eq "Chromium") {
                $useLite = $b.Name -in @("Google Chrome", "Opera", "Opera GX")
                $policyPaths = if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") { Get-OperaPolicyPaths -Name $b.Name } else { @($b.PolicyPath) }
                foreach ($policyPath in $policyPaths) {
                    Install-ChromiumExtensions -PolicyPath $policyPath -BrowserName $b.Name -UseLiteUBlock:$useLite
                }
                if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") {
                    Set-OperaProfileHardening -Browser $b
                }
                $extCount = $script:ChromiumExtensionsBase.Count + 1
                $results += "$($b.Name): $extCount extensions via registry"
            }
            elseif ($b.Engine -eq "Gecko") {
                # Registry policy (if path is known)
                if ($b.PolicyPath) {
                    Install-FirefoxExtensions -PolicyPath $b.PolicyPath -BrowserName $b.Name
                }
                # Always write policies.json as universal fallback for Gecko forks
                Write-GeckoPoliciesJson -InstallDir $b.InstallDir -BrowserName $b.Name
                $results += "$($b.Name): $($script:FirefoxExtensions.Count) extensions via " +
                    $(if ($b.PolicyPath) { "registry + policies.json" } else { "policies.json" })
            }
        }

        @{
            status          = "deployed"
            browsersFound   = $browsers.Count
            browsers        = ($browsers | ForEach-Object { $_.Name })
            details         = $results
            extensionCount  = @{
                chromium = $script:ChromiumExtensionsBase.Count + 1
                gecko    = $script:FirefoxExtensions.Count
            }
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Remove-UniversalBrowserExtensions {
    <#
    .SYNOPSIS Remove force-installed extensions from all detected browsers.
    #>
    Assert-IsAdmin
    try {
        $browsers = Get-InstalledBrowsers
        $results = @()

        foreach ($b in $browsers) {
            if ($b.Engine -eq "Chromium") {
                $policyPaths = if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") { Get-OperaPolicyPaths -Name $b.Name } else { Get-ChromiumPolicyPathsForBrowser -Browser $b }
                foreach ($policyPath in $policyPaths) {
                    Remove-BrowserPolicyValueIfPresent -Path $policyPath -Name "ExtensionSettings"
                    Remove-BrowserPolicyPathIfPresent -Path "$policyPath\ExtensionInstallForcelist"
                }
                $cleanup = Remove-ChromiumExtensionProfileData -Browser $b
                if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") {
                    Clear-OperaProfileHardening -Browser $b
                }
                $results += "$($b.Name): extension policy removed, profile items removed=$($cleanup.removed)"
            }
            elseif ($b.Engine -eq "Gecko") {
                if ($b.PolicyPath) {
                    Remove-BrowserPolicyPathIfPresent -Path "$($b.PolicyPath)\Extensions\Install"
                }
                Remove-GeckoPoliciesJson -InstallDir $b.InstallDir
                $cleanup = Remove-GeckoExtensionProfileData -Browser $b
                $results += "$($b.Name): extension policy removed, profile items removed=$($cleanup.removed)"
            }
        }

        @{ status = "removed"; browsersFound = $browsers.Count; details = $results }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Browser Hardening: Firefox
# ============================================================================

function Enable-FirefoxHardening {
    Assert-IsAdmin
    try {
        $policyPath = "HKLM:\SOFTWARE\Policies\Mozilla\Firefox"

        # Telemetry & Data Collection
        Set-RegistryValueSafe -Path $policyPath -Name "DisableTelemetry" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DisablePocket" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DisableFirefoxStudies" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DisableDefaultBrowserAgent" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\Preferences" -Name "datareporting.healthreport.uploadEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\Preferences" -Name "toolkit.telemetry.enabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\Preferences" -Name "network.captive-portal-service.enabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\Preferences" -Name "browser.newtabpage.activity-stream.feeds.topsites" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\Preferences" -Name "browser.newtabpage.activity-stream.showSponsoredTopSites" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\Preferences" -Name "browser.newtabpage.activity-stream.showSponsored" -Value 0 -Type DWord

        # Content blocking: strict
        Set-RegistryValueSafe -Path "$policyPath\EnableTrackingProtection" -Name "Value" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\EnableTrackingProtection" -Name "Cryptomining" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\EnableTrackingProtection" -Name "Fingerprinting" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path "$policyPath\EnableTrackingProtection" -Name "EmailTracking" -Value 1 -Type DWord

        Install-FirefoxExtensions -PolicyPath $policyPath -BrowserName "Firefox"

        @{ status = "hardened" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-FirefoxHardening {
    Assert-IsAdmin
    try {
        $profileRemoved = 0
        $profileErrors = @()
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Policies\Mozilla\Firefox" -Recurse -Force -ErrorAction SilentlyContinue
        Get-InstalledBrowsers |
            Where-Object { $_.Name -eq "Firefox" -or ($_.Engine -eq "Gecko" -and $_.PolicyPath -eq "HKLM:\SOFTWARE\Policies\Mozilla\Firefox") } |
            ForEach-Object {
                Remove-GeckoPoliciesJson -InstallDir $_.InstallDir
                $cleanup = Remove-GeckoExtensionProfileData -Browser $_
                $profileRemoved += [int]$cleanup.removed
                $profileErrors += @($cleanup.errors)
            }
        @{ status = "reset"; profileItemsRemoved = $profileRemoved; profileCleanupErrors = $profileErrors }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Browser Hardening: Brave
# ============================================================================

function Enable-BraveHardening {
    Assert-IsAdmin
    try {
        $policyPath = "HKLM:\SOFTWARE\Policies\BraveSoftware\Brave"
        $browser = (Get-InstalledBrowsers | Where-Object { $_.Name -eq "Brave" } | Select-Object -First 1)
        $profileResult = @{ written = 0; profilesFound = 0; errors = @() }
        if ($browser) {
            $profileResult = Set-BraveProfileHardening -Browser $browser
            if ($profileResult.running) {
                return @{ error = $true; message = "Close Brave before hardening - Brave overwrites Preferences on exit, so the privacy patch would be lost." }
            }
            if ($profileResult.profilesFound -eq 0) {
                return @{ error = $true; message = "No Brave profile found. Launch Brave at least once before hardening, then retry." }
            }
            if ($profileResult.errors.Count -gt 0 -and $profileResult.written -eq 0) {
                return @{ error = $true; message = "Brave Preferences patch failed: $($profileResult.errors -join '; ')" }
            }
        }

        # MetricsReportingEnabled is ignored on unmanaged installs but remains our
        # Chromium hardened-state sentinel. The actual P3A, daily usage ping, and
        # diagnostic-reporting switches are patched above in every Brave profile.
        Set-RegistryValueSafe -Path $policyPath -Name "MetricsReportingEnabled" -Value 0 -Type DWord
        # Sync intentionally left enabled: Brave Sync is end-to-end encrypted via a local
        # sync-chain code (no Brave account / Google servers), so it's not a privacy concern.
        # Ads & sponsored content
        Set-RegistryValueSafe -Path $policyPath -Name "BraveRewardsDisabled" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "BraveWalletDisabled" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "BraveVPNDisabled" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "BraveAIChatEnabled" -Value 0 -Type DWord
        # Safe Browsing → standard
        Set-RegistryValueSafe -Path $policyPath -Name "SafeBrowsingProtectionLevel" -Value 1 -Type DWord

        # Privacy (HTTP left enabled for local/LAN sites e.g. routers — no HttpsOnlyMode)
        Set-RegistryValueSafe -Path $policyPath -Name "BlockThirdPartyCookies" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DnsOverHttpsMode" -Value "automatic" -Type String
        Set-RegistryValueSafe -Path $policyPath -Name "PasswordLeakDetectionEnabled" -Value 1 -Type DWord

        Install-ChromiumExtensions -PolicyPath $policyPath -BrowserName "Brave"

        @{ status = "hardened"; profilePreferencesPatched = $profileResult.written }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-BraveHardening {
    Assert-IsAdmin
    try {
        $browser = (Get-InstalledBrowsers | Where-Object { $_.Name -eq "Brave" } | Select-Object -First 1)
        $cleanup = @{ removed = 0; errors = @() }
        $profileResult = @{ cleared = 0; errors = @() }
        if ($browser) {
            $profileResult = Clear-BraveProfileHardening -Browser $browser
            if ($profileResult.running) {
                return @{ error = $true; message = "Close Brave before restoring its hardening settings - Brave overwrites Preferences on exit." }
            }
            if ($profileResult.errors.Count -gt 0) {
                return @{ error = $true; message = "Brave Preferences restore failed: $($profileResult.errors -join '; ')" }
            }
            $cleanup = Reset-ChromiumBrowserPolicy -Browser $browser
        }
        else {
            Remove-BrowserPolicyPathIfPresent -Path "HKLM:\SOFTWARE\Policies\BraveSoftware\Brave"
            Remove-BrowserPolicyPathIfPresent -Path "HKCU:\SOFTWARE\Policies\BraveSoftware\Brave"
        }
        @{ status = "reset"; profilePreferencesRestored = $profileResult.cleared; profileItemsRemoved = $cleanup.removed; profileCleanupErrors = $cleanup.errors }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Copilot / AI Control
# ============================================================================

function Remove-CopilotAIComponents {
    Assert-IsAdmin
    try {
        $removed = @()
        $warnings = @()

        # Reuse the comprehensive policy helpers shared by the granular
        # Security & Apps controls. This keeps the existing toggle as the
        # canonical core switch instead of creating a second AI-settings path.
        try { Set-AIControlPolicies -Mode apply | Out-Null; $removed += "Windows and app AI policies disabled" } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlRegionPolicy -Mode apply | Out-Null; $removed += "Integrated service policies disabled" } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlSettingsVisibility -Mode apply | Out-Null; $removed += "AI settings pages hidden" } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlNotepadRewrite -Mode apply | Out-Null; $removed += "Notepad Rewrite disabled" } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlAppFeatures -Mode apply | Out-Null; $removed += "Photos, app actions, voice effects, and Gaming Copilot disabled" } catch { $warnings += $_.Exception.Message }

        # Remove the directly branded Copilot packages here. The existing
        # Packages & Apps item intentionally stays bounded; the full AIX/CoreAI
        # workload removal remains an explicit advanced action.
        $copilotPackages = @(
            "Microsoft.Windows.Ai.Copilot.Provider",
            "Microsoft.Copilot",
            "Microsoft.Windows.Copilot"
        )
        foreach ($pkg in $copilotPackages) {
            $found = Get-AppxPackage -Name $pkg -AllUsers -ErrorAction SilentlyContinue
            if ($found) {
                $found | Remove-AppxPackage -AllUsers -ErrorAction SilentlyContinue
                $removed += "Removed APPX: $pkg"
            }
            # Deprovision
            Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue |
                Where-Object { $_.PackageName -like "$pkg*" } |
                ForEach-Object {
                    Remove-AppxProvisionedPackage -Online -PackageName $_.PackageName -ErrorAction SilentlyContinue | Out-Null
                    $removed += "Deprovisioned: $pkg"
                }
            # Add to deprovisioned registry
            $deprovPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned\${pkg}_cw5n1h2txyewy"
            if (!(Test-Path $deprovPath)) { New-Item -Path $deprovPath -Force | Out-Null }
        }

        # Keep the existing executable block as defence in depth.
        $aiExes = @("CopilotRuntime.exe", "AIXHost.exe")
        foreach ($exe in $aiExes) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$exe"
            if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
            Set-ItemProperty -Path $path -Name "Debugger" -Value "%SystemRoot%\System32\systray.exe" -Type String -Force
            $removed += "IFEO blocked: $exe"
        }

        @{ status = if ($warnings.Count) { "partial" } else { "removed" }; actions = $removed; warnings = $warnings; count = $removed.Count }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Restore-CopilotAIComponents {
    Assert-IsAdmin
    try {
        $warnings = @()
        try { Set-AIControlPolicies -Mode revert | Out-Null } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlRegionPolicy -Mode revert | Out-Null } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlSettingsVisibility -Mode revert | Out-Null } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlNotepadRewrite -Mode revert | Out-Null } catch { $warnings += $_.Exception.Message }
        try { Set-AIControlAppFeatures -Mode revert | Out-Null } catch { $warnings += $_.Exception.Message }
        $aiExes = @("CopilotRuntime.exe", "AIXHost.exe")
        foreach ($exe in $aiExes) {
            $path = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$exe"
            Remove-ItemSecure -Path $path -Name "Debugger" -ErrorAction SilentlyContinue
        }
        @{ status = if ($warnings.Count) { "partial" } else { "restored" }; warnings = $warnings }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# HOST HARDENING: System Restore / VSS
# Disabling VSS removes shadow copies — a recover-from-attack safety net.
# reducesSecurity:true — warn dialog required before applying.
# ============================================================================

function Disable-SystemRestore {
    Assert-IsAdmin
    try {
        Disable-ComputerRestore -Drive "C:\" -ErrorAction SilentlyContinue
        Set-Service -Name VSS -StartupType Disabled -ErrorAction SilentlyContinue
        @{ status = "disabled"; requiresReboot = $false }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-SystemRestore {
    Assert-IsAdmin
    try {
        Enable-ComputerRestore -Drive "C:\" -ErrorAction SilentlyContinue
        Set-Service -Name VSS -StartupType Manual -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# HOST HARDENING: Require Password on Resume / No-sleep policy
# Sets the lock-screen-on-resume screensaver policy and disables Sleep in
# the power plan (AC path). Sleep is blocked via powercfg SubGroup SLEEP.
# ============================================================================

function Disable-SleepPassword {
    # KT: "Disable" = APPLY the policy (require PW + no sleep). The toggle id
    # is requirePwOnResume and its "disable" direction undoes the hardening.
    Assert-IsAdmin
    try {
        $scrPath = "HKCU:\Control Panel\Desktop"
        Set-ItemProperty -Path $scrPath -Name "ScreenSaverIsSecure" -Value "1" -Type String -Force
        # Prevent sleep on AC power: SubGroup 238C9FA8 (SLEEP), Setting 29F6C1DB (StandbyTimeout)
        powercfg /SETACVALUEINDEX SCHEME_CURRENT 238C9FA8-76B7-43A9-B3DC-10C6CE293B2A 29F6C1DB-FAF8-4A9B-B53E-B5B5B8BEA5FC 0 2>$null
        powercfg /SETACTIVE SCHEME_CURRENT 2>$null
        @{ status = "requirePwEnabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-SleepPassword {
    # KT: "Enable" here = UNDO the hardening (restore default sleep/PW policy).
    Assert-IsAdmin
    try {
        $scrPath = "HKCU:\Control Panel\Desktop"
        Set-ItemProperty -Path $scrPath -Name "ScreenSaverIsSecure" -Value "0" -Type String -Force
        # Restore default AC sleep timeout (Windows default = 1800 seconds / 30 minutes)
        powercfg /SETACVALUEINDEX SCHEME_CURRENT 238C9FA8-76B7-43A9-B3DC-10C6CE293B2A 29F6C1DB-FAF8-4A9B-B53E-B5B5B8BEA5FC 1800 2>$null
        powercfg /SETACTIVE SCHEME_CURRENT 2>$null
        @{ status = "requirePwDisabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# HOST HARDENING: Kernel DMA Protection
# Read-only probe — Kernel DMA Protection requires firmware IOMMU support
# and is enabled at boot by the firmware/BIOS. Windows cannot force-enable
# it at runtime. This function reports current status and attempts to set the
# registry preference (effective on next boot for supporting hardware only).
# NEVER reports "enabled" unless the CIM instance confirms it is active.
# ============================================================================

function Enable-KernelDMAProtection {
    Assert-IsAdmin
    try {
        # Attempt to set the policy preference (requires firmware IOMMU; no-op otherwise)
        $path = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Kernel DMA Protection"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "DeviceEnumerationPolicy" -Value 0 -Type DWord -Force

        # Read actual hardware status — the only authoritative source
        $dg = Get-CimInstance -ClassName Win32_DeviceGuard -Namespace root\Microsoft\Windows\DeviceGuard -ErrorAction SilentlyContinue
        $actuallyEnabled = ($dg -and ($dg.KernelDMAProtection -eq 2))
        @{
            status         = if ($actuallyEnabled) { "enabled" } else { "preference_set_reboot_needed" }
            actuallyActive = $actuallyEnabled
            note           = "Kernel DMA Protection requires firmware IOMMU. Policy preference set; reboot required on supported hardware."
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-KernelDMAProtection {
    Assert-IsAdmin
    try {
        Remove-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Kernel DMA Protection" -Name "DeviceEnumerationPolicy" -ErrorAction SilentlyContinue
        $dg = Get-CimInstance -ClassName Win32_DeviceGuard -Namespace root\Microsoft\Windows\DeviceGuard -ErrorAction SilentlyContinue
        $actuallyEnabled = ($dg -and ($dg.KernelDMAProtection -eq 2))
        @{ status = "preference_cleared"; actuallyActive = $actuallyEnabled }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-KernelDMAProtectionStatus {
    try {
        $dg = Get-CimInstance -ClassName Win32_DeviceGuard -Namespace root\Microsoft\Windows\DeviceGuard -ErrorAction SilentlyContinue
        $active = ($dg -and ($dg.KernelDMAProtection -eq 2))
        @{ enabled = $active; firmwareIommuRequired = $true }
    }
    catch { @{ enabled = $false; firmwareIommuRequired = $true } }
}

# ============================================================================
# HOST HARDENING: Exploit Protection (DEP / ASLR / CFG / SEHOP / Heap)
# Wraps Set-ProcessMitigation -System — each function flips one system-wide
# mitigation independently. Settings apply system-wide; already-running
# processes need a reboot to fully pick up the change (requiresReboot=$true).
# ============================================================================

function Enable-SystemDEP {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Enable DEP -ErrorAction Stop | Out-Null
        @{ status = "enabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-SystemDEP {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Disable DEP -ErrorAction Stop | Out-Null
        @{ status = "disabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-MandatoryASLR {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Enable ForceRelocateImages -ErrorAction Stop | Out-Null
        @{ status = "enabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-MandatoryASLR {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Disable ForceRelocateImages -ErrorAction Stop | Out-Null
        @{ status = "disabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-BottomUpASLR {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Enable BottomUp,HighEntropy -ErrorAction Stop | Out-Null
        @{ status = "enabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-BottomUpASLR {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Disable BottomUp,HighEntropy -ErrorAction Stop | Out-Null
        @{ status = "disabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-SystemCFG {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Enable CFG -ErrorAction Stop | Out-Null
        @{ status = "enabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-SystemCFG {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Disable CFG -ErrorAction Stop | Out-Null
        @{ status = "disabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-HeapIntegrity {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Enable TerminateOnError -ErrorAction Stop | Out-Null
        @{ status = "enabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-HeapIntegrity {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Disable TerminateOnError -ErrorAction Stop | Out-Null
        @{ status = "disabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-SEHOP {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Enable SEHOP -ErrorAction Stop | Out-Null
        @{ status = "enabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-SEHOP {
    Assert-IsAdmin
    try {
        Set-ProcessMitigation -System -Disable SEHOP -ErrorAction Stop | Out-Null
        @{ status = "disabled"; requiresReboot = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-ExploitProtectionStatus {
    # Get-ProcessMitigation -System property casing varies by OS build (Dep/DEP,
    # Aslr/ASLR, etc.) but PowerShell member access is case-insensitive, so the
    # real risk is a missing/renamed property or $m being $null — guard with
    # the try/catch below rather than the casing itself.
    function _mitOn($v) { return ($null -ne $v) -and ("$v" -match '^(?i)on$') }
    try {
        $m = Get-ProcessMitigation -System -ErrorAction SilentlyContinue
        @{
            depEnabled    = _mitOn $m.Dep.Enable
            aslrMandatory = _mitOn $m.Aslr.ForceRelocateImages
            aslrBottomUp  = _mitOn $m.Aslr.BottomUp
            cfgEnabled    = _mitOn $m.Cfg.Enable
            heapIntegrity = _mitOn $m.Heap.TerminateOnError
            sehopEnabled  = _mitOn $m.Sehop.Enable
        }
    }
    catch {
        @{
            depEnabled    = $false
            aslrMandatory = $false
            aslrBottomUp  = $false
            cfgEnabled    = $false
            heapIntegrity = $false
            sehopEnabled  = $false
        }
    }
}

# ============================================================================
# HOST HARDENING: Defender Exploit Guard (ASR / Controlled Folder Access / Network Protection)
# Wraps Set-MpPreference — each function flips one Defender-native exploit-
# guard control independently. These cmdlets require the Defender engine to
# be present and running; if Defender is disabled/removed they fail and the
# try/catch below returns the error envelope instead of throwing.
# ============================================================================

function Enable-ASRRules {
    Assert-IsAdmin
    try {
        $ids = @(
            '9E6C4E1F-7D60-472F-BA1A-A39EF669E4B2', # Block credential theft from LSASS
            'D4F940AB-401B-4EFC-AADC-AD5F3C50688A', # Block Office apps from creating child processes
            '3B576869-A4EC-4529-8536-B80A7769E899', # Block Office apps from creating executable content
            '75668C1F-73B5-4CF0-BB93-3ECF5CB7CC84', # Block Office apps from injecting code into other processes
            '5BEB7EFE-FD9A-4556-801D-275E5FFC04CC', # Block execution of potentially obfuscated scripts
            'D3E037E1-3EB8-44C8-A917-57927947596D', # Block JS/VBS from launching downloaded executable content
            'BE9BA2D9-53EA-4CDC-84E5-9B1EEEE46550', # Block executable content from email client and webmail
            'E6DB77E5-3DF2-4CF1-B95A-636979351E5B', # Block persistence through WMI event subscription
            'D1E49AAC-8F56-4280-B9BA-993A6D77406C', # Block process creations from PSExec/WMI commands
            'B2B3F03D-6A65-4F7B-A9C7-1C7EF74A9BA4'  # Block untrusted/unsigned processes from USB
        )
        Set-MpPreference -AttackSurfaceReductionRules_Ids $ids -AttackSurfaceReductionRules_Actions ($ids | ForEach-Object { 'Enabled' }) -ErrorAction Stop
        @{ status = "enabled"; ruleCount = $ids.Count }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-ASRRules {
    Assert-IsAdmin
    try {
        $ids = @(
            '9E6C4E1F-7D60-472F-BA1A-A39EF669E4B2', # Block credential theft from LSASS
            'D4F940AB-401B-4EFC-AADC-AD5F3C50688A', # Block Office apps from creating child processes
            '3B576869-A4EC-4529-8536-B80A7769E899', # Block Office apps from creating executable content
            '75668C1F-73B5-4CF0-BB93-3ECF5CB7CC84', # Block Office apps from injecting code into other processes
            '5BEB7EFE-FD9A-4556-801D-275E5FFC04CC', # Block execution of potentially obfuscated scripts
            'D3E037E1-3EB8-44C8-A917-57927947596D', # Block JS/VBS from launching downloaded executable content
            'BE9BA2D9-53EA-4CDC-84E5-9B1EEEE46550', # Block executable content from email client and webmail
            'E6DB77E5-3DF2-4CF1-B95A-636979351E5B', # Block persistence through WMI event subscription
            'D1E49AAC-8F56-4280-B9BA-993A6D77406C', # Block process creations from PSExec/WMI commands
            'B2B3F03D-6A65-4F7B-A9C7-1C7EF74A9BA4'  # Block untrusted/unsigned processes from USB
        )
        Set-MpPreference -AttackSurfaceReductionRules_Ids $ids -AttackSurfaceReductionRules_Actions ($ids | ForEach-Object { 'Disabled' }) -ErrorAction Stop
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-ControlledFolderAccess {
    Assert-IsAdmin
    try {
        Set-MpPreference -EnableControlledFolderAccess Enabled -ErrorAction Stop
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-ControlledFolderAccess {
    Assert-IsAdmin
    try {
        Set-MpPreference -EnableControlledFolderAccess Disabled -ErrorAction Stop
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-NetworkProtection {
    Assert-IsAdmin
    try {
        Set-MpPreference -EnableNetworkProtection Enabled -ErrorAction Stop
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-NetworkProtection {
    Assert-IsAdmin
    try {
        Set-MpPreference -EnableNetworkProtection Disabled -ErrorAction Stop
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Browser Hardening: Chrome
# ============================================================================

function Enable-ChromeHardening {
    Assert-IsAdmin
    try {
        $policyPath = "HKLM:\SOFTWARE\Policies\Google\Chrome"

        # Telemetry & data collection
        Set-RegistryValueSafe -Path $policyPath -Name "MetricsReportingEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "SafeBrowsingProtectionLevel" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "UrlKeyedAnonymizedDataCollectionEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "SpellCheckServiceEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "AlternateErrorPagesEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "NetworkPredictionOptions" -Value 2 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "SearchSuggestEnabled" -Value 0 -Type DWord
        # Disable Chrome's covered generative-AI features (2 = disabled).
        Set-RegistryValueSafe -Path $policyPath -Name "GenAiDefaultSettings" -Value 2 -Type DWord

        # Sync & sign-in
        Set-RegistryValueSafe -Path $policyPath -Name "SyncDisabled" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "BrowserSignin" -Value 0 -Type DWord

        # Sponsored content
        Set-RegistryValueSafe -Path $policyPath -Name "NTPCardsVisible" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "PromotionalTabsEnabled" -Value 0 -Type DWord

        # Privacy Sandbox
        Set-RegistryValueSafe -Path $policyPath -Name "PrivacySandboxPromptEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "PrivacySandboxAdTopicsEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "PrivacySandboxSiteEnabledAdsEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "PrivacySandboxAdMeasurementEnabled" -Value 0 -Type DWord

        # Privacy & debloat (HTTP left enabled for local/LAN sites e.g. routers — no HttpsOnlyMode)
        Set-RegistryValueSafe -Path $policyPath -Name "BlockThirdPartyCookies" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DnsOverHttpsMode" -Value "automatic" -Type String
        Set-RegistryValueSafe -Path $policyPath -Name "PasswordLeakDetectionEnabled" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "PromotionsEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "ShoppingListEnabled" -Value 0 -Type DWord

        Install-ChromiumExtensions -PolicyPath $policyPath -BrowserName "Google Chrome" -UseLiteUBlock

        @{ status = "hardened" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-ChromeHardening {
    Assert-IsAdmin
    try {
        $browser = (Get-InstalledBrowsers | Where-Object { $_.Name -eq "Google Chrome" } | Select-Object -First 1)
        $cleanup = @{ removed = 0; errors = @() }
        if ($browser) { $cleanup = Reset-ChromiumBrowserPolicy -Browser $browser }
        else {
            Remove-BrowserPolicyPathIfPresent -Path "HKLM:\SOFTWARE\Policies\Google\Chrome"
            Remove-BrowserPolicyPathIfPresent -Path "HKCU:\SOFTWARE\Policies\Google\Chrome"
        }
        @{ status = "reset"; profileItemsRemoved = $cleanup.removed; profileCleanupErrors = $cleanup.errors }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Browser Hardening: Edge
# ============================================================================

function Enable-EdgeHardening {
    Assert-IsAdmin
    try {
        $policyPath = "HKLM:\SOFTWARE\Policies\Microsoft\Edge"

        # Telemetry & data collection
        Set-RegistryValueSafe -Path $policyPath -Name "MetricsReportingEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DiagnosticData" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "PersonalizationReportingEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "SendSiteInfoToImproveServices" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "AlternateErrorPagesEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "NetworkPredictionOptions" -Value 2 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "SearchSuggestEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "SpellCheckServiceEnabled" -Value 0 -Type DWord

        # Safe Browsing → standard
        Set-RegistryValueSafe -Path $policyPath -Name "SmartScreenEnabled" -Value 1 -Type DWord

        # Edge-specific bloat
        Set-RegistryValueSafe -Path $policyPath -Name "EdgeShoppingAssistantEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "EdgeCollectionsEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "HubsSidebarEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "EdgeFollowEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "MicrosoftEdgeInsiderPromotionEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "ShowMicrosoftRewards" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "ConfigureDoNotTrack" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "BingAdsSuppression" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "CredentialProviderPromoEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DefaultBrowserSettingsCampaignEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "EdgeWalletCheckoutEnabled" -Value 0 -Type DWord

        # Disable Copilot and current AI surfaces. CopilotCDPPageContext and
        # DiscoverPageContextEnabled are retained for older Edge builds; the
        # remaining policies cover their supported replacements in Edge 150+.
        Set-RegistryValueSafe -Path $policyPath -Name "CopilotCDPPageContext" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DiscoverPageContextEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "CopilotPageContext" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "EdgeEntraCopilotPageContext" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "Microsoft365CopilotChatIconEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "AllowBrowsingWithCopilot" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "CopilotAddressBarSuggestionsEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "CopilotNewTabPageEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "ComposeInlineEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "BuiltInAIAPIsEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "AIGenThemesEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "EdgeHistoryAISearchEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "HideInternetExplorerRedirectUXForIncompatibleSitesEnabled" -Value 1 -Type DWord

        # Sync
        Set-RegistryValueSafe -Path $policyPath -Name "SyncDisabled" -Value 1 -Type DWord

        # New Tab Page
        Set-RegistryValueSafe -Path $policyPath -Name "NewTabPageContentEnabled" -Value 0 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "NewTabPageQuickLinksEnabled" -Value 0 -Type DWord

        # Privacy (HTTP left enabled for local/LAN sites e.g. routers — no HttpsOnlyMode)
        Set-RegistryValueSafe -Path $policyPath -Name "BlockThirdPartyCookies" -Value 1 -Type DWord
        Set-RegistryValueSafe -Path $policyPath -Name "DnsOverHttpsMode" -Value "automatic" -Type String

        Install-ChromiumExtensions -PolicyPath $policyPath -BrowserName "Microsoft Edge"

        @{ status = "hardened" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-EdgeHardening {
    Assert-IsAdmin
    try {
        $browser = (Get-InstalledBrowsers | Where-Object { $_.Name -eq "Microsoft Edge" } | Select-Object -First 1)
        $cleanup = @{ removed = 0; errors = @() }
        if ($browser) { $cleanup = Reset-ChromiumBrowserPolicy -Browser $browser }
        else {
            Remove-BrowserPolicyPathIfPresent -Path "HKLM:\SOFTWARE\Policies\Microsoft\Edge"
            Remove-BrowserPolicyPathIfPresent -Path "HKCU:\SOFTWARE\Policies\Microsoft\Edge"
        }
        @{ status = "reset"; profileItemsRemoved = $cleanup.removed; profileCleanupErrors = $cleanup.errors }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Dynamic Browser Hardening: by name (dispatches to specific or generic handler)
# ============================================================================

function Get-BrowserHardenedStatus {
    <#
    .SYNOPSIS Detect whether a browser is currently hardened.
    .DESCRIPTION For Chromium: checks if MetricsReportingEnabled = 0 is set under the policy key.
                 For Gecko: checks if DisableTelemetry = 1 is set under the policy key,
                 or if distribution/policies.json contains our extension list.
    #>
    param(
        [string]$Engine,
        [string]$PolicyPath,
        [string]$InstallDir,
        $Browser = $null
    )
    try {
        if ($Engine -eq "Chromium") {
            $paths = @($PolicyPath)
            if ($Browser -and ($Browser.Name -eq "Opera" -or $Browser.Name -eq "Opera GX")) {
                if (Test-OperaProfileHardening -Browser $Browser) { return $true }
                $paths = Get-OperaPolicyPaths -Name $Browser.Name
            } elseif ($Browser) {
                $paths = Get-ChromiumPolicyPathsForBrowser -Browser $Browser
            }
            foreach ($path in $paths) {
                if (!$path -or !(Test-Path $path)) { continue }
                $props = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue
                if ($props -eq $null) { continue }
                # Primary check: MetricsReportingEnabled = 0
                if ($props.MetricsReportingEnabled -ne $null -and $props.MetricsReportingEnabled -eq 0) { return $true }
                # Fallback: ExtensionSettings present means our hardening policy was applied
                if (Test-WinCommanderChromiumExtensionPolicy -Props $props) { return $true }
                # Fallback: legacy extension force-list contains one of our known extension IDs.
                if (Test-WinCommanderChromiumForcelistPolicy -PolicyPath $path) { return $true }
            }
            return $false
        } elseif ($Engine -eq "Gecko") {
            # Check registry policy first
            if ($PolicyPath -and (Test-Path $PolicyPath)) {
                $val = Get-ItemProperty -Path $PolicyPath -Name "DisableTelemetry" -ErrorAction SilentlyContinue
                if ($val -ne $null -and $val.DisableTelemetry -eq 1) { return $true }
            }
            # Check for our sentinel marker in policies.json (written by Write-GeckoPoliciesJson)
            $jsonPath = Join-Path $InstallDir "distribution\policies.json"
            if (Test-Path $jsonPath) {
                try {
                    $obj = Get-Content $jsonPath -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json
                    if ($obj.__WinCommanderHardened__ -eq $true) { return $true }
                } catch {}
            }
            return $false
        }
    } catch { return $false }
    return $false
}

function Get-InstalledBrowsersJson {
    <#
    .SYNOPSIS Return installed browser list with real hardening status.
    .DESCRIPTION Each entry includes Hardened = $true/$false based on live system state.
    #>
    $browsers = Get-InstalledBrowsers
    $result = @()
    foreach ($b in $browsers) {
        $hardened = Get-BrowserHardenedStatus -Engine $b.Engine -PolicyPath $b.PolicyPath -InstallDir $b.InstallDir -Browser $b
        $result += @{
            Name        = $b.Name
            Engine      = $b.Engine
            PolicyPath  = if ($b.PolicyPath) { $b.PolicyPath } else { "" }
            InstallDir  = $b.InstallDir
            ExePath     = $b.ExePath
            Hardened    = $hardened
        }
    }
    @{ browsers = $result; count = $result.Count }
}

function Enable-HardenBrowserByName {
    <#
    .SYNOPSIS Harden a specific browser by name.
    .DESCRIPTION Dispatches to a browser-specific hardening function for known browsers,
                 or applies generic Chromium/Gecko hardening for others.
    .PARAMETER Name Browser name as returned by Get-InstalledBrowsers.
    #>
    param([string]$Name)
    Assert-IsAdmin
    try {
        # Named browsers get dedicated functions with full policy coverage
        switch ($Name) {
            "Firefox"         { return Enable-FirefoxHardening }
            "Brave"           { return Enable-BraveHardening }
            "Google Chrome"   { return Enable-ChromeHardening }
            "Microsoft Edge"  { return Enable-EdgeHardening }
        }

        # For all other browsers: look up engine and apply generic hardening
        $browsers = Get-InstalledBrowsers | Where-Object { $_.Name -eq $Name }
        if (-not $browsers -or @($browsers).Count -eq 0) {
            return @{ error = $true; message = "Browser not found on system: $Name" }
        }
        $b = @($browsers)[0]

        if ($b.Engine -eq "Chromium") {
            # Generic Chromium: extensions + core telemetry/sync policies
            $policyPaths = if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") { Get-OperaPolicyPaths -Name $b.Name } else { @($b.PolicyPath) }
            $useLite = $b.Name -in @("Google Chrome", "Opera", "Opera GX")
            foreach ($policyPath in $policyPaths) {
                if ($policyPath) {
                    Install-ChromiumExtensions -PolicyPath $policyPath -BrowserName $b.Name -UseLiteUBlock:$useLite
                    Set-RegistryValueSafe -Path $policyPath -Name "MetricsReportingEnabled"    -Value 0 -Type DWord
                    # SyncDisabled intentionally NOT set here: generic Chromium browsers
                    # (Vivaldi, Arc, Opera) own their sync accounts and users opt into it.
                    Set-RegistryValueSafe -Path $policyPath -Name "SafeBrowsingProtectionLevel"-Value 1 -Type DWord
                    Set-RegistryValueSafe -Path $policyPath -Name "NetworkPredictionOptions"   -Value 2 -Type DWord
                    Set-RegistryValueSafe -Path $policyPath -Name "SearchSuggestEnabled"       -Value 0 -Type DWord
                    Set-RegistryValueSafe -Path $policyPath -Name "SpellCheckServiceEnabled"   -Value 0 -Type DWord
                    # Privacy hardening (HTTP left enabled for local/LAN sites e.g. routers)
                    Set-RegistryValueSafe -Path $policyPath -Name "BlockThirdPartyCookies"     -Value 1 -Type DWord
                    Set-RegistryValueSafe -Path $policyPath -Name "DnsOverHttpsMode"           -Value "automatic" -Type String
                    Set-RegistryValueSafe -Path $policyPath -Name "PasswordLeakDetectionEnabled" -Value 1 -Type DWord
                }
            }
            if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") {
                $opResult = Set-OperaProfileHardening -Browser $b
                if ($opResult.running) {
                    return @{ error = $true; message = "Close Opera before hardening - Opera locks its Preferences file while running, so the patch would be silently ignored." }
                }
                if ($opResult.profilesFound -eq 0) {
                    return @{ error = $true; message = "No Opera profile found. Launch Opera at least once before hardening, then retry." }
                }
                if ($opResult.errors.Count -gt 0 -and $opResult.written -eq 0) {
                    return @{ error = $true; message = "Opera Preferences patch failed: $($opResult.errors -join '; ')" }
                }
            }
        } elseif ($b.Engine -eq "Gecko") {
            # Generic Gecko: registry policy (if path known) + policies.json
            if ($b.PolicyPath) {
                Install-FirefoxExtensions -PolicyPath $b.PolicyPath -BrowserName $b.Name
            }
            Write-GeckoPoliciesJson -InstallDir $b.InstallDir -BrowserName $b.Name
        }

        @{ status = "hardened"; browser = $Name; engine = $b.Engine }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-HardenBrowserByName {
    <#
    .SYNOPSIS Restore a specific browser's hardening by name.
    .PARAMETER Name Browser name as returned by Get-InstalledBrowsers.
    #>
    param([string]$Name)
    Assert-IsAdmin
    try {
        switch ($Name) {
            "Firefox"         { return Disable-FirefoxHardening }
            "Brave"           { return Disable-BraveHardening }
            "Google Chrome"   { return Disable-ChromeHardening }
            "Microsoft Edge"  { return Disable-EdgeHardening }
        }

        $browsers = Get-InstalledBrowsers | Where-Object { $_.Name -eq $Name }
        if (-not $browsers -or @($browsers).Count -eq 0) {
            return @{ error = $true; message = "Browser not found on system: $Name" }
        }
        $b = @($browsers)[0]
        $cleanup = @{ removed = 0; errors = @() }

        if ($b.Engine -eq "Chromium" -and $b.PolicyPath) {
            $policyPaths = if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") { Get-OperaPolicyPaths -Name $b.Name } else { Get-ChromiumPolicyPathsForBrowser -Browser $b }
            foreach ($policyPath in $policyPaths) {
                Remove-BrowserPolicyPathIfPresent -Path $policyPath
            }
            $cleanup = Remove-ChromiumExtensionProfileData -Browser $b
            if ($b.Name -eq "Opera" -or $b.Name -eq "Opera GX") {
                Clear-OperaProfileHardening -Browser $b
            }
        } elseif ($b.Engine -eq "Gecko") {
            if ($b.PolicyPath) {
                Remove-BrowserPolicyPathIfPresent -Path $b.PolicyPath
            }
            Remove-GeckoPoliciesJson -InstallDir $b.InstallDir
            $cleanup = Remove-GeckoExtensionProfileData -Browser $b
        }

        @{ status = "restored"; browser = $Name; profileItemsRemoved = $cleanup.removed; profileCleanupErrors = $cleanup.errors }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Get-DefenderExclusions — read-only audit of Microsoft Defender exclusions.
# Surfaces ExclusionPath / ExclusionProcess / ExclusionExtension / ExclusionIpAddress
# and tags each entry with a severity hint based on common abuse patterns
# (drive roots, wildcards, user-writable directories, common LOLBin processes).
# Read-only by design — UI never offers a remove button without a license tier.
# ============================================================================
function Get-DefenderExclusions {
    Assert-IsAdmin
    try {
        # KT: `-ErrorAction SilentlyContinue` does not suppress the parser-level
        # "command not recognized" error you get when Defender has been removed.
        # Probe with Get-Command first, then fall through to a service-state
        # check so we can return a clean `disabled` status instead of leaking
        # a raw cmdlet error to the UI.
        if (-not (Get-Command Get-MpPreference -ErrorAction SilentlyContinue)) {
            $svc = Get-Service -Name WinDefend -ErrorAction SilentlyContinue
            if (-not $svc -or $svc.Status -ne 'Running') {
                return @{
                    status  = 'disabled'
                    reason  = if ($svc) { 'service-stopped' } else { 'service-removed' }
                    message = 'Defender is disabled on this system, so there are no exclusions to audit.'
                }
            }
            return @{ status = 'unavailable'; error = 'Defender preferences are not accessible - the cmdlet is missing but the service is running.' }
        }

        $pref = Get-MpPreference -ErrorAction SilentlyContinue
        if (-not $pref) {
            return @{ status = 'unavailable'; error = 'Defender preferences are not accessible - the service may be stopped or disabled.' }
        }

        # Lowercased lookup table for cheap LOLBin matching.
        $suspiciousProcessNames = @(
            'powershell.exe', 'powershell_ise.exe', 'pwsh.exe', 'cmd.exe',
            'wscript.exe', 'cscript.exe', 'mshta.exe', 'rundll32.exe',
            'regsvr32.exe', 'msbuild.exe', 'installutil.exe', 'bitsadmin.exe',
            'certutil.exe', 'curl.exe', 'wmic.exe'
        )

        $userWritableHints = @(
            '\\temp', '\\appdata', '\\downloads', '\\desktop',
            '\\public\\downloads', '\\users\\public', '\\windows\\temp'
        )

        function _Classify-Path {
            param([string]$p)
            if ([string]::IsNullOrWhiteSpace($p)) { return 'info' }
            $lower = $p.ToLower()
            # Drive root — `C:\`, `D:\`, `c:`
            if ($lower -match '^[a-z]:\\?$') { return 'critical' }
            # Wildcard masks
            if ($lower -match '\*' -or $lower -match '\?') { return 'high' }
            # User-writable directories
            foreach ($hint in $userWritableHints) {
                if ($lower.Contains($hint)) { return 'high' }
            }
            return 'info'
        }

        function _Classify-Process {
            param([string]$p)
            if ([string]::IsNullOrWhiteSpace($p)) { return 'info' }
            $name = (Split-Path $p -Leaf).ToLower()
            if ($suspiciousProcessNames -contains $name) { return 'critical' }
            # Contains a LOLBin name anywhere in the string (handles full paths).
            foreach ($needle in $suspiciousProcessNames) {
                if ($p.ToLower().Contains($needle)) { return 'critical' }
            }
            return 'info'
        }

        function _Classify-Extension {
            param([string]$e)
            # Extensions are inherently broad — flag the dangerous ones.
            if ([string]::IsNullOrWhiteSpace($e)) { return 'info' }
            $clean = $e.TrimStart('.').ToLower()
            $dangerous = @('exe', 'dll', 'ps1', 'bat', 'cmd', 'vbs', 'js', 'hta', 'lnk')
            if ($dangerous -contains $clean) { return 'high' }
            return 'info'
        }

        $rows = New-Object System.Collections.Generic.List[hashtable]

        if ($pref.ExclusionPath) {
            foreach ($p in @($pref.ExclusionPath)) {
                $rows.Add(@{ kind = 'path'; value = $p; severity = (_Classify-Path -p $p) })
            }
        }
        if ($pref.ExclusionProcess) {
            foreach ($p in @($pref.ExclusionProcess)) {
                $rows.Add(@{ kind = 'process'; value = $p; severity = (_Classify-Process -p $p) })
            }
        }
        if ($pref.ExclusionExtension) {
            foreach ($p in @($pref.ExclusionExtension)) {
                $rows.Add(@{ kind = 'extension'; value = $p; severity = (_Classify-Extension -e $p) })
            }
        }
        if ($pref.ExclusionIpAddress) {
            foreach ($p in @($pref.ExclusionIpAddress)) {
                # IP-level exclusions are rare and worth surfacing as `info`; we
                # don't have a heuristic for malicious vs admin use here.
                $rows.Add(@{ kind = 'ip'; value = $p; severity = 'info' })
            }
        }

        $bySeverity = @{
            critical = 0
            high     = 0
            info     = 0
        }
        foreach ($r in $rows) { $bySeverity[$r.severity]++ }

        return @{
            status     = 'ok'
            total      = $rows.Count
            bySeverity = $bySeverity
            rows       = @($rows)
        }
    }
    catch {
        return @{ status = 'unavailable'; error = $_.Exception.Message }
    }
}

