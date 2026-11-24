// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
#[test]
#[ignore = "requires elevated Windows test runner"]
fn staging_publish_and_identity_readback_with_held_destination() {
    use crate::vault_access::{VaultFs, WindowsVaultFs};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let directory =
        std::env::temp_dir().join(format!("wc-stage-test-{}", rand::random::<u64>()));
    std::fs::create_dir(&directory).unwrap();
    let target = directory.join("container.bin");
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
    let mut args = serde_json::json!({"SizeMB":1});
    let mut creation =
        Creation::prepare(&mut args, target.to_str().unwrap(), token).expect("prepare");
    let staged = args["Path"].as_str().unwrap();
    std::fs::write(staged, vec![42u8; 1024 * 1024]).unwrap();
    let mut result = serde_json::json!({"path": staged});
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    let writer = OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_READ)
        .open(staged)
        .unwrap();
    assert!(matches!(
        creation.publish(&mut result),
        Err(PublishFailure::SourceOpen)
    ));
    assert_eq!(
        std::fs::metadata(&target).unwrap().len(),
        0,
        "a locked source must not be copied"
    );
    drop(writer);
    creation.publish(&mut result).expect("publish");
    with_caller_impersonation(token, || {
        WindowsVaultFs.personal_creation_target_identity(&target)
    })
    .expect("caller identity");
    creation.commit();
    drop(creation);
    unsafe {
        CloseHandle(token);
    }
    std::fs::remove_file(target).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
#[test]
fn creation_sizes_are_positive_and_bounded_before_formatting() {
    assert_eq!(
        expected_bytes(&serde_json::json!({"SizeMB":32})).unwrap(),
        32 * 1024 * 1024
    );
    for size in [0, u64::MAX] {
        assert!(expected_bytes(&serde_json::json!({"SizeMB":size})).is_err());
    }
    assert!(
        expected_bytes(&serde_json::json!({"HostSizeMB":64,"SizeMB":32})).unwrap()
            == 64 * 1024 * 1024
    );
}
#[test]
fn keyfile_values_preserve_multiple_selections_and_reject_nonpaths() {
    assert_eq!(
        keyfile_paths(&Value::String("[\"C:\\\\a\",\"C:\\\\b\"]".into()))
            .unwrap()
            .len(),
        2
    );
    assert!(keyfile_paths(&serde_json::json!([true])).is_err());
    assert!(keyfile_paths(&serde_json::json!({"Path":"C:\\a"})).is_err());
}

#[test]
fn one_shared_volume_must_hold_both_copies() {
    assert!(check_capacity(32, 64, 64, true).is_ok());
    assert!(matches!(check_capacity(32, 63, 64, true), Err(PrepareFailure::InsufficientSpace)));
    assert!(matches!(check_capacity(32, 64, 63, true), Err(PrepareFailure::InsufficientSpace)));
}

#[test]
fn separate_volumes_each_need_one_copy() {
    assert!(check_capacity(32, 32, 32, false).is_ok());
    assert!(matches!(check_capacity(32, 31, 32, false), Err(PrepareFailure::InsufficientSpace)));
    assert!(matches!(check_capacity(32, 32, 31, false), Err(PrepareFailure::InsufficientSpace)));
}

#[test]
fn overflow_is_rejected_as_invalid_not_as_a_space_shortfall() {
    for same_volume in [true, false] {
        assert!(matches!(
            check_capacity(u64::MAX, u64::MAX, u64::MAX, same_volume),
            Err(PrepareFailure::Other)
        ));
    }
}

#[test]
fn preparation_failures_carry_distinct_diagnostic_codes() {
    assert_eq!(PrepareFailure::InsufficientSpace.diagnostic_code(), "VLT.CREATE.INSUFFICIENT_SPACE");
    assert_eq!(
        PrepareFailure::Other.diagnostic_code(),
        "VLT.CREATE.PREPARATION_FAILED"
    );
}
