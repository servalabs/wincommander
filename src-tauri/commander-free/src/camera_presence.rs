// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only hardware presence for the dashboard. This never opens a camera.

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CameraPresence {
    Present,
    Absent,
    Unknown,
}

#[cfg(windows)]
const ENUMERATE: &str = r#"
$ErrorActionPreference = 'Stop'
try { $devices = @(Get-PnpDevice -ErrorAction Stop); $pnpOk = $devices.Count -gt 0 }
catch { $devices = @(); $pnpOk = $false }
$pnpCameras = @($devices | Where-Object {
    $_.Present -eq $true -and ($_.Class -eq 'Camera' -or
      ($_.Class -in @('Image','Media','SoftwareDevice') -and
        ($_.FriendlyName -match '(?i)camera|webcam' -or $_.CompatibleID -match '(?i)USB\\Class_0E|VideoCapture')))
})
if ($pnpCameras.Count -gt 0 -and -not $wcRemote) { 'pnp_present'; exit 0 }
$videoCount = $null
$cameraAccess = 'Unknown'
try {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime -ErrorAction Stop
    $null = [Windows.Devices.Enumeration.DeviceInformation,Windows.Devices.Enumeration,ContentType=WindowsRuntime]
    $type = [Windows.Devices.Enumeration.DeviceInformationCollection,Windows.Devices.Enumeration,ContentType=WindowsRuntime]
    $method = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
    } | Select-Object -First 1
    $operation = [Windows.Devices.Enumeration.DeviceInformation]::FindAllAsync([Windows.Devices.Enumeration.DeviceClass]::VideoCapture)
    $task = $method.MakeGenericMethod($type).Invoke($null, @($operation))
    if (-not $task.Wait(4000)) { throw 'Video source enumeration timed out' }
    $videoCount = @($task.Result).Count
    $null = [Windows.Devices.Enumeration.DeviceAccessInformation,Windows.Devices.Enumeration,ContentType=WindowsRuntime]
    $cameraAccess = [string]([Windows.Devices.Enumeration.DeviceAccessInformation]::CreateFromDeviceClass([Windows.Devices.Enumeration.DeviceClass]::VideoCapture).CurrentStatus)
} catch { }
"#;

#[cfg(windows)]
const CLASSIFY: &str = r#"
$cameras = @($devices | Where-Object {
    $_.Class -eq 'Camera' -or
    ($_.Class -in @('Image','Media','SoftwareDevice') -and
      ($_.FriendlyName -match '(?i)camera|webcam' -or $_.CompatibleID -match '(?i)USB\\Class_0E|VideoCapture'))
})
if ($videoCount -gt 0) { 'present' }
elseif (@($cameras | Where-Object { $_.Present -eq $true }).Count -gt 0) { 'pnp_present' }
elseif (-not $pnpOk -or $null -eq $videoCount -or $cameraAccess -ne 'Allowed' -or $cameras.Count -gt 0) { 'unknown' }
elseif (@($devices | Where-Object { $_.Class -eq 'Image' }).Count -gt 0) { 'unknown' }
elseif (@($devices | Where-Object { $_.Present -eq $true -and $_.Class -in @('Media','SoftwareDevice') }).Count -gt 0) { 'unknown' }
else { 'absent' }
"#;

fn interpret_probe(output: &str, remote_session: bool) -> CameraPresence {
    match output.trim() {
        "present" => CameraPresence::Present,
        "pnp_present" if !remote_session => CameraPresence::Present,
        // Plug and Play describes the host. A redirected remote-session
        // camera need not be represented by a host camera device.
        "absent" if !remote_session => CameraPresence::Absent,
        _ => CameraPresence::Unknown,
    }
}

