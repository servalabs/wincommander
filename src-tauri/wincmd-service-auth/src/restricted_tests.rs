// SPDX-License-Identifier: AGPL-3.0-or-later
//! These tests alter only the current test thread's token; no account, ACL,
//! service configuration, settings, or credentials are changed.
use super::*;
use windows_sys::Win32::Security::{
    CreateRestrictedToken, CreateWellKnownSid, ImpersonateLoggedOnUser, RevertToSelf,
    WinBuiltinAdministratorsSid, DISABLE_MAX_PRIVILEGE, SID_AND_ATTRIBUTES, TOKEN_DUPLICATE,
    TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

struct Revert;
impl Drop for Revert {
    fn drop(&mut self) {
        assert_ne!(
            unsafe { RevertToSelf() },
            0,
            "restore the test thread token"
        );
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
    let mut length = std::mem::size_of_val(&sid) as u32;
    assert_ne!(
        unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                std::ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                &mut length,
            )
        },
        0
    );
    let deny_admin = SID_AND_ATTRIBUTES {
        Sid: sid.as_mut_ptr().cast(),
        Attributes: 0,
    };
    let mut reduced = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            CreateRestrictedToken(
                token.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                1,
                &deny_admin,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut reduced,
            )
        },
        0
    );
    let reduced = unsafe { OwnedHandle::from_raw_handle(reduced) };
    assert_ne!(
        unsafe { ImpersonateLoggedOnUser(reduced.as_raw_handle()) },
        0
    );
    let _restore = Revert;
    test()
}

#[test]
fn restricted_token_cannot_turn_own_process_into_the_registered_service() {
    restricted(|| {
        let guard = pin_process(std::process::id()).unwrap();
        assert!(
            require_running_pid(true, Some(std::process::id() + 1), std::process::id()).is_err()
        );
        drop(guard);
    });
}

#[tokio::test]
#[ignore = "read-only acceptance requires the installed WinCommander service to be running"]
async fn installed_service_authenticates_with_admin_group_disabled() {
    use tokio::net::windows::named_pipe::ClientOptions;
    restricted(|| {
        // Open the actual pipe under the restricted token, but send no Hello or
        // request. The production verifier uses only OS/SCM read-only queries.
        let client = ClientOptions::new()
            .open(wincmd_shared::svc::SVC_PIPE_NAME)
            .unwrap();
        let pid = server_process_id(&client).unwrap();
        // The updated service adds the exact caller's metadata-only grant
        // asynchronously after accept; no request is sent while waiting.
        let verified = verify_service_peer(&client).unwrap();
        let guard = pin_process(pid).unwrap();
        drop((verified, guard, client));
    });
}
