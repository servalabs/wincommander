# ============================================================================
# APPS - BCU UNINSTALLER MODULE (CLI-only wrapper)
# Comprehensive application inventory, bulk uninstall, leftover cleanup,
# and advanced program management via BCU CLI.
# ============================================================================
#
# BCU CLI reference (v5.9+):
#   BCU-console.exe list                      → table (DisplayName | Version | Source)
#   BCU-console.exe export <path.xml>         → full XML export
#   BCU-console.exe uninstall <path.bcul>     → uninstall via filter file
#     /Q   quiet uninstallers
#     /U   unattended (no user prompts)
#     /J=<Level>  junk/leftover cleanup (VeryGood|Good|Questionable|Bad)
#     /V   verbose
#   Exit codes: 0=success, 1=invalid args, 1223=cancelled
#
# XML schema (tested live):
#   Root: <ApplicationEntrySerializer><Items><ApplicationUninstallerEntry>
#   Fields: AboutUrl, BundleProviderKey, CacheIdOverride, Comment,
#           DisplayIcon, DisplayName, DisplayVersion, EstimatedSize (KB),
#           InstallDate, InstallLocation, InstallSource, Is64Bit (X64|X86|Unknown),
#           IsOrphaned, IsProtected, IsRegistered, IsUpdate, IsValid, IsWebBrowser,
#           Items, ModifyPath, Publisher, QuietUninstallString, RatingId,
#           RegistryKeyName, RegistryPath, SystemComponent,
#           UninstallerFullFilename, UninstallerKind, UninstallerLocation,
#           UninstallString
#   UninstallerKind values: Chocolatey, InnoSetup, InstallShield, Msiexec,
#                           Nsis, PowerShell, SimpleDelete, StoreApp,
#                           Unknown, WindowsFeature
# ============================================================================

# ── Helpers ──────────────────────────────────────────────────────────────────

function Get-BcuConsolePath {
    $candidates = @(
        "${env:ProgramFiles}\BCUninstaller\BCU-console.exe",
        "${env:ProgramFiles(x86)}\BCUninstaller\BCU-console.exe",
        "$env:LOCALAPPDATA\Programs\BCUninstaller\BCU-console.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\Klocman.BulkCrapUninstaller_Microsoft.Winget.Source_8wekyb3d8bbwe\BCUninstaller\BCU-console.exe"
    )
    foreach ($path in $candidates) {
        if (Test-Path $path) { return $path }
    }
    $inPath = Get-Command "BCU-console.exe" -ErrorAction SilentlyContinue
    if ($inPath) { return $inPath.Source }
    return $null
}

function Assert-BcuInstalled {
    $cli = Get-BcuConsolePath
    if (-not $cli) {
        throw "BCU CLI is not installed. Install it first."
    }
    return $cli
}

# ── Test / Install ──────────────────────────────────────────────────────────

function Test-BcuInstalled {
    try {
        $cli = Get-BcuConsolePath
        @{
            installed = ($null -ne $cli)
            cliPath   = if ($cli) { $cli } else { "" }
        }
    }
    catch {
        @{ installed = $false; cliPath = "" }
    }
}

function Install-BcuUninstaller {
    try {
        $wingetCmd = Get-Command "winget.exe" -ErrorAction SilentlyContinue
        if (-not $wingetCmd) {
            return @{ error = $true; message = "Winget is not available. Install it first." }
        }
        & $wingetCmd.Source install --id Klocman.BulkCrapUninstaller --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity 2>&1 | Out-Null
        Start-Sleep -Seconds 2
        $check = Test-BcuInstalled
        if ($check.installed) {
            @{ status = "installed"; cliPath = $check.cliPath }
        }
        else {
            @{ error = $true; message = "Installation completed but CLI not found at expected paths." }
        }
    }
    catch {
        @{ error = $true; message = "Failed to install: $($_.Exception.Message)" }
    }
}

# ── Application Scan ────────────────────────────────────────────────────────

