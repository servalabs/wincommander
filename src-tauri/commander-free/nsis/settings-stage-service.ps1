[CmdletBinding()]
param([ValidateSet('Status','Stop','Restore')][string]$Action = 'Status')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ServiceControllerStatus values are numeric and do not depend on Windows UI
# language. 0 represents a missing service and 255 means inspection failed.
function Get-Transition([int]$State, [string]$Target) {
    if ($Target -eq 'Stop') {
        switch ($State) {
            0 { return 'Done' }; 1 { return 'Done' }; 3 { return 'Wait' }; 4 { return 'Stop' }; 2 { return 'Wait' }
            default { return 'Error' }
        }
    }
    switch ($State) {
        4 { return 'Done' }; 1 { return 'Start' }; 2 { return 'Wait' }; 3 { return 'Wait' }
        default { return 'Error' }
    }
}
function Get-ActualServiceState {
    try {
        $service = Get-Service -Name 'WinCommanderSvc' -ErrorAction Stop
        return [int]$service.Status
    } catch {
        if ($_.FullyQualifiedErrorId -like 'NoServiceFoundForGivenName*') { return 0 }
        throw
    }
}
function Invoke-ServiceTransition([string]$Target, [scriptblock]$ReadState, [scriptblock]$RequestStart, [scriptblock]$RequestStop, [scriptblock]$Wait, [int]$Seconds) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $requested = $false
    while ($true) {
        $decision = Get-Transition ([int](& $ReadState)) $Target
        switch ($decision) {
            'Done' { return }
            'Error' { throw 'Service is missing, inaccessible, or in an unsupported state.' }
            'Start' { if (-not $requested) { & $RequestStart; $requested = $true } }
            'Stop' { if (-not $requested) { & $RequestStop; $requested = $true } }
            'Wait' { }
        }
        if ($clock.Elapsed.TotalSeconds -ge $Seconds) { throw 'Service transition timed out.' }
        & $Wait
    }
}
if ($MyInvocation.InvocationName -eq '.') { return }
try {
    if ($Action -eq 'Status') { exit (Get-ActualServiceState) }
    $seconds = if ($Action -eq 'Stop') { 135 } else { 60 }
    Invoke-ServiceTransition $Action { Get-ActualServiceState } { Start-Service -Name 'WinCommanderSvc' -ErrorAction Stop } { Stop-Service -Name 'WinCommanderSvc' -ErrorAction Stop } { Start-Sleep -Seconds 1 } $seconds
    exit 0
} catch {
    [Console]::Error.WriteLine("Cannot $Action WinCommander service: $($_.Exception.Message)")
    exit 255
}
