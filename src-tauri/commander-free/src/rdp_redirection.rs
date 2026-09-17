use serde::Serialize;
use std::process::Command;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpRedirectionStatus {
    pub is_windows_server: bool,
    pub product_name: String,
    pub installation_type: String,
    pub is_admin: bool,
    pub smart_cards: bool,
    pub drives: bool,
    pub clipboard: bool,
    pub printers: bool,
    pub audio_playback: bool,
    pub microphone: bool,
    pub pnp_devices: bool,
    pub camera: bool,
    pub webauthn: bool,
    pub generic_usb_disabled: bool,
    pub qwave_installed: bool,
    pub media_foundation_installed: bool,
}

fn powershell(script: &str) -> Result<String, String> {
    #[cfg(not(windows))]
    {
        let _ = script;
        return Err("RDP resource redirection is only available on Windows".into());
    }

    #[cfg(windows)]
    {
        let output = Command::new("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .output()
            .map_err(|e| format!("failed to start PowerShell: {e}"))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if err.is_empty() {
                format!("PowerShell exited with {}", output.status)
            } else {
                err
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

fn server_guard_script() -> &'static str {
    r#"
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
if (-not ($cv.ProductName -like 'Windows Server*' -or $cv.InstallationType -like 'Server*')) {
  throw 'This control is only available on Windows Server.'
}
"#
}

#[tauri::command]
pub fn get_rdp_redirection_status() -> Result<RdpRedirectionStatus, String> {
    let script = r#"
$ErrorActionPreference='Stop'
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$pol = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
$ws  = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp'
$usb = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services\Client'
function D([string]$p,[string]$n,[int]$fallback) {
  try { return [int](Get-ItemPropertyValue -Path $p -Name $n -ErrorAction Stop) } catch { return $fallback }
}
$isServer = ($cv.ProductName -like 'Windows Server*' -or $cv.InstallationType -like 'Server*')
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$qwave = $false; $mf = $false
try { $qwave = (Get-WindowsFeature -Name qWave -ErrorAction Stop).InstallState -eq 'Installed' } catch {}
try { $mf = (Get-WindowsFeature -Name Server-Media-Foundation -ErrorAction Stop).InstallState -eq 'Installed' } catch {}
[pscustomobject]@{
 isWindowsServer=$isServer
 productName=[string]$cv.ProductName
 installationType=[string]$cv.InstallationType
 isAdmin=$isAdmin
 smartCards=((D $pol 'fEnableSmartCard' 1) -eq 1)
 drives=((D $pol 'fDisableCdm' (D $ws 'fDisableCdm' 0)) -eq 0)
 clipboard=((D $pol 'fDisableClip' (D $ws 'fDisableClip' 0)) -eq 0)
 printers=((D $pol 'fDisableCpm' (D $ws 'fDisableCpm' 0)) -eq 0)
 audioPlayback=((D $pol 'fDisableCam' (D $ws 'fDisableCam' 0)) -eq 0)
 microphone=((D $pol 'fDisableAudioCapture' (D $ws 'fDisableAudioCapture' 0)) -eq 0)
 pnpDevices=((D $pol 'fDisablePNPRedir' 0) -eq 0)
 camera=((D $pol 'fDisableCameraRedir' 0) -eq 0)
 webauthn=((D $pol 'fDisableWebAuthn' 0) -eq 0)
 genericUsbDisabled=((D $usb 'fUsbRedirectionEnableMode' 0) -eq 0)
 qwaveInstalled=$qwave
 mediaFoundationInstalled=$mf
} | ConvertTo-Json -Compress
"#;
    let raw = powershell(script)?;
    serde_json::from_str(&raw).map_err(|e| format!("invalid Windows Server redirection status: {e}"))
}

#[tauri::command]
pub fn set_rdp_redirection_capability(capability: String, enabled: bool) -> Result<(), String> {
    let (name, value, listener_name): (&str, u32, Option<&str>) = match capability.as_str() {
        "smart_cards" => ("fEnableSmartCard", if enabled { 1 } else { 0 }, None),
        "drives" => ("fDisableCdm", if enabled { 0 } else { 1 }, Some("fDisableCdm")),
        "clipboard" => ("fDisableClip", if enabled { 0 } else { 1 }, Some("fDisableClip")),
        "printers" => ("fDisableCpm", if enabled { 0 } else { 1 }, Some("fDisableCpm")),
        "audio_playback" => ("fDisableCam", if enabled { 0 } else { 1 }, Some("fDisableCam")),
        "microphone" => ("fDisableAudioCapture", if enabled { 0 } else { 1 }, Some("fDisableAudioCapture")),
        "pnp_devices" => ("fDisablePNPRedir", if enabled { 0 } else { 1 }, None),
        "camera" => ("fDisableCameraRedir", if enabled { 0 } else { 1 }, None),
        "webauthn" => ("fDisableWebAuthn", if enabled { 0 } else { 1 }, None),
        _ => return Err("unknown RDP redirection capability".into()),
    };
    let listener = listener_name
        .map(|n| format!("Set-ItemProperty -Path $ws -Name '{n}' -Type DWord -Value {value}"))
        .unwrap_or_default();
    let script = format!(
        r#"
$ErrorActionPreference='Stop'
{}
$pol = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
$ws  = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp'
New-Item -Path $pol -Force | Out-Null
Set-ItemProperty -Path $pol -Name '{}' -Type DWord -Value {}
{}
gpupdate /target:computer /force | Out-Null
"#,
        server_guard_script(), name, value, listener
    );
    powershell(&script).map(|_| ())
}

#[tauri::command]
pub fn apply_recommended_rdp_redirection() -> Result<(), String> {
    let script = format!(
        r#"
$ErrorActionPreference='Stop'
{}
$pol = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
$ws  = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp'
$usb = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services\Client'
New-Item -Path $pol -Force | Out-Null
$values = @{{
 fEnableSmartCard=1; fDisableCdm=0; fDisableClip=0; fDisableCpm=0;
 fDisableCam=0; fDisableAudioCapture=0; fDisablePNPRedir=0;
 fDisableCameraRedir=0; fDisableWebAuthn=0
}}
foreach($kv in $values.GetEnumerator()) {{ Set-ItemProperty -Path $pol -Name $kv.Key -Type DWord -Value $kv.Value }}
foreach($n in @('fDisableCam','fDisableCdm','fDisableClip','fDisableCpm','fDisableAudioCapture')) {{ Set-ItemProperty -Path $ws -Name $n -Type DWord -Value 0 }}
Set-ItemProperty -Path $ws -Name fAutoClientDrives -Type DWord -Value 1
if (Test-Path $usb) {{ Remove-ItemProperty -Path $usb -Name fUsbRedirectionEnableMode -ErrorAction SilentlyContinue }}
try {{ if ((Get-WindowsFeature qWave).InstallState -ne 'Installed') {{ Install-WindowsFeature qWave | Out-Null }} }} catch {{}}
try {{ if ((Get-WindowsFeature Server-Media-Foundation).InstallState -ne 'Installed') {{ Install-WindowsFeature Server-Media-Foundation | Out-Null }} }} catch {{}}
gpupdate /target:computer /force | Out-Null
"#,
        server_guard_script()
    );
    powershell(&script).map(|_| ())
}

#[tauri::command]
pub fn get_rdp_client_profile() -> String {
    [
        "redirectsmartcards:i:1",
        "redirectclipboard:i:1",
        "redirectprinters:i:1",
        "drivestoredirect:s:*",
        "audiomode:i:0",
        "audiocapturemode:i:1",
        "camerastoredirect:s:*",
        "redirectwebauthn:i:1",
    ]
    .join("\r\n")
}
