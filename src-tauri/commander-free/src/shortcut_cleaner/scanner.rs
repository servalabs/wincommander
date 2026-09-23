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
                // A question mark cannot appear in a valid Windows file name.
                // If the link contains one (or a Unicode replacement marker),
                // its path was decoded lossily; omit it instead of showing or
                // acting on a misleading target.
                if has_unresolved_path_characters(&target) {
                    continue;
                }
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
    lnk_local_target_from_bytes(&data)
}

fn lnk_local_target_from_bytes(data: &[u8]) -> Option<PathBuf> {
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
    let unicode_suffix_offset = if header_size >= 0x24 {
        read_u32(&data, offset + 0x20)? as usize
    } else {
        0
    };
    let base_offset = read_u32(&data, offset + 0x10)? as usize;
    let suffix_offset = read_u32(&data, offset + 0x18)? as usize;
    let base = read_lnk_string(&data, offset, info_size, unicode_base_offset, true)
        .filter(|value| !value.is_empty())
        .or_else(|| read_lnk_string(&data, offset, info_size, base_offset, false))?;
    let suffix = read_lnk_string(&data, offset, info_size, unicode_suffix_offset, true)
        .filter(|value| !value.is_empty())
        .or_else(|| read_lnk_string(&data, offset, info_size, suffix_offset, false))
        .unwrap_or_default();
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
        let (pairs, _) = slice.as_chunks::<2>();
        let units: Vec<u16> = pairs
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|c| *c != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    } else {
        let bytes = slice.split(|byte| *byte == 0).next()?;
        Some(decode_ansi_path(bytes))
    }
}

fn decode_ansi_path(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        // The LinkInfo ANSI field follows Windows' active code page; UTF-8 is only a last-resort display fallback if conversion fails.
        return decode_windows_ansi(bytes)
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned());
    }
    #[cfg(not(windows))]
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(windows)]
fn decode_windows_ansi(bytes: &[u8]) -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetACP"]
        fn get_acp() -> u32;
    }

    let code_page = unsafe { get_acp() };
    decode_windows_ansi_with_code_page(code_page, bytes)
}

#[cfg(windows)]
fn decode_windows_ansi_with_code_page(code_page: u32, bytes: &[u8]) -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "MultiByteToWideChar"]
        fn multi_byte_to_wide_char(
            code_page: u32,
            flags: u32,
            input: *const u8,
            input_len: i32,
            output: *mut u16,
            output_len: i32,
        ) -> i32;
    }

    let input_len = i32::try_from(bytes.len()).ok()?;
    if input_len == 0 {
        return Some(String::new());
    }
    let required = unsafe {
        multi_byte_to_wide_char(
            code_page,
            0,
            bytes.as_ptr(),
            input_len,
            std::ptr::null_mut(),
            0,
        )
    };
    if required <= 0 {
        return None;
    }
    let mut wide = vec![0u16; required as usize];
    let written = unsafe {
        multi_byte_to_wide_char(
            code_page,
            0,
            bytes.as_ptr(),
            input_len,
            wide.as_mut_ptr(),
            required,
        )
    };
    if written <= 0 {
        return None;
    }
    wide.truncate(written as usize);
    String::from_utf16(&wide).ok()
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

fn has_unresolved_path_characters(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.contains('?') || path.contains('\u{fffd}')
}

#[cfg(test)]
mod tests {
    use super::{has_unresolved_path_characters, lnk_local_target_from_bytes, read_lnk_string};
    use std::path::Path;

    #[test]
    fn reads_unicode_link_paths_without_corrupting_non_ascii_characters() {
        let path = r"C:\Users\Zoë\Desktop\missing.exe";
        let mut data = vec![0, 0];
        data.extend(path.encode_utf16().flat_map(u16::to_le_bytes));
        data.extend([0, 0]);

        assert_eq!(
            read_lnk_string(&data, 0, data.len(), 2, true).as_deref(),
            Some(path)
        );
    }

    #[test]
    fn reads_unicode_common_path_suffix_without_question_marks() {
        let suffix = r"Program Files\Café\missing.exe";
        let mut data = vec![0, 0];
        data.extend(suffix.encode_utf16().flat_map(u16::to_le_bytes));
        data.extend([0, 0]);

        assert_eq!(
            read_lnk_string(&data, 0, data.len(), 2, true).as_deref(),
            Some(suffix)
        );
    }

    #[test]
    fn parses_unicode_base_and_suffix_from_link_info() {
        let mut data = vec![0; 0x4c];
        set_u32(&mut data, 0, 0x4c);
        set_u32(&mut data, 0x14, 0x2);

        let info_start = data.len();
        let header_size = 0x24;
        data.resize(info_start + header_size, 0);
        set_u32(&mut data, info_start + 4, header_size as u32);

        let base_ansi_offset = append_ansi(&mut data, b"C:\\Users\\??");
        let suffix_ansi_offset = append_ansi(&mut data, b"Desktop\\??.exe");
        let base_unicode_offset = append_unicode(&mut data, r"C:\Users\Zoë");
        let suffix_unicode_offset = append_unicode(&mut data, r"Desktop\Café.exe");

        let info_size = (data.len() - info_start) as u32;
        set_u32(&mut data, info_start, info_size);
        set_u32(&mut data, info_start + 0x10, base_ansi_offset);
        set_u32(&mut data, info_start + 0x18, suffix_ansi_offset);
        set_u32(&mut data, info_start + 0x1c, base_unicode_offset);
        set_u32(&mut data, info_start + 0x20, suffix_unicode_offset);

        let target = lnk_local_target_from_bytes(&data).unwrap();
        assert_eq!(
            target.to_string_lossy().replace('/', "\\"),
            r"C:\Users\Zoë\Desktop\Café.exe"
        );
    }

    #[test]
    fn rejects_a_lossily_decoded_missing_target_path() {
        assert!(has_unresolved_path_characters(Path::new(
            r"C:\Users\User\??\missing.exe"
        )));
        assert!(has_unresolved_path_characters(Path::new(
            "C:\\Users\\User\\bad\u{fffd}name.exe"
        )));
        assert!(!has_unresolved_path_characters(Path::new(
            r"C:\Users\Zoë\missing.exe"
        )));
    }

    fn append_ansi(data: &mut Vec<u8>, value: &[u8]) -> u32 {
        let offset = (data.len() - 0x4c) as u32;
        data.extend(value);
        data.push(0);
        offset
    }

    fn append_unicode(data: &mut Vec<u8>, value: &str) -> u32 {
        let offset = (data.len() - 0x4c) as u32;
        data.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
        data.extend([0, 0]);
        offset
    }

    fn set_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[cfg(windows)]
    #[test]
    fn decodes_ansi_link_paths_with_a_windows_legacy_code_page() {
        let path = b"C:\\Program Files\\caf\xe9.exe";
        assert_eq!(
            super::decode_windows_ansi_with_code_page(1252, path).as_deref(),
            Some("C:\\Program Files\\café.exe")
        );
    }

    #[cfg(windows)]
    #[test]
    fn active_code_page_decoder_preserves_ascii_shortcut_paths() {
        assert_eq!(
            super::decode_windows_ansi(b"C:\\Program Files\\Example\\app.exe").as_deref(),
            Some("C:\\Program Files\\Example\\app.exe")
        );
    }
}
