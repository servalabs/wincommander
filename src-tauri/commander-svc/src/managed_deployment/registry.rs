// SPDX-License-Identifier: AGPL-3.0-or-later
//! Keep machine authority under the SCM-owned key, away from user-writable paths.

use super::authority::Authority;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
    },
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegFlushKey, RegGetKeySecurity, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE,
        KEY_WOW64_64KEY, REG_BINARY,
    },
};

const KEY: &str = r"SYSTEM\CurrentControlSet\Services\WinCommanderSvc\Parameters\ManagedDeployment";
const ACL: &str = "D:P(A;;KA;;;SY)(A;;KA;;;BA)";
const MAX_STATE: u32 = 1_048_576;

pub(super) struct Store(HKEY);

impl Drop for Store {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

impl Store {
    pub fn existing() -> Result<Option<Self>, &'static str> {
        let mut key = std::ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(KEY).as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_WOW64_64KEY | 0x0002_0000,
                &mut key,
            )
        };
        if status == 2 {
            return Ok(None);
        }
        if status != 0 {
            return Err("management_store_unavailable");
        }
        let store = Self(key);
        store.verify_acl()?;
        Ok(Some(store))
    }

    pub fn open() -> Result<Self, &'static str> {
        Self::open_at(KEY)
    }

    fn open_at(path: &str) -> Result<Self, &'static str> {
        let mut descriptor = std::ptr::null_mut();
        // The descriptor is owned until RegCreateKeyExW returns and copied by Windows.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(ACL).as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err("management_store_insecure");
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let mut key = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(path).as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY | 0x0002_0000,
                &attributes,
                &mut key,
                std::ptr::null_mut(),
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        if status != 0 {
            return Err("management_store_unavailable");
        }
        let store = Self(key);
        store.verify_acl()?;
        Ok(store)
    }

    fn verify_acl(&self) -> Result<(), &'static str> {
        let mut size = 0;
        unsafe {
            RegGetKeySecurity(
                self.0,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                &mut size,
            );
        }
        if size == 0 || size > 65_536 {
            return Err("management_store_insecure");
        }
        let mut bytes = vec![0u8; size as usize];
        if unsafe {
            RegGetKeySecurity(
                self.0,
                DACL_SECURITY_INFORMATION,
                bytes.as_mut_ptr().cast(),
                &mut size,
            )
        } != 0
        {
            return Err("management_store_insecure");
        }
        let mut text = std::ptr::null_mut();
        if unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                bytes.as_mut_ptr().cast(),
                1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err("management_store_insecure");
        }
        // Windows owns this NUL-terminated allocation until LocalFree below.
        let rendered = unsafe {
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
        };
        unsafe {
            LocalFree(text.cast());
        }
        if rendered != ACL {
            return Err("management_store_insecure");
        }
        Ok(())
    }

    pub fn load(&self) -> Result<Authority, &'static str> {
        let value = wide("Authority");
        let mut kind = 0;
        let mut size = 0;
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                value.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status == 2 {
            return Ok(Authority::default());
        }
        if status != 0 || kind != REG_BINARY || size == 0 || size > MAX_STATE {
            return Err("management_store_invalid");
        }
        let mut bytes = vec![0u8; size as usize];
        if unsafe {
            RegQueryValueExW(
                self.0,
                value.as_ptr(),
                std::ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        } != 0
            || kind != REG_BINARY
        {
            return Err("management_store_unavailable");
        }
        bytes.truncate(size as usize);
        serde_json::from_slice(&bytes).map_err(|_| "management_store_invalid")
    }

    pub fn save(&self, state: &Authority) -> Result<(), &'static str> {
        let bytes = serde_json::to_vec(state).map_err(|_| "management_store_invalid")?;
        if bytes.len() > MAX_STATE as usize {
            return Err("management_store_full");
        }
        if unsafe {
            RegSetValueExW(
                self.0,
                wide("Authority").as_ptr(),
                0,
                REG_BINARY,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        } != 0
            || unsafe { RegFlushKey(self.0) } != 0
        {
            return Err("management_store_unavailable");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires elevated Windows; creates and removes one isolated SCM test subkey"]
    fn protected_registry_round_trip() {
        use windows_sys::Win32::System::Registry::RegDeleteKeyExW;
        let path = format!(r"{KEY}Test-{}", std::process::id());
        let store = Store::open_at(&path).expect("create protected test key");
        let state = Authority::default();
        store.save(&state).expect("flush protected state");
        assert!(store
            .load()
            .expect("reload protected state")
            .context
            .is_none());
        drop(store);
        assert_eq!(
            unsafe {
                RegDeleteKeyExW(HKEY_LOCAL_MACHINE, wide(&path).as_ptr(), KEY_WOW64_64KEY, 0)
            },
            0
        );
    }
}