function Get-BcuApplicationList {
    <#
    .SYNOPSIS
        Scans all installed applications via BCU CLI export.
        Returns registry apps, store apps, orphaned, etc.
        XML root: ApplicationEntrySerializer > Items > ApplicationUninstallerEntry
    #>
    try {
        $cli = Assert-BcuInstalled
        $tempFile = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.xml'

        # BCU export scans everything and writes XML
        $proc = Start-Process -FilePath $cli -ArgumentList "export `"$tempFile`"" `
            -Wait -PassThru -WindowStyle Hidden

        if (-not (Test-Path $tempFile)) {
            return @{ error = $true; message = "Scan produced no output."; apps = @() }
        }

        [xml]$xml = Get-Content $tempFile -Raw -Encoding UTF8
        $entries = $xml.ApplicationEntrySerializer.Items.ApplicationUninstallerEntry
        $apps = @()

        if ($entries) {
            foreach ($e in $entries) {
                $sizeKB = 0
                if ($e.EstimatedSize -and $e.EstimatedSize -ne "0") {
                    try { $sizeKB = [long]$e.EstimatedSize } catch {}
                }

                $apps += @{
                    displayName       = if ($e.DisplayName)          { $e.DisplayName }          else { "Unknown" }
                    publisher         = if ($e.Publisher)             { $e.Publisher }             else { "" }
                    displayVersion    = if ($e.DisplayVersion)        { $e.DisplayVersion }        else { "" }
                    installDate       = if ($e.InstallDate)           { $e.InstallDate }           else { "" }
                    installLocation   = if ($e.InstallLocation)       { $e.InstallLocation }       else { "" }
                    installSource     = if ($e.InstallSource)         { $e.InstallSource }         else { "" }
                    uninstallString   = if ($e.UninstallString)       { $e.UninstallString }       else { "" }
                    quietUninstall    = if ($e.QuietUninstallString)  { $e.QuietUninstallString }  else { "" }
                    estimatedSizeKB   = $sizeKB
                    isProtected       = ($e.IsProtected -eq "true")
                    isSystemComponent = ($e.SystemComponent -eq "true")
                    isOrphaned        = ($e.IsOrphaned -eq "true")
                    isUpdate          = ($e.IsUpdate -eq "true")
                    isValid           = ($e.IsValid -eq "true")
                    isRegistered      = ($e.IsRegistered -eq "true")
                    isWebBrowser      = ($e.IsWebBrowser -eq "true")
                    canQuietUninstall = [bool]$e.QuietUninstallString
                    uninstallerKind   = if ($e.UninstallerKind) { $e.UninstallerKind } else { "Unknown" }
                    is64Bit           = if ($e.Is64Bit) { $e.Is64Bit } else { "Unknown" }
                    registryKeyName   = if ($e.RegistryKeyName)  { $e.RegistryKeyName }  else { "" }
                    registryPath      = if ($e.RegistryPath)     { $e.RegistryPath }     else { "" }
                    aboutUrl          = if ($e.AboutUrl)          { $e.AboutUrl }          else { "" }
                    comment           = if ($e.Comment)           { $e.Comment }           else { "" }
                    displayIcon       = if ($e.DisplayIcon)       { $e.DisplayIcon }       else { "" }
                }
            }
        }

        Remove-Item $tempFile -Force -ErrorAction SilentlyContinue

        @{
            apps       = @($apps)
            totalCount = $apps.Count
            scanTime   = (Get-Date).ToString("o")
        }
    }
    catch {
        @{ error = $true; message = "BCU scan failed: $($_.Exception.Message)"; apps = @() }
    }
}

# ── Uninstallation ──────────────────────────────────────────────────────────

