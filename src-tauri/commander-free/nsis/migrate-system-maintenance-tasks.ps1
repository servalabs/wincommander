[CmdletBinding()]
param()

# Best-effort migration only. A scheduler failure must never block setup: the
# app continues to work and the next signed-in maintenance pass can retry.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$targetPath = '\System Maintenance\'
$targetFolderPath = '\System Maintenance'

function Ensure-TaskFolder {
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    $root = $service.GetFolder('\')
    try { [void]$service.GetFolder($targetFolderPath) }
    catch { [void]$root.CreateFolder('System Maintenance', $null) }
}

function Test-ExpectedTaskAction($Task, [string]$OldName) {
    $action = @($Task.Actions)[0]
    if ($null -eq $action) { return $false }
    $execute = [IO.Path]::GetFileName([string]$action.Execute)
    $arguments = [string]$action.Arguments
    return $execute -in @('powershell.exe', 'pwsh.exe', 'VeraCrypt.exe') -and
        ($arguments -match '(?i)WinCommander|VeraCrypt|ProgramData')
}

function Move-OwnedTask([string]$OldName, [string]$NewName) {
    $task = Get-ScheduledTask -TaskPath '\' -TaskName $OldName -ErrorAction SilentlyContinue
    if ($null -eq $task -or -not (Test-ExpectedTaskAction $task $OldName)) { return 'skipped' }
    if ([string]$task.State -eq 'Running') { return 'running' }
    $existing = Get-ScheduledTask -TaskPath $targetPath -TaskName $NewName -ErrorAction SilentlyContinue
    if ($existing) {
        if (-not (Test-ExpectedTaskAction $existing $OldName)) { return 'replacement-conflict' }
        Unregister-ScheduledTask -TaskPath '\' -TaskName $OldName -Confirm:$false -ErrorAction Stop
        return 'removed-legacy-duplicate'
    }
    $xml = Export-ScheduledTask -TaskPath '\' -TaskName $OldName -ErrorAction Stop
    Register-ScheduledTask -TaskPath $targetPath -TaskName $NewName -Xml $xml -Force -ErrorAction Stop | Out-Null
    $verified = Get-ScheduledTask -TaskPath $targetPath -TaskName $NewName -ErrorAction Stop
    if (-not (Test-ExpectedTaskAction $verified $OldName)) {
        Unregister-ScheduledTask -TaskPath $targetPath -TaskName $NewName -Confirm:$false -ErrorAction SilentlyContinue
        throw "replacement verification failed for $OldName"
    }
    Unregister-ScheduledTask -TaskPath '\' -TaskName $OldName -Confirm:$false -ErrorAction Stop
    'migrated'
}

try {
    Ensure-TaskFolder
    $map = [ordered]@{
        # Startup routes belong exclusively to configure-elevated-launchers.
        # Copying their old XML here could restore obsolete settings or OFF.
        'WinCommander Session Guard' = 'SM-SG';
        'WinCommander_AI_UpdateCleanup' = 'SM-UC';
        'WinCommanderShellPriorityLogon' = 'SM-SP';
        'Keep RDP Animation Effects' = 'SM-RA';
        'WinCommander_BtScan_System' = 'SM-BS';
        'System_DasWipe_System' = 'SM-BA'
    }
    $result = foreach ($entry in $map.GetEnumerator()) {
        [pscustomobject]@{ old = $entry.Key; new = $entry.Value; status = Move-OwnedTask $entry.Key $entry.Value }
    }
    foreach ($task in @(Get-ScheduledTask -TaskPath '\' -ErrorAction SilentlyContinue)) {
        if ($task.TaskName -match '^WinCommander_SSDOptimize_([A-Z])$') {
            [void](Move-OwnedTask $task.TaskName "SM-TR-$($Matches[1])")
        } elseif ($task.TaskName -match '^WinCommander-RdpIdleDismount-([A-F0-9-]+)$') {
            [void](Move-OwnedTask $task.TaskName "SM-RD-$($Matches[1])")
        }
    }
    "System Maintenance task migration: $($result | ConvertTo-Json -Compress)"
} catch {
    Write-Warning "System Maintenance task migration was deferred: $($_.Exception.Message)"
}

exit 0
