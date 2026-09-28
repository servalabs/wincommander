# ============================================================================
# TWEAKS - LOCAL USERS
# Hide selected local accounts from the Windows welcome/login screen.
# ============================================================================

$script:SpecialAccountsPath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts'
$script:UserListPath = "$script:SpecialAccountsPath\UserList"

function Test-ValidLocalLoginUserName {
    param([string]$Name)
    if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
    if ($Name.Length -gt 256) { return $false }
    return ($Name -notmatch '[\\/\[\]:;\|=,\+\*\?<>@"\x00-\x1F]')
}

function Test-BuiltInLocalUserSid {
    param([string]$Sid)
    if ([string]::IsNullOrWhiteSpace($Sid)) { return $false }
    return ($Sid -match '-(500|501|503|504)$')
}

function Get-UserListRegistryValues {
    if (Test-Path $script:UserListPath) {
        return Get-ItemProperty -Path $script:UserListPath -ErrorAction SilentlyContinue
    }
    return $null
}

function ConvertTo-LocalLoginUserRow {
    param(
        [Parameter(Mandatory = $true)]$Account,
        $UserListValues
    )

    $prop = $null
    if ($UserListValues) {
        $prop = $UserListValues.PSObject.Properties[$Account.Name]
    }

    $hidden = $false
    if ($null -ne $prop) {
        try { $hidden = ([int]$prop.Value -eq 0) } catch { $hidden = $false }
    }

    @{
        name            = [string]$Account.Name
        fullName        = if ($Account.FullName) { [string]$Account.FullName } else { "" }
        description     = if ($Account.Description) { [string]$Account.Description } else { "" }
        enabled         = (-not [bool]$Account.Disabled)
        hiddenFromLogin = [bool]$hidden
        builtIn         = [bool](Test-BuiltInLocalUserSid -Sid ([string]$Account.SID))
        currentUser     = ([string]$Account.Name -ieq [Environment]::UserName)
        sid             = [string]$Account.SID
    }
}

function Get-LocalLoginAccount {
    param([Parameter(Mandatory = $true)][string]$Name)
    $accounts = @(Get-CimInstance Win32_UserAccount -Filter "LocalAccount = True" -ErrorAction Stop)
    return $accounts | Where-Object { $_.Name -ieq $Name } | Select-Object -First 1
}

function Get-LocalLoginUsers {
    Assert-IsAdmin
    try {
        $values = Get-UserListRegistryValues
        $rows = New-Object System.Collections.Generic.List[hashtable]
        $accounts = @(Get-CimInstance Win32_UserAccount -Filter "LocalAccount = True" -ErrorAction Stop)
        foreach ($account in ($accounts | Where-Object { $_.Name } | Sort-Object Name)) {
            $rows.Add((ConvertTo-LocalLoginUserRow -Account $account -UserListValues $values))
        }
        return $rows.ToArray()
    }
    catch {
        @{ error = $true; message = "Unable to enumerate local users: $($_.Exception.Message)" }
    }
}

function Set-LocalLoginUserHidden {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][bool]$Hidden
    )

    Assert-IsAdmin
    $trimmed = $Name.Trim()
    if (-not (Test-ValidLocalLoginUserName -Name $trimmed)) {
        throw "Invalid local user name."
    }

    $account = Get-LocalLoginAccount -Name $trimmed
    if (-not $account) {
        throw "Local user not found: $trimmed"
    }

    $canonicalName = [string]$account.Name
    if ($Hidden) {
        if (Test-BuiltInLocalUserSid -Sid ([string]$account.SID)) {
            throw "Built-in Windows accounts cannot be hidden from the login screen."
        }
        if ($canonicalName -ieq [Environment]::UserName) {
            throw "Refusing to hide the currently signed-in account."
        }
        if (!(Test-Path $script:SpecialAccountsPath)) { New-Item -Path $script:SpecialAccountsPath -Force | Out-Null }
        if (!(Test-Path $script:UserListPath)) { New-Item -Path $script:UserListPath -Force | Out-Null }
        Set-ItemProperty -Path $script:UserListPath -Name $canonicalName -Value 0 -Type DWord -Force
    }
    else {
        if (Test-Path $script:UserListPath) {
            Remove-ItemProperty -Path $script:UserListPath -Name $canonicalName -ErrorAction SilentlyContinue
        }
    }

    $values = Get-UserListRegistryValues
    ConvertTo-LocalLoginUserRow -Account $account -UserListValues $values
}

