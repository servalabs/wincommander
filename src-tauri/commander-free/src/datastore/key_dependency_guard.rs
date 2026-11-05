// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only preflight for records which depend on an existing settings key.
#[cfg(test)]
use std::fs;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::Path,
};

const LIMIT: u64 = 64 * 1024 * 1024;
const UNAVAILABLE: &str = "Settings key is missing or its existing encrypted data could not be inspected; restore the matching key. Existing data preserved.";

/// Keep checked paths open until the replacement key has been published. This
/// prevents a checked existing file from being replaced or written mid-scan.
pub(super) struct DependencyGuard {
    _held: Vec<File>,
}

fn check_directory(path: &Path, held: &mut Vec<File>) -> Result<bool, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let directory = match OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(UNAVAILABLE.into()),
    };
    let metadata = directory.metadata().map_err(|_| UNAVAILABLE.to_string())?;
    use std::os::windows::fs::MetadataExt;
    if metadata.file_attributes() & 0x400 != 0 {
        return Err(UNAVAILABLE.into());
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(UNAVAILABLE.into());
    }
    held.push(directory);
    Ok(true)
}

fn check_file(path: &Path, any_nonempty: bool, held: &mut Vec<File>) -> Result<(), String> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    let mut file = match OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(UNAVAILABLE.into()),
    };
    let metadata = file.metadata().map_err(|_| UNAVAILABLE.to_string())?;
    if metadata.file_attributes() & 0x400 != 0 {
        return Err(UNAVAILABLE.into());
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > LIMIT {
        return Err(UNAVAILABLE.into());
    }
    if any_nonempty && metadata.len() != 0 {
        return Err(UNAVAILABLE.into());
    }
    let mut input = BufReader::new((&mut file).take(LIMIT + 1));
    let mut line = Vec::new();
    let mut read = 0u64;
    loop {
        line.clear();
        let count = input
            .read_until(b'\n', &mut line)
            .map_err(|_| UNAVAILABLE.to_string())?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > LIMIT
            || line.starts_with(b"L:")
            || line.starts_with(b"L2:")
            || line.starts_with(b"D1:")
        {
            return Err(UNAVAILABLE.into());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        // Only accept the legacy log timestamp; other formats may be ciphertext.
        let prefix = line.get(..21).ok_or_else(|| UNAVAILABLE.to_string())?;
        let timestamp = std::str::from_utf8(prefix).map_err(|_| UNAVAILABLE.to_string())?;
        if chrono::NaiveDateTime::parse_from_str(timestamp, "[%Y-%m-%d %H:%M:%S]").is_err() {
            return Err(UNAVAILABLE.into());
        }
    }
    held.push(file);
    Ok(())
}

/// `root` is an already-resolved user data directory; this creates nothing.
fn check_user_artifacts_held(
    root: &Path,
    machine_key: bool,
    held: &mut Vec<File>,
) -> Result<(), String> {
    if !check_directory(root, held)? {
        return Ok(());
    }
    let logs = root.join("logs");
    if check_directory(&logs, held)? {
        check_file(&logs.join("wincommander.log"), false, held)?;
        check_file(&logs.join("diagnostic-events.log"), false, held)?;
        if machine_key {
            check_file(&logs.join("pro-diagnostic-events.log"), false, held)?;
        }
    }
    if machine_key {
        let events = root.join("event-store");
        if check_directory(&events, held)? {
            check_file(&events.join("events.lines"), true, held)?;
        }
    }
    Ok(())
}

/// Inspect only the current user's known local paths without creating them.
/// Machine-scoped keys can also protect other users' data: this check alone
/// cannot establish that an unknown or redirected profile is empty.
pub(super) fn check_current_user(machine_key: bool) -> Result<DependencyGuard, String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or_else(|| UNAVAILABLE.to_string())?;
    let local = std::path::PathBuf::from(local);
    if !local.is_absolute() {
        return Err(UNAVAILABLE.into());
    }
    let app_dir = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    let mut held = Vec::new();
    let mut parents: Vec<_> = local.ancestors().collect();
    parents.reverse();
    for parent in parents {
        if !check_directory(parent, &mut held)? {
            return Err(UNAVAILABLE.into());
        }
    }
    check_user_artifacts_held(&local.join(app_dir), machine_key, &mut held)?;
    if machine_key && !app_dir.eq_ignore_ascii_case("WinCommander") {
        check_user_artifacts_held(&local.join("WinCommander"), true, &mut held)?;
    }
    Ok(DependencyGuard { _held: held })
}

