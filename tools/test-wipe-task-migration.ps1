# Opt-in real Task Scheduler round-trip. No cleanup action is executed.
[CmdletBinding()]
param([switch]$UseTaskScheduler)
$ErrorActionPreference = 'Stop'
if (-not $UseTaskScheduler) { throw 'Use -UseTaskScheduler to allow disposable tasks in an isolated test folder.' }
. (Join-Path $PSScriptRoot '..\src-tauri\wincmd-shared\scripts\auto-erase.ps1')
Assert-AutoEraseAdmin
Import-Module ScheduledTasks -ErrorAction Stop

$probeId = [guid]::NewGuid().ToString('N')
$script:probePath = "\WinCommanderMigrationProbe-$probeId\"
$script:allowedNames = @{}
$script:corruptReadback = $false
$scheduler = New-Object -ComObject Schedule.Service
$scheduler.Connect()
$root = $scheduler.GetFolder('\')
[void]$root.CreateFolder($script:probePath.Trim('\'))

function Assert-ProbeName([string]$Name) {
    if (-not $script:allowedNames.ContainsKey($Name)) { throw 'Migration escaped the disposable task allowlist.' }
}
function Get-ScheduledTask {
    param($TaskName, $TaskPath, $ErrorAction)
    $args = @{ TaskPath = $script:probePath; ErrorAction = 'Stop' }
    if ($TaskName) {
        Assert-ProbeName $TaskName
        $args.TaskName = $TaskName
        $args.ErrorAction = 'SilentlyContinue'
    }
    foreach ($task in @(ScheduledTasks\Get-ScheduledTask @args)) {
        # Production ownership is root-scoped. Project only this isolated folder.
        [pscustomobject]@{
            TaskName = $task.TaskName; TaskPath = '\'; Description = $task.Description
            State = $task.State; Principal = $task.Principal; Actions = $task.Actions; Triggers = $task.Triggers
        }
    }
}
function Export-ScheduledTask {
    param($TaskName, $TaskPath, $ErrorAction)
    Assert-ProbeName $TaskName
    [xml]$xml = ScheduledTasks\Export-ScheduledTask -TaskPath $script:probePath -TaskName $TaskName -ErrorAction Stop
    if ($script:corruptReadback -and $TaskName.StartsWith('SL-')) {
        $xml.Task.Principals.Principal.UserId = 'S-1-5-19'
    }
    $xml.OuterXml
}
function Register-ScheduledTask {
    param($TaskName, $TaskPath, $Xml, [switch]$Force, $ErrorAction)
    Assert-ProbeName $TaskName
    ScheduledTasks\Register-ScheduledTask -TaskPath $script:probePath -TaskName $TaskName -Xml $Xml -Force:$Force -ErrorAction Stop
}
function Unregister-ScheduledTask {
    param($TaskName, $TaskPath, $Confirm, $ErrorAction)
    Assert-ProbeName $TaskName
    ScheduledTasks\Unregister-ScheduledTask -TaskPath $script:probePath -TaskName $TaskName -Confirm:$false -ErrorAction Stop
}
function Enable-ScheduledTask {
    param($TaskName, $TaskPath, $ErrorAction)
    Assert-ProbeName $TaskName
    ScheduledTasks\Enable-ScheduledTask -TaskPath $script:probePath -TaskName $TaskName -ErrorAction Stop
}
function Start-ScheduledTask { throw 'The migration test must never execute a task.' }
function ConvertTo-AutoEraseTaskArgument { throw 'The current test payload must not be rewritten.' }

try {
    $currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    foreach ($case in @(
        @{ Owner = $currentSid; Disabled = $false; RejectReadback = $false },
        @{ Owner = 'S-1-5-18'; Disabled = $true; RejectReadback = $false },
        @{ Owner = 'S-1-5-18'; Disabled = $false; RejectReadback = $false },
        @{ Owner = $currentSid; Disabled = $false; RejectReadback = $true }
    )) {
        Write-Host "Checking Windows task: disabled=$($case.Disabled), systemOwner=$($case.Owner -eq 'S-1-5-18')"
        $oldName = "WinCommander_AutoErase_clipboard_$probeId"
        $newName = Get-AutoEraseTaskName 'clipboard' $case.Owner
        $script:allowedNames = @{ $oldName = $true; $newName = $true }
        $script:corruptReadback = $case.RejectReadback
        $inertPath = Join-Path $env:ProgramData "WinCommander\auto-erase\scripts\clipboard.probe-$probeId.ps1"
        if (Test-Path -LiteralPath $inertPath) { throw 'Unexpected test action file exists.' }
        # The target does not exist and the only trigger is far in the future.
        $action = New-ScheduledTaskAction -Execute "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument "-NoProfile -NonInteractive -WindowStyle Hidden -File `"$inertPath`""
        $trigger = New-ScheduledTaskTrigger -Once -At ([datetime]'2099-01-01T00:00:00') -RepetitionInterval (New-TimeSpan -Minutes 17) -RepetitionDuration (New-TimeSpan -Days 3650)
        $logon = if ($case.Owner -eq 'S-1-5-18') { 'ServiceAccount' } else { 'S4U' }
        $principal = New-ScheduledTaskPrincipal -UserId $case.Owner -LogonType $logon -RunLevel Highest
        $settings = New-ScheduledTaskSettingsSet -Hidden -Disable:$case.Disabled
        ScheduledTasks\Register-ScheduledTask -TaskPath $script:probePath -TaskName $oldName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description 'WinCommander scheduled wipe v2' | Out-Null
        [xml]$before = Export-ScheduledTask -TaskName $oldName
        if (-not $case.Disabled -and $before.Task.Settings.Enabled) { throw 'Fixture did not reproduce omitted Enabled=true.' }
        $result = Invoke-AutoEraseMigration
        if ($case.RejectReadback) {
            if (-not $result.error -or $result.message -notlike '*Principals*') { throw 'Unexpected readback was not rejected.' }
            $remaining = @(ScheduledTasks\Get-ScheduledTask -TaskPath $script:probePath)
            if ($remaining.Count -ne 1 -or $remaining[0].TaskName -ne $oldName) { throw 'Rollback lost the original or left a duplicate.' }
            $restored = ConvertTo-AutoEraseComparableTaskXml ([xml](Export-ScheduledTask -TaskName $oldName))
            $original = ConvertTo-AutoEraseComparableTaskXml $before
            foreach ($section in @('Actions', 'Triggers', 'Principals', 'Settings')) {
                if ($restored.Task.$section.OuterXml -ne $original.Task.$section.OuterXml) { throw "Rollback changed $section." }
            }
            $info = ScheduledTasks\Get-ScheduledTaskInfo -TaskPath $script:probePath -TaskName $oldName
            if ($info.LastRunTime.Year -gt 2000) { throw 'Rollback unexpectedly executed a task.' }
            Unregister-ScheduledTask -TaskName $oldName
            continue
        }
        if ($result.error) { throw $result.message }
        $remaining = @(ScheduledTasks\Get-ScheduledTask -TaskPath $script:probePath)
        if ($remaining.Count -ne 1 -or $remaining[0].TaskName -ne $newName) { throw 'Migration left duplicate or missing tasks.' }
        if (($remaining[0].State -eq 'Disabled') -ne $case.Disabled) { throw 'Migration changed enabled state.' }
        [xml]$after = Export-ScheduledTask -TaskName $newName
        $expected = ConvertTo-AutoEraseComparableTaskXml $before
        $actual = ConvertTo-AutoEraseComparableTaskXml $after
        foreach ($section in @('Actions', 'Triggers', 'Principals', 'Settings')) {
            if ($expected.Task.$section.OuterXml -ne $actual.Task.$section.OuterXml) { throw "Migration changed $section." }
        }
        $info = ScheduledTasks\Get-ScheduledTaskInfo -TaskPath $script:probePath -TaskName $newName
        if ($info.LastRunTime.Year -gt 2000) { throw 'Disposable task unexpectedly executed.' }
        $repeat = Invoke-AutoEraseMigration
        if ($repeat.error -or @($repeat.migrated).Count -ne 0) { throw 'Repeat migration was not a no-op.' }
        Unregister-ScheduledTask -TaskName $newName
    }
    'PASS: real Windows round-trip; omitted Enabled, user and SYSTEM owners, disabled state, schedules/actions, no execution, repeat no-op, rollback after rejected readback.'
} finally {
    # Delete only this invocation's disposable folder, never production tasks.
    foreach ($task in @(ScheduledTasks\Get-ScheduledTask -TaskPath $script:probePath -ErrorAction SilentlyContinue)) {
        ScheduledTasks\Unregister-ScheduledTask -TaskPath $script:probePath -TaskName $task.TaskName -Confirm:$false -ErrorAction Stop
    }
    $root.DeleteFolder($script:probePath.Trim('\'), 0)
}
