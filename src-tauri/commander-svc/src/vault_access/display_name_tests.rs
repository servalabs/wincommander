// SPDX-License-Identifier: AGPL-3.0-or-later

fn rename_store(
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    fail_next_write: Arc<AtomicBool>,
    acl: RecordingAcl,
) -> VaultAccessStore {
    VaultAccessStore::open(
        Box::new(PersonalFs { files, fail_next_write }),
        Box::new(Resolver),
        Box::new(acl),
        PathBuf::from("/policy"),
    )
}

fn named_personal_record(owner_sid: &str) -> PersonalVaultRecord {
    PersonalVaultRecord {
        container_path: r"C:\vaults\notes.hc".into(),
        container_identity: "v:1:i:2".into(),
        owner_sid: owner_sid.into(),
        scope: VaultPresentation::PerUser,
        created_by_session: 7,
        syncthing_opt_in: Some(false),
    }
}

const NAMES_OWNER: &str = "S-1-5-21-owner";

fn registry_path() -> PathBuf {
    PathBuf::from("/policy").join(PERSONAL_VAULTS_FILE)
}

fn names_path() -> PathBuf {
    PathBuf::from("/policy").join("vault-personal-names-v1.json")
}

/// A store loaded from a saved registry, as after a service restart.
fn store_with_saved_registry(
    files: &Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    fail: &Arc<AtomicBool>,
) -> VaultAccessStore {
    if !files.lock().unwrap().contains_key(&registry_path()) {
        let registry = serde_json::to_vec(&PersonalVaultRegistry {
            records: vec![named_personal_record(NAMES_OWNER)],
            pending: vec![],
            legacy_recoveries: vec![],
        })
        .unwrap();
        files.lock().unwrap().insert(registry_path(), registry);
    }
    let store = rename_store(Arc::clone(files), Arc::clone(fail), RecordingAcl::default());
    store.load_at_startup();
    store
}

fn rename_notes(store: &VaultAccessStore, name: &str) -> Result<(), VaultError> {
    let (target, record) = store.rename_target(r"C:\vaults\notes.hc", NAMES_OWNER, 7)?;
    assert_eq!(record, RenameRecord::Personal);
    store.rename_personal_vault(&target, NAMES_OWNER, name)
}

fn notes_name(store: &VaultAccessStore) -> Option<String> {
    store.personal_display_name_for_test(&named_personal_record(NAMES_OWNER))
}

#[test]
fn personal_rename_writes_only_the_names_file_and_leaves_the_registry_byte_identical() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let store = store_with_saved_registry(&files, &fail);
    let registry = files.lock().unwrap().get(&registry_path()).cloned().unwrap();
    rename_notes(&store, "Taxes").unwrap();
    assert_eq!(files.lock().unwrap().get(&registry_path()), Some(&registry));
    assert_eq!(notes_name(&store).as_deref(), Some("Taxes"));
    assert_eq!(store.personal_record_for_path(r"C:\vaults\notes.hc").unwrap(), Some(named_personal_record(NAMES_OWNER)));

    let restarted = store_with_saved_registry(&files, &fail);
    assert_eq!(notes_name(&restarted).as_deref(), Some("Taxes"));
}

#[test]
fn personal_rename_failure_to_save_keeps_the_previous_name_in_memory_and_on_disk() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let store = store_with_saved_registry(&files, &fail);
    rename_notes(&store, "Taxes").unwrap();
    let saved = files.lock().unwrap().clone();

    fail.store(true, Ordering::SeqCst);
    assert_eq!(rename_notes(&store, "Receipts"), Err(VaultError::Persistence));
    assert_eq!(notes_name(&store).as_deref(), Some("Taxes"));
    assert_eq!(*files.lock().unwrap(), saved, "neither file may change");
    assert_eq!(store.personal_record_for_path(r"C:\vaults\notes.hc").unwrap(), Some(named_personal_record(NAMES_OWNER)));
}

#[test]
fn a_missing_names_file_loads_with_no_names_and_an_unreadable_one_is_never_overwritten() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let store = store_with_saved_registry(&files, &fail);
    assert_eq!(notes_name(&store), None);
    assert!(store.display_names_for_caller(NAMES_OWNER, |_| false).is_empty());

    files.lock().unwrap().insert(names_path(), b"not json".to_vec());
    let store = store_with_saved_registry(&files, &fail);
    assert_eq!(notes_name(&store), None);
    assert_eq!(rename_notes(&store, "Taxes"), Err(VaultError::Persistence));
    assert_eq!(files.lock().unwrap().get(&names_path()).map(Vec::as_slice), Some(&b"not json"[..]));
}

