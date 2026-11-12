//! Windows object pins retain ancestor names and compare the final mutation handle.
use std::{fs::File, io, path::Path};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_DIRECTORY,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE, FILE_READ_ATTRIBUTES, DELETE,
    FileDispositionInfo, FILE_DISPOSITION_INFO,
};

pub(super) struct ObjectPin {
    file: File,
    identity: (u32, u64),
    directory: bool,
}

fn information(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other("refused reparse-point identity"));
    }
    Ok(info)
}

fn key(info: &BY_HANDLE_FILE_INFORMATION) -> (u32, u64) {
    (info.dwVolumeSerialNumber, (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
}

impl ObjectPin {
    pub(super) fn capture(path: &Path, directory: bool, deleting: bool) -> Result<Self, String> {
        let access = FILE_READ_ATTRIBUTES | if deleting && directory { DELETE } else { 0 };
        let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | if directory { 0 } else { FILE_SHARE_DELETE };
        let file = std::fs::OpenOptions::new().access_mode(access).share_mode(sharing)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path).map_err(|error| format!("cannot pin erase target: {error}"))?;
        let info = information(&file).map_err(|error| error.to_string())?;
        if (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory {
            return Err("erase target type changed".into());
        }
        if !directory && info.nNumberOfLinks != 1 {
            return Err("refused hard-linked erase target".into());
        }
        Ok(Self { file, identity: key(&info), directory })
    }

    pub(super) fn verify(&self, file: &File) -> Result<(), String> {
        let info = information(file).map_err(|error| error.to_string())?;
        if key(&info) != self.identity || (!self.directory && info.nNumberOfLinks != 1) {
            return Err("erase target identity changed".into());
        }
        Ok(())
    }

    pub(super) fn is_directory(&self) -> bool { self.directory }

    pub(super) fn guard_name(&self, path: &Path) -> Result<File, String> {
        let file = std::fs::OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)
            .map_err(|error| format!("cannot guard erase target name: {error}"))?;
        self.verify(&file)?;
        Ok(file)
    }

    pub(super) fn delete_directory(self) -> Result<(), String> {
        self.verify(&self.file)?;
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe { SetFileInformationByHandle(self.file.as_raw_handle(), FileDispositionInfo,
            std::ptr::from_ref(&disposition).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) } == 0 {
            return Err(format!("cannot remove pinned directory: {}", io::Error::last_os_error()));
        }
        Ok(())
    }
}

pub(super) fn pin_ancestors(path: &Path) -> Result<Vec<ObjectPin>, String> {
    let mut ancestors: Vec<_> = path.ancestors().skip(1).collect();
    ancestors.reverse();
    ancestors.into_iter().map(|ancestor| ObjectPin::capture(ancestor, true, false)).collect()
}