function Invoke-BcuUninstall {
    <#
    .SYNOPSIS
        Uninstall one or more applications using BCU CLI.
    .PARAMETER Names
        Comma-separated display names of applications to uninstall.
    .PARAMETER Quiet
        If "true", use quiet/silent uninstallation (/Q flag).
    .PARAMETER RemoveLeftovers
        If "true", auto-clean leftover files and registry (/J flag).
    .PARAMETER LeftoverLevel
        Confidence level: VeryGood, Good, Questionable, Bad
    #>
    param(
        [Parameter(Mandatory = $true)][string]$Names,
        [string]$Quiet = "true",
        [string]$RemoveLeftovers = "true",
        [string]$LeftoverLevel = "VeryGood"
    )
    Assert-IsAdmin
    try {
        $cli = Assert-BcuInstalled
        $bculPath = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.bcul'
        $nameList = $Names -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }

        # Build BCU .bcul filter XML
        # Schema discovered via .NET reflection on UninstallTools.Lists.UninstallList:
        #   Root: <UninstallList>  (NOT ArrayOfFilter)
        #   Children: <Filters> > <Filter> > <ComparisonEntries> > <FilterCondition>
        #   FilterCondition uses <TargetPropertyId> (NOT TargetProperty)
        #   Filter uses <Exclude> bool (NOT FilterType)
        #   All nodes need <Enabled>true</Enabled>
        $lines = @(
            '<?xml version="1.0" encoding="utf-8"?>',
            '<UninstallList xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">',
            '  <Filters>'
        )

        foreach ($name in $nameList) {
            $escaped = [System.Security.SecurityElement]::Escape($name)
            $lines += '    <Filter>'
            $lines += "      <Name>Remove $escaped</Name>"
            $lines += '      <Exclude>false</Exclude>'
            $lines += '      <ComparisonEntries>'
            $lines += '        <FilterCondition>'
            $lines += '          <InvertResults>false</InvertResults>'
            $lines += '          <ComparisonMethod>Equals</ComparisonMethod>'
            $lines += "          <FilterText>$escaped</FilterText>"
            $lines += '          <TargetPropertyId>DisplayName</TargetPropertyId>'
            $lines += '          <Enabled>true</Enabled>'
            $lines += '        </FilterCondition>'
            $lines += '      </ComparisonEntries>'
            $lines += '      <Enabled>true</Enabled>'
            $lines += '    </Filter>'
        }

        $lines += '  </Filters>'
        $lines += '  <Enabled>true</Enabled>'
        $lines += '</UninstallList>'
        $filterXml = $lines -join "`n"

        # Write without BOM — BCU's XML parser chokes on BOM at position (1,1)
        [System.IO.File]::WriteAllText($bculPath, $filterXml, (New-Object System.Text.UTF8Encoding $false))

        $cliArgs = @("uninstall", "`"$bculPath`"", "/U")
        if ($Quiet -eq "true") { $cliArgs += "/Q" }
        if ($RemoveLeftovers -eq "true") { $cliArgs += "/J=$LeftoverLevel" }

        $argString = $cliArgs -join " "
        # MUST use -NoNewWindow (not -WindowStyle Hidden) — BCU console calls
        # Console.CursorTop internally and crashes with "handle is invalid" if
        # no console is attached.
        $proc = Start-Process -FilePath $cli -ArgumentList $argString `
            -Wait -PassThru -NoNewWindow

        Remove-Item $bculPath -Force -ErrorAction SilentlyContinue

        if ($proc.ExitCode -eq 0) {
            @{ status = "completed"; exitCode = 0; appsTargeted = $nameList.Count; names = $nameList }
        }
        elseif ($proc.ExitCode -eq 1223) {
            @{ status = "cancelled"; exitCode = 1223 }
        }
        else {
            @{ error = $true; message = "Uninstall exited with code $($proc.ExitCode)"; exitCode = $proc.ExitCode }
        }
    }
    catch {
        @{ error = $true; message = "Uninstall failed: $($_.Exception.Message)" }
    }
}

function Invoke-BcuQuietUninstallSingle {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [string]$RemoveLeftovers = "true",
        [string]$LeftoverLevel = "VeryGood"
    )
    Invoke-BcuUninstall -Names $Name -Quiet "true" -RemoveLeftovers $RemoveLeftovers -LeftoverLevel $LeftoverLevel
}

