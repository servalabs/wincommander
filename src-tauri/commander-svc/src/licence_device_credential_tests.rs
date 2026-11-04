// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn disposable_registry_credential_is_encrypted_bound_and_not_replaced() {
    let path = format!(
        r"SOFTWARE\WinCommanderLicenceDeviceTest-{}",
        uuid::Uuid::new_v4()
    );
    let anchor = anchor::Anchor::test_user(path.clone());
    let binding = Binding {
        licence_id: "test-licence",
        device_hash: "test-device",
        issuer_origin: "https://license.example",
    };
    let credential = get_or_create_with(&anchor, &MachineDpapi, &binding).unwrap();
    let second = PendingCredential::generate().unwrap();
    assert_ne!(credential.id, second.id);
    assert_ne!(credential.secret(), second.secret());
    let original = anchor.read().unwrap().unwrap();
    let record: Record = serde_json::from_slice(&original).unwrap();
    assert_eq!(record.phase, "pending");
    let mut prior: serde_json::Value = serde_json::from_slice(&original).unwrap();
    prior.as_object_mut().unwrap().remove("receipt");
    prior.as_object_mut().unwrap().remove("accepted_at");
    let previous = decode(
        &MachineDpapi,
        &binding,
        &serde_json::to_vec(&prior).unwrap(),
    )
    .unwrap();
    assert_eq!(previous.secret(), credential.secret());
    assert_ne!(record.sealed, credential.secret().as_slice());
    let reopened = anchor::Anchor::test_user(path.clone());
    let loaded = get_or_create_with(&reopened, &MachineDpapi, &binding).unwrap();
    assert_eq!(loaded.id, credential.id);
    assert_eq!(loaded.secret(), credential.secret());
    let challenge = wincmd_pq_experiment::licence_device_proof::Challenge {
        operation: wincmd_pq_experiment::licence_device_proof::Operation::Refresh,
        credential_id: &credential.id,
        nonce: &[7; 32],
        request_digest: &[9; 32],
        expires_at: 2_000_000_000,
    };
    assert_eq!(
        wincmd_pq_experiment::licence_device_proof::sign(credential.secret(), &challenge),
        wincmd_pq_experiment::licence_device_proof::sign(loaded.secret(), &challenge),
    );
    assert!(save_locked(&anchor, &MachineDpapi, &binding, &second).is_err());
    assert_eq!(anchor.read().unwrap().unwrap(), original);
    for changed in [
        Binding {
            licence_id: "other",
            ..binding
        },
        Binding {
            device_hash: "other",
            ..binding
        },
        Binding {
            issuer_origin: "https://other.example",
            ..binding
        },
    ] {
        assert!(get_or_create_with(&anchor, &MachineDpapi, &changed).is_err());
    }
    let mut altered: Record = serde_json::from_slice(&original).unwrap();
    altered.id[0] ^= 1;
    anchor
        .write(&serde_json::to_vec(&altered).unwrap())
        .unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    assert!(save_locked(&anchor, &MachineDpapi, &binding, &second).is_err());
    altered.id = credential.id;
    let index = altered.sealed.len() / 2;
    altered.sealed[index] ^= 1;
    anchor
        .write(&serde_json::to_vec(&altered).unwrap())
        .unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    let mut false_confirmation: serde_json::Value = serde_json::from_slice(&original).unwrap();
    false_confirmation["phase"] = "confirmed".into();
    anchor
        .write(&serde_json::to_vec(&false_confirmation).unwrap())
        .unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    anchor.write(b"not a credential").unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    assert!(save_locked(&anchor, &MachineDpapi, &binding, &second).is_err());
    anchor::remove_test_key(&path).unwrap();
}

#[test]
fn concurrent_retries_reuse_one_pending_credential() {
    let path = format!(
        r"SOFTWARE\WinCommanderLicenceDeviceRace-{}",
        uuid::Uuid::new_v4()
    );
    let ids = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let anchor = anchor::Anchor::test_user(path.clone());
            let binding = Binding {
                licence_id: "licence",
                device_hash: "device",
                issuer_origin: "https://license.example",
            };
            get_or_create_with(&anchor, &MachineDpapi, &binding)
                .unwrap()
                .id
        });
        let second = scope.spawn(|| {
            let anchor = anchor::Anchor::test_user(path.clone());
            let binding = Binding {
                licence_id: "licence",
                device_hash: "device",
                issuer_origin: "https://license.example",
            };
            get_or_create_with(&anchor, &MachineDpapi, &binding)
                .unwrap()
                .id
        });
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(ids.0, ids.1);
    anchor::remove_test_key(&path).unwrap();
}

