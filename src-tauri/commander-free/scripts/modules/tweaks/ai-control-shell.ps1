# ============================================================================
# WINDOWS AI CONTROL — SERVICES, BROWSER, ACCESSIBILITY, AND SHELL SURFACES
# ============================================================================

function Set-AIControlServices {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    $root = Join-Path (Get-AIControlDataRoot) 'services'
    New-Item -Path $root -ItemType Directory -Force | Out-Null
    if ($Mode -eq 'revert') {
        $backups = @(Get-ChildItem -LiteralPath $root -Filter '*.reg' -ErrorAction SilentlyContinue)
        foreach ($backup in $backups) { & reg.exe import $backup.FullName | Out-Null }
        return $backups.Count
    }
    $services = @(
        Get-Service -Name 'WSAIFabricSvc', 'MicrosoftCopilotElevationService', 'AarSvc*' -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty Name -Unique
    )
    foreach ($service in $services) {
        $backup = Join-Path $root (($service -replace '[^A-Za-z0-9_.-]', '_') + '.reg')
        if (-not (Test-Path -LiteralPath $backup)) {
            & reg.exe export "HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Services\$service" $backup /y | Out-Null
        }
        Stop-Service -Name $service -Force -ErrorAction SilentlyContinue
        & sc.exe delete $service | Out-Null
    }
    $services.Count
}

function Set-AIControlProtocolHandlers {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    $root = Join-Path (Get-AIControlDataRoot) 'protocols'
    New-Item -Path $root -ItemType Directory -Force | Out-Null
    $keys = @(
        @{ Registry = 'HKEY_CLASSES_ROOT\.copilot'; Path = 'Registry::HKEY_CLASSES_ROOT\.copilot'; File = 'machine.reg' },
        @{ Registry = 'HKEY_CURRENT_USER\Software\Classes\.copilot'; Path = 'HKCU:\Software\Classes\.copilot'; File = 'user.reg' },
        @{ Registry = 'HKEY_CLASSES_ROOT\ms-office-ai'; Path = 'Registry::HKEY_CLASSES_ROOT\ms-office-ai'; File = 'ms-office-ai.reg' },
        @{ Registry = 'HKEY_CLASSES_ROOT\ms-copilot'; Path = 'Registry::HKEY_CLASSES_ROOT\ms-copilot'; File = 'ms-copilot.reg' },
        @{ Registry = 'HKEY_CLASSES_ROOT\ms-clicktodo'; Path = 'Registry::HKEY_CLASSES_ROOT\ms-clicktodo'; File = 'ms-clicktodo.reg' }
    )
    foreach ($key in $keys) {
        $backup = Join-Path $root $key.File
        if ($Mode -eq 'revert') {
            if (Test-Path -LiteralPath $backup) { & reg.exe import $backup | Out-Null }
        } elseif (Test-Path $key.Path) {
            if (-not (Test-Path -LiteralPath $backup)) { & reg.exe export $key.Registry $backup /y | Out-Null }
            Remove-Item -LiteralPath $key.Path -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

function Set-AIControlVoiceAccess {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    $paths = @(
        (Join-Path $env:windir 'System32\voiceaccess.exe'),
        (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Accessibility\VoiceAccess.lnk')
    )
    if ($Mode -eq 'revert') {
        $manifest = Join-Path (Get-AIControlDataRoot) 'files\voice-access\manifest.json'
        if (Test-Path -LiteralPath $manifest) { Restore-AIControlPaths -Name 'voice-access'; return 1 }
        return 0
    }
    $existing = @($paths | Where-Object { Test-Path -LiteralPath $_ })
    if (-not $existing.Count) { return 0 }
    Backup-AIControlPaths -Name 'voice-access' -Paths $existing
    $list = Join-Path (Get-AIControlDataRoot) 'voice-access-paths.txt'
    $existing | Set-Content -LiteralPath $list -Encoding Unicode
    $escaped = $list.Replace("'", "''")
    Invoke-AIControlTrustedScript -Script "Get-Content -LiteralPath '$escaped' | ForEach-Object { Remove-Item -LiteralPath `$_ -Force -ErrorAction SilentlyContinue }"
    1
}

function Set-AIControlEdgeFlags {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    $path = Join-Path $env:LOCALAPPDATA 'Microsoft\Edge\User Data\Local State'
    $backup = Join-Path (Get-AIControlDataRoot) 'edge-local-state.json'
    if ($Mode -eq 'revert') {
        if (Test-Path -LiteralPath $backup) {
            Stop-Process -Name msedge -Force -ErrorAction SilentlyContinue
            Copy-Item -LiteralPath $backup -Destination $path -Force
            return 1
        }
        return 0
    }
    if (-not (Test-Path -LiteralPath $path)) { return 0 }
    if (-not (Test-Path -LiteralPath $backup)) { Copy-Item -LiteralPath $path -Destination $backup -Force }
    Stop-Process -Name msedge -Force -ErrorAction SilentlyContinue
    $json = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
    if (-not $json.browser) { $json | Add-Member -NotePropertyName browser -NotePropertyValue ([pscustomobject]@{}) }
    if (-not $json.browser.enabled_labs_experiments) {
        $json.browser | Add-Member -NotePropertyName enabled_labs_experiments -NotePropertyValue @()
    }
    foreach ($flag in @('edge-copilot-mode@2', 'edge-ntp-composer@2', 'edge-compose@2')) {
        if ($json.browser.enabled_labs_experiments -notcontains $flag) { $json.browser.enabled_labs_experiments += $flag }
    }
    $json | ConvertTo-Json -Compress -Depth 100 | Set-Content -LiteralPath $path -Encoding UTF8
    1
}

function Set-AIControlShellExtensions {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    $snapshot = Join-Path (Get-AIControlDataRoot) 'shell-extensions-registry.json'
    if ($Mode -eq 'revert') {
        if (Test-Path -LiteralPath $snapshot) { Restore-AIControlRegistrySnapshot -Name 'shell-extensions'; return 1 }
        return 0
    }
    $entries = @()
    foreach ($package in @(Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.InstallLocation 'AppXManifest.xml') })) {
        try {
            $manifest = $package | Get-AppxPackageManifest
            $verbs = @($manifest.package.Applications.Application.Extensions.Extension.FileExplorerContextMenus.itemtype.verb)
            foreach ($verb in $verbs) {
                if ($verb.Id -notin @('AskM365Copilot', 'AskCopilot', 'CreateWithDesigner') -or -not $verb.Clsid) { continue }
                $entries += @{
                    Path = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked'
                    Name = "{$($verb.Clsid)}"
                    Type = 'String'
                    Disabled = [string]$verb.Id
                }
            }
        } catch {}
    }
    if (-not $entries.Count) { return 0 }
    Set-AIControlRegistryEntries -Snapshot 'shell-extensions' -Entries $entries -Mode apply
    $entries.Count
}
