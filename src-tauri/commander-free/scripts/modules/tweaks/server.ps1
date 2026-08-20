# ============================================================================
# TWEAKS - WINDOWS SERVER MODULE
# Server-SKU logon annoyances, credential hardening, and file-server surface
# ============================================================================
#
# Everything here targets machines running a Windows Server SKU. Three of the
# tweaks (Shutdown Event Tracker, Server Manager at logon, IE Enhanced
# Security) have no client-SKU equivalent at all — they are guarded by
# Assert-IsServerSku so a client machine gets a clear error instead of a
# registry key that Windows will never read.
#
# The rest (CAD, last-user, console lock, WDigest, LSA PPL, legacy NTLM, SMB
# signing, SMBv1, Remote Registry) are valid on client Windows too, but are
# grouped here because they are the settings that actually matter on a domain
# controller / RDS host / file server. There is no SKU-visibility gate in
# ToggleDef yet, so the section is rendered for everyone and the server-only
# entries self-report.

# --- SKU DETECTION ---

function Test-IsServerSku {
    # ProductType 1 = workstation, 2 = domain controller, 3 = member server.
    try {
        $pt = (Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop).ProductType
        return ($pt -eq 2 -or $pt -eq 3)
    }
    catch { return $false }
}

function Assert-IsServerSku {
    if (-not (Test-IsServerSku)) {
        throw "This tweak applies to Windows Server only."
    }
}

# --- PERSISTENT RDP ANIMATIONS ---

$script:RdpAnimationTaskName = 'Keep RDP Animation Effects'
$script:RdpAnimationDirectory = Join-Path $env:ProgramData 'WinCommander'
$script:RdpAnimationScriptPath = Join-Path $script:RdpAnimationDirectory 'Keep-RdpAnimationEffects.ps1'

$script:RdpAnimationTaskScript = @'
$ErrorActionPreference = 'Stop'

function Set-RdpAnimationRegistryValues {
    $values = @(
        @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects', 'VisualFXSetting', 2, 'DWord'),
        @('HKCU:\Control Panel\Desktop\WindowMetrics', 'MinAnimate', '1', 'String'),
        @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'TaskbarAnimations', 1, 'DWord')
    )
    foreach ($value in $values) {
        if (!(Test-Path $value[0])) { New-Item -Path $value[0] -Force | Out-Null }
        Set-ItemProperty -Path $value[0] -Name $value[1] -Value $value[2] -Type $value[3] -Force
    }

    $remoteRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Remote'
    Get-ChildItem -Path $remoteRoot -ErrorAction SilentlyContinue | ForEach-Object {
        Set-ItemProperty -Path $_.PSPath -Name 'TaskbarAnimations' -Value 1 -Type DWord -Force
    }
}

if (-not ('WinCommander.RdpAnimationApi' -as [type])) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;
namespace WinCommander {
    public static class RdpAnimationApi {
        [StructLayout(LayoutKind.Sequential)]
        public struct AnimationInfo { public uint cbSize; public int iMinAnimate; }

        [DllImport("user32.dll", SetLastError = true)]
        public static extern bool SystemParametersInfo(uint action, uint parameter, ref int value, uint flags);

        [DllImport("user32.dll", SetLastError = true)]
        public static extern bool SystemParametersInfo(uint action, uint parameter, ref AnimationInfo value, uint flags);
    }
}
"@
}

Set-RdpAnimationRegistryValues
$animation = New-Object WinCommander.RdpAnimationApi+AnimationInfo
$animation.cbSize = [Runtime.InteropServices.Marshal]::SizeOf($animation)
$animation.iMinAnimate = 1
[WinCommander.RdpAnimationApi]::SystemParametersInfo(0x0049, $animation.cbSize, [ref]$animation, 3) | Out-Null
foreach ($action in @(0x1043, 0x1003, 0x1013, 0x1017, 0x1019)) {
    $enabled = 1
    [WinCommander.RdpAnimationApi]::SystemParametersInfo($action, 0, [ref]$enabled, 3) | Out-Null
}
'@

