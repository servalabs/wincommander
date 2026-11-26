use super::*;
use serde_json::json;

fn request() -> serde_json::Value {
    json!({
        "VolumePath": r"D:\Vaults\notes.hc",
        "TargetKind": "file",
        "Password": "current-canary",
        "Keyfiles": "[]",
    })
}

#[test]
fn the_dialogs_request_is_accepted_without_copying_the_password() {
    let parsed = validate_request(&request()).expect("valid request");
    assert_eq!(parsed.volume_path, r"D:\Vaults\notes.hc");
    assert!(parsed.keyfiles.is_empty());
    assert!(!format!("{parsed:?}").contains("canary"));
    let mut with_pim = request();
    with_pim["Pim"] = json!("600");
    with_pim["Keyfiles"] = json!(json!([r"D:\keys\a.key"]).to_string());
    assert_eq!(validate_request(&with_pim).unwrap().keyfiles, [r"D:\keys\a.key"]);
}

#[test]
fn rejects_extra_fields_empty_credentials_and_bad_pims() {
    for (key, value) in [
        ("Target", json!("hidden")),
        ("OldPassword", json!("x")),
        ("Password", json!("")),
        ("Password", json!(7)),
        ("Pim", json!("0")),
        ("Keyfiles", json!([r"relative.key"])),
        ("VolumePath", json!("notes.hc")),
    ] {
        let mut args = request();
        args[key] = value;
        assert!(validate_request(&args).is_none(), "{key} must be rejected");
    }
    let mut missing = request();
    missing.as_object_mut().unwrap().remove("Password");
    assert!(validate_request(&missing).is_none());
}

#[test]
fn only_the_two_header_names_leave_the_service() {
    for header in ["outer", "hidden"] {
        assert_eq!(opened_header(&json!({ "status": "password_opened", "header": header })), Some(header));
    }
    for reply in [
        json!({ "status": "password_opened", "header": "both" }),
        json!({ "status": "password_opened" }),
        json!({ "status": "password_changed", "header": "outer" }),
    ] {
        assert_eq!(opened_header(&reply), None, "{reply}");
    }
}

#[test]
fn handler_refuses_before_the_broker_without_an_interactive_caller() {
    let store = crate::vault_access::test_store();
    let broker = VaultMountBroker::new();
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let error = runtime.block_on(handle(1, &store, &broker, request(), None, true)).unwrap_err();
    assert_eq!(error.kind, super::super::PERSONAL_VAULT_SESSION_ABSENT);
}

#[test]
fn a_noted_vault_refuses_its_decoy_password_and_a_hidden_answer_is_returned() {
    let unread = || -> Result<bool, crate::vault_access::VaultError> { unreachable!("a hidden answer never reads the note") };
    assert_eq!(super::super::vault_password_target::probe_outcome("hidden", unread).ok(), Some("hidden"));
    assert_eq!(super::super::vault_password_target::probe_outcome("outer", || Ok(false)).ok(), Some("outer"));
    let (_, refused) = super::super::vault_password_target::probe_outcome("outer", || Ok(true)).unwrap_err();
    assert_eq!((refused.kind, refused.message.as_str()), ("real_password_required", "this vault needs its real password"));
    let (_, unreadable) = super::super::vault_password_target::probe_outcome("outer", || {
        Err(crate::vault_access::VaultError::Persistence)
    })
    .unwrap_err();
    assert_eq!(unreadable.kind, "vault_policy_unavailable", "an unreadable note never lets a decoy through");
}

#[test]
fn only_a_noted_store_refuses_a_decoy_answer() {
    let store = crate::vault_access::test_policy_store();
    let target = crate::vault_access::PasswordChangeTarget {
        container_path: r"D:\Vaults\notes.hc".into(),
        container_identity: "v:9:i:3".into(),
        entry_id: None,
        caller_owns_entry: false,
    };
    let noted = || store.has_hidden_part(&target.container_identity);
    assert_eq!(super::super::vault_password_target::probe_outcome("outer", noted).ok(), Some("outer"));
    store.note_hidden_part(&target).unwrap();
    let noted = || store.has_hidden_part(&target.container_identity);
    assert_eq!(
        super::super::vault_password_target::probe_outcome("outer", noted).unwrap_err().1.kind,
        "real_password_required"
    );
}
