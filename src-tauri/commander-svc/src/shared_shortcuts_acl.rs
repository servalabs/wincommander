// SPDX-License-Identifier: AGPL-3.0-or-later
//! Temporarily protect a moved shared link while retaining its exact original DACL.

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SetSecurityInfo,
            SE_FILE_OBJECT,
        },
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    },
};

const PRIVATE_DACL: &str = "D:P(A;;FA;;;SY)(A;;FA;;;BA)";
const MAX_ACL: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LinkAcl {
    bytes: Vec<u8>,
    protected: bool,
}

pub(super) fn snapshot(file: &File) -> io::Result<LinkAcl> {
    unsafe {
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let status = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let result = (|| {
            if acl.is_null() {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            let size = (*acl).AclSize as usize;
            if !(std::mem::size_of::<ACL>()..=MAX_ACL).contains(&size) {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            let mut control = 0;
            let mut revision = 0;
            if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(LinkAcl {
                bytes: std::slice::from_raw_parts(acl.cast::<u8>(), size).to_vec(),
                protected: control & SE_DACL_PROTECTED != 0,
            })
        })();
        LocalFree(descriptor);
        result
    }
}

fn apply(file: &File, acl: &LinkAcl) -> io::Result<()> {
    let size = acl.bytes.len();
    if !(std::mem::size_of::<ACL>()..=MAX_ACL).contains(&size)
        || u16::from_ne_bytes([acl.bytes[2], acl.bytes[3]]) as usize != size
    {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut aligned = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        std::ptr::copy_nonoverlapping(acl.bytes.as_ptr(), aligned.as_mut_ptr().cast::<u8>(), size);
        let flag = if acl.protected {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        let status = SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | flag,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            aligned.as_ptr().cast::<ACL>(),
            std::ptr::null(),
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
    }
    Ok(())
}

pub(super) fn restrict(file: &File) -> io::Result<()> {
    let wide: Vec<u16> = PRIVATE_DACL.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut present = 0;
            let mut defaulted = 0;
            let mut acl: *mut ACL = std::ptr::null_mut();
            if GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) == 0 {
                return Err(io::Error::last_os_error());
            }
            if present == 0 || acl.is_null() {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            let status = SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl,
                std::ptr::null(),
            );
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            Ok(())
        })();
        LocalFree(descriptor);
        result
    }
}

pub(super) fn restore(file: &File, original: &LinkAcl) -> io::Result<()> {
    apply(file, original)?;
    if &snapshot(file)? != original {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, OPEN_EXISTING,
        },
    };

    #[test]
    fn repeated_recovery_steps_preserve_original_dacl_across_directory_moves() {
        let root = std::env::temp_dir().join(format!(
            "wincmd-shortcut-acl-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let public = root.join("public");
        let private = root.join("private");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::create_dir_all(&private).unwrap();
        let source = public.join("test.lnk");
        let backup = private.join("backup.lnk");
        std::fs::write(&source, b"same file, same bytes").unwrap();
        let original_attributes = std::fs::metadata(&source).unwrap().file_attributes();
        let wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0x8000_0000 | 0x0001_0000 | 0x0002_0000 | 0x0004_0000,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        let file = unsafe { File::from_raw_handle(handle) };
        let original = snapshot(&file).unwrap();
        let entry = crate::shared_shortcuts_file::OpenedLink {
            file,
            identity: crate::shared_shortcuts_file::LinkIdentity {
                sha256: String::new(),
                volume: 0,
                file_index: 0,
                attributes: 0,
            },
        };
        restrict(&entry.file).unwrap();
        // A restart after the journal and ACL update can repeat this step.
        restrict(&entry.file).unwrap();
        assert_ne!(snapshot(&entry.file).unwrap(), original);
        crate::shared_shortcuts_file::move_link(&entry, &backup).unwrap();
        // A restart after the move can also repeat the private ACL step.
        restrict(&entry.file).unwrap();
        assert!(!source.exists());
        crate::shared_shortcuts_file::move_link(&entry, &source).unwrap();
        restore(&entry.file, &original).unwrap();
        restore(&entry.file, &original).unwrap();
        assert_eq!(snapshot(&entry.file).unwrap(), original);
        assert_eq!(std::fs::read(&source).unwrap(), b"same file, same bytes");
        assert_eq!(
            std::fs::metadata(&source).unwrap().file_attributes(),
            original_attributes
        );
        drop(entry);
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(public).unwrap();
        std::fs::remove_dir(private).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
