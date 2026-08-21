use super::filters::{is_allowed_shell_verb, is_third_party_verb, safe_disabled_id};
use super::{
    CachedVerb, ExplorerContextEntry, ExplorerContextScan, Hive, DISABLED_ROOT, SHELL_ROOTS,
};

#[cfg(windows)]
pub(super) fn scan_verbs() -> Result<(ExplorerContextScan, Vec<CachedVerb>), String> {
    let mut entries = Vec::new();
    let mut skipped = 0;
    for hive in [Hive::CurrentUser, Hive::LocalMachine] {
        for shell_root in SHELL_ROOTS {
            enumerate_enabled(hive, shell_root, &mut entries, &mut skipped);
        }
    }
    enumerate_disabled(&mut entries, &mut skipped);
    let public = entries
        .iter()
        .map(|entry| ExplorerContextEntry {
            id: entry.id.clone(),
            label: entry.label.clone(),
            location: format!("{}\\{}", entry.hive.label(), entry.subkey),
            command: entry.command.clone(),
            enabled: entry.enabled,
        })
        .collect();
    Ok((
        ExplorerContextScan {
            entries: public,
            skipped_entries: skipped,
        },
        entries,
    ))
}

#[cfg(not(windows))]
pub(super) fn scan_verbs() -> Result<(ExplorerContextScan, Vec<CachedVerb>), String> {
    Ok((
        ExplorerContextScan {
            entries: Vec::new(),
            skipped_entries: 0,
        },
        Vec::new(),
    ))
}

#[cfg(windows)]
fn enumerate_enabled(
    hive: Hive,
    shell_root: &str,
    entries: &mut Vec<CachedVerb>,
    skipped: &mut usize,
) {
    use super::windows_registry::{read_string, root, wide};
    use uuid::Uuid;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, KEY_READ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root(hive), wide(shell_root).as_ptr(), 0, KEY_READ, &mut key)
            != ERROR_SUCCESS
        {
            return;
        }
        let mut index = 0;
        loop {
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            if RegEnumKeyExW(
                key,
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
            let label = String::from_utf16_lossy(&name[..length as usize]);
            let subkey = format!("{shell_root}\\{label}");
            let Some(command) = read_string(root(hive), &format!("{subkey}\\command"), "") else {
                *skipped += 1;
                continue;
            };
            if !is_allowed_shell_verb(&subkey) || !is_third_party_verb(&label, &command) {
                *skipped += 1;
                continue;
            }
            entries.push(CachedVerb {
                id: Uuid::new_v4().simple().to_string(),
                hive,
                subkey,
                label,
                command,
                enabled: true,
                disabled_id: None,
            });
        }
        RegCloseKey(key);
    }
}

#[cfg(windows)]
fn enumerate_disabled(entries: &mut Vec<CachedVerb>, skipped: &mut usize) {
    use super::windows_registry::{read_string, wide};
    use uuid::Uuid;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(DISABLED_ROOT).as_ptr(),
            0,
            KEY_READ,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return;
        }
        let mut index = 0;
        loop {
            let mut name = [0u16; 64];
            let mut length = name.len() as u32;
            if RegEnumKeyExW(
                key,
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
            let disabled_id = String::from_utf16_lossy(&name[..length as usize]);
            let base = format!("{DISABLED_ROOT}\\{disabled_id}");
            let hive = match read_string(HKEY_CURRENT_USER, &base, "OriginalHive").as_deref() {
                Some("HKCU") => Hive::CurrentUser,
                Some("HKLM") => Hive::LocalMachine,
                _ => {
                    *skipped += 1;
                    continue;
                }
            };
            let Some(subkey) = read_string(HKEY_CURRENT_USER, &base, "OriginalSubkey") else {
                *skipped += 1;
                continue;
            };
            let Some(command) =
                read_string(HKEY_CURRENT_USER, &format!("{base}\\Entry\\command"), "")
            else {
                *skipped += 1;
                continue;
            };
            let label = subkey.rsplit('\\').next().unwrap_or_default().to_string();
            if !safe_disabled_id(&disabled_id)
                || !is_allowed_shell_verb(&subkey)
                || !is_third_party_verb(&label, &command)
            {
                *skipped += 1;
                continue;
            }
            entries.push(CachedVerb {
                id: Uuid::new_v4().simple().to_string(),
                hive,
                subkey,
                label,
                command,
                enabled: false,
                disabled_id: Some(disabled_id),
            });
        }
        RegCloseKey(key);
    }
}
