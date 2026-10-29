// SPDX-License-Identifier: AGPL-3.0-or-later
//! The relay's fixed image and parents must remain administrator/SYSTEM owned.

use super::PeerAuthError;
use std::{
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        CreateWellKnownSid, EqualSid, GetAce, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, INHERIT_ONLY_ACE,
        OWNER_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};

const WRITE_MASK: u32 = 0x500d_0156;

pub(super) fn verify_path(path: &Path) -> Result<(), PeerAuthError> {
    // ProgramData itself permits creation of unrelated subfolders. Only WinCommander,
    // its bin directory, and the image determine this already-provisioned machine pin.
    for part in path.ancestors().take(3) {
        if std::fs::symlink_metadata(part)
            .map_err(|_| PeerAuthError::IdentityUnavailable)?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err(PeerAuthError::PathNotAllowed);
        }
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | 0x0002_0000)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(part)
            .map_err(|_| PeerAuthError::IdentityUnavailable)?;
        unsafe {
            verify(file.as_raw_handle())?;
        }
    }
    Ok(())
}

struct Descriptor(*mut core::ffi::c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

unsafe fn verify(handle: *mut core::ffi::c_void) -> Result<(), PeerAuthError> {
    let deny = PeerAuthError::PathNotAllowed;
    let mut owner = std::ptr::null_mut();
    let mut acl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    // The descriptor owns the owner/ACL pointers until the guard is dropped below.
    if GetSecurityInfo(
        handle,
        SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        &mut owner,
        std::ptr::null_mut(),
        &mut acl,
        std::ptr::null_mut(),
        &mut descriptor,
    ) != 0
    {
        return Err(deny);
    }
    let _descriptor = Descriptor(descriptor);
    let mut system = [0u32; 17];
    let mut admins = [0u32; 17];
    let mut length = 68;
    if CreateWellKnownSid(
        WinLocalSystemSid,
        std::ptr::null_mut(),
        system.as_mut_ptr().cast(),
        &mut length,
    ) == 0
    {
        return Err(deny);
    }
    length = 68;
    if CreateWellKnownSid(
        WinBuiltinAdministratorsSid,
        std::ptr::null_mut(),
        admins.as_mut_ptr().cast(),
        &mut length,
    ) == 0
    {
        return Err(deny);
    }
    let trusted = |sid| {
        EqualSid(sid, system.as_ptr().cast_mut().cast()) != 0
            || EqualSid(sid, admins.as_ptr().cast_mut().cast()) != 0
    };
    if owner.is_null() || !trusted(owner) || acl.is_null() || (*acl).AceCount == 0 {
        return Err(deny);
    }
    for index in 0..(*acl).AceCount {
        let mut entry = std::ptr::null_mut();
        if GetAce(acl, index as u32, &mut entry) == 0 {
            return Err(deny);
        }
        let header = &*entry.cast::<windows_sys::Win32::Security::ACE_HEADER>();
        if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        if header.AceType != 0 {
            return Err(deny);
        }
        let entry = &*entry.cast::<ACCESS_ALLOWED_ACE>();
        let sid = std::ptr::addr_of!(entry.SidStart).cast_mut().cast();
        if entry.Mask & WRITE_MASK != 0 && !trusted(sid) {
            return Err(deny);
        }
    }
    Ok(())
}