#[test]
fn a_stale_names_entry_is_ignored_and_pruned_on_the_next_rename() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let stale = serde_json::json!({ "names": [
        { "owner_sid": NAMES_OWNER, "container_identity": "replaced-file", "name": "Old" },
    ] });
    files.lock().unwrap().insert(names_path(), serde_json::to_vec(&stale).unwrap());
    let store = store_with_saved_registry(&files, &fail);
    assert_eq!(notes_name(&store), None);
    assert!(store.display_names_for_caller(NAMES_OWNER, |_| false).is_empty());

    rename_notes(&store, "Taxes").unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(files.lock().unwrap().get(&names_path()).unwrap()).unwrap();
    assert_eq!(saved, serde_json::json!({ "names": [
        { "owner_sid": NAMES_OWNER, "container_identity": "v:1:i:2", "name": "Taxes" },
    ] }));
}

#[test]
fn personal_rename_needs_a_registered_record_owned_by_the_caller() {
    let store = rename_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
        RecordingAcl::default(),
    );
    let path = r"C:\vaults\notes.hc";
    for unregistered in [path, r"\Device\Harddisk1\Partition1"] {
        assert_eq!(store.rename_target(unregistered, "S-1-5-21-owner", 7).unwrap().1, RenameRecord::Unregistered);
    }
    store.insert_personal_record_for_test(named_personal_record("S-1-5-21-owner"));
    assert_eq!(
        store.rename_target(path, "S-1-5-21-other", 7),
        Err(VaultError::Forbidden)
    );
    let (target, record) = store.rename_target(path, "S-1-5-21-owner", 7).unwrap();
    assert_eq!(record, RenameRecord::Personal);
    assert_eq!(
        store.rename_personal_vault(&target, "S-1-5-21-other", "Mine now"),
        Err(VaultError::Forbidden)
    );
    assert_eq!(store.personal_display_name_for_test(&named_personal_record("S-1-5-21-owner")), None);
}

#[test]
fn policy_rename_changes_only_the_label_and_reapplies_no_acl() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let acl = RecordingAcl::default();
    let applied = Arc::clone(&acl.grants);
    let store = rename_store(Arc::clone(&files), Arc::clone(&fail), acl);
    store.apply(policy(1, 0), 7).unwrap();
    let applied_before = applied.lock().unwrap().len();
    let before = store.policy().unwrap();

    store.rename_policy_entry("shared", "Finance").unwrap();
    let after = store.policy().unwrap();
    assert_eq!(after.entries[0].label, "Finance");
    let mut unchanged = after.entries[0].clone();
    unchanged.label = before.entries[0].label.clone();
    assert_eq!(unchanged, before.entries[0], "only the label may change");
    assert_eq!((after.version, after.expected_previous_version), (2, 1));
    assert_eq!(store.status().version, 2);
    assert_eq!(applied.lock().unwrap().len(), applied_before, "a rename re-applied an ACL");

    fail.store(true, Ordering::SeqCst);
    assert_eq!(store.rename_policy_entry("shared", "Payroll"), Err(VaultError::Persistence));
    assert_eq!(store.policy().unwrap(), after);
    assert_eq!(store.status().version, 2);
    assert_eq!(store.rename_policy_entry("missing", "Payroll"), Err(VaultError::Validation));

    let reloaded = rename_store(files, Arc::new(AtomicBool::new(false)), RecordingAcl::default());
    reloaded.load_at_startup();
    assert_eq!(reloaded.policy().unwrap().entries[0].label, "Finance");
}

#[test]
fn display_names_list_only_the_callers_named_vaults() {
    let store = rename_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
        RecordingAcl::default(),
    );
    store.apply(policy(1, 0), 7).unwrap();
    store.insert_personal_record_for_test(named_personal_record("S-1-5-21-owner"));
    assert!(store.display_names_for_caller("S-1-5-21-owner", |_| false).is_empty());
    let (target, _) = store.rename_target(r"C:\vaults\notes.hc", "S-1-5-21-owner", 7).unwrap();
    store.rename_personal_vault(&target, "S-1-5-21-owner", "Taxes").unwrap();
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-owner", |_| false),
        [display_row(r"C:\vaults\notes.hc", "Taxes")]
    );
    assert!(store.display_names_for_caller("S-1-5-21-other", |_| false).is_empty());
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-other", |id| id == "shared"),
        [display_row(r"C:\vaults\shared.hc", "Shared")]
    );
}

