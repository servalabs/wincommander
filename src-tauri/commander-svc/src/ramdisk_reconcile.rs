// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-time repair for duplicate ImDisk TEMP R: units left by older clients.

use anyhow::{bail, Context, Result};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::process::Command;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_FILE_NOT_FOUND, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, DefineDosDeviceW, FlushFileBuffers, GetVolumeInformationW, QueryDosDeviceW,
    DDD_EXACT_MATCH_ON_REMOVE, DDD_RAW_TARGET_PATH, DDD_REMOVE_DEFINITION, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, FSCTL_UNLOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::Win32::System::IO::DeviceIoControl;

const SIZE_BYTES: u64 = 768 * 1024 * 1024;
const LOCK_NAME: &str = "Global\\WinCommander-RamDisk-R";
const LOCK_SDDL: &str = "D:(A;;0x001F0001;;;SY)(A;;0x001F0001;;;BA)(A;;0x00100001;;;AU)";

#[derive(Debug)]
struct Unit {
    number: u32,
    letter: Option<char>,
    size_bytes: Option<u64>,
    is_vm: bool,
    has_no_image: bool,
}

impl Unit {
    fn is_temp_r_candidate(&self) -> bool {
        self.letter == Some('R')
            && self.size_bytes == Some(SIZE_BYTES)
            && self.is_vm
            && self.has_no_image
    }
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn parse_unit(number: u32, text: &str) -> Unit {
    let mut unit = Unit {
        number,
        letter: None,
        size_bytes: None,
        is_vm: false,
        has_no_image: false,
    };
    for line in text.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("Drive letter: ") {
            unit.letter = value.chars().next().map(|c| c.to_ascii_uppercase());
        } else if let Some(value) = line.strip_prefix("Mount point: ") {
            unit.letter = value.chars().next().map(|c| c.to_ascii_uppercase());
        } else if line == "No image file." {
            unit.has_no_image = true;
        } else if let Some(value) = line.strip_prefix("Size: ") {
            unit.size_bytes = value.split_whitespace().next().and_then(|n| n.parse().ok());
            unit.is_vm = value.to_ascii_lowercase().contains("virtual memory");
        }
    }
    unit
}

fn imdisk_exe() -> Option<PathBuf> {
    [
        "SystemRoot",
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ]
    .into_iter()
    .filter_map(|key| std::env::var_os(key).map(|root| (key, root)))
    .map(|(key, root)| {
        let mut path = PathBuf::from(root);
        if key == "SystemRoot" {
            path.push("System32");
        } else {
            path.push("ImDisk");
        }
        path.push("imdisk.exe");
        path
    })
    .find(|path| path.is_file())
}

fn run(exe: &PathBuf, args: &[&str]) -> Result<std::process::Output> {
    Command::new(exe)
        .args(args)
        .output()
        .with_context(|| format!("could not run ImDisk {}", args.join(" ")))
}

fn output_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn mount_owner() -> Result<Option<u32>> {
    let mut target = [0u16; 1024];
    // QueryDosDevice returns a multi-string; the first path is the active R: target.
    let length =
        unsafe { QueryDosDeviceW(wide(OsStr::new("R:")).as_ptr(), target.as_mut_ptr(), 1024) };
    if length == 0 {
        if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        bail!("could not resolve R: mount");
    }
    let first = target
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(length as usize);
    let path = OsString::from_wide(&target[..first])
        .to_string_lossy()
        .into_owned();
    if let Some(number) = path.strip_prefix("\\Device\\ImDisk") {
        return Ok(Some(number.parse().context("invalid R: ImDisk unit")?));
    }
    bail!("R: belongs to a different device");
}

