$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$maintenanceModule = Join-Path $repoRoot 'src-tauri\commander-free\scripts\modules\tweaks\maintenance.ps1'
. $maintenanceModule

$script:mockServices = @{
    ALG = [pscustomobject]@{ Name = 'ALG'; StartType = 'Manual'; Status = 'Stopped' }
    AppMgmt = [pscustomobject]@{ Name = 'AppMgmt'; StartType = 'Automatic'; Status = 'Stopped' }
    AppReadiness = [pscustomobject]@{ Name = 'AppReadiness'; StartType = 'Automatic'; Status = 'Stopped' }
    DPS = [pscustomobject]@{ Name = 'DPS'; StartType = 'Automatic'; Status = 'Running' }
}
$script:mockWrites = [System.Collections.Generic.List[string]]::new()
$script:mockStops = [System.Collections.Generic.List[string]]::new()

function Assert-IsAdmin {}
function Get-Service {
    [CmdletBinding()]
    param([string]$Name)
    return $script:mockServices[$Name]
}
function Set-Service {
    [CmdletBinding()]
    param(
        [string]$Name,
        [string]$StartupType
    )
    if ($Name -eq 'AppReadiness') { throw 'Mocked access denied.' }
    $script:mockServices[$Name].StartType = $StartupType
    [void]$script:mockWrites.Add("$Name=$StartupType")
}
function Stop-Service {
    [CmdletBinding()]
    param(
        [string]$Name,
        [switch]$Force
    )
    $script:mockServices[$Name].Status = 'Stopped'
    [void]$script:mockStops.Add($Name)
}

$result = Set-ServicesManual
if ($result.status -ne 'done') { throw 'The service profile did not complete its mocked pass.' }
if ($script:mockServices.AppMgmt.StartType -ne 'Manual') { throw 'The manual service change was not applied.' }
if ($script:mockServices.DPS.StartType -ne 'Disabled' -or $script:mockServices.DPS.Status -ne 'Stopped') {
    throw 'The disabled service change was not applied.'
}
if ($script:mockServices.ALG.StartType -ne 'Manual') { throw 'An already-correct service was unexpectedly changed.' }
if ($result.manual.failed.Count -ne 1 -or $result.manual.failed[0].name -ne 'AppReadiness') {
    throw 'The service profile did not preserve a per-service apply failure.'
}
if ($script:mockWrites.Count -ne 2 -or $script:mockStops.Count -ne 1) {
    throw 'The service profile mock did not exercise the expected apply paths.'
}

Write-Output 'Service Profile apply contract passed with mocked service state and a per-service failure.'