function Get-LocalServiceAccountSids {
    $sids = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($service in @(Get-CimInstance Win32_Service -ErrorAction Stop)) {
        $startName = [string]$service.StartName
        if ([string]::IsNullOrWhiteSpace($startName) -or
            $startName -match '^(LocalSystem|LocalService|NetworkService)$' -or
            $startName -match '^(NT AUTHORITY|NT SERVICE)\\') {
            continue
        }

        if ($startName.StartsWith('.\\')) {
            $startName = "$env:COMPUTERNAME\\$($startName.Substring(2))"
        }
        elseif ($startName -notmatch '\\') {
            $startName = "$env:COMPUTERNAME\\$startName"
        }

        try {
            $sid = ([System.Security.Principal.NTAccount]$startName).Translate([System.Security.Principal.SecurityIdentifier]).Value
            [void]$sids.Add([string]$sid)
        }
        catch {
            # A deleted or remote domain identity cannot match a local SID.
        }
    }
    return ,$sids
}

function Get-LocalPasswordExpirySnapshot {
    $serviceSids = Get-LocalServiceAccountSids
    $localUsersBySid = @{}
    foreach ($user in @(Get-LocalUser -ErrorAction SilentlyContinue)) {
        if ($user.SID) { $localUsersBySid[[string]$user.SID.Value] = $user }
    }

    $eligible = New-Object System.Collections.Generic.List[object]
    $skipped = @{
        builtIn = 0
        disabled = 0
        service = 0
        externalIdentity = 0
        nonUser = 0
    }

    foreach ($account in @(Get-CimInstance Win32_UserAccount -Filter 'LocalAccount = True' -ErrorAction Stop)) {
        $sid = [string]$account.SID
        if ([int]$account.SIDType -ne 1) {
            $skipped.nonUser++
            continue
        }
        if (Test-BuiltInLocalUserSid -Sid $sid) {
            $skipped.builtIn++
            continue
        }
        if ([bool]$account.Disabled) {
            $skipped.disabled++
            continue
        }
        if ($serviceSids.Contains($sid)) {
            $skipped.service++
            continue
        }
        $localUser = $localUsersBySid[$sid]
        if ($localUser -and [string]$localUser.PrincipalSource -match '^(MicrosoftAccount|AzureAD)$') {
            $skipped.externalIdentity++
            continue
        }
        $eligible.Add([pscustomobject]@{
            Sid             = $sid
            PasswordExpires = [bool]$account.PasswordExpires
        })
    }

    return [pscustomobject]@{
        eligible = $eligible.ToArray()
        skipped = $skipped
    }
}

function Get-LocalPasswordExpiryStatus {
    try {
        $snapshot = Get-LocalPasswordExpirySnapshot
        $eligible = @($snapshot.eligible)
        $expiresCount = @($eligible | Where-Object { $_.PasswordExpires }).Count
        $neverExpiresCount = $eligible.Count - $expiresCount
        [pscustomobject]@{
            isAdmin                  = [bool](Test-IsAdmin)
            totalEligible            = $eligible.Count
            passwordExpiresCount     = $expiresCount
            passwordNeverExpiresCount = $neverExpiresCount
            allEligibleNeverExpire   = ($eligible.Count -gt 0 -and $expiresCount -eq 0)
            skipped                  = $snapshot.skipped
        }
    }
    catch {
        @{ error = $true; message = "Unable to inspect local password-expiry settings: $($_.Exception.Message)" }
    }
}

function Set-LocalPasswordNeverExpires {
    param([Parameter(Mandatory = $true)][bool]$Enabled)

    Assert-IsAdmin
    $snapshot = Get-LocalPasswordExpirySnapshot
    $changed = 0
    $failed = 0
    foreach ($account in @($snapshot.eligible)) {
        if ($account.PasswordExpires -eq (-not $Enabled)) { continue }
        try {
            Set-LocalUser -SID ([System.Security.Principal.SecurityIdentifier]$account.Sid) -PasswordNeverExpires:$Enabled -ErrorAction Stop
            $changed++
        }
        catch {
            $failed++
        }
    }

    $status = Get-LocalPasswordExpiryStatus
    if ($status.error) { return $status }
    [pscustomobject]@{
        status       = $status
        changedCount = $changed
        failedCount  = $failed
    }
}
