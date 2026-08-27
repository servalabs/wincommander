# ============================================================================
# SHARED APP-CAPABILITY ACCESS MODULE  (Fleet Control Plane — P1)
# ============================================================================
#
# Single source of truth for remotely setting a Windows app-capability
# (camera / microphone / location / …) to Allow or Deny. Embedded into the
# Pro sidecar via `wincmd_shared::CAPABILITY_ACCESS_PS_MODULE` and invoked by
# the fleet command executor (handlers::dispatch → "Set-AppCapabilityAccess").
#
# Why a shared module (auto-erase precedent): the Free binary already owns a
# copy of Set-AppCapabilityAccess inside its encrypted telemetry.ps1; lifting a
# self-contained copy here lets Pro run the *same* four-layer enforcement
# without dot-sourcing Free's encrypted modules (which don't exist on disk).
#
# AV-clean: none of the function names or registry strings below appear in
# tools/strings-grep-forbidden.txt, so embedding this verbatim in the Free
# binary (wincmd-shared links into both) keeps `lint:strings-free` green.

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Assert-IsAdmin {
    if (-not (Test-IsAdmin)) {
        throw "Administrator privileges required."
    }
}

function Set-RegistryValueSafe {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][AllowNull()]$Value,
        [ValidateSet("DWord", "String", "QWord", "Binary", "MultiString", "ExpandString")]
        [string]$Type = "DWord"
    )
    if (!(Test-Path $Path)) { New-Item -Path $Path -Force | Out-Null }
    Set-ItemProperty -Path $Path -Name $Name -Value $Value -Type $Type -Force
}

# GUIDs Windows uses to identify camera & microphone at the DeviceAccess layer.
# CapabilityAccessManager reads DeviceAccess\Global\{GUID}\Value BEFORE
# ConsentStore — that's the real master switch the Settings UI flips.
$Script:DeviceAccessGuids = @{
    'webcam'     = '{E5323777-F976-4f5b-9B55-B94699C46E44}'
    'microphone' = '{2EEF81BE-33FA-4800-9670-1CD474972C3F}'
    'location'   = '{BFA794E4-F964-4FDB-90F6-51056BFE4B44}'
}

# AppPrivacy GP values: 0 = user in control, 1 = force allow, 2 = force deny.
$Script:AppPrivacyValueNames = @{
    'webcam'     = 'LetAppsAccessCamera'
    'microphone' = 'LetAppsAccessMicrophone'
    'location'   = 'LetAppsAccessLocation'
}

