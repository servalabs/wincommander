// SPDX-License-Identifier: AGPL-3.0-or-later

use super::rules::{ScanTarget, TargetOperation};
use super::{CachedCleanerItem, CachedFile, RoutineCleanerItem, RoutineCleanerScan};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

const RECENT_FILE_AGE: Duration = Duration::from_secs(60 * 60);
const MAX_FILES_PER_TARGET: usize = 50_000;

pub(super) fn scan_targets(
    targets: Vec<ScanTarget>,
    cancelled: &AtomicBool,
) -> Result<(RoutineCleanerScan, Vec<CachedCleanerItem>), String> {
    let mut public_items = Vec::new();
    let mut cached_items = Vec::new();
    let mut skipped_targets = 0;
    for target in targets {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        match scan_target(target, cancelled) {
            Ok(Some((public, cached))) => {
                public_items.push(public);
                cached_items.push(cached);
            }
            Ok(None) | Err(_) => skipped_targets += 1,
        }
    }
    let total_bytes = public_items.iter().map(|item| item.bytes).sum();
    let total_files = public_items.iter().map(|item| item.file_count).sum();
    Ok((
        RoutineCleanerScan {
            items: public_items,
            total_bytes,
            total_files,
            skipped_targets,
            cancelled: cancelled.load(Ordering::Acquire),
        },
        cached_items,
    ))
}

fn scan_target(
    target: ScanTarget,
    cancelled: &AtomicBool,
) -> Result<Option<(RoutineCleanerItem, CachedCleanerItem)>, String> {
    if !target.path.is_absolute() || !target.path.exists() {
        return Ok(None);
    }
    let metadata = fs::symlink_metadata(&target.path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Ok(None);
    }
    let canonical_root = fs::canonicalize(&target.path).map_err(|error| error.to_string())?;
    let (files, bytes, truncated) = match target.operation {
        TargetOperation::Delete => collect_deletable_files(&canonical_root, cancelled),
        TargetOperation::Vacuum => scan_database(&canonical_root)?,
    };
    if files.is_empty() || bytes == 0 {
        return Ok(None);
    }
    let id = Uuid::new_v4().to_string();
    let operation = match target.operation {
        TargetOperation::Delete => "delete",
        TargetOperation::Vacuum => "vacuum",
    };
    let public = RoutineCleanerItem {
        id: id.clone(),
        category: target.category.clone(),
        label: target.label.clone(),
        path: target.path.to_string_lossy().into_owned(),
        bytes,
        file_count: files.len(),
        recommended: target.recommended,
        operation: operation.into(),
        truncated,
    };
    let cached = CachedCleanerItem {
        id,
        label: target.label,
        root: target.path,
        canonical_root,
        files,
        operation: target.operation,
    };
    Ok(Some((public, cached)))
}

fn collect_deletable_files(root: &Path, cancelled: &AtomicBool) -> (Vec<CachedFile>, u64, bool) {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let now = SystemTime::now();
    let mut bytes = 0u64;
    let mut truncated = false;
    while let Some(directory) = stack.pop() {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if files.len() >= MAX_FILES_PER_TARGET {
                truncated = true;
                break;
            }
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() && is_old_enough(metadata.modified().ok(), now) {
                bytes = bytes.saturating_add(metadata.len());
                files.push(CachedFile {
                    path,
                    bytes: metadata.len(),
                });
            }
        }
        if truncated {
            break;
        }
    }
    (files, bytes, truncated)
}

fn scan_database(path: &Path) -> Result<(Vec<CachedFile>, u64, bool), String> {
    if !path.is_file() || !has_sqlite_header(path) {
        return Ok((Vec::new(), 0, false));
    }
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    let wal_size = fs::metadata(format!("{}-wal", path.to_string_lossy()))
        .map_or(0, |metadata| metadata.len());
    let estimated = wal_size.saturating_add(size / 10);
    Ok((
        vec![CachedFile {
            path: path.to_path_buf(),
            bytes: estimated,
        }],
        estimated,
        false,
    ))
}

pub(super) fn has_sqlite_header(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0u8; 16];
    file.read_exact(&mut header).is_ok() && &header == b"SQLite format 3\0"
}

fn is_old_enough(modified: Option<SystemTime>, now: SystemTime) -> bool {
    modified
        .and_then(|time| now.duration_since(time).ok())
        .is_none_or(|age| age >= RECENT_FILE_AGE)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_sqlite_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        fs::write(&path, b"SQLite format 3\0rest").unwrap();
        assert!(has_sqlite_header(&path));
    }
    #[test]
    fn rejects_non_sqlite_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        fs::write(&path, b"not a database").unwrap();
        assert!(!has_sqlite_header(&path));
    }
}
