// SPDX-License-Identifier: AGPL-3.0-or-later
use std::fs::{File, OpenOptions};
use std::path::Path;

pub(super) fn create() -> Result<tempfile::TempDir, String> {
    #[cfg(windows)]
    {
        let user = std::env::var_os("LOCALAPPDATA")
            .ok_or("Current-user temporary storage is unavailable")?;
        let mut parent = std::path::PathBuf::from(user);
        if !parent.is_absolute() {
            return Err("Current-user temporary storage must be absolute".into());
        }
        let mut guards = directories(&parent)?;
        for name in ["WinCommander", "scan-temp"] {
            parent.push(name);
            match std::fs::create_dir(&parent) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("Cannot create scan workspace: {error}")),
            }
            guards.push(open_directory(&parent)?);
        }
        tempfile::Builder::new()
            .prefix("duplicates-")
            .tempdir_in(parent)
            .map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        tempfile::Builder::new()
            .prefix("duplicates-")
            .tempdir()
            .map_err(|error| error.to_string())
    }
}

pub(super) fn directories(path: &Path) -> Result<Vec<File>, String> {
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    ancestors.into_iter().map(open_directory).collect()
}
fn open_directory(path: &Path) -> Result<File, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || super::super::safety::is_link_or_reparse(&metadata) {
        return Err("Linked or non-directory scan workspace refused".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let directory = options.open(path).map_err(|error| error.to_string())?;
    let opened = directory.metadata().map_err(|error| error.to_string())?;
    if !opened.is_dir() || super::super::safety::is_link_or_reparse(&opened) {
        return Err("Scan workspace directory changed or is linked".into());
    }
    Ok(directory)
}
pub(super) fn reserve_database(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
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
        .open(path)
        .map_err(|error| format!("Cannot reserve private scan database: {error}"))?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // The reserved handle stays open while SQLite uses the same file; writes are allowed, replacement is denied.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0
            || information.nNumberOfLinks != 1
        {
            return Err("Scan database identity is unavailable or hard-linked".into());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata().map_err(|error| error.to_string())?.nlink() != 1 {
            return Err("Hard-linked scan database refused".into());
        }
    }
    Ok(file)
}
