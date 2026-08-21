# ============================================================================
# TWEAKS - UI (GRANULAR)
# Per-feature granular UI toggles.
# Kept in its own file to stay under the 300-line house rule.
# ============================================================================

# ── DESKTOP ICONS (per-CLSID) ───────────────────────────────────────────
# Toggling these flips HKCU\...\Explorer\HideDesktopIcons\NewStartPanel\<CLSID>
# - value 0  -> visible
# - value 1  -> hidden
# ClassicStartMenu mirror is kept in sync for Group Policy / classic shell.

$Global:DesktopIconClsids = @{
    "thisPC"        = "{20D04FE0-3AEA-1069-A2D8-08002B30309D}"
    "userFiles"     = "{59031a47-3f72-44a7-89c5-5595fe6b30ee}"
    "network"       = "{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}"
    "recycleBin"    = "{645FF040-5081-101B-9F08-00AA002F954E}"
    "controlPanel"  = "{5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0}"
}

function _Set-DesktopIcon {
    param([string]$Clsid, [bool]$Visible)
    $value = if ($Visible) { 0 } else { 1 }
    foreach ($root in @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel",
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\ClassicStartMenu"
    )) {
        if (!(Test-Path $root)) { New-Item -Path $root -Force | Out-Null }
        Set-ItemProperty -Path $root -Name $Clsid -Value $value -Type DWord -Force
    }
}

function Show-DesktopIconThisPc      { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.thisPC       -Visible $true;  @{ status = "shown" } }
function Hide-DesktopIconThisPc      { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.thisPC       -Visible $false; @{ status = "hidden" } }
function Show-DesktopIconUserFiles   { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.userFiles    -Visible $true;  @{ status = "shown" } }
function Hide-DesktopIconUserFiles   { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.userFiles    -Visible $false; @{ status = "hidden" } }
function Show-DesktopIconNetwork     { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.network      -Visible $true;  @{ status = "shown" } }
function Hide-DesktopIconNetwork     { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.network      -Visible $false; @{ status = "hidden" } }
function Show-DesktopIconRecycleBin  { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.recycleBin   -Visible $true;  @{ status = "shown" } }
function Hide-DesktopIconRecycleBin  { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.recycleBin   -Visible $false; @{ status = "hidden" } }
function Show-DesktopIconControlPanel { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.controlPanel -Visible $true;  @{ status = "shown" } }
function Hide-DesktopIconControlPanel { _Set-DesktopIcon -Clsid $Global:DesktopIconClsids.controlPanel -Visible $false; @{ status = "hidden" } }

# ── SHORTCUT ARROW OVERLAY ──────────────────────────────────────────────
# HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Icons\29
# pointed at a blank icon hides the overlay. Removing the value restores it.

function Remove-ShortcutArrow {
    Assert-IsAdmin
    try {
        $shellIcons = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Icons"
        if (!(Test-Path $shellIcons)) { New-Item -Path $shellIcons -Force | Out-Null }
        # %SystemRoot%\System32\imageres.dll has a transparent icon at index 197 on Win10+ that we abuse;
        # on older builds, %windir%\System32\shell32.dll,-50 (blank icon) works too.
        Set-ItemProperty -Path $shellIcons -Name "29" -Value "%SystemRoot%\System32\imageres.dll,-1015" -Type String -Force
        @{ status = "removed"; requiresRestart = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Restore-ShortcutArrow {
    Assert-IsAdmin
    try {
        $shellIcons = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Icons"
        if (Test-Path $shellIcons) {
            Remove-ItemProperty -Path $shellIcons -Name "29" -ErrorAction SilentlyContinue
        }
        @{ status = "restored"; requiresRestart = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── SNAP-ASSIST FLYOUT ──────────────────────────────────────────────────
function Disable-SnapAssistFlyout {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "EnableSnapAssistFlyout" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}
function Enable-SnapAssistFlyout {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "EnableSnapAssistFlyout" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── EXPLORER COMPACT MODE ───────────────────────────────────────────────
function Enable-ExplorerCompactMode {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "UseCompactMode" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}
function Disable-ExplorerCompactMode {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "UseCompactMode" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── EXPLORER FILE CHECKBOXES ────────────────────────────────────────────
function Enable-ExplorerCheckboxes {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "AutoCheckSelect" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}
function Disable-ExplorerCheckboxes {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "AutoCheckSelect" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── WINDOW SHAKE TO MINIMISE ────────────────────────────────────────────
# DisallowShaking=1 disables; 0 enables (Windows default).
function Disable-WindowShake {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "DisallowShaking" -Value 1 -Type DWord -Force
        @{ status = "disabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}
function Enable-WindowShake {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "DisallowShaking" -Value 0 -Type DWord -Force
        @{ status = "enabled" }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# ── TASKBAR CLOCK SECONDS ───────────────────────────────────────────────
function Show-ClockSeconds {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSecondsInSystemClock" -Value 1 -Type DWord -Force
        @{ status = "shown"; requiresRestart = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}
function Hide-ClockSeconds {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSecondsInSystemClock" -Value 0 -Type DWord -Force
        @{ status = "hidden"; requiresRestart = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}
