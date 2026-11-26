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

/// Names an earlier version saved: the registered Vault's, then two unregistered containers'.
fn saved_names() -> serde_json::Value {
    serde_json::json!({ "names": [
        { "owner_sid": NAMES_OWNER, "container_identity": "v:1:i:2", "name": "Taxes" },
        { "owner_sid": NAMES_OWNER, "container_identity": "v:9:i:3", "name": "Test three",
          "container_path": r"D:\Vault\New folder\test3" },
        { "owner_sid": "S-1-5-21-other", "container_identity": "v:9:i:3", "name": "Theirs",
          "container_path": r"D:\Vault\New folder\test3" },
    ] })
}

fn store_with_saved_names(files: &Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>) -> VaultAccessStore {
    files
        .lock()
        .unwrap()
        .insert(names_path(), serde_json::to_vec(&saved_names()).unwrap());
    store_with_saved_registry(files, &Arc::new(AtomicBool::new(false)))
}

fn notes_name(store: &VaultAccessStore) -> Option<String> {
    store.personal_display_name_for_test(&named_personal_record(NAMES_OWNER))
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
fn names_saved_by_an_earlier_version_load_and_list_for_their_caller_only() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let store = store_with_saved_names(&files);
    assert_eq!(notes_name(&store).as_deref(), Some("Taxes"));
    assert_eq!(
        store.display_names_for_caller(NAMES_OWNER, |_| false),
        [display_row(r"C:\vaults\notes.hc", "Taxes"), display_row(r"D:\Vault\New folder\test3", "Test three")]
    );
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-other", |_| false),
        [display_row(r"D:\Vault\New folder\test3", "Theirs")]
    );
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
    store
        .move_records_for_rename(&unregistered(r"C:\vaults\notes.hc", "v:1:i:2"), "renamed.hc")
        .unwrap();
    assert_eq!(files.lock().unwrap().get(&names_path()).map(Vec::as_slice), Some(&b"not json"[..]));
}

#[test]
fn an_old_unregistered_name_follows_its_file_and_a_registered_name_follows_its_record() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let store = store_with_saved_names(&files);
    store
        .move_records_for_rename(&unregistered(r"D:\Vault\New folder\test3", "v:9:i:3"), "Test 3")
        .unwrap();
    store
        .move_records_for_rename(&unregistered(r"C:\vaults\notes.hc", "v:1:i:2"), "Receipts.hc")
        .unwrap();
    let expected = [
        display_row(r"C:\vaults\Receipts.hc", "Taxes"),
        display_row(r"D:\Vault\New folder\Test 3", "Test three"),
    ];
    assert_eq!(store.display_names_for_caller(NAMES_OWNER, |_| false), expected);
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-other", |_| false),
        [display_row(r"D:\Vault\New folder\Test 3", "Theirs")]
    );
    let restarted = store_with_saved_registry(&files, &Arc::new(AtomicBool::new(false)));
    assert_eq!(restarted.display_names_for_caller(NAMES_OWNER, |_| false), expected);
}

#[test]
fn display_names_list_a_policy_label_only_for_its_owner_or_a_writer() {
    let store = rename_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
        RecordingAcl::default(),
    );
    store.apply(policy(1, 0), 7).unwrap();
    assert!(store.display_names_for_caller("S-1-5-21-other", |_| false).is_empty());
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-other", |id| id == "shared"),
        [display_row(r"C:\vaults\shared.hc", "Shared")]
    );
}

#[test]
fn a_policy_name_comes_before_the_callers_own_name() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let mine = serde_json::json!({ "names": [
        { "owner_sid": NAMES_OWNER, "container_identity": "v:1:i:9", "name": "Mine",
          "container_path": r"C:\vaults\shared.hc" },
    ] });
    files.lock().unwrap().insert(names_path(), serde_json::to_vec(&mine).unwrap());
    let store = rename_store(Arc::clone(&files), Arc::new(AtomicBool::new(false)), RecordingAcl::default());
    store.load_at_startup();
    store.apply(policy(1, 0), 7).unwrap();
    assert_eq!(
        store.display_names_for_caller(NAMES_OWNER, |id| id == "shared"),
        [display_row(r"C:\vaults\shared.hc", "Shared"), display_row(r"C:\vaults\shared.hc", "Mine")]
    );
}
