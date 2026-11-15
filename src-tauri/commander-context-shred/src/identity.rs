//! Windows object pins compare the identity of every mutation handle.
use std::{fs::File, io, path::Path};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_DIRECTORY,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE, FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, DELETE,
    FileBasicInfo, FileDispositionInfo, FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_ATTRIBUTE_NORMAL,
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

    pub(super) fn verify_path(&self, path: &Path) -> Result<(), String> {
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|error| format!("cannot verify resolved erase target: {error}"))?;
        self.verify(&file)
    }

    fn verify_same_object(&self, file: &File) -> Result<(), String> {
        let info = information(file).map_err(|error| error.to_string())?;
        if key(&info) != self.identity {
            return Err("erase target identity changed".into());
        }
        Ok(())
    }

    pub(super) fn verify_pending_file(&self, file: &File) -> Result<(), String> {
        use windows_sys::Win32::Storage::FileSystem::{
            FileStandardInfo, GetFileInformationByHandleEx, FILE_STANDARD_INFO,
        };
        self.verify_same_object(file)?;
        let mut info = FILE_STANDARD_INFO::default();
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileStandardInfo,
                std::ptr::addr_of_mut!(info).cast(),
                std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
            )
        } == 0 {
            return Err(format!(
                "cannot inspect pending erase target: {}",
                io::Error::last_os_error()
            ));
        }
        // The selected name is delete-pending and therefore no longer counts
        // as a non-deleted link. Any remaining link makes overwrite unsafe.
        if info.Directory || !info.DeletePending || info.NumberOfLinks != 0 {
            return Err("refused erase target with another link or without pending deletion".into());
        }
        Ok(())
    }

    pub(super) fn is_directory(&self) -> bool { self.directory }

    pub(super) fn attributes(&self) -> Result<u32, String> {
        self.verify(&self.file)?;
        information(&self.file).map(|info| info.dwFileAttributes).map_err(|error| error.to_string())
    }

    pub(super) fn open_attribute_handle(&self, path: &Path) -> Result<File, String> {
        let file = std::fs::OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)
            .map_err(|error| format!("cannot open erase target attributes: {error}"))?;
        self.verify(&file)?;
        Ok(file)
    }

    pub(super) fn handle_attributes(&self, file: &File) -> Result<u32, String> {
        self.verify(file)?;
        information(file).map(|info| info.dwFileAttributes).map_err(|error| error.to_string())
    }

    pub(super) fn set_attributes(&self, file: &File, attributes: u32) -> Result<(), String> {
        self.verify(file)?;
        Self::write_attributes(file, attributes)
    }

    pub(super) fn restore_attributes(&self, file: &File, attributes: u32) -> Result<(), String> {
        // Restore the same object even if a new hard link appeared after the
        // readonly bit was cleared; restoration must not leave it writable.
        self.verify_same_object(file)?;
        Self::write_attributes(file, attributes)
    }

    fn write_attributes(file: &File, attributes: u32) -> Result<(), String> {
        let info = FILE_BASIC_INFO {
            CreationTime: 0, LastAccessTime: 0, LastWriteTime: 0, ChangeTime: 0,
            FileAttributes: if attributes == 0 { FILE_ATTRIBUTE_NORMAL } else { attributes },
        };
        if unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileBasicInfo,
            std::ptr::from_ref(&info).cast(), std::mem::size_of::<FILE_BASIC_INFO>() as u32) } == 0 {
            return Err(format!("cannot update erase target attributes: {}", io::Error::last_os_error()));
        }
        Ok(())
    }

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
