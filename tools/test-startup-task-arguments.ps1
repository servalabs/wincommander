[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$name = 'StartupArgumentProbe-' + [Guid]::NewGuid().ToString('N')
$identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$sessionId = [Diagnostics.Process]::GetCurrentProcess().SessionId
$registered = $false
try {
    $command = "-NoProfile -NonInteractive -WindowStyle Hidden -Command `"if ('`$(Arg0)' -eq '--autostart') { exit 31 }; if ('`$(Arg0)' -eq '--focus') { exit 32 }; exit 99`""
    $action = New-ScheduledTaskAction -Execute "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument $command
    $principal = New-ScheduledTaskPrincipal -UserId $identity -LogonType Interactive -RunLevel Limited
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::FromMinutes(1))
    Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings -ErrorAction Stop | Out-Null
    $registered = $true
    $scheduler = New-Object -ComObject Schedule.Service
    $scheduler.Connect()
    $task = $scheduler.GetFolder('\').GetTask($name)
    foreach ($case in @(@{ Value = '--autostart'; Exit = 31 }, @{ Value = '--focus'; Exit = 32 })) {
        $running = $task.RunEx([string]$case.Value, 4, $sessionId, $null)
        if ($null -eq $running) { throw 'Scheduler returned no running instance.' }
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
            Start-Sleep -Milliseconds 100
            $task = $scheduler.GetFolder('\').GetTask($name)
            $result = $task.LastTaskResult
        } while (($task.GetInstances(0).Count -gt 0 -or $result -ne $case.Exit) -and [DateTime]::UtcNow -lt $deadline)
        if ($result -ne $case.Exit) { throw "Argument substitution failed: $($case.Value), exit=$result" }
        Write-Output "PASS: RunEx single-string $($case.Value) substituted into Arg0 in session $sessionId (exit=$result)."
    }
} finally {
    if ($registered) {
        Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction Stop
        if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) { throw 'Disposable argument probe cleanup was not confirmed.' }
    }
}
