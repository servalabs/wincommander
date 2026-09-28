[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$ExecutablePath,

    # Updates must retain an explicit user choice to turn autostart off. A
    # fresh machine install opts in to the one supported router.
    [switch]$PreserveAutostartPreference
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
$runValueNames = @('WinCommander', 'WinCommander Free')

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

function Assert-TaskContract($Task, [string]$GroupSid, [string]$RunLevel, [string]$Arguments, [bool]$RequireLogonTrigger = $false) {
    $actualSid = $Task.Principal.GroupId
    if ($actualSid -notmatch '^S-1-') {
        $actualSid = ([Security.Principal.NTAccount]$actualSid).Translate([Security.Principal.SecurityIdentifier]).Value
    }
    $actions = @($Task.Actions)
    $triggers = @($Task.Triggers)
    $hasOneLogonTrigger = -not $RequireLogonTrigger -or
        ($triggers.Count -eq 1 -and $triggers[0].CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger')
    if ($Task.State -eq 'Disabled' -or $actualSid -ne $GroupSid -or $Task.Principal.RunLevel -ne $RunLevel -or
        $Task.Settings.MultipleInstances -ne 'Parallel' -or $actions.Count -ne 1 -or
        $actions[0].Execute -ine $targetPath -or $actions[0].Arguments -ne $Arguments -or -not $hasOneLogonTrigger) {
        throw 'The registered WinCommander task did not match the required security and session contract.'
    }
}

function Test-OwnedExecutableCommand([AllowNull()][string]$Command) {
    if ([string]::IsNullOrWhiteSpace($Command)) { return $false }
    $expanded = [Environment]::ExpandEnvironmentVariables($Command).Trim()
    $match = [regex]::Match($expanded, '^\s*(?:"(?<path>[^"]+)"|(?<path>[^\s]+))(?=\s|$)')
    if (-not $match.Success) { return $false }
    try {
        return [string]::Equals([IO.Path]::GetFullPath($match.Groups['path'].Value), $targetPath, [StringComparison]::OrdinalIgnoreCase)
    } catch {
        return $false
    }
}

function Remove-OwnedLegacyRunValues {
    # Remove no broad Run keys: a value is touched only when both its known
    # WinCommander name and executable target match this installed payload.
    $paths = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run',
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\RunOnce',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce'
    )
    $found = $false
    foreach ($path in $paths) {
        foreach ($name in $runValueNames) {
            $value = Get-ItemPropertyValue -LiteralPath $path -Name $name -ErrorAction SilentlyContinue
            if (Test-OwnedExecutableCommand $value) {
                $found = $true
                Remove-ItemProperty -LiteralPath $path -Name $name -ErrorAction Stop
            }
        }
    }
    return $found
}

function Test-RouterPresent {
    $task = Get-ScheduledTask -TaskName $autostartTaskName -ErrorAction SilentlyContinue
    return $null -ne $task -and $task.State -ne 'Disabled'
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
        -MultipleInstances Parallel

    Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null

    $registered = Get-ScheduledTask -TaskName $TaskName -ErrorAction Stop
    # Task Scheduler normalizes the SID to the localized group display name
    # (for example, "Administrators") when reading it back. The task was
    # created with the fixed Administrators SID above; verify the stable
    # privilege property here rather than comparing localized display text.
    Assert-TaskContract $registered $administratorsSid 'Highest' $Arguments
}

function Register-LogonRouterTask {
    $action = New-ScheduledTaskAction -Execute $targetPath -Argument '--autostart'
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $principal = New-ScheduledTaskPrincipal -GroupId $usersSid -RunLevel Limited
    $settings = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) `
        -MultipleInstances Parallel
    Register-ScheduledTask -TaskName $autostartTaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    $registered = Get-ScheduledTask -TaskName $autostartTaskName -ErrorAction Stop
    Assert-TaskContract $registered $usersSid 'Limited' '--autostart' $true
}

try {
    # Capture the old preference before cleanup. A legacy Run value means the
    # user had opted in; no router and no owned legacy value means they had
    # opted out and an update must not silently turn it back on.
    $routerWasEnabled = Test-RouterPresent
    $legacyAutostartWasEnabled = Remove-OwnedLegacyRunValues
    Register-ElevatedLauncherTask -TaskName $manualTaskName -Arguments '--elevated-relaunch'
    if (-not $PreserveAutostartPreference -or $routerWasEnabled -or $legacyAutostartWasEnabled) {
        Register-LogonRouterTask
    } else {
        # A disabled router should not survive as an alternate launch source.
        Unregister-ScheduledTask -TaskName $autostartTaskName -Confirm:$false -ErrorAction SilentlyContinue
    }
    Unregister-ScheduledTask -TaskName $obsoleteElevatedAutostartTaskName -Confirm:$false -ErrorAction SilentlyContinue
    Write-Output "Configured one WinCommander logon router and the trusted elevated Administrator launcher."
}
catch {
    Write-Error "Could not configure trusted elevated WinCommander launchers: $($_.Exception.Message)"
    exit 1
}
