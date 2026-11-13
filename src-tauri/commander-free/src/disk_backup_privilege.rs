//! Thread-local backup privilege for no-follow protected-root discovery only.
use std::{ffi::c_void, io, ptr};

type Handle = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct Luid { low: u32, high: i32 }
#[repr(C)]
struct TokenPrivileges { count: u32, luid: Luid, attributes: u32 }

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentThread() -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetLastError() -> u32;
    fn SetLastError(code: u32);
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn OpenThreadToken(thread: Handle, access: u32, open_as_self: i32, token: *mut Handle) -> i32;
    fn DuplicateTokenEx(existing: Handle, access: u32, attributes: *mut c_void, level: i32, token_type: i32, token: *mut Handle) -> i32;
    fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
    fn AdjustTokenPrivileges(token: Handle, disable_all: i32, new_state: *const TokenPrivileges, buffer_length: u32, previous: *mut c_void, return_length: *mut u32) -> i32;
    fn GetTokenInformation(token: Handle, class: u32, information: *mut c_void, length: u32, return_length: *mut u32) -> i32;
    fn SetThreadToken(thread: *const Handle, token: Handle) -> i32;
    fn RevertToSelf() -> i32;
}

struct OwnedHandle(Handle);
impl Drop for OwnedHandle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
struct ThreadImpersonation;
impl Drop for ThreadImpersonation {
    fn drop(&mut self) {
        // Continuing on a thread with elevated rights is less safe than stopping.
        if unsafe { RevertToSelf() } == 0 { std::process::abort(); }
    }
}

/// Validate the duplicate itself before impersonating it. A token inherited
/// from an elevated process can carry several enabled privileges; discovery
/// must not inherit those capabilities into its backup-only impersonation.
fn verify_only_backup_enabled(token: Handle, backup: Luid) -> io::Result<()> {
    const TOKEN_PRIVILEGES_CLASS: u32 = 3;
    const SE_PRIVILEGE_ENABLED: u32 = 2;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LuidAndAttributes { luid: Luid, attributes: u32 }
    let mut length = 0u32;
    unsafe { GetTokenInformation(token, TOKEN_PRIVILEGES_CLASS, ptr::null_mut(), 0, &mut length); }
    if length < 4 || length > 65536 { return Err(io::Error::other("token privilege inventory unavailable")); }
    let mut storage = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token, TOKEN_PRIVILEGES_CLASS, storage.as_mut_ptr().cast(), length, &mut length) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes = unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), length as usize) };
    let count = u32::from_ne_bytes(bytes.get(..4).ok_or_else(|| io::Error::other("invalid token privileges"))?.try_into().map_err(|_| io::Error::other("invalid token privileges"))?) as usize;
    let size = std::mem::size_of::<LuidAndAttributes>();
    if count > (bytes.len() - 4) / size { return Err(io::Error::other("invalid token privileges")); }
    let mut enabled_backup = false;
    for index in 0..count {
        let entry = unsafe { std::ptr::read_unaligned(bytes.as_ptr().add(4 + index * size).cast::<LuidAndAttributes>()) };
        if entry.attributes & SE_PRIVILEGE_ENABLED != 0 {
            if entry.luid.low != backup.low || entry.luid.high != backup.high || enabled_backup {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "unexpected enabled token privilege"));
            }
            enabled_backup = true;
        }
    }
    if !enabled_backup { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "backup privilege unavailable")); }
    Ok(())
}

/// Synchronous only: an existing thread impersonation is never replaced.
pub(crate) fn with_backup_privilege<T>(f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_DUPLICATE: u32 = 0x0002;
    const TOKEN_IMPERSONATE: u32 = 0x0004;
    const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
    const ERROR_NO_TOKEN: u32 = 1008;
    let mut existing: Handle = ptr::null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut existing) } != 0 {
        drop(OwnedHandle(existing));
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "existing thread impersonation is unsupported"));
    }
    if unsafe { GetLastError() } != ERROR_NO_TOKEN { return Err(io::Error::last_os_error()); }
    let mut process: Handle = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut process) } == 0 { return Err(io::Error::last_os_error()); }
    let process = OwnedHandle(process);
    let mut duplicate: Handle = ptr::null_mut();
    if unsafe { DuplicateTokenEx(process.0, TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES | TOKEN_IMPERSONATE, ptr::null_mut(), 2, 2, &mut duplicate) } == 0 { return Err(io::Error::last_os_error()); }
    let duplicate = OwnedHandle(duplicate);
    if unsafe { AdjustTokenPrivileges(duplicate.0, 1, ptr::null(), 0, ptr::null_mut(), ptr::null_mut()) } == 0 { return Err(io::Error::last_os_error()); }
    let name: Vec<u16> = "SeBackupPrivilege\0".encode_utf16().collect();
    let mut luid = Luid { low: 0, high: 0 };
    if unsafe { LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &mut luid) } == 0 { return Err(io::Error::last_os_error()); }
    let state = TokenPrivileges { count: 1, luid, attributes: 2 };
    unsafe { SetLastError(0); }
    if unsafe { AdjustTokenPrivileges(duplicate.0, 0, &state, 0, ptr::null_mut(), ptr::null_mut()) } == 0 { return Err(io::Error::last_os_error()); }
    let error = unsafe { GetLastError() };
    if error != 0 { return Err(io::Error::from_raw_os_error(error as i32)); }
    verify_only_backup_enabled(duplicate.0, luid)?;
    if unsafe { SetThreadToken(ptr::null(), duplicate.0) } == 0 { return Err(io::Error::last_os_error()); }
    let guard = ThreadImpersonation;
    let result = f();
    drop(guard);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread_has_token() -> bool {
        let mut token = ptr::null_mut();
        let opened = unsafe { OpenThreadToken(GetCurrentThread(), 0x0008, 1, &mut token) } != 0;
        if opened { drop(OwnedHandle(token)); }
        opened
    }

    // This test requires an administrator token with SeBackupPrivilege.
    #[test]
    #[ignore = "requires administrator backup privilege"]
    fn privilege_is_reverted_after_success_error_and_unwind() {
        assert!(!thread_has_token());
        with_backup_privilege(|| { assert!(thread_has_token()); Ok(()) }).unwrap();
        assert!(!thread_has_token());
        assert!(with_backup_privilege::<()>(|| Err(io::Error::other("fixture"))).is_err());
        assert!(!thread_has_token());
        let panic = std::panic::catch_unwind(|| { let _ = with_backup_privilege::<()>(|| panic!("fixture")); });
        assert!(panic.is_err());
        assert!(!thread_has_token());
        with_backup_privilege(|| {
            assert!(with_backup_privilege(|| Ok(())).is_err());
            assert!(thread_has_token());
            Ok(())
        }).unwrap();
        assert!(!thread_has_token());
    }
}
