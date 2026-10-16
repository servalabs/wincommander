$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $repoRoot 'src-tauri\commander-free\scripts\modules\tweaks\maintenance.ps1')
$targets = Get-ServiceProfileTargets
foreach ($timeService in @('W32Time', 'autotimesvc')) {
    if ($timeService -in $targets.manual -or $timeService -in $targets.disable) {
        throw "$timeService must remain under Windows automatic time management."
    }
}
$script:mockServices = @{
    ALG = [pscustomobject]@{ Name = 'ALG'; StartType = 'Manual'; Status = 'Stopped'; ServiceType = 16 }
    AppMgmt = [pscustomobject]@{ Name = 'AppMgmt'; StartType = 'Manual'; Status = 'Stopped'; ServiceType = 16 }
    DPS = [pscustomobject]@{ Name = 'DPS'; StartType = 'Disabled'; Status = 'Stopped'; ServiceType = 16 }
    NetBT = [pscustomobject]@{ Name = 'NetBT'; StartType = 'System'; Status = 'Running'; ServiceType = 1 }
}
function Get-Service {
    [CmdletBinding()]
    param([string]$Name)
    return $script:mockServices[$Name]
}
$status = Get-ServiceProfileStatus
if (-not $status.applied -or $status.driftCount -ne 0 -or $status.checkedCount -ne 3) {
    throw 'Already-correct service settings must stay resolved; driver settings are outside this profile.'
}
$script:mockServices.AppMgmt.StartType = 'Automatic'
if ((Get-ServiceProfileStatus).applied) { throw 'Actual Windows drift must make the profile actionable again.' }
function Invoke-ServiceChangePlan {
    param([Parameter(Mandatory)][string]$PlanId, [string]$ExpectedAction)
    if ($PlanId -ne 'approved-plan' -or $ExpectedAction -ne 'Profile') { throw 'Profile approval was not bound to its saved plan.' }
    return @{ success = $true; status = 'done' }
}
if (-not (Set-ServicesManual -PlanId 'approved-plan').success) { throw 'Profile execution did not use its approved plan.' }
Write-Output 'Service Profile status and approved-plan dispatch passed; automatic time settings and drivers remain untouched.'
