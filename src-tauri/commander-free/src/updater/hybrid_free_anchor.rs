// SPDX-License-Identifier: AGPL-3.0-or-later
//! A second copy of the signed update record. Registry ACLs still require audit.
use std::ptr;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegFlushKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_BINARY,
        REG_OPTION_NON_VOLATILE,
    },
};

pub(super) struct Anchor {
    root: HKEY,
    path: String,
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

impl Anchor {
    pub(super) fn machine() -> Self {
        Self {
            root: HKEY_LOCAL_MACHINE,
            path: r"SOFTWARE\ServaLabs\WinCommander\HybridFreeUpdate".into(),
        }
    }
    #[cfg(test)]
    pub(super) fn test_user(path: String) -> Self {
        Self {
            root: windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            path,
        }
    }
    fn open(&self, write: bool) -> Result<Option<Key>, String> {
        let mut handle: HKEY = ptr::null_mut();
        let path = wide(&self.path);
        let flags = KEY_QUERY_VALUE | KEY_WOW64_64KEY | if write { KEY_SET_VALUE } else { 0 };
        let result = unsafe {
            if write {
                RegCreateKeyExW(
                    self.root,
                    path.as_ptr(),
                    0,
                    ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    flags,
                    ptr::null(),
                    &mut handle,
                    ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(self.root, path.as_ptr(), 0, flags, &mut handle)
            }
        };
        if result == ERROR_FILE_NOT_FOUND && !write {
            return Ok(None);
        }
        if result != ERROR_SUCCESS || handle.is_null() {
            return Err("Free update registry is unavailable".into());
        }
        Ok(Some(Key(handle)))
    }
    pub(super) fn read(&self) -> Result<Option<Vec<u8>>, String> {
        let Some(key) = self.open(false)? else {
            return Ok(None);
        };
        let value = wide("SignedFloor");
        let mut kind = 0;
        let mut length = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                value.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut length,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS
            || kind != REG_BINARY
            || length == 0
            || length as usize > super::LIMIT
        {
            return Err("Free update registry record is invalid".into());
        }
        let mut bytes = vec![0u8; length as usize];
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                value.as_ptr(),
                ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut length,
            )
        };
        if status != ERROR_SUCCESS || kind != REG_BINARY || length as usize != bytes.len() {
            return Err("Free update registry record changed during reading".into());
        }
        Ok(Some(bytes))
    }
    pub(super) fn write(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > super::LIMIT {
            return Err("Free update registry record is invalid".into());
        }
        let key = self
            .open(true)?
            .ok_or("Free update registry is unavailable")?;
        let value = wide("SignedFloor");
        let status = unsafe {
            RegSetValueExW(
                key.0,
                value.as_ptr(),
                0,
                REG_BINARY,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        };
        if status != ERROR_SUCCESS || unsafe { RegFlushKey(key.0) } != ERROR_SUCCESS {
            return Err("Cannot save Free update registry record".into());
        }
        if self.read()?.as_deref() != Some(bytes) {
            return Err("Free update registry readback failed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn remove_test_key(path: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyExW, HKEY_CURRENT_USER};
    let status =
        unsafe { RegDeleteKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), KEY_WOW64_64KEY, 0) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Cannot remove test key: {status}"))
    }
}
