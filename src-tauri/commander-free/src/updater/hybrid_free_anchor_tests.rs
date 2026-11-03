// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use windows_sys::Win32::{
    Foundation::CloseHandle,
    Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

fn elevated() -> bool {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut info = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut length = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut info as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        ) != 0;
        CloseHandle(token);
        ok && info.TokenIsElevated != 0
    }
}

fn without_admin<T>(test: impl FnOnce() -> T) -> T {
    use windows_sys::Win32::Security::{
        CreateRestrictedToken, CreateWellKnownSid, ImpersonateLoggedOnUser, RevertToSelf,
        WinBuiltinAdministratorsSid, DISABLE_MAX_PRIVILEGE, SID_AND_ATTRIBUTES, TOKEN_DUPLICATE,
    };
    unsafe {
        let mut source = ptr::null_mut();
        assert_ne!(
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &mut source
            ),
            0
        );
        let mut sid = [0u32; 17];
        let mut length = std::mem::size_of_val(&sid) as u32;
        assert_ne!(
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                &mut length
            ),
            0
        );
        let disabled = SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: 0,
        };
        let mut restricted = ptr::null_mut();
        let ok = CreateRestrictedToken(
            source,
            DISABLE_MAX_PRIVILEGE,
            1,
            &disabled,
            0,
            ptr::null(),
            0,
            ptr::null(),
            &mut restricted,
        );
        CloseHandle(source);
        assert_ne!(ok, 0);
        assert_ne!(ImpersonateLoggedOnUser(restricted), 0);
        struct Revert(windows_sys::Win32::Foundation::HANDLE);
        impl Drop for Revert {
            fn drop(&mut self) {
                unsafe {
                    assert_ne!(RevertToSelf(), 0);
                    CloseHandle(self.0);
                }
            }
        }
        let _guard = Revert(restricted);
        test()
    }
}

#[test]
#[ignore = "explicit acceptance: requires an elevated Windows runner"]
fn secured_anchor_accepts_its_acl_and_rejects_existing_untrusted_acl() {
    assert!(
        elevated(),
        "Registry ACL acceptance requires an elevated Windows runner"
    );
    let parent_path = format!(
        r"SOFTWARE\WinCommanderPqAclTest-{}",
        uuid::Uuid::new_v4().simple()
    );
    Anchor::test_user(parent_path.clone())
        .write(b"parent")
        .unwrap();
    let path = format!(r"{parent_path}\Anchor");
    let secure = Anchor::test_secured(path.clone());
    assert!(secure.read().unwrap().is_none());
    secure.write(b"signed record").unwrap();
    assert_eq!(
        secure.read().unwrap().as_deref(),
        Some(&b"signed record"[..])
    );
    without_admin(|| {
        use windows_sys::Win32::{
            Foundation::ERROR_ACCESS_DENIED,
            System::Registry::{
                RegDeleteKeyExW, RegOpenKeyExW, HKEY_CURRENT_USER, KEY_CREATE_SUB_KEY,
                KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY,
            },
        };
        let mut parent = ptr::null_mut();
        let parent_status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(&parent_path).as_ptr(),
                0,
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                &mut parent,
            )
        };
        assert_eq!(parent_status, 0, "parent permissions were changed");
        unsafe {
            RegCloseKey(parent);
        }
        for access in [
            KEY_QUERY_VALUE,
            KEY_SET_VALUE,
            KEY_CREATE_SUB_KEY,
            0x0001_0000,
            0x0004_0000,
            0x0008_0000,
        ] {
            let mut key = ptr::null_mut();
            let status = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    wide(&path).as_ptr(),
                    0,
                    access | KEY_WOW64_64KEY,
                    &mut key,
                )
            };
            if !key.is_null() {
                unsafe {
                    RegCloseKey(key);
                }
            }
            assert_eq!(
                status, ERROR_ACCESS_DENIED,
                "restricted token gained registry access {access:#x}"
            );
        }
        let status =
            unsafe { RegDeleteKeyExW(HKEY_CURRENT_USER, wide(&path).as_ptr(), KEY_WOW64_64KEY, 0) };
        assert_eq!(
            status, ERROR_ACCESS_DENIED,
            "restricted token deleted the anchor key"
        );
    });
    remove_test_key(&path).unwrap();
    remove_test_key(&parent_path).unwrap();

    let path = format!(
        r"SOFTWARE\WinCommanderPqWeakAclTest-{}",
        uuid::Uuid::new_v4().simple()
    );
    let weak = Anchor::test_user(path.clone());
    weak.write(b"untrusted record").unwrap();
    let guarded = Anchor::test_secured(path.clone());
    assert!(guarded.read().is_err());
    assert!(guarded.write(b"replacement").is_err());
    assert_eq!(
        weak.read().unwrap().as_deref(),
        Some(&b"untrusted record"[..])
    );
    remove_test_key(&path).unwrap();
}