function Invoke-BcuLoudUninstallSingle {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [string]$RemoveLeftovers = "true",
        [string]$LeftoverLevel = "VeryGood"
    )
    Invoke-BcuUninstall -Names $Name -Quiet "false" -RemoveLeftovers $RemoveLeftovers -LeftoverLevel $LeftoverLevel
}

# ── Leftover / Junk Cleanup ────────────────────────────────────────────────

function Invoke-BcuCleanupProgramFiles {
    <#
    .SYNOPSIS
        Scan Program Files for orphaned/leftover directories.
    #>
    try {
        $results = @()
        $programDirs = @(
            $env:ProgramFiles,
            ${env:ProgramFiles(x86)}
        ) | Where-Object { $_ -and (Test-Path $_) }

        foreach ($dir in $programDirs) {
            Get-ChildItem -Path $dir -Directory -ErrorAction SilentlyContinue | ForEach-Object {
                $folderPath = $_.FullName
                $folderName = $_.Name
                $isEmpty = $false
                $isOrphaned = $false

                $childCount = (Get-ChildItem $folderPath -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 1)
                if (-not $childCount) { $isEmpty = $true }

                if (-not $isEmpty) {
                    $regPaths = @(
                        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
                        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
                        "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
                    )
                    $found = $false
                    foreach ($regPath in $regPaths) {
                        if (Test-Path $regPath) {
                            Get-ChildItem $regPath -ErrorAction SilentlyContinue | ForEach-Object {
                                $instLoc = (Get-ItemProperty $_.PSPath -Name "InstallLocation" -ErrorAction SilentlyContinue).InstallLocation
                                if ($instLoc -and $folderPath -like "$instLoc*") { $found = $true }
                            }
                        }
                        if ($found) { break }
                    }
                    if (-not $found) { $isOrphaned = $true }
                }

                if ($isEmpty -or $isOrphaned) {
                    $sizeBytes = 0
                    try {
                        $sizeBytes = (Get-ChildItem $folderPath -Recurse -File -ErrorAction SilentlyContinue |
                            Measure-Object -Property Length -Sum -ErrorAction SilentlyContinue).Sum
                        if (-not $sizeBytes) { $sizeBytes = 0 }
                    } catch {}

                    $results += @{
                        path       = $folderPath
                        name       = $folderName
                        isEmpty    = $isEmpty
                        isOrphaned = $isOrphaned
                        sizeBytes  = $sizeBytes
                    }
                }
            }
        }

        @{
            items      = @($results)
            totalCount = $results.Count
            totalSize  = ($results | Measure-Object -Property sizeBytes -Sum).Sum
        }
    }
    catch {
        @{ error = $true; message = "Program Files cleanup scan failed: $($_.Exception.Message)"; items = @() }
    }
}

function Remove-BcuOrphanedFolder {
    param(
        [Parameter(Mandatory = $true)][string]$Path
    )
    Assert-IsAdmin
    try {
        $allowed = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ }
        $isAllowed = $false
        foreach ($dir in $allowed) {
            if ($Path.StartsWith($dir, [System.StringComparison]::OrdinalIgnoreCase)) {
                $isAllowed = $true; break
            }
        }
        if (-not $isAllowed) {
            return @{ error = $true; message = "Path is not within Program Files directories." }
        }
        if (Test-Path $Path) {
            Remove-Item -Path $Path -Recurse -Force -ErrorAction Stop
            @{ status = "removed"; path = $Path }
        } else {
            @{ status = "not_found"; path = $Path }
        }
    }
    catch {
        @{ error = $true; message = "Failed to remove folder: $($_.Exception.Message)" }
    }
}

# ── Windows Features ────────────────────────────────────────────────────────
# BCU is now a Free/local surface. Keep enumeration and per-feature toggles in
# the same module so the panel does not cross the Pro sidecar boundary for one
# tab while the rest of the BCU workflow runs locally.

function Get-BcuWindowsFeatures {
    try {
        $features = Get-WindowsOptionalFeature -Online -ErrorAction Stop | ForEach-Object {
            @{
                featureName = $_.FeatureName
                displayName = if ($_.DisplayName) { $_.DisplayName } else { $_.FeatureName }
                state       = [string]$_.State
                restartRequired = $false
            }
        }
        @{ features = @($features); totalCount = @($features).Count }
    }
    catch {
        @{ error = $true; message = "Failed to enumerate Windows features: $($_.Exception.Message)"; features = @() }
    }
}

