// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateProgress {
    pub scan_id: String,
    pub running: bool,
    pub phase: String,
    pub scope_roots: Vec<String>,
    pub excluded_volumes: Vec<String>,
    pub current_path: String,
    pub scanned_files: usize,
    pub hashed_files: usize,
    pub candidate_files: usize,
    pub bytes_hashed: u64,
    pub skipped_entries: usize,
    pub skipped_links: usize,
    pub errors: Vec<String>,
}
static PROGRESS: OnceLock<Mutex<DuplicateProgress>> = OnceLock::new();
fn progress() -> &'static Mutex<DuplicateProgress> {
    PROGRESS.get_or_init(|| Mutex::new(DuplicateProgress::default()))
}
pub(super) fn update(change: impl FnOnce(&mut DuplicateProgress)) {
    if let Ok(mut guard) = progress().lock() {
        change(&mut guard);
    }
}
pub(super) fn snapshot() -> DuplicateProgress {
    progress()
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Returns an ordinary Explorer-readable path for progress UI. Canonicalized
/// Windows paths use the extended `\\?\` namespace internally, which is not a
/// useful location to show a person or paste into Explorer.
pub(super) fn display_path(path: &Path) -> Option<String> {
    let raw = path.to_string_lossy().replace('/', "\\");
    let display = if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() < 3 || bytes[1] != b':' || bytes[2] != b'\\' {
            return None;
        }
        rest.to_owned()
    } else {
        raw
    };
    (!display.contains('?') && !display.contains('\u{FFFD}')).then_some(display)
}

pub(super) fn begin(roots: &[PathBuf], excluded: Vec<String>) {
    update(|state| {
        *state = DuplicateProgress {
            scan_id: uuid::Uuid::new_v4().to_string(),
            running: true,
            phase: "enumerating".into(),
            scope_roots: roots
                .iter()
                .map(|path| {
                    display_path(path).unwrap_or_else(|| "Unavailable scan location".into())
                })
                .collect(),
            excluded_volumes: excluded,
            ..Default::default()
        }
    });
}
pub(super) fn error(path: &Path, error: impl std::fmt::Display) {
    update(|state| {
        state.skipped_entries += 1;
        if state.errors.len() < 20 {
            let path = display_path(path).unwrap_or_else(|| "Unavailable scan location".into());
            state.errors.push(format!("{path}: {error}"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::display_path;
    use std::path::Path;

    #[test]
    fn displays_extended_drive_paths_as_explorer_paths() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Program Files")),
            Some(r"C:\Program Files".into())
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\folder")),
            Some(r"\\server\share\folder".into())
        );
        assert_eq!(display_path(Path::new(r"\\?\Volume{test}")), None);
    }
}
pub(super) fn finish(cancelled: bool) {
    update(|state| {
        state.running = false;
        state.phase = if cancelled { "cancelled" } else { "ready" }.into();
    });
}
pub(super) fn fail(error: &str) {
    update(|state| {
        state.running = false;
        state.phase = "failed".into();
        state.errors.push(error.to_string());
    });
}

#[cfg(windows)]
pub(super) fn local_fixed_roots() -> Result<(Vec<PathBuf>, Vec<String>), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut roots = Vec::new();
    let mut excluded = Vec::new();
    for bit in 0u32..26 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + bit as u8) as char);
        let wide: Vec<u16> = std::ffi::OsStr::new(&root)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
        if kind == 3 {
            match std::fs::canonicalize(&root) {
                Ok(path) => roots.push(path),
                Err(error) => excluded.push(format!("{root} Fixed drive unavailable: {error}")),
            }
        } else {
            let label = match kind {
                2 => "Removable — choose a folder to opt in",
                4 => "Network — choose a folder to opt in",
                5 => "Optical — choose a folder to opt in",
                _ => "Unavailable or unsupported drive",
            };
            excluded.push(format!("{root} {label}"));
        }
    }
    if roots.is_empty() {
        return Err(
            "No eligible accessible local fixed drives were found. Choose a folder instead.".into(),
        );
    }
    Ok((roots, excluded))
}
#[cfg(not(windows))]
pub(super) fn local_fixed_roots() -> Result<(Vec<PathBuf>, Vec<String>), String> {
    Err("Automatic fixed-drive discovery is available on Windows. Choose folders instead.".into())
}
