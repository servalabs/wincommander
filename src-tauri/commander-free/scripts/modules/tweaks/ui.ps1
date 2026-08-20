# ============================================================================
# TWEAKS - UI MODULE
# Windows interface and personalization tweaks
# ============================================================================

# --- BACKGROUND APPS ---

function Disable-BackgroundApps {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications' -Name 'GlobalUserDisabled' -Value 1 -Type DWord -Force
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Search' -Name 'BackgroundAppGlobalToggle' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-BackgroundApps {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications' -Name 'GlobalUserDisabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Search' -Name 'BackgroundAppGlobalToggle' -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- NOTIFICATIONS ---

function Disable-Notifications {
    Assert-IsAdmin
    $path = 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer'
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'DisableNotificationCenter' -Value 1 -Type DWord -Force
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\PushNotifications' -Name 'ToastEnabled' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-Notifications {
    Assert-IsAdmin
    Invoke-7Erase -Path 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer' -Type RegistryProperty -Name 'DisableNotificationCenter'
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\PushNotifications' -Name 'ToastEnabled' -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- CONTEXT MENU ---

function Enable-ClassicContextMenu {
    Assert-IsAdmin
    $path = 'HKCU:\SOFTWARE\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32'
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name '(Default)' -Value '' -Force
    Restart-Explorer | Out-Null
    @{ status = 'enabled'; requiresRestart = $true }
}

function Disable-ClassicContextMenu {
    Assert-IsAdmin
    Invoke-7Erase -Path 'HKCU:\SOFTWARE\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}' -Type Registry
    Restart-Explorer | Out-Null
    @{ status = 'disabled'; requiresRestart = $true }
}

# --- TASKBAR ---

function Enable-EndTaskOnTaskbar {
    $updated = Set-AllUserExplorerDword -SubKey 'Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings' -Name 'TaskbarEndTask' -Value 1
    $restart = Restart-Explorer -AllUsers
    @{ status = 'enabled'; profilesUpdated = $updated; explorerRestart = $restart }
}

function Disable-EndTaskOnTaskbar {
    $updated = Set-AllUserExplorerDword -SubKey 'Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings' -Name 'TaskbarEndTask' -Value 0 -Remove
    $restart = Restart-Explorer -AllUsers
    @{ status = 'disabled'; profilesUpdated = $updated; explorerRestart = $restart }
}

# --- SEARCH ---

function Disable-BingSearch {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search"
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name "BingSearchEnabled" -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-BingSearch {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search"
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name "BingSearchEnabled" -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- EXPLORER ---

function Show-FileExtensions {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "HideFileExt" -Value 0 -Type DWord -Force
    Restart-Explorer -AllUsers | Out-Null
    @{ status = 'shown' }
}

function Hide-FileExtensions {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "HideFileExt" -Value 1 -Type DWord -Force
    Restart-Explorer -AllUsers | Out-Null
    @{ status = 'hidden' }
}

function Show-HiddenFiles {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "Hidden" -Value 1 -Type DWord -Force
    Restart-Explorer | Out-Null
    @{ status = 'shown' }
}

function Hide-HiddenFiles {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "Hidden" -Value 0 -Type DWord -Force
    Restart-Explorer | Out-Null
    @{ status = 'hidden' }
}

# --- EXPLORER CLUTTER ---

function Enable-RemoveGalleryHome {
    Assert-IsAdmin
    # 1. Remove Home (Win11 specific CLSID)
    $homeCLSID = "{f874310e-b6b7-47dc-bc84-b9e6b38f5903}"
    $homeNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$homeCLSID"
    if (Test-Path $homeNS) { Invoke-7Erase -Path $homeNS -Type Registry }
    
    # Force Explorer to "This PC" (LaunchTo = 1)
    Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name LaunchTo -Value 1 -Force
    
    # 2. Remove Gallery (Win11 specific CLSID)
    $galleryCLSID = "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}"
    $galleryNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$galleryCLSID"
    if (Test-Path $galleryNS) { Invoke-7Erase -Path $galleryNS -Type Registry }

    # Legacy method for HubMode as fallback
    Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "HubMode" -Value 1 -Type DWord -Force
    
    Restart-Explorer | Out-Null
    @{ status = 'removed' }
}

function Disable-RemoveGalleryHome {
    Assert-IsAdmin
    # 1. Restore Home
    $homeCLSID = "{f874310e-b6b7-47dc-bc84-b9e6b38f5903}"
    $homeNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$homeCLSID"
    if (!(Test-Path $homeNS)) { New-Item $homeNS -Force | Out-Null }
    Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name LaunchTo -Value 0 -Force
    
    # 2. Restore Gallery
    $galleryCLSID = "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}"
    $galleryNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$galleryCLSID"
    if (!(Test-Path $galleryNS)) { New-Item $galleryNS -Force | Out-Null }

    Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "HubMode" -Value 0 -Type DWord -Force
    
    Restart-Explorer | Out-Null
    @{ status = 'restored' }
}

# ============================================================================
# NEW: Explorer Enhancements (from ReviOS explorer.yml)
# ============================================================================

# --- Folder Type Discovery ---
function Disable-FolderTypeDiscovery {
    try {
        $path = "HKCU:\SOFTWARE\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags\AllFolders\Shell"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "FolderType" -Value "NotSpecified" -Type String -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-FolderTypeDiscovery {
    try {
        $path = "HKCU:\SOFTWARE\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags\AllFolders\Shell"
        if (Test-Path $path) {
            Remove-ItemSecure -Path $path -Name "FolderType" -ErrorAction SilentlyContinue
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Shortcut Suffix ---
function Remove-ShortcutSuffix {
    try {
        Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "link" -Value ([byte[]](0x00,0x00,0x00,0x00)) -Type Binary -Force
        @{ status = "removed" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Restore-ShortcutSuffix {
    try {
        Remove-ItemSecure -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "link" -ErrorAction SilentlyContinue
        @{ status = "restored" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- AutoPlay ---
function Disable-AutoPlay {
    try {
        $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "DisableAutoplay" -Value 1 -Type DWord -Force
        # Also disable AutoRun
        $polPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"
        if (!(Test-Path $polPath)) { New-Item -Path $polPath -Force | Out-Null }
        Set-ItemProperty -Path $polPath -Name "NoDriveTypeAutoRun" -Value 255 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutoPlay {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers" -Name "DisableAutoplay" -Value 0 -Type DWord -Force
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer" -Name "NoDriveTypeAutoRun" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Low Disk Space Check ---
function Disable-LowDiskCheck {
    try {
        $path = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "NoLowDiskSpaceChecks" -Value 1 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-LowDiskCheck {
    try {
        Remove-ItemSecure -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer" -Name "NoLowDiskSpaceChecks" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Explorer Opens This PC ---
function Set-ExplorerOpensThisPC {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "LaunchTo" -Value 1 -Type DWord -Force
        @{ status = "thispc" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Set-ExplorerOpensQuickAccess {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "LaunchTo" -Value 2 -Type DWord -Force
        @{ status = "quickaccess" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Sync Provider Notifications ---
function Hide-SyncProviderNotifications {
    try {
        Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSyncProviderNotifications" -Value 0 -Type DWord -Force
        @{ status = "hidden" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Show-SyncProviderNotifications {
    try {
        Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSyncProviderNotifications" -Value 1 -Type DWord -Force
        @{ status = "shown" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Transparency Effects ---
function Disable-TransparencyEffects {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name "EnableTransparency" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-TransparencyEffects {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name "EnableTransparency" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Full Path in Title Bar ---
function Enable-FullPathInTitleBar {
    try {
        $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "FullPath" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-FullPathInTitleBar {
    try {
        $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState"
        if (Test-Path $path) {
            Set-ItemProperty -Path $path -Name "FullPath" -Value 0 -Type DWord -Force
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Taskbar Debloat (always-on: hide widgets, task view, search, chat, people bar, meet now) ---
function Set-TaskbarDebloated {
    Assert-IsAdmin
    try {
        $adv = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
        # Hide Task View button
        Set-ItemProperty -Path $adv -Name "ShowTaskViewButton" -Value 0 -Type DWord -Force
        # Hide Widgets (TaskbarDa)
        Set-ItemProperty -Path $adv -Name "TaskbarDa" -Value 0 -Type DWord -Force -ErrorAction SilentlyContinue
        # Hide Chat icon (TaskbarMn)
        Set-ItemProperty -Path $adv -Name "TaskbarMn" -Value 0 -Type DWord -Force -ErrorAction SilentlyContinue
        # Hide Search and align taskbar left
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search" -Name "SearchboxTaskbarMode" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $adv -Name "TaskbarAl" -Value 0 -Type DWord -Force -ErrorAction SilentlyContinue

        # Policy-level blocks
        $feedsPath = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Feeds"
        if (!(Test-Path $feedsPath)) { New-Item -Path $feedsPath -Force | Out-Null }
        Set-ItemProperty -Path $feedsPath -Name "EnableFeeds" -Value 0 -Type DWord -Force

        $chatPath = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Chat"
        if (!(Test-Path $chatPath)) { New-Item -Path $chatPath -Force | Out-Null }
        Set-ItemProperty -Path $chatPath -Name "ChatIcon" -Value 3 -Type DWord -Force

        $dshPath = "HKLM:\SOFTWARE\Policies\Microsoft\Dsh"
        if (!(Test-Path $dshPath)) { New-Item -Path $dshPath -Force | Out-Null }
        Set-ItemProperty -Path $dshPath -Name "AllowNewsAndInterests" -Value 0 -Type DWord -Force

        # Hide People Bar
        $polExplorer = "HKCU:\Software\Policies\Microsoft\Windows\Explorer"
        if (!(Test-Path $polExplorer)) { New-Item -Path $polExplorer -Force | Out-Null }
        Set-ItemProperty -Path $polExplorer -Name "HidePeopleBar" -Value 1 -Type DWord -Force

        # Hide Meet Now
        $polExplorerCU = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"
        if (!(Test-Path $polExplorerCU)) { New-Item -Path $polExplorerCU -Force | Out-Null }
        Set-ItemProperty -Path $polExplorerCU -Name "HideSCAMeetNow" -Value 1 -Type DWord -Force

        Restart-Explorer -AllUsers | Out-Null
        @{ status = "debloated" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Reset-TaskbarDebloated {
    try {
        $adv = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
        Set-ItemProperty -Path $adv -Name "ShowTaskViewButton" -Value 1 -Type DWord -Force
        Remove-ItemSecure -Path $adv -Name "TaskbarDa" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path $adv -Name "TaskbarMn" -ErrorAction SilentlyContinue
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search" -Name "SearchboxTaskbarMode" -Value 2 -Type DWord -Force
        Set-ItemProperty -Path $adv -Name "TaskbarAl" -Value 1 -Type DWord -Force -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Software\Policies\Microsoft\Windows\Explorer" -Name "HidePeopleBar" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer" -Name "HideSCAMeetNow" -ErrorAction SilentlyContinue
        Restart-Explorer -AllUsers | Out-Null
        @{ status = "reset" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Start Menu Recommendations ---
function Disable-StartRecommendations {
    try {
        $adv = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
        Set-ItemProperty -Path $adv -Name "Start_IrisRecommendations" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $adv -Name "Start_AccountNotifications" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-StartRecommendations {
    try {
        $adv = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
        Set-ItemProperty -Path $adv -Name "Start_IrisRecommendations" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $adv -Name "Start_AccountNotifications" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Take Ownership Context Menu
# ============================================================================

function Enable-TakeOwnershipMenu {
    Assert-IsAdmin
    try {
        # Add "Take Ownership" to right-click context menu for files
        $filePath = "HKLM:\SOFTWARE\Classes\*\shell\TakeOwnership"
        if (!(Test-Path $filePath)) { New-Item -Path $filePath -Force | Out-Null }
        Set-ItemProperty -Path $filePath -Name "(Default)" -Value "Take Ownership" -Force
        Set-ItemProperty -Path $filePath -Name "HasLUAShield" -Value "" -Force
        Set-ItemProperty -Path $filePath -Name "NoWorkingDirectory" -Value "" -Force
        $fileCmd = "$filePath\command"
        if (!(Test-Path $fileCmd)) { New-Item -Path $fileCmd -Force | Out-Null }
        Set-ItemProperty -Path $fileCmd -Name "(Default)" -Value 'cmd.exe /c takeown /f "%1" && icacls "%1" /grant administrators:F /c /l' -Force
        Set-ItemProperty -Path $fileCmd -Name "IsolatedCommand" -Value 'cmd.exe /c takeown /f "%1" && icacls "%1" /grant administrators:F /c /l' -Force

        # Add for directories
        $dirPath = "HKLM:\SOFTWARE\Classes\Directory\shell\TakeOwnership"
        if (!(Test-Path $dirPath)) { New-Item -Path $dirPath -Force | Out-Null }
        Set-ItemProperty -Path $dirPath -Name "(Default)" -Value "Take Ownership" -Force
        Set-ItemProperty -Path $dirPath -Name "HasLUAShield" -Value "" -Force
        Set-ItemProperty -Path $dirPath -Name "NoWorkingDirectory" -Value "" -Force
        $dirCmd = "$dirPath\command"
        if (!(Test-Path $dirCmd)) { New-Item -Path $dirCmd -Force | Out-Null }
        Set-ItemProperty -Path $dirCmd -Name "(Default)" -Value 'cmd.exe /c takeown /f "%1" /r /d y && icacls "%1" /grant administrators:F /t /c /l' -Force
        Set-ItemProperty -Path $dirCmd -Name "IsolatedCommand" -Value 'cmd.exe /c takeown /f "%1" /r /d y && icacls "%1" /grant administrators:F /t /c /l' -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-TakeOwnershipMenu {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Classes\*\shell\TakeOwnership" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Classes\Directory\shell\TakeOwnership" -Recurse -Force -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Enthusiast Mode — Verbose file operation dialogs
# ============================================================================

function Enable-EnthusiastMode {
    try {
        $opPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager"
        if (!(Test-Path $opPath)) { New-Item -Path $opPath -Force | Out-Null }
        Set-ItemProperty -Path $opPath -Name "EnthusiastMode" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-EnthusiastMode {
    try {
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager" -Name "EnthusiastMode" -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Instant Start Menu (MenuShowDelay = 0)
# ============================================================================

function Enable-InstantMenuDelay {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "MenuShowDelay" -Value "0" -Type String -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-InstantMenuDelay {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "MenuShowDelay" -Value "400" -Type String -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Wallpaper JPEG Quality (prevent compression artifacts)
# ============================================================================

function Enable-WallpaperQuality {
    try {
        $path = "HKCU:\Control Panel\Desktop"
        Set-ItemProperty -Path $path -Name "JPEGImportQuality" -Value 100 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-WallpaperQuality {
    try {
        Remove-ItemSecure -Path "HKCU:\Control Panel\Desktop" -Name "JPEGImportQuality" -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Disable Accessibility Shortcuts (StickyKeys, ToggleKeys, FilterKeys)
# ============================================================================

function Disable-AccessibilityShortcuts {
    try {
        # StickyKeys: flags 506 = disabled (no sound, no popup, no enable)
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\StickyKeys" -Name "Flags" -Value "506" -Type String -Force
        # ToggleKeys
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\ToggleKeys" -Name "Flags" -Value "58" -Type String -Force
        # FilterKeys
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\Keyboard Response" -Name "Flags" -Value "122" -Type String -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AccessibilityShortcuts {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\StickyKeys" -Name "Flags" -Value "510" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\ToggleKeys" -Name "Flags" -Value "62" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\Keyboard Response" -Name "Flags" -Value "126" -Type String -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Disable Mouse Acceleration (Enhance Pointer Precision)
# ============================================================================

function Disable-MouseAcceleration {
    try {
        # Set flat acceleration curve (1:1 mouse movement)
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseSpeed" -Value "0" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold1" -Value "0" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold2" -Value "0" -Type String -Force
        # Disable "Enhance pointer precision"
        $accelCurve = [byte[]](0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0xC0,0xCC,0x0C,0x00,0x00,0x00,0x00,0x00,
                               0x80,0x99,0x19,0x00,0x00,0x00,0x00,0x00,
                               0x40,0x66,0x26,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x33,0x33,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x40,0x00,0x00,0x00,0x00,0x00,
                               0x00,0xCC,0x4C,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x99,0x59,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x66,0x66,0x00,0x00,0x00,0x00,0x00)
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseXCurve" -Value $accelCurve -Type Binary -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseYCurve" -Value $accelCurve -Type Binary -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-MouseAcceleration {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseSpeed" -Value "1" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold1" -Value "6" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold2" -Value "10" -Type String -Force
        Remove-ItemSecure -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseXCurve" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseYCurve" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Disable Autocorrect / Spellcheck / Text Predictions
# ============================================================================

function Disable-AutocorrectSpellcheck {
    try {
        $path = "HKCU:\Software\Microsoft\TabletTip\1.7"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "EnableAutocorrection" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableSpellchecking" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableTextPrediction" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnablePredictionSpaceInsertion" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableDoubleTapSpace" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutocorrectSpellcheck {
    try {
        $path = "HKCU:\Software\Microsoft\TabletTip\1.7"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "EnableAutocorrection" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableSpellchecking" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableTextPrediction" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnablePredictionSpaceInsertion" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableDoubleTapSpace" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Make PowerShell 7 the Windows Terminal default profile
#
# Scope: only Windows Terminal's own "defaultProfile" setting (which profile
# opens with no explicit profile argument -- Win+X "Terminal", the modern
# Explorer "Open in Terminal" verb). Deliberately does NOT touch the separate
# Windows 11 "Default Terminal Application" registry axis
# (HKCU:\Console\%%Startup DelegationConsole/DelegationTerminal), which
# repoints the host for every console app system-wide and is documented as
# unreliable under elevated processes -- this app always runs elevated.
# ============================================================================

function Get-WindowsTerminalSettingsPath {
    $packaged = Get-ChildItem -Path "$env:LOCALAPPDATA\Packages" -Filter 'Microsoft.WindowsTerminal_*' -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notmatch 'Preview|Canary' } |
        Select-Object -First 1
    if ($packaged) {
        $path = Join-Path $packaged.FullName 'LocalState\settings.json'
        if (Test-Path -LiteralPath $path -PathType Leaf) { return $path }
    }
    $unpackaged = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows Terminal\settings.json'
    if (Test-Path -LiteralPath $unpackaged -PathType Leaf) { return $unpackaged }
    return $null
}

function ConvertFrom-WindowsTerminalSettings {
    # Windows Terminal ships a leading "// ..." comment header and users
    # sometimes hand-edit trailing commas; both break strict ConvertFrom-Json.
    # This parsed copy is discovery-only (finding a profile's GUID) and is
    # never written back, so stripping comments/trailing-commas here can
    # never damage the user's actual on-disk formatting.
    param([string]$RawJson)
    $stripped = ($RawJson -split "`r?`n" | Where-Object { $_.TrimStart() -notmatch '^//' }) -join "`n"
    $stripped = [regex]::Replace($stripped, ',(\s*[}\]])', '$1')
    try { $stripped | ConvertFrom-Json -ErrorAction Stop } catch { $null }
}

function Find-PowerShell7ProfileGuid {
    param($ParsedSettings)
    if ($ParsedSettings.disabledProfileSources -contains 'Windows.Terminal.PowershellCore') { return $null }
    $profiles = @($ParsedSettings.profiles.list)
    $match = $profiles | Where-Object { $_.source -eq 'Windows.Terminal.PowershellCore' } | Select-Object -First 1
    if (-not $match) { $match = $profiles | Where-Object { $_.commandline -match 'pwsh\.exe' } | Select-Object -First 1 }
    if (-not $match) { $match = $profiles | Where-Object { $_.name -eq 'PowerShell' } | Select-Object -First 1 }
    if ($match) { return $match.guid }
    return $null
}

function Find-WindowsPowerShellProfileGuid {
    param($ParsedSettings)
    $profiles = @($ParsedSettings.profiles.list)
    $match = $profiles | Where-Object { $_.name -eq 'Windows PowerShell' } | Select-Object -First 1
    if ($match) { return $match.guid }
    # Windows Terminal's fixed, non-dynamic GUID for the built-in Windows PowerShell profile.
    return '{61c54bbd-c2c6-5271-96e7-009a87ff44bf}'
}

function Get-PowerShell7DefaultStatus {
    $settingsPath = Get-WindowsTerminalSettingsPath
    if (-not $settingsPath) { return @{ default = $false; available = $false; reason = 'Windows Terminal is not installed' } }
    $parsed = ConvertFrom-WindowsTerminalSettings -RawJson ([System.IO.File]::ReadAllText($settingsPath))
    if (-not $parsed) { return @{ default = $false; available = $false; reason = 'Could not read Windows Terminal settings' } }
    $ps7Guid = Find-PowerShell7ProfileGuid -ParsedSettings $parsed
    if (-not $ps7Guid) {
        return @{ default = $false; available = $false; reason = 'PowerShell 7 profile not found -- open Windows Terminal once after installing PowerShell 7' }
    }
    return @{ default = ($parsed.defaultProfile -eq $ps7Guid); available = $true }
}

function Enable-PowerShell7DefaultShell {
    if (-not (Test-PowerShell7Installed).installed) {
        return @{ error = $true; message = 'PowerShell 7 is not installed' }
    }
    $settingsPath = Get-WindowsTerminalSettingsPath
    if (-not $settingsPath) { return @{ error = $true; message = 'Windows Terminal is not installed' } }
    $raw = [System.IO.File]::ReadAllText($settingsPath)
    $parsed = ConvertFrom-WindowsTerminalSettings -RawJson $raw
    if (-not $parsed) { return @{ error = $true; message = 'Could not read Windows Terminal settings' } }
    $ps7Guid = Find-PowerShell7ProfileGuid -ParsedSettings $parsed
    if (-not $ps7Guid) {
        return @{ error = $true; message = 'PowerShell 7 profile not found in Windows Terminal -- open Windows Terminal once after installing PowerShell 7, then try again' }
    }
    # Surgical text replace of only the defaultProfile value -- preserves the
    # user's comments, key order, and every other hand-edited setting. Uses
    # plain substring surgery (not a regex Replace-all) so a hand-trimmed
    # file with no defaultProfile key can't have every "{" in the JSON
    # corrupted by a naive first-brace regex substitution.
    $keyPattern = [regex]'"defaultProfile"\s*:\s*"[^"]*"'
    $existingMatch = $keyPattern.Match($raw)
    $updated = if ($existingMatch.Success) {
        $raw.Substring(0, $existingMatch.Index) + "`"defaultProfile`": `"$ps7Guid`"" + $raw.Substring($existingMatch.Index + $existingMatch.Length)
    } else {
        $braceIndex = $raw.IndexOf('{')
        $raw.Substring(0, $braceIndex + 1) + "`n    `"defaultProfile`": `"$ps7Guid`"," + $raw.Substring($braceIndex + 1)
    }
    [System.IO.File]::WriteAllText($settingsPath, $updated)
    @{ status = 'enabled' }
}

function Disable-PowerShell7DefaultShell {
    $settingsPath = Get-WindowsTerminalSettingsPath
    if (-not $settingsPath) { return @{ status = 'disabled' } }
    $raw = [System.IO.File]::ReadAllText($settingsPath)
    $parsed = ConvertFrom-WindowsTerminalSettings -RawJson $raw
    if (-not $parsed) { return @{ status = 'disabled' } }
    $winPsGuid = Find-WindowsPowerShellProfileGuid -ParsedSettings $parsed
    $keyPattern = [regex]'"defaultProfile"\s*:\s*"[^"]*"'
    $existingMatch = $keyPattern.Match($raw)
    if ($existingMatch.Success) {
        $updated = $raw.Substring(0, $existingMatch.Index) + "`"defaultProfile`": `"$winPsGuid`"" + $raw.Substring($existingMatch.Index + $existingMatch.Length)
        [System.IO.File]::WriteAllText($settingsPath, $updated)
    }
    @{ status = 'disabled' }
}