function Disable-BcuWindowsFeature {
    param([Parameter(Mandatory = $true)][string]$FeatureName)
    Assert-IsAdmin
    try {
        $result = Disable-WindowsOptionalFeature -Online -FeatureName $FeatureName -NoRestart -ErrorAction Stop
        @{ status = "disabled"; featureName = $FeatureName; restartNeeded = $result.RestartNeeded }
    }
    catch {
        @{ error = $true; message = "Failed to disable feature '$FeatureName': $($_.Exception.Message)" }
    }
}

function Enable-BcuWindowsFeature {
    param([Parameter(Mandatory = $true)][string]$FeatureName)
    Assert-IsAdmin
    try {
        $result = Enable-WindowsOptionalFeature -Online -FeatureName $FeatureName -NoRestart -ErrorAction Stop
        @{ status = "enabled"; featureName = $FeatureName; restartNeeded = $result.RestartNeeded }
    }
    catch {
        @{ error = $true; message = "Failed to enable feature '$FeatureName': $($_.Exception.Message)" }
    }
}

# ── Startup Manager ────────────────────────────────────────────────────────

function Get-BcuStartupItems {
    try {
        $items = @()

        $runKeys = @(
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
            "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"
        )

        foreach ($key in $runKeys) {
            if (Test-Path $key) {
                Get-ItemProperty $key -ErrorAction SilentlyContinue | ForEach-Object {
                    $props = $_ | Get-Member -MemberType NoteProperty |
                        Where-Object { $_.Name -notin @('PSPath','PSParentPath','PSChildName','PSProvider','PSDrive') }
                    foreach ($prop in $props) {
                        $items += @{
                            name     = $prop.Name
                            command  = $_.$($prop.Name)
                            location = $key
                            type     = "Registry"
                            enabled  = $true
                        }
                    }
                }
            }
        }

        $startupFolders = @(
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup",
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\StartUp"
        )
        foreach ($folder in $startupFolders) {
            if (Test-Path $folder) {
                Get-ChildItem $folder -File -ErrorAction SilentlyContinue | ForEach-Object {
                    $items += @{
                        name     = $_.BaseName
                        command  = $_.FullName
                        location = $folder
                        type     = "StartupFolder"
                        enabled  = $true
                    }
                }
            }
        }

        try {
            Get-ScheduledTask -ErrorAction SilentlyContinue |
                Where-Object {
                    $_.Triggers -and ($_.Triggers | Where-Object {
                        $_ -is [Microsoft.Management.Infrastructure.CimInstance] -and
                        $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger'
                    })
                } |
                ForEach-Object {
                    $items += @{
                        name     = $_.TaskName
                        command  = if ($_.Actions.Count -gt 0) { $_.Actions[0].Execute } else { "" }
                        location = $_.TaskPath
                        type     = "ScheduledTask"
                        enabled  = ($_.State -ne 'Disabled')
                    }
                }
        } catch {}

        @{ items = @($items); totalCount = $items.Count }
    }
    catch {
        @{ error = $true; message = "Failed to enumerate startup items: $($_.Exception.Message)"; items = @() }
    }
}

function Remove-BcuStartupItem {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Location,
        [Parameter(Mandatory = $true)][string]$Type
    )
    Assert-IsAdmin
    try {
        switch ($Type) {
            "Registry" {
                if (Test-Path $Location) {
                    Remove-ItemProperty -Path $Location -Name $Name -Force -ErrorAction Stop
                }
                @{ status = "removed"; name = $Name }
            }
            "StartupFolder" {
                $filePath = Join-Path $Location "$Name.*"
                $files = Get-ChildItem $filePath -ErrorAction SilentlyContinue
                foreach ($f in $files) { Remove-Item $f.FullName -Force }
                $exactPath = Join-Path $Location $Name
                if (Test-Path $exactPath) { Remove-Item $exactPath -Force }
                @{ status = "removed"; name = $Name }
            }
            "ScheduledTask" {
                Unregister-ScheduledTask -TaskName $Name -Confirm:$false -ErrorAction Stop
                @{ status = "removed"; name = $Name }
            }
            default {
                @{ error = $true; message = "Unknown startup item type: $Type" }
            }
        }
    }
    catch {
        @{ error = $true; message = "Failed to remove startup item '$Name': $($_.Exception.Message)" }
    }
}

