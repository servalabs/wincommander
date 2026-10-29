// SPDX-License-Identifier: AGPL-3.0-or-later
//! Authorization/replay tests never install software or mutate machine state.

use super::authority::{Authority, Decision};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use wincmd_shared::{
    fleet::{canonical_command_bytes, canonical_json_sha256, ActionClass, DeviceId, SignedCommand},
    managed_deployment::{ManagedDeploymentContext, ManagedDeploymentRequest},
};

const DEVICE: &str = "38e39035-33d4-4b8c-b1ba-e36fcdd3cab5";
const SETUP: &str = "9416acb7-35ba-4d35-895f-87c06c103dd9";

#[test]
fn only_fixed_deployment_verbs_are_known_and_binding_stays_privileged() {
    use wincmd_shared::svc::{classify_verb, is_known_verb, CapabilityClass};
    for verb in [
        "svc.managed_deployment.bind",
        "svc.managed_deployment.apply",
        "svc.managed_deployment.status",
    ] {
        assert!(is_known_verb(verb));
    }
    assert_eq!(
        classify_verb("svc.managed_deployment.bind"),
        CapabilityClass::Privileged
    );
    for verb in [
        "svc.managed_deployment.reset",
        "svc.managed_deployment.apply.extra",
    ] {
        assert!(!is_known_verb(verb));
        assert_eq!(classify_verb(verb), CapabilityClass::Privileged);
    }
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[41; 32])
}

fn context() -> ManagedDeploymentContext {
    ManagedDeploymentContext {
        org_id: "local".into(),
        device_id: DEVICE.into(),
        signing_key_b64: STANDARD.encode(key().verifying_key().as_bytes()),
    }
}

fn request() -> ManagedDeploymentRequest {
    let bundle = json!({"org_id":"local","device_id":DEVICE,"setup_id":SETUP,
        "deployment_id":"approved-version","expires_at":3700,"private_config":"SYNTHETIC_CONFIG"});
    let command = SignedCommand {
        command_id: "server-delivery-id".into(),
        idempotency_key: format!("scanner-setup:{SETUP}"),
        device_id: DeviceId(DEVICE.into()),
        catalog_id: "managed_scanner.setup".into(),
        action_class: ActionClass::Destructive,
        payload: json!({}),
        epoch_version: 7,
        signature: String::new(),
        signer_key: context().signing_key_b64,
    };
    let mut request = ManagedDeploymentRequest { command, bundle };
    sign(&mut request);
    request
}

fn sign(request: &mut ManagedDeploymentRequest) {
    request.command.payload = json!({"setup_id":request.bundle["setup_id"],"device_id":DEVICE,
        "deployment_id":request.bundle["deployment_id"],"bundle_sha256":canonical_json_sha256(&request.bundle)});
    let command = &request.command;
    let bytes = canonical_command_bytes(
        &command.idempotency_key,
        &command.device_id.0,
        &command.catalog_id,
        command.action_class.as_wire_str(),
        &command.payload,
        command.epoch_version,
    );
    request.command.signature = STANDARD.encode(key().sign(&bytes).to_bytes());
}

fn bound() -> Authority {
    let mut state = Authority::default();
    state.bind(context()).unwrap();
    state
}

#[test]
fn unprovisioned_service_and_attempted_machine_rebinding_fail_closed() {
    let mut state = Authority::default();
    assert!(matches!(
        state.authorize(&request(), 100),
        Err("management_service_unprovisioned")
    ));
    state.bind(context()).unwrap();
    state.bind(context()).unwrap();
    let mut foreign = context();
    foreign.org_id = "other".into();
    assert_eq!(state.bind(foreign), Err("management_context_conflict"));
    let mut invalid = context();
    invalid.signing_key_b64 = "untrusted".into();
    assert_eq!(
        Authority::default().bind(invalid),
        Err("management_context_invalid")
    );
}

