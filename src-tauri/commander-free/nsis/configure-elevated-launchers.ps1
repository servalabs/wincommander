[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$ExecutablePath
)

# UAC correctly prevents a normal process from silently making itself elevated.
# These two installer-owned tasks are the trusted Windows boundary instead:
# only an Administrators-group principal can run them, and Task Scheduler gives
# that principal its highest available token. Standard users cannot use them
# and remain on the normal ShellExecute("runas") consent/credential path.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$administratorsSid = 'S-1-5-32-544'
$manualTaskName = 'WinCommander Elevated Launcher'
$autostartTaskName = 'WinCommander Elevated Autostart'

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
        [string]$Arguments,

        [switch]$AtLogon
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

    if ($AtLogon) {
        $trigger = New-ScheduledTaskTrigger -AtLogOn
        Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    }
    else {
        Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    }

    $registered = Get-ScheduledTask -TaskName $TaskName -ErrorAction Stop
    # Task Scheduler normalizes the SID to the localized group display name
    # (for example, "Administrators") when reading it back. The task was
    # created with the fixed Administrators SID above; verify the stable
    # privilege property here rather than comparing localized display text.
    if ($registered.Principal.RunLevel -ne 'Highest') {
        throw "Task $TaskName was not registered with the required Highest security context."
    }
}

try {
    Register-ElevatedLauncherTask -TaskName $manualTaskName -Arguments '--elevated-relaunch'
    Register-ElevatedLauncherTask -TaskName $autostartTaskName -Arguments '--elevated-relaunch --autostart' -AtLogon
    Write-Output "Configured trusted elevated WinCommander launchers for Administrators."
}
catch {
    Write-Error "Could not configure trusted elevated WinCommander launchers: $($_.Exception.Message)"
    exit 1
}
