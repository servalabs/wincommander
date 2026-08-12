use super::{Hive, BACKUP_ROOT};

#[cfg(windows)]
pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
pub(super) fn root(hive: Hive) -> windows_sys::Win32::System::Registry::HKEY {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    if hive == Hive::CurrentUser {
        HKEY_CURRENT_USER
    } else {
        HKEY_LOCAL_MACHINE
    }
}

#[cfg(windows)]
pub(super) fn key_exists(hive: Hive, subkey: &str) -> bool {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, KEY_READ};
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let found = RegOpenKeyExW(root(hive), wide(subkey).as_ptr(), 0, KEY_READ, &mut key)
            == ERROR_SUCCESS;
        if found {
            RegCloseKey(key);
        }
        found
    }
}

#[cfg(windows)]
pub(super) fn read_string(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    name: &str,
) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }
        let mut ty = 0;
        let mut data = [0u8; 8192];
        let mut len = data.len() as u32;
        let rc = RegQueryValueExW(
            key,
            wide(name).as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            data.as_mut_ptr(),
            &mut len,
        );
        RegCloseKey(key);
        if rc != ERROR_SUCCESS || !(ty == REG_SZ || ty == REG_EXPAND_SZ) || !len.is_multiple_of(2) {
            return None;
        }
        Some(
            String::from_utf16_lossy(std::slice::from_raw_parts(
                data.as_ptr() as *const u16,
                len as usize / 2,
            ))
            .trim_end_matches('\0')
            .to_string(),
        )
    }
}

#[cfg(windows)]
pub(super) fn write_string(
    hive: Hive,
    subkey: &str,
    name: &str,
    value: &str,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, KEY_WRITE, REG_OPTION_NON_VOLATILE,
        REG_SZ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegCreateKeyExW(
            root(hive),
            wide(subkey).as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null_mut(),
            &mut key,
            std::ptr::null_mut(),
        ) != ERROR_SUCCESS
        {
            return Err("could not create disabled Explorer menu store".into());
        }
        let data = wide(value);
        let rc = RegSetValueExW(
            key,
            wide(name).as_ptr(),
            0,
            REG_SZ,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
        );
        RegCloseKey(key);
        (rc == ERROR_SUCCESS)
            .then_some(())
            .ok_or_else(|| "could not write Explorer menu metadata".into())
    }
}

#[cfg(windows)]
pub(super) fn copy_tree(
    from_hive: Hive,
    from: &str,
    to_hive: Hive,
    to: &str,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCopyTreeW, RegCreateKeyExW, RegOpenKeyExW, HKEY, KEY_READ, KEY_WRITE,
        REG_OPTION_NON_VOLATILE,
    };
    unsafe {
        let mut source: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            root(from_hive),
            wide(from).as_ptr(),
            0,
            KEY_READ,
            &mut source,
        ) != ERROR_SUCCESS
        {
            return Err("Explorer menu entry changed since scan".into());
        }
        let mut destination: HKEY = std::ptr::null_mut();
        if RegCreateKeyExW(
            root(to_hive),
            wide(to).as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null_mut(),
            &mut destination,
            std::ptr::null_mut(),
        ) != ERROR_SUCCESS
        {
            RegCloseKey(source);
            return Err("could not create Explorer menu backup".into());
        }
        let rc = RegCopyTreeW(source, std::ptr::null(), destination);
        RegCloseKey(source);
        RegCloseKey(destination);
        (rc == ERROR_SUCCESS)
            .then_some(())
            .ok_or_else(|| "could not export Explorer menu backup".into())
    }
}

#[cfg(windows)]
pub(super) fn backup_and_copy(
    hive: Hive,
    source: &str,
    destination: &str,
) -> Result<String, String> {
    let backup = format!("{BACKUP_ROOT}\\{}", uuid::Uuid::new_v4().simple());
    copy_tree(hive, source, Hive::CurrentUser, &backup)?;
    copy_tree(hive, source, Hive::CurrentUser, destination)?;
    Ok(backup)
}

#[cfg(windows)]
pub(super) fn delete_tree(hive: Hive, subkey: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::RegDeleteTreeW;
    unsafe {
        (RegDeleteTreeW(root(hive), wide(subkey).as_ptr()) == ERROR_SUCCESS)
            .then_some(())
            .ok_or_else(|| "Explorer menu deletion failed; backup retained".into())
    }
}
