use super::filesystem::is_link_or_reparse;
use super::{BrokenShortcut, CachedShortcut, ShortcutScan, MAX_SHORTCUTS};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

pub(super) fn scan_shortcuts(
    cancelled: &AtomicBool,
) -> (ShortcutScan, HashMap<String, CachedShortcut>) {
    let mut public = Vec::new();
    let mut cached = HashMap::new();
    let mut scanned = 0;
    let mut truncated = false;
    for root in shortcut_roots() {
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if scanned >= MAX_SHORTCUTS {
                    truncated = true;
                    break;
                }
                let path = entry.path();
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if is_link_or_reparse(&meta) {
                    continue;
                }
                if meta.is_dir() {
                    if !is_system_shortcut_directory(&path) {
                        stack.push(path);
                    }
                    continue;
                }
                if !meta.is_file()
                    || !path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
                {
                    continue;
                }
                scanned += 1;
                let Some(target) = lnk_local_target(&path) else {
                    continue;
                };
                if target.exists() || is_protected_target(&target) {
                    continue;
                }
                let id = Uuid::new_v4().to_string();
                public.push(BrokenShortcut {
                    id: id.clone(),
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    target: target.to_string_lossy().into_owned(),
                });
                cached.insert(
                    id,
                    CachedShortcut {
                        path,
                        root: root.clone(),
                        bytes: meta.len(),
                        modified: meta.modified().ok(),
                    },
                );
            }
            if truncated {
                break;
            }
        }
        if cancelled.load(Ordering::Acquire) || truncated {
            break;
        }
    }
    public.sort_by(|left, right| left.path.cmp(&right.path));
    (
        ShortcutScan {
            shortcuts: public,
            scanned_shortcuts: scanned,
            cancelled: cancelled.load(Ordering::Acquire),
            truncated,
        },
        cached,
    )
}

fn shortcut_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(&profile).join("Desktop"));
    }
    if let Ok(app_data) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(app_data).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(program_data) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(program_data).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    roots
        .into_iter()
        .filter_map(|root| {
            let meta = fs::symlink_metadata(&root).ok()?;
            (!is_link_or_reparse(&meta) && meta.is_dir())
                .then(|| fs::canonicalize(root).ok())
                .flatten()
        })
        .collect()
}

pub(super) fn lnk_local_target(path: &Path) -> Option<PathBuf> {
    let data = fs::read(path).ok()?;
    if data.len() < 0x4c || read_u32(&data, 0)? != 0x4c || read_u32(&data, 0x14)? & 0x2 == 0 {
        return None;
    }
    let mut offset = 0x4c;
    if read_u32(&data, 0x14)? & 0x1 != 0 {
        offset += read_u16(&data, offset)? as usize + 2;
    }
    let info_size = read_u32(&data, offset)? as usize;
    let header_size = read_u32(&data, offset + 4)? as usize;
    if info_size < header_size || offset.checked_add(info_size)? > data.len() || header_size < 0x1c
    {
        return None;
    }
    let unicode_base_offset = if header_size >= 0x24 {
        read_u32(&data, offset + 0x1c)? as usize
    } else {
        0
    };
    let base_offset = read_u32(&data, offset + 0x10)? as usize;
    let suffix_offset = read_u32(&data, offset + 0x18)? as usize;
    let base = read_lnk_string(&data, offset, info_size, unicode_base_offset, true)
        .or_else(|| read_lnk_string(&data, offset, info_size, base_offset, false))?;
    let suffix =
        read_lnk_string(&data, offset, info_size, suffix_offset, false).unwrap_or_default();
    (!base.is_empty()).then(|| PathBuf::from(base).join(suffix))
}

fn read_lnk_string(
    data: &[u8],
    start: usize,
    size: usize,
    relative: usize,
    unicode: bool,
) -> Option<String> {
    if relative == 0 || relative >= size {
        return None;
    }
    let slice = &data[start + relative..start + size];
    if unicode {
        let units: Vec<u16> = slice
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|c| *c != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    } else {
        Some(String::from_utf8_lossy(slice.split(|byte| *byte == 0).next()?).into_owned())
    }
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}
pub(super) fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

pub(super) fn is_system_shortcut_directory(path: &Path) -> bool {
    [
        "system tools",
        "administrative tools",
        "accessibility",
        "windows powershell",
        "windows system",
        "windows accessories",
    ]
    .iter()
    .any(|name| {
        path.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
    })
}

fn is_protected_target(path: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains("\\windows\\")
        || path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\windowsapps\\")
}
