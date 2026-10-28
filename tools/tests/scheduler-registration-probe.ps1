$ErrorActionPreference = 'Stop'
$taskPath = '\'
$taskName = 'WinCommander-Probe-' + [Guid]::NewGuid().ToString('N')
$created = $false
try {
    $action = New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\cmd.exe') -Argument '/c exit 0'
    $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddDays(1)
    $settings = New-ScheduledTaskSettingsSet -Disable
    $account = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $principal = New-ScheduledTaskPrincipal -UserId $account -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskPath $taskPath -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -Principal $principal -ErrorAction Stop | Out-Null
    $created = $true
    $task = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction Stop
    if ($task.State -ne 'Disabled') { throw 'Test task was not registered disabled.' }
    Write-Output "Registered disabled test task: $taskPath$taskName"
} finally {
    if ($created) {
        Unregister-ScheduledTask -TaskPath $taskPath -TaskName $taskName -Confirm:$false -ErrorAction Stop
        if (Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction SilentlyContinue) { throw 'Test task is still present.' }
        Write-Output "Removed and verified absent: $taskPath$taskName"
    }
}
