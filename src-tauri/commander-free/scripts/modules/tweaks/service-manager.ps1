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
                StartMode      = $(if ($s.StartMode -eq 'Auto' -and $s.DelayedAutoStart) { 'AutomaticDelayedStart' } else { $s.StartMode })
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
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$StartMode, [Parameter(Mandatory)][string]$PlanId)
    Invoke-ServiceChangePlan -PlanId $PlanId -ExpectedAction 'SetStartMode' -ExpectedName $Name -ExpectedMode $StartMode
}

function Start-ServiceByName {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$PlanId)
    Invoke-ServiceChangePlan -PlanId $PlanId -ExpectedAction 'Start' -ExpectedName $Name
}

function Stop-ServiceByName {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$PlanId)
    Invoke-ServiceChangePlan -PlanId $PlanId -ExpectedAction 'Stop' -ExpectedName $Name
}

function Restart-ServiceByName {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$PlanId)
    Invoke-ServiceChangePlan -PlanId $PlanId -ExpectedAction 'Restart' -ExpectedName $Name
}
