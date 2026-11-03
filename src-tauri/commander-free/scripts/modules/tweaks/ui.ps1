# ============================================================================
# TWEAKS - UI MODULE
# Windows interface and personalization tweaks
# ============================================================================

function Get-ShellVisibilityValue {
    param([string]$Hive, [string]$Key, [string]$Name)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, 'Registry64')
    try {
        $sub = $base.OpenSubKey($Key, $false)
        if ($null -eq $sub) { return @{ exists = $false; value = $null; kind = $null } }
        try {
            if ($Name -notin $sub.GetValueNames()) { return @{ exists = $false; value = $null; kind = $null } }
            return @{ exists = $true; value = $sub.GetValue($Name, $null, 'DoNotExpandEnvironmentNames'); kind = [string]$sub.GetValueKind($Name) }
        } finally { $sub.Dispose() }
    } finally { $base.Dispose() }
}

function Set-ShellVisibilityValue {
    param([string]$Key, [string]$Name, $Value, [string]$Kind)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('LocalMachine', 'Registry64')
    try {
        $sub = $base.CreateSubKey($Key, $true)
        if ($null -eq $sub) { throw 'Windows denied the policy write.' }
        try { $sub.SetValue($Name, $Value, $Kind); $sub.Flush() } finally { $sub.Dispose() }
    } finally { $base.Dispose() }
}

function Remove-ShellVisibilityValue {
    param([string]$Key, [string]$Name)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('LocalMachine', 'Registry64')
    try {
        $sub = $base.OpenSubKey($Key, $true)
        if ($null -ne $sub) { try { $sub.DeleteValue($Name, $false); $sub.Flush() } finally { $sub.Dispose() } }
    } finally { $base.Dispose() }
}

function Invoke-WithShellVisibilityMutex {
    param([scriptblock]$Action)
    $mutex = [Threading.Mutex]::new($false, 'Global\WinCommander_ShellVisibility')
    $acquired = $false
    try {
        try { $acquired = $mutex.WaitOne(15000) } catch [Threading.AbandonedMutexException] { $acquired = $true }
        if (-not $acquired) { throw 'Timed out waiting for the Windows appearance lock.' }
        & $Action
    } finally { if ($acquired) { $mutex.ReleaseMutex() }; $mutex.Dispose() }
}

function Get-ShellVisibilityPlatform {
    try {
        $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
        $edition = Get-ShellVisibilityValue 'LocalMachine' 'SOFTWARE\Microsoft\Windows NT\CurrentVersion' 'EditionID'
        $eligibleEdition = $edition.exists -and $edition.kind -eq 'String' -and $edition.value -match '^(Professional|Enterprise|Education|IoTEnterprise)'
        return [bool]($os.ProductType -eq 1 -and [int]$os.BuildNumber -ge 22621 -and $eligibleEdition)
    } catch { return $false }
}