#[cfg(test)]
fn check_user_artifacts(root: &Path, machine_key: bool) -> Result<DependencyGuard, String> {
    let mut held = Vec::new();
    check_user_artifacts_held(root, machine_key, &mut held)?;
    Ok(DependencyGuard { _held: held })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_ciphertext_and_pro_events_block_new_keys_but_plaintext_logs_do_not() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("not-created");
        assert!(check_user_artifacts(&absent, true).is_ok());
        assert!(!absent.exists());
        fs::create_dir(root.path().join("logs")).unwrap();
        let log = root.path().join("logs").join("wincommander.log");
        fs::write(&log, b"[2026-11-05 01:02:03] old plaintext\n").unwrap();
        assert!(check_user_artifacts(root.path(), true).is_ok());
        fs::write(
            &log,
            b"[2026-11-05 01:02:03] old plaintext\nL2:2026-11-05:ciphertext\n",
        )
        .unwrap();
        assert!(check_user_artifacts(root.path(), true).is_err());
        fs::write(&log, b"L:2026-11-05:ciphertext\n").unwrap();
        assert!(check_user_artifacts(root.path(), false).is_err());
        fs::write(&log, b"[unknown-encrypted-format] secret\n").unwrap();
        assert!(check_user_artifacts(root.path(), false).is_err());
        fs::write(&log, b"unknown-format-record\n").unwrap();
        assert!(check_user_artifacts(root.path(), false).is_err());
        fs::write(&log, b"").unwrap();
        let diagnostic = root.path().join("logs").join("diagnostic-events.log");
        fs::write(&diagnostic, b"D1:2026-11-05:ciphertext\n").unwrap();
        assert!(check_user_artifacts(root.path(), false).is_err());
        fs::write(&diagnostic, b"").unwrap();
        let pro_diagnostic = root.path().join("logs").join("pro-diagnostic-events.log");
        fs::write(&pro_diagnostic, b"D1:2026-11-05:ciphertext\n").unwrap();
        assert!(check_user_artifacts(root.path(), true).is_err());
        assert!(check_user_artifacts(root.path(), false).is_ok());
        fs::write(&pro_diagnostic, b"").unwrap();
        fs::create_dir(root.path().join("event-store")).unwrap();
        let events = root.path().join("event-store").join("events.lines");
        fs::write(&events, b"existing event\n").unwrap();
        assert!(check_user_artifacts(root.path(), true).is_err());
        assert!(check_user_artifacts(root.path(), false).is_ok());
    }
    #[test]
    fn unreadable_shapes_and_oversized_files_are_not_treated_as_empty() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("logs")).unwrap();
        let log = root.path().join("logs").join("diagnostic-events.log");
        fs::create_dir(&log).unwrap();
        assert!(check_user_artifacts(root.path(), true).is_err());
        fs::remove_dir(&log).unwrap();
        let file = fs::File::create(&log).unwrap();
        file.set_len(LIMIT + 1).unwrap();
        assert!(check_user_artifacts(root.path(), true).is_err());
    }

    #[test]
    fn inspected_log_cannot_be_changed_or_replaced_until_guard_releases() {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let log = logs.join("wincommander.log");
        fs::write(&log, b"[2026-11-05 01:02:03] legacy plaintext\n").unwrap();
        let guard = check_user_artifacts(root.path(), true).unwrap();
        assert!(OpenOptions::new().append(true).open(&log).is_err());
        assert!(fs::rename(&log, logs.join("renamed.log")).is_err());
        drop(guard);
        assert!(OpenOptions::new().append(true).open(&log).is_ok());
    }

    #[test]
    fn actively_open_writer_blocks_key_creation_preflight() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let log = logs.join("wincommander.log");
        fs::write(&log, b"[2026-11-05 01:02:03] legacy plaintext\n").unwrap();
        let writer = OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ)
            .open(&log)
            .unwrap();
        assert!(check_user_artifacts(root.path(), true).is_err());
        drop(writer);
        assert!(check_user_artifacts(root.path(), true).is_ok());
    }

    #[test]
    fn inspected_log_is_protected_from_a_separate_process() {
        const CHILD_PATH: &str = "WINCOMMANDER_DEPENDENCY_GUARD_TEST_FILE";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            assert!(OpenOptions::new().append(true).open(path).is_err());
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let log = logs.join("wincommander.log");
        let original = b"[2026-11-05 01:02:03] legacy plaintext\n";
        fs::write(&log, original).unwrap();
        let guard = check_user_artifacts(root.path(), true).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("inspected_log_is_protected_from_a_separate_process")
            .env(CHILD_PATH, &log)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(fs::read(&log).unwrap(), original);
        drop(guard);
        assert!(OpenOptions::new().append(true).open(&log).is_ok());
    }
}
