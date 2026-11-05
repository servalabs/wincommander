// SPDX-License-Identifier: AGPL-3.0-or-later
//! Identity-bound, fail-closed clearing of legacy evidence.
use serde::{Deserialize, Serialize};
use std::{fs, io::ErrorKind, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FileStamp {
    volume: u32,
    index: u64,
    size: u64,
    last_write: u64,
    digest: String,
}

#[cfg(windows)]
pub(super) fn current_user() -> Result<String, String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{
            GetLengthSid, GetTokenInformation, IsValidSid, TokenUser, TOKEN_QUERY, TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    // SAFETY: token is a writable handle slot and the pseudo-handle is current process.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err("Cannot verify current user".into());
    }
    struct Handle(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let token = Handle(token);
    let mut len = 0u32;
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len);
    }
    if len == 0 || len > 65536 {
        return Err("Cannot verify current user".into());
    }
    let mut data = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token.0, TokenUser, data.as_mut_ptr().cast(), len, &mut len) }
        == 0
    {
        return Err("Cannot verify current user".into());
    }
    if (len as usize) < std::mem::size_of::<TOKEN_USER>() {
        return Err("Cannot verify current user".into());
    }
    // SAFETY: Windows returned a TOKEN_USER inside the live aligned token buffer.
    let sid = unsafe { (*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let begin = data.as_ptr() as usize;
    let end = begin
        .checked_add(len as usize)
        .ok_or("Cannot verify current user")?;
    let start = sid as usize;
    if start < begin || start.checked_add(8).is_none_or(|n| n > end) {
        return Err("Cannot verify current user".into());
    }
    // SAFETY: the fixed SID header is within the token buffer.
    let count = unsafe { *sid.cast::<u8>().add(1) } as usize;
    if count > 15
        || start.checked_add(8 + count * 4).is_none_or(|n| n > end)
        || unsafe { IsValidSid(sid) } == 0
    {
        return Err("Cannot verify current user".into());
    }
    let size = unsafe { GetLengthSid(sid) } as usize;
    if size != 8 + count * 4 {
        return Err("Cannot verify current user".into());
    }
    // SAFETY: validated SID has `size` bytes in the live token buffer.
    Ok(hex::encode(unsafe {
        std::slice::from_raw_parts(sid.cast::<u8>(), size)
    }))
}
#[cfg(not(windows))]
pub(super) fn current_user() -> Result<String, String> {
    Err("Evidence clear requires Windows".into())
}

pub(super) fn snapshot(path: &Path) -> Result<Option<FileStamp>, String> {
    with_verified_file(path, &|file, stamp| {
        let _ = file;
        Ok(stamp.clone())
    })
}

pub(super) fn clear_file(path: &Path, expected: &Option<FileStamp>) -> Result<(), String> {
    let actual = with_verified_file(path, &|file, stamp| {
        let Some(expected) = expected else {
            return Err("Old evidence appeared after clear began; preserve and review".into());
        };
        if stamp.volume != expected.volume || stamp.index != expected.index {
            return Err("Old evidence identity changed; preserve and review".into());
        }
        // An empty same-identity file is already cleared. A changed nonempty
        // file may contain an old writer's post-request events and is never erased.
        if stamp.size != 0 && stamp != expected {
            return Err("Old evidence changed after clear began; preserve and review".into());
        }
        if stamp.size != 0 {
            file.set_len(0)
                .and_then(|_| file.sync_all())
                .map_err(|_| "Cannot clear old evidence")?;
        }
        if file
            .metadata()
            .map_err(|_| "Cannot verify old evidence clear")?
            .len()
            != 0
        {
            return Err("Cannot verify old evidence clear".into());
        }
        Ok(())
    })?;
    if actual.is_none() && expected.is_some() {
        return Err("Old evidence disappeared; preserve and review".into());
    }
    Ok(())
}

fn with_verified_file<T>(
    path: &Path,
    operation: &impl Fn(&fs::File, &FileStamp) -> Result<T, String>,
) -> Result<Option<T>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect old evidence".into()),
        Ok(_) => (),
    }
    #[cfg(not(windows))]
    {
        let _ = operation;
        return Err("Clearing old evidence is not supported on this platform".into());
    }
    #[cfg(windows)]
    {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        if !path.is_absolute() {
            return Err("Old evidence path is invalid".into());
        }
        let parent = path
            .parent()
            .ok_or("Old evidence directory is unavailable")?;
        let mut ancestors: Vec<_> = parent.ancestors().collect();
        ancestors.reverse();
        let mut pins = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            let pin = fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor)
                .map_err(|_| "Cannot protect old evidence directory")?;
            let meta = pin
                .metadata()
                .map_err(|_| "Cannot inspect old evidence directory")?;
            if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err("Linked old evidence directory refused".into());
            }
            pins.push(pin);
        }
        let leaf = match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Cannot open old evidence".into()),
        };
        let meta = leaf.metadata().map_err(|_| "Cannot inspect old evidence")?;
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Linked old evidence refused".into());
        }
        let verify = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| "Cannot verify old evidence identity")?;
        let info = |f: &fs::File| -> Result<BY_HANDLE_FILE_INFORMATION, String> {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: both the live handle and output structure are valid.
            if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0 {
                return Err("Cannot verify old evidence identity".into());
            }
            Ok(info)
        };
        let first = info(&leaf)?;
        let second = info(&verify)?;
        if first.nNumberOfLinks != 1
            || second.nNumberOfLinks != 1
            || first.dwVolumeSerialNumber != second.dwVolumeSerialNumber
            || first.nFileIndexHigh != second.nFileIndexHigh
            || first.nFileIndexLow != second.nFileIndexLow
            || second.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err("Old evidence identity changed or is linked".into());
        }
        const MAX: u64 = 16 * 1024 * 1024;
        if meta.len() > MAX {
            return Err("Old evidence is too large to safely clear".into());
        }
        let mut contents = Vec::new();
        (&leaf)
            .take(MAX + 1)
            .read_to_end(&mut contents)
            .map_err(|_| "Cannot read old evidence")?;
        if contents.len() as u64 != meta.len() {
            return Err("Old evidence changed while being read".into());
        }
        let stamp = FileStamp {
            volume: first.dwVolumeSerialNumber,
            index: (u64::from(first.nFileIndexHigh) << 32) | u64::from(first.nFileIndexLow),
            size: meta.len(),
            last_write: (u64::from(first.ftLastWriteTime.dwHighDateTime) << 32)
                | u64::from(first.ftLastWriteTime.dwLowDateTime),
            digest: hex::encode(Sha256::digest(&contents)),
        };
        operation(&leaf, &stamp).map(Some)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn current_process_user_is_stable_and_nonempty() {
        let first = current_user().unwrap();
        assert!(!first.is_empty());
        assert_eq!(first, current_user().unwrap());
    }
    #[test]
    fn clear_refuses_later_content_and_different_file_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        fs::write(&path, b"old").unwrap();
        let initial = snapshot(&path).unwrap();
        fs::write(&path, b"old plus new event").unwrap();
        assert!(clear_file(&path, &initial).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old plus new event");
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"old").unwrap();
        assert!(clear_file(&path, &initial).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
    }
    #[test]
    fn same_bytes_rewritten_after_snapshot_are_not_cleared() {
        use std::time::{Duration, SystemTime};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        fs::write(&path, b"same event").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(3600)),
            )
            .unwrap();
        let initial = snapshot(&path).unwrap();
        fs::write(&path, b"same event").unwrap();
        assert!(clear_file(&path, &initial).is_err());
        assert_eq!(fs::read(path).unwrap(), b"same event");
    }
    #[test]
    fn already_truncated_file_can_finish_but_new_file_cannot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        fs::write(&path, b"old").unwrap();
        let initial = snapshot(&path).unwrap();
        clear_file(&path, &initial).unwrap();
        clear_file(&path, &initial).unwrap();
        let absent = snapshot(&dir.path().join("new")).unwrap();
        fs::write(dir.path().join("new"), b"new event").unwrap();
        assert!(clear_file(&dir.path().join("new"), &absent).is_err());
    }
    #[test]
    fn linked_files_are_refused_without_damage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        let other = dir.path().join("other");
        fs::write(&other, b"other").unwrap();
        fs::hard_link(&other, &path).unwrap();
        assert!(snapshot(&path).is_err());
        fs::remove_file(&path).unwrap();
        std::os::windows::fs::symlink_file(&other, &path).unwrap();
        assert!(snapshot(&path).is_err());
        assert_eq!(fs::read(other).unwrap(), b"other");
    }
    #[test]
    fn hardlink_added_after_snapshot_prevents_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        let other = dir.path().join("other");
        fs::write(&path, b"old").unwrap();
        let initial = snapshot(&path).unwrap();
        fs::hard_link(&path, &other).unwrap();
        assert!(clear_file(&path, &initial).is_err());
        assert_eq!(fs::read(other).unwrap(), b"old");
    }

    #[test]
    fn symlink_substitution_after_snapshot_does_not_touch_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        let other = dir.path().join("other");
        fs::write(&path, b"old").unwrap();
        fs::write(&other, b"other").unwrap();
        let initial = snapshot(&path).unwrap();
        fs::remove_file(&path).unwrap();
        std::os::windows::fs::symlink_file(&other, &path).unwrap();
        assert!(clear_file(&path, &initial).is_err());
        assert_eq!(fs::read(other).unwrap(), b"other");
    }
}
