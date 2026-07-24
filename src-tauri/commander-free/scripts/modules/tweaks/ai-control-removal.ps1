# ============================================================================
# WINDOWS AI CONTROL — PACKAGES, OPTIONAL FEATURES, CBS, AND FILES
# ============================================================================

function Set-AIControlPackageGuard {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    $root = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Appx\RemoveDefaultMicrosoftStorePackages'
    $entries = @(
        @{ Path = $root; Name = 'Enabled'; Type = 'DWord'; Disabled = 1 },
        @{ Path = "$root\Microsoft.Copilot_8wekyb3d8bbwe"; Name = 'RemovePackage'; Type = 'DWord'; Disabled = 1 },
        @{ Path = "$root\Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe"; Name = 'RemovePackage'; Type = 'DWord'; Disabled = 1 },
        @{ Path = "$root\Clipchamp.Clipchamp_yxz26nhyzhsrt"; Name = 'RemovePackage'; Type = 'DWord'; Disabled = 1 }
    )
    Set-AIControlRegistryEntries -Snapshot 'package-guard' -Entries $entries -Mode $Mode
    $markerRoot = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Deprovisioned'
    $manifestPath = Join-Path (Get-AIControlDataRoot) 'package-guard-keys.json'
    if ($Mode -eq 'apply') {
        $created = @()
        foreach ($package in @(Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue)) {
            $name = $package.Name
            if (@(Get-AIControlPackagePatterns | Where-Object { $name -like $_ }).Count -eq 0) { continue }
            $key = Join-Path $markerRoot $package.PackageFamilyName
            if (-not (Test-Path -LiteralPath $key)) { New-Item -Path $key -Force | Out-Null; $created += $key }
        }
        ConvertTo-Json -InputObject @($created) | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    } elseif (Test-Path -LiteralPath $manifestPath) {
        foreach ($key in @(Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json)) {
            if (-not $key) { continue }
            Remove-Item -LiteralPath $key -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    [pscustomobject]@{ status = if ($Mode -eq 'apply') { 'enabled' } else { 'restored' }; operation = 'package-guard'; changed = $entries.Count; requiresReboot = $false }
}

function Remove-AIControlAppxPackages {
    param([ValidateSet('apply', 'revert')][string]$Mode, [bool]$Backup = $true)
    Assert-AIControlAdmin
    $backupPath = Join-Path (Get-AIControlDataRoot) 'appx-packages.json'
    if ($Mode -eq 'revert') {
        $registered = 0
        if (Test-Path -LiteralPath $backupPath) {
            foreach ($package in @(Get-Content -Raw -LiteralPath $backupPath | ConvertFrom-Json)) {
                $manifest = Join-Path $package.InstallLocation 'AppxManifest.xml'
                if (Test-Path -LiteralPath $manifest) {
                    Add-AppxPackage -DisableDevelopmentMode -Register $manifest -ErrorAction SilentlyContinue
                    $registered++
                }
            }
        }
        Set-AIControlPackageGuard -Mode revert | Out-Null
        & dism.exe /Online /Cleanup-Image /RestoreHealth /NoRestart | Out-Null
        return [pscustomobject]@{ status = 'repair-requested'; operation = 'appx-packages'; changed = $registered; requiresReboot = $true }
    }
    $patterns = @(Get-AIControlPackagePatterns)
    $installed = @(Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue | Where-Object {
        $name = $_.Name
        @($patterns | Where-Object { $name -like $_ }).Count -gt 0
    })
    if ($Backup -and -not (Test-Path -LiteralPath $backupPath)) {
        $installed | Select-Object Name, PackageFullName, PackageFamilyName, InstallLocation |
            ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $backupPath -Encoding UTF8
    }
    $joined = ($patterns | ForEach-Object { $_.Replace("'", "''") }) -join "','"
    $script = @"
`$patterns = @('$joined')
`$store = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore'
`$installed = @(Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue)
foreach (`$package in `$installed) {
    if (@(`$patterns | Where-Object { `$package.Name -like `$_ }).Count -eq 0) { continue }
    New-Item -Path "`$store\Deprovisioned\`$(`$package.PackageFamilyName)" -Force | Out-Null
    Set-NonRemovableAppsPolicy -Online -PackageFamilyName `$package.PackageFamilyName -NonRemovable 0 -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath "`$store\InboxApplications\`$(`$package.PackageFullName)" -Recurse -Force -ErrorAction SilentlyContinue
    foreach (`$user in @(`$package.PackageUserInformation)) {
        `$sid = `$user.UserSecurityID.SID
        New-Item -Path "`$store\EndOfLife\`$sid\`$(`$package.PackageFullName)" -Force | Out-Null
        Remove-AppxPackage -Package `$package.PackageFullName -User `$sid -ErrorAction SilentlyContinue
    }
    Remove-AppxPackage -Package `$package.PackageFullName -AllUsers -ErrorAction SilentlyContinue
}
foreach (`$package in @(Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue)) {
    if (@(`$patterns | Where-Object { `$package.DisplayName -like `$_ -or `$package.PackageName -like "*`$_*" }).Count -gt 0) {
        Remove-AppxProvisionedPackage -Online -PackageName `$package.PackageName -AllUsers -ErrorAction SilentlyContinue | Out-Null
    }
}
"@
    Invoke-AIControlTrustedScript -Script $script
    Set-AIControlPackageGuard -Mode apply | Out-Null
    $remaining = @(Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue | Where-Object {
        $name = $_.Name
        @($patterns | Where-Object { $name -like $_ }).Count -gt 0
    })
    [pscustomobject]@{ status = if ($remaining.Count) { 'partial' } else { 'removed' }; operation = 'appx-packages'; changed = $installed.Count - $remaining.Count; remaining = $remaining.Count; requiresReboot = $true }
}

function Remove-AIControlRecallFeature {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    if ($Mode -eq 'apply') {
        & dism.exe /Online /Disable-Feature /FeatureName:Recall /Remove /NoRestart /Quiet | Out-Null
    } else {
        & dism.exe /Online /Enable-Feature /FeatureName:Recall /All /NoRestart /Quiet | Out-Null
    }
    $state = try { (Get-WindowsOptionalFeature -Online -FeatureName Recall -ErrorAction Stop).State.ToString() } catch { 'Unavailable' }
    [pscustomobject]@{ status = $state; operation = 'recall-feature'; changed = 1; requiresReboot = $true }
}

function Remove-AIControlCbsPackages {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    if ($Mode -eq 'revert') {
        & dism.exe /Online /Cleanup-Image /RestoreHealth /NoRestart | Out-Null
        return [pscustomobject]@{ status = 'repair-requested'; operation = 'cbs-packages'; changed = 0; requiresReboot = $true }
    }
    $root = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\Packages'
    $packages = @(Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue | Where-Object {
        $_.PSChildName -match 'AIX|Recall|Copilot|CoreAI|AIFabric'
    })
    $removed = 0
    foreach ($package in $packages) {
        $path = $package.PSPath
        Set-ItemProperty -LiteralPath $path -Name Visibility -Value 1 -Force -ErrorAction SilentlyContinue
        New-ItemProperty -LiteralPath $path -Name DefVis -PropertyType DWord -Value 2 -Force -ErrorAction SilentlyContinue | Out-Null
        Remove-Item -LiteralPath (Join-Path $path 'Owners') -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath (Join-Path $path 'Updates') -Recurse -Force -ErrorAction SilentlyContinue
        & dism.exe /Online /Remove-Package "/PackageName:$($package.PSChildName)" /NoRestart /Quiet | Out-Null
        if ($LASTEXITCODE -eq 0 -or $LASTEXITCODE -eq 3010) { $removed++ }
    }
    [pscustomobject]@{ status = if ($removed -eq $packages.Count) { 'removed' } else { 'partial' }; operation = 'cbs-packages'; changed = $removed; remaining = $packages.Count - $removed; requiresReboot = $true }
}

function Get-AIControlRemovalPaths {
    $keywords = @('AIX', 'Copilot', 'Recall', 'CoreAI', 'WindowsWorkload', 'Voiess', 'Speion', 'Livtop', 'InpApp', 'Filons')
    $roots = @(
        (Join-Path $env:windir 'SystemApps'),
        (Join-Path $env:ProgramFiles 'WindowsApps'),
        (Join-Path $env:ProgramData 'Microsoft\Windows\AppRepository'),
        (Join-Path $env:windir 'SystemApps\SxS'),
        (Join-Path $env:LOCALAPPDATA 'Packages')
    )
    $paths = @()
    foreach ($root in $roots) {
        if (-not (Test-Path -LiteralPath $root)) { continue }
        $paths += Get-ChildItem -LiteralPath $root -Directory -Force -ErrorAction SilentlyContinue | Where-Object {
            $name = $_.Name
            @($keywords | Where-Object { $name -like "*$_*" }).Count -gt 0
        } | Select-Object -ExpandProperty FullName
    }
    $paths += @(
        "$env:windir\System32\Windows.AI.MachineLearning.dll",
        "$env:windir\SysWOW64\Windows.AI.MachineLearning.dll",
        "$env:windir\System32\Windows.AI.MachineLearning.Preview.dll",
        "$env:windir\SysWOW64\Windows.AI.MachineLearning.Preview.dll",
        "$env:windir\System32\SettingsHandlers_Copilot.dll",
        "$env:windir\System32\SettingsHandlers_A9.dll",
        "$env:windir\System32\Windows.AI.Agents.dll",
        "$env:windir\SysWOW64\Windows.AI.Agents.dll",
        "$env:windir\System32\Windows.Internal.AI.PlatformCapability.dll",
        "$env:windir\SysWOW64\Windows.Internal.AI.PlatformCapability.dll",
        "$env:windir\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\ActionUI",
        "$env:windir\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\VisualAssist",
        "$env:windir\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\AppActions.exe",
        "$env:windir\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\AppActions.dll",
        "$env:windir\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\VisualAssistExe.exe",
        "$env:windir\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\VisualAssistExe.dll",
        "$env:LOCALAPPDATA\Microsoft\WindowsApps\ActionsMcpHost.exe",
        "$env:LOCALAPPDATA\Microsoft\WindowsApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\ActionsMcpHost.exe",
        "$env:windir\System32\config\systemprofile\AppData\Local\Microsoft\WindowsApps\ActionsMcpHost.exe",
        "$env:windir\System32\config\systemprofile\AppData\Local\Microsoft\WindowsApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\ActionsMcpHost.exe",
        "$env:ProgramFiles\Microsoft Office\root\vfs\ProgramFilesCommonX64\Microsoft Shared\Office16\AI",
        "$env:ProgramFiles\Microsoft Office\root\vfs\ProgramFilesCommonX86\Microsoft Shared\Office16\AI",
        "$env:ProgramFiles\Microsoft Office\root\Office16\AI",
        "$env:ProgramFiles\Microsoft Office\root\vfs\ProgramFilesCommonX64\Microsoft Shared\Office16\ActionsServer",
        "$env:ProgramFiles\Microsoft Office\root\Integration\Addons\aimgr.msix",
        "$env:ProgramFiles\Microsoft Office\root\Integration\Addons\WritingAssistant.msix",
        "$env:ProgramFiles\Microsoft Office\root\Integration\Addons\ActionsServer.msix"
    )
    $edgeRoot = "${env:ProgramFiles(x86)}\Microsoft"
    foreach ($pattern in @(
        'Edge\Application\*\copilot_provider_msix',
        'EdgeCore\*\copilot_provider_msix',
        'EdgeWebView\Application\*\copilot_provider_msix',
        'Edge\Application\*\*Copilot_setup*',
        'EdgeCore\*\*Copilot_setup*',
        'EdgeWebView\Application\*\*Copilot_setup*'
    )) {
        $paths += Get-Item -Path (Join-Path $edgeRoot $pattern) -Force -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty FullName
    }
    $edgeUpdateRoot = Join-Path $edgeRoot 'EdgeUpdate'
    if (Test-Path -LiteralPath $edgeUpdateRoot) {
        $paths += Get-ChildItem -LiteralPath $edgeUpdateRoot -Recurse -Filter '*CopilotUpdate.exe*' -Force -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty FullName
    }
    $inboxRoot = Join-Path $env:windir 'InboxApps'
    if (Test-Path -LiteralPath $inboxRoot) {
        $paths += Get-ChildItem -LiteralPath $inboxRoot -Filter '*Copilot*' -Force -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty FullName
    }
    @($paths | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -Unique)
}

function Remove-AIControlFiles {
    param([ValidateSet('apply', 'revert')][string]$Mode, [bool]$Backup = $true)
    Assert-AIControlAdmin
    if ($Mode -eq 'revert') {
        Restore-AIControlPaths -Name 'ai-files'
        Set-AIControlProtocolHandlers -Mode revert
        Restore-AIControlRegistryKeys -Name 'ai-files'
        Restore-AIControlRegistrySnapshot -Name 'ai-file-values'
        return [pscustomobject]@{ status = 'restored'; operation = 'ai-files'; changed = 1; requiresReboot = $true }
    }
    $paths = @(Get-AIControlRemovalPaths)
    if ($Backup) { Backup-AIControlPaths -Name 'ai-files' -Paths $paths }
    $registryKeys = @(
        @{ Registry = 'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\App Paths\ActionsMcpHost.exe'; Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\ActionsMcpHost.exe' },
        @{ Registry = 'HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\App Paths\ActionsMcpHost.exe'; Path = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\ActionsMcpHost.exe' },
        @{ Registry = 'HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\MicrosoftWindows.Client.CoreAI_cw5n1h2txyewy'; Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\MicrosoftWindows.Client.CoreAI_cw5n1h2txyewy' },
        @{ Registry = 'HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\Components'; Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\Components' }
    )
    if ($Backup) { Backup-AIControlRegistryKeys -Name 'ai-files' -Keys $registryKeys }
    $registryValues = @(
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\EdgeUpdate'; Name = 'CopilotUpdatePath'; Type = 'String' },
        @{ Path = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate'; Name = 'CopilotUpdatePath'; Type = 'String' },
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\Components'; Name = 'AIX'; Type = 'DWord' },
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\Components'; Name = 'CopilotNudges'; Type = 'DWord' },
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\Components'; Name = 'AIContext'; Type = 'DWord' }
    )
    if ($Backup) { Save-AIControlRegistrySnapshot -Name 'ai-file-values' -Entries $registryValues }
    Stop-Process -Name AppActions, VisualAssist -Force -ErrorAction SilentlyContinue
    $listPath = Join-Path (Get-AIControlDataRoot) 'remove-paths.txt'
    $paths | Set-Content -LiteralPath $listPath -Encoding Unicode
    $escaped = $listPath.Replace("'", "''")
    $script = "Get-Content -LiteralPath '$escaped' | ForEach-Object { if (Test-Path -LiteralPath `$_) { takeown.exe /f `$_ /r /d Y | Out-Null; icacls.exe `$_ /grant '*S-1-5-32-544:F' /t /c | Out-Null; Remove-Item -LiteralPath `$_ -Recurse -Force -ErrorAction SilentlyContinue } }"
    Invoke-AIControlTrustedScript -Script $script
    Set-AIControlProtocolHandlers -Mode apply
    Remove-Item -Path "$env:LOCALAPPDATA\CoreAIPlatform*" -Recurse -Force -ErrorAction SilentlyContinue
    if ($env:OneDrive) { Remove-Item -LiteralPath (Join-Path $env:OneDrive 'Microsoft Copilot Chat Files') -Recurse -Force -ErrorAction SilentlyContinue }
    & reg.exe delete 'HKLM\SOFTWARE\Microsoft\EdgeUpdate' /v CopilotUpdatePath /f | Out-Null
    & reg.exe delete 'HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate' /v CopilotUpdatePath /f | Out-Null
    & reg.exe delete 'HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\ActionsMcpHost.exe' /f | Out-Null
    & reg.exe delete 'HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\ActionsMcpHost.exe' /f | Out-Null
    & reg.exe delete 'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\MicrosoftWindows.Client.CoreAI_cw5n1h2txyewy' /f | Out-Null
    foreach ($name in @('AIX', 'CopilotNudges', 'AIContext')) {
        & reg.exe delete 'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell\Update\Packages\Components' /v $name /f | Out-Null
    }
    $remaining = @(Get-AIControlRemovalPaths)
    [pscustomobject]@{ status = if ($remaining.Count) { 'partial' } else { 'removed' }; operation = 'ai-files'; changed = $paths.Count - $remaining.Count; remaining = $remaining.Count; requiresReboot = $true }
}
