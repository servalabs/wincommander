// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fail-closed registry ACL policy for the experimental update anchor.
use std::{ffi::c_void, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_REGISTRY_KEY,
        },
        CreateWellKnownSid, EqualSid, GetAce, GetSecurityDescriptorControl, IsValidSid,
        WinBuiltinAdministratorsSid, WinLocalSystemSid, ACL, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES, SE_DACL_PROTECTED,
    },
    System::Registry::{HKEY, KEY_ALL_ACCESS},
};

pub(super) struct Security {
    descriptor: *mut c_void,
    attributes: SECURITY_ATTRIBUTES,
}
impl Security {
    pub(super) fn new() -> Result<Self, String> {
        // Administrator ownership prevents a restricted owner from rewriting the DACL.
        let sddl: Vec<u16> = "O:BAD:P(A;;KA;;;SY)(A;;KA;;;BA)"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err("Cannot prepare Free update registry security".into());
        }
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }
    pub(super) fn attributes(&self) -> *const SECURITY_ATTRIBUTES {
        &self.attributes
    }
}
impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.descriptor);
        }
    }
}
struct Descriptor(*mut c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// Accept only the exact administrator-owned, protected two-principal DACL.
pub(super) fn verify(key: HKEY) -> Result<(), String> {
    unsafe {
        let mut owner = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        if GetSecurityInfo(
            key,
            SE_REGISTRY_KEY,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        ) != 0
        {
            return Err("Cannot inspect Free update registry security".into());
        }
        let descriptor = Descriptor(descriptor);
        if owner.is_null() || IsValidSid(owner) == 0 || dacl.is_null() || (*dacl).AceCount != 2 {
            return Err("Free update registry security is unsafe".into());
        }
        let mut control = 0;
        let mut revision = 0;
        if GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) == 0
            || control & SE_DACL_PROTECTED == 0
        {
            return Err("Free update registry security is unsafe".into());
        }
        let mut expected = [[0u32; 17]; 2];
        for (sid, kind) in expected
            .iter_mut()
            .zip([WinLocalSystemSid, WinBuiltinAdministratorsSid])
        {
            let mut len = 68;
            if CreateWellKnownSid(kind, ptr::null_mut(), sid.as_mut_ptr().cast(), &mut len) == 0 {
                return Err("Cannot inspect Free update registry security".into());
            }
        }
        if EqualSid(owner, expected[1].as_ptr().cast_mut().cast()) == 0 {
            return Err("Free update registry owner is unsafe".into());
        }
        let mut found = [false; 2];
        for i in 0..(*dacl).AceCount {
            let mut ace = ptr::null_mut();
            if GetAce(dacl, u32::from(i), &mut ace) == 0 || ace.is_null() {
                return Err("Free update registry security is unsafe".into());
            }
            let (sid, mask) =
                ace_sid(dacl, ace).ok_or("Free update registry security is unsafe")?;
            if IsValidSid(sid) == 0 {
                return Err("Free update registry security is unsafe".into());
            }
            let index = (0..2)
                .find(|&n| EqualSid(sid, expected[n].as_ptr().cast_mut().cast()) != 0)
                .ok_or("Free update registry security is unsafe")?;
            if found[index] || mask != KEY_ALL_ACCESS {
                return Err("Free update registry security is unsafe".into());
            }
            found[index] = true;
        }
        if found.iter().any(|found| !found) {
            return Err("Free update registry security is unsafe".into());
        }
    }
    Ok(())
}

/// Bound the ACE and SID before asking Windows to inspect a potentially hostile descriptor.
unsafe fn ace_sid(acl: *mut ACL, raw: *mut c_void) -> Option<(*mut c_void, u32)> {
    use windows_sys::Win32::Security::ACE_HEADER;
    let base = acl as usize;
    let end = base.checked_add(usize::from((*acl).AclSize))?;
    let start = raw as usize;
    if start < base.checked_add(std::mem::size_of::<ACL>())? || start.checked_add(16)? > end {
        return None;
    }
    let header = &*raw.cast::<ACE_HEADER>();
    let size = usize::from(header.AceSize);
    if header.AceType != 0 || header.AceFlags != 0 || size < 16 || start.checked_add(size)? > end {
        return None;
    }
    let sid = raw.cast::<u8>().add(8);
    let subauthorities = usize::from(*sid.add(1));
    if subauthorities > 15 || 16 + 4 * subauthorities != size {
        return None;
    }
    Some((
        sid.cast(),
        ptr::read_unaligned(raw.cast::<u8>().add(4).cast::<u32>()),
    ))
}