fn display_row(path: &str, name: &str) -> VaultDisplayName {
    VaultDisplayName { path: path.into(), name: name.into() }
}

fn unregistered(path: &str, identity: &str) -> PasswordChangeTarget {
    PasswordChangeTarget {
        container_path: path.into(),
        container_identity: identity.into(),
        entry_id: None,
        caller_owns_entry: false,
    }
}

#[test]
fn an_unregistered_container_name_is_the_callers_own_and_survives_pruning_and_restart() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let store = store_with_saved_registry(&files, &fail);
    let loose = unregistered(r"D:\Vault\New folder\test3", "v:9:i:3");
    store.rename_unregistered_container(&loose, NAMES_OWNER, "Test three").unwrap();
    store.rename_unregistered_container(&loose, "S-1-5-21-other", "Theirs").unwrap();
    rename_notes(&store, "Taxes").unwrap();
    assert_eq!(
        store.display_names_for_caller(NAMES_OWNER, |_| false),
        [display_row(r"C:\vaults\notes.hc", "Taxes"), display_row(r"D:\Vault\New folder\test3", "Test three")]
    );
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-other", |_| false),
        [display_row(r"D:\Vault\New folder\test3", "Theirs")]
    );
    store.rename_unregistered_container(&loose, NAMES_OWNER, "Renamed").unwrap();
    let restarted = store_with_saved_registry(&files, &fail);
    assert_eq!(
        restarted.display_names_for_caller(NAMES_OWNER, |_| false)[1],
        display_row(r"D:\Vault\New folder\test3", "Renamed")
    );
    assert_eq!(notes_name(&restarted).as_deref(), Some("Taxes"), "an unregistered name is not a record's name");
}

#[test]
fn an_unregistered_name_is_refused_for_a_registered_container_or_a_policy_entry() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let store = store_with_saved_registry(&files, &fail);
    let (registered, _) = store.rename_target(r"C:\vaults\notes.hc", NAMES_OWNER, 7).unwrap();
    assert_eq!(
        store.rename_unregistered_container(&registered, NAMES_OWNER, "Sneaky"),
        Err(VaultError::ContainerIdentity)
    );
    let mut policy_target = unregistered(r"C:\vaults\shared.hc", "v:1:i:9");
    policy_target.entry_id = Some("shared".into());
    assert_eq!(
        store.rename_unregistered_container(&policy_target, NAMES_OWNER, "Sneaky"),
        Err(VaultError::Validation)
    );
    assert_eq!(notes_name(&store), None);
    fail.store(true, Ordering::SeqCst);
    assert_eq!(
        store.rename_unregistered_container(&unregistered(r"D:\x", "v:9:i:4"), NAMES_OWNER, "Lost"),
        Err(VaultError::Persistence)
    );
    assert!(store.display_names_for_caller(NAMES_OWNER, |_| false).is_empty());
}

#[test]
fn a_callers_oldest_unregistered_name_makes_room_for_a_new_one() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let store = store_with_saved_registry(&files, &fail);
    store.rename_unregistered_container(&unregistered(r"D:\other", "v:9:i:0"), "S-1-5-21-other", "Kept").unwrap();
    for index in 0..=display_name::MAX_UNREGISTERED_NAMES_PER_CALLER {
        let target = unregistered(&format!(r"D:\v{index}"), &format!("v:9:i:{}", index + 1));
        store.rename_unregistered_container(&target, NAMES_OWNER, &format!("N{index}")).unwrap();
    }
    let names = store.display_names_for_caller(NAMES_OWNER, |_| false);
    assert_eq!(names.len(), display_name::MAX_UNREGISTERED_NAMES_PER_CALLER);
    assert_eq!(names[0].name, "N1", "the oldest name was dropped first");
    assert_eq!(store.display_names_for_caller("S-1-5-21-other", |_| false).len(), 1);
}

#[test]
fn a_policy_name_comes_before_the_callers_own_name() {
    let store = rename_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
        RecordingAcl::default(),
    );
    store.apply(policy(1, 0), 7).unwrap();
    store
        .rename_unregistered_container(&unregistered(r"C:\vaults\shared.hc", "v:1:i:9"), NAMES_OWNER, "Mine")
        .unwrap();
    assert_eq!(
        store.display_names_for_caller(NAMES_OWNER, |id| id == "shared"),
        [display_row(r"C:\vaults\shared.hc", "Shared"), display_row(r"C:\vaults\shared.hc", "Mine")]
    );
}