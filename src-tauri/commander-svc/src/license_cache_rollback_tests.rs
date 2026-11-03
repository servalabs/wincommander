// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::{jws, Purpose, VerifyError};

#[test]
fn planned_rotation_preserves_history_without_accepting_old_issuance() {
    let classic = SigningKey::from_bytes(&[7; 32]);
    let old_ed = SigningKey::from_bytes(&[31; 32]);
    let old_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let new_ed = SigningKey::from_bytes(&[42; 32]);
    let new_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let old_ed_public = old_ed.verifying_key().to_bytes();
    let old_pq_public: [u8; wincmd_pq_experiment::ML_DSA_65_PUBLIC_KEY_LEN] =
        old_pq.public_key().as_ref().try_into().unwrap();
    let new_ed_public = new_ed.verifying_key().to_bytes();
    let new_pq_public: [u8; wincmd_pq_experiment::ML_DSA_65_PUBLIC_KEY_LEN] =
        new_pq.public_key().as_ref().try_into().unwrap();
    let current = jws::Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 8,
        ed_key_id: "new-ed",
        ed25519_public: &new_ed_public,
        pq_key_id: "new-pq",
        ml_dsa_65_public: &new_pq_public,
    };
    let old_ed_encoded = general_purpose::URL_SAFE_NO_PAD.encode(old_ed_public);
    let old_pq_encoded = general_purpose::URL_SAFE_NO_PAD.encode(old_pq_public);
    let fields = [
        Some(old_ed_encoded.as_str()),
        Some(old_pq_encoded.as_str()),
        Some("old-ed"),
        Some("old-pq"),
        Some("7"),
    ];
    let historical = history::parse(&current, fields).unwrap().unwrap();
    let current_ed_encoded = general_purpose::URL_SAFE_NO_PAD.encode(new_ed_public);
    let current_pq_encoded = general_purpose::URL_SAFE_NO_PAD.encode(new_pq_public);
    assert!(history::parse(
        &current,
        [
            Some(&current_ed_encoded),
            Some(&current_pq_encoded),
            fields[2],
            fields[3],
            fields[4]
        ]
    )
    .is_ok());
    assert!(history::parse(&current, [None; 5]).unwrap().is_none());
    assert!(history::parse(&current, [Some("bad"), None, None, None, None]).is_err());
    assert!(history::parse(
        &current,
        [fields[0], fields[1], fields[2], fields[3], Some("8")]
    )
    .is_err());
    let public = general_purpose::URL_SAFE_NO_PAD.encode(classic.verifying_key().to_bytes());
    let make = |id: &str,
                iat: u64,
                exp: u64,
                epoch: u64,
                ed: &SigningKey,
                pq: &PqdsaKeyPair,
                ed_id: &str,
                pq_id: &str| {
        let ed_public = ed.verifying_key().to_bytes();
        let pq_public = pq.public_key().as_ref().try_into().unwrap();
        let policy = jws::Trust {
            purpose: Purpose::Licence,
            minimum_epoch: 1,
            ed_key_id: ed_id,
            ed25519_public: &ed_public,
            pq_key_id: pq_id,
            ml_dsa_65_public: pq_public,
        };
        let payload =
            serde_json::json!({"license_id":id,"device_hash":"device", "iat":iat, "exp":exp})
                .to_string();
        let hybrid = jws::sign::assemble(
            payload.as_bytes(),
            epoch,
            &policy,
            |message| Ok(ed.sign(message).to_bytes().to_vec()),
            |message| {
                let mut sig = vec![0; 3309];
                pq.sign(message, &mut sig)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(sig)
            },
        )
        .unwrap();
        StoreLicenseCacheRequest {
            signature: general_purpose::URL_SAFE_NO_PAD
                .encode(classic.sign(payload.as_bytes()).to_bytes()),
            payload,
            hybrid: Some(String::from_utf8(hybrid).unwrap()),
            seats_used: None,
            seat_limit: None,
        }
    };
    let cache = |request: &StoreLicenseCacheRequest| {
        serde_json::to_vec(&serde_json::json!({"token":{"payload":request.payload,"signature":request.signature,"hybrid":request.hybrid},"last_verified_at":99})).unwrap()
    };
    let previous = make("lic-a", 100, 10000, 7, &old_ed, &old_pq, "old-ed", "old-pq");
    let next = make("lic-a", 101, 10000, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    let old_bytes = cache(&previous);
    let rotated_other = make("lic-b", 101, 10000, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    let rotated_bytes = cache(&rotated_other);
    let rotated_ledger = ledger::advance(Some(&old_bytes), None, &rotated_other, &rotated_bytes, &public, &current, Some(&historical)).unwrap();
    assert!(ledger::verify_current(&rotated_ledger, &rotated_other, &rotated_bytes, &public, &current, Some(&historical)).is_ok());
    assert!(ledger::verify_current(&rotated_ledger, &rotated_other, &rotated_bytes, &public, &current, None).is_err());
    assert!(validate_dual_request(&next, &public, &current).is_ok());
    assert!(
        check_previous_with_history(&old_bytes, &next, &public, &current, Some(&historical))
            .is_ok()
    );
    assert!(check_previous_with_history(&old_bytes, &next, &public, &current, None).is_err());
    let same_keys_old = make("lic-a", 100, 10000, 7, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(check_previous(&cache(&same_keys_old), &next, &public, &current).is_ok());
    let replay = make("lic-a", 99, 10000, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(
        check_previous_with_history(&old_bytes, &replay, &public, &current, Some(&historical))
            .is_err()
    );
    let other = make("lic-b", 99, 10000, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(
        check_previous_with_history(&old_bytes, &other, &public, &current, Some(&historical))
            .is_ok()
    );
    assert!(validate_dual_request(&previous, &public, &current).is_err());
    let old_too_late = make("lic-a", 100, 10000, 8, &old_ed, &old_pq, "old-ed", "old-pq");
    assert!(check_previous_with_history(
        &cache(&old_too_late),
        &next,
        &public,
        &current,
        Some(&historical)
    )
    .is_err());
    let previous_epoch9 = make("lic-a", 100, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    let high_bytes = cache(&previous_epoch9);
    assert!(check_previous(&high_bytes, &next, &public, &current).is_err());
    let same_second = make("lic-a", 100, 9999, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    let exact_retry = make("lic-a", 100, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    let later = make("lic-a", 101, 9999, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    let other_same_second = make("lic-b", 100, 9999, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(validate_dual_request(&same_second, &public, &current).is_ok());
    assert!(check_previous(&high_bytes, &same_second, &public, &current).is_err());
    assert!(check_previous(&high_bytes, &exact_retry, &public, &current).is_ok());
    assert!(check_previous(&high_bytes, &later, &public, &current).is_ok());
    assert!(check_previous(&high_bytes, &other_same_second, &public, &current).is_ok());
    let mut forged = previous;
    forged.hybrid.as_mut().unwrap().push('x');
    assert!(check_previous_with_history(
        &cache(&forged),
        &next,
        &public,
        &current,
        Some(&historical)
    )
    .is_err());
    let legacy = serde_json::to_vec(
        &serde_json::json!({"token":{"payload":forged.payload,"signature":forged.signature}}),
    )
    .unwrap();
    assert!(check_previous(&legacy, &next, &public, &current).is_ok());
    assert!(check_previous(b"damaged", &next, &public, &current).is_err());
    let dir = std::env::temp_dir().join(format!(
        "wincmd-license-rollback-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("license_cache.json");
    let anchor_path = format!(r"SOFTWARE\WinCommanderLicenceTest-{}-{}", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed));
    let anchor = anchor::Anchor::test_user(anchor_path.clone());
    fs::write(&path, &high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, &public, &current, &anchor).is_err());
    // Upgrade the previous single-record format without losing its floor.
    anchor.write(&high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(&path, &same_second, b"replacement", &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), high_bytes);
    assert!(write_with_anchor(&path, &next, b"replacement", &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), high_bytes);
    assert!(write_with_anchor(&path, &previous_epoch9, &high_bytes, &public, &current, &anchor).is_ok());
    let retained_before_check = anchor.read().unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, &public, &current, &anchor).is_ok());
    assert_eq!(anchor.read().unwrap(), retained_before_check);
    let mut changed_time: serde_json::Value = serde_json::from_slice(&high_bytes).unwrap();
    changed_time["last_verified_at"] = 9999.into();
    fs::write(&path, serde_json::to_vec(&changed_time).unwrap()).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, &public, &current, &anchor).is_err());
    changed_time.as_object_mut().unwrap().remove("last_verified_at");
    fs::write(&path, serde_json::to_vec(&changed_time).unwrap()).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, &public, &current, &anchor).is_err());
    fs::write(&path, &high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &same_second, &public, &current, &anchor).is_err());
    fs::remove_file(&path).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, &public, &current, &anchor).is_err());
    let older = make("lic-a", 99, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_with_anchor(&path, &older, &cache(&older), &public, &current, &anchor).is_err());
    assert!(!path.exists());
    assert!(write_with_anchor(&path, &exact_retry, &cache(&exact_retry), &public, &current, &anchor).is_ok());
    let later_bytes = cache(&later);
    assert!(ledger::advance(None, None, &later, &high_bytes, &public, &current, None).is_err());
    let mut ambiguous: serde_json::Value = serde_json::from_slice(&high_bytes).unwrap();
    ambiguous["version"] = 1.into();
    ambiguous["records"] = serde_json::json!([]);
    assert!(ledger::advance(Some(&serde_json::to_vec(&ambiguous).unwrap()), None, &later, &later_bytes, &public, &current, None).is_err());
    let missing_parent = dir.join("missing").join("license_cache.json");
    assert!(write_with_anchor(&missing_parent, &later, &later_bytes, &public, &current, &anchor).is_err());
    assert!(verify_with_anchor(&path, &exact_retry, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &previous_epoch9, &high_bytes, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    assert!(verify_with_anchor(&path, &later, &public, &current, &anchor).is_ok());
    fs::write(&path, &high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, &public, &current, &anchor).is_err());
    fs::write(&path, &later_bytes).unwrap();
    let other = make("lic-b", 102, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(&path, &previous_epoch9, &high_bytes, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    for id in ["lic-c", "lic-d"] {
        let item = make(id, 103, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
        assert!(write_with_anchor(&path, &item, &cache(&item), &public, &current, &anchor).is_ok());
    }
    let before = fs::read(&path).unwrap();
    let full = anchor.read().unwrap().unwrap();
    let fifth = make("lic-e", 104, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_with_anchor(&path, &fifth, &cache(&fifth), &public, &current, &anchor).is_err());
    assert_eq!(anchor.read().unwrap().unwrap(), full);
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::write(&path, cache(&fifth)).unwrap();
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_err());
    assert_eq!(anchor.read().unwrap().unwrap(), full);
    assert_eq!(fs::read(&path).unwrap(), cache(&fifth));
    fs::write(&path, &before).unwrap();
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    let mut damaged: serde_json::Value = serde_json::from_slice(&anchor.read().unwrap().unwrap()).unwrap();
    damaged["records"][0] = "invalid".into();
    anchor.write(&serde_json::to_vec(&damaged).unwrap()).unwrap();
    assert!(verify_with_anchor(&path, &later, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), later_bytes);
    anchor.write(b"invalid").unwrap();
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), later_bytes);
    // Missing anchor: preserve the verified current licence before switching.
    anchor::remove_test_key(&anchor_path).unwrap();
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(&path, &previous_epoch9, &high_bytes, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    // A restored older anchor and newer cache must be reconciled first.
    anchor.write(&high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &later, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), later_bytes);
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(&path, &previous_epoch9, &high_bytes, &public, &current, &anchor).is_err());
    // A restored history with a different licence retains the current cache too.
    anchor.write(&cache(&other)).unwrap();
    fs::write(&path, &later_bytes).unwrap();
    let third = make("lic-c", 103, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_with_anchor(&path, &third, &cache(&third), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(&path, &previous_epoch9, &high_bytes, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    fs::remove_file(path).unwrap();
    anchor::remove_test_key(&anchor_path).unwrap();
    fs::remove_dir(dir).unwrap();
}
