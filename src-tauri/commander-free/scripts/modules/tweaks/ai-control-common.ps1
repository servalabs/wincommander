# ============================================================================
# WINDOWS AI CONTROL — SHARED SAFETY AND BACKUP HELPERS
# ============================================================================

function Assert-AIControlAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'This operation requires an elevated WinCommander session.'
    }
}

function Get-AIControlDataRoot {
    $root = Join-Path $env:ProgramData 'WinCommander\AIControl'
    if (-not (Test-Path -LiteralPath $root)) {
        New-Item -Path $root -ItemType Directory -Force | Out-Null
        & icacls.exe $root /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' /t /c | Out-Null
    }
    $root
}

function Get-AIControlPackagePatterns {
    @(
        'MicrosoftWindows.Client.AIX', 'MicrosoftWindows.Client.CoPilot',
        'Microsoft.Windows.Ai.Copilot.Provider', 'Microsoft.Copilot',
        'Microsoft.MicrosoftOfficeHub', 'MicrosoftWindows.Client.CoreAI',
        'Microsoft.Edge.GameAssist', 'Microsoft.Office.ActionsServer',
        'aimgr', 'Microsoft.WritingAssistant', 'Clipchamp.Clipchamp',
        'Microsoft.AIFabric.CBS*', 'MicrosoftWindows.*.Voiess',
        'MicrosoftWindows.*.Speion', 'MicrosoftWindows.*.Livtop',
        'MicrosoftWindows.*.InpApp', 'MicrosoftWindows.*.Filons',
        'WindowsWorkload.Data.Analysis*', 'WindowsWorkload.Manager.*',
        'WindowsWorkload.PSOnnxRuntime*', 'WindowsWorkload.PSTokenizer*',
        'WindowsWorkload.QueryBlockList.*', 'WindowsWorkload.QueryProcessor*',
        'WindowsWorkload.SemanticText*', 'WindowsWorkload.Data.ContentExtraction*',
        'WindowsWorkload.ScrRegDetection*', 'WindowsWorkload.TextRecognition*',
        'WindowsWorkload.Data.ImageSearch*', 'WindowsWorkload.ImageContentModeration*',
        'WindowsWorkload.ImageSearch*', 'WindowsWorkload.PSTokenizerShared*',
        'WindowsWorkload.ImageTextSearch*', 'WindowsWorkload.SettingsModel*',
        'WindowsWorkload.Data.PhiSilica*', 'WindowsWorkload.EP.Qualcomm*',
        'WindowsWorkload.ImageDescription*', 'WindowsWorkload.ImageLLMAdapter*',
        'WindowsWorkload.LanguageModel*', 'WindowsWorkload.SessionManager*',
        'WindowsWorkload.TextContentModeration*', 'WindowsWorkload.WinMLShared*',
        'WindowsWorkload.Data.SettingsModel*', 'MicrosoftCorporationII.WinML.Qualcomm*'
    )
}

function Save-AIControlRegistrySnapshot {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][array]$Entries)
    if ($Name -notmatch '^[a-z0-9-]+$') { throw 'Invalid snapshot name.' }
    $path = Join-Path (Get-AIControlDataRoot) "$Name-registry.json"
    if (Test-Path -LiteralPath $path) { return }
    $snapshot = foreach ($entry in $Entries) {
        $exists = $false
        $value = $null
        try {
            $item = Get-ItemProperty -LiteralPath $entry.Path -Name $entry.Name -ErrorAction Stop
            $value = $item.($entry.Name)
            $exists = $true
        } catch {}
        [pscustomobject]@{
            path = $entry.Path
            name = $entry.Name
            type = $entry.Type
            existed = $exists
            value = $value
        }
    }
    $snapshot | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $path -Encoding UTF8
}

function Restore-AIControlRegistrySnapshot {
    param([Parameter(Mandatory)][string]$Name)
    if ($Name -notmatch '^[a-z0-9-]+$') { throw 'Invalid snapshot name.' }
    $path = Join-Path (Get-AIControlDataRoot) "$Name-registry.json"
    if (-not (Test-Path -LiteralPath $path)) { throw "No $Name registry backup is available." }
    $snapshot = @(Get-Content -Raw -LiteralPath $path | ConvertFrom-Json)
    foreach ($entry in $snapshot) {
        if ($entry.existed) {
            if (-not (Test-Path -LiteralPath $entry.path)) {
                New-Item -Path $entry.path -Force | Out-Null
            }
            New-ItemProperty -LiteralPath $entry.path -Name $entry.name -Value $entry.value -PropertyType $entry.type -Force | Out-Null
        } elseif (Test-Path -LiteralPath $entry.path) {
            Remove-ItemProperty -LiteralPath $entry.path -Name $entry.name -Force -ErrorAction SilentlyContinue
        }
    }
}

function Set-AIControlRegistryEntries {
    param(
        [Parameter(Mandatory)][string]$Snapshot,
        [Parameter(Mandatory)][array]$Entries,
        [Parameter(Mandatory)][ValidateSet('apply', 'revert')][string]$Mode
    )
    if ($Mode -eq 'revert') {
        Restore-AIControlRegistrySnapshot -Name $Snapshot
        return
    }
    Save-AIControlRegistrySnapshot -Name $Snapshot -Entries $Entries
    foreach ($entry in $Entries) {
        if (-not (Test-Path -LiteralPath $entry.Path)) {
            New-Item -Path $entry.Path -Force | Out-Null
        }
        New-ItemProperty -LiteralPath $entry.Path -Name $entry.Name -Value $entry.Disabled -PropertyType $entry.Type -Force | Out-Null
    }
}

