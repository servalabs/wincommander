// SPDX-License-Identifier: AGPL-3.0-or-later
//! Preserve legacy readability while requiring protected keys for new writes.

pub(super) fn protect_legacy_material(
    material: &[u8; 32],
    protect: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
    unprotect: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
    persist: impl FnOnce(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    (|| {
        let protected = protect(material)?;
        if protected.len() <= material.len() || unprotect(&protected)?.as_slice() != material {
            return Err("Protected settings key could not be verified".to_string());
        }
        persist(&protected)
    })().map_err(|_| "Could not protect the existing settings key. Reopen WinCommander as administrator and retry; existing settings are preserved.".to_string())
}

/// The open handle prevents replacement of the lock file on Windows. The OS
/// releases its lock if a writer terminates, so another writer can recover.
#[cfg(windows)]
pub(super) fn acquire_writer_lock(directory: &std::path::Path) -> Result<std::fs::File, String> {
    use std::{
        os::windows::fs::{MetadataExt, OpenOptionsExt},
        time::{Duration, Instant},
    };
    const UNAVAILABLE: &str = "Could not access the settings key migration lock. Reopen WinCommander as administrator and retry; existing settings are preserved.";
    const BUSY: &str = "Settings key protection is busy. Wait and retry; existing settings are preserved.";
    let path = directory.join(".install.material.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .share_mode(0x0000_0001 | 0x0000_0002) // read/write sharing; no delete sharing
        .custom_flags(0x0020_0000) // FILE_FLAG_OPEN_REPARSE_POINT
        .open(path)
        .map_err(|_| UNAVAILABLE.to_string())?;
    let metadata = file.metadata().map_err(|_| UNAVAILABLE.to_string())?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err(UNAVAILABLE.into());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(15))
            }
            Err(std::fs::TryLockError::WouldBlock) => return Err(BUSY.into()),
            Err(_) => return Err(UNAVAILABLE.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::protect_legacy_material;

    #[test]
    fn failed_protection_refuses_write() {
        let key = [0x42; 32];
        let result = protect_legacy_material(
            &key,
            |_| Err("failed".into()),
            |_| panic!("nothing to decrypt"),
            |_| panic!("must not persist"),
        );
        assert!(result.is_err());
        let result = protect_legacy_material(
            &key,
            |_| Ok(vec![9; 64]),
            |_| Ok(vec![7; 32]),
            |_| panic!("mismatched key must not be persisted"),
        );
        assert!(result.is_err());
        let result = protect_legacy_material(
            &key,
            |_| Ok(vec![9; 64]),
            |_| Ok(key.to_vec()),
            |_| Err("destination is locked".into()),
        );
        assert!(result.is_err());
    }

    #[cfg(windows)]
    #[test]
    fn writer_lock_serializes_distinct_processes() {
        const DIR: &str = "WINCOM_TEST_SETTINGS_LOCK_DIR";
        if let Some(path) = std::env::var_os(DIR) {
            let directory = std::path::PathBuf::from(path);
            std::fs::write(directory.join("ready"), b"ready").unwrap();
            let _guard = super::acquire_writer_lock(&directory).unwrap();
            return;
        }
        let directory = std::env::temp_dir().join(format!(
            "wincom-settings-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let guard = super::acquire_writer_lock(&directory).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("writer_lock_serializes_distinct_processes")
            .env(DIR, &directory)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while !directory.join("ready").exists()
            && start.elapsed() < std::time::Duration::from_secs(10)
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(directory.join("ready").exists(), "child did not start");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            child.try_wait().unwrap().is_none(),
            "child bypassed the lock"
        );
        drop(guard);
        assert!(child.wait().unwrap().success());
        std::fs::remove_file(directory.join("ready")).unwrap();
        std::fs::remove_file(directory.join(".install.material.lock")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
