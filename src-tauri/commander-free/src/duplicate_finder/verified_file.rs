use super::CachedFile;
use std::fs::{File, OpenOptions};
use std::sync::atomic::AtomicBool;

pub(super) fn open_verified(
    file: &CachedFile,
    deleting: bool,
    cancelled: &AtomicBool,
) -> Result<File, String> {
    super::safety::validated_file(file)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_READ,
        };
        // Hold the verified copy against writes, replacement, and deletion until the removal completes.
        options
            .access_mode(FILE_GENERIC_READ | if deleting { DELETE } else { 0 })
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(not(windows))]
    let _ = deleting;
    let mut handle = options
        .open(&file.path)
        .map_err(|error| error.to_string())?;
    let metadata = handle.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || super::safety::is_link_or_reparse(&metadata)
        || metadata.len() != file.bytes
        || metadata.modified().ok() != file.modified
    {
        return Err("duplicate file changed after scanning; scan again".into());
    }
    #[cfg(windows)]
    verify_handle(&handle, file)?;
    let hash = super::scan::hash_reader(&mut handle, cancelled)?;
    if hash != file.hash {
        return Err("duplicate content changed after scanning; scan again".into());
    }
    Ok(handle)
}

#[cfg(windows)]
pub(super) fn handle_identity(handle: &File) -> Result<Option<(u32, u64)>, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let raw = handle.as_raw_handle();
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(raw, &mut information) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(Some((
        information.dwVolumeSerialNumber,
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    )))
}
#[cfg(not(windows))]
pub(super) fn handle_identity(_handle: &File) -> Result<Option<(u32, u64)>, String> {
    Ok(None)
}

#[cfg(windows)]
fn verify_handle(handle: &File, file: &CachedFile) -> Result<(), String> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let raw = handle.as_raw_handle();
    let identity = handle_identity(handle)?;
    if file.identity.is_none() || identity != file.identity {
        return Err("duplicate file identity changed; scan again".into());
    }
    let required = unsafe { GetFinalPathNameByHandleW(raw, std::ptr::null_mut(), 0, 0) };
    if required == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut buffer = vec![0u16; required as usize + 1];
    let written =
        unsafe { GetFinalPathNameByHandleW(raw, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if written == 0 || written as usize >= buffer.len() {
        return Err("cannot resolve duplicate file handle".into());
    }
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written as usize]));
    if !path.starts_with(&file.root) {
        return Err("duplicate file escaped its scanned root".into());
    }
    Ok(())
}

pub(super) fn delete_verified(handle: File, file: &CachedFile) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
        };
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                handle.as_raw_handle(),
                FileDispositionInfo,
                std::ptr::from_ref(&disposition).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let _ = file;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        drop(handle);
        super::safety::validated_file(file)?;
        std::fs::remove_file(&file.path).map_err(|error| error.to_string())
    }
}
