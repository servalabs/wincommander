// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{
    classify_file, cleanup_hint, format_mtime, progress, risk_for, DiskNode, LargeDiskItem,
    MIN_LARGE_BYTES,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_ENTRIES: u64 = 2_000_000;
const MAX_DEPTH: usize = 128;

pub(super) struct WalkAccum {
    pub children_by_dir: HashMap<String, Vec<DiskNode>>,
    pub large: Vec<LargeDiskItem>,
    pub skipped_entries: u64,
    pub skipped_links: u64,
    pub truncated: bool,
    visited: u64,
    limit: u64,
    publish: bool,
}

impl WalkAccum {
    pub fn new() -> Self {
        Self {
            children_by_dir: HashMap::new(),
            large: Vec::new(),
            skipped_entries: 0,
            skipped_links: 0,
            truncated: false,
            visited: 0,
            limit: MAX_ENTRIES,
            publish: true,
        }
    }

    fn skip(&mut self, link: bool) {
        self.skipped_entries += 1;
        self.skipped_links += u64::from(link);
        if self.publish {
            progress::SKIPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(super) fn walk_dir(
    dir: &Path,
    acc: &mut WalkAccum,
    cancelled: &AtomicBool,
    depth: usize,
    entry_id: u64,
) -> DiskNode {
    #[cfg(windows)]
    {
        return walk_dir_windows(dir, acc, cancelled, depth, entry_id);
    }
    #[cfg(not(windows))]
    {
        walk_dir_portable(dir, acc, cancelled, depth, entry_id)
    }
}

#[cfg(not(windows))]
fn walk_dir_portable(
    dir: &Path,
    acc: &mut WalkAccum,
    cancelled: &AtomicBool,
    depth: usize,
    entry_id: u64,
) -> DiskNode {
    let full_path = dir.to_string_lossy().into_owned();
    acc.children_by_dir.insert(full_path.clone(), Vec::new());
    let mut node = DiskNode {
        entry_id,
        name: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| full_path.clone()),
        full_path: full_path.clone(),
        size: 0,
        allocated: None,
        is_dir: true,
        last_modified: String::new(),
        file_count: 0,
        folder_count: 0,
    };
    if cancelled.load(Ordering::Acquire) {
        return node;
    }
    if depth > MAX_DEPTH {
        acc.truncated = true;
        acc.skip(false);
        return node;
    }
    // Recheck every directory at traversal time; junctions must not turn a tree into a cycle.
    match fs::symlink_metadata(dir) {
        Ok(metadata) if is_link(&metadata) => {
            acc.skip(true);
            return node;
        }
        Ok(metadata) if metadata.is_dir() => {
            node.last_modified = format_mtime(metadata.modified().ok());
        }
        _ => {
            acc.skip(false);
            return node;
        }
    }
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(_) => {
            acc.skip(false);
            return node;
        }
    };
    let mut children = Vec::new();
    for entry in read {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        if acc.visited >= acc.limit {
            acc.truncated = true;
            acc.skip(false);
            break;
        }
        acc.visited += 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                acc.skip(false);
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                acc.skip(false);
                continue;
            }
        };
        if is_link(&metadata) {
            acc.skip(true);
            continue;
        }
        let child = if metadata.is_dir() {
            if acc.publish {
                progress::FOLDERS.fetch_add(1, Ordering::Relaxed);
            }
            let entry_id = acc.visited;
            let child = walk_dir(&path, acc, cancelled, depth + 1, entry_id);
            node.file_count = node.file_count.saturating_add(child.file_count);
            node.folder_count = node
                .folder_count
                .saturating_add(child.folder_count)
                .saturating_add(1);
            child
        } else if metadata.is_file() {
            node.file_count += 1;
            if acc.publish {
                progress::FILES.fetch_add(1, Ordering::Relaxed);
                progress::BYTES.fetch_add(metadata.len(), Ordering::Relaxed);
            }
            DiskNode {
                entry_id: acc.visited,
                name: entry.file_name().to_string_lossy().into_owned(),
                full_path: path.to_string_lossy().into_owned(),
                size: metadata.len(),
                allocated: None,
                is_dir: false,
                last_modified: format_mtime(metadata.modified().ok()),
                file_count: 0,
                folder_count: 0,
            }
        } else {
            acc.skip(false);
            continue;
        };
        node.size = node.size.saturating_add(child.size);
        if !child.is_dir {
            add_large(&child, acc);
        }
        children.push(child);
        if acc.truncated && acc.visited >= acc.limit {
            break;
        }
    }
    children.sort_by_key(|child| std::cmp::Reverse(child.size));
    acc.children_by_dir.insert(full_path, children);
    add_large(&node, acc);
    node
}

