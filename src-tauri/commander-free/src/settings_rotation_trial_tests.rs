// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use serde_json::json;

fn sections(key: &[u8; 32]) -> Vec<(String, String)> {
    let old = derive_section_key(key, None).unwrap();
    [
        ("settings", json!({"private_value":"first secret"})),
        ("payments", json!({"token":"second secret"})),
    ]
    .into_iter()
    .map(|(name, value)| {
        (
            name.into(),
            encode_section(
                &old,
                &serde_json::to_vec(&value).unwrap(),
                &format!("machine:{name}"),
            )
            .unwrap(),
        )
    })
    .collect()
}

#[test]
fn full_generation_uses_fresh_protected_material_and_reopens_all_sections() {
    let old = [42; 32];
    let input = sections(&old);
    let before = input.clone();
    let first = prepare(&old, &input).unwrap();
    let second = prepare(&old, &input).unwrap();
    assert_ne!(first, second);
    assert_eq!(input, before);
    let raw = std::str::from_utf8(&first).unwrap();
    for secret in ["first secret", "second secret", "private_value", "payments"] {
        assert!(!raw.contains(secret));
    }
    let first_envelope: Bundle = serde_json::from_slice(&first).unwrap();
    let unwrapped = Zeroizing::new(
        dpapi_unprotect(&B64.decode(&first_envelope.protected_material).unwrap()).unwrap(),
    );
    assert_ne!(unwrapped.as_slice(), &old);
    let opened = open(&first).unwrap();
    assert_eq!(opened["settings"]["private_value"], "first secret");
    assert_eq!(opened["payments"]["token"], "second secret");
}
#[test]
fn invalid_or_ambiguous_sections_abort_before_publication() {
    let old = [42; 32];
    let mut input = sections(&old);
    assert!(prepare(&[43; 32], &input).is_err());
    input[1].1.push('X');
    assert!(prepare(&old, &input).is_err());
    let duplicate = vec![sections(&old)[0].clone(), sections(&old)[0].clone()];
    assert!(prepare(&old, &duplicate).is_err());
}
#[test]
fn failed_publication_retains_old_generation_and_success_reopens_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("generation");
    let old = [42; 32];
    let existing = prepare(&old, &sections(&old)).unwrap();
    std::fs::write(&destination, &existing).unwrap();
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    assert!(publish(&destination, &candidate, &old, &input, &|_, _| Err(
        "injected failure".into()
    ))
    .is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), existing);
    publish(&destination, &candidate, &old, &input, &crate::datastore_io::atomic_write).unwrap();
    assert_eq!(
        open(&std::fs::read(&destination).unwrap()).unwrap()["payments"]["token"],
        "second secret"
    );
    let mut damaged = candidate;
    let i = damaged.len() - 2;
    damaged[i] ^= 1;
    assert!(publish(&destination, &damaged, &old, &input, &crate::datastore_io::atomic_write).is_err());
    assert_ne!(std::fs::read(destination).unwrap(), damaged);
}

#[test]
fn unknown_commit_outcome_can_be_read_back_without_assuming_rollback() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("generation");
    let old = [42; 32];
    let before = prepare(&old, &sections(&old)).unwrap();
    std::fs::write(&destination, &before).unwrap();
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let result = publish(&destination, &candidate, &old, &input, &|path, bytes| {
        crate::datastore_io::atomic_write(path, bytes)?;
        Err("commit outcome unknown".into())
    });
    assert!(result.is_err());
    let observed = std::fs::read(&destination).unwrap();
    assert_eq!(observed, candidate);
    assert_eq!(
        open(&observed).unwrap()["settings"]["private_value"],
        "first secret"
    );
}

#[test]
fn legacy_v1_section_is_preserved_in_fresh_generation() {
    use super::super::{nonce_from_bytes, Aead, Aes256Gcm, KeyInit, FORMAT_PREFIX_V1};
    let old = [42; 32];
    let key = derive_section_key(&old, None).unwrap();
    let nonce_bytes = [7u8; 12];
    let cipher = Aes256Gcm::new((&key).into());
    let encrypted = cipher
        .encrypt(
            nonce_from_bytes(&nonce_bytes).unwrap(),
            b"{\"old\":true}".as_ref(),
        )
        .unwrap();
    let mut bytes = nonce_bytes.to_vec();
    bytes.extend(encrypted);
    let encoded = format!("{FORMAT_PREFIX_V1}{}", B64.encode(bytes));
    let bundle = prepare(&old, &[("settings".into(), encoded)]).unwrap();
    assert_eq!(open(&bundle).unwrap()["settings"]["old"], true);
}

