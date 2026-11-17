// SPDX-License-Identifier: AGPL-3.0-or-later
//! Absolute System32 paths for helper programs spawned by the SYSTEM service.
//! A bare program name must never be resolved through the process search path.
use std::path::PathBuf;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

pub(crate) fn system_directory() -> Result<PathBuf, &'static str> {
    const UNAVAILABLE: &str = "Windows system directory is unavailable";
    let mut buffer = vec![0u16; 32768];
    // The buffer is valid and writable for the whole call.
    let count = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if count == 0 || count >= buffer.len() {
        return Err(UNAVAILABLE);
    }
    let path = PathBuf::from(String::from_utf16(&buffer[..count]).map_err(|_| UNAVAILABLE)?);
    if !path.is_absolute() {
        return Err(UNAVAILABLE);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn system_directory_is_absolute_and_contains_cmd() {
        let dir = super::system_directory().unwrap();
        assert!(dir.is_absolute());
        assert!(dir.join("cmd.exe").is_file());
    }
}
