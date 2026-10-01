// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::vault_access::VaultError;
use std::fs::{File, OpenOptions};
use std::os::windows::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt},
    io::AsRawHandle,
};
use std::path::{Path, PathBuf};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CreateDirectoryW, FileDispositionInfo, GetDiskFreeSpaceExW, SetFileInformationByHandle,
        DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};

#[cfg(test)]
#[path = "vault_create_files_tests.rs"]
mod tests;

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub(super) fn pin_parents(path: &Path) -> Result<Vec<File>, VaultError> {
    let mut held = Vec::new();
    for parent in path
        .parent()
        .ok_or(VaultError::Validation)?
        .ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if !parent.is_absolute() {
            continue;
        }
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent)
            .map_err(|_| VaultError::AclReadback)?;
        let metadata = file.metadata().map_err(|_| VaultError::ContainerIdentity)?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(VaultError::ContainerIdentity);
        }
        held.push(file);
    }
    Ok(held)
}

pub(super) fn regular(file: &File) -> Result<u64, VaultError> {
    let metadata = file.metadata().map_err(|_| VaultError::ContainerIdentity)?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(VaultError::ContainerIdentity);
    }
    Ok(metadata.len())
}

pub(super) fn read_file(path: &Path) -> Result<(File, Vec<File>), VaultError> {
    let parents = pin_parents(path)?;
    let file = OpenOptions::new()
        .access_mode(FILE_GENERIC_READ)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| VaultError::AclReadback)?;
    regular(&file)?;
    Ok((file, parents))
}

pub(super) struct Destination {
    pub file: File,
    _parents: Vec<File>,
    committed: bool,
}

impl Destination {
    pub fn create(path: &Path) -> Result<Self, VaultError> {
        let parents = pin_parents(path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE)
            .create_new(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| VaultError::AclApply)?;
        regular(&file)?;
        Ok(Self {
            file,
            _parents: parents,
            committed: false,
        })
    }
    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for Destination {
    fn drop(&mut self) {
        if !self.committed {
            let info = FILE_DISPOSITION_INFO { DeleteFile: true };
            // Delete only the exact CREATE_NEW file still held open, never a path replacement.
            if unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileDispositionInfo,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32,
                )
            } == 0
            {
                eprintln!("[wincommander-svc] incomplete new vault cleanup needs attention");
            }
        }
    }
}

pub(super) struct Stage {
    pub path: PathBuf,
    _parents: Vec<File>,
    directory: Option<File>,
    pub owned_files: Vec<PathBuf>,
}

impl Stage {
    pub fn create() -> Result<Self, VaultError> {
        use rand::{rngs::OsRng, RngCore};
        let base = std::env::var_os("ProgramData").ok_or(VaultError::Persistence)?;
        let mut nonce = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| VaultError::Persistence)?;
        let suffix = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let path = PathBuf::from(base)
            .join("WinCommander")
            .join(format!("vault-create-{suffix}"));
        let parents = pin_parents(&path)?;
        let sddl: Vec<u16> = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(VaultError::AclApply);
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let created = unsafe { CreateDirectoryW(wide(&path).as_ptr(), &attributes) };
        unsafe {
            LocalFree(descriptor);
        }
        if created == 0 {
            return Err(VaultError::Persistence);
        }
        let directory = match OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
        {
            Ok(directory) => directory,
            Err(_) => {
                let _ = std::fs::remove_dir(&path);
                return Err(VaultError::Persistence);
            }
        };
        Ok(Self {
            path,
            _parents: parents,
            directory: Some(directory),
            owned_files: Vec::new(),
        })
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        for path in self.owned_files.iter().rev() {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("[wincommander-svc] protected vault staging cleanup needs attention");
                }
            }
        }
        drop(self.directory.take());
        if std::fs::remove_dir(&self.path).is_err() {
            eprintln!("[wincommander-svc] protected vault staging directory retained");
        }
    }
}

pub(super) fn available_space(path: &Path) -> Result<u64, VaultError> {
    let mut available = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            wide(path).as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(VaultError::Persistence);
    }
    Ok(available)
}
