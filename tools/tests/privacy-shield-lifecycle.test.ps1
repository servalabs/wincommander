$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1')

$session = [Diagnostics.Process]::GetCurrentProcess().SessionId
$script:items = @(
    [pscustomobject]@{ Name='python.exe'; SessionId=$session; ProcessId=9001; CommandLine='--wc-privacy-shield' },
    [pscustomobject]@{ Name='python.exe'; SessionId=$session; ProcessId=9002; CommandLine='unrelated.py' },
    [pscustomobject]@{ Name='python.exe'; SessionId=($session + 1); ProcessId=9003; CommandLine='--wc-privacy-shield' }
)
$script:stopped = @()
$script:cameraProbes = 0
$script:changedIdentity = $false
$script:cimFailed = $false
$script:acceptGracefulStop = $true
$script:requests = @()
$script:forced = @()
$script:exitBeforeOpen = $false
function Get-PrivacyShieldCameraAvailability { $script:cameraProbes++; throw 'hardware enumeration must not run during Stop' }
function Get-PrivacyShieldPidFromMarker { return $null }
function Get-CimInstance {
    [CmdletBinding()] param($ClassName, $Filter)
    if ($script:cimFailed) { throw 'CIM unavailable' }
    if ($Filter -match '^ProcessId=(\d+)$') {
        $id = [int]$Matches[1]
        if ($script:changedIdentity) {
            return [pscustomobject]@{ Name='python.exe'; SessionId=$session; ProcessId=$id; CommandLine='unrelated.py' }
        }
        return @($script:items | Where-Object { $_.ProcessId -eq $id })
    }
    return $script:items
}
function Stop-Process {
    [CmdletBinding()] param($Id, [switch]$Force)
    $script:stopped += $Id
    $script:items = @($script:items | Where-Object { $_.ProcessId -ne $Id })
}
function Wait-Process { [CmdletBinding()] param($Id, $Timeout) }
function Get-Process {
    [CmdletBinding()] param($Id)
    if ($script:exitBeforeOpen) {
        $script:items = @($script:items | Where-Object { $_.ProcessId -ne $Id })
        throw 'detector exited before handle opened'
    }
    $process = [pscustomobject]@{ Id=$Id; Handle=1 }
    $process | Add-Member ScriptMethod WaitForExit { param($Timeout) return @($script:items | Where-Object { $_.ProcessId -eq $this.Id }).Count -eq 0 }
    $process | Add-Member ScriptMethod Kill { $script:forced += $this.Id; Stop-Process -Id $this.Id }
    $process | Add-Member ScriptMethod Dispose { }
    return $process
}
function Request-PrivacyShieldGracefulStop {
    param($DetectorPid)
    $script:requests += $DetectorPid
    if ($script:acceptGracefulStop) { Stop-Process -Id $DetectorPid }
}

$result = Stop-PrivacyShield
if ($result.success -ne $true -or $result.running -ne $false -or $result.shieldLifecycle -ne 'stopped') { throw 'verified Stop receipt missing' }
if (($script:stopped -join ',') -ne '9001') { throw 'Stop touched an unrelated process/session' }
if ($script:cameraProbes -ne 0) { throw 'Stop performed unnecessary camera enumeration' }
if (($script:requests -join ',') -ne '9001' -or $script:forced.Count -ne 0) { throw 'normal Stop did not allow recording flush' }

$script:acceptGracefulStop = $false
$script:items += [pscustomobject]@{ Name='python.exe'; SessionId=$session; ProcessId=9005; CommandLine='--wc-privacy-shield' }
$fallback = Stop-PrivacyShield
if ($fallback.success -ne $true -or ($script:forced -join ',') -ne '9005') { throw 'stalled detector fallback did not stop its retained process' }

$script:items += [pscustomobject]@{ Name='python.exe'; SessionId=$session; ProcessId=9004; CommandLine='--wc-privacy-shield' }
$script:changedIdentity = $true
$changed = Stop-PrivacyShield
if ($changed.error -ne $true -or $script:stopped -contains 9004) { throw 'changed process identity was not rejected' }
$script:cimFailed = $true
$unknown = Stop-PrivacyShield
if ($unknown.error -ne $true) { throw 'unknown process state must not claim a verified Stop' }

$script:cimFailed = $false
$script:changedIdentity = $false
$script:items = @([pscustomobject]@{ Name='python.exe'; SessionId=$session; ProcessId=9006; CommandLine='--wc-privacy-shield' })
$script:exitBeforeOpen = $true
$exitedDuringStop = Stop-PrivacyShield
$script:exitBeforeOpen = $false
if ($exitedDuringStop.success -ne $true -or $script:forced -contains 9006) { throw 'already-exited detector was falsely reported as Stop failure' }

& {
    $script:cimFailed = $false
    $script:ready = $false
    $script:items = @([pscustomobject]@{ Name='python.exe'; SessionId=$session; ProcessId=9010; CommandLine='--wc-privacy-shield --run-id 0123456789abcdef0123456789abcdef' })
    function Get-PrivacyShieldLaunchLogTail {
        param($Path, $RunId)
        if ($script:ready) { return "[now] [$RunId] Status: Shield Active" }
        return ''
    }
    $warming = Get-PrivacyShieldStatus
    if ($warming.running -ne $false -or $warming.detectorProcessRunning -ne $true -or $warming.shieldLifecycle -ne 'starting') {
        throw 'a live warming-up process was reported as active protection'
    }
    $script:ready = $true
    $active = Get-PrivacyShieldStatus
    if ($active.running -ne $true -or $active.shieldLifecycle -ne 'running' -or $active.cameraStatus -ne 'available') {
        throw 'verified detector readiness was not reflected immediately'
    }
    if ($script:cameraProbes -ne 0) { throw 'active detector status unnecessarily enumerated hardware' }
}

$log = Join-Path ([IO.Path]::GetTempPath()) ('wc-shield-ready-' + [guid]::NewGuid().ToString('N') + '.log')
$runId = '0123456789abcdef0123456789abcdef'
$oldRun = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
try {
    [IO.File]::WriteAllText($log, "[now] [$oldRun] Status: Shield Active`r`n")
    $live = [pscustomobject]@{ HasExited = $false }
    if (Wait-PrivacyShieldReady $live $log $runId 120) { throw 'an older run supplied readiness' }
    [IO.File]::AppendAllText($log, "[now] [$runId] Shield worker started`r`n")
    if (Wait-PrivacyShieldReady $live $log $runId 120) { throw 'process startup alone supplied readiness' }
    [IO.File]::AppendAllText($log, "[now] [$runId] Status: Shield Active`r`n")
    $timer = [Diagnostics.Stopwatch]::StartNew()
    if (-not (Wait-PrivacyShieldReady $live $log $runId 1000)) { throw 'own detector readiness was not observed' }
    if ($timer.ElapsedMilliseconds -ge 500) { throw 'ready detector still paid a fixed startup delay' }
    if (Wait-PrivacyShieldReady ([pscustomobject]@{ HasExited=$true }) $log $runId 120) { throw 'exited detector was reported ready' }
} finally {
    if (Test-Path -LiteralPath $log) { Remove-Item -LiteralPath $log -Force }
}
Write-Output 'PASS: verified Stop, no camera enumeration, process/session isolation, readiness ownership and timeout'
