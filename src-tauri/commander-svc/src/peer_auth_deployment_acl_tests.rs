// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use windows_sys::Win32::Security::{
    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityDescriptorDacl,
    GetSecurityDescriptorOwner,
};

fn inspect(sddl: &str) -> Result<(), PeerAuthError> {
    let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            ),
            0
        );
        let _descriptor = Descriptor(descriptor);
        let mut owner = std::ptr::null_mut();
        let mut owner_defaulted = 0;
        assert_ne!(
            GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted),
            0
        );
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = std::ptr::null_mut();
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
            0
        );
        verify_acl(owner, acl)
    }
}

#[test]
fn accepts_windows_installer_owner_and_writes_on_protected_ancestors() {
    let sddl = format!("O:{TRUSTED_INSTALLER_SID}D:P(A;;FA;;;{TRUSTED_INSTALLER_SID})(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;BU)");
    assert_eq!(inspect(&sddl), Ok(()));
    for owner in ["SY", "BA"] {
        assert_eq!(
            inspect(&format!(
                "O:{owner}D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;BU)"
            )),
            Ok(())
        );
    }
}

#[test]
fn rejects_untrusted_owners_and_every_unprivileged_writer() {
    assert_eq!(
        inspect("O:BUD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;BU)"),
        Err(PeerAuthError::PathNotAllowed)
    );
    for writer in ["BU", "WD", "AU", "S-1-5-80-1-2-3-4-5"] {
        let sddl = format!("O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FW;;;{writer})");
        assert_eq!(inspect(&sddl), Err(PeerAuthError::PathNotAllowed));
    }
    let sddl = "O:S-1-5-80-1-2-3-4-5D:P(A;;FA;;;SY)(A;;FA;;;BA)";
    assert_eq!(inspect(sddl), Err(PeerAuthError::PathNotAllowed));
}

#[test]
fn rejects_absent_or_empty_dacl() {
    assert_eq!(
        inspect("O:SYD:NO_ACCESS_CONTROL"),
        Err(PeerAuthError::PathNotAllowed)
    );
    assert_eq!(inspect("O:SYD:P"), Err(PeerAuthError::PathNotAllowed));
}

#[test]
fn rejects_a_reparse_component_before_inspecting_its_target_acl() {
    let root = std::env::temp_dir().join(format!(
        "wincmd-relay-reparse-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir(&root).unwrap();
    let target = root.join("target");
    let junction = root.join("junction");
    std::fs::create_dir(&target).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture could not be created"
    );
    let result = verify_component_with(&junction, |_| panic!("reparse target must not be trusted"));
    std::fs::remove_dir(&junction).unwrap();
    std::fs::remove_dir(&target).unwrap();
    std::fs::remove_dir(&root).unwrap();
    assert_eq!(result, Err(PeerAuthError::PathNotAllowed));
}