function Set-AppCapabilityAccess {
    param(
        [Parameter(Mandatory = $true)][string]$Capability,
        [Parameter(Mandatory = $true)][ValidateSet("Allow", "Deny")][string]$Access
    )

    Assert-IsAdmin
    try {
        $touched = 0
        $isDeny = ($Access -eq 'Deny')

        # ── Layer 0: Capability-specific OS policy (HKCU + HKLM) ──
        if ($Capability -eq 'webcam') {
            foreach ($hive in @('HKCU:\SOFTWARE\Policies\Microsoft\Camera', 'HKLM:\SOFTWARE\Policies\Microsoft\Camera')) {
                try {
                    if ($isDeny) {
                        Set-RegistryValueSafe -Path $hive -Name 'AllowCamera' -Value 0 -Type DWord
                    } else {
                        Remove-ItemProperty -Path $hive -Name 'AllowCamera' -ErrorAction SilentlyContinue
                    }
                    $touched++
                } catch {}
            }
        }

        # ── Layer 1: AppPrivacy Group Policy (HKCU + HKLM) ──
        if ($Script:AppPrivacyValueNames.ContainsKey($Capability)) {
            $valueName = $Script:AppPrivacyValueNames[$Capability]
            foreach ($hive in @('HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy', 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy')) {
                try {
                    if ($isDeny) {
                        Set-RegistryValueSafe -Path $hive -Name $valueName -Value 2 -Type DWord
                    } else {
                        Remove-ItemProperty -Path $hive -Name $valueName -ErrorAction SilentlyContinue
                    }
                    $touched++
                } catch {}
            }
        }

        # ── Layer 2: DeviceAccess Global master switch ──
        if ($Script:DeviceAccessGuids.ContainsKey($Capability)) {
            $guid = $Script:DeviceAccessGuids[$Capability]
            $deviceAccessPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DeviceAccess\Global\$guid"
            try {
                Set-RegistryValueSafe -Path $deviceAccessPath -Name "Value" -Value $Access -Type String
                $touched++
            } catch {}
        }

        # ── Layer 3: ConsentStore (the per-app permission tree) ──
        $consentRoots = @(
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore",
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore"
        )
        foreach ($root in $consentRoots) {
            $capPath = Join-Path $root $Capability
            try {
                Set-RegistryValueSafe -Path $capPath -Name "Value" -Value $Access -Type String
                $touched++
            } catch {}
            if (Test-Path -LiteralPath $capPath) {
                Get-ChildItem -LiteralPath $capPath -ErrorAction SilentlyContinue | ForEach-Object {
                    try {
                        Set-RegistryValueSafe -Path $_.PSPath -Name "Value" -Value $Access -Type String
                        $touched++
                    } catch {
                        # Per-package subkeys owned by TrustedInstaller can reject
                        # elevated-but-not-SYSTEM writes; skip silently.
                    }
                }
            }
        }

        # ── Policy refresh — force Settings to re-read immediately ──
        try {
            if (-not ([System.Management.Automation.PSTypeName]'WC_PolicyRefresh').Type) {
                Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public class WC_PolicyRefresh {
    [DllImport("userenv.dll", SetLastError=true)]
    public static extern bool RefreshPolicyEx(bool bMachine, uint dwOptions);
}
'@ -ErrorAction SilentlyContinue
            }
            [WC_PolicyRefresh]::RefreshPolicyEx($true, 1) | Out-Null
        } catch {}

        @{ status = "updated"; capability = $Capability; value = $Access; entriesTouched = $touched }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Get-AppCapabilityAccessStatus {
    param(
        [Parameter(Mandatory = $true)][string]$Capability
    )

    try {
        # Return the effective Windows decision. Set-AppCapabilityAccess writes
        # more than the current user's ConsentStore, so reading only that one
        # value can wrongly claim a camera is allowed/blocked after policy or
        # Settings changes.
        $denied = $false
        foreach ($root in @(
            "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore",
            "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore"
        )) {
            $value = (Get-ItemProperty -Path (Join-Path $root $Capability) -Name "Value" -ErrorAction SilentlyContinue).Value
            if ($value -eq "Deny") { $denied = $true }
        }
        if ($Capability -eq 'webcam') {
            $denied = $denied -or ((Get-ItemProperty -Path 'HKCU:\SOFTWARE\Policies\Microsoft\Camera' -Name 'AllowCamera' -ErrorAction SilentlyContinue).AllowCamera -eq 0) -or ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Camera' -Name 'AllowCamera' -ErrorAction SilentlyContinue).AllowCamera -eq 0) -or ((Get-ItemProperty -Path 'HKCU:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy' -Name 'LetAppsAccessCamera' -ErrorAction SilentlyContinue).LetAppsAccessCamera -eq 2) -or ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy' -Name 'LetAppsAccessCamera' -ErrorAction SilentlyContinue).LetAppsAccessCamera -eq 2) -or ((Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\DeviceAccess\Global\{E5323777-F976-4f5b-9B55-B94699C46E44}' -Name 'Value' -ErrorAction SilentlyContinue).Value -eq 'Deny')
        }
        $value = if ($denied) { "Deny" } else { "Allow" }
        @{
            capability = $Capability
            value      = $value
            disabled   = $denied
        }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message; capability = $Capability; value = "Allow"; disabled = $false }
    }
}
