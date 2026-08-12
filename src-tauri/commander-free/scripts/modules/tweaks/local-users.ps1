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
