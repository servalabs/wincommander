// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental, existing-file-only Windows coordination. No runtime wiring.
use super::{Coordination, CoordinationGuard};
use std::{
    fs::{File, OpenOptions},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Storage::FileSystem::{
        GetFileInformationByHandle, LockFileEx, UnlockFileEx, BY_HANDLE_FILE_INFORMATION,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    },
    System::IO::OVERLAPPED,
};

const ERROR: &str = "fence_unavailable";
const READ_CONTROL: u32 = 0x0002_0000;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

pub(super) struct WindowsCoordination {
    pub(super) directory: PathBuf,
}

struct Guard {
    _base: File,
    _parent: File,
    _directory: File,
    lock: File,
}
impl CoordinationGuard for Guard {}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut offset: OVERLAPPED = unsafe { std::mem::zeroed() };
        unsafe {
            UnlockFileEx(self.lock.as_raw_handle(), 0, 1, 0, &mut offset);
        }
    }
}

fn open_directory(path: &Path) -> Result<File, &'static str> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| ERROR)?;
    crate::peer_auth_deployment_acl::verify_open_file(&file, true).map_err(|_| ERROR)?;
    Ok(file)
}

fn open_base(path: &Path) -> Result<File, &'static str> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| ERROR)?;
    // ProgramData legitimately permits creation; retain its handle and reject
    // reparse points without imposing a product-private DACL on the base.
    crate::peer_auth_deployment_acl::verify_open_file_type(&file, true).map_err(|_| ERROR)?;
    Ok(file)
}

fn lock_file(file: &File) -> Result<(), &'static str> {
    let mut offset: OVERLAPPED = unsafe { std::mem::zeroed() };
    if unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut offset,
        )
    } == 0
    {
        return Err(ERROR);
    }
    Ok(())
}

impl Coordination for WindowsCoordination {
    fn validates_directory(&self, directory: &Path) -> bool {
        let Some(base) = std::env::var_os("ProgramData") else {
            return false;
        };
        let expected = PathBuf::from(base)
            .join("WinCommander")
            .join("fleet-machine");
        directory.is_absolute() && directory == expected && self.directory == expected
    }
    fn acquire(&self) -> Result<Box<dyn CoordinationGuard + '_>, &'static str> {
        let base_path = PathBuf::from(std::env::var_os("ProgramData").ok_or(ERROR)?);
        let expected = base_path.clone().join("WinCommander").join("fleet-machine");
        if !self.directory.is_absolute() || self.directory != expected {
            return Err(ERROR);
        }
        let base = open_base(&base_path)?;
        let parent_path = self.directory.parent().ok_or(ERROR)?;
        let parent = open_directory(parent_path)?;
        let directory = open_directory(&self.directory)?;
        let lock = OpenOptions::new()
            .access_mode(GENERIC_READ | GENERIC_WRITE | READ_CONTROL)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.directory.join("effect-fence.lock"))
            .map_err(|_| ERROR)?;
        crate::peer_auth_deployment_acl::verify_open_file(&lock, false).map_err(|_| ERROR)?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(lock.as_raw_handle(), &mut info) } == 0
            || info.nNumberOfLinks != 1
            || info.nFileSizeHigh != 0
            || info.nFileSizeLow != 1
            || info.dwVolumeSerialNumber == 0
            || (info.nFileIndexHigh == 0 && info.nFileIndexLow == 0)
        {
            return Err(ERROR);
        }
        lock_file(&lock)?;
        Ok(Box::new(Guard {
            _base: base,
            _parent: parent,
            _directory: directory,
            lock,
        }))
    }
}
