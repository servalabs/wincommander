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
    assert!(!parsed.checks_collision);
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
        ("OldHiddenPassword", json!("old-real")),
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

fn decoy_follow_up() -> serde_json::Value {
    json!({
        "VolumePath": r"D:\Vaults\decoy.hc",
        "Target": "outer",
        "OldPassword": "old-decoy-canary",
        "NewPassword": "new-decoy-canary",
        "HiddenPassword": "proven-real-canary",
        "HiddenKeyfiles": json!([r"D:\keys\real.key"]).to_string(),
        "HiddenPim": "600",
    })
}

#[test]
fn only_a_decoy_change_carrying_the_proven_real_password_checks_a_collision() {
    let parsed = validate_request(&decoy_follow_up()).expect("valid follow-up");
    assert_eq!(parsed.target, "outer");
    assert!(parsed.checks_collision);
    assert_eq!(parsed.keyfiles, [r"D:\keys\real.key"], "the real keyfile is held too");
    assert!(!format!("{parsed:?}").contains("canary"));

    let mut hidden_target = decoy_follow_up();
    hidden_target["Target"] = json!("hidden");
    assert!(validate_request(&hidden_target).is_none(), "a hidden change never carries a second password");
    let mut pim_only = request();
    pim_only["Target"] = json!("outer");
    pim_only["HiddenPim"] = json!("600");
    assert!(validate_request(&pim_only).is_none(), "a PIM alone is no proof");
    for (key, value) in [("HiddenPassword", json!(7)), ("HiddenPim", json!("0")), ("HiddenKeyfiles", json!([r"real.key"]))] {
        let mut args = decoy_follow_up();
        args[key] = value;
        assert!(validate_request(&args).is_none(), "{key} must be rejected");
    }
}

#[test]
fn blank_real_password_fields_mean_no_collision_check() {
    let mut args = request();
    args["Target"] = json!("outer");
    for key in HIDDEN_KEYS {
        args[key] = json!("");
    }
    args["HiddenKeyfiles"] = json!("[]");
    assert!(!validate_request(&args).unwrap().checks_collision);
}

#[test]
fn broker_failures_map_to_the_contract_codes() {
    for (reason, code) in [
        (VaultMountReason::WrongPassword, "wrong_password"),
        (VaultMountReason::EngineUnlockFailed, "wrong_password"),
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
fn handler_refuses_before_the_broker_without_an_interactive_caller() {
    let store = crate::vault_access::test_store();
    let broker = VaultMountBroker::new();
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let error = runtime
        .block_on(handle(1, &store, &broker, request(), None))
        .unwrap_err();
    assert_eq!(error.kind, PERSONAL_VAULT_SESSION_ABSENT);
}

#[test]
fn an_outer_change_on_a_noted_vault_needs_proven_hidden_credentials() {
    use crate::vault_access::VaultError;
    let unread = || -> Result<bool, VaultError> { unreachable!("a hidden change never reads the note") };
    assert_eq!(outer_gate("hidden", false, unread).ok(), Some(OuterGate::Allowed));
    assert_eq!(outer_gate("outer", false, || Ok(false)).ok(), Some(OuterGate::Allowed));
    assert_eq!(outer_gate("outer", true, || Ok(false)).ok(), Some(OuterGate::Allowed), "the engine still checks the collision");
    assert_eq!(outer_gate("outer", false, || Ok(true)).unwrap_err().1.kind, "real_password_required");
    assert_eq!(outer_gate("outer", true, || Ok(true)).ok(), Some(OuterGate::ProveHidden));
    assert_eq!(outer_gate("outer", true, || Err(VaultError::Persistence)).unwrap_err().1.kind, "vault_policy_unavailable");
}

#[test]
fn hidden_credentials_count_only_when_the_probe_opens_the_hidden_header() {
    assert!(hidden_proof_refusal(Ok("hidden")).is_ok());
    for wrong in [Ok("outer"), Err(VaultMountReason::WrongPassword), Err(VaultMountReason::EngineUnlockFailed)] {
        let (_, refused) = hidden_proof_refusal(wrong).unwrap_err();
        assert_eq!(refused.kind, "real_password_required", "{wrong:?}");
    }
    assert_eq!(hidden_proof_refusal(Err(VaultMountReason::EngineFailed)).unwrap_err().1.kind, "engine_failed");
}

#[test]
fn the_hidden_proof_carries_only_the_real_credential_and_is_scrubbed() {
    let mut args = request();
    args["Target"] = json!("outer");
    args["TargetKind"] = json!("file");
    args["HiddenPassword"] = json!("real-canary");
    args["HiddenPim"] = json!("600");
    args["HiddenKeyfiles"] = json!(json!([r"D:\keys\real.key"]).to_string());
    let mut proof = hidden_proof_args(&args);
    assert_eq!(
        proof.0,
        json!({ "TargetKind": "file", "Password": "real-canary", "Pim": "600", "Keyfiles": json!([r"D:\keys\real.key"]).to_string() })
    );
    zeroize_json(&mut proof.0);
    assert!(!proof.0.to_string().contains("real-canary"));
    let keyfiles_only = hidden_proof_args(&json!({ "HiddenKeyfiles": "[]" }));
    assert_eq!(keyfiles_only.0["Password"], "");
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
