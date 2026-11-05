// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unwired installer/service seam for a pre-provisioned machine lock file.
//! The caller must establish the product root's authority and keep every root
//! ancestor pinned without delete sharing throughout this operation. This does
//! not enable generation cutover or modify an existing file's ACL.
#![allow(dead_code)]
use std::{fs::File, io, os::windows::{ffi::OsStrExt, io::{AsRawHandle, FromRawHandle}}, path::Path};
use windows_sys::Win32::{
    Foundation::{LocalFree, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE},
    Security::{Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_FILE_OBJECT},
        EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES},
    Storage::FileSystem::{CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, CREATE_NEW, OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE},
};

// Explicit non-inheriting protected DACL: SYSTEM/Admin full, Users read.
const LOCK_SDDL: &str = "O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)";
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const DACL_PROTECTED: u16 = 0x1000;

struct Descriptor(*mut core::ffi::c_void);
impl Descriptor {
    fn expected() -> io::Result<Self> { Self::from_sddl(LOCK_SDDL) }
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut value = std::ptr::null_mut();
        // SAFETY: text is NUL terminated and Windows initializes value on success.
        if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), 1, &mut value, std::ptr::null_mut()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(value))
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0, bInheritHandle: 0 }
    }
}
impl Drop for Descriptor { fn drop(&mut self) { unsafe { LocalFree(self.0); } } }

// Validate the SID fits inside the ACE before EqualSid may inspect it.
unsafe fn checked_allowed_ace<'a>(ptr: *const core::ffi::c_void) -> Option<&'a ACCESS_ALLOWED_ACE> {
    let header = &*ptr.cast::<ACE_HEADER>();
    let offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
    let size = header.AceSize as usize;
    if header.AceType != 0 || header.AceFlags != 0 || size < offset + 8 { return None; }
    let sid = ptr.cast::<u8>().add(offset);
    let revision = *sid;
    let subauthorities = *sid.add(1) as usize;
    if revision != 1 || subauthorities > 15 || offset + 8 + subauthorities * 4 > size { return None; }
    Some(&*ptr.cast::<ACCESS_ALLOWED_ACE>())
}

// Compare the actual ACEs with the descriptor built from SDDL, regardless of
// ordering. Reject unknown ACE forms, inherited/extra permissions and owners.
unsafe fn acl_matches(actual: *mut ACL, expected: *mut ACL) -> io::Result<bool> {
    if actual.is_null() || expected.is_null() || (*actual).AceCount != (*expected).AceCount { return Ok(false); }
    let mut seen = vec![false; (*expected).AceCount as usize];
    for i in 0..(*actual).AceCount {
        let mut p = std::ptr::null_mut();
        if GetAce(actual, i as u32, &mut p) == 0 || p.is_null() { return Err(io::Error::last_os_error()); }
        let Some(a) = checked_allowed_ace(p) else { return Ok(false); };
        let mut matched = false;
        for j in 0..(*expected).AceCount {
            let mut q = std::ptr::null_mut();
            if GetAce(expected, j as u32, &mut q) == 0 || q.is_null() { return Err(io::Error::last_os_error()); }
            let Some(e) = checked_allowed_ace(q) else { return Ok(false); };
            if !seen[j as usize] && a.Mask == e.Mask && EqualSid(std::ptr::addr_of!(a.SidStart).cast_mut().cast(), std::ptr::addr_of!(e.SidStart).cast_mut().cast()) != 0 {
                seen[j as usize] = true; matched = true; break;
            }
        }
        if !matched { return Ok(false); }
    }
    Ok(true)
}

fn verify(file: &File, expected: &Descriptor) -> io::Result<()> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: all handles and output structures remain live through calls.
    unsafe {
        if GetFileInformationByHandle(file.as_raw_handle(), &mut info) == 0 { return Err(io::Error::last_os_error()); }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || info.nNumberOfLinks != 1
            || info.dwFileAttributes & 0x10 != 0 { return Err(io::Error::other("invalid settings lock file")); }
        let mut owner = std::ptr::null_mut();
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut raw_descriptor = std::ptr::null_mut();
        let status = GetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner, std::ptr::null_mut(), &mut acl, std::ptr::null_mut(), &mut raw_descriptor);
        if status != 0 { return Err(io::Error::from_raw_os_error(status as i32)); }
        let actual = Descriptor(raw_descriptor);
        let mut control = 0u16; let mut revision = 0u32;
        if GetSecurityDescriptorControl(actual.0, &mut control, &mut revision) == 0 { return Err(io::Error::last_os_error()); }
        let mut expected_owner = std::ptr::null_mut(); let mut owner_defaulted = 0;
        let mut expected_acl = std::ptr::null_mut(); let mut present = 0; let mut defaulted = 0;
        if GetSecurityDescriptorOwner(expected.0, &mut expected_owner, &mut owner_defaulted) == 0
            || GetSecurityDescriptorDacl(expected.0, &mut present, &mut expected_acl, &mut defaulted) == 0 {
            return Err(io::Error::last_os_error());
        }
        if control & DACL_PROTECTED == 0 || owner.is_null() || expected_owner.is_null() || present == 0
            || EqualSid(owner, expected_owner) == 0 || !acl_matches(acl, expected_acl)? {
            return Err(io::Error::other("settings lock permissions are not authorized"));
        }
    }
    Ok(())
}

