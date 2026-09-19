use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
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
        Err("RDP resource redirection is only available on Windows".into())
    }

    #[cfg(windows)]
    {
        let output = Command::new(crate::service_repair_paths::powershell_executable()?)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .creation_flags(0x08000000)
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
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Administrator rights are required to change machine-wide RDP redirection policy.' }
"#
}

pub fn get_status() -> Result<RdpRedirectionStatus, String> {
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
if ($isServer) {
  try { $qwave = (Get-WindowsFeature -Name qWave -ErrorAction Stop).InstallState -eq 'Installed' } catch {}
  try { $mf = (Get-WindowsFeature -Name Server-Media-Foundation -ErrorAction Stop).InstallState -eq 'Installed' } catch {}
}
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
    serde_json::from_str(&raw)
        .map_err(|e| format!("invalid Windows Server redirection status: {e}"))
}

fn set_capability(capability: &str, enabled: bool) -> Result<(), String> {
    let (name, value, listener_name): (&str, u32, Option<&str>) = match capability {
        "smart_cards" => ("fEnableSmartCard", if enabled { 1 } else { 0 }, None),
        "drives" => (
            "fDisableCdm",
            if enabled { 0 } else { 1 },
            Some("fDisableCdm"),
        ),
        "clipboard" => (
            "fDisableClip",
            if enabled { 0 } else { 1 },
            Some("fDisableClip"),
        ),
        "printers" => (
            "fDisableCpm",
            if enabled { 0 } else { 1 },
            Some("fDisableCpm"),
        ),
        "audio_playback" => (
            "fDisableCam",
            if enabled { 0 } else { 1 },
            Some("fDisableCam"),
        ),
        "microphone" => (
            "fDisableAudioCapture",
            if enabled { 0 } else { 1 },
            Some("fDisableAudioCapture"),
        ),
        "pnp_devices" => ("fDisablePNPRedir", if enabled { 0 } else { 1 }, None),
        "camera" => ("fDisableCameraRedir", if enabled { 0 } else { 1 }, None),
        "webauthn" => ("fDisableWebAuthn", if enabled { 0 } else { 1 }, None),
        _ => return Err("unknown RDP redirection capability".into()),
    };
    let listener = listener_name
        .map(|n| format!("Set-ItemProperty -Path $ws -Name '{n}' -Value {value}"))
        .unwrap_or_default();
    let script = format!(
        r#"
$ErrorActionPreference='Stop'
{}
$pol = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services'
$ws  = 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp'
New-Item -Path $pol -Force | Out-Null
New-ItemProperty -Path $pol -Name '{}' -PropertyType DWord -Value {} -Force | Out-Null
{}
gpupdate /target:computer /force | Out-Null
"#,
        server_guard_script(),
        name,
        value,
        listener
    );
    powershell(&script).map(|_| ())
}

fn apply_recommended() -> Result<(), String> {
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
foreach($kv in $values.GetEnumerator()) {{ New-ItemProperty -Path $pol -Name $kv.Key -PropertyType DWord -Value $kv.Value -Force | Out-Null }}
foreach($n in @('fDisableCam','fDisableCdm','fDisableClip','fDisableCpm','fDisableAudioCapture')) {{ Set-ItemProperty -Path $ws -Name $n -Value 0 }}
Set-ItemProperty -Path $ws -Name fAutoClientDrives -Value 1
if (Test-Path $usb) {{ Remove-ItemProperty -Path $usb -Name fUsbRedirectionEnableMode -ErrorAction SilentlyContinue }}
try {{ if ((Get-WindowsFeature qWave).InstallState -ne 'Installed') {{ Install-WindowsFeature qWave | Out-Null }} }} catch {{}}
try {{ if ((Get-WindowsFeature Server-Media-Foundation).InstallState -ne 'Installed') {{ Install-WindowsFeature Server-Media-Foundation | Out-Null }} }} catch {{}}
gpupdate /target:computer /force | Out-Null
"#,
        server_guard_script()
    );
    powershell(&script).map(|_| ())
}

fn client_profile() -> &'static str {
    "redirectsmartcards:i:1\r\nredirectclipboard:i:1\r\nredirectprinters:i:1\r\ndrivestoredirect:s:*\r\naudiomode:i:0\r\naudiocapturemode:i:1\r\ncamerastoredirect:s:*\r\nredirectwebauthn:i:1"
}

/// Extension carried through the already-registered privileged
/// `apply_machine_setting` Tauri command. Keeping this behind the existing
/// machine-setting seam avoids introducing a second generic privileged IPC
/// surface. Every mutation independently re-checks Windows Server + admin.
pub fn handle(value: &Value) -> Result<Value, String> {
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "RDP redirection action is required".to_string())?;

    match action {
        "status" => serde_json::to_value(get_status()?).map_err(|e| e.to_string()),
        "set_capability" => {
            let capability = value
                .get("capability")
                .and_then(Value::as_str)
                .ok_or_else(|| "RDP redirection capability is required".to_string())?;
            let enabled = value
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| "RDP redirection enabled flag is required".to_string())?;
            set_capability(capability, enabled)?;
            serde_json::to_value(get_status()?).map_err(|e| e.to_string())
        }
        "apply_recommended" => {
            apply_recommended()?;
            serde_json::to_value(get_status()?).map_err(|e| e.to_string())
        }
        "client_profile" => Ok(json!({ "profile": client_profile() })),
        _ => Err("unknown RDP redirection action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{client_profile, RdpRedirectionStatus};

    #[test]
    fn client_profile_uses_native_channels_only() {
        let p = client_profile();
        assert!(p.contains("redirectsmartcards:i:1"));
        assert!(p.contains("drivestoredirect:s:*"));
        assert!(p.contains("camerastoredirect:s:*"));
        assert!(p.contains("redirectwebauthn:i:1"));
        assert!(!p.to_ascii_lowercase().contains("usbdevicestoredirect"));
    }

    #[test]
    fn status_deserializes_the_powershell_payload() {
        let status: RdpRedirectionStatus = serde_json::from_str(
            r#"{
            "isWindowsServer": true,
            "productName": "Windows Server 2025",
            "installationType": "Server",
            "isAdmin": true,
            "smartCards": true,
            "drives": true,
            "clipboard": true,
            "printers": true,
            "audioPlayback": true,
            "microphone": true,
            "pnpDevices": true,
            "camera": true,
            "webauthn": true,
            "genericUsbDisabled": false,
            "qwaveInstalled": true,
            "mediaFoundationInstalled": true
        }"#,
        )
        .expect("PowerShell status payload should deserialize");

        assert!(status.is_windows_server);
        assert_eq!(status.product_name, "Windows Server 2025");
        assert!(!status.generic_usb_disabled);
    }
}
