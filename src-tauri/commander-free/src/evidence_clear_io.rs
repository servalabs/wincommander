// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fail-closed truncation of one explicitly selected legacy evidence file.
use std::{fs, io::ErrorKind, path::Path};

pub(super) fn clear_file(path: &Path) -> Result<(), String> {
    clear_file_using(path, &|file| {
        file.set_len(0).and_then(|_| file.sync_all()).map_err(|_| "Cannot clear old evidence".into())
    })
}

fn clear_file_using(path: &Path, clear: &impl Fn(&fs::File) -> Result<(), String>) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Cannot inspect old evidence".into()),
        Ok(_) => (),
    }
    #[cfg(not(windows))]
    { let _ = clear; return Err("Clearing old evidence is not supported on this platform".into()); }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        if !path.is_absolute() { return Err("Old evidence path is invalid".into()); }
        let parent = path.parent().ok_or("Old evidence directory is unavailable")?;
        let mut ancestors: Vec<_> = parent.ancestors().collect();
        ancestors.reverse();
        let mut pins = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            let pin = fs::OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor).map_err(|_| "Cannot protect old evidence directory")?;
            let meta = pin.metadata().map_err(|_| "Cannot inspect old evidence directory")?;
            if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err("Linked old evidence directory refused".into());
            }
            pins.push(pin);
        }
        let open_leaf = || fs::OpenOptions::new().read(true).write(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path);
        let leaf = match open_leaf() {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("Cannot open old evidence".into()),
        };
        let before = leaf.metadata().map_err(|_| "Cannot inspect old evidence")?;
        if !before.is_file() || before.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Linked old evidence refused".into());
        }
        // Keep the writable handle open without delete sharing. Reopen the
        // selected leaf independently and compare file IDs, rejecting hardlinks
        // so this clear cannot truncate another name for the same file.
        let verify = fs::OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)
            .map_err(|_| "Cannot verify old evidence identity")?;
        let info = |f: &fs::File| -> Result<BY_HANDLE_FILE_INFORMATION, String> {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: the live file handle and output structure remain valid for this call.
            if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0 {
                return Err("Cannot verify old evidence identity".into());
            }
            Ok(info)
        };
        let first = info(&leaf)?;
        let second = info(&verify)?;
        if first.nNumberOfLinks != 1 || second.nNumberOfLinks != 1
            || first.dwVolumeSerialNumber != second.dwVolumeSerialNumber
            || first.nFileIndexHigh != second.nFileIndexHigh || first.nFileIndexLow != second.nFileIndexLow
            || second.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Old evidence identity changed or is linked".into());
        }
        clear(&leaf)?;
        let meta = leaf.metadata().map_err(|_| "Cannot verify old evidence clear")?;
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || meta.len() != 0 {
            return Err("Old evidence clear could not be verified".into());
        }
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn clears_only_selected_file_and_preserves_other_profile() {
        let dir = tempfile::tempdir().unwrap();
        let selected = dir.path().join("selected"); let other = dir.path().join("other");
        fs::write(&selected, b"old").unwrap(); fs::write(&other, b"other").unwrap();
        clear_file(&selected).unwrap();
        assert!(fs::read(&selected).unwrap().is_empty());
        assert_eq!(fs::read(&other).unwrap(), b"other");
    }
    #[test]
    fn linked_legacy_file_is_not_followed_or_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let selected = dir.path().join("selected"); let other = dir.path().join("other");
        fs::write(&other, b"other").unwrap();
        std::os::windows::fs::symlink_file(&other, &selected).unwrap();
        assert!(clear_file(&selected).is_err());
        assert_eq!(fs::read(&other).unwrap(), b"other");
    }
    #[test]
    fn hardlinked_legacy_file_is_not_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let selected = dir.path().join("selected"); let other = dir.path().join("other");
        fs::write(&other, b"other").unwrap();
        fs::hard_link(&other, &selected).unwrap();
        assert!(clear_file(&selected).is_err());
        assert_eq!(fs::read(&other).unwrap(), b"other");
        assert_eq!(fs::read(&selected).unwrap(), b"other");
    }
    #[test]
    fn verified_leaf_cannot_be_replaced_during_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("selected"); let moved = dir.path().join("moved");
        fs::write(&path, b"before").unwrap();
        clear_file_using(&path, &|file| {
            assert!(fs::rename(&path, &moved).is_err());
            file.set_len(0).and_then(|_| file.sync_all()).map_err(|e| e.to_string())
        }).unwrap();
        assert!(!moved.exists());
        assert!(fs::read(path).unwrap().is_empty());
    }
    #[test]
    fn publish_failure_preserves_existing_bytes() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("old");
        fs::write(&path, b"original").unwrap();
        assert!(clear_file_using(&path, &|_| Err("injected failure".into())).is_err());
        assert_eq!(fs::read(path).unwrap(), b"original");
    }
}
