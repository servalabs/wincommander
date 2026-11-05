// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
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
    let f6 = serde_json::to_vec(&json!({"signing_seed_hex": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})).unwrap();
    input.push(("f6-keystore".into(), encode_section(&old_key, &f6, "machine:f6-keystore").unwrap()));
    let legacy = root.path().join("f6-keystore.dat");
    std::fs::write(&legacy, &input[2].1).unwrap();
    let path = root.path().join("selected-generation");
    let candidate = prepare(&old, &input).unwrap();
    select_initial_generation(&path, &candidate, &old, &input, &crate::datastore_io::publish_new).unwrap();
    let restarted = std::fs::read(&path).unwrap();
    assert_eq!(open(&restarted).unwrap()["f6-keystore"]["signing_seed_hex"], "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
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
    update_selected_generation(&path, "SETTINGS", json!({"case_changed": true}), &crate::datastore_io::atomic_write).unwrap();
    let case_update = open(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(case_update["settings"]["case_changed"], true);
    assert!(!case_update.contains_key("SETTINGS"));
    update_selected_generation(&path, "new-section", json!({"enabled": true}), &crate::datastore_io::atomic_write).unwrap();
    let added = open(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(added["new-section"]["enabled"], true);
    assert_eq!(added["payments"]["token"], "second secret");
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(update_selected_generation(&path, "settings", json!({}), &crate::datastore_io::atomic_write).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
}

use crate::datastore::settings_generation_cutover::{self as cutover, Anchor, AnchorState, ProtectedFileAnchor, SelectedFile};

fn fixture_anchor(root: &std::path::Path, scope: StoreScope) -> ProtectedFileAnchor {
    let anchor = ProtectedFileAnchor::new(root, scope);
    anchor.provision_legacy().unwrap();
    anchor
}
fn fixture_select(root: &std::path::Path, scope: StoreScope, candidate: &[u8], old: &[u8; 32], input: &[(String, String)]) {
    let owner = LockedSelectedGeneration::acquire_for(root, scope).unwrap();
    let anchor = ProtectedFileAnchor::new(root, scope);
    cutover::select_initial(&owner._ownership, &anchor, &SelectedFile::new(root), candidate,
        &|bytes| verify_source_for(scope, bytes, old, input)).unwrap();
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn anchored_recovery_fails_closed_and_reopens_committed_generations() {
    let root = tempfile::tempdir().unwrap();
    provision_test_lock(root.path());
    let old = [42; 32]; let input = sections(&old);
    let original = prepare(&old, &input).unwrap();
    assert!(inspect_startup_once(root.path(), StoreScope::Machine).is_err());
    let anchor = fixture_anchor(root.path(), StoreScope::Machine);
    assert_eq!(inspect_startup_once(root.path(), StoreScope::Machine).unwrap(), StartupView::Legacy);
    anchor.write(&AnchorState::Preparing { next: cutover::digest(&original) }).unwrap();
    assert_eq!(inspect_startup_once(root.path(), StoreScope::Machine).unwrap(), StartupView::RecoveryRequired);
    std::fs::write(root.path().join(".settings-generation.v1"), &original).unwrap();
    assert!(LockedSelectedGeneration::try_open(root.path()).is_err());
    fixture_select(root.path(), StoreScope::Machine, &original, &old, &input);
    let next = updated_generation(&original, "settings", json!({"updated": true})).unwrap();
    let pending = AnchorState::Updating { prior: cutover::digest(&original), next: cutover::digest(&next) };
    anchor.write(&pending).unwrap();
    assert_eq!(inspect_startup_once(root.path(), StoreScope::Machine).unwrap(), StartupView::RecoveryRequired);
    assert!(read_with_legacy(root.path(), StoreScope::Machine, "settings", || panic!("must not fall back")).unwrap().get("private_value").is_some());
    anchor.write(&pending).unwrap();
    crate::datastore_io::atomic_write(&root.path().join(".settings-generation.v1"), &next).unwrap();
    assert_eq!(read_with_legacy(root.path(), StoreScope::Machine, "settings", || panic!("must not fall back")).unwrap()["updated"], true);
    std::fs::remove_file(root.path().join(".settings-cutover-anchor.v1")).unwrap();
    assert!(inspect_startup_once(root.path(), StoreScope::Machine).is_err());
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn current_user_generation_preserves_mixed_f6_and_rejects_machine_scope() {
    let root = tempfile::tempdir().unwrap(); provision_user_test_lock(root.path());
    let old = [42; 32]; let mut input = sections(&old);
    let wrapped = crate::datastore::settings_section_envelope::seal("user-install:f6-keystore",
        br#"{"signing_seed_hex":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#, user_dpapi_protect, user_dpapi_unprotect).unwrap();
    input.push(("f6-keystore".into(), wrapped));
    let candidate = prepare_for(StoreScope::CurrentUser, &old, &input).unwrap();
    assert!(verify_source_for(StoreScope::Machine, &candidate, &old, &input).is_err());
    fixture_anchor(root.path(), StoreScope::CurrentUser);
    fixture_select(root.path(), StoreScope::CurrentUser, &candidate, &old, &input);
    assert!(LockedSelectedGeneration::try_open_for(root.path(), StoreScope::Machine).is_err());
    let user = LockedSelectedGeneration::try_open_for(root.path(), StoreScope::CurrentUser).unwrap();
    assert_eq!(user.read_all().unwrap()["f6-keystore"]["signing_seed_hex"], "a".repeat(64));
    user.write_section("SETTINGS", json!({"user": true})).unwrap(); drop(user);
    let values = read_selected_once(root.path(), StoreScope::CurrentUser).unwrap();
    assert_eq!(values["settings"]["user"], true);
    assert!(!values.contains_key("SETTINGS"));
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn guarded_runtime_never_falls_back_from_pending_and_preserves_f6() {
    use std::cell::Cell;
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let anchor = fixture_anchor(root.path(), StoreScope::Machine);
    let calls = Cell::new(0);
    assert_eq!(read_with_legacy(root.path(), StoreScope::Machine, "settings", || { calls.set(calls.get()+1); Ok(json!({"legacy":true})) }).unwrap()["legacy"], true);
    let old = [42; 32]; let mut input = sections(&old); let key = derive_section_key(&old, None).unwrap();
    input.push(("f6-keystore".into(), encode_section(&key, br#"{"signing_seed_hex":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#, "machine:f6-keystore").unwrap()));
    let candidate = prepare(&old, &input).unwrap();
    anchor.write(&AnchorState::Preparing { next: cutover::digest(&candidate) }).unwrap();
    assert!(read_with_legacy(root.path(), StoreScope::Machine, "settings", || panic!("must not fall back")).is_err());
    assert!(write_with_legacy(root.path(), StoreScope::Machine, "settings", &json!({}), || panic!("must not fall back")).is_err());
    fixture_select(root.path(), StoreScope::Machine, &candidate, &old, &input);
    let f6 = read_with_legacy(root.path(), StoreScope::Machine, "F6-KEYSTORE", || panic!("must not fall back")).unwrap();
    write_with_legacy(root.path(), StoreScope::Machine, "settings", &json!({"updated":true}), || panic!("must not fall back")).unwrap();
    assert_eq!(read_with_legacy(root.path(), StoreScope::Machine, "f6-keystore", || panic!("must not fall back")).unwrap(), f6);
    assert_eq!(calls.get(), 1);
}

#[test]
fn invalid_preexisting_or_initialized_f6_fails_closed() {
    let old = [42; 32]; let old_key = derive_section_key(&old, None).unwrap();
    for value in [json!({}), json!({"signing_seed_hex":"bad"}), json!({"signing_seed_hex": 4})] {
        let mut input = sections(&old);
        input.push(("f6-keystore".into(), encode_section(&old_key, &serde_json::to_vec(&value).unwrap(), "machine:f6-keystore").unwrap()));
        assert!(prepare(&old, &input).is_err());
    }
    let input = sections(&old); let candidate = prepare(&old, &input).unwrap();
    let mut state = open_generation(&candidate).unwrap(); state.f6_initialized = true;
    assert!(open_generation(&seal_generation(&state).unwrap()).is_err());
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn f6_shared_read_rechecks_and_concurrent_creators_return_one_winner() {
    use std::sync::{Arc, Barrier, atomic::{AtomicUsize, Ordering}};
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let old = [42; 32]; let input = sections(&old); let candidate = prepare(&old, &input).unwrap();
    fixture_anchor(root.path(), StoreScope::Machine); fixture_select(root.path(), StoreScope::Machine, &candidate, &old, &input);
    let barrier = Arc::new(Barrier::new(2)); let created = Arc::new(AtomicUsize::new(0));
    let workers: Vec<_> = (1..=2).map(|index| {
        let path = root.path().to_path_buf(); let barrier = barrier.clone(); let created = created.clone();
        std::thread::spawn(move || {
            barrier.wait();
            read_f6_or_create(&path, StoreScope::Machine, || panic!("no legacy fallback"), || {
                let owner = (0..15).find_map(|_| LockedSelectedGeneration::acquire(&path).ok()).ok_or("could not acquire ownership")?;
                owner.get_or_create_f6(|| { created.fetch_add(1, Ordering::SeqCst); Ok(json!({"signing_seed_hex":format!("{index:02x}").repeat(32)})) })
            }).unwrap()
        })
    }).collect();
    let values: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(values[0], values[1]); assert_eq!(created.load(Ordering::SeqCst), 1);
    assert_eq!(read_f6_or_create(root.path(), StoreScope::Machine, || panic!("no legacy fallback"), || panic!("existing key needs no writer")).unwrap(), values[0]);
    let owner = LockedSelectedGeneration::try_open(root.path()).unwrap();
    assert!(owner.write_section("f6-keystore", json!({"signing_seed_hex":"ff".repeat(32)})).is_err());
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn staged_initial_cutover_resumes_exact_candidate_and_preserves_source() {
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let old = [42; 32]; let input = sections(&old);
    let anchor = fixture_anchor(root.path(), StoreScope::Machine);
    let owner = LockedSelectedGeneration::acquire(root.path()).unwrap();
    let candidate = prepare(&old, &input).unwrap();
    let stage = root.path().join(".settings-cutover-candidate.v1");
    std::fs::write(&stage, &candidate).unwrap();
    anchor.write(&AnchorState::Preparing { next: cutover::digest(&candidate) }).unwrap();
    let wrong = [&input[..1], &[("extra".into(), "changed".into())]].concat();
    assert!(stage_and_select(&owner._ownership, &anchor, root.path(), || panic!("must use staged bytes"), &|bytes| verify_source_for(StoreScope::Machine, bytes, &old, &wrong)).is_err());
    assert_eq!(std::fs::read(&stage).unwrap(), candidate);
    assert!(!root.path().join(".settings-generation.v1").exists());
    stage_and_select(&owner._ownership, &anchor, root.path(), || panic!("must use staged bytes"), &|bytes| verify_source_for(StoreScope::Machine, bytes, &old, &input)).unwrap();
    assert_eq!(std::fs::read(&stage).unwrap(), candidate);
    assert_eq!(owner.read_all().unwrap()["payments"]["token"], "second secret");
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn opted_in_runtime_never_provisions_a_missing_anchor_or_uses_legacy() {
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let old_key = root.path().join(".install.material");
    std::fs::write(&old_key, b"preserve existing material").unwrap();
    assert!(read_with_legacy(root.path(), StoreScope::Machine, "settings", || panic!("must not use legacy")).is_err());
    assert!(write_with_legacy(root.path(), StoreScope::Machine, "settings", &json!({}), || panic!("must not use legacy")).is_err());
    assert!(!root.path().join(".settings-cutover-anchor.v1").exists());
    assert_eq!(std::fs::read(old_key).unwrap(), b"preserve existing material");
}

#[test]
fn recovery_reports_busy_separately_from_unavailable_ownership() {
    let busy = OwnerAcquireError::from_io(std::io::Error::from(std::io::ErrorKind::WouldBlock));
    assert_eq!(busy, OwnerAcquireError::Busy);
    assert!(busy.message().contains("busy"));
    assert!(!busy.message().contains("administrator"));
    let denied = OwnerAcquireError::from_io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert_eq!(denied, OwnerAcquireError::Unavailable);
    assert!(denied.message().contains("authorized"));
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn fresh_install_selects_encrypted_empty_generation_and_resumes_pending() {
    let root = tempfile::tempdir().unwrap();
    provision_test_lock(root.path());
    let anchor = fixture_anchor(root.path(), StoreScope::Machine);
    let owner = LockedSelectedGeneration::acquire(root.path()).unwrap();
    let candidate = prepare_fresh_generation(StoreScope::Machine).unwrap();
    std::fs::write(root.path().join(".settings-cutover-candidate.v1"), &candidate).unwrap();
    anchor.write(&AnchorState::Preparing { next: cutover::digest(&candidate) }).unwrap();
    assert!(owner.read_all().is_err());
    select_fresh_generation(&owner._ownership, root.path(), StoreScope::Machine, &|| Ok(())).unwrap();
    assert!(owner.read_all().unwrap().is_empty());
    let seed = json!({"signing_seed_hex": "c".repeat(64)});
    assert_eq!(owner.get_or_create_f6(|| Ok(seed.clone())).unwrap(), seed);
    assert_eq!(owner.get_or_create_f6(|| panic!("existing key must be returned")).unwrap(), seed);
    assert_eq!(std::fs::read(root.path().join(".settings-cutover-candidate.v1")).unwrap(), candidate);
    assert!(!root.path().join(".install.material").exists());
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn first_existing_migration_checks_maintenance_and_preserves_f6_source() {
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let old = [42; 32]; let mut input = sections(&old); let old_key = derive_section_key(&old, None).unwrap();
    let f6 = json!({"signing_seed_hex": "d".repeat(64)});
    let encrypted_f6 = encode_section(&old_key, &serde_json::to_vec(&f6).unwrap(), "machine:f6-keystore").unwrap();
    input.push(("f6-keystore".into(), encrypted_f6.clone()));
    let source_path = root.path().join("f6-original.dat");
    std::fs::write(&source_path, encrypted_f6.as_bytes()).unwrap();
    let candidate = prepare(&old, &input).unwrap();
    let owner = LockedSelectedGeneration::acquire(root.path()).unwrap();
    let anchor = ProtectedFileAnchor::new(root.path(), StoreScope::Machine);
    assert!(cutover::provision_first_migration(&owner._ownership, &anchor, root.path(), &|| Err("writer running".into())).is_err());
    assert!(anchor.read().unwrap().is_none());
    cutover::provision_first_migration(&owner._ownership, &anchor, root.path(), &|| verify_source(&candidate, &old, &input)).unwrap();
    stage_and_select(&owner._ownership, &anchor, root.path(), || Ok(candidate.clone()), &|bytes| verify_source(bytes, &old, &input)).unwrap();
    assert_eq!(owner.read_all().unwrap()["f6-keystore"], f6);
    assert_eq!(std::fs::read_to_string(source_path).unwrap(), encrypted_f6);
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn first_migration_authenticates_preexisting_staged_candidate_before_anchor() {
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let old = [42; 32]; let input = sections(&old);
    let owner = LockedSelectedGeneration::acquire(root.path()).unwrap();
    let anchor = ProtectedFileAnchor::new(root.path(), StoreScope::Machine);
    let candidate = prepare(&old, &input).unwrap();
    let stage = root.path().join(".settings-cutover-candidate.v1");
    std::fs::write(&stage, &candidate).unwrap();
    let wrong = [&input[..1], &[("changed".into(), "stale".into())]].concat();
    assert!(cutover::provision_first_migration_with_stage(&owner._ownership, &anchor, root.path(), &|bytes| verify_source(bytes.unwrap(), &old, &wrong)).is_err());
    assert!(anchor.read().unwrap().is_none());
    assert_eq!(std::fs::read(&stage).unwrap(), candidate);
    cutover::provision_first_migration_with_stage(&owner._ownership, &anchor, root.path(), &|bytes| verify_source(bytes.unwrap(), &old, &input)).unwrap();
    stage_and_select(&owner._ownership, &anchor, root.path(), || panic!("reuse exact staged bytes"), &|bytes| verify_source(bytes, &old, &input)).unwrap();
    assert_eq!(std::fs::read(stage).unwrap(), candidate);
    assert_eq!(owner.read_all().unwrap()["payments"]["token"], "second secret");
}
