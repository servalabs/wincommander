// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/downloads_watcher.rs
//
// ═══════════════════════════════════════════════════════════════════════
// RECENT DOWNLOADS — live view of the user's Downloads folder
// ═══════════════════════════════════════════════════════════════════════
//
// Watches %USERPROFILE%\Downloads (non-recursive) via the shared
// fs_watcher service and keeps an in-memory ring buffer of the most
// recent files. The Dashboard's RecentDownloadsCard reads the buffer
// once via `get_recent_downloads` and then live-updates from the
// `downloads://changed` event.
//
// In-memory only — nothing is persisted. The buffer is rebuilt from an
// initial directory scan on every app start, so a fresh launch always
// shows the current Downloads contents (newest first).
//
// Partial-download artifacts (.crdownload / .part / .tmp / .download)
// are filtered out so the list only shows completed files.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const MAX_ENTRIES: usize = 50;
const PARTIAL_EXTS: &[&str] = &["crdownload", "part", "tmp", "download", "partial"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadEntry {
    /// File name (no directory).
    pub name: String,
    /// Absolute path — used by the card to open the containing folder.
    pub path: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Last-modified time, epoch seconds. The card renders this as a
    /// relative "x ago" label.
    pub modified_at: i64,
}

static RECENT: Lazy<Mutex<VecDeque<DownloadEntry>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)));

/// Resolve the current user's Downloads directory via the Windows Known
/// Folder API. Crucially this honours a *relocated* Downloads folder — a
/// user who moved Downloads to another drive (Properties → Location) has a
/// path that is NOT `%USERPROFILE%\Downloads`, so the old env-join missed
/// their files entirely. Falls back to `%USERPROFILE%\Downloads` only if the
/// API fails.
#[cfg(windows)]
fn downloads_dir() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath};

    unsafe {
        let mut path_ptr: *mut u16 = std::ptr::null_mut();
        // dwFlags = 0 (default), hToken = null (current user).
        let hr = SHGetKnownFolderPath(
            &FOLDERID_Downloads as *const _,
            0,
            std::ptr::null_mut(),
            &mut path_ptr,
        );
        if hr == 0 && !path_ptr.is_null() {
            // The returned buffer is a NUL-terminated wide string.
            let mut len = 0usize;
            while *path_ptr.add(len) != 0 {
                len += 1;
            }
            let os = OsString::from_wide(std::slice::from_raw_parts(path_ptr, len));
            CoTaskMemFree(path_ptr as *const core::ffi::c_void);
            let p = PathBuf::from(os);
            if !p.as_os_str().is_empty() {
                return Some(p);
            }
        } else if !path_ptr.is_null() {
            // SHGetKnownFolderPath allocates even on some failure paths.
            CoTaskMemFree(path_ptr as *const core::ffi::c_void);
        }
    }

    // Fallback: the classic profile-relative path.
    std::env::var("USERPROFILE")
        .ok()
        .map(|p| PathBuf::from(p).join("Downloads"))
}

/// Non-Windows fallback (dev machines) — $HOME/Downloads.
#[cfg(not(windows))]
fn downloads_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join("Downloads"))
}

fn is_partial(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => PARTIAL_EXTS.iter().any(|p| ext.eq_ignore_ascii_case(p)),
        None => false,
    }
}

/// Build a DownloadEntry from a path by reading its metadata. Returns None
/// for directories, partial-download artifacts, or unreadable files.
fn entry_for(path: &Path) -> Option<DownloadEntry> {
    if is_partial(path) {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().to_string();
    // Hidden / temp dotfiles are noise.
    if name.starts_with('.') || name.starts_with('~') {
        return None;
    }
    let modified_at = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some(DownloadEntry {
        name,
        path: path.to_string_lossy().to_string(),
        size_bytes: meta.len(),
        modified_at,
    })
}

/// Insert or refresh an entry, keeping the buffer newest-first and capped.
fn upsert(entry: DownloadEntry) {
    let mut buf = RECENT.lock().unwrap();
    // Drop any existing entry for the same path so a file that's modified
    // several times during a download doesn't appear twice.
    buf.retain(|e| e.path != entry.path);
    let insert_at = buf
        .iter()
        .position(|existing| existing.modified_at < entry.modified_at)
        .unwrap_or(buf.len());
    buf.insert(insert_at, entry);
    while buf.len() > MAX_ENTRIES {
        buf.pop_back();
    }
}

fn remove_path(path: &Path) {
    let target = path.to_string_lossy().to_string();
    let mut buf = RECENT.lock().unwrap();
    buf.retain(|e| e.path != target);
}

/// Select the newest entries without sorting the whole Downloads directory.
fn newest_entries(dir: &Path) -> Vec<DownloadEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut newest = Vec::with_capacity(MAX_ENTRIES);
    for entry in read.flatten().filter_map(|entry| entry_for(&entry.path())) {
        retain_newest(&mut newest, entry);
    }
    newest.sort_by_key(|entry| std::cmp::Reverse(entry.modified_at));
    newest
}

