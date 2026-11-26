use super::*;
use serde_json::json;

fn request() -> serde_json::Value {
    json!({
        "VolumePath": r"D:\Vaults\notes.hc",
        "Target": "hidden",
        "OldPassword": "old-canary",
        "NewPassword": "new-canary",
    })
}

#[test]
fn a_complete_request_is_accepted_without_copying_passwords() {
    let parsed = validate_request(&request()).expect("valid request");
    assert_eq!(parsed.volume_path, r"D:\Vaults\notes.hc");
    assert_eq!(parsed.target, "hidden");
    assert!(!format!("{parsed:?}").contains("canary"));
}

#[test]
fn target_defaults_to_the_outer_or_only_password() {
    let mut args = request();
    args.as_object_mut().unwrap().remove("Target");
    assert_eq!(validate_request(&args).unwrap().target, "outer");
}

#[test]
fn rejects_unknown_fields_targets_and_relative_paths() {
    for (key, value) in [
        ("TargetSessionId", json!(0)),
        ("Target", json!("both")),
        ("VolumePath", json!(r"notes.hc")),
        ("VolumePath", json!(r"\Device\Harddisk1\Partition1")),
        ("TargetKind", json!("device")),
        ("OldPim", json!("0")),
        ("NewPim", json!("2147469")),
        ("NewPim", json!("12a")),
        ("OldKeyfiles", json!("not json")),
        ("NewKeyfiles", json!([r"relative.key"])),
    ] {
        let mut args = request();
        args[key] = value;
        assert!(validate_request(&args).is_none(), "{key} must be rejected");
    }
}

#[test]
fn a_device_target_must_name_a_partition() {
    let mut args = request();
    args["TargetKind"] = json!("device");
    args["VolumePath"] = json!(r"\Device\Harddisk1\Partition2");
    assert!(validate_request(&args).is_some());
}

#[test]
fn empty_password_is_allowed_only_with_keyfiles_for_that_side() {
    let mut args = request();
    args["NewPassword"] = json!("");
    assert!(validate_request(&args).is_none());
    args["NewKeyfiles"] = json!(serde_json::to_string(&[r"D:\keys\new.key"]).unwrap());
    assert_eq!(validate_request(&args).unwrap().keyfiles, [r"D:\keys\new.key"]);
    args["OldPassword"] = json!("");
    assert!(validate_request(&args).is_none());
    args["OldKeyfiles"] = json!([r"D:\keys\old.key"]);
    assert_eq!(validate_request(&args).unwrap().keyfiles.len(), 2);
}

#[test]
fn missing_or_non_string_passwords_are_rejected() {
    for key in ["OldPassword", "NewPassword"] {
        let mut args = request();
        args.as_object_mut().unwrap().remove(key);
        assert!(validate_request(&args).is_none());
        args[key] = json!(42);
        assert!(validate_request(&args).is_none());
    }
}

#[test]
fn broker_failures_map_to_the_contract_codes() {
    for (reason, code) in [
        (VaultMountReason::WrongPassword, "wrong_password"),
        (VaultMountReason::EngineUnlockFailed, "wrong_password"),
        (VaultMountReason::WrongHeader, "wrong_header"),
        (VaultMountReason::NewPasswordCollides, "new_password_collides"),
        (VaultMountReason::WeakNewPassword, "weak_new_password"),
        (VaultMountReason::AlreadyMounted, "volume_mounted"),
        (VaultMountReason::EngineFailed, "engine_failed"),
        (VaultMountReason::MountStateUnknown, "vault_mount_state_unknown"),
        (VaultMountReason::ProNotInstalled, "vault_pro_not_installed"),
    ] {
        assert_eq!(failure_code(reason), code);
    }
}

#[test]
fn policy_vaults_need_the_owner_or_a_write_grant() {
    let personal = PasswordChangeTarget {
        container_path: r"D:\Vaults\notes.hc".into(),
        container_identity: "file-1".into(),
        entry_id: None,
        caller_owns_entry: false,
    };
    assert!(policy_change_allowed(&personal, false, None));
    let mut managed = PasswordChangeTarget { entry_id: Some("shared".into()), ..personal };
    assert!(!policy_change_allowed(&managed, false, None));
    assert!(!policy_change_allowed(&managed, true, Some(VaultAccess::Read)));
    assert!(!policy_change_allowed(&managed, false, Some(VaultAccess::Write)));
    assert!(policy_change_allowed(&managed, true, Some(VaultAccess::Write)));
    managed.caller_owns_entry = true;
    assert!(policy_change_allowed(&managed, false, None));
}

