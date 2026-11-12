// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned device identity for activation. No desktop-provided hash is trusted.
use sha2::{Digest, Sha256};
use std::{os::windows::process::CommandExt, path::PathBuf, process::{Command, Stdio}, time::{Duration, Instant}};
use windows_sys::Win32::{Storage::FileSystem::GetVolumeInformationW,
    System::{SystemInformation::GetSystemDirectoryW, Threading::CREATE_NO_WINDOW}};

const INVALID: &str = "licence device identity is unavailable";
const OUTPUT_LIMIT: usize = 4096;

pub(crate) struct MachineIdentity {
    pub device_hash: String,
    pub portable: bool,
}

fn identity_from_parts(portable: bool, volume: Option<&str>, motherboard: &str, disk: &str)
    -> Result<MachineIdentity, &'static str> {
    let motherboard = motherboard.trim();
    let disk = disk.trim();
    let mut hash = Sha256::new();
    hash.update(b"wincommander-license-v1");
    if portable {
        let serial = volume.filter(|s| s.len() == 8 && s.bytes().all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c)))
            .ok_or(INVALID)?;
        hash.update(b"|WTG|");
        hash.update(serial.as_bytes());
    } else {
        if (motherboard.is_empty() && disk.is_empty()) || motherboard.len() > 512 || disk.len() > 512 {
            return Err(INVALID);
        }
        hash.update(b"|MB|"); hash.update(motherboard.as_bytes());
        hash.update(b"|DS|"); hash.update(disk.as_bytes());
    }
    let digest = hash.finalize();
    let device_hash = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(MachineIdentity { device_hash, portable })
}

fn system_directory() -> Result<PathBuf, &'static str> {
    let mut buffer = vec![0u16; 32768];
    // The buffer remains valid and writable for the entire Win32 call.
    let count = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if count == 0 || count >= buffer.len() { return Err(INVALID); }
    let value = String::from_utf16(&buffer[..count]).map_err(|_| INVALID)?;
    let path = PathBuf::from(value);
    if !path.is_absolute() { return Err(INVALID); }
    Ok(path)
}

fn probe(command: &str) -> Result<String, &'static str> {
    let powershell = system_directory()?.join(r"WindowsPowerShell\v1.0\powershell.exe");
    let mut child = Command::new(powershell)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", command])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW).spawn().map_err(|_| INVALID)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err(INVALID),
            Err(_) => { let _ = child.kill(); let _ = child.wait(); return Err(INVALID); }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill(); let _ = child.wait(); return Err(INVALID);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    let output = child.wait_with_output().map_err(|_| INVALID)?.stdout;
    if output.len() > OUTPUT_LIMIT { return Err(INVALID); }
    Ok(String::from_utf8(output).map_err(|_| INVALID)?.trim().to_owned())
}

fn volume_serial() -> Result<String, &'static str> {
    let system = system_directory()?;
    let mut root = system.ancestors().last().ok_or(INVALID)?.as_os_str().to_string_lossy().encode_utf16().collect::<Vec<_>>();
    root.push(0);
    let mut serial = 0u32;
    // Root is nul-terminated; optional output pointers are unused.
    let ok = unsafe { GetVolumeInformationW(root.as_ptr(), std::ptr::null_mut(), 0, &mut serial,
        std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) };
    if ok == 0 { Err(INVALID) } else { Ok(format!("{serial:08X}")) }
}

/// Run in a blocking worker, never on an async runtime thread.
pub(crate) fn read_machine_identity() -> Result<MachineIdentity, &'static str> {
    let portable = match probe("$ErrorActionPreference='Stop'; try { $k=Get-Item -LiteralPath 'HKLM:\\SYSTEM\\CurrentControlSet\\Control'; $k.GetValue('PortableOperatingSystem',$null) -eq 1 } catch { exit 1 }")?.as_str() {
        "True" => true, "False" => false, _ => return Err(INVALID),
    };
    if portable { return identity_from_parts(true, Some(&volume_serial()?), "", ""); }
    let board = probe("(Get-CimInstance -ClassName Win32_ComputerSystemProduct).UUID").unwrap_or_default();
    let disk = probe("(Get-CimInstance -ClassName Win32_DiskDrive | Where-Object { $_.Index -eq 0 }).SerialNumber").unwrap_or_default();
    identity_from_parts(false, None, &board, &disk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_identity_matches_the_existing_hardware_format() {
        let actual = identity_from_parts(false, None, " board ", " disk ").unwrap();
        let expected = Sha256::digest(b"wincommander-license-v1|MB|board|DS|disk");
        assert_eq!(actual.device_hash, expected.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
        assert!(!actual.portable);
        assert!(identity_from_parts(false, None, " ", "").is_err());
        let portable = identity_from_parts(true, Some("01ABCDEF"), "", "").unwrap();
        let expected = Sha256::digest(b"wincommander-license-v1|WTG|01ABCDEF");
        assert_eq!(portable.device_hash, expected.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
        assert!(portable.portable);
        assert!(identity_from_parts(true, Some("01abcdef"), "", "").is_err());
        assert!(identity_from_parts(true, None, "", "").is_err());
    }

    #[test]
    #[ignore = "explicit native probe: depends on the current machine's WMI and registry"]
    fn native_machine_probe_returns_a_stable_identity() {
        let first = read_machine_identity().unwrap();
        let second = read_machine_identity().unwrap();
        assert_eq!(first.device_hash, second.device_hash);
        assert_eq!(first.portable, second.portable);
        assert_eq!(first.device_hash.len(), 64);
    }
}