#[test]
fn unscoped_legacy_ciphertext_is_rejected_for_new_generation() {
    use super::super::{nonce_from_bytes, Aead, Aes256Gcm, KeyInit, FORMAT_PREFIX_V1};
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let mut bundle: Bundle = serde_json::from_slice(&candidate).unwrap();
    let protected = B64.decode(&bundle.protected_material).unwrap();
    let material = Zeroizing::new(dpapi_unprotect(&protected).unwrap());
    let material: &[u8; 32] = material.as_slice().try_into().unwrap();
    let key = derive_section_key(material, None).unwrap();
    let nonce_bytes = [9u8; 12];
    let cipher = Aes256Gcm::new((&key).into());
    let payload = cipher
        .encrypt(
            nonce_from_bytes(&nonce_bytes).unwrap(),
            br#"{"settings":{}}"#.as_ref(),
        )
        .unwrap();
    let mut raw = nonce_bytes.to_vec();
    raw.extend(payload);
    bundle.ciphertext = format!("{FORMAT_PREFIX_V1}{}", B64.encode(raw));
    let substituted = serde_json::to_vec(&bundle).unwrap();
    assert!(open(&substituted).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generation");
    std::fs::write(&path, &candidate).unwrap();
    assert!(publish(&path, &substituted, &old, &input, &crate::datastore_io::atomic_write).is_err());
    assert_eq!(std::fs::read(path).unwrap(), candidate);
}

#[test]
fn reopened_generation_rejects_changed_missing_added_or_duplicate_sources() {
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("generation");
    publish(&path, &candidate, &old, &input, &crate::datastore_io::atomic_write).unwrap();
    let reopened = std::fs::read(&path).unwrap();
    verify_source(&reopened, &old, &input).unwrap();
    assert!(verify_source(&reopened, &[43; 32], &input).is_err());
    let mut changed = input.clone();
    changed[0].1.push('X');
    assert!(verify_source(&reopened, &old, &changed).is_err());
    assert!(verify_source(&reopened, &old, &input[..1]).is_err());
    let mut added = input.clone();
    added.push(("new_section".into(), "new data".into()));
    assert!(verify_source(&reopened, &old, &added).is_err());
    let mut duplicate = input.clone();
    duplicate.push(input[0].clone());
    assert!(verify_source(&reopened, &old, &duplicate).is_err());
    assert_eq!(std::fs::read(path).unwrap(), candidate);
}

#[test]
fn corrupt_authenticated_generation_cannot_validate_a_source_snapshot() {
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let mut bundle: Bundle = serde_json::from_slice(&candidate).unwrap();
    let mut ciphertext = bundle.ciphertext.into_bytes();
    let index = "enc:v2:".len() + 4;
    ciphertext[index] = if ciphertext[index] == b'A' { b'B' } else { b'A' };
    bundle.ciphertext = String::from_utf8(ciphertext).unwrap();
    let tampered = serde_json::to_vec(&bundle).unwrap();
    assert!(verify_source(&tampered, &old, &input).is_err());
}

#[test]
fn validly_resealed_candidate_cannot_change_values_with_original_manifest() {
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let mut bundle: Bundle = serde_json::from_slice(&candidate).unwrap();
    let protected = B64.decode(&bundle.protected_material).unwrap();
    let material = Zeroizing::new(dpapi_unprotect(&protected).unwrap());
    let material: &[u8; 32] = material.as_slice().try_into().unwrap();
    let key = Zeroizing::new(derive_section_key(material, None).unwrap());
    let plaintext = Zeroizing::new(decode_section(&key, &bundle.ciphertext, SCOPE).unwrap());
    let mut generation: Generation = serde_json::from_slice(&plaintext).unwrap();
    generation.sections.get_mut("payments").unwrap()["token"] = json!("forged secret");
    let forged_plain = Zeroizing::new(serde_json::to_vec(&generation).unwrap());
    bundle.ciphertext = encode_section(&key, &forged_plain, SCOPE).unwrap();
    let forged = serde_json::to_vec(&bundle).unwrap();
    // This is a valid AES-GCM generation; only comparison to old values rejects it.
    assert_eq!(open(&forged).unwrap()["payments"]["token"], "forged secret");
    assert!(verify_source(&forged, &old, &input).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generation");
    std::fs::write(&path, &candidate).unwrap();
    assert!(publish(&path, &forged, &old, &input, &crate::datastore_io::atomic_write).is_err());
    assert_eq!(std::fs::read(path).unwrap(), candidate);
}

#[test]
fn staged_readback_rejects_corruption_and_preserves_existing_destination() {
    use std::io::{Seek, SeekFrom, Write};
    let dir = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let existing = dir.path().join("existing");
    std::fs::write(&existing, b"do not replace").unwrap();
    assert!(stage_new_with_hook(&existing, &candidate, &old, &input, &|_, _| Ok(())).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(), b"do not replace");
    let staged = dir.path().join("staged");
    let result = stage_new_with_hook(&staged, &candidate, &old, &input, &|_, file| {
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        file.write_all(b"X").map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    });
    assert!(result.is_err());
    assert!(staged.exists());
    assert_eq!(std::fs::read(existing).unwrap(), b"do not replace");
}

#[test]
fn staged_readback_rejects_swapped_file_and_oversized_data() {
    let dir = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let staged = dir.path().join("staged");
    let moved = dir.path().join("moved");
    let swapped = stage_new_with_hook(&staged, &candidate, &old, &input, &|path, _| {
        std::fs::rename(path, &moved).map_err(|e| e.to_string())?;
        std::fs::write(path, &candidate).map_err(|e| e.to_string())
    });
    assert!(swapped.is_err());
    let oversized = dir.path().join("oversized");
    let result = stage_new_with_hook(&oversized, &candidate, &old, &input, &|_, file| {
        file.set_len((MAX_TOTAL * 2 + 65537) as u64).map_err(|e| e.to_string())
    });
    assert!(result.is_err());
}

#[test]
fn staged_readback_succeeds_for_an_unchanged_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let staged = dir.path().join("staged");
    stage_new_with_hook(&staged, &candidate, &old, &input, &|_, _| Ok(())).unwrap();
    assert_eq!(std::fs::read(staged).unwrap(), candidate);
}

#[cfg(unix)]
#[test]
fn held_stage_guard_detects_path_swap_after_first_readback() {
    let dir = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let path = dir.path().join("stage");
    let moved = dir.path().join("original-stage");
    let stage = stage_new_with_hook(&path, &candidate, &old, &input, &|_, _| Ok(())).unwrap();
    std::fs::rename(&path, &moved).unwrap();
    std::fs::write(&path, &candidate).unwrap();
    assert!(stage.verify(&candidate, &old, &input).is_err());
    assert_eq!(std::fs::read(moved).unwrap(), candidate);
}

#[test]
fn restart_recovery_authenticates_stage_and_leaves_partial_or_stale_files_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let path = dir.path().join("stage");
    drop(stage_new_with_hook(&path, &candidate, &old, &input, &|_, _| Ok(())).unwrap());
    let (guard, recovered) = recover_stage(&path, &old, &input).unwrap();
    assert_eq!(recovered, candidate);
    guard.verify(&candidate, &old, &input).unwrap();
    drop(guard);
    let changed = sections(&[43; 32]);
    assert!(recover_stage(&path, &old, &changed).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), candidate);
    std::fs::write(&path, &candidate[..candidate.len()/2]).unwrap();
    let partial = std::fs::read(&path).unwrap();
    assert!(recover_stage(&path, &old, &input).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), partial);
}

