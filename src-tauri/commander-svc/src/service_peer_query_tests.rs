// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::io::BufRead;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use windows_sys::Win32::Security::{
    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, CreateRestrictedToken,
    CreateWellKnownSid, EqualSid, GetAce, ImpersonateLoggedOnUser, RevertToSelf,
    SetKernelObjectSecurity, WinBuiltinAdministratorsSid, ACCESS_ALLOWED_ACE,
    DISABLE_MAX_PRIVILEGE, SID_AND_ATTRIBUTES, TOKEN_DUPLICATE,
};

fn inspect_exact_grant_and_add_deny(process: HANDLE, sid: *mut core::ffi::c_void) {
    use windows_sys::Win32::Security::Authorization::DENY_ACCESS;
    let mut acl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                process,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut acl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    let _descriptor = LocalAllocation(descriptor);
    assert_eq!(
        unsafe { (*acl).AceCount },
        3,
        "two original grants plus one exact caller"
    );
    let mut count = 0;
    for index in 0..unsafe { (*acl).AceCount } {
        let mut ace = std::ptr::null_mut();
        assert_ne!(unsafe { GetAce(acl, index as u32, &mut ace) }, 0);
        let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        if unsafe { EqualSid(std::ptr::addr_of!(ace.SidStart).cast_mut().cast(), sid) } != 0 {
            count += 1;
            assert_eq!(ace.Mask, PROCESS_QUERY_LIMITED_INFORMATION);
            assert_eq!(ace.Header.AceFlags, 0, "no inheritance");
            assert_eq!(ace.Header.AceType, 0, "allow only");
        }
    }
    assert_eq!(count, 1);
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: PROCESS_QUERY_LIMITED_INFORMATION,
        grfAccessMode: DENY_ACCESS,
        grfInheritance: 0,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        },
    };
    let mut replacement = std::ptr::null_mut();
    assert_eq!(
        unsafe { SetEntriesInAclW(1, &entry, acl, &mut replacement) },
        0
    );
    let _replacement = LocalAllocation(replacement.cast());
    assert_eq!(
        unsafe {
            SetSecurityInfo(
                process,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                replacement,
                std::ptr::null_mut(),
            )
        },
        0
    );
}

struct Revert;
impl Drop for Revert {
    fn drop(&mut self) {
        assert_ne!(unsafe { RevertToSelf() }, 0);
    }
}

fn restricted<T>(test: impl FnOnce() -> T) -> T {
    let mut token = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY,
                &mut token,
            )
        },
        0
    );
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut sid = [0u32; 17];
    let mut size = std::mem::size_of_val(&sid) as u32;
    assert_ne!(
        unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                std::ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                &mut size,
            )
        },
        0
    );
    let disable = SID_AND_ATTRIBUTES {
        Sid: sid.as_mut_ptr().cast(),
        Attributes: 0,
    };
    let mut result = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            CreateRestrictedToken(
                token.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                1,
                &disable,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut result,
            )
        },
        0
    );
    let result = unsafe { OwnedHandle::from_raw_handle(result) };
    assert_ne!(
        unsafe { ImpersonateLoggedOnUser(result.as_raw_handle()) },
        0
    );
    let _restore = Revert;
    test()
}

fn can_open(pid: u32, rights: u32) -> bool {
    let handle = unsafe { OpenProcess(rights, 0, pid) };
    if handle.is_null() {
        return false;
    }
    drop(unsafe { OwnedHandle::from_raw_handle(handle) });
    true
}

#[test]
#[ignore = "disposable helper invoked only by its parent regression"]
fn helper_process() {
    println!("metadata-fixture-ready");
    std::thread::sleep(std::time::Duration::from_secs(30));
}

struct Helper(std::process::Child);
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn exact_account_grant_allows_identity_query_but_not_process_control() {
    // No installed process or Windows service is changed. This test owns and
    // terminates only its explicitly spawned, hidden, disposable helper.
    let mut helper = Helper(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "pipe::service_peer_query::tests::helper_process",
                "--nocapture",
            ])
            .creation_flags(0x08000000)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let output = helper.0.stdout.take().unwrap();
    assert!(std::io::BufReader::new(output)
        .lines()
        .any(|line| line.unwrap().contains("metadata-fixture-ready")));
    let handle = helper.0.as_raw_handle();
    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        },
        0
    );
    let _descriptor = LocalAllocation(descriptor);
    assert_ne!(
        unsafe { SetKernelObjectSecurity(handle, DACL_SECURITY_INFORMATION, descriptor) },
        0
    );
    let pid = helper.0.id();
    restricted(|| assert!(!can_open(pid, PROCESS_QUERY_LIMITED_INFORMATION)));
    let mut token = std::ptr::null_mut();
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        0
    );
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let buffer = token_user(token.as_raw_handle()).unwrap();
    let sid = unsafe { (&*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    grant_query_only(handle, sid).unwrap();
    grant_query_only(handle, sid).unwrap(); // repeated acceptance never widens rights
    restricted(|| {
        assert!(can_open(pid, PROCESS_QUERY_LIMITED_INFORMATION));
        for rights in [0x0001, 0x0002, 0x0010, 0x0020, 0x0040, 0x40000] {
            assert!(
                !can_open(pid, rights),
                "must not grant process access {rights:#x}"
            );
        }
    });
    inspect_exact_grant_and_add_deny(handle, sid);
    assert!(grant_query_only(handle, sid).is_err());
    restricted(|| {
        assert!(
            !can_open(pid, PROCESS_QUERY_LIMITED_INFORMATION),
            "an explicit deny must never be removed"
        )
    });
}

#[test]
fn arbitrary_handle_cannot_supply_a_pipe_identity() {
    assert!(allow_connected_account(unsafe { GetCurrentProcess() }).is_err());
}