function Get-ShellVisibilitySpec {
    param([string]$Item)
    switch ($Item) {
        'StartRecommended' { return @{ Key = 'SOFTWARE\Policies\Microsoft\Windows\Explorer'; Name = 'HideRecommendedSection'; Kind = 'DWord' } }
        'SettingsHome' { return @{ Key = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer'; Name = 'SettingsPageVisibility'; Kind = 'String' } }
        default { throw 'Unknown appearance setting.' }
    }
}

function Test-ShellVisibilityValue {
    param($Actual, [string]$Kind, [string]$Value)
    if ($Kind -eq 'Absent') { return -not $Actual.exists }
    return $Actual.exists -and $Actual.kind -eq $Kind -and [string]$Actual.value -ceq $Value
}

function Get-ShellVisibilityState {
    param([string]$Item, [bool]$Eligible)
    $spec = Get-ShellVisibilitySpec $Item
    $snapshot = "SOFTWARE\WinCommander\ShellVisibility\$Item"
    $current = Get-ShellVisibilityValue 'LocalMachine' $spec.Key $spec.Name
    $userPolicy = Get-ShellVisibilityValue 'CurrentUser' $spec.Key $spec.Name
    $phase = Get-ShellVisibilityValue 'LocalMachine' $snapshot 'Phase'
    $beforeKind = Get-ShellVisibilityValue 'LocalMachine' $snapshot 'BeforeKind'
    $before = Get-ShellVisibilityValue 'LocalMachine' $snapshot 'BeforeValue'
    $applied = Get-ShellVisibilityValue 'LocalMachine' $snapshot 'AppliedValue'
    $valid = $phase.exists -and $phase.kind -eq 'DWord' -and $phase.value -in @(1,2) -and $beforeKind.exists -and $beforeKind.kind -eq 'String' -and $beforeKind.value -in @('Absent',$spec.Kind) -and $before.exists -and $before.kind -eq 'String' -and $applied.exists -and $applied.kind -eq 'String'
    if ($valid) {
        if ($beforeKind.value -eq 'Absent' -and $before.value -cne '') { $valid = $false }
        if ($Item -eq 'StartRecommended' -and ($applied.value -cne '1' -or $beforeKind.value -ne 'Absent')) { $valid = $false }
        if ($Item -eq 'SettingsHome') {
            $appliedPages = if ($applied.value -match '^hide:') { @($applied.value.Substring(5) -split ';' | ForEach-Object { $_.Trim() }) } else { @() }
            if ($appliedPages -notcontains 'home' -or ($beforeKind.value -eq 'String' -and $before.value -notmatch '^hide:')) { $valid = $false }
        }
    }
    $owned = $valid -and $phase.value -eq 2
    $pending = $valid -and $phase.value -eq 1
    $conflict = (($phase.exists -or $beforeKind.exists -or $before.exists -or $applied.exists) -and -not $valid) -or ($owned -and -not (Test-ShellVisibilityValue $current $spec.Kind $applied.value)) -or ($pending -and -not (Test-ShellVisibilityValue $current $beforeKind.value $before.value))
    $externalHidden = $false
    $canApply = -not $current.exists
    if ($current.exists -and $Item -eq 'StartRecommended') { $externalHidden = $current.kind -eq 'DWord' -and $current.value -eq 1 }
    if ($current.exists -and $Item -eq 'SettingsHome' -and $current.kind -eq 'String' -and $current.value -match '^hide:') {
        $pages = @($current.value.Substring(5) -split ';' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
        $externalHidden = $pages -contains 'home'
        $canApply = -not $externalHidden
    }
    return @{ item = $Item; eligible = $Eligible; owned = [bool]$owned; pending = [bool]$pending; conflict = [bool]$conflict; userPolicy = [bool]$userPolicy.exists; existing = [bool]$current.exists; externalHidden = [bool]($externalHidden -and -not $owned); canApply = [bool]($canApply -or $pending); beforeKind = if ($valid) { [string]$beforeKind.value } else { $null }; beforeValue = if ($valid) { [string]$before.value } else { $null }; appliedValue = if ($valid) { [string]$applied.value } else { $null } }
}

function Get-ShellVisibilityStatus {
    return Invoke-WithShellVisibilityMutex {
        $eligible = Get-ShellVisibilityPlatform
        @{ isAdmin = [bool](Test-IsAdmin); items = @((Get-ShellVisibilityState 'StartRecommended' $eligible), (Get-ShellVisibilityState 'SettingsHome' $eligible)) }
    }
}

function Set-ShellVisibility {
    param([Parameter(Mandatory = $true)][ValidateSet('StartRecommended','SettingsHome')][string]$Item, [Parameter(Mandatory = $true)][ValidateSet('hide','restore')][string]$Mode)
    Assert-IsAdmin
    return Invoke-WithShellVisibilityMutex {
        $spec = Get-ShellVisibilitySpec $Item
        $snapshot = "SOFTWARE\WinCommander\ShellVisibility\$Item"
        $state = Get-ShellVisibilityState $Item (Get-ShellVisibilityPlatform)
        if ($state.conflict) { throw 'The setting or backup changed outside WinCommander. No policy was changed.' }
        if ($state.userPolicy) { throw 'A separate account policy controls this setting. No policy was changed.' }
        if ($Mode -eq 'hide') {
            if (-not $state.eligible) { throw 'This Windows edition or version is not supported for this policy.' }
            if (-not $state.owned -and -not $state.canApply) { throw 'An existing policy controls this setting. No policy was changed.' }
            if (-not $state.owned -and -not $state.pending) {
                $current = Get-ShellVisibilityValue 'LocalMachine' $spec.Key $spec.Name
                if ($Item -eq 'StartRecommended' -and $current.exists) { throw 'The policy changed before it could be saved.' }
                if ($Item -eq 'SettingsHome' -and $current.exists) {
                    if ($current.kind -ne 'String' -or $current.value -notmatch '^hide:') { throw 'The Settings policy changed before it could be saved.' }
                    $pages = @($current.value.Substring(5) -split ';' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
                    if ($pages -contains 'home') { throw 'Settings Home was already hidden outside WinCommander.' }
                }
                $beforeKind = if ($current.exists) { $current.kind } else { 'Absent' }
                $beforeValue = if ($current.exists) { [string]$current.value } else { '' }
                $applied = if ($Item -eq 'StartRecommended') { '1' } elseif (-not $current.exists -or $beforeValue -match '^hide:\s*;*\s*$') { 'hide:home' } else { $beforeValue.TrimEnd(';') + ';home' }
                Set-ShellVisibilityValue $snapshot 'BeforeKind' $beforeKind 'String'
                Set-ShellVisibilityValue $snapshot 'BeforeValue' $beforeValue 'String'
                Set-ShellVisibilityValue $snapshot 'AppliedValue' $applied 'String'
                Set-ShellVisibilityValue $snapshot 'Phase' 1 'DWord'
                $state = Get-ShellVisibilityState $Item $true
                if (-not $state.pending -or $state.conflict) { throw 'Windows could not confirm the policy backup.' }
            }
            if (-not $state.owned) {
                $value = if ($spec.Kind -eq 'DWord') { [int]$state.appliedValue } else { $state.appliedValue }
                Set-ShellVisibilityValue $spec.Key $spec.Name $value $spec.Kind
                if (-not (Test-ShellVisibilityValue (Get-ShellVisibilityValue 'LocalMachine' $spec.Key $spec.Name) $spec.Kind $state.appliedValue)) { throw 'Windows could not verify the policy write.' }
                Set-ShellVisibilityValue $snapshot 'Phase' 2 'DWord'
            }
            $result = Get-ShellVisibilityState $Item $true
            if (-not $result.owned -or $result.conflict) { throw 'Windows could not verify the policy change.' }
        } else {
            if (-not $state.owned) { throw 'WinCommander has no confirmed change to restore.' }
            if ($state.beforeKind -eq 'Absent') { Remove-ShellVisibilityValue $spec.Key $spec.Name }
            else { $value = if ($state.beforeKind -eq 'DWord') { [int]$state.beforeValue } else { $state.beforeValue }; Set-ShellVisibilityValue $spec.Key $spec.Name $value $state.beforeKind }
            if (-not (Test-ShellVisibilityValue (Get-ShellVisibilityValue 'LocalMachine' $spec.Key $spec.Name) $state.beforeKind $state.beforeValue)) { throw 'Windows could not verify restoration.' }
            foreach ($name in @('Phase','BeforeKind','BeforeValue','AppliedValue')) { Remove-ShellVisibilityValue $snapshot $name }
            $result = Get-ShellVisibilityState $Item (Get-ShellVisibilityPlatform)
            if ($result.owned -or $result.pending -or $result.conflict) { throw 'The policy was restored, but its backup could not be cleared.' }
        }
        @{ status = 'updated'; verified = $true; item = $result; requiresSignOut = $true }
    }
}

# These three Windows handlers are changed only at their exact registration
# values. Their keys, other values, and globally shared COM classes are kept.
function Get-ContextItemSpec {
    param([string]$Item)
    switch ($Item) {
        'IncludeLibrary' { return @{ Id = '{3dad6c5d-2167-4cae-9914-f99e41c12cfa}'; Paths = @('SOFTWARE\Classes\Folder\ShellEx\ContextMenuHandlers\Library Location') } }
        'GiveAccess' { return @{ Id = '{f81e9010-6ea4-11ce-a7ff-00aa003ca9f6}'; Paths = @('SOFTWARE\Classes\*\shellex\ContextMenuHandlers\Sharing', 'SOFTWARE\Classes\Directory\Background\shellex\ContextMenuHandlers\Sharing', 'SOFTWARE\Classes\Directory\shellex\ContextMenuHandlers\Sharing', 'SOFTWARE\Classes\Drive\shellex\ContextMenuHandlers\Sharing', 'SOFTWARE\Classes\LibraryFolder\background\shellex\ContextMenuHandlers\Sharing', 'SOFTWARE\Classes\UserLibraryFolder\shellex\ContextMenuHandlers\Sharing') } }
        'Share' { return @{ Id = '{e2bf9676-5f8f-435c-97eb-11607a5bedf7}'; Paths = @('SOFTWARE\Classes\*\shellex\ContextMenuHandlers\ModernSharing') } }
        default { throw 'Unknown right-click item.' }
    }
}

function Get-ContextItemAccountSid { return [Security.Principal.WindowsIdentity]::GetCurrent().User.Value }

function Get-ContextHandlerValue {
    param([string]$Hive, [string]$Key)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, 'Registry64')
    try {
        $sub = $base.OpenSubKey($Key, $false)
        if ($null -eq $sub) { return @{ keyExists = $false; exists = $false; value = $null; kind = $null } }
        try {
            if ('' -notin $sub.GetValueNames()) { return @{ keyExists = $true; exists = $false; value = $null; kind = $null } }
            return @{ keyExists = $true; exists = $true; value = $sub.GetValue('', $null, 'DoNotExpandEnvironmentNames'); kind = [string]$sub.GetValueKind('') }
        } finally { $sub.Dispose() }
    } finally { $base.Dispose() }
}

function Set-ContextHandlerValue {
    param([string]$Hive, [string]$Key, [string]$Value)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, 'Registry64')
    try {
        $sub = $base.OpenSubKey($Key, $true)
        if ($null -eq $sub) { throw 'A right-click registration no longer exists.' }
        try {
            if ('' -in $sub.GetValueNames()) { throw 'A right-click registration appeared before restoration.' }
            $sub.SetValue('', $Value, 'String'); $sub.Flush()
        } finally { $sub.Dispose() }
    } finally { $base.Dispose() }
}

function Remove-ContextHandlerValue {
    param([string]$Hive, [string]$Key, [string]$Expected)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, 'Registry64')
    try {
        $sub = $base.OpenSubKey($Key, $true)
        if ($null -eq $sub) { throw 'A right-click registration no longer exists.' }
        try {
            if ('' -notin $sub.GetValueNames() -or $sub.GetValueKind('') -ne 'String' -or [string]$sub.GetValue('') -ine $Expected) { throw 'A right-click registration changed before removal.' }
            $sub.DeleteValue('', $false); $sub.Flush()
        } finally { $sub.Dispose() }
    } finally { $base.Dispose() }
}

function Get-ContextItemState {
    param([string]$Item)
    $spec = Get-ContextItemSpec $Item
    $snapshot = "SOFTWARE\WinCommander\ContextItemVisibility\$Item"
    $phase = Get-ShellVisibilityValue 'LocalMachine' $snapshot 'Phase'
    $saved = Get-ShellVisibilityValue 'LocalMachine' $snapshot 'Snapshot'
    $targets = @(
        foreach ($path in $spec.Paths) {
            foreach ($hive in @('LocalMachine','CurrentUser')) {
                $current = Get-ContextHandlerValue $hive $path
                [pscustomobject]@{ Hive = $hive; Path = $path; Current = $current }
            }
        }
    )
    $hasSnapshot = $phase.exists -or $saved.exists
    $valid = $phase.exists -and $phase.kind -eq 'DWord' -and $phase.value -in @(1,2) -and $saved.exists -and $saved.kind -eq 'String' -and $saved.value.Length -le 16384
    $records = @(); $sameAccount = $true
    if ($valid) {
        try {
            $document = $saved.value | ConvertFrom-Json -ErrorAction Stop
            $sameAccount = [string]$document.accountSid -ceq (Get-ContextItemAccountSid)
            $records = @($document.records)
            if ($records.Count -ne $targets.Count) { $valid = $false }
            if (@($records | Where-Object { $_.present -eq $true }).Count -eq 0) { $valid = $false }
            if ($valid) {
                for ($i = 0; $i -lt $targets.Count; $i++) {
                    $record = $records[$i]; $target = $targets[$i]
                    if ($record.hive -cne $target.Hive -or $record.path -cne $target.Path -or $record.present -isnot [bool] -or ($record.present -and ($record.value -isnot [string] -or $record.value -ine $spec.Id)) -or (-not $record.present -and $null -ne $record.value)) { $valid = $false; break }
                }
            }
        } catch { $valid = $false }
    }
    $conflict = $hasSnapshot -and (-not $valid -or -not $sameAccount)
    $detected = 0
    for ($i = 0; $i -lt $targets.Count; $i++) {
        $current = $targets[$i].Current
        if ($current.exists) { $detected++ }
        if (-not $valid) {
            if ($current.exists -and ($current.kind -ne 'String' -or [string]$current.value -ine $spec.Id)) { $conflict = $true }
        } else {
            $record = $records[$i]
            if ($record.present -and -not $current.keyExists) { $conflict = $true }
            if ($current.exists -and (-not $record.present -or $current.kind -ne 'String' -or [string]$current.value -cne [string]$record.value -or $phase.value -eq 2)) { $conflict = $true }
        }
    }
    return @{ item = $Item; detected = $detected; owned = [bool]($valid -and $sameAccount -and $phase.value -eq 2); pending = [bool]($valid -and $sameAccount -and $phase.value -eq 1); conflict = [bool]$conflict; otherAccount = [bool]($valid -and -not $sameAccount); records = if ($valid -and $sameAccount) { $records } else { @() } }
}

function Get-ContextItemVisibility {
    return Invoke-WithShellVisibilityMutex {
        @{ isAdmin = [bool](Test-IsAdmin); items = @((Get-ContextItemState 'IncludeLibrary'), (Get-ContextItemState 'GiveAccess'), (Get-ContextItemState 'Share')) }
    }
}

function Set-ContextItemVisibility {
    param([Parameter(Mandatory = $true)][ValidateSet('IncludeLibrary','GiveAccess','Share')][string]$Item, [Parameter(Mandatory = $true)][ValidateSet('hide','restore')][string]$Mode)
    Assert-IsAdmin
    return Invoke-WithShellVisibilityMutex {
        $spec = Get-ContextItemSpec $Item
        $snapshot = "SOFTWARE\WinCommander\ContextItemVisibility\$Item"
        $state = Get-ContextItemState $Item
        if ($state.conflict) { throw 'A right-click registration or its backup changed outside WinCommander. No entry was changed.' }
        if ($Mode -eq 'hide') {
            if (-not $state.owned -and -not $state.pending) {
                if ($state.detected -eq 0) { throw 'This right-click registration was not detected on this PC.' }
                $records = @(
                    foreach ($path in $spec.Paths) {
                        foreach ($hive in @('LocalMachine','CurrentUser')) {
                            $value = Get-ContextHandlerValue $hive $path
                            if ($value.exists -and ($value.kind -ne 'String' -or [string]$value.value -ine $spec.Id)) { throw 'A right-click registration changed before it could be saved.' }
                            [pscustomobject]@{ hive = $hive; path = $path; present = [bool]$value.exists; value = if ($value.exists) { [string]$value.value } else { $null } }
                        }
                    }
                )
                $json = ConvertTo-Json -InputObject @{ accountSid = Get-ContextItemAccountSid; records = $records } -Depth 5 -Compress
                Set-ShellVisibilityValue $snapshot 'Snapshot' $json 'String'
                Set-ShellVisibilityValue $snapshot 'Phase' 1 'DWord'
                $state = Get-ContextItemState $Item
                if (-not $state.pending -or $state.conflict) { throw 'Windows could not confirm the right-click backup.' }
            }
            if (-not $state.owned) {
                foreach ($record in $state.records) {
                    if ($record.present) {
                        $current = Get-ContextHandlerValue $record.hive $record.path
                        if ($current.exists) { Remove-ContextHandlerValue $record.hive $record.path $record.value }
                    }
                }
                $state = Get-ContextItemState $Item
                if ($state.conflict -or $state.detected -ne 0) { throw 'Windows could not verify that the selected registrations were hidden.' }
                Set-ShellVisibilityValue $snapshot 'Phase' 2 'DWord'
            }
            $result = Get-ContextItemState $Item
            if (-not $result.owned -or $result.conflict) { throw 'Windows could not verify the right-click change.' }
        } else {
            if (-not ($state.owned -or $state.pending)) { throw 'WinCommander has no right-click change to restore.' }
            Set-ShellVisibilityValue $snapshot 'Phase' 1 'DWord'
            foreach ($record in $state.records) {
                if ($record.present) {
                    $current = Get-ContextHandlerValue $record.hive $record.path
                    if (-not $current.exists) { Set-ContextHandlerValue $record.hive $record.path $record.value }
                }
            }
            $state = Get-ContextItemState $Item
            if ($state.conflict) { throw 'Windows could not verify the restored registrations.' }
            foreach ($record in $state.records) {
                $current = Get-ContextHandlerValue $record.hive $record.path
                if ($record.present -ne $current.exists -or ($record.present -and [string]$record.value -cne [string]$current.value)) { throw 'Windows could not verify the restored registrations.' }
            }
            foreach ($name in @('Phase','Snapshot')) { Remove-ShellVisibilityValue $snapshot $name }
            $result = Get-ContextItemState $Item
            if ($result.owned -or $result.pending -or $result.conflict) { throw 'The entries were restored, but their backup could not be cleared.' }
        }
        @{ status = 'updated'; verified = $true; item = $result; requiresSignOut = $true }
    }
}

# --- BACKGROUND APPS ---

function Disable-BackgroundApps {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications' -Name 'GlobalUserDisabled' -Value 1 -Type DWord -Force
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Search' -Name 'BackgroundAppGlobalToggle' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-BackgroundApps {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications' -Name 'GlobalUserDisabled' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Search' -Name 'BackgroundAppGlobalToggle' -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- NOTIFICATIONS ---

function Disable-Notifications {
    Assert-IsAdmin
    $path = 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer'
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'DisableNotificationCenter' -Value 1 -Type DWord -Force
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\PushNotifications' -Name 'ToastEnabled' -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-Notifications {
    Assert-IsAdmin
    Invoke-7Erase -Path 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\Explorer' -Type RegistryProperty -Name 'DisableNotificationCenter'
    Set-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\PushNotifications' -Name 'ToastEnabled' -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- CONTEXT MENU ---

function Enable-ClassicContextMenu {
    Assert-IsAdmin
    $path = 'HKCU:\SOFTWARE\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32'
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name '(Default)' -Value '' -Force
    Restart-Explorer | Out-Null
    @{ status = 'enabled'; requiresRestart = $true }
}

function Disable-ClassicContextMenu {
    Assert-IsAdmin
    Invoke-7Erase -Path 'HKCU:\SOFTWARE\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}' -Type Registry
    Restart-Explorer | Out-Null
    @{ status = 'disabled'; requiresRestart = $true }
}

# --- TASKBAR ---

function Enable-EndTaskOnTaskbar {
    if (-not (Test-ExplorerShellAvailable)) {
        return @{ status = 'unsupported'; profilesUpdated = 0; requiresSignOut = $false; warning = 'End Task on Taskbar requires Explorer Desktop Experience and is unavailable on Windows Server Core.' }
    }
    $updated = Set-AllUserExplorerDword -SubKey 'Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings' -Name 'TaskbarEndTask' -Value 1
    $restart = Restart-Explorer -AllUsers
    @{ status = 'enabled'; profilesUpdated = $updated; shellRefresh = $restart; requiresSignOut = $true }
}

function Disable-EndTaskOnTaskbar {
    if (-not (Test-ExplorerShellAvailable)) {
        return @{ status = 'unsupported'; profilesUpdated = 0; requiresSignOut = $false; warning = 'End Task on Taskbar requires Explorer Desktop Experience and is unavailable on Windows Server Core.' }
    }
    $updated = Set-AllUserExplorerDword -SubKey 'Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings' -Name 'TaskbarEndTask' -Value 0 -Remove
    $restart = Restart-Explorer -AllUsers
    @{ status = 'disabled'; profilesUpdated = $updated; shellRefresh = $restart; requiresSignOut = $true }
}

# --- SEARCH ---

function Disable-BingSearch {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search"
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name "BingSearchEnabled" -Value 0 -Type DWord -Force
    @{ status = 'disabled' }
}

function Enable-BingSearch {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search"
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name "BingSearchEnabled" -Value 1 -Type DWord -Force
    @{ status = 'enabled' }
}

# --- EXPLORER ---

function Show-FileExtensions {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "HideFileExt" -Value 0 -Type DWord -Force
    Restart-Explorer -AllUsers | Out-Null
    @{ status = 'shown' }
}

function Hide-FileExtensions {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "HideFileExt" -Value 1 -Type DWord -Force
    Restart-Explorer -AllUsers | Out-Null
    @{ status = 'hidden' }
}

function Show-HiddenFiles {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "Hidden" -Value 1 -Type DWord -Force
    Restart-Explorer | Out-Null
    @{ status = 'shown' }
}

function Hide-HiddenFiles {
    $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    Set-ItemProperty -Path $path -Name "Hidden" -Value 0 -Type DWord -Force
    Restart-Explorer | Out-Null
    @{ status = 'hidden' }
}

# --- EXPLORER CLUTTER ---

function Enable-RemoveGalleryHome {
    Assert-IsAdmin
    # 1. Remove Home (Win11 specific CLSID)
    $homeCLSID = "{f874310e-b6b7-47dc-bc84-b9e6b38f5903}"
    $homeNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$homeCLSID"
    if (Test-Path $homeNS) { Invoke-7Erase -Path $homeNS -Type Registry }
    
    # Force Explorer to "This PC" (LaunchTo = 1)
    Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name LaunchTo -Value 1 -Force
    
    # 2. Remove Gallery (Win11 specific CLSID)
    $galleryCLSID = "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}"
    $galleryNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$galleryCLSID"
    if (Test-Path $galleryNS) { Invoke-7Erase -Path $galleryNS -Type Registry }

    # Legacy method for HubMode as fallback
    Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "HubMode" -Value 1 -Type DWord -Force
    
    Restart-Explorer | Out-Null
    @{ status = 'removed' }
}

function Disable-RemoveGalleryHome {
    Assert-IsAdmin
    # 1. Restore Home
    $homeCLSID = "{f874310e-b6b7-47dc-bc84-b9e6b38f5903}"
    $homeNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$homeCLSID"
    if (!(Test-Path $homeNS)) { New-Item $homeNS -Force | Out-Null }
    Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name LaunchTo -Value 0 -Force
    
    # 2. Restore Gallery
    $galleryCLSID = "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}"
    $galleryNS = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\$galleryCLSID"
    if (!(Test-Path $galleryNS)) { New-Item $galleryNS -Force | Out-Null }

    Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "HubMode" -Value 0 -Type DWord -Force
    
    Restart-Explorer | Out-Null
    @{ status = 'restored' }
}

# ============================================================================
# NEW: Explorer Enhancements (from ReviOS explorer.yml)
# ============================================================================

# --- Folder Type Discovery ---
function Disable-FolderTypeDiscovery {
    try {
        $path = "HKCU:\SOFTWARE\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags\AllFolders\Shell"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "FolderType" -Value "NotSpecified" -Type String -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-FolderTypeDiscovery {
    try {
        $path = "HKCU:\SOFTWARE\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags\AllFolders\Shell"
        if (Test-Path $path) {
            Remove-ItemSecure -Path $path -Name "FolderType" -ErrorAction SilentlyContinue
        }
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Shortcut Suffix ---
function Remove-ShortcutSuffix {
    try {
        Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "link" -Value ([byte[]](0x00,0x00,0x00,0x00)) -Type Binary -Force
        @{ status = "removed" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Restore-ShortcutSuffix {
    try {
        Remove-ItemSecure -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" -Name "link" -ErrorAction SilentlyContinue
        @{ status = "restored" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- AutoPlay ---
function Disable-AutoPlay {
    try {
        $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "DisableAutoplay" -Value 1 -Type DWord -Force
        # Also disable AutoRun
        $polPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"
        if (!(Test-Path $polPath)) { New-Item -Path $polPath -Force | Out-Null }
        Set-ItemProperty -Path $polPath -Name "NoDriveTypeAutoRun" -Value 255 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutoPlay {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers" -Name "DisableAutoplay" -Value 0 -Type DWord -Force
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer" -Name "NoDriveTypeAutoRun" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Low Disk Space Check ---
function Disable-LowDiskCheck {
    try {
        $path = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "NoLowDiskSpaceChecks" -Value 1 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-LowDiskCheck {
    try {
        Remove-ItemSecure -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer" -Name "NoLowDiskSpaceChecks" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Explorer Opens This PC ---
function Set-ExplorerOpensThisPC {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "LaunchTo" -Value 1 -Type DWord -Force
        @{ status = "thispc" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Set-ExplorerOpensQuickAccess {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "LaunchTo" -Value 2 -Type DWord -Force
        @{ status = "quickaccess" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Sync Provider Notifications ---
function Hide-SyncProviderNotifications {
    try {
        Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSyncProviderNotifications" -Value 0 -Type DWord -Force
        @{ status = "hidden" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Show-SyncProviderNotifications {
    try {
        Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "ShowSyncProviderNotifications" -Value 1 -Type DWord -Force
        @{ status = "shown" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Transparency Effects ---
function Disable-TransparencyEffects {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name "EnableTransparency" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-TransparencyEffects {
    try {
        Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name "EnableTransparency" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Full Path in Title Bar ---
function Enable-FullPathInTitleBar {
    try {
        $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "FullPath" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-FullPathInTitleBar {
    try {
        $path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState"
        if (Test-Path $path) {
            Set-ItemProperty -Path $path -Name "FullPath" -Value 0 -Type DWord -Force
        }
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Taskbar Debloat (always-on: hide widgets, task view, search, chat, people bar, meet now) ---
function Set-TaskbarDebloated {
    Assert-IsAdmin
    if (-not (Test-ExplorerShellAvailable)) {
        return @{ status = 'unsupported'; profilesUpdated = 0; requiresSignOut = $false; warning = 'Taskbar Debloat requires Explorer Desktop Experience and is unavailable on Windows Server Core.' }
    }
    try {
        # Taskbar choices are HKCU-only, so apply them to every known profile
        # plus Default instead of leaving an RDS host half-configured.
        $profilesUpdated = 0
        foreach ($entry in @(
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'ShowTaskViewButton', 0),
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'TaskbarDa', 0),
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'TaskbarMn', 0),
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'TaskbarAl', 0),
            @('Software\Microsoft\Windows\CurrentVersion\Search', 'SearchboxTaskbarMode', 0),
            @('Software\Policies\Microsoft\Windows\Explorer', 'HidePeopleBar', 1),
            @('Software\Microsoft\Windows\CurrentVersion\Policies\Explorer', 'HideSCAMeetNow', 1)
        )) {
            $profilesUpdated = [Math]::Max($profilesUpdated, (Set-AllUserExplorerDword -SubKey $entry[0] -Name $entry[1] -Value $entry[2]))
        }

        # Policy-level blocks
        $feedsPath = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Feeds"
        if (!(Test-Path $feedsPath)) { New-Item -Path $feedsPath -Force | Out-Null }
        Set-ItemProperty -Path $feedsPath -Name "EnableFeeds" -Value 0 -Type DWord -Force

        $chatPath = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Chat"
        if (!(Test-Path $chatPath)) { New-Item -Path $chatPath -Force | Out-Null }
        Set-ItemProperty -Path $chatPath -Name "ChatIcon" -Value 3 -Type DWord -Force

        $dshPath = "HKLM:\SOFTWARE\Policies\Microsoft\Dsh"
        if (!(Test-Path $dshPath)) { New-Item -Path $dshPath -Force | Out-Null }
        Set-ItemProperty -Path $dshPath -Name "AllowNewsAndInterests" -Value 0 -Type DWord -Force

        $shellRefresh = Restart-Explorer -AllUsers
        @{ status = "debloated"; profilesUpdated = $profilesUpdated; shellRefresh = $shellRefresh; requiresSignOut = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Reset-TaskbarDebloated {
    Assert-IsAdmin
    if (-not (Test-ExplorerShellAvailable)) {
        return @{ status = 'unsupported'; profilesUpdated = 0; requiresSignOut = $false; warning = 'Taskbar Debloat requires Explorer Desktop Experience and is unavailable on Windows Server Core.' }
    }
    try {
        $profilesUpdated = 0
        foreach ($entry in @(
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'ShowTaskViewButton', 1, $false),
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'TaskbarDa', 0, $true),
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'TaskbarMn', 0, $true),
            @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'TaskbarAl', 1, $false),
            @('Software\Microsoft\Windows\CurrentVersion\Search', 'SearchboxTaskbarMode', 2, $false),
            @('Software\Policies\Microsoft\Windows\Explorer', 'HidePeopleBar', 0, $true),
            @('Software\Microsoft\Windows\CurrentVersion\Policies\Explorer', 'HideSCAMeetNow', 0, $true)
        )) {
            $params = @{ SubKey = $entry[0]; Name = $entry[1]; Value = $entry[2] }
            if ($entry[3]) { $params['Remove'] = $true }
            $profilesUpdated = [Math]::Max($profilesUpdated, (Set-AllUserExplorerDword @params))
        }
        $shellRefresh = Restart-Explorer -AllUsers
        @{ status = "reset"; profilesUpdated = $profilesUpdated; shellRefresh = $shellRefresh; requiresSignOut = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- Start Menu Recommendations ---
function Disable-StartRecommendations {
    try {
        $adv = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
        Set-ItemProperty -Path $adv -Name "Start_IrisRecommendations" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $adv -Name "Start_AccountNotifications" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-StartRecommendations {
    try {
        $adv = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
        Set-ItemProperty -Path $adv -Name "Start_IrisRecommendations" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $adv -Name "Start_AccountNotifications" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Take Ownership Context Menu
# ============================================================================

function Enable-TakeOwnershipMenu {
    Assert-IsAdmin
    try {
        # Add "Take Ownership" to right-click context menu for files
        $filePath = "HKLM:\SOFTWARE\Classes\*\shell\TakeOwnership"
        if (!(Test-Path $filePath)) { New-Item -Path $filePath -Force | Out-Null }
        Set-ItemProperty -Path $filePath -Name "(Default)" -Value "Take Ownership" -Force
        Set-ItemProperty -Path $filePath -Name "HasLUAShield" -Value "" -Force
        Set-ItemProperty -Path $filePath -Name "NoWorkingDirectory" -Value "" -Force
        $fileCmd = "$filePath\command"
        if (!(Test-Path $fileCmd)) { New-Item -Path $fileCmd -Force | Out-Null }
        Set-ItemProperty -Path $fileCmd -Name "(Default)" -Value 'cmd.exe /c takeown /f "%1" && icacls "%1" /grant administrators:F /c /l' -Force
        Set-ItemProperty -Path $fileCmd -Name "IsolatedCommand" -Value 'cmd.exe /c takeown /f "%1" && icacls "%1" /grant administrators:F /c /l' -Force

        # Add for directories
        $dirPath = "HKLM:\SOFTWARE\Classes\Directory\shell\TakeOwnership"
        if (!(Test-Path $dirPath)) { New-Item -Path $dirPath -Force | Out-Null }
        Set-ItemProperty -Path $dirPath -Name "(Default)" -Value "Take Ownership" -Force
        Set-ItemProperty -Path $dirPath -Name "HasLUAShield" -Value "" -Force
        Set-ItemProperty -Path $dirPath -Name "NoWorkingDirectory" -Value "" -Force
        $dirCmd = "$dirPath\command"
        if (!(Test-Path $dirCmd)) { New-Item -Path $dirCmd -Force | Out-Null }
        Set-ItemProperty -Path $dirCmd -Name "(Default)" -Value 'cmd.exe /c takeown /f "%1" /r /d y && icacls "%1" /grant administrators:F /t /c /l' -Force
        Set-ItemProperty -Path $dirCmd -Name "IsolatedCommand" -Value 'cmd.exe /c takeown /f "%1" /r /d y && icacls "%1" /grant administrators:F /t /c /l' -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-TakeOwnershipMenu {
    Assert-IsAdmin
    try {
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Classes\*\shell\TakeOwnership" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKLM:\SOFTWARE\Classes\Directory\shell\TakeOwnership" -Recurse -Force -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Enthusiast Mode — Verbose file operation dialogs
# ============================================================================

function Enable-EnthusiastMode {
    try {
        $opPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager"
        if (!(Test-Path $opPath)) { New-Item -Path $opPath -Force | Out-Null }
        Set-ItemProperty -Path $opPath -Name "EnthusiastMode" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-EnthusiastMode {
    try {
        Remove-ItemSecure -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager" -Name "EnthusiastMode" -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Instant Start Menu (MenuShowDelay = 0)
# ============================================================================

function Enable-InstantMenuDelay {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "MenuShowDelay" -Value "0" -Type String -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-InstantMenuDelay {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "MenuShowDelay" -Value "400" -Type String -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Wallpaper JPEG Quality (prevent compression artifacts)
# ============================================================================

function Enable-WallpaperQuality {
    try {
        $path = "HKCU:\Control Panel\Desktop"
        Set-ItemProperty -Path $path -Name "JPEGImportQuality" -Value 100 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-WallpaperQuality {
    try {
        Remove-ItemSecure -Path "HKCU:\Control Panel\Desktop" -Name "JPEGImportQuality" -ErrorAction SilentlyContinue
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Disable Accessibility Shortcuts (StickyKeys, ToggleKeys, FilterKeys)
# ============================================================================

function Disable-AccessibilityShortcuts {
    try {
        # StickyKeys: flags 506 = disabled (no sound, no popup, no enable)
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\StickyKeys" -Name "Flags" -Value "506" -Type String -Force
        # ToggleKeys
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\ToggleKeys" -Name "Flags" -Value "58" -Type String -Force
        # FilterKeys
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\Keyboard Response" -Name "Flags" -Value "122" -Type String -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AccessibilityShortcuts {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\StickyKeys" -Name "Flags" -Value "510" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\ToggleKeys" -Name "Flags" -Value "62" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Accessibility\Keyboard Response" -Name "Flags" -Value "126" -Type String -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Disable Mouse Acceleration (Enhance Pointer Precision)
# ============================================================================

function Disable-MouseAcceleration {
    try {
        # Set flat acceleration curve (1:1 mouse movement)
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseSpeed" -Value "0" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold1" -Value "0" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold2" -Value "0" -Type String -Force
        # Disable "Enhance pointer precision"
        $accelCurve = [byte[]](0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0xC0,0xCC,0x0C,0x00,0x00,0x00,0x00,0x00,
                               0x80,0x99,0x19,0x00,0x00,0x00,0x00,0x00,
                               0x40,0x66,0x26,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x33,0x33,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x40,0x00,0x00,0x00,0x00,0x00,
                               0x00,0xCC,0x4C,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x99,0x59,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x66,0x66,0x00,0x00,0x00,0x00,0x00)
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseXCurve" -Value $accelCurve -Type Binary -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseYCurve" -Value $accelCurve -Type Binary -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-MouseAcceleration {
    try {
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseSpeed" -Value "1" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold1" -Value "6" -Type String -Force
        Set-ItemProperty -Path "HKCU:\Control Panel\Mouse" -Name "MouseThreshold2" -Value "10" -Type String -Force
        Remove-ItemSecure -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseXCurve" -ErrorAction SilentlyContinue
        Remove-ItemSecure -Path "HKCU:\Control Panel\Mouse" -Name "SmoothMouseYCurve" -ErrorAction SilentlyContinue
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Disable Autocorrect / Spellcheck / Text Predictions
# ============================================================================

function Disable-AutocorrectSpellcheck {
    try {
        $path = "HKCU:\Software\Microsoft\TabletTip\1.7"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "EnableAutocorrection" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableSpellchecking" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableTextPrediction" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnablePredictionSpaceInsertion" -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableDoubleTapSpace" -Value 0 -Type DWord -Force
        @{ status = "disabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-AutocorrectSpellcheck {
    try {
        $path = "HKCU:\Software\Microsoft\TabletTip\1.7"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "EnableAutocorrection" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableSpellchecking" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableTextPrediction" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnablePredictionSpaceInsertion" -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name "EnableDoubleTapSpace" -Value 1 -Type DWord -Force
        @{ status = "enabled" }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# Make PowerShell 7 the Windows Terminal default profile
#
# Scope: only Windows Terminal's own "defaultProfile" setting (which profile
# opens with no explicit profile argument -- Win+X "Terminal", the modern
# Explorer "Open in Terminal" verb). Deliberately does NOT touch the separate
# Windows 11 "Default Terminal Application" registry axis
# (HKCU:\Console\%%Startup DelegationConsole/DelegationTerminal), which
# repoints the host for every console app system-wide and is documented as
# unreliable under elevated processes -- this app always runs elevated.
# ============================================================================

function Get-WindowsTerminalSettingsPath {
    $packaged = Get-ChildItem -Path "$env:LOCALAPPDATA\Packages" -Filter 'Microsoft.WindowsTerminal_*' -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notmatch 'Preview|Canary' } |
        Select-Object -First 1
    if ($packaged) {
        $path = Join-Path $packaged.FullName 'LocalState\settings.json'
        if (Test-Path -LiteralPath $path -PathType Leaf) { return $path }
    }
    $unpackaged = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows Terminal\settings.json'
    if (Test-Path -LiteralPath $unpackaged -PathType Leaf) { return $unpackaged }
    return $null
}

function ConvertFrom-WindowsTerminalSettings {
    # Windows Terminal ships a leading "// ..." comment header and users
    # sometimes hand-edit trailing commas; both break strict ConvertFrom-Json.
    # This parsed copy is discovery-only (finding a profile's GUID) and is
    # never written back, so stripping comments/trailing-commas here can
    # never damage the user's actual on-disk formatting.
    param([string]$RawJson)
    $stripped = ($RawJson -split "`r?`n" | Where-Object { $_.TrimStart() -notmatch '^//' }) -join "`n"
    $stripped = [regex]::Replace($stripped, ',(\s*[}\]])', '$1')
    try { $stripped | ConvertFrom-Json -ErrorAction Stop } catch { $null }
}

function Find-PowerShell7ProfileGuid {
    param($ParsedSettings)
    if ($ParsedSettings.disabledProfileSources -contains 'Windows.Terminal.PowershellCore') { return $null }
    $profiles = @($ParsedSettings.profiles.list)
    $match = $profiles | Where-Object { $_.source -eq 'Windows.Terminal.PowershellCore' } | Select-Object -First 1
    if (-not $match) { $match = $profiles | Where-Object { $_.commandline -match 'pwsh\.exe' } | Select-Object -First 1 }
    if (-not $match) { $match = $profiles | Where-Object { $_.name -eq 'PowerShell' } | Select-Object -First 1 }
    if ($match) { return $match.guid }
    return $null
}

function Find-WindowsPowerShellProfileGuid {
    param($ParsedSettings)
    $profiles = @($ParsedSettings.profiles.list)
    $match = $profiles | Where-Object { $_.name -eq 'Windows PowerShell' } | Select-Object -First 1
    if ($match) { return $match.guid }
    # Windows Terminal's fixed, non-dynamic GUID for the built-in Windows PowerShell profile.
    return '{61c54bbd-c2c6-5271-96e7-009a87ff44bf}'
}

function Get-PowerShell7DefaultStatus {
    $settingsPath = Get-WindowsTerminalSettingsPath
    if (-not $settingsPath) { return @{ default = $false; available = $false; reason = 'Windows Terminal is not installed' } }
    $parsed = ConvertFrom-WindowsTerminalSettings -RawJson ([System.IO.File]::ReadAllText($settingsPath))
    if (-not $parsed) { return @{ default = $false; available = $false; reason = 'Could not read Windows Terminal settings' } }
    $ps7Guid = Find-PowerShell7ProfileGuid -ParsedSettings $parsed
    if (-not $ps7Guid) {
        return @{ default = $false; available = $false; reason = 'PowerShell 7 profile not found -- open Windows Terminal once after installing PowerShell 7' }
    }
    return @{ default = ($parsed.defaultProfile -eq $ps7Guid); available = $true }
}

function Enable-PowerShell7DefaultShell {
    if (-not (Test-PowerShell7Installed).installed) {
        return @{ error = $true; message = 'PowerShell 7 is not installed' }
    }
    $settingsPath = Get-WindowsTerminalSettingsPath
    if (-not $settingsPath) { return @{ error = $true; message = 'Windows Terminal is not installed' } }
    $raw = [System.IO.File]::ReadAllText($settingsPath)
    $parsed = ConvertFrom-WindowsTerminalSettings -RawJson $raw
    if (-not $parsed) { return @{ error = $true; message = 'Could not read Windows Terminal settings' } }
    $ps7Guid = Find-PowerShell7ProfileGuid -ParsedSettings $parsed
    if (-not $ps7Guid) {
        return @{ error = $true; message = 'PowerShell 7 profile not found in Windows Terminal -- open Windows Terminal once after installing PowerShell 7, then try again' }
    }
    # Surgical text replace of only the defaultProfile value -- preserves the
    # user's comments, key order, and every other hand-edited setting. Uses
    # plain substring surgery (not a regex Replace-all) so a hand-trimmed
    # file with no defaultProfile key can't have every "{" in the JSON
    # corrupted by a naive first-brace regex substitution.
    $keyPattern = [regex]'"defaultProfile"\s*:\s*"[^"]*"'
    $existingMatch = $keyPattern.Match($raw)
    $updated = if ($existingMatch.Success) {
        $raw.Substring(0, $existingMatch.Index) + "`"defaultProfile`": `"$ps7Guid`"" + $raw.Substring($existingMatch.Index + $existingMatch.Length)
    } else {
        $braceIndex = $raw.IndexOf('{')
        $raw.Substring(0, $braceIndex + 1) + "`n    `"defaultProfile`": `"$ps7Guid`"," + $raw.Substring($braceIndex + 1)
    }
    [System.IO.File]::WriteAllText($settingsPath, $updated)
    @{ status = 'enabled' }
}

function Disable-PowerShell7DefaultShell {
    $settingsPath = Get-WindowsTerminalSettingsPath
    if (-not $settingsPath) { return @{ status = 'disabled' } }
    $raw = [System.IO.File]::ReadAllText($settingsPath)
    $parsed = ConvertFrom-WindowsTerminalSettings -RawJson $raw
    if (-not $parsed) { return @{ status = 'disabled' } }
    $winPsGuid = Find-WindowsPowerShellProfileGuid -ParsedSettings $parsed
    $keyPattern = [regex]'"defaultProfile"\s*:\s*"[^"]*"'
    $existingMatch = $keyPattern.Match($raw)
    if ($existingMatch.Success) {
        $updated = $raw.Substring(0, $existingMatch.Index) + "`"defaultProfile`": `"$winPsGuid`"" + $raw.Substring($existingMatch.Index + $existingMatch.Length)
        [System.IO.File]::WriteAllText($settingsPath, $updated)
    }
    @{ status = 'disabled' }
}
