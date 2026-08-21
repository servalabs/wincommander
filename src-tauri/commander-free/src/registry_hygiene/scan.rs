use super::rules::{is_allowed_orphan, is_valid_clsid};
use super::{CachedOrphan, RegistryCleanerScan, RegistryOrphan, CLSID_ROOT};

#[cfg(windows)]
pub(super) fn scan_orphans() -> Result<(RegistryCleanerScan, Vec<CachedOrphan>), String> {
    use super::windows_registry::{read_default, read_wide, wide};
    use uuid::Uuid;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };
    let mut entries = Vec::new();
    let mut skipped_entries = 0;
    unsafe {
        let mut root: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(CLSID_ROOT).as_ptr(),
            0,
            KEY_READ,
            &mut root,
        ) != ERROR_SUCCESS
        {
            return Ok((
                RegistryCleanerScan {
                    entries: Vec::new(),
                    skipped_entries,
                },
                Vec::new(),
            ));
        }
        let mut index = 0;
        loop {
            let mut name = [0u16; 64];
            let mut length = name.len() as u32;
            if RegEnumKeyExW(
                root,
                index,
                name.as_mut_ptr(),
                &mut length,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != ERROR_SUCCESS
            {
                break;
            }
            index += 1;
            let class_id = read_wide(&name[..length as usize]);
            if !is_valid_clsid(&class_id) {
                skipped_entries += 1;
                continue;
            }
            for server_kind in ["InprocServer32", "LocalServer32"] {
                let subkey = format!("{CLSID_ROOT}\\{class_id}\\{server_kind}");
                let Some(server) = read_default(HKEY_CURRENT_USER, &subkey) else {
                    continue;
                };
                if !is_allowed_orphan(&class_id, &server) {
                    continue;
                }
                entries.push(CachedOrphan {
                    id: Uuid::new_v4().simple().to_string(),
                    subkey,
                    class_id: class_id.clone(),
                    server_kind: server_kind.into(),
                    server_path: server,
                });
            }
        }
        RegCloseKey(root);
    }
    let public = entries
        .iter()
        .map(|entry| RegistryOrphan {
            id: entry.id.clone(),
            class_id: entry.class_id.clone(),
            server_kind: entry.server_kind.clone(),
            missing_server: entry.server_path.clone(),
            hive: "HKCU".into(),
        })
        .collect();
    Ok((
        RegistryCleanerScan {
            entries: public,
            skipped_entries,
        },
        entries,
    ))
}

#[cfg(not(windows))]
pub(super) fn scan_orphans() -> Result<(RegistryCleanerScan, Vec<CachedOrphan>), String> {
    Ok((
        RegistryCleanerScan {
            entries: Vec::new(),
            skipped_entries: 0,
        },
        Vec::new(),
    ))
}
