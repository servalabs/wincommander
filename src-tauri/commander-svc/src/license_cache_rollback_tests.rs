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
            serde_json::json!({"license_id":id,"device_hash":"device", "iat":iat, "exp":exp, "iss":"wincommander-license-worker", "sub":"com.servalabs.wincommander", "is_portable":false})
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
    let with_generation = |source: &StoreLicenseCacheRequest, generation: Option<u64>, iat: u64| {
        let mut value: serde_json::Value = serde_json::from_str(&source.payload).unwrap();
        value["iat"] = iat.into();
        if let Some(generation) = generation { value["activation_generation"] = generation.into(); }
        let payload = value.to_string();
        let hybrid = jws::sign::assemble(payload.as_bytes(), 8, &current,
            |message| Ok(new_ed.sign(message).to_bytes().to_vec()), |message| {
                let mut sig = vec![0; 3309];
                new_pq.sign(message, &mut sig).map_err(|_| VerifyError::InvalidSignature)?;
                Ok(sig)
            }).unwrap();
        StoreLicenseCacheRequest { signature: general_purpose::URL_SAFE_NO_PAD.encode(classic.sign(payload.as_bytes()).to_bytes()),
            payload, hybrid: Some(String::from_utf8(hybrid).unwrap()), seats_used: None, seat_limit: None }
    };
    let generation_two = with_generation(&next, Some(2), 102);
    let generation_one_later = with_generation(&next, Some(1), 103);
    let generation_missing_later = with_generation(&next, None, 103);
    let generation_three_same_second = with_generation(&next, Some(3), 102);
    assert!(check_previous(&cache(&generation_two), &generation_one_later, &public, &current).is_err());
    assert!(check_previous(&cache(&generation_two), &generation_missing_later, &public, &current).is_err());
    assert!(check_previous(&cache(&generation_two), &generation_three_same_second, &public, &current).is_ok());
    assert!(check_previous(&cache(&next), &generation_two, &public, &current).is_ok());
    assert!(validate_dual_request(&with_generation(&next, Some(0), 103), &public, &current).is_err());
    assert!(validate_dual_request(&with_generation(&next, Some(9_007_199_254_740_992), 103), &public, &current).is_err());
    let generation_record = ledger::advance(None, None, &generation_two, &cache(&generation_two), &public, &current, None).unwrap();
    assert!(ledger::advance(Some(&generation_record), None, &generation_one_later, &cache(&generation_one_later), &public, &current, None).is_err());
    assert!(ledger::advance(Some(&generation_record), None, &generation_missing_later, &cache(&generation_missing_later), &public, &current, None).is_err());
    assert!(ledger::advance(Some(&generation_record), None, &generation_three_same_second, &cache(&generation_three_same_second), &public, &current, None).is_ok());
    let released = ledger::mark_released(&generation_record, &generation_two, 99, &cache(&generation_two), &public, &current, None).unwrap();
    assert!(ledger::verify_released_current(&generation_record, &generation_two, 99, &cache(&generation_two), &public, &current, None).is_err());
    assert!(ledger::verify_released_current(&released, &generation_two, 99, &cache(&generation_two), &public, &current, None).is_ok());
    let released_legacy = ledger::mark_released(&cache(&generation_two), &generation_two, 99, &cache(&generation_two), &public, &current, None).unwrap();
    assert_eq!(ledger::mark_released(&released_legacy, &generation_two, 99, &cache(&generation_two), &public, &current, None).unwrap(), released_legacy);
    assert!(ledger::advance(Some(&released_legacy), None, &generation_two, &cache(&generation_two), &public, &current, None).is_err());
    assert_eq!(ledger::mark_released(&released, &generation_two, 99, &cache(&generation_two), &public, &current, None).unwrap(), released);
    assert!(ledger::mark_released(&released, &generation_two, 100, &cache(&generation_two), &public, &current, None).is_err());
    assert!(ledger::verify_current(&released, &generation_two, 99, &cache(&generation_two), &public, &current, None).is_err());
    assert!(ledger::advance(Some(&released), None, &generation_two, &cache(&generation_two), &public, &current, None).is_err());
    assert!(ledger::advance(Some(&released), None, &generation_one_later, &cache(&generation_one_later), &public, &current, None).is_err());
    let reactivated = ledger::advance(Some(&released), None, &generation_three_same_second, &cache(&generation_three_same_second), &public, &current, None).unwrap();
    assert!(ledger::verify_current(&reactivated, &generation_three_same_second, 99, &cache(&generation_three_same_second), &public, &current, None).is_ok());
    assert!(ledger::advance(Some(&reactivated), None, &generation_two, &cache(&generation_two), &public, &current, None).is_err());
    let other = make("lic-b", 103, 10000, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    let switched = ledger::advance(Some(&released), None, &other, &cache(&other), &public, &current, None).unwrap();
    assert!(ledger::advance(Some(&switched), None, &generation_two, &cache(&generation_two), &public, &current, None).is_err());
    assert!(ledger::advance(Some(&switched), None, &generation_three_same_second, &cache(&generation_three_same_second), &public, &current, None).is_ok());
    let mut corrupt: serde_json::Value = serde_json::from_slice(&released).unwrap();
    corrupt["released"][0]["generation"] = 0.into();
    assert!(ledger::advance(Some(&serde_json::to_vec(&corrupt).unwrap()), None, &other, &cache(&other), &public, &current, None).is_err());
    let mut missing: serde_json::Value = serde_json::from_slice(&released).unwrap();
    missing.as_object_mut().unwrap().remove("released");
    assert!(ledger::advance(Some(&serde_json::to_vec(&missing).unwrap()), None, &other, &cache(&other), &public, &current, None).is_err());
    missing["released"] = serde_json::json!([]);
    assert!(ledger::advance(Some(&serde_json::to_vec(&missing).unwrap()), None, &other, &cache(&other), &public, &current, None).is_err());
    let old_bytes = cache(&previous);
    let rotated_other = make("lic-b", 101, 10000, 8, &new_ed, &new_pq, "new-ed", "new-pq");
    let rotated_bytes = cache(&rotated_other);
    let rotated_ledger = ledger::advance(
        Some(&old_bytes),
        None,
        &rotated_other,
        &rotated_bytes,
        &public,
        &current,
        Some(&historical),
    )
    .unwrap();
    assert!(ledger::verify_current(
        &rotated_ledger,
        &rotated_other,
        99,
        &rotated_bytes,
        &public,
        &current,
        Some(&historical)
    )
    .is_ok());
    assert!(ledger::verify_current(
        &rotated_ledger,
        &rotated_other,
        99,
        &rotated_bytes,
        &public,
        &current,
        None
    )
    .is_err());
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
    // A renewal must not overwrite a changed cache and must preserve seat metadata.
    let renewal_dir = std::env::temp_dir().join(format!(
        "wincmd-renewal-cas-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&renewal_dir).unwrap();
    let retired_path = renewal_dir.join("retired-cache.json");
    let retired_anchor_path = format!(r"SOFTWARE\WinCommanderRetiredGenerationTest-{}", uuid::Uuid::new_v4());
    let retired_anchor = anchor::Anchor::test_user(retired_anchor_path.clone());
    fs::write(&retired_path, cache(&generation_two)).unwrap();
    retired_anchor.write(&generation_record).unwrap();
    assert!(released_snapshot_with_anchor(&retired_path, &generation_two, 99, &public, &current, &retired_anchor).is_err());
    assert!(mark_acknowledged_release_with_anchor(&retired_path, &generation_two, 100, &public, &current, &retired_anchor).is_err());
    assert_eq!(retired_anchor.read().unwrap().unwrap(), generation_record);
    retired_anchor.write(b"damaged").unwrap();
    assert!(mark_acknowledged_release_with_anchor(&retired_path, &generation_two, 99, &public, &current, &retired_anchor).is_err());
    assert_eq!(fs::read(&retired_path).unwrap(), cache(&generation_two));
    retired_anchor.write(&generation_record).unwrap();
    mark_acknowledged_release_with_anchor(&retired_path, &generation_two, 99, &public, &current, &retired_anchor).unwrap();
    assert_eq!(released_snapshot_with_anchor(&retired_path, &generation_two, 99, &public, &current, &retired_anchor).unwrap(), (cache(&generation_two), 2));
    let reopened_anchor = anchor::Anchor::test_user(retired_anchor_path.clone());
    assert!(verify_with_anchor(&retired_path, &generation_two, 99, &public, &current, &reopened_anchor).is_err());
    assert!(with_verified_anchor_mode(&retired_path, &generation_two, 99, &public, &current, &reopened_anchor, true, || Ok(())).is_ok());
    assert!(load_snapshot_with_anchor_mode(&retired_path, &public, &current, &reopened_anchor, false).is_err());
    assert!(load_snapshot_with_anchor_mode(&retired_path, &public, &current, &reopened_anchor, true).is_ok());
    mark_acknowledged_release_with_anchor(&retired_path, &generation_two, 99, &public, &current, &reopened_anchor).unwrap();
    assert!(mark_acknowledged_release_with_anchor(&retired_path, &generation_one_later, 99, &public, &current, &reopened_anchor).is_err());
    let recovery_gate_path = format!(r"SOFTWARE\WinCommanderReleaseRecoveryTest-{}", uuid::Uuid::new_v4());
    let recovery_gate = anchor::Anchor::test_user(recovery_gate_path.clone());
    let context = [8; 32];
    super::super::release_gate::begin_at(&recovery_gate, &context).unwrap();
    super::super::release_gate::bind_request_at(&recovery_gate, &context, b"release").unwrap();
    super::super::release_gate::prepare_finalization_at(&recovery_gate, &context, b"release", "lic-a", "device", &[3; 32], 2, &cache(&generation_two)).unwrap();
    let journal_bytes = recovery_gate.read().unwrap().unwrap();
    let read_again = || with_prepared_release_recovery_at(&public, &current, &anchor::Anchor::test_user(recovery_gate_path.clone()),
        &anchor::Anchor::test_user(retired_anchor_path.clone()), |journal, snapshot, wire| {
            assert_eq!(journal.context, context);
            assert_eq!(journal.credential_id, [3; 32]);
            assert_eq!(snapshot.licence_id, "lic-a");
            assert!(!wire.is_empty());
            Ok(())
        });
    assert!(matches!(read_again(), Ok(Some(()))));
    let remove_once = || with_prepared_release_recovery_at(&public, &current, &anchor::Anchor::test_user(recovery_gate_path.clone()),
        &anchor::Anchor::test_user(retired_anchor_path.clone()), |journal, _, _| remove_prepared_cache_with(&retired_path, journal, false, || Ok(())));
    fs::write(&retired_path, cache(&generation_three_same_second)).unwrap();
    assert!(remove_once().is_err());
    assert_eq!(fs::read(&retired_path).unwrap(), cache(&generation_three_same_second));
    fs::write(&retired_path, cache(&generation_two)).unwrap();
    let fail_before = with_prepared_release_recovery_at(&public, &current, &recovery_gate, &reopened_anchor,
        |journal, _, _| remove_prepared_cache_with(&retired_path, journal, false, || Err("injected")));
    assert!(fail_before.is_err());
    assert_eq!(fs::read(&retired_path).unwrap(), cache(&generation_two));
    let moved_path = renewal_dir.join("replaced-cache.json");
    let moved_parent = renewal_dir.with_extension("moved");
    let raced = with_prepared_release_recovery_at(&public, &current, &recovery_gate, &reopened_anchor,
        |journal, _, _| remove_prepared_cache_with(&retired_path, journal, false, || {
            assert!(fs::write(&retired_path, b"changed").is_err());
            assert!(fs::rename(&retired_path, &moved_path).is_err());
            assert!(fs::rename(&renewal_dir, &moved_parent).is_err());
            Ok(())
        }));
    assert!(matches!(raced, Ok(Some(()))));
    assert!(matches!(remove_once(), Ok(Some(()))));
    assert!(!retired_path.exists());
    // A reopened journal permits the exact operation to finish after a crash.
    assert!(matches!(remove_once(), Ok(Some(()))));
    let complete = |request: &[u8], acknowledgement: &[u8]| with_prepared_release_recovery_at(&public, &current,
        &anchor::Anchor::test_user(recovery_gate_path.clone()), &anchor::Anchor::test_user(retired_anchor_path.clone()),
        |journal, snapshot, _| mark_release_completed_with_anchor(journal, snapshot, request, acknowledgement, &public, &current, &reopened_anchor));
    assert!(matches!(complete(b"release", b"sealed acknowledgement"), Ok(Some(()))));
    let completed_bytes = reopened_anchor.read().unwrap().unwrap();
    assert!(matches!(complete(b"release", b"sealed acknowledgement"), Ok(Some(()))));
    assert_eq!(reopened_anchor.read().unwrap().unwrap(), completed_bytes);
    assert!(complete(b"other request", b"sealed acknowledgement").is_err());
    assert!(complete(b"release", b"different acknowledgement").is_err());
    let mut broken_completion: serde_json::Value = serde_json::from_slice(&completed_bytes).unwrap();
    broken_completion["released"][0].as_object_mut().unwrap().remove("completion");
    reopened_anchor.write(&serde_json::to_vec(&broken_completion).unwrap()).unwrap();
    assert!(read_again().is_err());
    let mut invalid_digest: serde_json::Value = serde_json::from_slice(&completed_bytes).unwrap();
    invalid_digest["released"][0]["completion"]["request_digest"] = "invalid".into();
    reopened_anchor.write(&serde_json::to_vec(&invalid_digest).unwrap()).unwrap();
    assert!(read_again().is_err());
    reopened_anchor.write(&completed_bytes).unwrap();
    let next_activation = ledger::advance(Some(&completed_bytes), None, &generation_three_same_second,
        &cache(&generation_three_same_second), &public, &current, None).unwrap();
    let may_replace = |record: &[u8], credential: &[u8; 32]| ledger::can_replace_completed_release(&next_activation,
        &generation_three_same_second, 99, &cache(&generation_three_same_second), credential, record, &public, &current, None);
    assert_eq!(may_replace(b"sealed acknowledgement", &[3; 32]), Ok(true));
    assert_eq!(may_replace(b"different acknowledgement", &[3; 32]), Ok(false));
    assert_eq!(may_replace(b"sealed acknowledgement", &[4; 32]), Ok(false));
    assert!(ledger::can_replace_completed_release(&completed_bytes, &generation_two, 99, &cache(&generation_two),
        &[3; 32], b"sealed acknowledgement", &public, &current, None).is_err());
    assert!(ledger::can_replace_completed_release(&next_activation, &generation_three_same_second, 100,
        &cache(&generation_three_same_second), &[3; 32], b"sealed acknowledgement", &public, &current, None).is_err());
    let next_release = ledger::mark_released(&next_activation, &generation_three_same_second, 99,
        &cache(&generation_three_same_second), &public, &current, None).unwrap();
    let next_record: serde_json::Value = serde_json::from_slice(&next_release).unwrap();
    assert_eq!(next_record["version"], 2);
    assert!(next_record["released"][0].get("completion").is_none());
    fs::create_dir(&retired_path).unwrap();
    assert!(remove_once().is_err());
    fs::remove_dir(&retired_path).unwrap();
    assert!(matches!(read_again(), Ok(Some(()))));
    let mut modified: serde_json::Value = serde_json::from_slice(&journal_bytes).unwrap();
    modified["finalization"]["generation"] = 3.into();
    recovery_gate.write(&serde_json::to_vec(&modified).unwrap()).unwrap();
    assert!(read_again().is_err());
    recovery_gate.write(&journal_bytes).unwrap();
    assert!(matches!(read_again(), Ok(Some(()))));
    let prepared = super::super::release_gate::prepared_at(&recovery_gate).unwrap().unwrap();
    super::super::release_gate::finish_at(&recovery_gate, &prepared, b"release").unwrap();
    let completed_recovery = |request: &[u8], acknowledgement: &[u8]| with_release_recovery_at(&public, &current,
        &anchor::Anchor::test_user(recovery_gate_path.clone()), &anchor::Anchor::test_user(retired_anchor_path.clone()),
        Some(&retired_path), |journal, snapshot, _| {
            super::super::release_gate::check_request_proof_at(&recovery_gate, &journal.context, request)?;
            let candidate = StoreLicenseCacheRequest { payload: snapshot.payload.clone(), signature: snapshot.signature.clone(),
                hybrid: Some(snapshot.hybrid.clone()), seats_used: None, seat_limit: None };
            let retained = reopened_anchor.read().map_err(|_| "unavailable")?.ok_or("unavailable")?;
            ledger::verify_release_completed(&retained, &candidate, snapshot.last_verified_at, journal,
                request, acknowledgement, &public, &current, None)
        });
    assert!(matches!(completed_recovery(b"release", b"sealed acknowledgement"), Ok(Some(()))));
    assert!(completed_recovery(b"other", b"sealed acknowledgement").is_err());
    assert!(completed_recovery(b"release", b"other").is_err());
    fs::write(&retired_path, cache(&generation_three_same_second)).unwrap();
    assert!(matches!(completed_recovery(b"release", b"sealed acknowledgement"), Ok(None)));
    fs::remove_file(&retired_path).unwrap();
    anchor::remove_test_key(&recovery_gate_path).unwrap();
    anchor::remove_test_key(&retired_anchor_path).unwrap();
    let renewal_path = renewal_dir.join("license_cache.json");
    let renewal_anchor_path = format!(
        r"SOFTWARE\WinCommanderRenewalCas-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    );
    let renewal_anchor = anchor::Anchor::test_user(renewal_anchor_path.clone());
    let mut prior: serde_json::Value = serde_json::from_slice(&high_bytes).unwrap();
    prior["seats_used"] = 1.into();
    prior["seat_limit"] = 3.into();
    let prior_bytes = serde_json::to_vec(&prior).unwrap();
    // Simulate a crash after the protected history was advanced but before the
    // cache file was replaced. Retrying the exact candidate must finish safely.
    let crash_path = renewal_dir.join("interrupted.json");
    let crash_anchor_path = format!(
        r"SOFTWARE\WinCommanderRenewalCrash-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    );
    let crash_anchor = anchor::Anchor::test_user(crash_anchor_path.clone());
    write_with_anchor(
        &crash_path,
        &previous_epoch9,
        &prior_bytes,
        &public,
        &current,
        &crash_anchor,
    )
    .unwrap();
    let gate_path = format!(r"SOFTWARE\WinCommanderReleaseFenceTest-{}", uuid::Uuid::new_v4());
    let gate = anchor::Anchor::test_user(gate_path.clone());
    super::super::release_gate::begin_at(&gate, &[77; 32]).unwrap();
    let before_fence = fs::read(&crash_path).unwrap();
    let retained_before_fence = crash_anchor.read().unwrap();
    let blocked = "license release is in progress or unavailable";
    assert_eq!(write_with_gate(&crash_path, &later, &cache(&later), &public, &current, &crash_anchor, &gate), Err(blocked));
    assert_eq!(write_renewal_with_gate(&crash_path, (&previous_epoch9, 99), &later, 200,
        &public, &current, &crash_anchor, &gate), Err(blocked));
    assert_eq!(fs::read(&crash_path).unwrap(), before_fence);
    assert_eq!(crash_anchor.read().unwrap(), retained_before_fence);
    anchor::remove_test_key(&gate_path).unwrap();
    assert!(
        !recover_pending_with_anchor(&crash_path, 200, &public, &current, &crash_anchor).unwrap()
    );
    let retained = crash_anchor.read().unwrap().unwrap();
    let mut next_cache: serde_json::Value = serde_json::from_slice(&cache(&later)).unwrap();
    next_cache["last_verified_at"] = 100.into();
    next_cache["seats_used"] = 1.into();
    next_cache["seat_limit"] = 3.into();
    let next_bytes = serde_json::to_vec(&next_cache).unwrap();
    let advanced = ledger::advance(
        Some(&retained),
        Some(&prior_bytes),
        &later,
        &next_bytes,
        &public,
        &current,
        None,
    )
    .unwrap();
    crash_anchor.write(&advanced).unwrap();
    let alternative = make("lic-a", 102, 9999, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_renewal_with_anchor(
        &crash_path,
        (&previous_epoch9, 99),
        &alternative,
        200,
        &public,
        &current,
        &crash_anchor
    )
    .is_err());
    assert!(write_renewal_with_anchor(
        &crash_path,
        (&previous_epoch9, 99),
        &other_same_second,
        200,
        &public,
        &current,
        &crash_anchor
    )
    .is_err());
    assert!(
        recover_pending_with_anchor(&crash_path, 99, &public, &current, &crash_anchor).is_err()
    );
    assert_eq!(fs::read(&crash_path).unwrap(), prior_bytes);
    assert!(
        recover_pending_with_anchor(&crash_path, 200, &public, &current, &crash_anchor).unwrap()
    );
    assert!(
        !recover_pending_with_anchor(&crash_path, 200, &public, &current, &crash_anchor).unwrap()
    );
    assert!(verify_with_anchor(&crash_path, &later, 100, &public, &current, &crash_anchor).is_ok());
    let recovered_bytes = fs::read(&crash_path).unwrap();
    crash_anchor.write(b"invalid").unwrap();
    assert!(
        recover_pending_with_anchor(&crash_path, 200, &public, &current, &crash_anchor).is_err()
    );
    assert_eq!(fs::read(&crash_path).unwrap(), recovered_bytes);
    anchor::remove_test_key(&crash_anchor_path).unwrap();
    write_with_anchor(
        &renewal_path,
        &previous_epoch9,
        &prior_bytes,
        &public,
        &current,
        &renewal_anchor,
    )
    .unwrap();
    assert!(write_renewal_with_anchor(
        &renewal_path,
        (&previous_epoch9, 99),
        &other_same_second,
        200,
        &public,
        &current,
        &renewal_anchor
    )
    .is_err());
    let outcomes = std::thread::scope(|scope| {
        let attempt = || {
            let local_anchor = anchor::Anchor::test_user(renewal_anchor_path.clone());
            write_renewal_with_anchor(
                &renewal_path,
                (&previous_epoch9, 99),
                &later,
                200,
                &public,
                &current,
                &local_anchor,
            )
        };
        let first = scope.spawn(attempt);
        let second = scope.spawn(attempt);
        [
            first.join().unwrap().is_ok(),
            second.join().unwrap().is_ok(),
        ]
    });
    assert_eq!(outcomes.into_iter().filter(|ok| *ok).count(), 1);
    let after = fs::read(&renewal_path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&after).unwrap();
    assert_eq!(saved["seats_used"], 1);
    assert_eq!(saved["seat_limit"], 3);
    assert_eq!(saved["last_verified_at"], 101);
    assert!(verify_with_anchor(
        &renewal_path,
        &later,
        101,
        &public,
        &current,
        &renewal_anchor
    )
    .is_ok());
    assert!(write_renewal_with_anchor(
        &renewal_path,
        (&previous_epoch9, 99),
        &later,
        200,
        &public,
        &current,
        &renewal_anchor
    )
    .is_err());
    assert_eq!(fs::read(&renewal_path).unwrap(), after);
    anchor::remove_test_key(&renewal_anchor_path).unwrap();
    fs::remove_dir_all(renewal_dir).unwrap();
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
    let anchor_path = format!(
        r"SOFTWARE\WinCommanderLicenceTest-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    );
    let anchor = anchor::Anchor::test_user(anchor_path.clone());
    fs::write(&path, &high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor).is_err());
    // Upgrade the previous single-record format without losing its floor.
    anchor.write(&high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor).is_ok());
    // Confirmation runs while cache writers are excluded, and never runs for
    // a stale cache. This closes the check-then-confirm race.
    let mut called = false;
    with_verified_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor, || {
        called = true;
        assert!(WRITE_LOCK.try_lock().is_err());
        Ok(())
    }).unwrap();
    assert!(called);
    called = false;
    assert!(with_verified_anchor(&path, &previous_epoch9, 100, &public, &current, &anchor, || {
        called = true;
        Ok(())
    }).is_err());
    assert!(!called);
    assert!(verify_with_anchor(&path, &previous_epoch9, 100, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(
        &path,
        &same_second,
        b"replacement",
        &public,
        &current,
        &anchor
    )
    .is_err());
    assert_eq!(fs::read(&path).unwrap(), high_bytes);
    assert!(write_with_anchor(&path, &next, b"replacement", &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), high_bytes);
    assert!(write_with_anchor(
        &path,
        &previous_epoch9,
        &high_bytes,
        &public,
        &current,
        &anchor
    )
    .is_ok());
    let retained_before_check = anchor.read().unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor).is_ok());
    assert_eq!(anchor.read().unwrap(), retained_before_check);
    let mut changed_time: serde_json::Value = serde_json::from_slice(&high_bytes).unwrap();
    changed_time["last_verified_at"] = 9999.into();
    fs::write(&path, serde_json::to_vec(&changed_time).unwrap()).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor).is_err());
    changed_time
        .as_object_mut()
        .unwrap()
        .remove("last_verified_at");
    fs::write(&path, serde_json::to_vec(&changed_time).unwrap()).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor).is_err());
    fs::write(&path, &high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &same_second, 99, &public, &current, &anchor).is_err());
    fs::remove_file(&path).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor).is_err());
    let older = make("lic-a", 99, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_with_anchor(&path, &older, &cache(&older), &public, &current, &anchor).is_err());
    assert!(!path.exists());
    assert!(write_with_anchor(
        &path,
        &exact_retry,
        &cache(&exact_retry),
        &public,
        &current,
        &anchor
    )
    .is_ok());
    let later_bytes = cache(&later);
    assert!(ledger::advance(None, None, &later, &high_bytes, &public, &current, None).is_err());
    let mut ambiguous: serde_json::Value = serde_json::from_slice(&high_bytes).unwrap();
    ambiguous["version"] = 1.into();
    ambiguous["records"] = serde_json::json!([]);
    assert!(ledger::advance(
        Some(&serde_json::to_vec(&ambiguous).unwrap()),
        None,
        &later,
        &later_bytes,
        &public,
        &current,
        None
    )
    .is_err());
    let missing_parent = dir.join("missing").join("license_cache.json");
    assert!(write_with_anchor(
        &missing_parent,
        &later,
        &later_bytes,
        &public,
        &current,
        &anchor
    )
    .is_err());
    assert!(verify_with_anchor(&path, &exact_retry, 99, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(
        &path,
        &previous_epoch9,
        &high_bytes,
        &public,
        &current,
        &anchor
    )
    .is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    assert!(verify_with_anchor(&path, &later, 99, &public, &current, &anchor).is_ok());
    fs::write(&path, &high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &previous_epoch9, 99, &public, &current, &anchor).is_err());
    fs::write(&path, &later_bytes).unwrap();
    let other = make("lic-b", 102, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(
        &path,
        &previous_epoch9,
        &high_bytes,
        &public,
        &current,
        &anchor
    )
    .is_err());
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
    let mut damaged: serde_json::Value =
        serde_json::from_slice(&anchor.read().unwrap().unwrap()).unwrap();
    damaged["records"][0] = "invalid".into();
    anchor
        .write(&serde_json::to_vec(&damaged).unwrap())
        .unwrap();
    assert!(verify_with_anchor(&path, &later, 99, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), later_bytes);
    anchor.write(b"invalid").unwrap();
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), later_bytes);
    // Missing anchor: preserve the verified current licence before switching.
    anchor::remove_test_key(&anchor_path).unwrap();
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(
        &path,
        &previous_epoch9,
        &high_bytes,
        &public,
        &current,
        &anchor
    )
    .is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    // A restored older anchor and newer cache must be reconciled first.
    anchor.write(&high_bytes).unwrap();
    assert!(verify_with_anchor(&path, &later, 99, &public, &current, &anchor).is_err());
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_err());
    assert_eq!(fs::read(&path).unwrap(), later_bytes);
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(&path, &other, &cache(&other), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(
        &path,
        &previous_epoch9,
        &high_bytes,
        &public,
        &current,
        &anchor
    )
    .is_err());
    // A restored history with a different licence retains the current cache too.
    anchor.write(&cache(&other)).unwrap();
    fs::write(&path, &later_bytes).unwrap();
    let third = make("lic-c", 103, 10000, 9, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(write_with_anchor(&path, &third, &cache(&third), &public, &current, &anchor).is_ok());
    assert!(write_with_anchor(
        &path,
        &previous_epoch9,
        &high_bytes,
        &public,
        &current,
        &anchor
    )
    .is_err());
    assert!(write_with_anchor(&path, &later, &later_bytes, &public, &current, &anchor).is_ok());
    fs::remove_file(path).unwrap();
    anchor::remove_test_key(&anchor_path).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn worker_issued_licence_survives_history_reload_and_rejects_tampering() {
    use wincmd_pq_experiment::ML_DSA_65_PUBLIC_KEY_LEN;
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
    let public = fixture["publicClassical"].as_str().unwrap();
    let request = StoreLicenseCacheRequest {
        payload: fixture["payload"].as_str().unwrap().to_owned(),
        signature: fixture["signature"].as_str().unwrap().to_owned(),
        hybrid: Some(fixture["hybrid"].as_str().unwrap().to_owned()),
        seats_used: None,
        seat_limit: None,
    };
    let verified_at = serde_json::from_str::<serde_json::Value>(&request.payload).unwrap()["iat"]
        .as_u64()
        .unwrap();
    let cache = serde_json::to_vec(&serde_json::json!({"token":{"payload":request.payload,"signature":request.signature,"hybrid":request.hybrid},"last_verified_at":verified_at})).unwrap();
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("wincmd-worker-history-{}-{id}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("license_cache.json");
    let anchor_path = format!(
        r"SOFTWARE\WinCommanderWorkerHistoryTest-{}-{id}",
        std::process::id()
    );
    let anchor = anchor::Anchor::test_user(anchor_path.clone());
    write_with_anchor(&path, &request, &cache, public, &trust, &anchor).unwrap();
    drop(anchor);
    let reopened = anchor::Anchor::test_user(anchor_path.clone());
    verify_with_anchor(&path, &request, verified_at, public, &trust, &reopened).unwrap();
    let snapshot = load_snapshot_with_anchor(&path, public, &trust, &reopened).unwrap();
    assert_eq!(snapshot.licence_id, "licence-test");
    assert_eq!(snapshot.device_hash, "device-test");
    assert_eq!(snapshot.payload, request.payload);
    assert_eq!(snapshot.signature, request.signature);
    assert_eq!(Some(snapshot.hybrid), request.hybrid);
    assert_eq!(snapshot.last_verified_at, verified_at);
    assert!(
        verify_with_anchor(&path, &request, verified_at + 1, public, &trust, &reopened).is_err()
    );
    let mut tampered: serde_json::Value = serde_json::from_slice(&cache).unwrap();
    tampered["last_verified_at"] = (verified_at + 1).into();
    fs::write(&path, serde_json::to_vec(&tampered).unwrap()).unwrap();
    assert!(load_snapshot_with_anchor(&path, public, &trust, &reopened).is_err());
    assert!(
        verify_with_anchor(&path, &request, verified_at + 1, public, &trust, &reopened).is_err()
    );
    let mut altered_identity: serde_json::Value = serde_json::from_slice(&cache).unwrap();
    altered_identity["token"]["payload"] = request.payload.replace("device-test", "other-device").into();
    fs::write(&path, serde_json::to_vec(&altered_identity).unwrap()).unwrap();
    assert!(load_snapshot_with_anchor(&path, public, &trust, &reopened).is_err());
    let mut missing_hybrid: serde_json::Value = serde_json::from_slice(&cache).unwrap();
    missing_hybrid["token"].as_object_mut().unwrap().remove("hybrid");
    fs::write(&path, serde_json::to_vec(&missing_hybrid).unwrap()).unwrap();
    assert!(load_snapshot_with_anchor(&path, public, &trust, &reopened).is_err());
    fs::write(&path, &cache).unwrap();
    anchor::remove_test_key(&anchor_path).unwrap();
    assert!(load_snapshot_with_anchor(&path, public, &trust, &reopened).is_err());
    assert!(verify_with_anchor(&path, &request, verified_at, public, &trust, &reopened).is_err());
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
#[ignore = "explicit acceptance: requires elevated Windows to set test ACLs"]
fn release_cache_delete_enforces_protected_windows_acls() {
    fn icacls(path: &Path, args: &[&str]) {
        let output = std::process::Command::new("icacls.exe").arg(path).args(args).output().unwrap();
        assert!(output.status.success(), "icacls failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let dir = std::env::temp_dir().join(format!("wincmd-release-acl-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&dir).unwrap();
    icacls(&dir, &["/setowner", "*S-1-5-32-544"]);
    icacls(&dir, &["/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"]);
    let path = dir.join("license_cache.json");
    fs::write(&path, b"synthetic cache").unwrap();
    icacls(&path, &["/setowner", "*S-1-5-32-544"]);
    let journal = super::super::release_gate::PreparedRelease { context: [1; 32], licence_id: "licence".into(),
        device_hash: "device".into(), credential_id: [2; 32], generation: 1, cache: b"synthetic cache".to_vec() };
    icacls(&path, &["/grant:r", "*S-1-1-0:F"]);
    assert!(remove_prepared_cache_with(&path, &journal, true, || Ok(())).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"synthetic cache");
    icacls(&path, &["/remove:g", "*S-1-1-0"]);
    let linked_file = dir.join("linked-cache.json");
    std::os::windows::fs::symlink_file(&path, &linked_file).unwrap();
    assert!(remove_prepared_cache_with(&linked_file, &journal, true, || Ok(())).is_err());
    fs::remove_file(&linked_file).unwrap();
    let linked_dir = dir.join("linked-parent");
    std::os::windows::fs::symlink_dir(&dir, &linked_dir).unwrap();
    assert!(remove_prepared_cache_with(&linked_dir.join("license_cache.json"), &journal, true, || Ok(())).is_err());
    fs::remove_dir(&linked_dir).unwrap();
    remove_prepared_cache_with(&path, &journal, true, || Ok(())).unwrap();
    assert!(!path.exists());
    remove_prepared_cache_with(&path, &journal, true, || Ok(())).unwrap();
    fs::remove_dir(dir).unwrap();
}
