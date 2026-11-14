// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn legacy_source_proof_binds_scope_key_names_and_exact_source_bytes() {
    let generation = |scope, key: &[u8], sections: &[(&str, &[u8])]| Generation {
        scope, f6_initialized: false, sections: BTreeMap::new(),
        source_key_digest: source_digest(key),
        source_sections: sections.iter().map(|(name, bytes)| ((*name).into(), source_digest(bytes))).collect(),
    };
    let one = generation(StoreScope::Machine, b"key-one", &[("ab", b"c"), ("d", b"e")]);
    let baseline = legacy_source_proof(&one).unwrap();
    assert_eq!(baseline.len(), 64);
    assert_eq!(baseline, legacy_source_proof(&one).unwrap());
    for other in [
        generation(StoreScope::CurrentUser, b"key-one", &[("ab", b"c"), ("d", b"e")]),
        generation(StoreScope::Machine, b"key-two", &[("ab", b"c"), ("d", b"e")]),
        generation(StoreScope::Machine, b"key-one", &[("a", b"c"), ("bd", b"e")]),
        generation(StoreScope::Machine, b"key-one", &[("ab", b"changed"), ("d", b"e")]),
        generation(StoreScope::Machine, b"key-one", &[("ab", b"c")]),
    ] { assert_ne!(legacy_source_proof(&other).unwrap(), baseline); }
}
use serde_json::json;

// Machine-lock fixture requires an elevated Windows test process; it creates
// only a disposable test file and never repairs an existing descriptor.
fn provision_test_lock(root: &std::path::Path) {
    #[cfg(windows)]
    crate::datastore::settings_generation_acl_windows::provision_or_verify(root).unwrap();
    #[cfg(not(windows))]
    std::fs::write(root.join(".settings-generation.lock"), b"").unwrap();
}

fn provision_user_test_lock(root: &std::path::Path) {
    #[cfg(windows)]
    crate::datastore::settings_generation_acl_windows::provision_user_or_verify(root).unwrap();
    #[cfg(not(windows))]
    std::fs::write(root.join(".settings-generation.lock"), b"").unwrap();
}

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

include!("settings_rotation_recovery_tests.rs");
include!("settings_rotation_install_tests.rs");
