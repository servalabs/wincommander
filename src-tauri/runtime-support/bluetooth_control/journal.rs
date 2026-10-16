// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{
    engine::Journal,
    security::{self, Descriptor},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    Security::Authorization::SE_REGISTRY_KEY,
    System::Registry::*,
};

const PATH: &str = "SOFTWARE\\WinCommanderBluetoothPolicyV2";
const VALUE: &str = "State";
const MAX_BYTES: u32 = 1_048_576;
const STATE_ERROR: &str = "[BT_STATE_UNVERIFIED] Bluetooth policy ownership could not be read or saved. No uncertain policy will be changed; contact your administrator.";

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

fn open(path: &str, write: bool) -> Result<Option<Key>, String> {
    let mut key = std::ptr::null_mut();
    let rights =
        KEY_QUERY_VALUE | KEY_WOW64_64KEY | 0x0002_0000 | if write { KEY_SET_VALUE } else { 0 };
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(path).as_ptr(),
            REG_OPTION_OPEN_LINK,
            rights,
            &mut key,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(STATE_ERROR.into());
    }
    let key = Key(key);
    // A registry symbolic link must never redirect recovery authority.
    let mut size = 0;
    let link = unsafe {
        RegQueryValueExW(
            key.0,
            wide("SymbolicLinkValue").as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if link != ERROR_FILE_NOT_FOUND {
        return Err(STATE_ERROR.into());
    }
    security::verify(key.0.cast(), SE_REGISTRY_KEY, 0x500d_0006)?;
    Ok(Some(key))
}

fn state_key(write: bool) -> Result<Option<Key>, String> {
    if let Some(key) = open(PATH, write)? {
        return Ok(Some(key));
    }
    if !write {
        return Ok(None);
    }
    let descriptor = Descriptor::new()?;
    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(PATH).as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY | 0x0002_0000,
            &descriptor.attributes(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if result != ERROR_SUCCESS {
        return Err(STATE_ERROR.into());
    }
    drop(Key(key));
    // Reopen without following a leaf registry link and verify actual protection.
    open(PATH, true)
}

pub(super) fn read() -> Result<Journal, String> {
    let Some(bytes) = read_bytes(PATH)? else {
        return Ok(Journal::default());
    };
    serde_json::from_slice(&bytes).map_err(|_| STATE_ERROR.into())
}

pub(super) fn read_bytes(path: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(key) = open(path, false)? else {
        return Ok(None);
    };
    let mut kind = 0;
    let mut bytes = 0;
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            wide(VALUE).as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        // An empty protected key has not authorized a policy change.
        return Ok(None);
    }
    if result != ERROR_SUCCESS || kind != REG_BINARY || bytes == 0 || bytes > MAX_BYTES {
        return Err(STATE_ERROR.into());
    }
    let mut buffer = vec![0u8; bytes as usize];
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            wide(VALUE).as_ptr(),
            std::ptr::null(),
            &mut kind,
            buffer.as_mut_ptr(),
            &mut bytes,
        )
    };
    if result != ERROR_SUCCESS || kind != REG_BINARY || bytes as usize > buffer.len() {
        return Err(STATE_ERROR.into());
    }
    buffer.truncate(bytes as usize);
    Ok(Some(buffer))
}

pub(super) fn write(journal: &Journal) -> Result<(), String> {
    let bytes = serde_json::to_vec(journal).map_err(|_| STATE_ERROR.to_string())?;
    if bytes.len() > MAX_BYTES as usize {
        return Err(STATE_ERROR.into());
    }
    let key = state_key(true)?.ok_or(STATE_ERROR)?;
    // One registry value is the transaction unit. Flush must succeed before any
    // device mutation: the preceding intent must survive device/power restart.
    if unsafe {
        RegSetValueExW(
            key.0,
            wide(VALUE).as_ptr(),
            0,
            REG_BINARY,
            bytes.as_ptr(),
            bytes.len() as u32,
        )
    } != ERROR_SUCCESS
        || unsafe { RegFlushKey(key.0) } != ERROR_SUCCESS
    {
        return Err(STATE_ERROR.into());
    }
    let verified = read()?;
    if serde_json::to_vec(&verified).map_err(|_| STATE_ERROR.to_string())? != bytes {
        return Err(STATE_ERROR.into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
