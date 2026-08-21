# ============================================================================
# WINDOWS AI CONTROL — POLICIES AND APP-LEVEL SWITCHES
# ============================================================================

function Get-AIControlPolicyEntries {
    $entries = @()
    foreach ($hive in @('HKLM:', 'HKCU:')) {
        $path = "$hive\SOFTWARE\Policies\Microsoft\Windows\WindowsAI"
        foreach ($name in @(
            'DisableAIDataAnalysis', 'TurnOffSavingSnapshots', 'DisableClickToDo',
            'DisableAIDataAnalysisOnBattery', 'DisableRecallDataProviders',
            'DisableSettingsAgent', 'DisableAgentConnectors', 'DisableAgentWorkspaces',
            'DisableRemoteAgentConnectors'
        )) {
            $entries += @{ Path = $path; Name = $name; Type = 'DWord'; Disabled = 1 }
        }
        $entries += @{ Path = $path; Name = 'AllowRecallEnablement'; Type = 'DWord'; Disabled = 0 }
        $entries += @{ Path = "$hive\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot"; Name = 'TurnOffWindowsCopilot'; Type = 'DWord'; Disabled = 1 }
    }
    $entries += @(
        @{ Path = 'HKLM:\Software\Microsoft\Windows\Shell\Copilot\BingChat'; Name = 'IsUserEligible'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKLM:\Software\Microsoft\Windows\Shell\Copilot'; Name = 'IsCopilotAvailable'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKLM:\Software\Microsoft\Windows\Shell\Copilot'; Name = 'CopilotDisabledReason'; Type = 'String'; Disabled = 'FeatureIsDisabled' },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\Shell\Copilot\BingChat'; Name = 'IsUserEligible'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\Shell\Copilot'; Name = 'IsCopilotAvailable'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\Shell\Copilot'; Name = 'CopilotDisabledReason'; Type = 'String'; Disabled = 'FeatureIsDisabled' },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\Microsoft.Copilot_8wekyb3d8bbwe'; Name = 'Value'; Type = 'String'; Disabled = 'Deny' },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe'; Name = 'Value'; Type = 'String'; Disabled = 'Deny' },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\systemAIModels'; Name = 'Value'; Type = 'String'; Disabled = 'Deny' },
        @{ Path = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\Capabilities\systemAIModels'; Name = 'RecordUsageData'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Speech_OneCore\Settings\VoiceActivation\UserPreferenceForAllApps'; Name = 'AgentActivationEnabled'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'; Name = 'ShowCopilotButton'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\input\Settings'; Name = 'InsightsEnabled'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\Shell\ClickToDo'; Name = 'DisableClickToDo'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\M365Copilot'; Name = 'AutoStartDelayEnabled'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\M365Copilot'; Name = 'IsCompanionWindowAvailable'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer'; Name = 'DisableSearchBoxSuggestions'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsCopilot'; Name = 'AllowCopilotRuntime'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\CopilotKey'; Name = 'SetCopilotHardwareKey'; Type = 'String'; Disabled = ' ' },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband\AuxilliaryPins'; Name = 'CopilotPWAPin'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband\AuxilliaryPins'; Name = 'RecallPin'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'; Name = 'TaskbarCompanion'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\SettingSync\WindowsSettingHandlers'; Name = 'A9HomeContentEnabled'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\InputPersonalization'; Name = 'RestrictImplicitInkCollection'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\InputPersonalization'; Name = 'RestrictImplicitTextCollection'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\InputPersonalization\TrainedDataStore'; Name = 'HarvestContacts'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CPSS\Store\InkingAndTypingPersonalization'; Name = 'Value'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent'; Name = 'DisableConsumerAccountStateContent'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Notifications\Settings'; Name = 'AutoOpenCopilotLargeScreens'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\generativeAI'; Name = 'Value'; Type = 'String'; Disabled = 'Deny' },
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\systemAIModels'; Name = 'Value'; Type = 'String'; Disabled = 'Deny' },
        @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy'; Name = 'LetAppsAccessGenerativeAI'; Type = 'DWord'; Disabled = 2 },
        @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy'; Name = 'LetAppsAccessSystemAIModels'; Type = 'DWord'; Disabled = 2 },
        @{ Path = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData\Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe\WebViewHostStartupId'; Name = 'State'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Paint'; Name = 'DisableImageCreator'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'IsSignedUpForTargetingService'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'LeftTargetingService'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'IsNotInterestedInTargetingService'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'GettingStartedWelcomePageViewed'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'GettingStartedStickerGeneratorPageViewed'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'GettingStartedGenerativeImageEditPageViewed'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'GettingStartedGenerativeErasePageViewed'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'GettingStartedGenerativeFillPageViewed'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'GettingStartedImageCreatorPageViewed'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Applets\Paint\View'; Name = 'GettingStartedCocreatorPageViewed'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKCU:\Software\Microsoft\VoiceAccess'; Name = 'RunningState'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Microsoft\VoiceAccess'; Name = 'TextCorrection'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot'; Name = 'TurnOffGamingCopilot'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot'; Name = 'TurnOffGamingCopilotAutoLaunch'; Type = 'DWord'; Disabled = 1 }
    )
    foreach ($name in @(
        'CopilotCDPPageContext', 'CopilotPageContext', 'HubsSidebarEnabled',
        'EdgeEntraCopilotPageContext', 'Microsoft365CopilotChatIconEnabled',
        'EdgeHistoryAISearchEnabled', 'ComposeInlineEnabled', 'BuiltInAIAPIsEnabled',
        'AIGenThemesEnabled', 'ShareBrowsingHistoryWithCopilotSearchAllowed',
        'AllowBrowsingWithCopilot', 'CopilotNewTabPageEnabled',
        'M365LinksAutoOpenCopilotEnabled', 'CopilotAddressBarSuggestionsEnabled'
    )) {
        $entries += @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge'; Name = $name; Type = 'DWord'; Disabled = 0 }
    }
    $entries += @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge'; Name = 'GenAILocalFoundationalModelSettings'; Type = 'DWord'; Disabled = 1 }
    $entries += @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge'; Name = 'DevToolsGenAiSettings'; Type = 'DWord'; Disabled = 2 }
    foreach ($app in @('Word', 'Excel')) {
        $entries += @{ Path = "HKCU:\Software\Microsoft\Office\16.0\$app\Options"; Name = 'EnableCopilot'; Type = 'DWord'; Disabled = 0 }
    }
    $entries += @(
        @{ Path = 'HKCU:\Software\Microsoft\Office\16.0\PowerPoint\Options'; Name = 'Enable Copilot in Settings'; Type = 'DWord'; Disabled = 0 },
        @{ Path = 'HKCU:\Software\Policies\Microsoft\office\16.0\common\privacy'; Name = 'controllerconnectedservicesenabled'; Type = 'DWord'; Disabled = 2 },
        @{ Path = 'HKCU:\Software\Policies\Microsoft\office\16.0\common\privacy'; Name = 'usercontentdisabled'; Type = 'DWord'; Disabled = 2 },
        @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\office\16.0\common\ai\training\general'; Name = 'disabletraining'; Type = 'DWord'; Disabled = 1 },
        @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\office\16.0\common\ai\training\specific\adaptivefloatie'; Name = 'disabletrainingofadaptivefloatie'; Type = 'DWord'; Disabled = 1 }
    )
    foreach ($area in @('general', 'specific\alternativetext', 'specific\imagequestionandanswering', 'specific\promptassistance', 'specific\rewrite', 'specific\summarization', 'specific\summarizationwithreferences', 'specific\texttotable')) {
        $entries += @{ Path = "HKLM:\SOFTWARE\Policies\Microsoft\office\16.0\common\ai\contentsafety\$area"; Name = 'disablecontentsafety'; Type = 'DWord'; Disabled = 1 }
    }
    foreach ($name in @('CopilotEnabled', 'CopilotNotebooksEnabled', 'CopilotSkittleEnabled')) {
        $entries += @{ Path = 'HKCU:\Software\Microsoft\Office\16.0\OneNote\Options\Copilot'; Name = $name; Type = 'DWord'; Disabled = 0 }
    }
    foreach ($package in @('Microsoft.Copilot_8wekyb3d8bbwe', 'Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe')) {
        foreach ($name in @('DisabledByUser', 'Disabled', 'SleepDisabled')) {
            $entries += @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications\$package"; Name = $name; Type = 'DWord'; Disabled = 1 }
        }
    }
    $entries
}

