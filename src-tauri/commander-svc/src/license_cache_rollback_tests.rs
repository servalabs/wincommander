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
    .is_err());
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
            serde_json::json!({"license_id":id,"device_hash":"device", "iat":iat, "exp":10000})
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
        serde_json::to_vec(&serde_json::json!({"token":{"payload":request.payload,"signature":request.signature,"hybrid":request.hybrid}})).unwrap()
    };
    let previous = make("lic-a", 100, 7, &old_ed, &old_pq, "old-ed", "old-pq");
    let next = make("lic-a", 101, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    let old_bytes = cache(&previous);
    assert!(validate_dual_request(&next, &public, &current).is_ok());
    assert!(
        check_previous_with_history(&old_bytes, &next, &public, &current, Some(&historical))
            .is_ok()
    );
    assert!(check_previous_with_history(&old_bytes, &next, &public, &current, None).is_err());
    let same_keys_old = make("lic-a", 100, 7, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(check_previous(&cache(&same_keys_old), &next, &public, &current).is_ok());
    let replay = make("lic-a", 99, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(
        check_previous_with_history(&old_bytes, &replay, &public, &current, Some(&historical))
            .is_err()
    );
    let other = make("lic-b", 99, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(
        check_previous_with_history(&old_bytes, &other, &public, &current, Some(&historical))
            .is_ok()
    );
    assert!(validate_dual_request(&previous, &public, &current).is_err());
    let old_too_late = make("lic-a", 100, 8, &old_ed, &old_pq, "old-ed", "old-pq");
    assert!(check_previous_with_history(
        &cache(&old_too_late),
        &next,
        &public,
        &current,
        Some(&historical)
    )
    .is_err());
    let previous_epoch9 = make("lic-a", 100, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    let high_bytes = cache(&previous_epoch9);
    assert!(check_previous(&high_bytes, &next, &public, &current).is_err());
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
    fs::write(&path, &high_bytes).unwrap();
    assert!(write(&path, &next, b"replacement", &public, &current).is_err());
    assert_eq!(fs::read(&path).unwrap(), high_bytes);
    assert!(write(&path, &previous_epoch9, &high_bytes, &public, &current).is_ok());
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
}
