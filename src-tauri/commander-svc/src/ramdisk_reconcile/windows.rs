// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-time repair for duplicate ImDisk TEMP R: units left by older clients.

use anyhow::{bail, Context, Result};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
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

const LOCK_NAME: &str = "Global\\WinCommander-RamDisk-R";
const LOCK_SDDL: &str = "D:(A;;0x001F0001;;;SY)(A;;0x001F0001;;;BA)(A;;0x00100001;;;AU)";

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

pub(super) fn mount_owner() -> Result<Option<u32>> {
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

pub(super) fn mounted_label() -> Result<String> {
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

pub(super) fn unit_label(number: u32) -> Result<String> {
    let mut label = [0u16; 64];
    let root = format!(r"\\?\GLOBALROOT\Device\ImDisk{number}\");
    let ok = unsafe {
        GetVolumeInformationW(
            wide(OsStr::new(&root)).as_ptr(),
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
        bail!("could not read ImDisk unit {number} label");
    }
    let end = label.iter().position(|c| *c == 0).unwrap_or(label.len());
    Ok(OsString::from_wide(&label[..end])
        .to_string_lossy()
        .into_owned())
}

pub(super) fn mount_existing_unit(number: u32) -> Result<bool> {
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

pub(super) fn eject_without_mountpoint(number: u32) -> Result<()> {
    if !unit_label(number)?.eq_ignore_ascii_case("TEMP") {
        bail!("ImDisk unit {number} is not a TEMP volume");
    }
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

pub(super) fn with_coordination_lock<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
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
        work()
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

#[cfg(test)]
mod tests {
    use super::*;

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
