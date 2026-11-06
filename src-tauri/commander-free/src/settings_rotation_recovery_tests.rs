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

