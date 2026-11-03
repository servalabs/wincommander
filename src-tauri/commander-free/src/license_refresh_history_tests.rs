// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::VerifyError;

#[test]
fn configured_old_worker_fixture_can_only_request_refresh() {
    if option_env!("WINCMD_PQ_LIC_TEST_TRANSITION") != Some("1") {
        return;
    }
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../commander-svc/src/fixtures/workers-hybrid-licence.json"
    ))
    .unwrap();
    let token = SignedTokenEnvelope {
        payload: fixture["payload"].as_str().unwrap().to_owned(),
        signature: fixture["signature"].as_str().unwrap().to_owned(),
        hybrid: Some(fixture["hybrid"].as_str().unwrap().to_owned()),
    };
    let public = fixture["publicClassical"].as_str().unwrap();
    assert!(verify_token_claims(&token, public).is_err());
    let (_, historical) = verify_token_for_refresh(&token, public).unwrap();
    assert!(historical);
}

#[test]
fn old_proof_is_valid_for_bounded_refresh_but_not_current_entitlement() {
    let classic = SigningKey::from_bytes(&[7; 32]);
    let old_ed = SigningKey::from_bytes(&[31; 32]);
    let old_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let new_ed = SigningKey::from_bytes(&[42; 32]);
    let new_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let new_ed_public = new_ed.verifying_key().to_bytes();
    let new_pq_public: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
        new_pq.public_key().as_ref().try_into().unwrap();
    let current = jws::Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 8,
        ed_key_id: "new-ed",
        ed25519_public: &new_ed_public,
        pq_key_id: "new-pq",
        ml_dsa_65_public: &new_pq_public,
    };
    let old_ed_public = old_ed.verifying_key().to_bytes();
    let old_pq_public: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
        old_pq.public_key().as_ref().try_into().unwrap();
    let ed_encoded = general_purpose::URL_SAFE_NO_PAD.encode(old_ed_public);
    let pq_encoded = general_purpose::URL_SAFE_NO_PAD.encode(old_pq_public);
    let fields = [
        Some(ed_encoded.as_str()),
        Some(pq_encoded.as_str()),
        Some("old-ed"),
        Some("old-pq"),
        Some("7"),
        Some("1000"),
    ];
    let historical = parse(&current, fields).unwrap().unwrap();
    assert!(parse(&current, [None; 6]).unwrap().is_none());
    assert!(parse(&current, [fields[0], None, None, None, None, None]).is_err());
    assert!(parse(
        &current,
        [
            fields[0],
            fields[1],
            fields[2],
            fields[3],
            Some("8"),
            fields[5]
        ]
    )
    .is_err());
    let old_policy = jws::Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 1,
        ed_key_id: "old-ed",
        ed25519_public: &old_ed_public,
        pq_key_id: "old-pq",
        ml_dsa_65_public: &old_pq_public,
    };
    let public = general_purpose::URL_SAFE_NO_PAD.encode(classic.verifying_key().to_bytes());
    let make = |plan: &str, epoch| {
        let payload = serde_json::json!({"license_id":"lic-a", "device_hash":"device", "plan":plan, "iat":1, "exp":10000}).to_string();
        let proof = jws::sign::assemble(
            payload.as_bytes(),
            epoch,
            &old_policy,
            |message| Ok(old_ed.sign(message).to_bytes().to_vec()),
            |message| {
                let mut sig = vec![0; 3309];
                old_pq
                    .sign(message, &mut sig)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(sig)
            },
        )
        .unwrap();
        SignedTokenEnvelope {
            signature: general_purpose::URL_SAFE_NO_PAD
                .encode(classic.sign(payload.as_bytes()).to_bytes()),
            payload,
            hybrid: Some(String::from_utf8(proof).unwrap()),
        }
    };
    let old = make("pro", 7);
    assert!(verify_matching_hybrid(&old, &current).is_err());
    assert!(verify(&old, &public, &current, &historical, 999).is_ok());
    assert!(verify(&old, &public, &current, &historical, 1001).is_err());
    assert!(verify(&old, &public, &current, &historical, 0).is_err());
    assert!(verify(&make("pro", 8), &public, &current, &historical, 999).is_err());
    assert!(verify(&make("trial", 7), &public, &current, &historical, 999).is_err());
    let same_ed = general_purpose::URL_SAFE_NO_PAD.encode(new_ed_public);
    let same_pq = general_purpose::URL_SAFE_NO_PAD.encode(new_pq_public);
    let same_history = parse(
        &current,
        [
            Some(&same_ed),
            Some(&same_pq),
            Some("new-ed"),
            Some("new-pq"),
            Some("7"),
            Some("1000"),
        ],
    )
    .unwrap()
    .unwrap();
    let same_policy = jws::Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 1,
        ed_key_id: "new-ed",
        ed25519_public: &new_ed_public,
        pq_key_id: "new-pq",
        ml_dsa_65_public: &new_pq_public,
    };
    let same_proof = jws::sign::assemble(
        old.payload.as_bytes(),
        7,
        &same_policy,
        |message| Ok(new_ed.sign(message).to_bytes().to_vec()),
        |message| {
            let mut sig = vec![0; 3309];
            new_pq
                .sign(message, &mut sig)
                .map_err(|_| VerifyError::InvalidSignature)?;
            Ok(sig)
        },
    )
    .unwrap();
    let mut same_token = old.clone();
    same_token.hybrid = Some(String::from_utf8(same_proof).unwrap());
    assert!(verify_matching_hybrid(&same_token, &current).is_err());
    assert!(verify(&same_token, &public, &current, &same_history, 999).is_ok());
    let mut forged = old;
    forged.hybrid.as_mut().unwrap().push('x');
    assert!(verify(&forged, &public, &current, &historical, 999).is_err());
}