#[tauri::command]
pub(crate) async fn dashboard_camera_presence() -> CameraPresence {
    #[cfg(windows)]
    {
        use std::time::Duration;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};

        static PROBE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        let Ok(_guard) = PROBE.get_or_init(|| tokio::sync::Mutex::new(())).try_lock() else {
            return CameraPresence::Unknown;
        };

        // Enumerate PnP metadata only. Disabled cameras still count as present;
        // camera permissions and an in-use camera are separate questions.
        // Image-class devices without a recognizable camera name are
        // ambiguous (for example scanners), so they do not prove absence.
        let executable = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .map(|root| root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
        let Some(executable) = executable else { return CameraPresence::Unknown };
        let mut command = tokio::process::Command::new(executable);
        let remote_session = unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 };
        let script = format!("$wcRemote = {};\n{ENUMERATE}\n{CLASSIFY}", if remote_session { "$true" } else { "$false" });
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        command.creation_flags(0x08000000).kill_on_drop(true);
        let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(15), command.output()).await else {
            return CameraPresence::Unknown;
        };
        if !output.status.success() {
            return CameraPresence::Unknown;
        }
        interpret_probe(&String::from_utf8_lossy(&output.stdout), remote_session)
    }
    #[cfg(not(windows))]
    {
        CameraPresence::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_successful_local_empty_inventory_means_absent() {
        assert_eq!(interpret_probe("present\r\n", false), CameraPresence::Present);
        assert_eq!(interpret_probe("present", true), CameraPresence::Present);
        assert_eq!(interpret_probe("pnp_present", false), CameraPresence::Present);
        assert_eq!(interpret_probe("pnp_present", true), CameraPresence::Unknown);
        assert_eq!(interpret_probe("absent", false), CameraPresence::Absent);
        assert_eq!(interpret_probe("absent", true), CameraPresence::Unknown);
        for output in ["", "unknown", "error", "present\nabsent"] {
            assert_eq!(interpret_probe(output, false), CameraPresence::Unknown);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_inventory_distinguishes_cameras_from_absent_and_unverified_devices() {
        use std::os::windows::process::CommandExt;
        let powershell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let samples = [
            ("[pscustomobject]@{Class='Camera'; FriendlyName='Integrated Camera'; Present=$true; Status='OK'}", "$true", "0", "Allowed", "pnp_present"),
            ("[pscustomobject]@{Class='Image'; FriendlyName='localized'; CompatibleID='USB\\Class_0E'; Present=$true}", "$true", "0", "Allowed", "pnp_present"),
            ("[pscustomobject]@{Class='SoftwareDevice'; FriendlyName='localized'; Present=$true}", "$true", "1", "Allowed", "present"),
            ("[pscustomobject]@{Class='Camera'; FriendlyName='Camera'; Present=$true; Status='Error'}", "$true", "0", "DeniedByUser", "pnp_present"),
            ("[pscustomobject]@{Class='Camera'; FriendlyName='Camera'; Present=$false; Status='Unknown'}", "$true", "0", "Allowed", "unknown"),
            ("[pscustomobject]@{Class='Image'; FriendlyName='Scanner'; Present=$true; Status='OK'}", "$true", "0", "Allowed", "unknown"),
            ("[pscustomobject]@{Class='Media'; FriendlyName='localized'; Present=$true}", "$true", "0", "Allowed", "unknown"),
            ("[pscustomobject]@{Class='DiskDrive'; FriendlyName='Disk'; Present=$true; Status='OK'}", "$true", "0", "Allowed", "absent"),
            ("[pscustomobject]@{Class='DiskDrive'; FriendlyName='Disk'; Present=$true}", "$true", "0", "DeniedByUser", "unknown"),
            ("", "$false", "$null", "Unknown", "unknown"),
        ];
        for (devices, pnp_ok, video_count, access, expected) in samples {
            let script = format!("$devices=@({devices}); $pnpOk={pnp_ok}; $videoCount={video_count}; $cameraAccess='{access}'\n{CLASSIFY}");
            let output = std::process::Command::new(&powershell)
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .creation_flags(0x08000000)
                .output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
        }
    }
}
