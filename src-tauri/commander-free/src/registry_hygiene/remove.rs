use super::rules::{is_allowed_orphan, is_valid_clsid};
use super::{CachedOrphan, RegistryCleanerResult, CLSID_ROOT};

#[cfg(windows)]
pub(super) fn remove_orphans(entries: &[CachedOrphan]) -> Result<RegistryCleanerResult, String> {
    use super::windows_registry::{read_default, wide};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCopyTreeW, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE,
    };
    let mut backups = Vec::new();
    for entry in entries {
        if !entry.subkey.starts_with(&format!("{CLSID_ROOT}\\"))
            || !is_valid_clsid(&entry.class_id)
            || !matches!(
                entry.server_kind.as_str(),
                "InprocServer32" | "LocalServer32"
            )
        {
            return Err("refused an out-of-scope registry key".into());
        }
        let current = read_default(HKEY_CURRENT_USER, &entry.subkey)
            .ok_or_else(|| "registry entry changed since scan; scan again".to_string())?;
        if current != entry.server_path || !is_allowed_orphan(&entry.class_id, &current) {
            return Err("registry entry changed since scan; scan again".into());
        }
        let backup_subkey = format!("Software\\WinCommander\\RegistryBackups\\{}", entry.id);
        unsafe {
            let mut source: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(&entry.subkey).as_ptr(),
                0,
                KEY_READ,
                &mut source,
            ) != ERROR_SUCCESS
            {
                return Err("registry entry changed since scan; scan again".into());
            }
            let mut backup: HKEY = std::ptr::null_mut();
            let rc = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&backup_subkey).as_ptr(),
                0,
                std::ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null_mut(),
                &mut backup,
                std::ptr::null_mut(),
            );
            if rc != ERROR_SUCCESS {
                RegCloseKey(source);
                return Err(format!("could not create registry backup: {rc}"));
            }
            let copy_rc = RegCopyTreeW(source, std::ptr::null(), backup);
            RegCloseKey(source);
            RegCloseKey(backup);
            if copy_rc != ERROR_SUCCESS {
                return Err(format!("could not export registry backup: {copy_rc}"));
            }
            let delete_rc = RegDeleteTreeW(HKEY_CURRENT_USER, wide(&entry.subkey).as_ptr());
            if delete_rc != ERROR_SUCCESS {
                return Err(format!(
                    "registry backup retained; removal failed: {delete_rc}"
                ));
            }
        }
        backups.push(format!("HKCU\\{backup_subkey}"));
    }
    Ok(RegistryCleanerResult {
        removed: entries.len(),
        backup_locations: backups,
    })
}

#[cfg(not(windows))]
pub(super) fn remove_orphans(_entries: &[CachedOrphan]) -> Result<RegistryCleanerResult, String> {
    Err("registry cleaning is only available on Windows".into())
}
