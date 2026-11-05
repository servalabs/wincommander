// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only preflight for records which depend on an existing settings key.
use std::{fs, io::{BufRead, BufReader, Read}, path::Path};

const LIMIT: u64 = 64 * 1024 * 1024;
const UNAVAILABLE: &str = "Settings key is missing or its existing encrypted data could not be inspected; restore the matching key. Existing data preserved.";

fn check_directory(path: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(UNAVAILABLE.into()),
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return Err(UNAVAILABLE.into()); }
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() { return Err(UNAVAILABLE.into()); }
    Ok(true)
}

fn check_file(path: &Path, any_nonempty: bool) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(UNAVAILABLE.into()),
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return Err(UNAVAILABLE.into()); }
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > LIMIT {
        return Err(UNAVAILABLE.into());
    }
    if any_nonempty && metadata.len() != 0 { return Err(UNAVAILABLE.into()); }
    let file = fs::File::open(path).map_err(|_| UNAVAILABLE.to_string())?;
    let mut input = BufReader::new(file.take(LIMIT + 1));
    let mut line = Vec::new();
    let mut read = 0u64;
    loop {
        line.clear();
        let count = input.read_until(b'\n', &mut line).map_err(|_| UNAVAILABLE.to_string())?;
        if count == 0 { return Ok(()); }
        read += count as u64;
        if read > LIMIT || line.starts_with(b"L:") || line.starts_with(b"L2:") || line.starts_with(b"D1:") {
            return Err(UNAVAILABLE.into());
        }
        let text = line.iter().copied().find(|byte| !byte.is_ascii_whitespace());
        // Unknown records might be a future encrypted format. Recognized
        // timestamped legacy plaintext remains migratable without a key.
        if text.is_some_and(|byte| byte != b'[') { return Err(UNAVAILABLE.into()); }
    }
}

/// `root` is an already-resolved user data directory; this creates nothing.
pub(super) fn check_user_artifacts(root: &Path, machine_key: bool) -> Result<(), String> {
    if !check_directory(root)? { return Ok(()); }
    let logs = root.join("logs");
    if check_directory(&logs)? {
        check_file(&logs.join("wincommander.log"), false)?;
        check_file(&logs.join("diagnostic-events.log"), false)?;
        if machine_key { check_file(&logs.join("pro-diagnostic-events.log"), false)?; }
    }
    if machine_key {
        let events = root.join("event-store");
        if check_directory(&events)? { check_file(&events.join("events.lines"), true)?; }
    }
    Ok(())
}

/// Inspect only the current user's known local paths without creating them.
/// Machine-scoped keys can also protect other users' data: this check alone
/// cannot establish that an unknown or redirected profile is empty.
pub(super) fn check_current_user(machine_key: bool) -> Result<(), String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or_else(|| UNAVAILABLE.to_string())?;
    let local = std::path::PathBuf::from(local);
    if !local.is_absolute() { return Err(UNAVAILABLE.into()); }
    let app_dir = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    check_user_artifacts(&local.join(app_dir), machine_key)?;
    if machine_key && !app_dir.eq_ignore_ascii_case("WinCommander") {
        check_user_artifacts(&local.join("WinCommander"), true)?;
    }
    Ok(())
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
        fs::write(&log, b"[2026-11-05] old plaintext\n").unwrap();
        assert!(check_user_artifacts(root.path(), true).is_ok());
        fs::write(&log, b"[2026-11-05] old plaintext\nL2:2026-11-05:ciphertext\n").unwrap();
        assert!(check_user_artifacts(root.path(), true).is_err());
        fs::write(&log, b"L:2026-11-05:ciphertext\n").unwrap();
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
}