function Backup-AIControlRegistryKeys {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][array]$Keys)
    if ($Name -notmatch '^[a-z0-9-]+$') { throw 'Invalid registry backup name.' }
    $root = Join-Path (Get-AIControlDataRoot) "registry\$Name"
    $manifestPath = Join-Path $root 'manifest.json'
    if (Test-Path -LiteralPath $manifestPath) { return }
    New-Item -Path $root -ItemType Directory -Force | Out-Null
    $manifest = @()
    foreach ($key in $Keys) {
        if (-not (Test-Path -LiteralPath $key.Path)) { continue }
        $file = ([guid]::NewGuid().ToString('N') + '.reg')
        & reg.exe export $key.Registry (Join-Path $root $file) /y | Out-Null
        if ($LASTEXITCODE -eq 0) { $manifest += [pscustomobject]@{ file = $file } }
    }
    ConvertTo-Json -InputObject @($manifest) -Depth 3 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
}

function Restore-AIControlRegistryKeys {
    param([Parameter(Mandatory)][string]$Name)
    if ($Name -notmatch '^[a-z0-9-]+$') { throw 'Invalid registry backup name.' }
    $root = Join-Path (Get-AIControlDataRoot) "registry\$Name"
    $manifestPath = Join-Path $root 'manifest.json'
    if (-not (Test-Path -LiteralPath $manifestPath)) { return }
    foreach ($entry in @(Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json)) {
        if (-not $entry -or -not $entry.file) { continue }
        $file = Join-Path $root $entry.file
        if (Test-Path -LiteralPath $file) { & reg.exe import $file | Out-Null }
    }
}

function Backup-AIControlPaths {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][array]$Paths)
    if ($Name -notmatch '^[a-z0-9-]+$') { throw 'Invalid backup name.' }
    $root = Join-Path (Get-AIControlDataRoot) "files\$Name"
    $manifestPath = Join-Path $root 'manifest.json'
    if (Test-Path -LiteralPath $manifestPath) { return }
    New-Item -Path $root -ItemType Directory -Force | Out-Null
    $manifest = @()
    foreach ($source in ($Paths | Select-Object -Unique)) {
        if (-not (Test-Path -LiteralPath $source)) { continue }
        $id = [guid]::NewGuid().ToString('N')
        $destination = Join-Path $root $id
        Copy-Item -LiteralPath $source -Destination $destination -Recurse -Force -ErrorAction Stop
        $manifest += [pscustomobject]@{ source = $source; item = $id }
    }
    ConvertTo-Json -InputObject @($manifest) -Depth 4 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
}

function Restore-AIControlPaths {
    param([Parameter(Mandatory)][string]$Name)
    if ($Name -notmatch '^[a-z0-9-]+$') { throw 'Invalid backup name.' }
    $root = Join-Path (Get-AIControlDataRoot) "files\$Name"
    $manifestPath = Join-Path $root 'manifest.json'
    if (-not (Test-Path -LiteralPath $manifestPath)) { throw "No $Name file backup is available." }
    foreach ($entry in @(Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json)) {
        if (-not $entry) { continue }
        $stored = Join-Path $root $entry.item
        if (-not (Test-Path -LiteralPath $stored)) { continue }
        $parent = Split-Path -Parent $entry.source
        if (-not (Test-Path -LiteralPath $parent)) { New-Item -Path $parent -ItemType Directory -Force | Out-Null }
        if (Test-Path -LiteralPath $entry.source) { Remove-Item -LiteralPath $entry.source -Recurse -Force }
        Move-Item -LiteralPath $stored -Destination $entry.source -Force
    }
}

function Invoke-AIControlTrustedScript {
    param([Parameter(Mandatory)][string]$Script)
    Assert-AIControlAdmin
    $expectedPath = Join-Path $env:SystemRoot 'servicing\TrustedInstaller.exe'
    $service = Get-CimInstance -ClassName Win32_Service -Filter "Name='TrustedInstaller'"
    $currentPath = [Environment]::ExpandEnvironmentVariables(([string]$service.PathName).Trim('"'))
    if (-not $currentPath.Equals($expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Windows Modules Installer has a nonstandard executable path; privileged package work was refused.'
    }
    $marker = Join-Path $env:TEMP ("WinCommander-AI-" + [guid]::NewGuid().ToString('N') + '.done')
    $escapedMarker = $marker.Replace("'", "''")
    $wrapped = "try { $Script } finally { Set-Content -LiteralPath '$escapedMarker' -Value done -Force }"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($wrapped))
    try {
        Stop-Service -Name TrustedInstaller -Force -ErrorAction SilentlyContinue
        & sc.exe config TrustedInstaller binPath= "powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded" | Out-Null
        & sc.exe start TrustedInstaller | Out-Null
        $deadline = (Get-Date).AddMinutes(5)
        while (-not (Test-Path -LiteralPath $marker) -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 500
        }
        if (-not (Test-Path -LiteralPath $marker)) { throw 'Privileged package operation timed out.' }
    } finally {
        & sc.exe config TrustedInstaller binPath= "`"$expectedPath`"" | Out-Null
        Stop-Service -Name TrustedInstaller -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $marker -Force -ErrorAction SilentlyContinue
    }
}

function New-AIControlRestorePoint {
    Assert-AIControlAdmin
    Enable-ComputerRestore -Drive "$env:SystemDrive\" -ErrorAction SilentlyContinue
    Checkpoint-Computer -Description ("WinCommander AI Control " + (Get-Date -Format 'yyyy-MM-dd HHmm')) -RestorePointType MODIFY_SETTINGS -ErrorAction Stop
    [pscustomobject]@{ status = 'created'; operation = 'restore-point'; requiresReboot = $false }
}