fn retain_newest(entries: &mut Vec<DownloadEntry>, entry: DownloadEntry) {
    if entries.len() < MAX_ENTRIES {
        entries.push(entry);
        return;
    }
    let (oldest_index, oldest_modified) = entries
        .iter()
        .enumerate()
        .min_by_key(|(_, entry)| entry.modified_at)
        .map(|(index, entry)| (index, entry.modified_at))
        .expect("non-empty bounded Downloads selection");
    if entry.modified_at > oldest_modified {
        entries[oldest_index] = entry;
    }
}

fn merge_initial_scan(dir: &Path) {
    for entry in newest_entries(dir) {
        upsert(entry);
    }
}

/// Snapshot of the current buffer (newest-first). Free-tier command.
#[tauri::command]
pub fn get_recent_downloads() -> Vec<DownloadEntry> {
    RECENT.lock().unwrap().iter().cloned().collect()
}

/// Start watching the Downloads folder. Call once from `run()`'s setup hook.
/// Best-effort: if the folder is missing or the watcher can't attach, the
/// card simply shows whatever the initial scan found (possibly nothing).
pub fn init(app: &AppHandle) {
    let Some(dir) = downloads_dir() else {
        crate::log_message(
            "warn",
            "[Downloads] no Downloads dir resolved — watcher idle",
        );
        return;
    };

    let app = app.clone();
    let mut handle = match crate::services::fs_watcher::subscribe(dir.clone(), false) {
        Ok(handle) => handle,
        Err(e) => {
            crate::log_message(
                "warn",
                &format!(
                    "[Downloads] fs_watcher subscribe {} failed: {}",
                    dir.display(),
                    e
                ),
            );
            return;
        }
    };
    crate::log_message("debug", "[Downloads] watcher started");
    crate::startup_trace::job_started(&app, "downloads.initial-scan");

    let scan_app = app.clone();
    std::thread::spawn(move || {
        merge_initial_scan(&dir);
        crate::startup_trace::job_finished(&scan_app, "downloads.initial-scan", true);
        let _ = scan_app.emit("downloads://changed", ());
    });

    tauri::async_runtime::spawn(async move {
        use notify::EventKind;
        while let Some(event) = handle.rx.recv().await {
            let mut changed = false;
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    for path in &event.paths {
                        if let Some(entry) = entry_for(path) {
                            upsert(entry);
                            changed = true;
                        }
                    }
                }
                EventKind::Remove(_) => {
                    for path in &event.paths {
                        remove_path(path);
                        changed = true;
                    }
                }
                _ => {}
            }
            if changed {
                let _ = app.emit("downloads://changed", ());
            }
        }
        crate::log_message("debug", "[Downloads] watcher stopped");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    static RECENT_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn entry(name: &str, modified_at: i64) -> DownloadEntry {
        DownloadEntry {
            name: name.to_string(),
            path: name.to_string(),
            size_bytes: 0,
            modified_at,
        }
    }

    #[test]
    fn updates_keep_recent_downloads_newest_first() {
        let _lock = RECENT_TEST_LOCK.lock().unwrap();
        RECENT.lock().unwrap().clear();
        upsert(entry("older", 10));
        upsert(entry("newer", 20));
        upsert(entry("middle", 15));

        let names: Vec<String> = get_recent_downloads()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["newer", "middle", "older"]);
        RECENT.lock().unwrap().clear();
    }

    #[test]
    fn refresh_replaces_an_existing_path_without_duplication() {
        let _lock = RECENT_TEST_LOCK.lock().unwrap();
        RECENT.lock().unwrap().clear();
        upsert(entry("same", 10));
        upsert(entry("same", 20));

        let downloads = get_recent_downloads();
        assert_eq!(downloads.len(), 1);
        assert_eq!(downloads[0].modified_at, 20);
        RECENT.lock().unwrap().clear();
    }

    #[test]
    fn initial_selection_retains_only_the_newest_fifty() {
        let mut selected = Vec::new();
        for modified_at in 0..=MAX_ENTRIES as i64 {
            retain_newest(&mut selected, entry(&modified_at.to_string(), modified_at));
        }
        selected.sort_by_key(|entry| std::cmp::Reverse(entry.modified_at));

        assert_eq!(selected.len(), MAX_ENTRIES);
        assert_eq!(selected.first().unwrap().modified_at, MAX_ENTRIES as i64);
        assert_eq!(selected.last().unwrap().modified_at, 1);
    }
}