function Set-AIControlPolicies {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    $entries = @(Get-AIControlPolicyEntries)
    Set-AIControlRegistryEntries -Snapshot 'policies' -Entries $entries -Mode $Mode
    $defaultHive = Join-Path $env:SystemDrive 'Users\Default\NTUSER.DAT'
    if (Test-Path -LiteralPath $defaultHive) {
        & reg.exe load 'HKU\WinCommanderDefault' $defaultHive | Out-Null
        try {
            $defaultEntries = @($entries | Where-Object { $_.Path -like 'HKCU:\*' } | ForEach-Object {
                @{
                    Path = $_.Path.Replace('HKCU:\', 'Registry::HKEY_USERS\WinCommanderDefault\')
                    Name = $_.Name
                    Type = $_.Type
                    Disabled = $_.Disabled
                }
            })
            Set-AIControlRegistryEntries -Snapshot 'default-policies' -Entries $defaultEntries -Mode $Mode
        } finally {
            [gc]::Collect()
            & reg.exe unload 'HKU\WinCommanderDefault' | Out-Null
        }
    }
    Restart-Explorer -AllUsers | Out-Null
    [pscustomobject]@{ status = if ($Mode -eq 'apply') { 'disabled' } else { 'restored' }; operation = 'policies'; changed = $entries.Count; requiresReboot = $false }
}

function Set-AIControlRegionPolicy {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    Assert-AIControlAdmin
    $targets = @(
        (Join-Path $env:windir 'System32\IntegratedServicesRegionPolicySet.json'),
        (Join-Path $env:windir 'SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\VisualAssist\VisualAssistActions.json')
    )
    $backupRoot = Join-Path (Get-AIControlDataRoot) 'region-policy'
    if ($Mode -eq 'revert') {
        if (-not (Test-Path -LiteralPath $backupRoot)) { throw 'No region-policy backup is available.' }
        foreach ($target in $targets) {
            $backup = Join-Path $backupRoot ([IO.Path]::GetFileName($target))
            if (Test-Path -LiteralPath $backup) { Copy-Item -LiteralPath $backup -Destination $target -Force }
        }
        return [pscustomobject]@{ status = 'restored'; operation = 'region-policy'; changed = 2; requiresReboot = $true }
    }
    New-Item -Path $backupRoot -ItemType Directory -Force | Out-Null
    $changed = 0
    $region = $targets[0]
    if (Test-Path -LiteralPath $region) {
        $backup = Join-Path $backupRoot ([IO.Path]::GetFileName($region))
        if (-not (Test-Path -LiteralPath $backup)) { Copy-Item -LiteralPath $region -Destination $backup -Force }
        & takeown.exe /f $region | Out-Null
        & icacls.exe $region /grant '*S-1-5-32-544:F' | Out-Null
        $json = Get-Content -Raw -LiteralPath $region | ConvertFrom-Json
        foreach ($policy in @($json.policies)) {
            $comment = [string]$policy.'$comment'
            if ($comment -match 'CoPilot|Manage Recall') { $policy.defaultState = 'disabled'; $changed++ }
            elseif ($comment -match 'A9|Settings Agent') { $policy.defaultState = 'enabled'; $changed++ }
        }
        $json | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $region -Encoding UTF8
    }
    $visual = $targets[1]
    if (Test-Path -LiteralPath $visual) {
        $backup = Join-Path $backupRoot ([IO.Path]::GetFileName($visual))
        if (-not (Test-Path -LiteralPath $backup)) { Copy-Item -LiteralPath $visual -Destination $backup -Force }
        & takeown.exe /f $visual | Out-Null
        & icacls.exe $visual /grant '*S-1-5-32-544:F' | Out-Null
        $json = Get-Content -Raw -LiteralPath $visual | ConvertFrom-Json
        $json.actions | Add-Member -MemberType NoteProperty -Name usesGenerativeAI -Value $false -Force
        $json | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $visual -Encoding UTF8
        $changed++
    }
    [pscustomobject]@{ status = 'disabled'; operation = 'region-policy'; changed = $changed; requiresReboot = $true }
}

function Set-AIControlSettingsVisibility {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    $entry = @(@{ Path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer'; Name = 'SettingsPageVisibility'; Type = 'String'; Disabled = 'hide:aicomponents;appactions;' })
    if ($Mode -eq 'apply') {
        $existing = try { Get-ItemPropertyValue -LiteralPath $entry[0].Path -Name $entry[0].Name -ErrorAction Stop } catch { $null }
        if ($existing -like 'showonly:*') { throw 'SettingsPageVisibility already uses showonly; WinCommander will not overwrite it.' }
        $entry[0].Disabled = if ([string]::IsNullOrWhiteSpace($existing)) { 'hide:aicomponents;appactions;' } elseif ($existing -match 'aicomponents') { $existing } else { $existing.TrimEnd(';') + ';aicomponents;appactions;' }
    }
    Set-AIControlRegistryEntries -Snapshot 'settings-visibility' -Entries $entry -Mode $Mode
    [pscustomobject]@{ status = if ($Mode -eq 'apply') { 'hidden' } else { 'restored' }; operation = 'settings-page'; changed = 1; requiresReboot = $false }
}

function Set-AIControlNotepadRewrite {
    param([ValidateSet('apply', 'revert')][string]$Mode)
    $entries = @(@{ Path = 'HKLM:\SOFTWARE\Policies\WindowsNotepad'; Name = 'DisableAIFeatures'; Type = 'DWord'; Disabled = 1 })
    Set-AIControlRegistryEntries -Snapshot 'notepad-rewrite' -Entries $entries -Mode $Mode
    [pscustomobject]@{ status = if ($Mode -eq 'apply') { 'disabled' } else { 'restored' }; operation = 'notepad-rewrite'; changed = 1; requiresReboot = $false }
}
