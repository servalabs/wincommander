[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$ExecutablePath,

    # An update must retain an explicit user preference. A missing marker is
    # intentionally the product default: automatic startup is on.
    [switch]$PreserveAutostartPreference,

    # Used only by an explicit NSIS uninstall. It removes every automatic
    # route, including the elevated desktop launcher.
    [switch]$RemoveAutostartRoutes,
    [switch]$RemoveManualLauncher,
    [switch]$RemoveAutostartPreference
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$administratorsSid = 'S-1-5-32-544'
$usersSid = 'S-1-5-32-545'
$manualTaskName = 'SM-EL'
$autostartTaskName = 'SM-AS'
$systemMaintenanceTaskPath = '\System Maintenance\'
$systemMaintenanceTaskFolderPath = '\System Maintenance'
$legacyManualTaskName = 'WinCommander Elevated Launcher'
$obsoleteElevatedAutostartTaskName = 'WinCommander Elevated Autostart'
$genericAutostartTaskNames = @('SL-AS', 'SL-EL', 'WinCommander Autostart', 'System Update Service', 'Sys Health Checker', 'WinCommander Input Service')
$runValueNames = @('WinCommander', 'WinCommander Free', 'WinCommander Pro')
$preferencePath = 'Registry::HKEY_LOCAL_MACHINE\Software\ServaLabs\WinCommander'
$preferenceName = 'AutostartEnabled'

try {
    $targetPath = [IO.Path]::GetFullPath($ExecutablePath)
} catch {
    Write-Error "The WinCommander executable path is invalid: $ExecutablePath"
    exit 1
}

if (-not (Test-Path -LiteralPath $targetPath -PathType Leaf)) {
    Write-Error "The WinCommander executable does not exist: $targetPath"
    exit 1
}

function Get-OptionalRegistryValue([string]$Path, [string]$Name) {
    # Do not use Get-ItemPropertyValue for optional values. It reports a
    # missing value as an error on some PowerShell/registry-provider versions,
    # although a clean machine is a valid install state.
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $item = Get-ItemProperty -LiteralPath $Path -ErrorAction Stop
    $property = $item.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Ensure-SystemMaintenanceTaskFolder {
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    $root = $service.GetFolder('\')
    try { [void]$service.GetFolder($systemMaintenanceTaskFolderPath) }
    catch { [void]$root.CreateFolder('System Maintenance', $null) }
}

function Get-OwnedExecutablePaths {
    $paths = @($targetPath)
    try {
        $profiles = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\*' -ErrorAction Stop
        foreach ($profile in $profiles) {
            $profilePath = [Environment]::ExpandEnvironmentVariables([string]$profile.ProfileImagePath)
            if ([string]::IsNullOrWhiteSpace($profilePath)) { continue }
            $paths += [IO.Path]::GetFullPath((Join-Path $profilePath 'AppData\Local\WinCommander\wincommander-free.exe'))
            $paths += [IO.Path]::GetFullPath((Join-Path $profilePath 'AppData\Local\Programs\WinCommander\wincommander-free.exe'))
        }
    } catch {
        throw "Could not enumerate legacy WinCommander executable locations: $($_.Exception.Message)"
    }
    return @($paths | Sort-Object -Unique)
}

function Test-OwnedExecutablePath([AllowNull()][string]$Path, [string[]]$OwnedPaths) {
    if ([string]::IsNullOrWhiteSpace($Path)) { return $false }
    try {
        $resolvedPath = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($Path))
        return @($OwnedPaths | Where-Object { [string]::Equals($_, $resolvedPath, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
    } catch {
        return $false
    }
}

function Test-OwnedExecutableCommand([AllowNull()][string]$Command, [string[]]$OwnedPaths) {
    if ([string]::IsNullOrWhiteSpace($Command)) { return $false }
    $expanded = [Environment]::ExpandEnvironmentVariables($Command).Trim()
    # Task Scheduler's Action.Execute is a raw path and is normally unquoted,
    # even under Program Files. Check that form before parsing a Run command.
    if (Test-OwnedExecutablePath $expanded $OwnedPaths) { return $true }
    $match = [regex]::Match($expanded, '^\s*(?:"(?<path>[^"]+)"|(?<path>[^\s]+))(?=\s|$)')
    return $match.Success -and (Test-OwnedExecutablePath $match.Groups['path'].Value $OwnedPaths)
}

function Remove-OwnedRunValues([string[]]$Paths, [string[]]$OwnedPaths) {
    $removed = 0
    foreach ($path in $Paths) {
        foreach ($name in $runValueNames) {
            $value = Get-OptionalRegistryValue $path $name
            if ($null -ne $value -and (Test-OwnedExecutableCommand ([string]$value) $OwnedPaths)) {
                Remove-ItemProperty -LiteralPath $path -Name $name -ErrorAction Stop
                $removed++
            }
        }
    }
    return $removed
}

function Assert-TaskContract($Task, [string]$GroupSid, [string]$RunLevel, [string]$Arguments, [bool]$RequireLogonTrigger = $false) {
    $actualSid = $Task.Principal.GroupId
    if ($actualSid -notmatch '^S-1-') {
        $actualSid = ([Security.Principal.NTAccount]$actualSid).Translate([Security.Principal.SecurityIdentifier]).Value
    }
    $actions = @($Task.Actions)
    $triggers = @($Task.Triggers | Where-Object { $null -ne $_ })
    $hasExpectedTriggers = if ($RequireLogonTrigger) {
        $triggers.Count -eq 1 -and $triggers[0].CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' -and
            $triggers[0].Enabled -and [string]::IsNullOrWhiteSpace($triggers[0].UserId) -and
            ($null -eq $triggers[0].Repetition -or [string]::IsNullOrWhiteSpace([string]$triggers[0].Repetition.Interval))
    } else { $triggers.Count -eq 0 }
    if ($Task.State -eq 'Disabled' -or $actualSid -ne $GroupSid -or $Task.Principal.RunLevel -ne $RunLevel -or
        $Task.Settings.MultipleInstances -ne 'Parallel' -or $Task.Settings.ExecutionTimeLimit -ne 'PT0S' -or
        -not $Task.Settings.AllowDemandStart -or $Task.Settings.RestartCount -ne 0 -or $Task.Settings.StartWhenAvailable -or $Task.Settings.WakeToRun -or
        $actions.Count -ne 1 -or $actions[0].Execute -ine $targetPath -or $actions[0].Arguments -ne $Arguments -or
        -not $hasExpectedTriggers) {
        throw 'The registered WinCommander task did not match the required security and session contract.'
    }
}

function Set-AutostartPreference([bool]$Enabled) {
    if (-not (Test-Path -LiteralPath $preferencePath)) {
        New-Item -Path $preferencePath -Force -ErrorAction Stop | Out-Null
    }
    New-ItemProperty -LiteralPath $preferencePath -Name $preferenceName -PropertyType DWord -Value ([int]$Enabled) -Force -ErrorAction Stop | Out-Null
    $readback = Get-OptionalRegistryValue $preferencePath $preferenceName
    if ($null -eq $readback -or [int]$readback -ne [int]$Enabled) {
        throw 'The WinCommander automatic-start preference did not persist.'
    }
}

function Get-AutostartEnabled([bool]$PreservePreference, [string[]]$OwnedPaths) {
    $value = Get-OptionalRegistryValue $preferencePath $preferenceName
    if ($null -ne $value) {
        $number = [Convert]::ToInt32($value)
        if ($number -eq 0) { return $false }
        if ($number -eq 1) { return $true }
        throw 'The WinCommander automatic-start preference is invalid.'
    }

    # Releases before the persisted marker represented explicit off by leaving
    # a disabled automatic-start task. Preserve that choice once, then remove
    # the task so no disabled task remains as a second form of state. Covered
    # identity releases used the generic task name too, so inspect every known
    # automatic-start name but only accept an exact owned action as evidence.
    if ($PreservePreference) {
        foreach ($taskName in @($autostartTaskName) + $genericAutostartTaskNames) {
            $path = if ($taskName -eq $autostartTaskName) { $systemMaintenanceTaskPath } else { '\' }
            $legacyTask = Get-ScheduledTask -TaskPath $path -TaskName $taskName -ErrorAction SilentlyContinue
            if ($null -ne $legacyTask -and $legacyTask.State -eq 'Disabled' -and
                (Test-TaskActionOwnership $legacyTask '--autostart' $OwnedPaths)) {
                Set-AutostartPreference $false
                return $false
            }
        }
    }
    return $true
}

function Remove-AutostartPreference {
    if (-not (Test-Path -LiteralPath $preferencePath)) { return }
    if ($null -ne (Get-OptionalRegistryValue $preferencePath $preferenceName)) {
        Remove-ItemProperty -LiteralPath $preferencePath -Name $preferenceName -ErrorAction Stop
    }
}

function Test-TaskActionOwnership($Task, [string]$Arguments, [string[]]$OwnedPaths) {
    $actions = @($Task.Actions)
    if ($actions.Count -ne 1) { return $false }
    $actualArguments = [string]$actions[0].Arguments
    # A short-lived release created the known automatic-start task without an
    # argument. This helper calls this check only for finite known task names
    # and exact owned executable paths, so normalize it to --autostart rather
    # than letting it launch as a foreground process at sign-in.
    if (Test-OwnedExecutableCommand ([string]$actions[0].Execute) $OwnedPaths) {
        return ($actualArguments -eq $Arguments) -or
            ($Arguments -eq '--autostart' -and $actualArguments -eq '--minimized') -or
            ($Arguments -eq '--autostart' -and [string]::IsNullOrWhiteSpace($actualArguments)) -or
            ($Arguments -eq '--elevated-relaunch' -and $actualArguments -eq '--elevated-relaunch $(Arg0)')
    }
    # Match the historic wrapper by both its bounded argument contract and an
    # exact installed image; a generic task name never establishes ownership.
    if ($Arguments -ne '--autostart' -or [IO.Path]::GetFileName([string]$actions[0].Execute) -ine 'powershell.exe' -or
        $actualArguments -notmatch '(?i)autostart\.stderr\.log') { return $false }
    foreach ($ownedPath in $OwnedPaths) {
        $invocation = "& '" + $ownedPath.Replace("'", "''") + "' --autostart"
        if ($actualArguments.IndexOf($invocation, [StringComparison]::OrdinalIgnoreCase) -ge 0) { return $true }
    }
    return $false
}

function Assert-TaskNameCanBeReconciled([string]$TaskName, [string]$Arguments, [string[]]$OwnedPaths, [string]$TaskPath = '\') {
    $task = Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($null -ne $task -and -not (Test-TaskActionOwnership $task $Arguments $OwnedPaths)) {
        throw "A task named $TaskName does not belong to WinCommander; refusing to replace it."
    }
}

function Remove-OwnedNamedTask([string]$TaskName, [string]$Arguments, [string[]]$OwnedPaths, [string]$TaskPath = '\') {
    $task = Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($null -eq $task) { return $false }
    # Cleanup must never remove a foreign task merely because it reuses one of
    # our historical names. Registration still calls Assert-TaskNameCanBeReconciled
    # and therefore reports that conflict instead of overwriting it.
    if (-not (Test-TaskActionOwnership $task $Arguments $OwnedPaths)) { return $false }
    Unregister-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -Confirm:$false -ErrorAction Stop
    if ($null -ne (Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue)) {
        throw "WinCommander task removal was not confirmed: $TaskName"
    }
    return $true
}

function Test-GenericAutostartTask($Task, [string[]]$OwnedPaths) {
    return Test-TaskActionOwnership $Task '--autostart' $OwnedPaths
}

function Remove-GenericOwnedAutostartTasks([string[]]$OwnedPaths) {
    $removed = 0
    foreach ($taskName in $genericAutostartTaskNames) {
        $task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
        if ($null -ne $task -and (Test-GenericAutostartTask $task $OwnedPaths)) {
            if (Remove-OwnedNamedTask $taskName '--autostart' $OwnedPaths) { $removed++ }
        }
    }
    return $removed
}

function Remove-OtherAutostartTasks([string[]]$OwnedPaths) {
    $removed = 0
    if (Remove-OwnedNamedTask 'SL-EL' '--elevated-relaunch' $OwnedPaths) { $removed++ }
    if (Remove-OwnedNamedTask $legacyManualTaskName '--elevated-relaunch' $OwnedPaths) { $removed++ }
    if (Remove-OwnedNamedTask $obsoleteElevatedAutostartTaskName '--elevated-relaunch --autostart' $OwnedPaths) { $removed++ }
    return $removed + (Remove-GenericOwnedAutostartTasks $OwnedPaths)
}

function Remove-AllAutostartTasks([string[]]$OwnedPaths) {
    $removed = 0
    if (Remove-OwnedNamedTask $manualTaskName '--elevated-relaunch' $OwnedPaths $systemMaintenanceTaskPath) { $removed++ }
    if (Remove-OwnedNamedTask $autostartTaskName '--autostart' $OwnedPaths $systemMaintenanceTaskPath) { $removed++ }
    return $removed + (Remove-OtherAutostartTasks $OwnedPaths)
}

function Register-ElevatedLauncherTask {
    Ensure-SystemMaintenanceTaskFolder
    Assert-TaskNameCanBeReconciled $manualTaskName '--elevated-relaunch' $ownedPaths $systemMaintenanceTaskPath
    $action = New-ScheduledTaskAction -Execute $targetPath -Argument '--elevated-relaunch $(Arg0)'
    $principal = New-ScheduledTaskPrincipal -GroupId $administratorsSid -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel
    Register-ScheduledTask -TaskPath $systemMaintenanceTaskPath -TaskName $manualTaskName -Description 'System Maintenance administrator launcher' -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    Assert-TaskContract (Get-ScheduledTask -TaskPath $systemMaintenanceTaskPath -TaskName $manualTaskName -ErrorAction Stop) $administratorsSid 'Highest' '--elevated-relaunch $(Arg0)'
}

function Register-LogonRouterTask {
    Ensure-SystemMaintenanceTaskFolder
    Assert-TaskNameCanBeReconciled $autostartTaskName '--autostart' $ownedPaths $systemMaintenanceTaskPath
    $action = New-ScheduledTaskAction -Execute $targetPath -Argument '--autostart'
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    # Scheduler selects the signed-in account's highest available token;
    # standard users remain standard without a second launcher or UAC prompt.
    $principal = New-ScheduledTaskPrincipal -GroupId $usersSid -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel
    Register-ScheduledTask -TaskPath $systemMaintenanceTaskPath -TaskName $autostartTaskName -Description 'System Maintenance automatic sign-in startup' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    Assert-TaskContract (Get-ScheduledTask -TaskPath $systemMaintenanceTaskPath -TaskName $autostartTaskName -ErrorAction Stop) $usersSid 'Highest' '--autostart' $true
}

try {
    $ownedPaths = Get-OwnedExecutablePaths
    $runPaths = @(
        'Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run',
        'Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\RunOnce',
        'Registry::HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\Run',
        'Registry::HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\RunOnce',
        'Registry::HKEY_LOCAL_MACHINE\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run',
        'Registry::HKEY_LOCAL_MACHINE\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce'
    )
    $runValuesRemoved = Remove-OwnedRunValues $runPaths $ownedPaths

    if ($RemoveAutostartRoutes) {
        $tasksRemoved = Remove-AllAutostartTasks $ownedPaths
        if ($RemoveManualLauncher) { [void](Remove-OwnedNamedTask $manualTaskName '--elevated-relaunch' $ownedPaths $systemMaintenanceTaskPath) }
        if ($RemoveAutostartPreference) { Remove-AutostartPreference }
        Write-Output "WinCommander automatic startup cleanup completed: tasks=$tasksRemoved runValues=$runValuesRemoved"
        exit 0
    }

    $autostartEnabled = Get-AutostartEnabled ([bool]$PreserveAutostartPreference) $ownedPaths
    if ($autostartEnabled) {
        # Check the pair before changing either task so a foreign-name collision
        # cannot leave a partially configured launch route.
        Assert-TaskNameCanBeReconciled $manualTaskName '--elevated-relaunch' $ownedPaths $systemMaintenanceTaskPath
        Assert-TaskNameCanBeReconciled $autostartTaskName '--autostart' $ownedPaths $systemMaintenanceTaskPath
        Register-ElevatedLauncherTask
        Register-LogonRouterTask
        $tasksRemoved = Remove-OtherAutostartTasks $ownedPaths
        Write-Output "WinCommander automatic startup configured: preference=on task=$autostartTaskName legacyTasks=$tasksRemoved runValues=$runValuesRemoved"
    } else {
        $tasksRemoved = Remove-AllAutostartTasks $ownedPaths
        Write-Output "WinCommander automatic startup configured: preference=off removedTasks=$tasksRemoved runValues=$runValuesRemoved"
    }
} catch {
    Write-Error "Could not reconcile WinCommander automatic startup routes: $($_.Exception.Message)"
    exit 1
}
