# ============================================================================
# TWEAKS - SERVICE MANAGER
# Enumerate Windows services and let the user change their start mode.
# Wraps Get-Service + Get-CimInstance Win32_Service so the UI can show
# DisplayName / StartMode / Description in one row.
# ============================================================================

# Curated set of services that the WinCommander service profile flips to
# "Manual" or "Disabled". Surfaces in the UI as a "Recommended profile"
# button; users can still flip individual services.
$Global:WincmdServiceRecommendations = @{
    # ── Recommended: Disabled ──
    "DiagTrack"                   = "Disabled"   # Connected User Experiences and Telemetry
    "dmwappushservice"            = "Disabled"   # WAP push routing
    "MapsBroker"                  = "Disabled"   # Downloaded Maps Manager
    "RetailDemo"                  = "Disabled"
    "lfsvc"                       = "Disabled"   # Geolocation
    "WMPNetworkSvc"               = "Disabled"
    "XblAuthManager"              = "Disabled"
    "XblGameSave"                 = "Disabled"
    "XboxGipSvc"                  = "Disabled"
    "XboxNetApiSvc"               = "Disabled"
    "WerSvc"                      = "Disabled"   # Windows Error Reporting
    "PcaSvc"                      = "Disabled"   # Program Compatibility Assistant
    "DPS"                         = "Disabled"   # Diagnostic Policy Service
    "DiagSvc"                     = "Disabled"
    "Fax"                         = "Disabled"
    "PrintNotify"                 = "Disabled"
    "RemoteRegistry"              = "Disabled"
    "TabletInputService"          = "Disabled"
    "WSearch"                     = "Disabled"
    # ── Recommended: Manual ──
    "BITS"                        = "Manual"
    "wuauserv"                    = "Manual"
    "WlanSvc"                     = "Manual"
    "Spooler"                     = "Manual"
    "BluetoothUserService"        = "Manual"
    "BthAvctpSvc"                 = "Manual"
    "BTAGService"                 = "Manual"
}

function Get-AllServices {
    Assert-IsAdmin
    try {
        # CIM brings DisplayName + StartMode + Description in one call
        $cim = Get-CimInstance Win32_Service -ErrorAction SilentlyContinue
        $out = @()
        foreach ($s in $cim) {
            $rec = $null
            if ($Global:WincmdServiceRecommendations.ContainsKey($s.Name)) {
                $rec = $Global:WincmdServiceRecommendations[$s.Name]
            }
            $out += [PSCustomObject]@{
                Name           = $s.Name
                DisplayName    = $s.DisplayName
                Description    = $s.Description
                StartMode      = $s.StartMode      # "Auto","Manual","Disabled","Boot","System"
                State          = $s.State           # "Running","Stopped","Paused"
                Status         = $s.Status
                CanPauseAndContinue = $s.AcceptPause
                CanStop        = $s.AcceptStop
                Recommended    = $rec
            }
        }
        $out | Sort-Object DisplayName
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Set-ServiceStartMode {
    param(
        [string]$Name,
        [ValidateSet("Automatic","Manual","Disabled","AutomaticDelayedStart")]
        [string]$StartMode
    )
    Assert-IsAdmin
    try {
        if ($StartMode -eq "AutomaticDelayedStart") {
            # PowerShell Set-Service exposes only Automatic/Manual/Disabled; use sc.exe for delayed.
            $null = & sc.exe config $Name start= delayed-auto 2>$null
            if ($LASTEXITCODE -ne 0) { throw "sc.exe failed with code $LASTEXITCODE" }
        } else {
            Set-Service -Name $Name -StartupType $StartMode -ErrorAction Stop
        }
        @{ success = $true; name = $Name; startMode = $StartMode }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Start-ServiceByName {
    param([string]$Name)
    Assert-IsAdmin
    try {
        Start-Service -Name $Name -ErrorAction Stop
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Stop-ServiceByName {
    param([string]$Name)
    Assert-IsAdmin
    try {
        Stop-Service -Name $Name -Force -ErrorAction Stop
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Restart-ServiceByName {
    param([string]$Name)
    Assert-IsAdmin
    try {
        Restart-Service -Name $Name -Force -ErrorAction Stop
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# Bulk apply happens via the existing Set-ServicesManual command
# (tweaks/maintenance) — exposed in the panel's System Maintenance card
# as "Optimize Services". Don't add a second one here.
# The $Global:WincmdServiceRecommendations map above is still used as a
# read-only "★ Recommended" hint in the per-row dropdown.
