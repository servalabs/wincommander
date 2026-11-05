// SPDX-License-Identifier: AGPL-3.0-or-later
//! Exclusive ownership of the protected machine Fleet store. A held file
//! handle fences cooperating service processes, not older non-cooperating code.
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
};
use windows_sys::Win32::Foundation::{
    LocalFree, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_ALWAYS,
};

const LOCK_FILE: &str = ".fleet-machine-owner.lock";
const UNAVAILABLE: &str = "fleet_owner_unavailable";

/// Holds both the directory and exclusive lock-file handles for the complete
/// store lifetime. Neither handle permits rename or deletion while held.
pub(super) struct OwnerLease {
    _directory: File,
    _lock: File,
}

fn verify_handle(file: &File, directory: bool) -> Result<(), &'static str> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // The opened file owns the handle for the duration of this synchronous call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(UNAVAILABLE);
    }
    let is_dir = info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || is_dir != directory {
        return Err(UNAVAILABLE);
    }
    if !directory && (info.nNumberOfLinks != 1 || info.nFileSizeHigh != 0 || info.nFileSizeLow != 0)
    {
        return Err(UNAVAILABLE);
    }
    crate::peer_auth_deployment_acl::verify_file_handle(file).map_err(|_| UNAVAILABLE)
}

pub(super) fn acquire(directory: &Path) -> Result<OwnerLease, &'static str> {
    if !directory.is_absolute() {
        return Err(UNAVAILABLE);
    }
    let parent = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | 0x0002_0000) // READ_CONTROL
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(directory)
        .map_err(|_| UNAVAILABLE)?;
    verify_handle(&parent, true)?;
    // The directory is protected, but its ACL may not have inheritable ACEs.
    // Supply a protected SYSTEM/Administrators DACL when creating the file;
    // Windows ignores this descriptor for an existing file, which is checked
    // through the exclusive handle below and never modified or truncated.
    let sddl: Vec<u16> = "D:P(A;;FA;;;SY)(A;;FA;;;BA)"
        .encode_utf16()
        .chain(Some(0))
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
        return Err(UNAVAILABLE);
    }
    struct Descriptor(*mut core::ffi::c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let descriptor = Descriptor(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let path: Vec<u16> = directory
        .join(LOCK_FILE)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            &attributes,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(UNAVAILABLE);
    }
    // CreateFileW returned a unique owned handle; File closes it on every exit.
    let lock = unsafe { File::from_raw_handle(handle) };
    verify_handle(&lock, false)?;
    Ok(OwnerLease {
        _directory: parent,
        _lock: lock,
    })
}

#[cfg(test)]
#[path = "fleet_machine_owner_tests.rs"]
mod tests;
