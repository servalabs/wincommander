// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hold and move the verified file itself; a public directory entry is never a mutation target.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::{
    ffi::OsStrExt,
    io::{AsRawHandle, FromRawHandle},
};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::{LocalFree, INVALID_HANDLE_VALUE},
    Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        CreateWellKnownSid, EqualSid, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        OWNER_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{
        CreateFileW, FileDispositionInfo, FileRenameInfo, GetFileInformationByHandle,
        SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO, FILE_RENAME_INFO_0, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    },
};

const GENERIC_READ: u32 = 0x8000_0000;
const DELETE: u32 = 0x0001_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const MAX_LINK_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct LinkIdentity {
    pub(super) sha256: String,
    pub(super) volume: u32,
    pub(super) file_index: u64,
    pub(super) attributes: u32,
}

pub(super) struct OpenedLink {
    pub(super) file: File,
    pub(super) identity: LinkIdentity,
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub(super) fn volume_of(file: &File) -> io::Result<u32> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.dwVolumeSerialNumber)
}

pub(super) fn hold_parent(path: &Path) -> io::Result<Vec<File>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut guards = Vec::new();
    for ancestor in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let handle = unsafe {
            CreateFileW(
                wide(ancestor).as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_handle(handle) };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        guards.push(file);
    }
    Ok(guards)
}

pub(super) fn open_link(path: &Path, expected_target: &Path) -> io::Result<Option<OpenedLink>> {
    open_link_with_icon(path, expected_target, false)
}

pub(super) fn open_calculator_link(
    path: &Path,
    expected_target: &Path,
) -> io::Result<Option<OpenedLink>> {
    open_link_with_icon(path, expected_target, true)
}

fn open_link_with_icon(
    path: &Path,
    expected_target: &Path,
    require_icon: bool,
) -> io::Result<Option<OpenedLink>> {
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            GENERIC_READ | DELETE | READ_CONTROL | 0x0004_0000,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let mut file = unsafe { File::from_raw_handle(handle) };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.nNumberOfLinks != 1
        || file.metadata()?.len() > MAX_LINK_BYTES
        || !trusted_owner(&file)?
    {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_LINK_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_LINK_BYTES {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let target = super::shared_shortcuts_link::target_from_bytes(&bytes)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    if !paths_equal(&target, expected_target) {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    if require_icon
        && !super::shared_shortcuts_link::calculator_icon_matches(&bytes, expected_target)
    {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    let hash = Sha256::digest(&bytes);
    let identity = LinkIdentity {
        sha256: hash.iter().map(|byte| format!("{byte:02x}")).collect(),
        volume: info.dwVolumeSerialNumber,
        file_index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        attributes: info.dwFileAttributes,
    };
    Ok(Some(OpenedLink { file, identity }))
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left);
    let right = std::fs::canonicalize(right);
    match (left, right) {
        (Ok(left), Ok(right)) => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
        _ => false,
    }
}

fn trusted_owner(file: &File) -> io::Result<bool> {
    unsafe {
        let mut owner = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let status = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let mut system = [0u32; 17];
        let mut administrators = [0u32; 17];
        let mut size = 68;
        let system_ok = CreateWellKnownSid(
            WinLocalSystemSid,
            std::ptr::null_mut(),
            system.as_mut_ptr().cast(),
            &mut size,
        ) != 0;
        size = 68;
        let administrators_ok = CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            std::ptr::null_mut(),
            administrators.as_mut_ptr().cast(),
            &mut size,
        ) != 0;
        let trusted = system_ok
            && administrators_ok
            && !owner.is_null()
            && (EqualSid(owner, system.as_mut_ptr().cast()) != 0
                || EqualSid(owner, administrators.as_mut_ptr().cast()) != 0);
        LocalFree(descriptor);
        Ok(trusted)
    }
}

pub(super) fn move_link(link: &OpenedLink, destination: &Path) -> io::Result<()> {
    let name: Vec<u16> = destination.as_os_str().encode_wide().collect();
    let header = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let length = header + name.len() * 2;
    let words = length.div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; words];
    let rename = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*rename).Anonymous = FILE_RENAME_INFO_0 {
            ReplaceIfExists: false,
        };
        (*rename).RootDirectory = std::ptr::null_mut();
        (*rename).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*rename).FileName).cast::<u16>(),
            name.len(),
        );
        if SetFileInformationByHandle(
            link.file.as_raw_handle(),
            FileRenameInfo,
            rename.cast(),
            length as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub(super) fn delete_link(link: &OpenedLink) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            link.file.as_raw_handle(),
            FileDispositionInfo,
            std::ptr::addr_of!(info).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
#[path = "shared_shortcuts_file_tests.rs"]
mod tests;
