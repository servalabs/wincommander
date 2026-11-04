// SPDX-License-Identifier: AGPL-3.0-or-later
//! A second copy of the signed update record, protected by a verified registry ACL.
use std::ptr;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegFlushKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        HKEY, HKEY_LOCAL_MACHINE, KEY_CREATE_SUB_KEY, KEY_QUERY_VALUE, KEY_SET_VALUE,
        KEY_WOW64_64KEY, REG_BINARY, REG_OPTION_NON_VOLATILE,
    },
};

#[path = "hybrid_free_anchor_security.rs"]
mod security;

const LIMIT: usize = 65536;

pub(crate) struct Anchor {
    root: HKEY,
    path: String,
    secured: bool,
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
    pub(crate) fn machine_named(path: &str) -> Self {
        Self {
            root: HKEY_LOCAL_MACHINE,
            path: path.into(),
            secured: true,
        }
    }
    #[cfg(test)]
    pub(crate) fn test_user(path: String) -> Self {
        Self {
            root: windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            path,
            secured: false,
        }
    }
    #[cfg(test)]
    pub(crate) fn test_secured(path: String) -> Self {
        Self {
            root: windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            path,
            secured: true,
        }
    }
    fn open(&self, write: bool) -> Result<Option<Key>, String> {
        let mut handle: HKEY = ptr::null_mut();
        let path = wide(&self.path);
        let flags =
            KEY_QUERY_VALUE | KEY_WOW64_64KEY | 0x0002_0000 | if write { KEY_SET_VALUE } else { 0 }; // READ_CONTROL
        let security = if self.secured && write {
            Some(security::Security::new()?)
        } else {
            None
        };
        // Create missing parent keys with their ordinary inherited permissions;
        // the protected descriptor belongs only on our dedicated leaf key.
        let parent = if self.secured && write {
            let (parent_path, leaf) = self
                .path
                .rsplit_once('\\')
                .ok_or("update registry path is invalid")?;
            let mut parent_handle = ptr::null_mut();
            let result = unsafe {
                RegCreateKeyExW(
                    self.root,
                    wide(parent_path).as_ptr(),
                    0,
                    ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_CREATE_SUB_KEY | KEY_WOW64_64KEY,
                    ptr::null(),
                    &mut parent_handle,
                    ptr::null_mut(),
                )
            };
            if result != ERROR_SUCCESS || parent_handle.is_null() {
                return Err("update registry parent is unavailable".into());
            }
            Some((Key(parent_handle), wide(leaf)))
        } else {
            None
        };
        let result = unsafe {
            if write {
                let (base, name) = parent
                    .as_ref()
                    .map_or((self.root, path.as_ptr()), |(key, leaf)| {
                        (key.0, leaf.as_ptr())
                    });
                RegCreateKeyExW(
                    base,
                    name,
                    0,
                    ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    flags,
                    security
                        .as_ref()
                        .map_or(ptr::null(), |security| security.attributes()),
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
            return Err("update registry is unavailable".into());
        }
        let key = Key(handle);
        if self.secured {
            security::verify(key.0)?;
        }
        Ok(Some(key))
    }
    /// A present but empty or malformed key still blocks a classical downgrade.
    #[allow(dead_code)] // Only the service's classical build uses this check.
    pub(crate) fn exists(&self) -> Result<bool, String> {
        Ok(self.open(false)?.is_some())
    }
    pub(crate) fn read(&self) -> Result<Option<Vec<u8>>, String> {
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
            || length as usize > LIMIT
        {
            return Err("update registry record is invalid".into());
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
            return Err("update registry record changed during reading".into());
        }
        Ok(Some(bytes))
    }
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > LIMIT {
            return Err("update registry record is invalid".into());
        }
        let key = self
            .open(true)?
            .ok_or("update registry is unavailable")?;
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
            return Err("Cannot save update registry record".into());
        }
        if self.read()?.as_deref() != Some(bytes) {
            return Err("update registry readback failed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn remove_test_key(path: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyExW, HKEY_CURRENT_USER};
    let status =
        unsafe { RegDeleteKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), KEY_WOW64_64KEY, 0) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Cannot remove test key: {status}"))
    }
}

#[cfg(test)]
#[path = "hybrid_free_anchor_tests.rs"]
mod tests;