#[cfg(windows)]
fn walk_dir_windows(
    dir: &Path,
    acc: &mut WalkAccum,
    cancelled: &AtomicBool,
    depth: usize,
    entry_id: u64,
) -> DiskNode {
    use std::ffi::c_void;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::{
        Foundation::{
            GetLastError, ERROR_FILE_NOT_FOUND, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{
            FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH,
            WIN32_FIND_DATAW,
        },
    };

    struct FindHandle(HANDLE);
    impl Drop for FindHandle {
        fn drop(&mut self) {
            // SAFETY: this wrapper is constructed only with a successful find
            // handle, and owns it until the corresponding FindClose call.
            unsafe {
                FindClose(self.0);
            }
        }
    }

    fn find_first(pattern: &[u16], data: &mut WIN32_FIND_DATAW, large_fetch: bool) -> HANDLE {
        // SAFETY: `pattern` is a live, null-terminated UTF-16 path and `data`
        // is a valid output buffer for the duration of the Windows API call.
        unsafe {
            FindFirstFileExW(
                pattern.as_ptr(),
                FindExInfoBasic,
                data as *mut WIN32_FIND_DATAW as *mut c_void,
                FindExSearchNameMatch,
                std::ptr::null(),
                if large_fetch {
                    FIND_FIRST_EX_LARGE_FETCH
                } else {
                    0
                },
            )
        }
    }

    fn format_find_mtime(data: &WIN32_FIND_DATAW) -> String {
        const WINDOWS_EPOCH_OFFSET_100NS: u64 = 116_444_736_000_000_000;
        let ticks = (u64::from(data.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(data.ftLastWriteTime.dwLowDateTime);
        ticks
            .checked_sub(WINDOWS_EPOCH_OFFSET_100NS)
            .map(|value| (value / 10_000_000).to_string())
            .unwrap_or_default()
    }

    let full_path = dir.to_string_lossy().into_owned();
    acc.children_by_dir.insert(full_path.clone(), Vec::new());
    let mut node = DiskNode {
        entry_id,
        name: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| full_path.clone()),
        full_path: full_path.clone(),
        size: 0,
        allocated: None,
        is_dir: true,
        last_modified: String::new(),
        file_count: 0,
        folder_count: 0,
    };
    if cancelled.load(Ordering::Acquire) {
        return node;
    }
    if depth > MAX_DEPTH {
        acc.truncated = true;
        acc.skip(false);
        return node;
    }

    // Every directory is checked immediately before enumeration. The native
    // find result is used only for children, so a reparse point cannot make a
    // recursive scan leave the selected tree.
    match fs::symlink_metadata(dir) {
        Ok(metadata) if is_link(&metadata) => {
            acc.skip(true);
            return node;
        }
        Ok(metadata) if metadata.is_dir() => {
            node.last_modified = format_mtime(metadata.modified().ok());
        }
        _ => {
            acc.skip(false);
            return node;
        }
    }

    let mut pattern = dir.to_path_buf();
    pattern.push("*");
    let pattern: Vec<u16> = pattern.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut data = WIN32_FIND_DATAW::default();
    let mut handle = find_first(&pattern, &mut data, true);
    if handle == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        if matches!(
            error,
            ERROR_INVALID_FUNCTION | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED
        ) {
            handle = find_first(&pattern, &mut data, false);
        }
    }
    if handle == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        if error != ERROR_FILE_NOT_FOUND {
            acc.skip(false);
        }
        return node;
    }
    let _handle = FindHandle(handle);
    let mut children = Vec::new();

    loop {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let name_length = data
            .cFileName
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(data.cFileName.len());
        let name = std::ffi::OsString::from_wide(&data.cFileName[..name_length]);
        if name != "." && name != ".." {
            if acc.visited >= acc.limit {
                acc.truncated = true;
                acc.skip(false);
                break;
            }
            acc.visited += 1;
            let attributes = data.dwFileAttributes;
            if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                acc.skip(true);
            } else {
                let path = dir.join(&name);
                let child = if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                    if acc.publish {
                        progress::FOLDERS.fetch_add(1, Ordering::Relaxed);
                    }
                    let child = walk_dir_windows(&path, acc, cancelled, depth + 1, acc.visited);
                    node.file_count = node.file_count.saturating_add(child.file_count);
                    node.folder_count = node
                        .folder_count
                        .saturating_add(child.folder_count)
                        .saturating_add(1);
                    child
                } else {
                    let size = (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow);
                    node.file_count += 1;
                    if acc.publish {
                        progress::FILES.fetch_add(1, Ordering::Relaxed);
                        progress::BYTES.fetch_add(size, Ordering::Relaxed);
                    }
                    DiskNode {
                        entry_id: acc.visited,
                        name: name.to_string_lossy().into_owned(),
                        full_path: path.to_string_lossy().into_owned(),
                        size,
                        allocated: None,
                        is_dir: false,
                        last_modified: format_find_mtime(&data),
                        file_count: 0,
                        folder_count: 0,
                    }
                };
                node.size = node.size.saturating_add(child.size);
                if !child.is_dir {
                    add_large(&child, acc);
                }
                children.push(child);
            }
        }

        if unsafe { FindNextFileW(handle, &mut data) } == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_NO_MORE_FILES {
                acc.skip(false);
            }
            break;
        }
    }
    children.sort_by_key(|child| std::cmp::Reverse(child.size));
    acc.children_by_dir.insert(full_path, children);
    add_large(&node, acc);
    node
}