function Set-RdpAnimationValuesForRegistryRoot {
    param([Parameter(Mandatory = $true)][string]$RegistryRoot)

    $visualEffects = "$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects"
    $windowMetrics = "$RegistryRoot\Control Panel\Desktop\WindowMetrics"
    $explorer = "$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    foreach ($path in @($visualEffects, $windowMetrics, $explorer)) {
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    }
    Set-ItemProperty -Path $visualEffects -Name 'VisualFXSetting' -Value 2 -Type DWord -Force
    Set-ItemProperty -Path $windowMetrics -Name 'MinAnimate' -Value '1' -Type String -Force
    Set-ItemProperty -Path $explorer -Name 'TaskbarAnimations' -Value 1 -Type DWord -Force

    $remoteRoot = "$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\Remote"
    Get-ChildItem -Path $remoteRoot -ErrorAction SilentlyContinue | ForEach-Object {
        Set-ItemProperty -Path $_.PSPath -Name 'TaskbarAnimations' -Value 1 -Type DWord -Force
    }
}

function Reset-RdpAnimationValuesForRegistryRoot {
    param([Parameter(Mandatory = $true)][string]$RegistryRoot)

    $visualEffects = "$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects"
    $windowMetrics = "$RegistryRoot\Control Panel\Desktop\WindowMetrics"
    $explorer = "$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
    if (!(Test-Path $visualEffects)) { New-Item -Path $visualEffects -Force | Out-Null }
    Set-ItemProperty -Path $visualEffects -Name 'VisualFXSetting' -Value 1 -Type DWord -Force
    if (Test-Path $windowMetrics) { Remove-ItemProperty -Path $windowMetrics -Name 'MinAnimate' -Force -ErrorAction SilentlyContinue }
    if (Test-Path $explorer) { Remove-ItemProperty -Path $explorer -Name 'TaskbarAnimations' -Force -ErrorAction SilentlyContinue }

    $remoteRoot = "$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\Remote"
    Get-ChildItem -Path $remoteRoot -ErrorAction SilentlyContinue | ForEach-Object {
        Remove-ItemProperty -Path $_.PSPath -Name 'TaskbarAnimations' -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-ForLoadedRdpUserProfiles {
    param([Parameter(Mandatory = $true)][scriptblock]$Operation)

    Get-ChildItem -Path 'Registry::HKEY_USERS' -ErrorAction Stop |
        Where-Object { $_.PSChildName -match '^S-1-5-21-(?:\d+-){3}\d+$' } |
        ForEach-Object { & $Operation "Registry::HKEY_USERS\$($_.PSChildName)" }
}

function Invoke-ForDefaultRdpUserProfile {
    param([Parameter(Mandatory = $true)][scriptblock]$Operation)

    $hivePath = Join-Path $env:SystemDrive 'Users\Default\NTUSER.DAT'
    if (!(Test-Path -LiteralPath $hivePath)) { throw 'The Windows Default user profile hive was not found.' }
    $mountName = 'WinCommanderRdpAnimationsDefault'
    & reg.exe load "HKU\$mountName" $hivePath | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not load the Windows Default user profile hive.' }
    try {
        & $Operation "Registry::HKEY_USERS\$mountName"
    }
    finally {
        [gc]::Collect()
        [gc]::WaitForPendingFinalizers()
        & reg.exe unload "HKU\$mountName" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not unload the Windows Default user profile hive.' }
    }
}

function Enable-PersistentRdpAnimations {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        $policyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DWM'
        if (!(Test-Path $policyPath)) { New-Item -Path $policyPath -Force | Out-Null }
        Set-ItemProperty -Path $policyPath -Name 'DisallowAnimations' -Value 0 -Type DWord -Force

        Invoke-ForLoadedRdpUserProfiles ${function:Set-RdpAnimationValuesForRegistryRoot}
        Invoke-ForDefaultRdpUserProfile ${function:Set-RdpAnimationValuesForRegistryRoot}

        if (!(Test-Path $script:RdpAnimationDirectory)) {
            New-Item -Path $script:RdpAnimationDirectory -ItemType Directory -Force | Out-Null
        }
        Set-Content -LiteralPath $script:RdpAnimationScriptPath -Value $script:RdpAnimationTaskScript -Encoding UTF8 -Force
        & icacls.exe $script:RdpAnimationScriptPath /inheritance:r /grant:r '*S-1-5-18:(F)' '*S-1-5-32-544:(F)' '*S-1-5-32-545:(RX)' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not secure the RDP animation task helper.' }

        $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$script:RdpAnimationScriptPath`""
        $logonTrigger = New-ScheduledTaskTrigger -AtLogOn
        $logonTrigger.Delay = 'PT2S'
        $remoteTriggerClass = Get-CimClass -Namespace 'Root/Microsoft/Windows/TaskScheduler' -ClassName 'MSFT_TaskSessionStateChangeTrigger'
        $remoteTrigger = New-CimInstance -CimClass $remoteTriggerClass -ClientOnly
        $remoteTrigger.Enabled = $true
        $remoteTrigger.StateChange = 3
        $remoteTrigger.Delay = 'PT2S'
        $principal = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel Limited
        $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
        Register-ScheduledTask -TaskName $script:RdpAnimationTaskName -Action $action -Trigger @($logonTrigger, $remoteTrigger) -Principal $principal -Settings $settings -Force | Out-Null

        & $script:RdpAnimationScriptPath
        @{ status = 'enabled'; scope = 'server'; taskName = $script:RdpAnimationTaskName }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-PersistentRdpAnimations {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        Unregister-ScheduledTask -TaskName $script:RdpAnimationTaskName -Confirm:$false -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $script:RdpAnimationScriptPath -Force -ErrorAction SilentlyContinue
        Remove-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DWM' -Name 'DisallowAnimations' -Force -ErrorAction SilentlyContinue

        Invoke-ForLoadedRdpUserProfiles ${function:Reset-RdpAnimationValuesForRegistryRoot}
        Invoke-ForDefaultRdpUserProfile ${function:Reset-RdpAnimationValuesForRegistryRoot}
        @{ status = 'disabled'; scope = 'windows-managed' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-PersistentRdpAnimationsStatus {
    $task = Get-ScheduledTask -TaskName $script:RdpAnimationTaskName -ErrorAction SilentlyContinue
    $policy = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DWM' -Name 'DisallowAnimations' -ErrorAction SilentlyContinue
    $visualEffects = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects' -Name 'VisualFXSetting' -ErrorAction SilentlyContinue
    $windowMetrics = Get-ItemProperty -Path 'HKCU:\Control Panel\Desktop\WindowMetrics' -Name 'MinAnimate' -ErrorAction SilentlyContinue
    $explorer = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'TaskbarAnimations' -ErrorAction SilentlyContinue
    $hasLogonTrigger = [bool](@($task.Triggers) | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' })
    $hasRemoteConnectTrigger = [bool](@($task.Triggers) | Where-Object {
        $_.CimClass.CimClassName -eq 'MSFT_TaskSessionStateChangeTrigger' -and $_.StateChange -eq 3
    })
    $hasManagedAction = [bool](@($task.Actions) | Where-Object { $_.Arguments -like '*Keep-RdpAnimationEffects.ps1*' })
    $usersCanWriteHelper = $false
    if (Test-Path -LiteralPath $script:RdpAnimationScriptPath) {
        foreach ($rule in (Get-Acl -LiteralPath $script:RdpAnimationScriptPath).Access) {
            try { $ruleSid = $rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value }
            catch { continue }
            if ($ruleSid -eq 'S-1-5-32-545' -and $rule.AccessControlType -eq 'Allow' -and
                ($rule.FileSystemRights -band [System.Security.AccessControl.FileSystemRights]::Write) -ne 0) {
                $usersCanWriteHelper = $true
            }
        }
    }

    $remoteOverridesOk = $true
    Get-ChildItem -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Remote' -ErrorAction SilentlyContinue | ForEach-Object {
        $override = Get-ItemProperty -Path $_.PSPath -Name 'TaskbarAnimations' -ErrorAction SilentlyContinue
        if (-not $override -or $override.TaskbarAnimations -ne 1) { $remoteOverridesOk = $false }
    }

    @{
        persistentRdpAnimations = [bool](
            $task -and $task.State -ne 'Disabled' -and $hasLogonTrigger -and $hasRemoteConnectTrigger -and
            $hasManagedAction -and (Test-Path -LiteralPath $script:RdpAnimationScriptPath) -and -not $usersCanWriteHelper -and
            $policy.DisallowAnimations -eq 0 -and $visualEffects.VisualFXSetting -eq 2 -and
            $windowMetrics.MinAnimate -eq '1' -and $explorer.TaskbarAnimations -eq 1 -and $remoteOverridesOk
        )
    }
}

# --- CTRL+ALT+DEL (SECURE ATTENTION SEQUENCE) ---
#
# DisableCAD=1 drops the Ctrl+Alt+Del requirement at the logon screen. Server
# and domain-joined machines require it by default; it is a genuine (small)
# security control — a credential-harvesting app cannot fake the real SAS —
# so this is flagged reducesSecurity in the registry.

function Disable-CtrlAltDelLogon {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'DisableCAD' -Value 1 -Type DWord -Force
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-CtrlAltDelLogon {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'DisableCAD' -Value 0 -Type DWord -Force
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- LAST SIGNED-IN USER ---

function Enable-HideLastSignedInUser {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'DontDisplayLastUserName' -Value 1 -Type DWord -Force
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-HideLastSignedInUser {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'DontDisplayLastUserName' -Value 0 -Type DWord -Force
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- CONSOLE INACTIVITY LOCK ---
#
# InactivityTimeoutSecs locks the machine at the logon-session level, which is
# what CIS/STIG check — unlike a per-user screensaver it cannot be turned off
# by whoever is signed in. 900s (15 min) matches the CIS Server benchmark.

$script:ServerInactivityTimeoutSecs = 900

function Enable-ConsoleInactivityLock {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'InactivityTimeoutSecs' -Value $script:ServerInactivityTimeoutSecs -Type DWord -Force
        @{ status = 'enabled'; timeoutSeconds = $script:ServerInactivityTimeoutSecs }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-ConsoleInactivityLock {
    Assert-IsAdmin
    try {
        # 0 means "never lock" — the documented off value. Writing 0 rather than
        # deleting keeps the setting explicit for drift detection.
        $path = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'InactivityTimeoutSecs' -Value 0 -Type DWord -Force
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- SHUTDOWN EVENT TRACKER (SERVER-ONLY) ---
#
# The "why are you shutting down this computer?" reason dialog. Server-only —
# the policy key is ignored on client SKUs.

function Disable-ShutdownEventTracker {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        $path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Reliability'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'ShutdownReasonOn' -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name 'ShutdownReasonUI' -Value 0 -Type DWord -Force
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-ShutdownEventTracker {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        # Server's default is "on with no policy present", so restoring means
        # removing the values rather than writing 1.
        $path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Reliability'
        if (Test-Path $path) {
            Remove-ItemSecure -Path $path -Name 'ShutdownReasonOn' -ErrorAction SilentlyContinue
            Remove-ItemSecure -Path $path -Name 'ShutdownReasonUI' -ErrorAction SilentlyContinue
        }
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- SERVER MANAGER AT LOGON (SERVER-ONLY) ---

function Disable-ServerManagerAtLogon {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        $path = 'HKLM:\SOFTWARE\Microsoft\ServerManager'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'DoNotOpenServerManagerAtLogon' -Value 1 -Type DWord -Force
        # The registry value covers interactive logon; the scheduled task is what
        # actually relaunches it on some builds, so disable both.
        Disable-ScheduledTask -TaskPath '\Microsoft\Windows\Server Manager\' -TaskName 'ServerManager' -ErrorAction SilentlyContinue | Out-Null
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-ServerManagerAtLogon {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        $path = 'HKLM:\SOFTWARE\Microsoft\ServerManager'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'DoNotOpenServerManagerAtLogon' -Value 0 -Type DWord -Force
        Enable-ScheduledTask -TaskPath '\Microsoft\Windows\Server Manager\' -TaskName 'ServerManager' -ErrorAction SilentlyContinue | Out-Null
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- IE ENHANCED SECURITY CONFIGURATION (SERVER-ONLY) ---
#
# Two Active Setup components: ...A7 = Administrators, ...A8 = Users.

$script:IeEscAdminKey = 'HKLM:\SOFTWARE\Microsoft\Active Setup\Installed Components\{A509B1A7-37EF-4b3f-8CFC-4F3A74704073}'
$script:IeEscUserKey  = 'HKLM:\SOFTWARE\Microsoft\Active Setup\Installed Components\{A509B1A8-37EF-4b3f-8CFC-4F3A74704073}'

function Disable-IEEnhancedSecurity {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        foreach ($key in @($script:IeEscAdminKey, $script:IeEscUserKey)) {
            if (Test-Path $key) { Set-ItemProperty -Path $key -Name 'IsInstalled' -Value 0 -Type DWord -Force }
        }
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-IEEnhancedSecurity {
    Assert-IsAdmin
    Assert-IsServerSku
    try {
        foreach ($key in @($script:IeEscAdminKey, $script:IeEscUserKey)) {
            if (Test-Path $key) { Set-ItemProperty -Path $key -Name 'IsInstalled' -Value 1 -Type DWord -Force }
        }
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- WDIGEST CLEARTEXT CREDENTIALS ---
#
# UseLogonCredential=1 makes LSASS cache plaintext passwords (what Mimikatz
# reads). Windows defaults to secure when the value is absent, but the value
# is writable by anything with admin, so pinning it to 0 is the hardened state.

function Block-WDigestCredentials {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\SecurityProviders\WDigest'
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name 'UseLogonCredential' -Value 0 -Type DWord -Force
        @{ status = 'blocked'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Allow-WDigestCredentials {
    Assert-IsAdmin
    try {
        # Remove rather than write 1 — absent is Windows' own default and we
        # should not leave a machine in the actively-insecure state.
        $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\SecurityProviders\WDigest'
        if (Test-Path $path) {
            Remove-ItemSecure -Path $path -Name 'UseLogonCredential' -ErrorAction SilentlyContinue
        }
        @{ status = 'allowed'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- LSA PROTECTION (RunAsPPL) ---
#
# Runs LSASS as a protected process so non-PPL code cannot open its memory.
# Takes effect only after reboot. RunAsPPLBoot is the UEFI-variable companion
# on 2022+/11; setting both is what Microsoft documents.

function Enable-LsaProtection {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa'
        Set-ItemProperty -Path $path -Name 'RunAsPPL' -Value 1 -Type DWord -Force
        Set-ItemProperty -Path $path -Name 'RunAsPPLBoot' -Value 1 -Type DWord -Force
        @{ status = 'enabled'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-LsaProtection {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa'
        Set-ItemProperty -Path $path -Name 'RunAsPPL' -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $path -Name 'RunAsPPLBoot' -Value 0 -Type DWord -Force
        @{ status = 'disabled'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- LEGACY LM / NTLMv1 ---
#
# LmCompatibilityLevel=5: send NTLMv2 only, refuse LM and NTLM. Level 5 is the
# CIS/STIG setting. Anything that still speaks NTLMv1 (very old NAS boxes,
# some appliances) will stop authenticating — hence reducesSecurity=false but
# not safeDefault.

function Block-LegacyNtlm {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa'
        Set-ItemProperty -Path $path -Name 'LmCompatibilityLevel' -Value 5 -Type DWord -Force
        @{ status = 'blocked'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Allow-LegacyNtlm {
    Assert-IsAdmin
    try {
        $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa'
        if (Test-Path $path) {
            Remove-ItemSecure -Path $path -Name 'LmCompatibilityLevel' -ErrorAction SilentlyContinue
        }
        @{ status = 'allowed'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- SMB SIGNING ---
#
# Required signing defeats SMB relay. On a file server this is the single
# highest-value SMB setting; the cost is a few percent throughput.

function Enable-SmbSigningRequired {
    Assert-IsAdmin
    try {
        Set-SmbServerConfiguration -RequireSecuritySignature $true -EnableSecuritySignature $true -Force -ErrorAction Stop
        # Client side too, so this box cannot be relayed when it initiates.
        Set-SmbClientConfiguration -RequireSecuritySignature $true -EnableSecuritySignature $true -Force -ErrorAction SilentlyContinue
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Disable-SmbSigningRequired {
    Assert-IsAdmin
    try {
        Set-SmbServerConfiguration -RequireSecuritySignature $false -Force -ErrorAction Stop
        Set-SmbClientConfiguration -RequireSecuritySignature $false -Force -ErrorAction SilentlyContinue
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- SMBv1 ---

function Disable-Smb1Protocol {
    Assert-IsAdmin
    try {
        Set-SmbServerConfiguration -EnableSMB1Protocol $false -Force -ErrorAction SilentlyContinue
        # The optional feature is the part that removes the client driver.
        Disable-WindowsOptionalFeature -Online -FeatureName 'SMB1Protocol' -NoRestart -ErrorAction SilentlyContinue | Out-Null
        @{ status = 'disabled'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-Smb1Protocol {
    Assert-IsAdmin
    try {
        Enable-WindowsOptionalFeature -Online -FeatureName 'SMB1Protocol' -NoRestart -ErrorAction SilentlyContinue | Out-Null
        Set-SmbServerConfiguration -EnableSMB1Protocol $true -Force -ErrorAction SilentlyContinue
        @{ status = 'enabled'; requiresRestart = $true }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- REMOTE REGISTRY ---
#
# Ships Automatic-triggered on Server. Nothing but remote management tools use
# it, and it is a standing remote-read surface.

function Disable-RemoteRegistryService {
    Assert-IsAdmin
    try {
        Stop-Service -Name 'RemoteRegistry' -Force -ErrorAction SilentlyContinue
        Set-Service -Name 'RemoteRegistry' -StartupType Disabled -ErrorAction Stop
        @{ status = 'disabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Enable-RemoteRegistryService {
    Assert-IsAdmin
    try {
        # Automatic (not Manual) is the Server default for this service.
        Set-Service -Name 'RemoteRegistry' -StartupType Automatic -ErrorAction Stop
        Start-Service -Name 'RemoteRegistry' -ErrorAction SilentlyContinue
        @{ status = 'enabled' }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# --- STATUS PROBE ---
#
# Mirrors the boolean shape settings-bridge.ps1 writes to current.tweaks.server.*
# so the probe and the bridge can never disagree about what "on" means.

function Get-ServerTweakStatus {
    $policy = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' -ErrorAction SilentlyContinue
    $reliability = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Reliability' -ErrorAction SilentlyContinue
    $serverManager = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\ServerManager' -Name 'DoNotOpenServerManagerAtLogon' -ErrorAction SilentlyContinue
    $ieEscAdmin = Get-ItemProperty -Path $script:IeEscAdminKey -Name 'IsInstalled' -ErrorAction SilentlyContinue
    $wdigest = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\SecurityProviders\WDigest' -Name 'UseLogonCredential' -ErrorAction SilentlyContinue
    $lsa = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa' -ErrorAction SilentlyContinue
    $remoteReg = Get-Service -Name 'RemoteRegistry' -ErrorAction SilentlyContinue

    $smbSigning = $false
    $smb1Disabled = $false
    try {
        $smb = Get-SmbServerConfiguration -ErrorAction Stop
        $smbSigning = [bool]$smb.RequireSecuritySignature
        $smb1Disabled = (-not $smb.EnableSMB1Protocol)
    }
    catch { }

    @{
        isServerSku             = (Test-IsServerSku)
        # DisableCAD=1 means the Ctrl+Alt+Del requirement is off.
        ctrlAltDelDisabled      = ($policy -and $policy.DisableCAD -eq 1)
        lastSignedInUserHidden  = ($policy -and $policy.DontDisplayLastUserName -eq 1)
        consoleInactivityLock   = ($policy -and $policy.InactivityTimeoutSecs -gt 0)
        shutdownTrackerDisabled = ($reliability -and $reliability.ShutdownReasonOn -eq 0)
        serverManagerAtLogonDisabled = ($serverManager -and $serverManager.DoNotOpenServerManagerAtLogon -eq 1)
        ieEnhancedSecurityDisabled   = ($ieEscAdmin -and $ieEscAdmin.IsInstalled -eq 0)
        wdigestBlocked          = ($wdigest -and $wdigest.UseLogonCredential -eq 0)
        lsaProtectionEnabled    = ($lsa -and $lsa.RunAsPPL -eq 1)
        legacyNtlmBlocked       = ($lsa -and $lsa.LmCompatibilityLevel -ge 5)
        smbSigningRequired      = $smbSigning
        smb1Disabled            = $smb1Disabled
        remoteRegistryDisabled  = ($remoteReg -and $remoteReg.StartType -eq 'Disabled')
    }
}
