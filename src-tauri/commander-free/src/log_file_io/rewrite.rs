// SPDX-License-Identifier: AGPL-3.0-or-later
//! Snapshot and replacement for currently unused legacy log rewrites.
use super::lock;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant},
};

const MAX_REWRITE_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const MAX_MIGRATION_TIME: Duration = Duration::from_secs(2);

/// A Windows read handle denies write sharing for the entire rewrite, so a
/// writer from an older app cannot append between our read and replacement.
fn open_snapshot(path: &Path) -> Result<Option<File>, String> {
    let mut options = OpenOptions::new();
    options.read(true);
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
    match options.open(path) {
        Ok(file) => {
            let metadata = file.metadata().map_err(|_| "log unavailable")?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("invalid log file".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err("invalid log file".into());
                }
            }
            Ok(Some(file))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("log unavailable or in use".into()),
    }
}

/// Read, transform and replace while holding the shared cross-process lease.
/// `None` means no rewrite. The staged bytes are verified before replacement.
pub(crate) fn rewrite(
    path: &Path,
    transform: impl FnOnce(&str) -> Result<Option<String>, String>,
) -> Result<(), String> {
    let _guard = lock(path)?;
    let Some(mut source) = open_snapshot(path)? else {
        return Ok(());
    };
    if source.metadata().map_err(|_| "log unavailable")?.len() > MAX_REWRITE_BYTES {
        return Err("log too large for automatic migration".into());
    }
    let mut original = String::new();
    (&mut source)
        .take(MAX_REWRITE_BYTES + 1)
        .read_to_string(&mut original)
        .map_err(|_| "log read failed")?;
    if original.len() as u64 > MAX_REWRITE_BYTES {
        return Err("log too large for automatic migration".into());
    }
    let Some(replacement) = transform(&original)? else {
        return Ok(());
    };
    let mut temporary = tempfile::Builder::new()
        .prefix(".log-encrypted-")
        .tempfile_in(path.parent().ok_or("log directory unavailable")?)
        .map_err(|_| "log staging unavailable")?;
    temporary
        .write_all(replacement.as_bytes())
        .map_err(|_| "log staging failed")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "log staging failed")?;
    if std::fs::read(temporary.path()).map_err(|_| "log staging verification failed")?
        != replacement.as_bytes()
    {
        return Err("log staging verification failed".into());
    }
    // Detect a writer that ignored the coordinating lock before publishing.
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| "log verification failed")?;
    let mut current = String::new();
    (&mut source)
        .take(MAX_REWRITE_BYTES + 1)
        .read_to_string(&mut current)
        .map_err(|_| "log verification failed")?;
    if current != original {
        return Err("log changed during migration".into());
    }
    let temporary_path = temporary.into_temp_path();
    replace_existing(path, &temporary_path)?;
    if std::fs::read(path).map_err(|_| "log replacement verification failed")?
        != replacement.as_bytes()
    {
        return Err("log replacement verification failed".into());
    }
    Ok(())
}

pub(super) fn encrypted_log(line: &str) -> bool {
    line.starts_with("L:") || line.starts_with("L2:")
}

pub(crate) fn migrate_with(
    path: &Path,
    encrypt: impl Fn(&str, &str) -> Result<String, String>,
) -> Result<(), String> {
    rewrite(path, |content| {
        if !content
            .lines()
            .any(|line| !line.is_empty() && !encrypted_log(line))
        {
            return Ok(None);
        }
        let started = Instant::now();
        let mut output = String::new();
        for (index, line) in content.lines().filter(|line| !line.is_empty()).enumerate() {
            if started.elapsed() >= MAX_MIGRATION_TIME {
                return Err("log migration exceeded time limit".into());
            }
            if index >= 100_000 {
                return Err("too many log records for automatic migration".into());
            }
            let record = if encrypted_log(line) {
                line.to_owned()
            } else if let Some((date, body)) = line.strip_prefix('[').and_then(|rest| {
                let date = rest.get(..10)?;
                let body = rest.get(11..)?;
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
                Some((date, body))
            }) {
                encrypt(date, &format!("[{body}"))?
            } else {
                encrypt(&chrono::Local::now().format("%Y-%m-%d").to_string(), line)?
            };
            if started.elapsed() >= MAX_MIGRATION_TIME {
                return Err("log migration exceeded time limit".into());
            }
            if !encrypted_log(&record) || record.contains(['\r', '\n']) {
                return Err("invalid encrypted log record".into());
            }
            if output.len().saturating_add(record.len()).saturating_add(1) > 64 * 1024 * 1024 {
                return Err("encrypted log exceeds migration limit".into());
            }
            output.push_str(&record);
            output.push('\n');
        }
        Ok(Some(output))
    })
}

#[cfg(windows)]
fn replace_existing(path: &Path, temporary: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let replacement: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    // ReplaceFileW retains the original file's security attributes. No backup
    // file is created, so migration never makes a second plaintext log.
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err("log atomic replacement failed".into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_existing(path: &Path, temporary: &Path) -> Result<(), String> {
    std::fs::rename(temporary, path).map_err(|_| "log atomic replacement failed".into())
}
