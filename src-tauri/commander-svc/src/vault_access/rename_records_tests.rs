// SPDX-License-Identifier: AGPL-3.0-or-later
// The hidden-part note and the records a file rename moves.

/// Lets `writes_left` saves succeed, fails the next one, then succeeds again.
struct CountdownFs {
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    writes_left: Arc<AtomicUsize>,
}
impl VaultFs for CountdownFs {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if self.writes_left.fetch_sub(1, Ordering::SeqCst) == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::Other));
        }
        self.files.lock().unwrap().insert(path.into(), bytes.into());
        Ok(())
    }
    fn stable_file_identity(&self, _: &Path) -> Result<String, VaultError> {
        Ok("v:1:i:2".into())
    }
    fn normalize_personal_creation_path(&self, path: &Path) -> Result<PathBuf, VaultError> {
        lexical_normalize_creation_path(path)
    }
    fn personal_creation_target_exists(&self, path: &Path) -> Result<bool, VaultError> {
        Ok(self.files.lock().unwrap().contains_key(path))
    }
    fn validate_dedicated_parent(&self, _: &Path, _: &Path) -> Result<(), VaultError> {
        Ok(())
    }
}

const NOTED_PATH: &str = r"D:\Vault\private folder\notes.hc";

fn notes_path() -> PathBuf {
    PathBuf::from("/policy").join("vault-hidden-parts-v1.json")
}

fn noted_target() -> PasswordChangeTarget {
    unregistered(NOTED_PATH, "v:1:i:2")
}

fn countdown_store(files: &Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>) -> (VaultAccessStore, Arc<AtomicUsize>) {
    let writes_left = Arc::new(AtomicUsize::new(usize::MAX));
    let store = VaultAccessStore::open(
        Box::new(CountdownFs { files: Arc::clone(files), writes_left: Arc::clone(&writes_left) }),
        Box::new(Resolver),
        Box::new(RecordingAcl::default()),
        PathBuf::from("/policy"),
    );
    store.load_at_startup();
    (store, writes_left)
}

#[test]
fn a_noted_vault_stays_noted_after_a_restart_and_noting_it_again_writes_nothing() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let (store, writes_left) = countdown_store(&files);
    assert_eq!(store.has_hidden_part("v:1:i:2"), Ok(false));
    store.note_hidden_part(&noted_target()).unwrap();
    writes_left.store(0, Ordering::SeqCst);
    store.note_hidden_part(&noted_target()).unwrap();
    assert_eq!(store.has_hidden_part("v:1:i:2"), Ok(true));
    assert_eq!(store.has_hidden_part("v:1:i:3"), Ok(false));

    let (restarted, _) = countdown_store(&files);
    assert_eq!(restarted.has_hidden_part("v:1:i:2"), Ok(true));
}

#[test]
fn an_unreadable_note_file_fails_closed_and_is_never_overwritten() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    files.lock().unwrap().insert(notes_path(), b"not json".to_vec());
    let (store, _) = countdown_store(&files);
    assert_eq!(store.has_hidden_part("v:1:i:2"), Err(VaultError::Persistence));
    assert_eq!(store.note_hidden_part(&noted_target()), Err(VaultError::Persistence));
    assert_eq!(store.move_records_for_rename(&noted_target(), "x.hc"), Err(VaultError::Persistence));
    assert_eq!(files.lock().unwrap().get(&notes_path()).map(Vec::as_slice), Some(&b"not json"[..]));
}

#[test]
fn a_failed_note_save_keeps_the_previous_set() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let (store, writes_left) = countdown_store(&files);
    writes_left.store(0, Ordering::SeqCst);
    assert_eq!(store.note_hidden_part(&noted_target()), Err(VaultError::Persistence));
    assert_eq!(store.has_hidden_part("v:1:i:2"), Ok(false));
    assert!(!files.lock().unwrap().contains_key(&notes_path()));
}

#[test]
fn the_oldest_note_makes_room_at_the_bound() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let full: Vec<serde_json::Value> = (0..hidden_note::MAX_HIDDEN_PART_NOTES)
        .map(|index| serde_json::json!({ "container_identity": format!("v:7:i:{index}"), "container_path": format!(r"D:\v{index}") }))
        .collect();
    files
        .lock()
        .unwrap()
        .insert(notes_path(), serde_json::to_vec(&serde_json::json!({ "notes": full })).unwrap());
    let (store, _) = countdown_store(&files);
    store.note_hidden_part(&noted_target()).unwrap();
    assert_eq!(store.has_hidden_part("v:7:i:0"), Ok(false));
    assert_eq!(store.has_hidden_part("v:7:i:1"), Ok(true));
    assert_eq!(store.has_hidden_part("v:1:i:2"), Ok(true));
}

#[test]
fn nothing_the_store_returns_to_a_caller_names_a_noted_vault() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let (store, _) = countdown_store(&files);
    store.apply(policy(1, 0), 7).unwrap();
    store.note_hidden_part(&noted_target()).unwrap();
    let returned = serde_json::json!({
        "status": store.status(),
        "caller_status": store.caller_status(NAMES_OWNER, true),
        "policy": store.policy(),
        "names": store
            .display_names_for_caller(NAMES_OWNER, |_| true)
            .into_iter()
            .map(|row| (row.path, row.name))
            .collect::<Vec<_>>(),
    })
    .to_string();
    assert!(!returned.contains("private folder"), "{returned}");
}

