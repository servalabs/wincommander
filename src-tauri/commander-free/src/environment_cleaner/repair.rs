use super::registry;
use super::{CachedEntry, EnvironmentRepairResult};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub(super) fn repair_environment(entries: Vec<CachedEntry>) -> EnvironmentRepairResult {
    let mut result = EnvironmentRepairResult {
        repaired: 0,
        backup_locations: Vec::new(),
        errors: Vec::new(),
        environment_broadcast: false,
    };
    let mut paths = HashMap::new();
    for entry in entries {
        match entry {
            CachedEntry::Path {
                scope,
                old_path,
                missing_entry,
                value_type,
            } => {
                let bucket = paths
                    .entry((scope, old_path.clone()))
                    .or_insert_with(|| (old_path, value_type, HashSet::new()));
                bucket.2.insert(missing_entry.to_ascii_lowercase());
            }
            CachedEntry::Variable { scope, value } => {
                match registry::delete_value(scope, &value, &Uuid::new_v4().to_string()) {
                    Ok(backup) => {
                        result.repaired += 1;
                        result.backup_locations.push(backup);
                    }
                    Err(error) => result.errors.push(error),
                }
            }
        }
    }
    for ((scope, old_path), (_, value_type, removals)) in paths {
        let next: Vec<_> = old_path
            .split(';')
            .map(str::trim)
            .filter(|entry| !entry.is_empty() && !removals.contains(&entry.to_ascii_lowercase()))
            .collect();
        if next.is_empty() {
            result
                .errors
                .push("refused to remove the last PATH entry".into());
            continue;
        }
        match registry::replace_path(
            scope,
            &old_path,
            &next.join(";"),
            &Uuid::new_v4().to_string(),
        ) {
            Ok(backup) => {
                let removed = old_path
                    .split(';')
                    .filter(|entry| removals.contains(&entry.trim().to_ascii_lowercase()))
                    .count();
                result.repaired += removed;
                result.backup_locations.push(backup);
            }
            Err(error) => result.errors.push(error),
        }
        let _ = value_type;
    }
    if result.repaired > 0 {
        result.environment_broadcast = broadcast_environment_change();
    }
    result
}

#[cfg(windows)]
fn broadcast_environment_change() -> bool {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, SMTO_ABORTIFHUNG};
    unsafe {
        let mut result = 0usize;
        SendMessageTimeoutW(
            0xffffusize as HWND,
            0x001A,
            0,
            wide("Environment").as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5_000,
            &mut result,
        ) != 0
    }
}
#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
#[cfg(not(windows))]
fn broadcast_environment_change() -> bool {
    false
}
