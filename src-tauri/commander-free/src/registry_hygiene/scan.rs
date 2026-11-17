use super::rules::{is_allowed_orphan, is_valid_clsid};
use super::{CachedOrphan, RegistryCleanerScan, RegistryOrphan, CLSID_ROOT};

#[cfg(windows)]
pub(super) fn scan_orphans() -> Result<(RegistryCleanerScan, Vec<CachedOrphan>), String> {
    use super::windows_registry::{read_default, wide};
    use uuid::Uuid;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };
    use crate::runtime_visibility::registry::{enum_key_name, EnumCursor};
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
        let mut cursor = EnumCursor::default();
        while let Some(class_id) = cursor.next(|i, _| enum_key_name(root, i)) {
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
