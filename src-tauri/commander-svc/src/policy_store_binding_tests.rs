// SPDX-License-Identifier: AGPL-3.0-or-later
fn binding_envelope(input: &EpochInstallInput, generation: &str) -> fleet_proto::PolicyEnvelope {
    let mut envelope: fleet_proto::PolicyEnvelope=serde_json::from_value(serde_json::json!({
        "org_id":"11111111-1111-4111-8111-111111111111","device_id":"22222222-2222-4222-8222-222222222222",
        "enrollment_generation":generation,"config_revision":input.version,"resolution_revision":input.version,
        "config":{"org_id":"11111111-1111-4111-8111-111111111111","version":input.version,"config_json":input.config,
            "locked_paths":input.locked_paths,"managed":input.managed,"target_kind":input.target_kind,"target_id":input.target_id,
            "signature":input.signature_b64,"signer_key":input.signer_key_b64},
        "clipboard_guard_enabled":true,"shield":null,"shield_revision":0,"signature":"","signer_key":input.signer_key_b64,
    })).unwrap();
    sign_binding_envelope(&mut envelope);
    envelope
}
fn sign_binding_envelope(envelope: &mut fleet_proto::PolicyEnvelope) {
    use base64::Engine;
    use ed25519_dalek::Signer;
    let key = ed25519_dalek::SigningKey::from_bytes(&[91; 32]);
    if let Some(epoch) = envelope.config.as_mut() {
        let bytes = fleet_proto::epoch_preimage(&fleet_proto::EpochSigningInput {
            version: epoch.version,
            config: &epoch.config_json,
            locked_paths: &epoch.locked_paths,
            managed: epoch.managed,
            target_kind: &epoch.target_kind,
            target_id: epoch.target_id.as_deref(),
        });
        epoch.signature =
            base64::engine::general_purpose::STANDARD.encode(key.sign(&bytes).to_bytes());
    }
    envelope.signature =
        base64::engine::general_purpose::STANDARD.encode(key.sign(&envelope.preimage()).to_bytes());
}
fn binding_store(pinned: String) -> PolicyStore {
    PolicyStore::open(
        Box::new(FakePolicyFs::new()),
        Box::new(FakeClock(AtomicI64::new(1))),
        PathBuf::from("/fake/binding"),
        pinned,
    )
    .unwrap()
}
#[test]
fn retained_content_is_not_relabelled_when_new_machine_scope_has_no_policy_or_other_intent() {
    let (input, pinned) = content_input(1, true);
    let store = binding_store(pinned);
    store.install_content_epoch(input.clone()).unwrap();
    let a = binding_envelope(&input, "33333333-3333-4333-8333-333333333333");
    assert!(
        store.bound_clipboard_response(true, Some(&a)).unwrap()["reporting_binding"]
            .as_str()
            .is_some_and(|binding| binding.len() == 64)
    );
    let mut b = a.clone();
    b.enrollment_generation = Some("44444444-4444-4444-8444-444444444444".into());
    b.config = None;
    sign_binding_envelope(&mut b);
    assert!(store.bound_clipboard_response(true, Some(&b)).is_err());
    assert!(store.bound_clipboard_response(true, None).is_err());
    b = a.clone();
    b.config.as_mut().unwrap().config_json = serde_json::json!({"privacy":{"other":true}});
    sign_binding_envelope(&mut b);
    assert!(store.bound_clipboard_response(true, Some(&b)).is_err());
    b = a.clone();
    b.config.as_mut().unwrap().config_json["contentRules"]["enabled"] = serde_json::json!(false);
    sign_binding_envelope(&mut b);
    assert!(store.bound_clipboard_response(true, Some(&b)).is_err());
}
#[test]
fn current_generation_may_authorize_identical_dlp_intent_despite_unrelated_shield_changes() {
    use sha2::Digest;
    let (input, pinned) = content_input(1, true);
    let store = binding_store(pinned);
    store.install_content_epoch(input.clone()).unwrap();
    let a = binding_envelope(&input, "33333333-3333-4333-8333-333333333333");
    let old = store.bound_clipboard_response(true, Some(&a)).unwrap();
    let mut b = a.clone();
    b.enrollment_generation = Some("44444444-4444-4444-8444-444444444444".into());
    b.config.as_mut().unwrap().config_json["privacy"] =
        serde_json::json!({"privacyShield":{"enabled":true}});
    sign_binding_envelope(&mut b);
    let response = store.bound_clipboard_response(true, Some(&b)).unwrap();
    let bytes = serde_json::to_vec(&[
        b.signer_key.as_str(),
        b.org_id.0.as_str(),
        b.device_id.0.as_str(),
        b.enrollment_generation.as_deref().unwrap(),
    ])
    .unwrap();
    let expected: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(response["reporting_binding"], serde_json::json!(expected));
    assert_ne!(response["reporting_binding"], old["reporting_binding"]);
    assert_eq!(response["content_rules"], old["content_rules"]);
}
