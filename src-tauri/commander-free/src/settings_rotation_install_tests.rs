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
fn fresh_machine_stage_requires_confirmation_without_touching_source() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("missing-root");
    assert!(stage_root_exists(&root, StoreScope::Machine).unwrap_err().starts_with("SETTINGS_IDENTITY_UNVERIFIED:"));
    assert!(!root.exists());
    std::fs::create_dir(&root).unwrap();
    assert!(stage_keyless_store(&root, StoreScope::Machine).unwrap_err().starts_with("SETTINGS_IDENTITY_UNVERIFIED:"));
    let store = root.join("store"); std::fs::create_dir(&store).unwrap();
    assert!(stage_keyless_store(&root, StoreScope::Machine).unwrap_err().starts_with("SETTINGS_IDENTITY_UNVERIFIED:"));
    let existing = store.join("settings.dat"); std::fs::write(&existing, b"existing data").unwrap();
    let error = stage_keyless_store(&root, StoreScope::Machine).unwrap_err();
    assert!(!error.starts_with("SETTINGS_IDENTITY_UNVERIFIED:"));
    assert_eq!(std::fs::read(existing).unwrap(), b"existing data");
}

#[test]
fn unconfirmed_missing_identity_does_not_leave_a_stale_candidate() {
    let root = tempfile::tempdir().unwrap();
    let stage = root.path().join(".settings-cutover-candidate.v1");
    let old = [42; 32]; let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    let error = publish_stage_after_identity_check(&stage, &candidate).unwrap_err();
    assert!(error.starts_with("SETTINGS_IDENTITY_UNVERIFIED:"));
    assert!(!stage.exists());
    let key = derive_section_key(&old, None).unwrap();
    let mut changed = input.clone();
    changed[0].1 = encode_section(&key, &serde_json::to_vec(&json!({"changed": true})).unwrap(), "machine:settings").unwrap();
    let later = prepare(&old, &changed).unwrap();
    assert!(publish_stage_after_identity_check(&stage, &later).unwrap_err().starts_with("SETTINGS_IDENTITY_UNVERIFIED:"));
    assert!(!stage.exists());
    let f6 = json!({"signing_seed_hex": "a".repeat(64)});
    let mut existing = input;
    existing.push(("f6-keystore".into(), encode_section(&key, &serde_json::to_vec(&f6).unwrap(), "machine:f6-keystore").unwrap()));
    let with_identity = prepare(&old, &existing).unwrap();
    assert_eq!(publish_stage_after_identity_check(&stage, &with_identity).unwrap(), with_identity);
    assert_eq!(std::fs::read(stage).unwrap(), with_identity);
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn staging_retry_recovers_only_authenticated_interrupted_updates() {
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let anchor = fixture_anchor(root.path(), StoreScope::Machine);
    let owner = LockedSelectedGeneration::acquire(root.path()).unwrap();
    assert!(!reconcile_stage_owner(&owner).unwrap());
    select_fresh_generation(&owner._ownership, root.path(), StoreScope::Machine, &|| Ok(())).unwrap();
    let old = owner.read_reconciled().unwrap();
    let next = updated_generation(&old, "settings", json!({"fixture": "updated"})).unwrap();
    let pending = AnchorState::Updating { prior: cutover::digest(&old), next: cutover::digest(&next) };

    anchor.write(&pending).unwrap();
    assert!(reconcile_stage_owner(&owner).unwrap());
    assert_eq!(anchor.read().unwrap(), Some(AnchorState::Active { digest: cutover::digest(&old) }));
    assert_eq!(owner.read_reconciled().unwrap(), old);

    anchor.write(&pending).unwrap();
    std::fs::write(&owner.path, &next).unwrap();
    assert!(reconcile_stage_owner(&owner).unwrap());
    assert_eq!(anchor.read().unwrap(), Some(AnchorState::Active { digest: cutover::digest(&next) }));
    assert_eq!(owner.read_reconciled().unwrap(), next);

    anchor.write(&pending).unwrap();
    std::fs::write(&owner.path, b"unrelated or corrupt").unwrap();
    assert!(reconcile_stage_owner(&owner).is_err());
    assert_eq!(anchor.read().unwrap(), Some(pending));
    assert_eq!(std::fs::read(&owner.path).unwrap(), b"unrelated or corrupt");
    let matching_corrupt = AnchorState::Updating {
        prior: cutover::digest(&next), next: cutover::digest(b"unrelated or corrupt"),
    };
    anchor.write(&matching_corrupt).unwrap();
    assert!(reconcile_stage_owner(&owner).is_err());
    assert_eq!(anchor.read().unwrap(), Some(matching_corrupt));
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
    let view = inspect_generation_files(&owner._ownership, &owner.path, &owner.authority, StoreScope::Machine).unwrap();
    assert!(require_selected_startup(view, StoreScope::Machine, |_| true).is_err());
    let seed = json!({"signing_seed_hex": "c".repeat(64)});
    assert!(provision_machine_f6_owned(&owner, || panic!("new identity requires confirmation")).is_err());
    owner.create_new_f6_confirmed(|| Ok(seed.clone())).unwrap();
    provision_machine_f6_owned(&owner, || panic!("existing key must be retained")).unwrap();
    let defaults = json!({"deviceId": "fresh-device", "policy": {}});
    provision_fresh_machine_settings_owned(&owner, || Ok(defaults.clone())).unwrap();
    provision_fresh_machine_settings_owned(&owner, || panic!("defaults must not be replaced on retry")).unwrap();
    assert_eq!(owner.read_all().unwrap()["settings"], defaults);
    let view = inspect_generation_files(&owner._ownership, &owner.path, &owner.authority, StoreScope::Machine).unwrap();
    assert!(require_selected_startup(view, StoreScope::Machine, |_| true).is_ok());
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
    provision_machine_f6_owned(&owner, || panic!("existing F6 identity must not be regenerated")).unwrap();
    let before = owner.read_all().unwrap();
    provision_fresh_machine_settings_owned(&owner, || panic!("migrated settings must not be initialized as fresh")).unwrap();
    assert_eq!(owner.read_all().unwrap(), before);
    assert_eq!(owner.read_all().unwrap()["f6-keystore"], f6);
    assert_eq!(std::fs::read_to_string(source_path).unwrap(), encrypted_f6);
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn migrated_generation_without_settings_is_not_treated_as_fresh() {
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let old = [42; 32];
    let input: Vec<_> = sections(&old).into_iter().filter(|(name, _)| name != "settings").collect();
    let candidate = prepare(&old, &input).unwrap();
    let owner = LockedSelectedGeneration::acquire(root.path()).unwrap();
    let anchor = ProtectedFileAnchor::new(root.path(), StoreScope::Machine);
    cutover::provision_first_migration(&owner._ownership, &anchor, root.path(), &|| verify_source(&candidate, &old, &input)).unwrap();
    stage_and_select(&owner._ownership, &anchor, root.path(), || Ok(candidate.clone()), &|bytes| verify_source(bytes, &old, &input)).unwrap();
    let before = owner.read_all().unwrap();
    provision_fresh_machine_settings_owned(&owner, || panic!("migrated source must never receive fresh defaults")).unwrap();
    assert_eq!(owner.read_all().unwrap(), before);
}

#[test]
#[cfg_attr(windows, ignore = "requires elevated machine-lock ACL provisioning; run explicitly in Windows acceptance")]
fn migration_without_machine_f6_does_not_generate_an_unverified_identity() {
    let root = tempfile::tempdir().unwrap(); provision_test_lock(root.path());
    let old = [42; 32]; let input = sections(&old);
    let candidate = prepare(&old, &input).unwrap();
    assert!(verify_f6_creation_authority(&open_generation(&candidate).unwrap()).is_err());
    let owner = LockedSelectedGeneration::acquire(root.path()).unwrap();
    let anchor = ProtectedFileAnchor::new(root.path(), StoreScope::Machine);
    cutover::provision_first_migration(&owner._ownership, &anchor, root.path(), &|| verify_source(&candidate, &old, &input)).unwrap();
    stage_and_select(&owner._ownership, &anchor, root.path(), || Ok(candidate.clone()), &|bytes| verify_source(bytes, &old, &input)).unwrap();
    let before = owner.read_reconciled().unwrap();
    let error = provision_machine_f6_owned(&owner, || panic!("unverified identity must not be generated")).unwrap_err();
    assert!(error.contains("identity could not be verified"));
    assert!(owner.write_section("f6-keystore", json!({"signing_seed_hex": "c".repeat(64)})).is_err());
    assert_eq!(owner.read_reconciled().unwrap(), before);
    #[cfg(windows)] {
        let runtime = RuntimeWriteGuard { owner, _root_pins: Vec::new(), selected: true };
        assert!(runtime.get_or_create_f6(|| panic!("runtime must not generate an unverified identity")).is_err());
        assert!(runtime.write("f6-keystore", &json!({"signing_seed_hex": "c".repeat(64)})).is_err());
        assert_eq!(runtime.owner.read_reconciled().unwrap(), before);
        let new_identity = json!({"signing_seed_hex": "e".repeat(64)});
        assert_eq!(runtime.owner.create_new_f6_confirmed(|| Ok(new_identity.clone())).unwrap(), new_identity);
        assert!(runtime.owner.create_new_f6_confirmed(|| panic!("must never replace an existing identity")).is_err());
        assert_eq!(runtime.owner.read_all().unwrap()["f6-keystore"], new_identity);
    }
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

#[test]
#[cfg(windows)]
fn duplicated_search_fence_is_locked_by_installer_process() {
    use std::{io::Read, os::windows::{fs::OpenOptionsExt, io::{FromRawHandle, RawHandle}}, process::{Command, Stdio}};
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    const DIR: &str = "WINCMD_TEST_SEARCH_FENCE_DIR";
    const PID: &str = "WINCMD_TEST_SEARCH_FENCE_PID";
    if let (Some(dir), Ok(pid)) = (std::env::var_os(DIR), std::env::var(PID)) {
        duplicate_search_fence_handles(std::path::Path::new(&dir), pid.parse().unwrap(), Vec::new()).unwrap();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let search_dir = dir.path().join("fresh-search-parent");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "datastore::settings_rotation_trial::tests::duplicated_search_fence_is_locked_by_installer_process", "--nocapture"])
        .env(DIR, &search_dir).env(PID, std::process::id().to_string()).stdout(Stdio::piped()).spawn().unwrap();
    let mut output = String::new(); helper.stdout.take().unwrap().read_to_string(&mut output).unwrap();
    assert!(helper.wait().unwrap().success());
    let marker = "WINCOMMANDER_SEARCH_FENCE_HANDLE_V1:";
    let digits: String = output.split_once(marker).unwrap().1.chars().take_while(|c| c.is_ascii_digit()).collect();
    let handle: usize = digits.parse().unwrap();
    // SAFETY: the helper duplicated this handle into the current process and
    // transfers its ownership to this test exactly once.
    let fence = unsafe { std::fs::File::from_raw_handle(handle as RawHandle) };
    fence.try_lock().unwrap();
    let reader = std::fs::OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(search_dir.join(".encrypted-search-maintenance.lock")).unwrap();
    assert!(reader.try_lock_shared().is_err());
    drop(fence);
    reader.try_lock_shared().unwrap();
}

#[test]
fn installer_startup_never_falls_back_to_legacy_or_incomplete_generation() {
    let machine = StoreScope::Machine;
    let valid = |value: &Value| value.get("deviceId").and_then(Value::as_str) == Some("valid-fixture");
    assert!(require_selected_startup(StartupView::Legacy, machine, valid).is_err());
    assert!(require_selected_startup(StartupView::RecoveryRequired, machine, valid).is_err());
    let selected = |sections, fresh| StartupView::Selected { sections, fresh };
    assert!(require_selected_startup(selected(BTreeMap::new(), true), machine, valid).is_err());
    assert!(require_selected_startup(selected(BTreeMap::new(), false), machine, valid).is_ok());
    let mut sections = BTreeMap::from([("settings".into(), json!({"deviceId": "valid-fixture"}))]);
    assert!(require_selected_startup(selected(sections.clone(), true), machine, valid).is_err());
    assert!(require_selected_startup(selected(sections.clone(), false), machine, valid).is_ok());
    sections.insert("F6-KEYSTORE".into(), json!({"signing_seed_hex": "c".repeat(64)}));
    assert!(require_selected_startup(selected(sections.clone(), true), machine, valid).is_ok());
    sections.insert("settings".into(), json!({"deviceId": "invalid"}));
    assert!(require_selected_startup(selected(sections.clone(), true), machine, valid).is_err());
    sections.insert("settings".into(), json!({}));
    assert!(require_selected_startup(selected(sections, true), machine, valid).is_err());
    assert!(require_selected_startup(selected(BTreeMap::new(), true), StoreScope::CurrentUser, valid).is_ok());
}