fn add_large(node: &DiskNode, acc: &mut WalkAccum) {
    if node.size < MIN_LARGE_BYTES {
        return;
    }
    acc.large.push(LargeDiskItem {
        entry_id: node.entry_id,
        name: node.name.clone(),
        full_path: node.full_path.clone(),
        size: node.size,
        allocated: None,
        is_dir: node.is_dir,
        last_modified: node.last_modified.clone(),
        file_count: node.file_count,
        folder_count: node.folder_count,
        item_type: if node.is_dir {
            "Folder".into()
        } else {
            classify_file(&node.name)
        },
        cleanup_hint: cleanup_hint(&node.name, &node.full_path, node.is_dir),
        risk: risk_for(&node.full_path),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(path: &Path, limit: u64, cancelled: bool) -> (DiskNode, WalkAccum) {
        let mut acc = WalkAccum::new();
        acc.publish = false;
        acc.limit = limit;
        let node = walk_dir(path, &mut acc, &AtomicBool::new(cancelled), 0, 0);
        (node, acc)
    }

    #[test]
    fn counts_nested_files_once_and_never_claims_allocation() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("nested")).unwrap();
        fs::write(temp.path().join("first"), b"1234").unwrap();
        fs::write(temp.path().join("nested/second"), b"56789").unwrap();
        let (root, acc) = scan(temp.path(), MAX_ENTRIES, false);
        assert_eq!((root.size, root.file_count, root.folder_count), (9, 2, 1));
        assert_eq!(root.allocated, None);
        assert_eq!(acc.skipped_entries, 0);
        assert!(!acc.truncated);
    }

    #[cfg(windows)]
    #[test]
    fn native_enumerator_reports_file_size_and_last_modified() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("example.bin"), b"12345").unwrap();

        let (root, acc) = scan(temp.path(), MAX_ENTRIES, false);
        let item = acc.children_by_dir[&root.full_path][0].clone();
        assert_eq!((root.file_count, root.size, item.size), (1, 5, 5));
        assert!(!item.last_modified.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn native_enumerator_does_not_mark_an_empty_directory_partial_at_the_entry_limit() {
        let temp = tempfile::tempdir().unwrap();
        let (root, acc) = scan(temp.path(), 0, false);
        assert_eq!((root.file_count, root.folder_count, root.size), (0, 0, 0));
        assert_eq!(acc.skipped_entries, 0);
        assert!(!acc.truncated);
    }

    #[test]
    fn every_scanned_entry_has_a_distinct_backend_id_including_nested_folders() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("nested")).unwrap();
        fs::write(temp.path().join("first"), b"1234").unwrap();
        fs::write(temp.path().join("nested/second"), b"56789").unwrap();
        let (root, acc) = scan(temp.path(), MAX_ENTRIES, false);
        let ids: Vec<_> = acc
            .children_by_dir
            .values()
            .flatten()
            .map(|item| item.entry_id)
            .collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(root.entry_id, 0);
        assert_eq!(ids.len(), 3);
        assert_eq!(unique.len(), 3);
        assert!(!ids.contains(&0));
    }

    #[test]
    fn reports_missing_directories_as_incomplete() {
        let temp = tempfile::tempdir().unwrap();
        let (root, acc) = scan(&temp.path().join("missing"), MAX_ENTRIES, false);
        assert_eq!(root.size, 0);
        assert_eq!(acc.skipped_entries, 1);
    }

    #[test]
    fn reports_entry_limit_and_cancellation_without_complete_totals() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("first"), b"1234").unwrap();
        let (root, acc) = scan(temp.path(), 0, false);
        assert_eq!(root.size, 0);
        assert!(acc.truncated);
        assert_eq!(acc.skipped_entries, 1);
        let (root, acc) = scan(temp.path(), MAX_ENTRIES, true);
        assert_eq!(root.file_count, 0);
        assert_eq!(acc.children_by_dir.len(), 1);
        assert!(acc.children_by_dir.values().all(Vec::is_empty));
    }

    #[cfg(windows)]
    #[test]
    fn skips_junction_cycles_and_does_not_count_the_target_twice() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("first"), b"1234").unwrap();
        let link = temp.path().join("cycle");
        let status = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(status.status.success());
        let (root, acc) = scan(temp.path(), MAX_ENTRIES, false);
        assert_eq!((root.size, root.file_count), (4, 1));
        assert_eq!((acc.skipped_entries, acc.skipped_links), (1, 1));
        fs::remove_dir(link).unwrap();
    }
}