/// Verify the exact still-open, no-delete-sharing handle that will be locked.
/// The caller must also pin and authorize the parent directory and its ACL.
pub(super) fn verify_machine_handle(file: &File) -> io::Result<()> {
    verify(file, &Descriptor::expected()?)
}

/// Create once or verify an existing lock; never replace or repair an unknown
/// descriptor. Call only from an elevated, gated provisioning flow after the
/// caller has authorized and pinned the product root and its ancestors.
pub(super) fn provision_or_verify(root: &Path) -> io::Result<()> {
    let descriptor = Descriptor::expected()?;
    let mut attributes = descriptor.attributes();
    let name: Vec<u16> = root.join(".settings-generation.lock").as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: the path and security attributes remain valid through CreateFileW.
    let mut handle = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE, &mut attributes, CREATE_NEW,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT, std::ptr::null_mut()) };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) { return Err(error); }
        // Do not alter a pre-existing file: inspect its held handle only.
        handle = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT, std::ptr::null_mut()) };
        if handle == INVALID_HANDLE_VALUE { return Err(io::Error::last_os_error()); }
    }
    // SAFETY: CreateFileW returned a unique owned file handle.
    let file = unsafe { File::from_raw_handle(handle) };
    verify(&file, &descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn descriptor_acl(d: &Descriptor) -> *mut ACL {
        let mut acl = std::ptr::null_mut(); let mut present = 0; let mut defaulted = 0;
        assert_ne!(GetSecurityDescriptorDacl(d.0, &mut present, &mut acl, &mut defaulted), 0);
        assert_ne!(present, 0);
        acl
    }
    #[test]
    fn exact_policy_rejects_extra_or_overprivileged_rules() {
        let expected = Descriptor::expected().unwrap();
        let exact = Descriptor::from_sddl(LOCK_SDDL).unwrap();
        let extra = Descriptor::from_sddl("O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)(A;;FR;;;AU)").unwrap();
        let overprivileged = Descriptor::from_sddl("O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;BU)").unwrap();
        unsafe {
            assert!(acl_matches(descriptor_acl(&exact), descriptor_acl(&expected)).unwrap());
            assert!(!acl_matches(descriptor_acl(&extra), descriptor_acl(&expected)).unwrap());
            assert!(!acl_matches(descriptor_acl(&overprivileged), descriptor_acl(&expected)).unwrap());
        }
    }
    #[test]
    fn malformed_sid_is_rejected_before_sid_comparison() {
        let mut memory = [0u32; 8];
        let header = memory.as_mut_ptr().cast::<ACE_HEADER>();
        unsafe {
            (*header).AceType = 0;
            (*header).AceFlags = 0;
            (*header).AceSize = 8;
            assert!(checked_allowed_ace(header.cast()).is_none());
            (*header).AceSize = 16;
            let sid = header.cast::<u8>().add(std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart));
            *sid = 1; *sid.add(1) = 2;
            assert!(checked_allowed_ace(header.cast()).is_none());
        }
    }
    // Requires an elevated Windows test process; run separately as an elevated
    // Administrator and as LocalSystem before the installer can use this seam.
    #[test]
    fn elevated_provision_is_idempotent_and_rejects_foreign_acl_without_modification() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        provision_or_verify(root.path()).unwrap();
        provision_or_verify(root.path()).unwrap();
        let file = std::fs::File::open(root.path().join(".settings-generation.lock")).unwrap();
        verify_machine_handle(&file).unwrap();
        drop(file);
        let other = tempfile::tempdir().unwrap();
        let name: Vec<u16> = other.path().join(".settings-generation.lock").as_os_str().encode_wide().chain(Some(0)).collect();
        let descriptor = Descriptor::from_sddl("D:P(A;;FA;;;WD)").unwrap();
        let mut attrs = descriptor.attributes();
        let handle = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, &mut attrs, CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT, std::ptr::null_mut()) };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        let mut foreign = unsafe { File::from_raw_handle(handle) };
        foreign.write_all(b"preserve foreign bytes").unwrap();
        drop(foreign);
        assert!(provision_or_verify(other.path()).is_err());
        assert_eq!(std::fs::read(other.path().join(".settings-generation.lock")).unwrap(), b"preserve foreign bytes");
    }

}
