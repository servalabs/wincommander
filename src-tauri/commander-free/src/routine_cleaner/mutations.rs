// SPDX-License-Identifier: AGPL-3.0-or-later

use super::rules::TargetOperation;
use super::scanner::{has_sqlite_header, is_old_enough};
use super::{CachedCleanerItem, CachedFile, RoutineCleanerCleanResult, RoutineCleanerError};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

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
        match delete_cached_file(file, &item.canonical_root) {
            Ok(()) => {
                files_cleaned += 1;
                bytes = bytes.saturating_add(file.bytes);
            }
            Err(error) if error == "cached file no longer exists" => continue,
            Err(error) => errors.push(error),
        }
    }
    prune_empty_directories(&item.root, &item.canonical_root);
    (files_cleaned, bytes, errors)
}

fn validate_cached_metadata(
    file: &CachedFile,
    metadata: &fs::Metadata,
    current_identity: Option<(u32, u64)>,
) -> Result<(), String> {
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse_point(metadata) {
        return Err("cached file is no longer a regular file".into());
    }
    if metadata.len() != file.bytes || current_identity != file.file_identity {
        return Err("cached file identity changed after the scan".into());
    }
    if !is_old_enough(
        metadata.modified().ok(),
        SystemTime::now(),
        file.minimum_age,
    ) {
        return Err("cached file was modified after the scan".into());
    }
    Ok(())
}

#[cfg(windows)]
fn delete_cached_file(file: &CachedFile, canonical_root: &Path) -> Result<(), String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, GetFileInformationByHandle, GetFinalPathNameByHandleW,
        SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, DELETE, FILE_DISPOSITION_INFO,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, VOLUME_NAME_DOS,
    };

    let handle_file = fs::OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&file.path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "cached file no longer exists".to_string()
            } else {
                format!("cannot open cached file for verified deletion: {error}")
            }
        })?;
    let metadata = handle_file
        .metadata()
        .map_err(|error| format!("cannot inspect cached file handle: {error}"))?;
    let handle = handle_file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(format!(
            "cannot read cached file identity: {}",
            std::io::Error::last_os_error()
        ));
    }
    let current_identity = Some((
        information.dwVolumeSerialNumber,
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ));
    validate_cached_metadata(file, &metadata, current_identity)?;

    let flags = FILE_NAME_NORMALIZED | VOLUME_NAME_DOS;
    let required = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, flags) };
    if required == 0 {
        return Err(format!(
            "cannot resolve cached file handle: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut buffer = vec![0u16; required as usize + 1];
    let written = unsafe {
        GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, flags)
    };
    if written == 0 || written as usize >= buffer.len() {
        return Err(format!(
            "cannot resolve cached file handle: {}",
            std::io::Error::last_os_error()
        ));
    }
    let final_path = PathBuf::from(OsString::from_wide(&buffer[..written as usize]));
    if !path_is_below(&final_path, canonical_root) {
        return Err("refused a file that escaped its scanned cache root".into());
    }

    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let deleted = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            std::ptr::from_ref(&disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if deleted == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(windows)]
fn path_is_below(path: &Path, root: &Path) -> bool {
    let normalize = |value: &Path| {
        value
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', r"\")
            .to_ascii_lowercase()
    };
    let path = normalize(path);
    let mut root = normalize(root);
    root.push('\\');
    path.starts_with(&root)
}

#[cfg(not(windows))]
fn delete_cached_file(file: &CachedFile, canonical_root: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(&file.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("cached file no longer exists".into());
        }
        Err(error) => return Err(format!("cannot inspect cached file: {error}")),
    };
    validate_cached_metadata(file, &metadata, None)?;
    let canonical = fs::canonicalize(&file.path)
        .map_err(|error| format!("cannot resolve cached file: {error}"))?;
    if !canonical.starts_with(canonical_root) || canonical == canonical_root {
        return Err("refused a file that escaped its scanned cache root".into());
    }
    fs::remove_file(canonical).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn prune_empty_directories(_root: &Path, _canonical_root: &Path) {
    // KT: file deletion is handle-bound; path-based directory pruning would reopen the junction race.
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[cfg(windows)]
    fn old_cached_file(path: &Path) -> CachedFile {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(
            fs::FileTimes::new().set_modified(
                SystemTime::now()
                    .checked_sub(Duration::from_secs(2 * 60 * 60))
                    .unwrap(),
            ),
        )
        .unwrap();
        let metadata = file.metadata().unwrap();
        CachedFile {
            path: path.to_path_buf(),
            bytes: metadata.len(),
            minimum_age: Duration::ZERO,
            file_identity: super::super::scanner::file_identity(path),
        }
    }

    #[test]
    fn refuses_a_cached_path_that_changed_into_a_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cache");
        let substituted = root.join("item.bin");
        fs::create_dir_all(&substituted).unwrap();
        let item = CachedCleanerItem {
            id: "item".into(),
            label: "Test cache".into(),
            root: root.clone(),
            canonical_root: fs::canonicalize(&root).unwrap(),
            files: vec![super::super::CachedFile {
                path: substituted,
                bytes: 1,
                minimum_age: Duration::ZERO,
                file_identity: None,
            }],
            operation: TargetOperation::Delete,
        };
        let (files, bytes, errors) = clean_file_group(&item, &AtomicBool::new(false));
        assert_eq!((files, bytes), (0, 0));
        assert_eq!(errors.len(), 1);
        assert!(root.join("item.bin").is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn handle_paths_must_be_strictly_below_the_scanned_root() {
        assert!(path_is_below(
            Path::new(r"\\?\C:\Cache\Nested\item.bin"),
            Path::new(r"C:\cache"),
        ));
        assert!(!path_is_below(
            Path::new(r"\\?\C:\Cache-Other\item.bin"),
            Path::new(r"C:\cache"),
        ));
        assert!(!path_is_below(
            Path::new(r"\\?\C:\Cache"),
            Path::new(r"C:\cache"),
        ));
    }

    #[cfg(windows)]
    #[test]
    fn handle_bound_delete_removes_an_unchanged_old_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cache");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("old.bin");
        fs::write(&path, b"cache").unwrap();
        let cached = old_cached_file(&path);

        delete_cached_file(&cached, &fs::canonicalize(&root).unwrap()).unwrap();

        assert!(!path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn handle_bound_delete_refuses_replaced_file_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cache");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("old.bin");
        fs::write(&path, b"first").unwrap();
        let cached = old_cached_file(&path);
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"other").unwrap();
        let replacement = old_cached_file(&path);
        assert_ne!(replacement.file_identity, cached.file_identity);

        let error = delete_cached_file(&cached, &fs::canonicalize(&root).unwrap()).unwrap_err();

        assert!(error.contains("identity changed"));
        assert!(path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn handle_bound_delete_preserves_a_locked_file() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cache");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("locked.bin");
        fs::write(&path, b"cache").unwrap();
        let cached = old_cached_file(&path);
        let _locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();

        assert!(delete_cached_file(&cached, &fs::canonicalize(&root).unwrap()).is_err());
        assert!(path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn handle_bound_delete_refuses_a_final_path_outside_the_root() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("allowed");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&allowed).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let path = outside.join("old.bin");
        fs::write(&path, b"cache").unwrap();
        let cached = old_cached_file(&path);

        let error = delete_cached_file(&cached, &fs::canonicalize(&allowed).unwrap()).unwrap_err();

        assert!(error.contains("escaped"));
        assert!(path.exists());
    }
}
