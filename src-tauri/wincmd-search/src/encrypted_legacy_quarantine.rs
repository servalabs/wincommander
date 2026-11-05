// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental reversible quarantine, never secure erasure. Caller must hold
//! the selected encrypted generation and stop all old binaries first. The lock
//! below only excludes cooperating Tantivy writers; it is not a process fence.
use std::io;
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
use std::path::{Path, PathBuf};

/// Deterministic within a generation, so recovery can locate a moved backup
/// after an interrupted rename without a separately written sidecar record.
pub(crate) fn backup_name(index_id: &[u8; 16]) -> io::Result<String> {
    if *index_id == [0; 16] { return Err(io::Error::other("invalid index identity")); }
    Ok(format!(".wincommander-legacy-index-backup-{}", hex::encode(index_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_name_is_deterministic_and_scoped_to_generation() {
        assert_eq!(backup_name(&[1; 16]).unwrap(), backup_name(&[1; 16]).unwrap());
        assert_ne!(backup_name(&[1; 16]).unwrap(), backup_name(&[2; 16]).unwrap());
        assert!(backup_name(&[0; 16]).is_err());
    }
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(crate) fn quarantine(old: &Path, index_id: &[u8; 16]) -> io::Result<PathBuf> {
    move_to_sibling(old, &backup_name(index_id)?)
}

/// Explicit recovery only: do not overwrite a recreated legacy directory.
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(crate) fn restore(old: &Path, index_id: &[u8; 16]) -> io::Result<PathBuf> {
    let parent = old.parent().ok_or_else(|| io::Error::other("invalid legacy index path"))?;
    let name = old.file_name().and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("invalid legacy index name"))?;
    let source = parent.join(backup_name(index_id)?);
    move_to_sibling(&source, name)
}

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
fn move_to_sibling(old: &Path, destination_name: &str) -> io::Result<PathBuf> {
    use crate::read_only::{AccessMode, CheckedDirectory};
    use std::os::windows::{ffi::OsStrExt, fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO,
        FILE_RENAME_INFO_0, FILE_SHARE_READ, FILE_SHARE_WRITE, FileRenameInfo,
    };
    let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "legacy search quarantine refused");
    let mut destination = std::path::Path::new(destination_name).components();
    if !matches!(destination.next(), Some(std::path::Component::Normal(_))) || destination.next().is_some() {
        return Err(denied());
    }
    let parent = old.parent().ok_or_else(denied)?;
    let parent_pins = CheckedDirectory::pin_root_path(parent)?;
    let _parent_handle = parent_pins.last().ok_or_else(denied)?;
    let old_pins = CheckedDirectory::pin_root_path(old)?;
    let old_handle = old_pins.last().ok_or_else(denied)?;
    let identity = |file: &std::fs::File| -> io::Result<(u32, u32, u32)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file and output struct stay valid for the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
    };
    let before = identity(old_handle)?;
    let legacy = CheckedDirectory::open(old, AccessMode::Writable).map_err(|e| io::Error::other(e.to_string()))?;
    let _writer = legacy.acquire_lock_pinned(&tantivy::directory::INDEX_WRITER_LOCK)
        .map_err(|e| io::Error::other(e.to_string()))?;
    // Allow our delete-capable handle to open, then verify it is exactly the
    // pinned object. Parent pins remain held across this narrow transition.
    drop(old_pins);
    let handle = std::fs::OpenOptions::new().read(true)
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(old).map_err(|e| io::Error::new(e.kind(), format!("source-open: {e}")))?;
    let metadata = handle.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || identity(&handle)? != before {
        return Err(denied());
    }
    let name = destination_name;
    let destination_path = parent.join(name);
    let wide: Vec<u16> = destination_path.as_os_str().encode_wide().collect();
    let name_bytes = wide.len().checked_mul(2).ok_or_else(denied)?;
    let size = std::mem::offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes).and_then(|n| n.checked_add(2)).ok_or_else(denied)?;
    let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: aligned allocation covers the structure and UTF-16 name;
    // both the source and destination directory handles remain alive.
    unsafe {
        (*info).Anonymous = FILE_RENAME_INFO_0 { ReplaceIfExists: false };
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = u32::try_from(name_bytes).map_err(|_| denied())?;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(), wide.len());
        if SetFileInformationByHandle(handle.as_raw_handle(), FileRenameInfo, info.cast(), u32::try_from(size).map_err(|_| denied())?) == 0 {
            let e = io::Error::last_os_error();
            return Err(io::Error::new(e.kind(), format!("source-rename: {e}")));
        }
    }
    // An error after the rename may be ambiguous after a crash; never delete a
    // backup automatically. Directory-entry durability is not established.
    Ok(parent.join(name))
}
