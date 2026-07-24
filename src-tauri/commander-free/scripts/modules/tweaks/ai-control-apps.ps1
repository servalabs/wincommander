# ============================================================================
# WINDOWS AI CONTROL — APP SETTINGS, SERVICES, AND SHELL INTEGRATIONS
# ============================================================================

function Set-AIControlUwpEntries {
    param([Parameter(Mandatory)][string]$FilePath, [Parameter(Mandatory)][array]$Entries)
    if (-not (Test-Path -LiteralPath $FilePath)) { return 0 }
    $hive = 'HKU\WinCommanderAppSettings'
    & reg.exe unload $hive | Out-Null
    Stop-Process -Name AppActions, SearchHost, FESearchHost, TextInputHost, VisualAssistExe, SnippingTool -Force -ErrorAction SilentlyContinue
    $loaded = $false
    for ($attempt = 0; $attempt -lt 30 -and -not $loaded; $attempt++) {
        & reg.exe load $hive $FilePath | Out-Null
        $loaded = $LASTEXITCODE -eq 0
        if (-not $loaded) { Start-Sleep -Milliseconds 100 }
    }
    if (-not $loaded) { throw "Unable to load app settings file: $FilePath" }
    $regFile = Join-Path $env:TEMP ("WinCommander-AppSettings-" + [guid]::NewGuid().ToString('N') + '.reg')
    try {
        $content = "Windows Registry Editor Version 5.00`r`n"
        foreach ($entry in $Entries) {
            $type = if ($entry.Type) { $entry.Type } else { '5f5e10b' }
            $bytes = switch ($type) {
                '5f5e104' { [BitConverter]::GetBytes([int]$entry.Value) }
                default { [byte[]]@([byte][int]$entry.Value) }
            }
            $value = ($bytes | ForEach-Object { '{0:x2}' -f $_ }) -join ','
            $stamp = ([BitConverter]::GetBytes([int64](Get-Date).ToFileTime()) | ForEach-Object { '{0:x2}' -f $_ }) -join ','
            $key = if ($entry.Path) { "$hive\$($entry.Path)" } else {
                $evoke = Get-ChildItem "Registry::$hive" -Recurse -ErrorAction SilentlyContinue | Where-Object PSChildName -Like '*Evoke' | Select-Object -First 1
                if (-not $evoke) { continue }
                $evoke.Name
            }
            $content += "`r`n[$key]`r`n`"$($entry.Name)`"=hex($type):$value,$stamp`r`n"
        }
        $content | Set-Content -LiteralPath $regFile -Encoding Unicode
        & reg.exe import $regFile | Out-Null
    } finally {
        [gc]::Collect()
        & reg.exe unload $hive | Out-Null
        Remove-Item -LiteralPath $regFile -Force -ErrorAction SilentlyContinue
    }
    $Entries.Count
}

function Set-AIControlAppFeatures {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    $disabled = if ($Mode -eq 'apply') { 0 } else { 1 }
    $changed = 0
    $photosPath = "$env:LOCALAPPDATA\Packages\Microsoft.Windows.Photos_8wekyb3d8bbwe\Settings\settings.dat"
    $photoNames = @(
        'OneDriveOnlineSearchFallbackFilter-IsEnabled', 'ClipChampPromo-TeachingMoment-AlternateButtonBackground-IsEnabled',
        'FileExplorer-ContextMenu-CreateWithDesigner-IsEnabled', 'ViewerOcr-IsEnabled',
        'MoodboardIsEnabledIntel', 'ClipchampNewIconIsEnabled', 'WindowsIndexerSemanticSearchIsEnabledQCOM',
        'RingTesterPublic', 'DuplicateVideoProject', 'EditHVC-BackgroundBlur-IsEnabled',
        'Designer-NewIcon-IsEnabled', 'StoryBuilder-FX-3DEffectsInAppBar', 'EditHVC-Stylizer-IsEnabled-LNL',
        'SDXL-IsEnabled', 'ViewerCopilotOnContextMenu-IsEnabled', 'EditHVC-AIBadges-IsEnabled',
        'EditHVCSuperResolutionIsEnabledQCOM', 'EditHVCStylizerIsEnabledQCOM', 'MoodboardIsEnabledAMD',
        'EditHVC-Win10-BackgroundBlur-IsEnabled', 'ViewerOcr-SearchInWeb-IsEnabled',
        'StoryBuilder-CreateDropdownUpdate-Enabled', 'LocationSearch-IsEnabled',
        'StoryBuilder-AddNewSimpleTextStyles', 'ImageCategorizationIsEnabledAMD',
        'StoryBuilder-ExportFlow-Variant', 'OneDriveOnlineSearch-IsEnabled',
        'EditHVCSuperResolutionIsEnabledAMD', 'StoryBuilder-Report-ExportIssues-IsEnabled',
        'Collections-ShowFolderAndSubfoldersFeature-IsEnabled', 'StoryBuilder-CreateDropdown-NewStrings',
        'MoodboardIsEnabledQCOM', 'ClipChampPromo-MTCButtonAlternateToolTip-IsEnabled',
        'DesignerEditor-SupportAllLanguages', 'EditHVC-UseSpotFixWhenGenerativeEraseAreaIsSmall-IsEnabled',
        'Moodboard-IsEnabled', 'StoryBuilder-CreateDropdown-ReorderVideoButtons',
        'StoryBuilder-AudioRoaming', 'StoryBuilder-CardEdit-TimeableText', 'Gallery-SplashScreen-IsEnabled',
        'Designer-IsEnabled', 'VO-UnifiedAudioButton', 'EditHVCRelightIsEnabledQCOM',
        'UnifiedEditorOnV0-IsEnabled', 'RingTester', 'ClipChampPromo-ButtonAlternateText-IsEnabled',
        'EditHVC-GenerativeErase-IsEnabled', 'EditHVC-Stylizer-IsEnabled',
        'StoryBuilder-Rotate', 'Collections-ShowFolderAndSubfoldersDefault-IsEnabled',
        'SpecialEffects-NewRemoveIcon', 'UserActivity-IsEnabled', 'OneDriveOnlineSearch-IndexWarming-IsEnabled',
        'WindowsIndexerSemanticSearchIsEnabledIntel', 'EditHVC-Win10-GenerativeErase-IsEnabled',
        'VideoProjects-ShowAllByDefault', 'WindowsIndexerSemanticSearchIsEnabledAMD',
        'Moodboard-IsEnabled-STX', 'StoryBuilder-OnlineContentControl',
        'ClipChampPromo_TeachingMomentAlternateText_IsEnabled', 'WindowsIndexerSearchIsEnabled',
        'ClipChampPromo-OneUpViewer-TitleBarOverflow-ButtonHasDesc', 'OneDriveOnlineSearch_IsEnabled',
        'OneDriveOnlineSearch_IndexWarming_IsEnabled', 'EditHVC-NewAutoEnhance-IsEnabled',
        'ExternalFileDragAndDrop-IsEnabled', 'EditHVCStylizerIsEnabledAMD',
        'EditHVC_BackgroundBlur_IsEnabled', 'StoryBuilder-RememberLastUsedTextStyleAndDefaultLayout',
        'ImageCategorizationIsEnabledIntel', 'EditHVCSuperResolutionIsEnabledIntel',
        'ClipChampPromo-PurpleIcon-IsEnabled', 'VideoEditorAppBarReorganization',
        'ICloud-EmptyStatesExperimentV2-IsEnabled', 'VO-NewPage',
        'EditHVC-SuperResolution-IsEnabled', 'ViewerBingVisualSearch-IsEnabled',
        'WindowsIndexerSemanticSearchIsEnabledLNL', 'EditHVC-Stylizer-IsEnabled-STX',
        'StoryBuilder-ReorderTextStyles', 'WindowsIndexerSemanticSearchIsEnabledSTX',
        'SingleClick-IsEnabled', 'LocationSearch_IsEnabled', 'StoryBuilder-EmptyNewProject-Enabled',
        'Moodboard-IsEnabled-LNL', 'EditHVCStylizerIsEnabledIntel',
        'ImageCategorizationIsEnabledQCOM', 'ICloud-InWin10-IsEnabled'
    )
    if (Test-Path -LiteralPath $photosPath) {
        $changed += Set-AIControlUwpEntries -FilePath $photosPath -Entries @($photoNames | ForEach-Object { @{ Name = $_; Value = $disabled } })
    }
    $actionsPath = "$env:LOCALAPPDATA\Packages\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\Settings\settings.dat"
    $actionApps = @(
        'Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe',
        'Microsoft.Office.ActionsServer_8wekyb3d8bbwe', 'MSTeams_8wekyb3d8bbwe',
        'Microsoft.Paint_8wekyb3d8bbwe', 'Microsoft.Windows.Photos_8wekyb3d8bbwe',
        'MicrosoftWindows.Client.CBS_cw5n1h2txyewy'
    )
    if (Test-Path -LiteralPath $actionsPath) {
        $actionValue = if ($Mode -eq 'apply') { 1 } else { 0 }
        $changed += Set-AIControlUwpEntries -FilePath $actionsPath -Entries @($actionApps | ForEach-Object {
            @{ Name = $_; Path = 'LocalState\DisabledApps'; Value = $actionValue }
        })
    }
    $snipPath = "$env:LOCALAPPDATA\Packages\Microsoft.ScreenSketch_8wekyb3d8bbwe\Settings\settings.dat"
    if (Test-Path -LiteralPath $snipPath) {
        $changed += Set-AIControlUwpEntries -FilePath $snipPath -Entries @(
            @{ Name = 'DeviceHasNpu'; Path = 'LocalState'; Value = $disabled; Type = '5f5e104' }
        )
    }
    $gamePath = "$env:LOCALAPPDATA\Packages\Microsoft.XboxGamingOverlay_8wekyb3d8bbwe\LocalState\profileDataSettings.txt"
    $gameBackup = Join-Path (Get-AIControlDataRoot) 'gaming-overlay.json'
    if ($Mode -eq 'revert' -and (Test-Path -LiteralPath $gameBackup)) {
        Copy-Item -LiteralPath $gameBackup -Destination $gamePath -Force
    } elseif ($Mode -eq 'apply' -and (Test-Path -LiteralPath $gamePath)) {
        if (-not (Test-Path -LiteralPath $gameBackup)) { Copy-Item -LiteralPath $gamePath -Destination $gameBackup -Force }
        $json = Get-Content -Raw -LiteralPath $gamePath | ConvertFrom-Json
        $property = $json.profile.settingsStorage.PSObject.Properties | Where-Object Name -Like '*GamingCompanionWidget*' | Select-Object -First 1
        if ($property) {
            foreach ($item in $property.Value.PSObject.Properties) {
                $item.Value = $item.Name -in @('suppressFirstFavorite', 'suppressFirstLaunch')
            }
            $property.Value | Add-Member -NotePropertyName homeMenuVisibleUser -NotePropertyValue $false -Force
            $json | ConvertTo-Json -Depth 10 -Compress | Set-Content -LiteralPath $gamePath -Encoding UTF8
            $changed++
        }
    }
    $voiceEntries = @()
    foreach ($fx in @(Get-ChildItem 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture' -Recurse -ErrorAction SilentlyContinue | Where-Object PSChildName -Eq 'FxProperties')) {
        $values = Get-ItemProperty -LiteralPath $fx.PSPath
        if ($values.PSObject.Properties.Value -match '#VocaEffectPack') {
            $voiceEntries += @{ Path = $fx.PSPath; Name = '{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5'; Type = 'DWord'; Disabled = 1 }
        }
    }
    if ($voiceEntries.Count) {
        Set-AIControlRegistryEntries -Snapshot 'voice-effects' -Entries $voiceEntries -Mode $Mode
        $changed += $voiceEntries.Count
    }
    $changed += Set-AIControlServices -Mode $Mode
    Set-AIControlProtocolHandlers -Mode $Mode
    $changed += Set-AIControlVoiceAccess -Mode $Mode
    $changed += Set-AIControlEdgeFlags -Mode $Mode
    $changed += Set-AIControlShellExtensions -Mode $Mode
    [pscustomobject]@{ status = if ($Mode -eq 'apply') { 'disabled' } else { 'restored' }; operation = 'app-features'; changed = $changed; requiresReboot = $true }
}