#[test]
fn selected_generation_preserves_f6_and_legacy_and_recovers_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let mut input = sections(&old);
    let old_key = derive_section_key(&old, None).unwrap();
    let f6 = serde_json::to_vec(&json!({"signing_seed_hex": "a1b2c3"})).unwrap();
    input.push(("f6-keystore".into(), encode_section(&old_key, &f6, "machine:f6-keystore").unwrap()));
    let legacy = root.path().join("f6-keystore.dat");
    std::fs::write(&legacy, &input[2].1).unwrap();
    let path = root.path().join("selected-generation");
    let candidate = prepare(&old, &input).unwrap();
    select_initial_generation(&path, &candidate, &old, &input, &crate::datastore_io::publish_new).unwrap();
    let restarted = std::fs::read(&path).unwrap();
    assert_eq!(open(&restarted).unwrap()["f6-keystore"]["signing_seed_hex"], "a1b2c3");
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), input[2].1);
    let another_candidate = prepare(&old, &input).unwrap();
    select_initial_generation(&path, &another_candidate, &old, &input, &crate::datastore_io::publish_new).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), restarted);
}

#[test]
fn selected_generation_rejects_corruption_stale_sources_and_ambiguous_publish() {
    let root = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let mut forged = open_generation(&candidate).unwrap();
    forged.sections.insert("unexpected".into(), json!({"value": true}));
    assert!(verify_source(&seal_generation(&forged).unwrap(), &old, &input).is_err());
    let path = root.path().join("selected-generation");
    assert!(select_initial_generation(&path, &candidate, &old, &input, &|_, _| Err("write failed".into())).is_err());
    assert!(!path.exists());
    select_initial_generation(&path, &candidate, &old, &input, &|path, bytes| {
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
        Err("ambiguous after commit".into())
    }).unwrap();
    let mut changed = input.clone();
    changed[0].1.push('X');
    assert!(select_initial_generation(&path, &candidate, &old, &changed, &crate::datastore_io::publish_new).is_err());
    std::fs::write(&path, b"partial generation").unwrap();
    assert!(select_initial_generation(&path, &candidate, &old, &input, &crate::datastore_io::publish_new).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"partial generation");
}

