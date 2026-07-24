// SPDX-License-Identifier: AGPL-3.0-or-later

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum Scope {
    User,
    Machine,
}
impl Scope {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Machine => "machine",
        }
    }
}
#[derive(Clone)]
pub(super) struct Value {
    pub(super) name: String,
    pub(super) text: String,
    pub(super) value_type: u32,
}

#[cfg(windows)]
const USER_KEY: &str = "Environment";
#[cfg(windows)]
const MACHINE_KEY: &str = "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment";

#[cfg(windows)]
pub(super) fn values(scope: Scope) -> Result<Vec<Value>, String> {
    use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    let (root, key) = root_and_key(scope);
    unsafe {
        let mut handle: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(key).as_ptr(), 0, KEY_READ, &mut handle) != ERROR_SUCCESS {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for index in 0..256u32 {
            let mut name = [0u16; 32_768];
            let mut name_len = name.len() as u32;
            let mut data = [0u8; 65_536];
            let mut data_len = data.len() as u32;
            let mut value_type = 0u32;
            let rc = RegEnumValueW(
                handle,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                &mut value_type,
                data.as_mut_ptr(),
                &mut data_len,
            );
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS
                || !(value_type == REG_SZ || value_type == REG_EXPAND_SZ)
                || !data_len.is_multiple_of(2)
            {
                continue;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(
                data.as_ptr() as *const u16,
                data_len as usize / 2,
            ))
            .trim_end_matches('\0')
            .to_string();
            result.push(Value {
                name: String::from_utf16_lossy(&name[..name_len as usize]),
                text,
                value_type,
            });
        }
        RegCloseKey(handle);
        Ok(result)
    }
}

#[cfg(windows)]
pub(super) fn replace_path(
    scope: Scope,
    expected: &str,
    next: &str,
    backup_id: &str,
) -> Result<String, String> {
    let current = values(scope)?
        .into_iter()
        .find(|value| value.name.eq_ignore_ascii_case("path"))
        .ok_or_else(|| "PATH changed since scan; scan again".to_string())?;
    if current.text != expected {
        return Err("PATH changed since scan; scan again".into());
    }
    backup(scope, &current, backup_id)?;
    set_value(scope, "Path", next, current.value_type)?;
    Ok(backup_location(backup_id))
}

#[cfg(windows)]
pub(super) fn delete_value(
    scope: Scope,
    expected: &Value,
    backup_id: &str,
) -> Result<String, String> {
    let current = values(scope)?
        .into_iter()
        .find(|value| value.name.eq_ignore_ascii_case(&expected.name))
        .ok_or_else(|| "environment value changed since scan; scan again".to_string())?;
    if current.text != expected.text || current.value_type != expected.value_type {
        return Err("environment value changed since scan; scan again".into());
    }
    backup(scope, &current, backup_id)?;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, KEY_WRITE,
    };
    let (root, key) = root_and_key(scope);
    unsafe {
        let mut handle: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(key).as_ptr(), 0, KEY_WRITE, &mut handle) != ERROR_SUCCESS {
            return Err("could not open environment registry key for writing".into());
        }
        let rc = RegDeleteValueW(handle, wide(&current.name).as_ptr());
        RegCloseKey(handle);
        if rc != ERROR_SUCCESS {
            return Err(format!("environment removal failed: {rc}"));
        }
    }
    Ok(backup_location(backup_id))
}

#[cfg(windows)]
fn backup(scope: Scope, value: &Value, backup_id: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    let path = format!("Software\\WinCommander\\RegistryBackups\\Environment\\{backup_id}");
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(&path).as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != ERROR_SUCCESS
        {
            return Err("could not create environment registry backup".into());
        }
        for (name, text, value_type) in [
            ("Scope", scope.label(), REG_SZ),
            ("Variable", value.name.as_str(), REG_SZ),
            ("OriginalValue", value.text.as_str(), value.value_type),
        ] {
            let units = wide(text);
            let rc = RegSetValueExW(
                key,
                wide(name).as_ptr(),
                0,
                value_type,
                units.as_ptr() as *const u8,
                (units.len() * 2) as u32,
            );
            if rc != ERROR_SUCCESS {
                RegCloseKey(key);
                return Err(format!("could not write environment registry backup: {rc}"));
            }
        }
        RegCloseKey(key);
    }
    Ok(())
}

#[cfg(windows)]
fn set_value(scope: Scope, name: &str, text: &str, value_type: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, KEY_WRITE,
    };
    let (root, key) = root_and_key(scope);
    let units = wide(text);
    unsafe {
        let mut handle: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(key).as_ptr(), 0, KEY_WRITE, &mut handle) != ERROR_SUCCESS {
            return Err("could not open PATH registry key for writing".into());
        }
        let rc = RegSetValueExW(
            handle,
            wide(name).as_ptr(),
            0,
            value_type,
            units.as_ptr() as *const u8,
            (units.len() * 2) as u32,
        );
        RegCloseKey(handle);
        if rc != ERROR_SUCCESS {
            return Err(format!("PATH repair failed: {rc}"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn root_and_key(scope: Scope) -> (windows_sys::Win32::System::Registry::HKEY, &'static str) {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    match scope {
        Scope::User => (HKEY_CURRENT_USER, USER_KEY),
        Scope::Machine => (HKEY_LOCAL_MACHINE, MACHINE_KEY),
    }
}
#[cfg(windows)]
fn backup_location(id: &str) -> String {
    format!("HKCU\\Software\\WinCommander\\RegistryBackups\\Environment\\{id}")
}
#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
pub(super) fn values(_scope: Scope) -> Result<Vec<Value>, String> {
    Ok(Vec::new())
}
#[cfg(not(windows))]
pub(super) fn replace_path(
    _scope: Scope,
    _expected: &str,
    _next: &str,
    _backup_id: &str,
) -> Result<String, String> {
    Err("environment repair is only available on Windows".into())
}
#[cfg(not(windows))]
pub(super) fn delete_value(
    _scope: Scope,
    _expected: &Value,
    _backup_id: &str,
) -> Result<String, String> {
    Err("environment repair is only available on Windows".into())
}
