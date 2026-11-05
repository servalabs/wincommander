// SPDX-License-Identifier: AGPL-3.0-or-later
//! Coordinated appends and atomic rewrites of the local log.
use std::{
    fs::{File, OpenOptions, TryLockError},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant},
};

const MAX_REWRITE_BYTES: u64 = 32 * 1024 * 1024;
// Keep startup migration well below the five-second writer lock timeout.
const MAX_MIGRATION_TIME: Duration = Duration::from_secs(2);

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
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("invalid log file".into());
        }
    }
    let mut record = Vec::with_capacity(line.len() + 1);
    record.extend_from_slice(line.as_bytes());
    record.push(b'\n');
    file.write_all(&record)
        .map_err(|_| "log append failed".into())
}

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
pub(super) fn rewrite(
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

fn encrypted_log(line: &str) -> bool {
    line.starts_with("L:") || line.starts_with("L2:")
}

pub(super) fn migrate_with(
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

pub(super) fn clear(path: &Path) -> Result<(), String> {
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
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("invalid log file".into());
        }
    }
    file.set_len(0).map_err(|_| "log clear failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn sealed(date: &str, _body: &str) -> Result<String, String> {
        Ok(format!("L2:{date}:authenticated-ciphertext"))
    }

    #[cfg(windows)]
    #[test]
    fn append_rejects_symbolic_link_without_touching_its_target() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.txt");
        let path = directory.path().join("wincommander.log");
        std::fs::write(&target, b"target remains unchanged").unwrap();
        std::os::windows::fs::symlink_file(&target, &path)
            .expect("test requires symlink privilege");
        assert!(append_encrypted(&path, || sealed("2026-11-05", "private")).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"target remains unchanged");
    }

    #[test]
    fn unavailable_key_or_encryption_never_appends_plaintext() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wincommander.log");
        for error in ["key unavailable", "encryption failed"] {
            assert!(append_encrypted(&path, || Err(error.into())).is_err());
            assert!(!path.exists());
        }
        std::fs::write(&path, b"existing ciphertext\n").unwrap();
        assert!(append_encrypted(&path, || Err("key unavailable".into())).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing ciphertext\n");
        assert!(append_encrypted(&path, || Ok("[ENCRYPT_FAIL] private text".into())).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing ciphertext\n");
    }

    #[test]
    fn migration_is_all_or_nothing_and_preserves_unicode_input_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wincommander.log");
        let original = "[2026-11-05 01:02:03] [WARN] sensitive\n[😀😀😀😀] detail\n";
        std::fs::write(&path, original).unwrap();
        let calls = std::cell::Cell::new(0);
        assert!(migrate_with(&path, |date, body| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err("encryption unavailable".into())
            } else {
                sealed(date, body)
            }
        })
        .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        migrate_with(&path, sealed).unwrap();
        let result = std::fs::read_to_string(&path).unwrap();
        assert_eq!(result.lines().count(), 2);
        assert!(result.lines().all(encrypted_log));
        assert!(!result.contains("sensitive") && !result.contains("detail"));
    }

    #[test]
    fn concurrent_append_waits_until_migration_is_committed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wincommander.log");
        std::fs::write(&path, b"legacy text\n").unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker_path = path.clone();
        let migration = std::thread::spawn(move || {
            migrate_with(&worker_path, |date, body| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                sealed(date, body)
            })
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            let result = append_encrypted(&writer_path, || sealed("2026-11-05", "new"));
            done_tx.send(()).unwrap();
            result
        });
        assert!(done_rx.recv_timeout(Duration::from_millis(150)).is_err());
        release_tx.send(()).unwrap();
        migration.join().unwrap().unwrap();
        done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        writer.join().unwrap().unwrap();
        let result = std::fs::read_to_string(&path).unwrap();
        assert_eq!(result.lines().count(), 2);
        assert!(result.lines().all(encrypted_log));
    }

    #[test]
    fn slow_migration_preserves_original_and_releases_waiting_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wincommander.log");
        let original = "legacy private text\n";
        std::fs::write(&path, original).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let worker_path = path.clone();
        let migration = std::thread::spawn(move || {
            migrate_with(&worker_path, |date, body| {
                started_tx.send(()).unwrap();
                std::thread::sleep(MAX_MIGRATION_TIME + Duration::from_millis(100));
                sealed(date, body)
            })
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let start = Instant::now();
        let result = append_encrypted(&path, || sealed("2026-11-05", "new"));
        assert!(migration.join().unwrap().is_err());
        result.unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.starts_with(original));
        assert!(content.ends_with("L2:2026-11-05:authenticated-ciphertext\n"));
    }

    #[test]
    fn uncoordinated_append_cannot_be_lost_by_migration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wincommander.log");
        std::fs::write(&path, b"legacy text\n").unwrap();
        let result = migrate_with(&path, |date, body| {
            let mut writer = OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|_| "external writer blocked")?;
            writer.write_all(b"external append\n").unwrap();
            sealed(date, body)
        });
        assert!(result.is_err());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("legacy text\n"));
        #[cfg(not(windows))]
        assert!(content.ends_with("external append\n"));
        #[cfg(windows)]
        assert_eq!(content, "legacy text\n");
    }
}
