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
