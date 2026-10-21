// SPDX-License-Identifier: AGPL-3.0-or-later

use super::rules::{validate_recursive_containment, ScanTarget, TargetOperation};
use super::{CachedCleanerItem, CachedFile, RoutineCleanerItem, RoutineCleanerScan};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

const RECENT_FILE_AGE: Duration = Duration::from_secs(60 * 60);
const MAX_FILES_PER_TARGET: usize = 50_000;
const MAX_ENTRIES_PER_TARGET: usize = 200_000;

pub(super) fn scan_targets(
    targets: Vec<ScanTarget>,
    cancelled: &AtomicBool,
) -> Result<(RoutineCleanerScan, Vec<CachedCleanerItem>), String> {
    let mut public_items = Vec::new();
    let mut cached_items = Vec::new();
    let mut skipped_targets = 0;
    let mut seen = HashSet::new();
    for target in targets {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        match scan_target(target, cancelled) {
            Ok(Some((mut public, mut cached))) => {
                if cached.operation == TargetOperation::Delete {
                    cached.files.retain(|file| seen.insert(file.path.clone()));
                    public.file_count = cached.files.len();
                    public.bytes = cached.files.iter().map(|file| file.bytes).sum();
                    if cached.files.is_empty() {
                        continue;
                    }
                }
                public_items.push(public);
                cached_items.push(cached);
            }
            Ok(None) | Err(_) => skipped_targets += 1,
        }
    }
    public_items.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| right.bytes.cmp(&left.bytes))
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.path.cmp(&right.path))
    });
    let total_bytes = public_items.iter().map(|item| item.bytes).sum();
    let total_files = public_items.iter().map(|item| item.file_count).sum();
    Ok((
        RoutineCleanerScan {
            scan_root: None,
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
    if let (Some(source), Some(containment_root)) =
        (&target.containment_source, &target.containment_root)
    {
        if !validate_recursive_containment(source, containment_root) {
            return Ok(None);
        }
    }
    let canonical_root = fs::canonicalize(&target.path).map_err(|error| error.to_string())?;
    if target.category == "projects"
        && !validate_recursive_containment(&target.path, &canonical_root)
    {
        return Ok(None);
    }
    if let Some(containment_root) = &target.containment_root {
        if canonical_root == *containment_root || !canonical_root.starts_with(containment_root) {
            return Ok(None);
        }
    }
    let (files, bytes, truncated) = match target.operation {
        TargetOperation::Delete => collect_deletable_files(
            &canonical_root,
            target.minimum_age,
            cancelled,
            MAX_ENTRIES_PER_TARGET,
        ),
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
        requires_plain_path: target.category == "projects",
        files,
        operation: target.operation,
    };
    Ok(Some((public, cached)))
}

fn collect_deletable_files(
    root: &Path,
    minimum_age: Duration,
    cancelled: &AtomicBool,
    entry_limit: usize,
) -> (Vec<CachedFile>, u64, bool) {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let now = SystemTime::now();
    let mut bytes = 0u64;
    let mut truncated = false;
    let mut partial = false;
    let mut entries_scanned = 0;
    while let Some(directory) = stack.pop() {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let Ok(directory_metadata) = fs::symlink_metadata(&directory) else {
            partial = true;
            continue;
        };
        if !directory_metadata.is_dir()
            || directory_metadata.file_type().is_symlink()
            || is_reparse_point(&directory_metadata)
        {
            partial = true;
            continue;
        }
        let Ok(canonical_directory) = fs::canonicalize(&directory) else {
            partial = true;
            continue;
        };
        if !canonical_directory.starts_with(root) {
            partial = true;
            continue;
        }
        let Ok(entries) = fs::read_dir(&canonical_directory) else {
            partial = true;
            continue;
        };
        for entry in entries {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    partial = true;
                    continue;
                }
            };
            if files.len() >= MAX_FILES_PER_TARGET || entries_scanned >= entry_limit {
                truncated = true;
                break;
            }
            entries_scanned += 1;
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                partial = true;
                continue;
            };
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                partial = true;
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file()
                && is_old_enough(metadata.modified().ok(), now, minimum_age)
            {
                let identity = file_identity(&path);
                if cfg!(windows) && identity.is_none() {
                    partial = true;
                    continue;
                }
                bytes = bytes.saturating_add(metadata.len());
                files.push(CachedFile {
                    file_identity: identity,
                    path,
                    bytes: metadata.len(),
                    minimum_age,
                });
            }
        }
        if truncated {
            break;
        }
    }
    (files, bytes, truncated || partial)
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
            minimum_age: Duration::ZERO,
            file_identity: None,
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

pub(super) fn is_old_enough(
    modified: Option<SystemTime>,
    now: SystemTime,
    minimum_age: Duration,
) -> bool {
    modified
        .and_then(|time| now.duration_since(time).ok())
        .is_some_and(|age| age >= minimum_age.max(RECENT_FILE_AGE))
}

#[cfg(windows)]
pub(crate) fn file_identity(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return None;
    }
    let index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Some((information.dwVolumeSerialNumber, index))
}

#[cfg(not(windows))]
pub(crate) fn file_identity(_path: &Path) -> Option<(u32, u64)> {
    None
}

#[cfg(windows)]
pub(super) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
pub(super) fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
#[path = "scanner_tests.rs"]
mod tests;
