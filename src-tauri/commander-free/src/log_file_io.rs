// SPDX-License-Identifier: AGPL-3.0-or-later
//! Coordinated appends and atomic rewrites of the local log.
use std::{
    fs::{File, OpenOptions, TryLockError},
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

fn lock(path: &Path) -> Result<File, String> {
    let parent = path.parent().ok_or("log directory unavailable")?;
    let lock_path = parent.join(".wincommander-log.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(lock_path)
        .map_err(|_| "log lock unavailable")?;
    let metadata = file.metadata().map_err(|_| "log lock unavailable")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("invalid log lock".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("invalid log lock".into());
        }
    }
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => return Err("log lock unavailable".into()),
        }
    }
}

// Keep each ancestor open without delete sharing, from root to leaf, so no
// checked directory can be replaced by a junction before the file is opened.
#[cfg(windows)]
fn hold_ordinary_ancestors(path: &Path) -> Result<Vec<File>, String> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    if !path.is_absolute() {
        return Err("log path must be absolute".into());
    }
    let mut ancestors: Vec<_> = path
        .parent()
        .ok_or("log directory unavailable")?
        .ancestors()
        .collect();
    ancestors.reverse();
    let mut held = Vec::with_capacity(ancestors.len());
    for ancestor in ancestors {
        let directory = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(ancestor)
            .map_err(|_| "log directory unavailable")?;
        let metadata = directory
            .metadata()
            .map_err(|_| "log directory unavailable")?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("invalid log directory".into());
        }
        held.push(directory);
    }
    Ok(held)
}

/// The callback may fail before any file is opened; failures never append.
pub(super) fn append_encrypted(
    path: &Path,
    encrypt: impl FnOnce() -> Result<String, String>,
) -> Result<(), String> {
    let line = encrypt()?;
    if line.contains(['\r', '\n']) || !line.starts_with("L2:") {
        return Err("invalid encrypted log record".into());
    }
    append_line(path, &line)
}

pub(super) fn append_line(path: &Path, line: &str) -> Result<(), String> {
    #[cfg(windows)]
    let _ancestors = hold_ordinary_ancestors(path)?;
    let _guard = lock(path)?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options.open(path).map_err(|_| "log unavailable")?;
    let metadata = file.metadata().map_err(|_| "log unavailable")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("invalid log file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0
            || information.nNumberOfLinks != 1
        {
            return Err("invalid log file".into());
        }
    }
    let mut record = Vec::with_capacity(line.len() + 1);
    record.extend_from_slice(line.as_bytes());
    record.push(b'\n');
    file.write_all(&record)
        .map_err(|_| "log append failed".into())
}

pub(super) fn clear(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let _ancestors = hold_ordinary_ancestors(path)?;
    let _guard = lock(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(|_| "log clear failed")?;
    let metadata = file.metadata().map_err(|_| "log clear failed")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("invalid log file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0
            || information.nNumberOfLinks != 1
        {
            return Err("invalid log file".into());
        }
    }
    file.set_len(0).map_err(|_| "log clear failed".into())
}

#[path = "log_file_io/rewrite.rs"]
mod rewrite;
#[cfg(test)]
use rewrite::{encrypted_log, MAX_MIGRATION_TIME};
pub(super) use rewrite::{migrate_with, rewrite};

#[cfg(test)]
#[path = "log_file_io/tests.rs"]
mod tests;