#[test]
fn pending_confirmation_requires_both_signatures_and_exact_bound_receipt() {
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    use wincmd_pq_experiment::{jws, Purpose, VerifyError};

    let path = format!(
        r"SOFTWARE\WinCommanderLicenceConfirmTest-{}",
        uuid::Uuid::new_v4()
    );
    let anchor = anchor::Anchor::test_user(path.clone());
    let binding = Binding {
        licence_id: "licence",
        device_hash: "device",
        issuer_origin: "https://license.example",
    };
    let pending = get_or_create_with(&anchor, &MachineDpapi, &binding).unwrap();
    let before = anchor.read().unwrap().unwrap();
    let ed = SigningKey::from_bytes(&[73; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let ed_public = ed.verifying_key().to_bytes();
    let pq_public = pq.public_key().as_ref().try_into().unwrap();
    let trust = jws::Trust {
        purpose: Purpose::LicenceEnrolment,
        minimum_epoch: 7,
        ed_key_id: "ed",
        ed25519_public: &ed_public,
        pq_key_id: "pq",
        ml_dsa_65_public: pq_public,
    };
    let mut hash = Sha256::new();
    hash.update(b"wincommander:licence-device-secret:v1\0");
    hash.update(pending.secret());
    let claims = serde_json::json!({ "kind":"licence-device-enrolment-v1", "licence_id":binding.licence_id,
        "device_hash":binding.device_hash, "issuer_origin":binding.issuer_origin,
        "credential_id":URL_SAFE_NO_PAD.encode(pending.id), "secret_commitment":URL_SAFE_NO_PAD.encode(hash.finalize()),
        "iat":1000, "exp":1100 });
    let sign = |payload: &serde_json::Value, purpose: Purpose| {
        let policy = jws::Trust { purpose, ..trust };
        let bytes = jws::sign::assemble(
            payload.to_string().as_bytes(),
            7,
            &policy,
            |message| Ok(ed.sign(message).to_bytes().to_vec()),
            |message| {
                let mut signature = vec![0; 3309];
                pq.sign(message, &mut signature)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(signature)
            },
        )
        .unwrap();
        String::from_utf8(bytes).unwrap()
    };
    let valid = sign(&claims, Purpose::LicenceEnrolment);
    let mut later_claims = claims.clone();
    later_claims["iat"] = 1001.into();
    let later_receipt = sign(&later_claims, Purpose::LicenceEnrolment);
    let wrong_purpose = sign(&claims, Purpose::Licence);
    let mut wrong = claims.clone();
    wrong["credential_id"] = URL_SAFE_NO_PAD.encode([3u8; 32]).into();
    let wrong_id = sign(&wrong, Purpose::LicenceEnrolment);
    wrong = claims.clone();
    wrong["secret_commitment"] = URL_SAFE_NO_PAD.encode([9u8; 32]).into();
    let wrong_secret = sign(&wrong, Purpose::LicenceEnrolment);
    wrong = claims.clone();
    wrong["licence_id"] = "other".into();
    let wrong_licence = sign(&wrong, Purpose::LicenceEnrolment);
    wrong = claims.clone();
    wrong["device_hash"] = "other".into();
    let wrong_device = sign(&wrong, Purpose::LicenceEnrolment);
    wrong = claims.clone();
    wrong["issuer_origin"] = "https://other.example".into();
    let wrong_issuer = sign(&wrong, Purpose::LicenceEnrolment);
    wrong = claims.clone();
    wrong["exp"] = 2000.into();
    let excessive_lifetime = sign(&wrong, Purpose::LicenceEnrolment);
    let mut incomplete: serde_json::Value = serde_json::from_str(&valid).unwrap();
    incomplete["signatures"].as_array_mut().unwrap().pop();
    for bad in [
        &wrong_purpose,
        &wrong_id,
        &wrong_secret,
        &wrong_licence,
        &wrong_device,
        &wrong_issuer,
        &excessive_lifetime,
        &incomplete.to_string(),
    ] {
        assert!(confirm_with(&anchor, &MachineDpapi, &binding, &trust, bad, 1050).is_err());
        assert_eq!(anchor.read().unwrap().unwrap(), before);
    }
    assert!(confirm_with(&anchor, &MachineDpapi, &binding, &trust, &valid, 1101).is_err());
    assert_eq!(anchor.read().unwrap().unwrap(), before);
    let confirmed = confirm_with(&anchor, &MachineDpapi, &binding, &trust, &valid, 1050).unwrap();
    assert_eq!(confirmed.id, pending.id);
    assert_eq!(confirmed.secret(), pending.secret());
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    let stored = anchor.read().unwrap().unwrap();
    assert!(confirm_with(
        &anchor,
        &MachineDpapi,
        &binding,
        &trust,
        &later_receipt,
        1050
    )
    .is_ok());
    assert_eq!(anchor.read().unwrap().unwrap(), stored);
    assert!(confirm_with(
        &anchor,
        &MachineDpapi,
        &binding,
        &trust,
        &later_receipt,
        2000
    )
    .is_err());
    assert!(decode_confirmed(&MachineDpapi, &binding, &trust, &stored, 1049).is_err());
    let reopened = decode_confirmed(&MachineDpapi, &binding, &trust, &stored, 2000).unwrap();
    assert_eq!(reopened.secret(), pending.secret());
    assert!(confirm_with(&anchor, &MachineDpapi, &binding, &trust, &valid, 2000).is_ok());
    assert!(confirm_with(&anchor, &MachineDpapi, &binding, &trust, &wrong_id, 2000).is_err());
    let mut changed: serde_json::Value = serde_json::from_slice(&stored).unwrap();
    changed["accepted_at"] = 1051.into();
    assert!(decode_confirmed(
        &MachineDpapi,
        &binding,
        &trust,
        &serde_json::to_vec(&changed).unwrap(),
        2000
    )
    .is_err());
    anchor::remove_test_key(&path).unwrap();
}

#[test]
fn worker_enrolment_receipt_interoperates_and_is_not_a_licence() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/workers-enrolment-receipt.json")).unwrap();
    let ed: [u8; 32] = URL_SAFE_NO_PAD
        .decode(fixture["publicEd"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = URL_SAFE_NO_PAD
        .decode(fixture["publicPq"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let trust = jws::Trust {
        purpose: Purpose::LicenceEnrolment,
        minimum_epoch: 7,
        ed_key_id: "ed",
        ed25519_public: &ed,
        pq_key_id: "pq",
        ml_dsa_65_public: &pq,
    };
    let binding = Binding {
        licence_id: "licence",
        device_hash: "device",
        issuer_origin: "https://license.example",
    };
    let credential = PendingCredential {
        id: [23; 32],
        secret: Zeroizing::new([42; 32]),
    };
    let encoded = fixture["receipt"].as_str().unwrap();
    assert!(receipt::verify(encoded, &trust, &binding, &credential, 1050).is_ok());
    let path = format!(
        r"SOFTWARE\WinCommanderWorkerReceiptTest-{}",
        uuid::Uuid::new_v4()
    );
    let anchor = anchor::Anchor::test_user(path.clone());
    save_locked(&anchor, &MachineDpapi, &binding, &credential).unwrap();
    let confirmed = confirm_with(&anchor, &MachineDpapi, &binding, &trust, encoded, 1050).unwrap();
    assert_eq!(confirmed.id, credential.id);
    assert_eq!(confirmed.secret(), credential.secret());
    let stored = anchor.read().unwrap().unwrap();
    assert!(decode_confirmed(&MachineDpapi, &binding, &trust, &stored, 2000).is_ok());
    anchor::remove_test_key(&path).unwrap();
    let other = PendingCredential {
        id: [23; 32],
        secret: Zeroizing::new([43; 32]),
    };
    assert!(receipt::verify(encoded, &trust, &binding, &other, 1050).is_err());
    let licence = jws::Trust {
        purpose: Purpose::Licence,
        ..trust
    };
    assert!(jws::verify(encoded.as_bytes(), &licence).is_err());
}

#[test]
fn authorized_worker_issuer_receipt_confirms_persisted_pending_credential() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/workers-enrolment-issuer.json")).unwrap();
    let ed: [u8; 32] = URL_SAFE_NO_PAD
        .decode(fixture["publicEd"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = URL_SAFE_NO_PAD
        .decode(fixture["publicPq"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let trust = jws::Trust {
        purpose: Purpose::LicenceEnrolment,
        minimum_epoch: 7,
        ed_key_id: "ed",
        ed25519_public: &ed,
        pq_key_id: "pq",
        ml_dsa_65_public: &pq,
    };
    let binding = Binding {
        licence_id: "license-test",
        device_hash: "device-test",
        issuer_origin: "https://license.example",
    };
    let pending = PendingCredential {
        id: [5; 32],
        secret: Zeroizing::new([17; 32]),
    };
    let path = format!(
        r"SOFTWARE\WinCommanderIssuerReceiptTest-{}",
        uuid::Uuid::new_v4()
    );
    let anchor = anchor::Anchor::test_user(path.clone());
    save_locked(&anchor, &MachineDpapi, &binding, &pending).unwrap();
    let receipt = fixture["receipt"].as_str().unwrap();
    let accepted_at = fixture["acceptedAt"].as_u64().unwrap();
    let body = wire::enrolment_request(&binding, &pending, " test-licence-key ").unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body_json["appId"], "com.servalabs.wincommander");
    assert_eq!(body_json["licenseKey"], "test-licence-key");
    assert_eq!(body_json["deviceHash"], binding.device_hash);
    assert_eq!(
        body_json["credentialId"],
        URL_SAFE_NO_PAD.encode(pending.id)
    );
    assert_eq!(
        body_json["deviceSecret"],
        URL_SAFE_NO_PAD.encode(pending.secret())
    );
    assert!(wire::enrolment_request(&binding, &pending, " ").is_err());
    assert!(wire::enrolment_request(&binding, &pending, &"x".repeat(513)).is_err());
    let good_response = serde_json::json!({"ok": true, "receipt": receipt});
    assert_eq!(
        wire::enrolment_receipt(good_response.to_string().as_bytes()).unwrap(),
        receipt
    );
    assert!(wire::enrolment_receipt(br#"{"ok":false,"receipt":"x"}"#).is_err());
    assert!(wire::enrolment_receipt(br#"{"ok":true,"receipt":"x","extra":1}"#).is_err());
    assert!(wire::enrolment_receipt(&[b' '; 65_537]).is_err());

    let before = anchor.read().unwrap().unwrap();
    let lost = enrol_with(
        &anchor,
        &MachineDpapi,
        &binding,
        &trust,
        || accepted_at,
        |id, secret| {
            assert_eq!(id, &pending.id);
            assert_eq!(secret, pending.secret());
            Err("response lost")
        },
    );
    assert!(lost.is_err());
    assert_eq!(anchor.read().unwrap().unwrap(), before);
    let invalid = enrol_with(
        &anchor,
        &MachineDpapi,
        &binding,
        &trust,
        || accepted_at,
        |_, _| Ok("not a receipt".into()),
    );
    assert!(invalid.is_err());
    assert_eq!(anchor.read().unwrap().unwrap(), before);
    let confirmed = enrol_with(
        &anchor,
        &MachineDpapi,
        &binding,
        &trust,
        || accepted_at,
        |id, secret| {
            assert_eq!(id, &pending.id);
            assert_eq!(secret, pending.secret());
            Ok(receipt.to_string())
        },
    )
    .unwrap();
    assert_eq!(confirmed.id, pending.id);
    assert_eq!(confirmed.secret(), pending.secret());
    let body = br#"{"example":"validated request"}"#.to_vec();
    let digest: [u8; 32] = Sha256::digest(&body).into();
    let nonce = [7u8; 32];
    let expiry = accepted_at + 120;
    let challenge_request: serde_json::Value =
        serde_json::from_slice(&wire::challenge_request(&confirmed, &body).unwrap()).unwrap();
    assert_eq!(
        challenge_request["credentialId"],
        URL_SAFE_NO_PAD.encode(pending.id)
    );
    assert_eq!(
        challenge_request["oldRequest"],
        URL_SAFE_NO_PAD.encode(&body)
    );
    let challenge_response = serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(pending.id),
        "nonce": URL_SAFE_NO_PAD.encode(nonce), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": expiry });
    let request = wire::renewal_request(
        &confirmed,
        body.clone(),
        challenge_response.to_string().as_bytes(),
        accepted_at,
    )
    .unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(decoded["oldRequest"], URL_SAFE_NO_PAD.encode(&body));
    assert_eq!(decoded["credentialId"], challenge_response["credentialId"]);
    assert_eq!(decoded["nonce"], challenge_response["nonce"]);
    let mut invalid = challenge_response.clone();
    invalid["requestDigest"] = URL_SAFE_NO_PAD.encode([0u8; 32]).into();
    assert!(wire::renewal_request(
        &confirmed,
        body.clone(),
        invalid.to_string().as_bytes(),
        accepted_at
    )
    .is_err());
    invalid = challenge_response.clone();
    invalid["credentialId"] = URL_SAFE_NO_PAD.encode([0u8; 32]).into();
    assert!(wire::renewal_request(
        &confirmed,
        body.clone(),
        invalid.to_string().as_bytes(),
        accepted_at
    )
    .is_err());
    invalid = challenge_response.clone();
    invalid["expiresAt"] = accepted_at.into();
    assert!(wire::renewal_request(
        &confirmed,
        body.clone(),
        invalid.to_string().as_bytes(),
        accepted_at
    )
    .is_err());
    invalid = challenge_response.clone();
    invalid["unexpected"] = true.into();
    assert!(wire::renewal_request(
        &confirmed,
        body.clone(),
        invalid.to_string().as_bytes(),
        accepted_at
    )
    .is_err());
    assert!(wire::renewal_request(&confirmed, body.clone(), &[b' '; 4097], accepted_at).is_err());
    let proof = sign_refresh_wire(
        &confirmed,
        body.clone(),
        &pending.id,
        &nonce,
        &digest,
        expiry,
        accepted_at,
    )
    .unwrap();
    assert_eq!(proof.body, body);
    let challenge = wincmd_pq_experiment::licence_device_proof::Challenge {
        operation: wincmd_pq_experiment::licence_device_proof::Operation::Refresh,
        credential_id: &pending.id,
        nonce: &nonce,
        request_digest: &digest,
        expires_at: expiry,
    };
    assert_eq!(
        proof.tag,
        wincmd_pq_experiment::licence_device_proof::sign(pending.secret(), &challenge)
    );
    assert!(sign_refresh_wire(
        &confirmed,
        body.clone(),
        &[4; 32],
        &nonce,
        &digest,
        expiry,
        accepted_at
    )
    .is_err());
    assert!(sign_refresh_wire(
        &confirmed,
        body.clone(),
        &pending.id,
        &nonce,
        &[0; 32],
        expiry,
        accepted_at
    )
    .is_err());
    assert!(sign_refresh_wire(
        &confirmed,
        body.clone(),
        &pending.id,
        &nonce,
        &digest,
        accepted_at - 1,
        accepted_at
    )
    .is_err());
    assert!(sign_refresh_wire(
        &confirmed,
        body.clone(),
        &pending.id,
        &nonce,
        &digest,
        accepted_at,
        accepted_at
    )
    .is_err());
    assert!(sign_refresh_wire(
        &confirmed,
        body.clone(),
        &pending.id,
        &nonce,
        &digest,
        accepted_at + 180,
        accepted_at
    )
    .is_ok());
    assert!(sign_refresh_wire(
        &confirmed,
        body.clone(),
        &pending.id,
        &nonce,
        &digest,
        accepted_at + 181,
        accepted_at
    )
    .is_err());
    assert!(sign_refresh_wire(
        &confirmed,
        Vec::new(),
        &pending.id,
        &nonce,
        &digest,
        expiry,
        accepted_at
    )
    .is_err());
    let stored = anchor.read().unwrap().unwrap();
    assert!(decode_confirmed(&MachineDpapi, &binding, &trust, &stored, accepted_at + 1).is_ok());
    assert!(enrol_with(
        &anchor,
        &MachineDpapi,
        &binding,
        &trust,
        || accepted_at + 1,
        |_, _| panic!("confirmed credential must not be retransmitted")
    )
    .is_ok());
    anchor::remove_test_key(&path).unwrap();
}
