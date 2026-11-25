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
        display_name: None,
    }
}

#[test]
fn personal_rename_failure_to_save_keeps_the_previous_name_in_memory_and_on_disk() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let store = rename_store(Arc::clone(&files), Arc::clone(&fail), RecordingAcl::default());
    let owner = "S-1-5-21-owner";
    store.insert_personal_record_for_test(named_personal_record(owner));
    let target = store.rename_target(r"C:\vaults\notes.hc", owner, 7).unwrap().unwrap();
    store.rename_personal_vault(&target, owner, "Taxes").unwrap();
    let registry_path = PathBuf::from("/policy").join(PERSONAL_VAULTS_FILE);
    let saved = files.lock().unwrap().get(&registry_path).cloned().unwrap();

    fail.store(true, Ordering::SeqCst);
    assert_eq!(
        store.rename_personal_vault(&target, owner, "Receipts"),
        Err(VaultError::Persistence)
    );
    let record = store.personal_record_for_path(r"C:\vaults\notes.hc").unwrap().unwrap();
    let mut expected = named_personal_record(owner);
    expected.display_name = Some("Taxes".into());
    assert_eq!(record, expected, "identity, owner and settings must be untouched");
    assert_eq!(files.lock().unwrap().get(&registry_path), Some(&saved));
}

#[test]
fn personal_rename_needs_a_registered_record_owned_by_the_caller() {
    let store = rename_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
        RecordingAcl::default(),
    );
    let path = r"C:\vaults\notes.hc";
    assert_eq!(store.rename_target(path, "S-1-5-21-owner", 7), Ok(None));
    assert_eq!(
        store.rename_target(r"\Device\Harddisk1\Partition1", "S-1-5-21-owner", 7),
        Ok(None)
    );
    store.insert_personal_record_for_test(named_personal_record("S-1-5-21-owner"));
    assert_eq!(
        store.rename_target(path, "S-1-5-21-other", 7),
        Err(VaultError::Forbidden)
    );
    let target = store.rename_target(path, "S-1-5-21-owner", 7).unwrap().unwrap();
    assert_eq!(
        store.rename_personal_vault(&target, "S-1-5-21-other", "Mine now"),
        Err(VaultError::Forbidden)
    );
    assert_eq!(store.personal_record_for_path(path).unwrap().unwrap().display_name, None);
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
    let target = store.rename_target(r"C:\vaults\notes.hc", "S-1-5-21-owner", 7).unwrap().unwrap();
    store.rename_personal_vault(&target, "S-1-5-21-owner", "Taxes").unwrap();
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-owner", |_| false),
        [(r"C:\vaults\notes.hc".to_owned(), "Taxes".to_owned())]
    );
    assert!(store.display_names_for_caller("S-1-5-21-other", |_| false).is_empty());
    assert_eq!(
        store.display_names_for_caller("S-1-5-21-other", |id| id == "shared"),
        [(r"C:\vaults\shared.hc".to_owned(), "Shared".to_owned())]
    );
}
