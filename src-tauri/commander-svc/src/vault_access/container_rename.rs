// SPDX-License-Identifier: AGPL-3.0-or-later
//! The container file is renamed in the caller's token, inside its own folder,
//! through the handle whose identity was checked: a file swapped in at the
//! path is never the one renamed, and an existing name is never overwritten.

use super::*;

/// Why a rename of the container file did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameFault {
    /// A file or folder already has the new name.
    Exists,
    File(FileFault),
    /// The file at the path is no longer the container that was checked.
    Identity,
    /// Windows refused for a reason with no code of its own.
    Failed,
}

#[cfg(windows)]
fn fault_from_io(error: &std::io::Error) -> RenameFault {
    const ERROR_FILE_EXISTS: i32 = 80;
    const ERROR_ALREADY_EXISTS: i32 = 183;
    match error.raw_os_error() {
        Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS) => RenameFault::Exists,
        _ => FileFault::from_io(error).map_or(RenameFault::Failed, RenameFault::File),
    }
}

#[cfg(windows)]
fn impersonated<T>(
    caller_token: windows_sys::Win32::Foundation::HANDLE,
    operation: impl FnOnce() -> Result<T, RenameFault>,
) -> Result<T, RenameFault> {
    with_caller_impersonation(caller_token, || Ok(operation())).unwrap_or(Err(RenameFault::Failed))
}

#[cfg(windows)]
fn open_container(path: &Path, access: u32, share: u32, identity: &str) -> Result<std::fs::File, RenameFault> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT};
    let file = std::fs::OpenOptions::new()
        .access_mode(access)
        .share_mode(share)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| fault_from_io(&error))?;
    let metadata = file.metadata().map_err(|_| RenameFault::Identity)?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || file_identity_from_handle(&file).ok().as_deref() != Some(identity)
    {
        return Err(RenameFault::Identity);
    }
    Ok(file)
}

/// Proves the caller may write and rename the container and add a file to its folder.
#[cfg(windows)]
pub(crate) fn prove_rename_access_as_caller(
    target: &PasswordChangeTarget,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
) -> Result<(), RenameFault> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ADD_FILE, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    impersonated(caller_token, || {
        let path = Path::new(&target.container_path);
        open_container(path, FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE, share, &target.container_identity)?;
        let folder = path.parent().ok_or(RenameFault::File(FileFault::NotFound))?;
        std::fs::OpenOptions::new()
            .access_mode(FILE_ADD_FILE)
            .share_mode(share)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(folder)
            .map(drop)
            .map_err(|error| fault_from_io(&error))
    })
}

#[cfg(windows)]
pub(crate) fn target_exists_as_caller(
    path: &str,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
) -> Result<bool, RenameFault> {
    impersonated(caller_token, || match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(fault_from_io(&error)),
    })
}

/// Renames `from` to `to` (same folder) only if it is still `identity`.
#[cfg(windows)]
pub(crate) fn rename_container_as_caller(
    from: &str,
    identity: &str,
    to: &str,
    caller_token: windows_sys::Win32::Foundation::HANDLE,
) -> Result<(), RenameFault> {
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_READ_ATTRIBUTES, FILE_SHARE_READ};
    impersonated(caller_token, || {
        // No write or delete sharing: a program holding the file open for writing makes this "in use".
        let file = open_container(Path::new(from), DELETE | FILE_READ_ATTRIBUTES, FILE_SHARE_READ, identity)?;
        rename_handle(&file, Path::new(to))
    })
}

#[cfg(windows)]
fn rename_handle(file: &std::fs::File, to: &Path) -> Result<(), RenameFault> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO};
    let name: Vec<u16> = to.as_os_str().encode_wide().collect();
    let size = std::mem::offset_of!(FILE_RENAME_INFO, FileName) + (name.len() + 1) * 2;
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    let renamed = unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(), name.len());
        SetFileInformationByHandle(
            file.as_raw_handle() as _,
            FileRenameInfo,
            info.cast_const().cast(),
            (buffer.len() * 8) as u32,
        )
    };
    if renamed == 0 {
        return Err(fault_from_io(&std::io::Error::last_os_error()));
    }
    Ok(())
}
