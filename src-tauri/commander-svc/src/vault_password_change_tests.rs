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