#[test]
fn selected_generation_update_preserves_other_sections_and_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let old = [42; 32];
    let input = sections(&old);
    let path = root.path().join("selected-generation");
    let initial = prepare(&old, &input).unwrap();
    select_initial_generation(&path, &initial, &old, &input, &crate::datastore_io::publish_new).unwrap();
    assert!(update_selected_generation(&path, "settings", json!({"changed": true}), &|_, _| Err("injected failure".into())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), initial);
    update_selected_generation(&path, "settings", json!({"changed": true}), &crate::datastore_io::atomic_write).unwrap();
    let updated = std::fs::read(&path).unwrap();
    let contents = open(&updated).unwrap();
    assert_eq!(contents["settings"]["changed"], true);
    assert_eq!(contents["payments"]["token"], "second secret");
    assert!(update_selected_generation(&path, "SETTINGS", json!({}), &crate::datastore_io::atomic_write).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), updated);
    update_selected_generation(&path, "new-section", json!({"enabled": true}), &crate::datastore_io::atomic_write).unwrap();
    let added = open(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(added["new-section"]["enabled"], true);
    assert_eq!(added["payments"]["token"], "second secret");
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(update_selected_generation(&path, "settings", json!({}), &crate::datastore_io::atomic_write).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
}

#[test]
fn locked_selected_generation_serializes_owners_and_never_falls_back() {
    let root = tempfile::tempdir().unwrap();
    assert!(LockedSelectedGeneration::try_open(root.path()).is_err());
    let old = [42; 32];
    let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let path = root.path().join(".settings-generation.v1");
    select_initial_generation(&path, &candidate, &old, &input, &crate::datastore_io::publish_new).unwrap();
    let generation = LockedSelectedGeneration::try_open(root.path()).unwrap();
    assert!(LockedSelectedGeneration::try_open(root.path()).is_err());
    generation.write_section("settings", json!({"new_value": true})).unwrap();
    assert_eq!(generation.read_all().unwrap()["settings"]["new_value"], true);
    assert_eq!(generation.read_all().unwrap()["payments"]["token"], "second secret");
    drop(generation);
    assert!(LockedSelectedGeneration::try_open(root.path()).is_ok());
    std::fs::write(path, b"broken").unwrap();
    assert!(LockedSelectedGeneration::try_open(root.path()).is_err());
}