#[test]
fn handler_refuses_before_the_broker_without_an_interactive_caller() {
    let store = crate::vault_access::test_store();
    let broker = VaultMountBroker::new();
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let error = runtime
        .block_on(handle(1, &store, &broker, request(), None))
        .unwrap_err();
    assert_eq!(error.kind, PERSONAL_VAULT_SESSION_ABSENT);
}

fn both_request() -> serde_json::Value {
    json!({
        "VolumePath": r"D:\Vaults\decoy.hc",
        "Target": "both",
        "OldPassword": "old-decoy-canary",
        "NewPassword": "new-decoy-canary",
        "OldHiddenPassword": "old-real-canary",
        "NewHiddenPassword": "new-real-canary",
        "OldHiddenKeyfiles": "[]",
        "NewHiddenKeyfiles": json!([r"D:\keys\real.key"]).to_string(),
        "NewHiddenPim": "600",
    })
}

#[test]
fn both_passwords_need_both_credentials_and_hold_every_keyfile() {
    let parsed = validate_request(&both_request()).expect("valid request");
    assert_eq!(parsed.target, "both");
    assert_eq!(parsed.keyfiles, [r"D:\keys\real.key"]);
    assert!(!format!("{parsed:?}").contains("canary"));
    for (key, value) in [
        ("OldHiddenPassword", json!("")),
        ("NewHiddenPassword", json!(7)),
        ("NewHiddenPim", json!("0")),
        ("OldHiddenKeyfiles", json!([r"relative.key"])),
    ] {
        let mut args = both_request();
        args[key] = value;
        assert!(validate_request(&args).is_none(), "{key} must be rejected");
    }
    let mut missing = both_request();
    missing.as_object_mut().unwrap().remove("NewHiddenPassword");
    assert!(validate_request(&missing).is_none());
}

#[test]
fn one_password_requests_accept_only_blank_real_password_fields() {
    let mut args = request();
    for key in HIDDEN_KEYS {
        args[key] = json!("");
    }
    args["OldHiddenKeyfiles"] = json!("[]");
    assert_eq!(validate_request(&args).unwrap().target, "hidden");
    args["NewHiddenPassword"] = json!("filled-canary");
    assert!(validate_request(&args).is_none(), "a filled real password would be silently ignored");
}

#[test]
fn a_copied_as_path_selection_is_accepted() {
    let mut args = request();
    args["VolumePath"] = json!("\"D:\\Vault\\New folder\\test3\"");
    assert_eq!(validate_request(&args).unwrap().volume_path, r"D:\Vault\New folder\test3");
    args["VolumePath"] = json!("\"test3\"");
    assert!(validate_request(&args).is_none());
    assert_eq!(selected_path("  \"D:\\a b\\c\"  "), r"D:\a b\c");
    assert_eq!(selected_path("\"unbalanced"), "\"unbalanced");
}

#[test]
fn only_a_closed_refusal_of_both_passwords_becomes_its_own_code() {
    for code in DUAL_REFUSALS {
        let reply = json!({ "status": "password_refused", "code": code });
        assert_eq!(dual_refusal("both", &reply), Some(code));
        assert_eq!(dual_refusal("outer", &reply), None, "one-password replies never use these codes");
    }
    for reply in [
        json!({ "status": "password_refused", "code": "wrong_password" }),
        json!({ "status": "password_refused" }),
        json!({ "status": "password_changed", "code": "wrong_real_password" }),
    ] {
        assert_eq!(dual_refusal("both", &reply), None, "{reply}");
    }
}

/// The exact target the service resolves for an extensionless container in a
/// folder with a space, on this PC's real filesystem, and its write proof.
#[cfg(windows)]
#[test]
fn a_real_extensionless_container_resolves_and_passes_the_write_proof() {
    use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let dir = std::env::temp_dir().join(format!("wc-chpw-target-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("New folder")).unwrap();
    let file = dir.join("New folder").join("test3");
    std::fs::write(&file, vec![0; 1 << 20]).unwrap();
    let store = crate::vault_access::test_windows_fs_store(dir.join("policy"));
    let mut args = request();
    args["VolumePath"] = json!(format!("\"{}\"", file.display()));
    args["Target"] = json!("outer");
    let parsed = validate_request(&args).expect("the dialog's request is valid");
    let target = store.password_change_target(&parsed.volume_path, "S-1-5-21-owner", 7).unwrap();
    assert_eq!(target.entry_id, None);
    assert!(target.container_path.ends_with(r"New folder\test3"));
    let mut token = std::ptr::null_mut();
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE, &mut token) },
        0
    );
    let held = crate::vault_access::hold_password_change_files(&target, &[], token);
    unsafe { windows_sys::Win32::Foundation::CloseHandle(token) };
    assert!(held.is_ok());
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}