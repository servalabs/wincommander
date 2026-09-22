[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$ExecutablePath
)

# UAC correctly prevents a normal process from silently making itself elevated.
# The Administrators-only task below is the trusted Windows elevation boundary.
# A separate single, limited Users-group logon task routes every interactive
# session: Administrator sessions start this trusted task before a window is
# created, while standard users retain one normal process. Do not install a
# second elevated logon trigger because it races the router at logon.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$administratorsSid = 'S-1-5-32-544'
$usersSid = 'S-1-5-32-545'
$manualTaskName = 'WinCommander Elevated Launcher'
$autostartTaskName = 'WinCommander Autostart'
$obsoleteElevatedAutostartTaskName = 'WinCommander Elevated Autostart'

try {
    $targetPath = [System.IO.Path]::GetFullPath($ExecutablePath)
}
catch {
    Write-Error "The WinCommander executable path is invalid: $ExecutablePath"
    exit 1
}

if (-not (Test-Path -LiteralPath $targetPath -PathType Leaf)) {
    Write-Error "The WinCommander executable does not exist: $targetPath"
    exit 1
}

function Register-ElevatedLauncherTask {
    param(
        [Parameter(Mandatory)]
        [string]$TaskName,

        [Parameter(Mandatory)]
        [string]$Arguments
    )

    $action = New-ScheduledTaskAction -Execute $targetPath -Argument $Arguments
    # Group principal means the interactive user must be an Administrator to
    # run the task. Highest is mandatory for an Administrators-group task.
    $principal = New-ScheduledTaskPrincipal -GroupId $administratorsSid -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) `
        -MultipleInstances IgnoreNew

    Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null

    $registered = Get-ScheduledTask -TaskName $TaskName -ErrorAction Stop
    # Task Scheduler normalizes the SID to the localized group display name
    # (for example, "Administrators") when reading it back. The task was
    # created with the fixed Administrators SID above; verify the stable
    # privilege property here rather than comparing localized display text.
    if ($registered.Principal.RunLevel -ne 'Highest') {
        throw "Task $TaskName was not registered with the required Highest security context."
    }
}

function Register-LogonRouterTask {
    $action = New-ScheduledTaskAction -Execute $targetPath -Argument '--autostart'
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $principal = New-ScheduledTaskPrincipal -GroupId $usersSid -RunLevel Limited
    $settings = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) `
        -MultipleInstances IgnoreNew
    Register-ScheduledTask -TaskName $autostartTaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    $registered = Get-ScheduledTask -TaskName $autostartTaskName -ErrorAction Stop
    if ($registered.Principal.RunLevel -ne 'Limited') {
        throw "Task $autostartTaskName was not registered with the required Limited security context."
    }
}

try {
    Register-ElevatedLauncherTask -TaskName $manualTaskName -Arguments '--elevated-relaunch'
    Register-LogonRouterTask
    Unregister-ScheduledTask -TaskName $obsoleteElevatedAutostartTaskName -Confirm:$false -ErrorAction SilentlyContinue
    Write-Output "Configured one WinCommander logon router and the trusted elevated Administrator launcher."
}
catch {
    Write-Error "Could not configure trusted elevated WinCommander launchers: $($_.Exception.Message)"
    exit 1
}
