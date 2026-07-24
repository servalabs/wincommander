# ============================================================================
# TWEAKS - SCHEDULED TASKS MANAGER
# Wraps Get/Enable/Disable/Start/Stop/Unregister-ScheduledTask so the UI can
# browse the Task Scheduler tree, toggle individual tasks, run them on
# demand, or delete them.
# ============================================================================

function Get-AllScheduledTasks {
    Assert-IsAdmin
    try {
        $tasks = Get-ScheduledTask -ErrorAction SilentlyContinue
        $out = @()
        foreach ($t in $tasks) {
            $info = $null
            try { $info = Get-ScheduledTaskInfo -TaskName $t.TaskName -TaskPath $t.TaskPath -ErrorAction SilentlyContinue } catch {}
            $author = $t.Author
            # Mark Microsoft-shipped tasks (the bulk of them) so the UI can filter them.
            $isMicrosoft = $t.TaskPath -like "\Microsoft\*" -or $author -match "(?i)Microsoft"
            $out += [PSCustomObject]@{
                Name        = $t.TaskName
                Path        = $t.TaskPath
                State       = "$($t.State)"
                Description = $t.Description
                Author      = $author
                IsMicrosoft = $isMicrosoft
                LastRunTime = if ($info) { "$($info.LastRunTime)" } else { $null }
                NextRunTime = if ($info) { "$($info.NextRunTime)" } else { $null }
                LastResult  = if ($info) { $info.LastTaskResult } else { $null }
            }
        }
        # Sort: enabled first, then alphabetically by full path
        $out | Sort-Object @{Expression={$_.State -eq 'Disabled'}}, @{Expression={"$($_.Path)$($_.Name)"}}
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Disable-ScheduledTaskByPath {
    param([string]$Path, [string]$Name)
    Assert-IsAdmin
    try {
        Disable-ScheduledTask -TaskPath $Path -TaskName $Name -ErrorAction Stop | Out-Null
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-ScheduledTaskByPath {
    param([string]$Path, [string]$Name)
    Assert-IsAdmin
    try {
        Enable-ScheduledTask -TaskPath $Path -TaskName $Name -ErrorAction Stop | Out-Null
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Start-ScheduledTaskByPath {
    param([string]$Path, [string]$Name)
    Assert-IsAdmin
    try {
        Start-ScheduledTask -TaskPath $Path -TaskName $Name -ErrorAction Stop
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Stop-ScheduledTaskByPath {
    param([string]$Path, [string]$Name)
    Assert-IsAdmin
    try {
        Stop-ScheduledTask -TaskPath $Path -TaskName $Name -ErrorAction Stop
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

function Remove-ScheduledTaskByPath {
    param([string]$Path, [string]$Name)
    Assert-IsAdmin
    try {
        Unregister-ScheduledTask -TaskPath $Path -TaskName $Name -Confirm:$false -ErrorAction Stop
        @{ success = $true }
    } catch { @{ error = $true; message = $_.Exception.Message } }
}

# CL-03: Remove all WinCommander_AutoErase_* tasks the app registered. Also
# removes legacy System_AutoErase_* tasks that may still exist on machines
# that haven't run Invoke-AutoEraseMigration yet. Called from the Lockdown
# cascade so that a lockdown run leaves no persistent scheduled re-erasers.
function Remove-AutoEraseTasks {
    Assert-IsAdmin
    try {
        $tasks = Get-ScheduledTask -ErrorAction SilentlyContinue |
            Where-Object { $_.TaskName -like 'WinCommander_AutoErase_*' -or $_.TaskName -like 'System_AutoErase_*' }
        $removed = 0
        foreach ($t in $tasks) {
            try {
                Unregister-ScheduledTask -TaskName $t.TaskName -TaskPath $t.TaskPath `
                    -Confirm:$false -ErrorAction Stop
                $removed++
            } catch {}
        }
        @{ success = $true; removed = $removed }
    } catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}
