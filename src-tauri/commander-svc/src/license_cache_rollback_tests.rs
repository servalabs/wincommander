// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

#[test]
fn older_same_licence_is_rejected_without_blocking_legacy_or_new_licence() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/workers-hybrid-licence.json")).unwrap();
    let ed: [u8; 32] = general_purpose::URL_SAFE_NO_PAD
        .decode(fixture["publicEd"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = general_purpose::URL_SAFE_NO_PAD
        .decode(fixture["publicPq"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let trust = jws::Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 7,
        ed_key_id: "ed-key",
        ed25519_public: &ed,
        pq_key_id: "pq-key",
        ml_dsa_65_public: &pq,
    };
    let payload = fixture["payload"].as_str().unwrap();
    let public = fixture["publicClassical"].as_str().unwrap();
    let mut claims: serde_json::Value = serde_json::from_str(payload).unwrap();
    let issued = claims["iat"].as_u64().unwrap();
    let existing = serde_json::json!({"token": {"payload": payload, "signature":fixture["signature"], "hybrid":fixture["hybrid"]}});
    let bytes = serde_json::to_vec(&existing).unwrap();
    let make = |claims: &serde_json::Value| StoreLicenseCacheRequest {
        payload: claims.to_string(),
        signature: String::new(),
        hybrid: None,
        seats_used: None,
        seat_limit: None,
    };
    // The production caller validates the candidate's signatures before this check.
    let current = StoreLicenseCacheRequest {
        payload: payload.to_owned(),
        signature: fixture["signature"].as_str().unwrap().to_owned(),
        hybrid: Some(fixture["hybrid"].as_str().unwrap().to_owned()),
        seats_used: None,
        seat_limit: None,
    };
    assert!(validate_dual_request(&current, public, &trust).is_ok());
    assert!(check_previous(&bytes, &current, public, &trust).is_ok());
    claims["iat"] = (issued - 1).into();
    assert!(check_previous(&bytes, &make(&claims), public, &trust).is_err());
    let dir = std::env::temp_dir().join(format!(
        "wincmd-license-rollback-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("license_cache.json");
    fs::write(&path, &bytes).unwrap();
    assert!(write(&path, &make(&claims), b"replacement", public, &trust).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(write(&path, &current, &bytes, public, &trust).is_ok());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
    claims["iat"] = (issued + 1).into();
    assert!(check_previous(&bytes, &make(&claims), public, &trust).is_ok());
    claims["iat"] = (issued - 1).into();
    claims["license_id"] = "another-licence".into();
    assert!(check_previous(&bytes, &make(&claims), public, &trust).is_ok());
    let mut legacy = existing.clone();
    legacy["token"].as_object_mut().unwrap().remove("hybrid");
    assert!(check_previous(
        &serde_json::to_vec(&legacy).unwrap(),
        &make(&claims),
        public,
        &trust
    )
    .is_ok());
    let mut forged = existing;
    forged["token"]["hybrid"] = "invalid".into();
    assert!(check_previous(
        &serde_json::to_vec(&forged).unwrap(),
        &make(&claims),
        public,
        &trust
    )
    .is_err());
    assert!(check_previous(b"damaged", &make(&claims), public, &trust).is_err());
}
