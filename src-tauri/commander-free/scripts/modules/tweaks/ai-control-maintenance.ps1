# ============================================================================
# WINDOWS AI CONTROL — TASKS, UPDATE CLEANUP, AND REPAIR
# ============================================================================

function Set-AIControlScheduledTasks {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    $backupRoot = Join-Path (Get-AIControlDataRoot) 'task-backups'
    if ($Mode -eq 'revert') {
        if (-not (Test-Path -LiteralPath $backupRoot)) { throw 'No scheduled-task backup is available.' }
        $restored = 0
        foreach ($file in @(Get-ChildItem -LiteralPath $backupRoot -Filter '*.xml' -ErrorAction SilentlyContinue)) {
            $metadata = Get-Content -Raw -LiteralPath ($file.FullName + '.json') | ConvertFrom-Json
            Register-ScheduledTask -TaskName $metadata.name -TaskPath $metadata.path -Xml (Get-Content -Raw -LiteralPath $file.FullName) -Force | Out-Null
            $restored++
        }
        foreach ($channel in @('Microsoft-Windows-AI-ModelContextProtocol/Admin', 'Microsoft-Windows-AI-Platform/Admin', 'Microsoft-Windows-AI-ModelContextProtocol/Operational', 'Microsoft-Windows-AI-Platform/Operational')) {
            & wevtutil.exe sl $channel /e:true | Out-Null
        }
        return [pscustomobject]@{ status = 'restored'; operation = 'scheduled-tasks'; changed = $restored; requiresReboot = $false }
    }
    New-Item -Path $backupRoot -ItemType Directory -Force | Out-Null
    $tasks = @(Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object {
        $_.TaskPath -like '*\WindowsAI\*' -or $_.TaskName -like '*Office Actions Server*'
    })
    foreach ($task in $tasks) {
        $id = [guid]::NewGuid().ToString('N')
        Export-ScheduledTask -TaskName $task.TaskName -TaskPath $task.TaskPath |
            Set-Content -LiteralPath (Join-Path $backupRoot "$id.xml") -Encoding Unicode
        @{ name = $task.TaskName; path = $task.TaskPath } | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $backupRoot "$id.xml.json") -Encoding UTF8
        Unregister-ScheduledTask -TaskName $task.TaskName -TaskPath $task.TaskPath -Confirm:$false -ErrorAction SilentlyContinue
    }
    foreach ($channel in @('Microsoft-Windows-AI-ModelContextProtocol/Admin', 'Microsoft-Windows-AI-Platform/Admin', 'Microsoft-Windows-AI-ModelContextProtocol/Operational', 'Microsoft-Windows-AI-Platform/Operational')) {
        & wevtutil.exe sl $channel /e:false | Out-Null
    }
    [pscustomobject]@{ status = 'removed'; operation = 'scheduled-tasks'; changed = $tasks.Count; requiresReboot = $false }
}
function Set-AIControlUpdateCleanup {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    $taskName = 'WinCommander_AI_UpdateCleanup'
    $root = Get-AIControlDataRoot
    $scriptPath = Join-Path $root 'update-cleanup.ps1'
    if ($Mode -eq 'revert') {
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ status = 'disabled'; operation = 'update-cleanup'; changed = 1; requiresReboot = $false }
    }
    $patterns = (Get-AIControlPackagePatterns | ForEach-Object { "'$($_.Replace("'", "''"))'" }) -join ','
    $cleanup = @"
`$ErrorActionPreference = 'SilentlyContinue'
`$statePath = 'HKLM:\SOFTWARE\WinCommander\AIControl'
`$build = [Environment]::OSVersion.Version.ToString()
`$cached = try { Get-ItemPropertyValue -LiteralPath `$statePath -Name CachedBuild } catch { `$null }
if (`$cached -eq `$build) { exit 0 }
New-Item -Path `$statePath -Force | Out-Null
New-ItemProperty -LiteralPath `$statePath -Name CachedBuild -Value `$build -PropertyType String -Force | Out-Null
`$patterns = @($patterns)
foreach (`$package in @(Get-AppxPackage -AllUsers)) {
    if (@(`$patterns | Where-Object { `$package.Name -like `$_ }).Count -eq 0) { continue }
    New-Item -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned\`$(`$package.PackageFamilyName)" -Force | Out-Null
    Remove-AppxPackage -Package `$package.PackageFullName -AllUsers
}
foreach (`$package in @(Get-AppxProvisionedPackage -Online)) {
    if (@(`$patterns | Where-Object { `$package.DisplayName -like `$_ -or `$package.PackageName -like "*`$_*" }).Count -gt 0) {
        Remove-AppxProvisionedPackage -Online -PackageName `$package.PackageName -AllUsers | Out-Null
    }
}
New-Item -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI' -Force | Out-Null
New-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI' -Name DisableAIDataAnalysis -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI' -Name TurnOffSavingSnapshots -Value 1 -PropertyType DWord -Force | Out-Null
"@
    $cleanup | Set-Content -LiteralPath $scriptPath -Encoding UTF8
    & icacls.exe $scriptPath /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$scriptPath`""
    $triggers = @((New-ScheduledTaskTrigger -AtLogOn), (New-ScheduledTaskTrigger -Daily -At 3am))
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 20)
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $triggers -Principal $principal -Settings $settings -Force | Out-Null
    [pscustomobject]@{ status = 'enabled'; operation = 'update-cleanup'; changed = 1; requiresReboot = $false }
}

# The command surface intentionally contains only maintained repair actions.