#[test]
fn the_note_and_the_personal_record_follow_a_renamed_file_across_a_restart() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let mut record = named_personal_record(NAMES_OWNER);
    record.container_path = NOTED_PATH.into();
    let registry = PersonalVaultRegistry { records: vec![record], pending: vec![], legacy_recoveries: vec![] };
    files.lock().unwrap().insert(registry_path(), serde_json::to_vec(&registry).unwrap());
    let (store, _) = countdown_store(&files);
    store.note_hidden_part(&noted_target()).unwrap();

    store.move_records_for_rename(&noted_target(), "Taxes 2026.hc").unwrap();
    let renamed = r"D:\Vault\private folder\Taxes 2026.hc";
    for store in [store, countdown_store(&files).0] {
        assert_eq!(store.personal_record_for_path(NOTED_PATH).unwrap(), None);
        let moved = store.personal_record_for_path(renamed).unwrap().expect("the record moved");
        assert_eq!((moved.container_identity.as_str(), moved.owner_sid.as_str()), ("v:1:i:2", NAMES_OWNER));
        assert_eq!(store.has_hidden_part("v:1:i:2"), Ok(true));
        assert_eq!(store.password_change_target(renamed, NAMES_OWNER, 7).unwrap().container_path, renamed);
    }
    let notes: serde_json::Value = serde_json::from_slice(files.lock().unwrap().get(&notes_path()).unwrap()).unwrap();
    assert_eq!(notes["notes"][0]["container_path"], renamed);
}

#[test]
fn a_failed_save_restores_every_file_already_written_and_keeps_memory() {
    let mut record = named_personal_record(NAMES_OWNER);
    record.container_path = NOTED_PATH.into();
    let registry = serde_json::to_vec(&PersonalVaultRegistry { records: vec![record], pending: vec![], legacy_recoveries: vec![] }).unwrap();
    let names = serde_json::json!({ "names": [
        { "owner_sid": "S-1-5-21-other", "container_identity": "v:1:i:2", "name": "Old", "container_path": NOTED_PATH },
    ] });
    let notes = serde_json::json!({ "notes": [ { "container_identity": "v:1:i:2", "container_path": NOTED_PATH } ] });
    // Registry, then names, then the note: fail the second save and the third.
    for successful_writes in [1, 2] {
        let files = Arc::new(Mutex::new(HashMap::from([
            (registry_path(), registry.clone()),
            (names_path(), serde_json::to_vec(&names).unwrap()),
            (notes_path(), serde_json::to_vec(&notes).unwrap()),
        ])));
        let before = files.lock().unwrap().clone();
        let (store, writes_left) = countdown_store(&files);
        writes_left.store(successful_writes, Ordering::SeqCst);
        assert_eq!(store.move_records_for_rename(&noted_target(), "moved.hc"), Err(VaultError::Persistence));
        writes_left.store(usize::MAX, Ordering::SeqCst);
        assert_eq!(*files.lock().unwrap(), before, "after {successful_writes} saves");
        assert!(store.personal_record_for_path(NOTED_PATH).unwrap().is_some());
        assert_eq!(store.display_names_for_caller("S-1-5-21-other", |_| false)[0].path, NOTED_PATH);
    }
}

#[test]
fn a_policy_entry_moves_with_its_file_without_reapplying_an_acl() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let acl = RecordingAcl::default();
    let applied = Arc::clone(&acl.grants);
    let store = rename_store(Arc::clone(&files), Arc::new(AtomicBool::new(false)), acl);
    store.apply(policy(1, 0), 7).unwrap();
    let applied_before = applied.lock().unwrap().len();
    let before = store.policy().unwrap();
    let mut shared = unregistered(r"C:\vaults\shared.hc", "v:1:i:3");
    shared.entry_id = Some("shared".into());
    assert_eq!(store.move_records_for_rename(&shared, "SyncTest 2.hc"), Err(VaultError::ContainerIdentity));
    assert_eq!(store.policy().unwrap(), before);

    shared.container_identity = "v:1:i:2".into();
    store.move_records_for_rename(&shared, "SyncTest 2.hc").unwrap();
    let after = store.policy().unwrap();
    let mut unchanged = after.entries[0].clone();
    assert_eq!(unchanged.container_path, r"C:\vaults\SyncTest 2.hc");
    unchanged.container_path = before.entries[0].container_path.clone();
    assert_eq!(unchanged, before.entries[0], "only the path may change");
    assert_eq!((after.version, after.expected_previous_version), (2, 1));
    assert_eq!(store.status().version, 2);
    assert_eq!(applied.lock().unwrap().len(), applied_before, "a rename re-applied an ACL");
    let resolved = store.password_change_target(r"C:\vaults\SyncTest 2.hc", "S-1-5-21-other", 7).unwrap();
    assert_eq!(resolved.entry_id.as_deref(), Some("shared"));

    let reloaded = rename_store(files, Arc::new(AtomicBool::new(false)), RecordingAcl::default());
    reloaded.load_at_startup();
    assert_eq!(reloaded.policy().unwrap().entries[0].container_path, r"C:\vaults\SyncTest 2.hc");
    assert_eq!(reloaded.status().validation_state, VaultValidationState::Current);
}
