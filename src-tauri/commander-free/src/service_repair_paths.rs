// SPDX-License-Identifier: AGPL-3.0-or-later
//! Elevated service repair must not trust inherited ProgramFiles/PATH values.
#![cfg(windows)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

pub(crate) fn protected_install_dir() -> Result<PathBuf, String> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath};
    unsafe {
        let mut raw = std::ptr::null_mut();
        let result =
            SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &mut raw);
        if result != 0 || raw.is_null() {
            if !raw.is_null() {
                CoTaskMemFree(raw.cast());
            }
            return Err("Windows Program Files location is unavailable".into());
        }
        let mut length = 0;
        while *raw.add(length) != 0 {
            length += 1;
        }
        let path = PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(raw, length)));
        CoTaskMemFree(raw.cast());
        if !path.is_absolute() {
            return Err("Windows Program Files path is invalid".into());
        }
        Ok(path.join("WinCommander"))
    }
}

fn system_directory() -> Result<PathBuf, String> {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err("Windows system directory is unavailable".into());
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..length]));
    if !path.is_absolute() {
        return Err("Windows system directory is invalid".into());
    }
    Ok(path)
}

pub(crate) fn service_control_executable() -> Result<PathBuf, String> {
    Ok(system_directory()?.join("sc.exe"))
}

pub(crate) fn powershell_executable() -> Result<PathBuf, String> {
    Ok(system_directory()?
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_uses_absolute_os_resolved_paths_without_executing_anything() {
        let install = protected_install_dir().unwrap();
        assert!(install.is_absolute());
        assert_eq!(install.file_name().unwrap(), "WinCommander");
        let executable = service_control_executable().unwrap();
        assert!(executable.is_absolute());
        assert!(executable.is_file());
        assert_eq!(executable.file_name().unwrap(), "sc.exe");
        let powershell = powershell_executable().unwrap();
        assert!(powershell.is_absolute());
        assert!(powershell.is_file());
    }
}
