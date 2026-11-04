// SPDX-License-Identifier: AGPL-3.0-or-later
//! Owner-rights and integrity-level acceptance tests.
use super::*;
use windows_sys::Win32::{
    Foundation::ERROR_ACCESS_DENIED,
    Security::{
        CreateRestrictedToken, CreateWellKnownSid, EqualSid, GetLengthSid, GetTokenInformation,
        ImpersonateLoggedOnUser, RevertToSelf, SetTokenInformation, TokenElevation,
        TokenIntegrityLevel, TokenOwner, TokenUser, WinBuiltinAdministratorsSid, WinMediumLabelSid,
        DISABLE_MAX_PRIVILEGE, OWNER_SECURITY_INFORMATION, SID_AND_ATTRIBUTES,
        TOKEN_ADJUST_DEFAULT, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_MANDATORY_LABEL, TOKEN_OWNER,
        TOKEN_QUERY, TOKEN_USER,
    },
    System::Threading::{GetCurrentProcess, OpenMutexW, OpenProcessToken},
};
const WRITE_DAC: u32 = 0x0004_0000;
const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;
/// Runs only the current test thread as the same user with administrators
/// disabled and medium integrity; it does not create or change any account.
fn token_user(token: HANDLE) -> Vec<u64> {
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
    }
    assert!(needed as usize >= std::mem::size_of::<TOKEN_USER>());
    let mut buffer = vec![0u64; (needed as usize).div_ceil(std::mem::size_of::<u64>())];
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        },
        0
    );
    buffer
}

fn as_restricted_medium<T>(test: impl FnOnce(HANDLE) -> T) -> T {
    let mut source = ptr::null_mut();
    assert_ne!(
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
                &mut source,
            )
        },
        0
    );
    let source = OwnedHandle(source);
    let mut admin = [0u32; 17];
    let mut length = std::mem::size_of_val(&admin) as u32;
    assert_ne!(
        unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                ptr::null_mut(),
                admin.as_mut_ptr().cast(),
                &mut length,
            )
        },
        0
    );
    let deny = SID_AND_ATTRIBUTES {
        Sid: admin.as_mut_ptr().cast(),
        Attributes: 0,
    };
    let mut reduced = ptr::null_mut();
    assert_ne!(
        unsafe {
            CreateRestrictedToken(
                source.0,
                DISABLE_MAX_PRIVILEGE,
                1,
                &deny,
                0,
                ptr::null(),
                0,
                ptr::null(),
                &mut reduced,
            )
        },
        0
    );
    let reduced = OwnedHandle(reduced);
    let user = token_user(reduced.0);
    let user_sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let owner = TOKEN_OWNER { Owner: user_sid };
    assert_ne!(
        unsafe {
            SetTokenInformation(
                reduced.0,
                TokenOwner,
                (&owner as *const TOKEN_OWNER).cast(),
                std::mem::size_of::<TOKEN_OWNER>() as u32,
            )
        },
        0,
        "test could not select the user SID as token owner"
    );
    let mut medium = [0u32; 17];
    let mut length = std::mem::size_of_val(&medium) as u32;
    assert_ne!(
        unsafe {
            CreateWellKnownSid(
                WinMediumLabelSid,
                ptr::null_mut(),
                medium.as_mut_ptr().cast(),
                &mut length,
            )
        },
        0
    );
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: medium.as_mut_ptr().cast(),
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    let bytes = std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32
        + unsafe { GetLengthSid(label.Label.Sid) };
    assert_ne!(
        unsafe {
            SetTokenInformation(
                reduced.0,
                TokenIntegrityLevel,
                (&label as *const TOKEN_MANDATORY_LABEL).cast(),
                bytes,
            )
        },
        0,
        "test could not create a medium-integrity token: {}",
        unsafe { GetLastError() }
    );
    assert_ne!(unsafe { ImpersonateLoggedOnUser(reduced.0) }, 0);
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            assert_ne!(unsafe { RevertToSelf() }, 0);
        }
    }
    let _revert = Revert;
    test(reduced.0)
}

#[test]
fn restricted_owner_cannot_reopen_its_mutex_for_dacl_write() {
    as_restricted_medium(|token| {
        let name = name();
        let guard = acquire_named(&name, 1_000).expect("restricted owner creates valid mutex");
        let mut owner = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        assert_eq!(
            unsafe {
                GetSecurityInfo(
                    guard.handle,
                    SE_KERNEL_OBJECT,
                    OWNER_SECURITY_INFORMATION,
                    &mut owner,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut descriptor,
                )
            },
            0
        );
        let user = token_user(token);
        let user_sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let same_owner = unsafe { EqualSid(owner, user_sid) };
        unsafe {
            LocalFree(descriptor);
        }
        assert_ne!(same_owner, 0, "test token must be the actual mutex owner");
        let raw = unsafe { OpenMutexW(WRITE_DAC, 0, wide(&name).as_ptr()) };
        if !raw.is_null() {
            unsafe {
                CloseHandle(raw);
            }
            panic!("restricted owner acquired WRITE_DAC despite owner-rights ACE");
        }
        assert_eq!(unsafe { GetLastError() }, ERROR_ACCESS_DENIED);
        let raw = unsafe { OpenMutexW(ACCESS, 0, wide(&name).as_ptr()) };
        assert!(
            !raw.is_null(),
            "restricted owner cannot reopen with normal access"
        );
        drop(OwnedHandle(raw));
        drop(guard);
    });
}

#[test]
#[ignore = "explicit acceptance: requires an elevated Windows runner"]
fn elevated_first_and_restricted_medium_second_share_the_same_mutex() {
    let mut token = ptr::null_mut();
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        0
    );
    let token = OwnedHandle(token);
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut used = 0;
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut used,
            )
        },
        0
    );
    assert_ne!(
        elevation.TokenIsElevated, 0,
        "this acceptance test must run elevated"
    );
    let name = name();
    let first = acquire_named(&name, 1_000).expect("elevated owner creates valid mutex");
    let second_name = name.clone();
    let (busy_tx, busy_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        as_restricted_medium(|_| {
            let error = match acquire_named(&second_name, 100) {
                Ok(_) => panic!("medium token acquired held mutex"),
                Err(error) => error,
            };
            assert!(
                error.contains("already in progress"),
                "medium token must reach the mutex, not fail its ACL: {error}"
            );
            busy_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            drop(
                acquire_named(&second_name, 1_000)
                    .expect("medium token acquires after elevated owner releases"),
            );
        })
    });
    busy_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("medium waiter did not enter");
    drop(first);
    release_tx.send(()).unwrap();
    thread.join().unwrap();
}

// Two distinct real Windows user SIDs require a controlled multi-account
// Windows runner; a restricted token above is not a substitute. That
// acceptance case remains open until run on such a host.