# ── Registry Leftovers Scanner ──────────────────────────────────────────────

function Get-BcuRegistryLeftovers {
    try {
        $leftovers = @()
        $regPaths = @(
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
        )

        foreach ($regPath in $regPaths) {
            if (-not (Test-Path $regPath)) { continue }
            Get-ChildItem $regPath -ErrorAction SilentlyContinue | ForEach-Object {
                $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                $displayName = $props.DisplayName
                $installLoc = $props.InstallLocation
                $uninstallStr = $props.UninstallString

                if ($displayName) {
                    $isOrphaned = $false

                    if ($installLoc -and $installLoc.Trim() -ne "" -and -not (Test-Path $installLoc)) {
                        $isOrphaned = $true
                    }

                    if ($uninstallStr -and -not $isOrphaned) {
                        $exePath = $uninstallStr -replace '"', '' -replace '\s+/.*$', '' -replace '\s+--.*$', ''
                        if ($exePath -and $exePath.Trim() -ne "" -and
                            -not (Test-Path $exePath) -and
                            $exePath -notlike "MsiExec*" -and $exePath -notlike "msiexec*") {
                            $isOrphaned = $true
                        }
                    }

                    if ($isOrphaned) {
                        $leftovers += @{
                            displayName     = $displayName
                            registryPath    = $_.PSPath
                            registryKey     = $_.PSChildName
                            installLocation = if ($installLoc)    { $installLoc }    else { "" }
                            uninstallString = if ($uninstallStr)  { $uninstallStr }  else { "" }
                            publisher       = if ($props.Publisher) { $props.Publisher } else { "" }
                            hive            = if ($regPath -like "HKCU:*") { "HKCU" } else { "HKLM" }
                        }
                    }
                }
            }
        }

        @{ leftovers = @($leftovers); totalCount = $leftovers.Count }
    }
    catch {
        @{ error = $true; message = "Registry leftover scan failed: $($_.Exception.Message)"; leftovers = @() }
    }
}

function Remove-BcuRegistryLeftover {
    param([Parameter(Mandatory = $true)][string]$RegistryPath)
    Assert-IsAdmin
    try {
        if (Test-Path $RegistryPath) {
            Remove-Item -Path $RegistryPath -Recurse -Force -ErrorAction Stop
            @{ status = "removed"; registryPath = $RegistryPath }
        } else {
            @{ status = "not_found"; registryPath = $RegistryPath }
        }
    }
    catch {
        @{ error = $true; message = "Failed to remove registry entry: $($_.Exception.Message)" }
    }
}

# ── Export ──────────────────────────────────────────────────────────────────

function Export-BcuApplicationList {
    param([string]$OutputPath = "")
    try {
        $cli = Assert-BcuInstalled
        if (-not $OutputPath -or $OutputPath.Trim() -eq "") {
            $OutputPath = Join-Path $env:USERPROFILE "Desktop\InstalledApps_$(Get-Date -Format 'yyyyMMdd_HHmmss').xml"
        }

        $proc = Start-Process -FilePath $cli -ArgumentList "export `"$OutputPath`"" `
            -Wait -PassThru -WindowStyle Hidden

        if ($proc.ExitCode -eq 0 -and (Test-Path $OutputPath)) {
            @{ status = "exported"; path = $OutputPath }
        } else {
            @{ error = $true; message = "Export failed with exit code $($proc.ExitCode)." }
        }
    }
    catch {
        @{ error = $true; message = "Export failed: $($_.Exception.Message)" }
    }
}
