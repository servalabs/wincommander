# ============================================================================
# WINDOWS AI CONTROL — TASKS, UPDATE CLEANUP, REPAIR, AND CLASSIC APPS
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

function Install-AIControlPhotoViewer {
    $extensions = @('.Bmp', '.Cr2', '.Dib', '.Gif', '.JFIF', '.Jpe', '.Jpeg', '.Jpg', '.Jxr', '.Png', '.Tif', '.Tiff', '.Wdp')
    foreach ($extension in $extensions) {
        $association = if ($extension -in @('.Cr2', '.Tif', '.Tiff')) { 'PhotoViewer.FileAssoc.Tiff' } elseif ($extension -in @('.Dib', '.Bmp')) { 'PhotoViewer.FileAssoc.Bitmap' } elseif ($extension -in @('.Jpg', '.Jpe', '.Jpeg')) { 'PhotoViewer.FileAssoc.Jpeg' } else { "PhotoViewer.FileAssoc$extension" }
        & reg.exe add 'HKLM\SOFTWARE\Microsoft\Windows Photo Viewer\Capabilities\FileAssociations' /v $extension.ToLower() /t REG_SZ /d $association /f | Out-Null
        & reg.exe add "HKLM\SOFTWARE\Classes\$association\shell\open\command" /ve /t REG_EXPAND_SZ /d '%SystemRoot%\System32\rundll32.exe "%ProgramFiles%\Windows Photo Viewer\PhotoViewer.dll", ImageView_Fullscreen %1' /f | Out-Null
        & reg.exe add "HKLM\SOFTWARE\Classes\$association\shell\open\DropTarget" /v Clsid /t REG_SZ /d '{FFE2A43C-56B9-4bf5-9A79-CC6D4285608A}' /f | Out-Null
    }
    [pscustomobject]@{ status = 'installed'; operation = 'classic-photo-viewer'; changed = $extensions.Count; requiresReboot = $false }
}

function Find-AIControlLegacyBinary {
    param([ValidateSet('paint', 'snipping')][string]$App)
    $name = if ($App -eq 'paint') { 'mspaint.exe' } else { 'SnippingTool.exe' }
    $candidates = @((Join-Path (Get-AIControlDataRoot) "sources\$name"))
    $systemDrive = $env:SystemDrive.TrimEnd(':')
    foreach ($drive in @(Get-Volume -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter -and $_.DriveLetter -ne $systemDrive })) {
        $candidates += "$($drive.DriveLetter):\Windows\System32\$name"
    }
    foreach ($candidate in $candidates) {
        if (-not (Test-Path -LiteralPath $candidate)) { continue }
        $signature = Get-AuthenticodeSignature -LiteralPath $candidate
        if ($signature.Status -eq 'Valid' -and $signature.SignerCertificate.Subject -like '*Microsoft*') { return $candidate }
    }
    throw "A Microsoft-signed $name source wasn't found. Mount compatible Windows media or place it in the protected WinCommander AIControl sources directory."
}

function Install-AIControlLegacyBinary {
    param([ValidateSet('paint', 'snipping')][string]$App)
    Assert-AIControlAdmin
    $source = Find-AIControlLegacyBinary -App $App
    $name = Split-Path -Leaf $source
    $destinationRoot = Join-Path (Get-AIControlDataRoot) "classic\$App"
    New-Item -Path $destinationRoot -ItemType Directory -Force | Out-Null
    $destination = Join-Path $destinationRoot $name
    Copy-Item -LiteralPath $source -Destination $destination -Force
    $shortcutName = if ($App -eq 'paint') { 'Paint.lnk' } else { 'Accessories\Snipping Tool.lnk' }
    $shortcutPath = Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\$shortcutName"
    New-Item -Path (Split-Path -Parent $shortcutPath) -ItemType Directory -Force | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $destination
    $shortcut.Save()
    New-Item -Path (Join-Path $destinationRoot '.installed') -ItemType File -Force | Out-Null
    [pscustomobject]@{ status = 'installed'; operation = "classic-$App"; changed = 1; requiresReboot = $false }
}

function Install-AIControlNotepad {
    Assert-AIControlAdmin
    Get-AppxPackage -AllUsers -Name '*WindowsNotepad*' -ErrorAction SilentlyContinue | Remove-AppxPackage -AllUsers -ErrorAction SilentlyContinue
    Add-WindowsCapability -Online -Name 'Microsoft.Windows.Notepad.System~~~~0.0.1.0' -LimitAccess -ErrorAction Stop | Out-Null
    Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\notepad.exe' -Force -ErrorAction SilentlyContinue
    & reg.exe add 'HKLM\SOFTWARE\Classes\Applications\notepad.exe\shell\open\command' /ve /t REG_EXPAND_SZ /d '%SystemRoot%\system32\NOTEPAD.EXE "%1"' /f | Out-Null
    & reg.exe add 'HKLM\SOFTWARE\Classes\SystemFileAssociations\text\shell\edit\command' /ve /t REG_EXPAND_SZ /d '%SystemRoot%\system32\NOTEPAD.EXE "%1"' /f | Out-Null
    [pscustomobject]@{ status = 'installed'; operation = 'classic-notepad'; changed = 1; requiresReboot = $false }
}

function Install-AIControlPhotosLegacy {
    Assert-AIControlAdmin
    $existing = Get-AppxPackage -AllUsers -Name '*PhotosLegacy*' -ErrorAction SilentlyContinue
    if (-not $existing) {
        $store = Get-Command store.exe -ErrorAction SilentlyContinue
        if ($store) { & $store.Source install 9NV2L4XVMCXM | Out-Null }
        else { & winget.exe install --id 9NV2L4XVMCXM --source msstore --accept-package-agreements --accept-source-agreements --silent | Out-Null }
    }
    [pscustomobject]@{ status = 'installed'; operation = 'photos-legacy'; changed = 1; requiresReboot = $false }
}