fn mounted_label() -> Result<String> {
    let mut label = [0u16; 64];
    let ok = unsafe {
        GetVolumeInformationW(
            wide(OsStr::new("R:\\")).as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        bail!("could not read R: label");
    }
    let end = label.iter().position(|c| *c == 0).unwrap_or(label.len());
    Ok(OsString::from_wide(&label[..end])
        .to_string_lossy()
        .into_owned())
}

fn mount_existing_unit(number: u32) -> Result<bool> {
    if mount_owner()?.is_some() {
        bail!("R: became occupied before recovery");
    }
    let mount_name = wide(OsStr::new("Global\\R:"));
    let target = wide(OsStr::new(&format!(r"\Device\ImDisk{number}")));
    if unsafe { DefineDosDeviceW(DDD_RAW_TARGET_PATH, mount_name.as_ptr(), target.as_ptr()) } == 0 {
        bail!("could not restore R: for ImDisk unit {number}");
    }
    let matches = mount_owner().ok() == Some(Some(number))
        && mounted_label().is_ok_and(|label| label.eq_ignore_ascii_case("TEMP"));
    if !matches {
        // Remove only the mapping we just added, leaving other mappings alone.
        let removed = unsafe {
            DefineDosDeviceW(
                DDD_RAW_TARGET_PATH | DDD_REMOVE_DEFINITION | DDD_EXACT_MATCH_ON_REMOVE,
                mount_name.as_ptr(),
                target.as_ptr(),
            )
        };
        if removed == 0 {
            bail!("could not roll back R: recovery for ImDisk unit {number}");
        }
    }
    Ok(matches)
}

fn eject_without_mountpoint(number: u32) -> Result<()> {
    let name = wide(OsStr::new(&format!(
        r"\\?\GLOBALROOT\Device\ImDisk{number}"
    )));
    // Eject the verified stale device directly. ImDisk's `-d -u` also removes
    // its recorded R: DOS device, which can now belong to the live unit.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0xC000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        bail!("could not open stale ImDisk unit {number}");
    }
    let mut returned = 0u32;
    let mut locked = false;
    let result = (|| {
        if unsafe { FlushFileBuffers(handle) } == 0 {
            bail!("could not flush stale ImDisk unit {number}");
        }
        let mut ioctl = |code| unsafe {
            DeviceIoControl(
                handle,
                code,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ioctl(FSCTL_LOCK_VOLUME) == 0 {
            bail!("could not lock stale ImDisk unit {number}");
        }
        locked = true;
        if ioctl(FSCTL_DISMOUNT_VOLUME) == 0 || ioctl(IOCTL_STORAGE_EJECT_MEDIA) == 0 {
            bail!("could not eject stale ImDisk unit {number}");
        }
        Ok(())
    })();
    if locked {
        unsafe {
            DeviceIoControl(
                handle,
                FSCTL_UNLOCK_VOLUME,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            );
        }
    }
    unsafe { CloseHandle(handle) };
    result
}

pub fn reconcile_temp_r() -> Result<()> {
    let Some(exe) = imdisk_exe() else {
        return Ok(());
    };
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(OsStr::new(LOCK_SDDL)).as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        bail!("could not configure RAM disk coordination mutex");
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let mutex = unsafe { CreateMutexW(&attributes, 0, wide(OsStr::new(LOCK_NAME)).as_ptr()) };
    unsafe { LocalFree(descriptor) };
    if mutex.is_null() {
        bail!("could not acquire RAM disk coordination mutex");
    }
    let wait = unsafe { WaitForSingleObject(mutex, 30_000) };
    let result = if wait == 0 || wait == 0x80 {
        reconcile_locked(&exe)
    } else {
        Err(anyhow::anyhow!(
            "timed out waiting for RAM disk coordination"
        ))
    };
    if wait == 0 || wait == 0x80 {
        unsafe { ReleaseMutex(mutex) };
    }
    unsafe { CloseHandle(mutex) };
    result
}

fn reconcile_locked(exe: &PathBuf) -> Result<()> {
    let mut owner = mount_owner()?;
    if owner.is_some() {
        let mounted = run(exe, &["-l", "-m", "R:"])?;
        if !mounted.status.success() {
            bail!("could not inspect live R: RAM disk");
        }
    }
    let list = run(exe, &["-l", "-n"])?;
    if !list.status.success() {
        bail!("could not enumerate ImDisk units");
    }
    let mut units = Vec::new();
    for line in output_text(&list).lines().map(str::trim).filter(|line| !line.is_empty()) {
        let number: u32 = line.parse().context("unrecognized ImDisk device list")?;
        let details = run(exe, &["-l", "-u", &number.to_string()])?;
        if !details.status.success() {
            bail!("could not inspect ImDisk unit {number}");
        }
        units.push(parse_unit(number, &output_text(&details)));
    }
    if let Some(number) = owner {
        if !units
            .iter()
            .any(|unit| unit.number == number && unit.is_temp_r_candidate())
            || !mounted_label()?.eq_ignore_ascii_case("TEMP")
        {
            return Ok(()); // Unrelated R: disk: leave it untouched.
        }
    }
    if units
        .iter()
        .any(|unit| unit.letter == Some('R') && !unit.is_temp_r_candidate())
    {
        bail!("R: contains an unrecognized ImDisk unit");
    }
    if owner.is_none() {
        for candidate in units.iter().filter(|unit| unit.is_temp_r_candidate()) {
            if mount_existing_unit(candidate.number)? {
                owner = Some(candidate.number);
                break;
            }
        }
        if owner.is_none() {
            return Ok(()); // No matching attached TEMP disk; creation is opt-in.
        }
        if !run(exe, &["-l", "-m", "R:"])?.status.success() {
            bail!("restored R: could not be inspected by ImDisk");
        }
    }
    let Some(owner) = owner else { return Ok(()) };
    for unit in units
        .iter()
        .filter(|unit| unit.is_temp_r_candidate() && unit.number != owner)
    {
        eject_without_mountpoint(unit.number)?;
        if mount_owner()? != Some(owner) {
            bail!("R: mount changed during duplicate removal");
        }
        let mut remains = true;
        for _ in 0..15 {
            remains = run(exe, &["-l", "-u", &unit.number.to_string()])?
                .status
                .success();
            if !remains {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if remains {
            bail!("stale ImDisk unit {} remains attached", unit.number);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_cli_shape_identifies_only_matching_temp_ram_disk() {
        let live = parse_unit(7, "Drive letter: R\nNo image file.\nSize: 805306368 bytes (768 MB), Removable, Virtual Memory, HDD, Modified.\n");
        assert!(live.is_temp_r_candidate());
        assert_eq!(live.number, 7);
        let image = parse_unit(8, "Drive letter: R\nImage file: C:\\disk.img\nSize: 805306368 bytes (768 MB), Virtual Memory, HDD.\n");
        assert!(!image.is_temp_r_candidate());
        let other_size = parse_unit(9, "Drive letter: R\nNo image file.\nSize: 536870912 bytes (512 MB), Virtual Memory, HDD.\n");
        assert!(!other_size.is_temp_r_candidate());
    }

    #[test]
    fn coordination_mutex_acl_is_valid_on_windows() {
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(OsStr::new(LOCK_SDDL)).as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(ok, 0);
        assert!(!descriptor.is_null());
        unsafe { LocalFree(descriptor) };
    }
}