#[test]
fn target_signature_bundle_and_expiry_are_independently_verified() {
    let mut bad_signature = request();
    bad_signature.command.signature = STANDARD.encode([0; 64]);
    assert!(matches!(
        bound().authorize(&bad_signature, 100),
        Err("deployment_signature_invalid")
    ));
    let mut bad_target = request();
    bad_target.bundle["device_id"] = json!("other");
    sign(&mut bad_target);
    assert!(matches!(
        bound().authorize(&bad_target, 100),
        Err("deployment_target_invalid")
    ));
    let mut bad_org = request();
    bad_org.bundle["org_id"] = json!("other");
    sign(&mut bad_org);
    assert!(matches!(
        bound().authorize(&bad_org, 100),
        Err("deployment_target_invalid")
    ));
    let mut changed = request();
    changed.bundle["private_config"] = json!("tampered");
    assert!(matches!(
        bound().authorize(&changed, 100),
        Err("deployment_digest_invalid")
    ));
    for expires in [99, 100, 86_501] {
        let mut expired = request();
        expired.bundle["expires_at"] = json!(expires);
        sign(&mut expired);
        assert!(matches!(
            bound().authorize(&expired, 100),
            Err("deployment_expired")
        ));
    }
    let mut wrong_class = request();
    wrong_class.command.action_class = ActionClass::Safe;
    sign(&mut wrong_class);
    assert!(matches!(
        bound().authorize(&wrong_class, 100),
        Err("deployment_authority_invalid")
    ));
}

#[test]
fn durable_claim_survives_restart_and_does_not_repeat_ambiguous_installation() {
    let mut state = bound();
    assert!(matches!(
        state.authorize(&request(), 100),
        Ok(Decision::Apply { .. })
    ));
    let encoded = serde_json::to_vec(&state).unwrap();
    assert!(!String::from_utf8_lossy(&encoded).contains("SYNTHETIC_CONFIG"));
    let mut reloaded: Authority = serde_json::from_slice(&encoded).unwrap();
    let Decision::Observed(unknown) = reloaded.authorize(&request(), 100).unwrap() else {
        panic!("claim repeated")
    };
    assert_eq!(unknown["state"], "unknown");
    let result = json!({"schema_version":1,"setup_id":SETUP,"device_id":DEVICE,"state":"installed","client_id":"C.1234567890abcdef"});
    reloaded.complete(SETUP, result.clone()).unwrap();
    assert!(
        matches!(reloaded.authorize(&request(),4000),Ok(Decision::Observed(value)) if value==result)
    );
    let mut swapped = request();
    swapped.bundle["private_config"] = json!("different approved request");
    sign(&mut swapped);
    assert!(matches!(
        reloaded.authorize(&swapped, 100),
        Err("deployment_replay_conflict")
    ));
}

#[test]
fn older_epoch_and_unrestricted_helper_output_are_refused() {
    let mut state = bound();
    state.authorize(&request(), 100).unwrap();
    let mut stale = request();
    stale.command.epoch_version = 6;
    sign(&mut stale);
    assert!(matches!(
        state.authorize(&stale, 100),
        Err("deployment_authority_invalid")
    ));
    let base = json!({"schema_version":1,"setup_id":SETUP,"device_id":DEVICE,"state":"failed","reason_code":"package_not_approved"});
    for field in ["contents", "client_config", "file_location"] {
        let mut unsafe_result = base.clone();
        unsafe_result[field] = json!("SYNTHETIC_PRIVATE_VALUE");
        assert_eq!(
            state.complete(SETUP, unsafe_result).unwrap_err(),
            "deployment_receipt_invalid"
        );
    }
    let mut path_reason = base.clone();
    path_reason["reason_code"] = json!(r"C:\Private\file.txt");
    assert_eq!(
        state.complete(SETUP, path_reason).unwrap_err(),
        "deployment_receipt_invalid"
    );
    assert_eq!(state.complete(SETUP, base.clone()).unwrap(), base);
    let mut competing = base;
    competing["state"] = json!("installed");
    assert_eq!(
        state.complete(SETUP, competing).unwrap_err(),
        "deployment_receipt_conflict"
    );
}
