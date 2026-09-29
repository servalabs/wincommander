// SPDX-License-Identifier: AGPL-3.0-or-later
//! Let a connecting local account inspect only this service process's identity.
//! This pre-Hello metadata grant is NOT request authorization. Every command
//! still uses the exact post-Hello named-pipe impersonation token in pipe.rs.
use std::collections::HashSet;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Mutex;
use windows_sys::Win32::{
    Foundation::{LocalFree, HANDLE},
    Security::{
        Authorization::{
            GetSecurityInfo, SetEntriesInAclW, SetSecurityInfo, EXPLICIT_ACCESS_W, GRANT_ACCESS,
            NO_MULTIPLE_TRUSTEE, SE_KERNEL_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
        },
        GetAce, GetLengthSid, GetTokenInformation, IsValidSid, TokenUser, ACE_HEADER, ACL,
        DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        Pipes::GetNamedPipeClientProcessId,
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

const MAX_CALLER_GRANTS: usize = 256;
static GRANTED: Mutex<Option<HashSet<Vec<u8>>>> = Mutex::new(None);

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "service peer metadata unavailable",
    )
}

struct LocalAllocation(*mut core::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn pipe_pid(pipe: HANDLE) -> io::Result<u32> {
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 || pid == 0 {
        return Err(denied());
    }
    Ok(pid)
}

fn token_user(token: HANDLE) -> io::Result<Vec<usize>> {
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed > 65536 {
        return Err(denied());
    }
    // TOKEN_USER contains a pointer, so byte/u32 storage is not sufficiently
    // aligned on x64. The SID pointer remains valid while this buffer is held.
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(denied());
    }
    Ok(buffer)
}

pub(super) fn allow_connected_account(pipe: HANDLE) -> io::Result<()> {
    let pid = pipe_pid(pipe)?;
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(denied());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(denied());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let buffer = token_user(token.as_raw_handle())?;
    let sid = unsafe { (&*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    if unsafe { IsValidSid(sid) } == 0 || pipe_pid(pipe)? != pid {
        return Err(denied());
    }
    let length = unsafe { GetLengthSid(sid) } as usize;
    let identity = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), length) }.to_vec();
    let mut cache = GRANTED.lock().map_err(|_| denied())?;
    let granted = cache.get_or_insert_with(HashSet::new);
    if granted.contains(&identity) {
        return Ok(());
    }
    if granted.len() >= MAX_CALLER_GRANTS {
        return Err(denied());
    }
    grant_query_only(unsafe { GetCurrentProcess() }, sid)?;
    granted.insert(identity);
    Ok(())
}

fn grant_query_only(process: HANDLE, sid: *mut core::ffi::c_void) -> io::Result<()> {
    let mut old_acl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    let result = unsafe {
        GetSecurityInfo(
            process,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old_acl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(denied());
    }
    let _descriptor = LocalAllocation(descriptor);
    // Never turn a missing/null DACL into an implicit broad grant.
    if old_acl.is_null() {
        return Err(denied());
    }
    // SetEntriesInAcl(GRANT_ACCESS) can reorder/merge a same-SID deny. A custom
    // process restriction must never be weakened just to make the app start.
    // The service's default DACL contains ordinary allow ACEs only; leave any
    // deny, callback, or object-specific policy byte-for-byte untouched.
    for index in 0..unsafe { (*old_acl).AceCount } {
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(old_acl, index as u32, &mut ace) } == 0
            || unsafe { (*ace.cast::<ACE_HEADER>()).AceType } != 0
        {
            return Err(denied());
        }
    }
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: PROCESS_QUERY_LIMITED_INFORMATION,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: 0,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        },
    };
    let mut acl = std::ptr::null_mut();
    if unsafe { SetEntriesInAclW(1, &entry, old_acl, &mut acl) } != 0 {
        return Err(denied());
    }
    let _acl = LocalAllocation(acl.cast());
    if unsafe {
        SetSecurityInfo(
            process,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Err(denied());
    }
    Ok(())
}

#[cfg(test)]
#[path = "service_peer_query_tests.rs"]
mod tests;
