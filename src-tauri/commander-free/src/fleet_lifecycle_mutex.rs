// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cross-session Fleet admission. This mutex is coordination, not authorization.
use std::{marker::PhantomData, rc::Rc};

/// A Windows mutex must be released by its owning OS thread.
pub(super) struct Guard {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    _thread_bound: PhantomData<Rc<()>>,
}

pub(super) fn acquire() -> Result<Guard, String> {
    #[cfg(windows)]
    {
        acquire_named(r"Global\WinCommander.Fleet.Lifecycle.v2", 5_000)
    }
    #[cfg(not(windows))]
    {
        Ok(Guard {
            _thread_bound: PhantomData,
        })
    }
}

#[cfg(windows)]
const ACCESS: u32 = 0x0012_0001; // SYNCHRONIZE | READ_CONTROL | MUTEX_MODIFY_STATE
#[cfg(windows)]
const ALL: u32 = 0x001f_0001;
#[cfg(windows)]
const SDDL: &str = "D:P(A;;0x00120001;;;OW)(A;;0x00120001;;;AU)(A;;0x001f0001;;;SY)(A;;0x001f0001;;;BA)S:(ML;;NW;;;ME)";
#[cfg(windows)]
const UNAVAILABLE: &str = "Fleet connection coordination is unavailable or unsafe. Close older WinCommander sessions and retry.";

#[cfg(windows)]
struct Descriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);
#[cfg(windows)]
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_named(name: &str, timeout_ms: u32) -> Result<Guard, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
        System::Threading::{CreateMutexExW, WaitForSingleObject},
    };
    let sddl: Vec<u16> = SDDL.encode_utf16().chain(Some(0)).collect();
    let mut raw_descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut raw_descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(UNAVAILABLE.into());
    }
    let descriptor = Descriptor(raw_descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { CreateMutexExW(&attributes, name.as_ptr(), 0, ACCESS) };
    if handle.is_null() {
        return Err(UNAVAILABLE.into());
    }
    if verify(handle).is_err() {
        unsafe {
            CloseHandle(handle);
        }
        return Err(UNAVAILABLE.into());
    }
    match unsafe { WaitForSingleObject(handle, timeout_ms) } {
        // An abandoned mutex transfers ownership. Callers re-read settings and
        // verify the current Pro/service bindings after acquiring this guard.
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Guard {
            handle,
            _thread_bound: PhantomData,
        }),
        WAIT_TIMEOUT => {
            unsafe {
                CloseHandle(handle);
            }
            Err("A Fleet connection change is already in progress. Retry shortly.".into())
        }
        _ => {
            unsafe {
                CloseHandle(handle);
            }
            Err(UNAVAILABLE.into())
        }
    }
}

#[cfg(windows)]
fn verify(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<(), ()> {
    use windows_sys::Win32::Security::{
        Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
        CreateWellKnownSid, EqualSid, GetAce, GetSecurityDescriptorControl,
        GetSecurityDescriptorSacl, IsValidSid, WinAuthenticatedUserSid,
        WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid, WinLocalSystemSid,
        WinMediumLabelSid, ACL, DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
    };
    unsafe {
        let mut owner = std::ptr::null_mut();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut raw = std::ptr::null_mut();
        if GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut raw,
        ) != 0
        {
            return Err(());
        }
        let descriptor = Descriptor(raw);
        if owner.is_null() || IsValidSid(owner) == 0 || dacl.is_null() || (*dacl).AceCount != 4 {
            return Err(());
        }
        let mut control = 0;
        let mut revision = 0;
        if GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) == 0
            || control & SE_DACL_PROTECTED == 0
        {
            return Err(());
        }
        let mut expected = [[0u32; 17]; 5];
        for (sid, kind) in expected.iter_mut().zip([
            WinCreatorOwnerRightsSid,
            WinAuthenticatedUserSid,
            WinLocalSystemSid,
            WinBuiltinAdministratorsSid,
            WinMediumLabelSid,
        ]) {
            let mut len = 68;
            if CreateWellKnownSid(
                kind,
                std::ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                &mut len,
            ) == 0
            {
                return Err(());
            }
        }
        let mut found = [false; 4];
        for i in 0..(*dacl).AceCount {
            let mut raw_ace = std::ptr::null_mut();
            if GetAce(dacl, u32::from(i), &mut raw_ace) == 0 || raw_ace.is_null() {
                return Err(());
            }
            let (sid, mask) = ace_sid(dacl, raw_ace, 0)?;
            if IsValidSid(sid) == 0 {
                return Err(());
            }
            let index = (0..4)
                .find(|&j| EqualSid(sid, expected[j].as_ptr().cast_mut().cast()) != 0)
                .ok_or(())?;
            if found[index] || mask != if index < 2 { ACCESS } else { ALL } {
                return Err(());
            }
            found[index] = true;
        }
        if found.iter().any(|found| !found) {
            return Err(());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut sacl: *mut ACL = std::ptr::null_mut();
        if GetSecurityDescriptorSacl(descriptor.0, &mut present, &mut sacl, &mut defaulted) == 0
            || present == 0
            || sacl.is_null()
            || (*sacl).AceCount != 1
        {
            return Err(());
        }
        let mut raw_ace = std::ptr::null_mut();
        if GetAce(sacl, 0, &mut raw_ace) == 0 || raw_ace.is_null() {
            return Err(());
        }
        let (sid, mask) = ace_sid(sacl, raw_ace, 0x11)?;
        if mask != 1
            || IsValidSid(sid) == 0
            || EqualSid(sid, expected[4].as_ptr().cast_mut().cast()) == 0
        {
            return Err(());
        }
    }
    Ok(())
}

/// Bound all reads before asking Windows to inspect an ACE from an existing
/// named object; its security descriptor may have been supplied by another user.
#[cfg(windows)]
unsafe fn ace_sid(
    acl: *const windows_sys::Win32::Security::ACL,
    raw: *mut std::ffi::c_void,
    expected_type: u8,
) -> Result<(*mut std::ffi::c_void, u32), ()> {
    let base = acl as usize;
    let end = base.checked_add(usize::from((*acl).AclSize)).ok_or(())?;
    let start = raw as usize;
    if start
        < base
            .checked_add(std::mem::size_of::<windows_sys::Win32::Security::ACL>())
            .ok_or(())?
        || start.checked_add(16).ok_or(())? > end
    {
        return Err(());
    }
    let header = &*raw.cast::<windows_sys::Win32::Security::ACE_HEADER>();
    let size = usize::from(header.AceSize);
    if header.AceType != expected_type
        || header.AceFlags != 0
        || size < 16
        || start.checked_add(size).ok_or(())? > end
    {
        return Err(());
    }
    let sid = raw.cast::<u8>().add(8);
    let subauthorities = usize::from(*sid.add(1));
    if subauthorities > 15 || 16 + 4 * subauthorities != size {
        return Err(());
    }
    let mask = std::ptr::read_unaligned(raw.cast::<u8>().add(4).cast::<u32>());
    Ok((sid.cast(), mask))
}

#[cfg(windows)]
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.handle);
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(all(test, windows))]
#[path = "fleet_lifecycle_mutex_tests.rs"]
mod tests;
