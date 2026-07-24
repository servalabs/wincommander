// SPDX-License-Identifier: AGPL-3.0-or-later

use super::rules::TargetOperation;
use super::scanner::has_sqlite_header;
use super::{CachedCleanerItem, RoutineCleanerCleanResult, RoutineCleanerError};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(super) fn clean_items(
    items: Vec<CachedCleanerItem>,
    cancelled: &AtomicBool,
) -> (RoutineCleanerCleanResult, Vec<String>) {
    let mut result = RoutineCleanerCleanResult {
        bytes_recovered: 0,
        files_cleaned: 0,
        items_cleaned: 0,
        errors: Vec::new(),
        cancelled: false,
    };
    let mut cleaned_ids = Vec::new();
    for item in items {
        if cancelled.load(Ordering::Acquire) {
            result.cancelled = true;
            break;
        }
        match item.operation {
            TargetOperation::Delete => {
                let (files, bytes, errors) = clean_file_group(&item, cancelled);
                result.files_cleaned += files;
                result.bytes_recovered = result.bytes_recovered.saturating_add(bytes);
                if errors.is_empty() {
                    result.items_cleaned += 1;
                    cleaned_ids.push(item.id);
                } else {
                    result.errors.push(RoutineCleanerError {
                        id: item.id,
                        label: item.label,
                        reason: errors.join("; "),
                    });
                }
            }
            TargetOperation::Vacuum => match vacuum_database(&item) {
                Ok((files, bytes)) => {
                    result.files_cleaned += files;
                    result.bytes_recovered = result.bytes_recovered.saturating_add(bytes);
                    result.items_cleaned += 1;
                    cleaned_ids.push(item.id);
                }
                Err(reason) => result.errors.push(RoutineCleanerError {
                    id: item.id,
                    label: item.label,
                    reason,
                }),
            },
        }
    }
    (result, cleaned_ids)
}

fn clean_file_group(item: &CachedCleanerItem, cancelled: &AtomicBool) -> (usize, u64, Vec<String>) {
    let mut files_cleaned = 0;
    let mut bytes = 0u64;
    let mut errors = Vec::new();
    for file in &item.files {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        if !file.path.exists() {
            continue;
        }
        let canonical = match fs::canonicalize(&file.path) {
            Ok(path) => path,
            Err(error) => {
                errors.push(format!("cannot resolve cached file: {error}"));
                continue;
            }
        };
        if !canonical.starts_with(&item.canonical_root) || canonical == item.canonical_root {
            errors.push("refused a file that escaped its scanned cache root".into());
            continue;
        }
        match fs::remove_file(&canonical) {
            Ok(()) => {
                files_cleaned += 1;
                bytes = bytes.saturating_add(file.bytes);
            }
            Err(error) => errors.push(error.to_string()),
        }
    }
    prune_empty_directories(&item.root, &item.canonical_root);
    (files_cleaned, bytes, errors)
}

fn prune_empty_directories(root: &Path, canonical_root: &Path) {
    let mut directories = Vec::<PathBuf>::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if safe_directory(&path) {
                stack.push(path);
            }
        }
        directories.push(directory);
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        if safe_directory(&directory)
            && fs::canonicalize(&directory).is_ok_and(|path| path.starts_with(canonical_root))
        {
            let _ = fs::remove_dir(&directory);
        }
    }
}

fn safe_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_dir() && !metadata.file_type().is_symlink() && !is_reparse_point(&metadata)
    })
}

fn vacuum_database(item: &CachedCleanerItem) -> Result<(usize, u64), String> {
    let path = &item
        .files
        .first()
        .ok_or_else(|| "database scan item is empty".to_string())?
        .path;
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("database unavailable: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || is_reparse_point(&metadata)
        || !has_sqlite_header(path)
    {
        return Err("database changed or is no longer a safe SQLite file".into());
    }
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("database unavailable: {error}"))?;
    if canonical != item.canonical_root {
        return Err("database changed location after preview".into());
    }
    let before = database_total_size(path);
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|error| format!("database unavailable: {error}"))?;
    connection
        .busy_timeout(Duration::from_millis(750))
        .map_err(|error| error.to_string())?;
    connection
        .execute_batch("VACUUM;")
        .map_err(|error| format!("database optimize failed: {error}"))?;
    drop(connection);
    Ok((1, before.saturating_sub(database_total_size(path))))
}

fn database_total_size(path: &Path) -> u64 {
    let main = fs::metadata(path).map_or(0, |metadata| metadata.len());
    let wal = fs::metadata(format!("{}-wal", path.to_string_lossy()))
        .map_or(0, |metadata| metadata.len());
    main.saturating_add(wal)
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
