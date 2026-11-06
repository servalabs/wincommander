    use super::*;
    use std::cell::RefCell;
    struct TestAnchor(RefCell<Option<AnchorState>>);
    impl Anchor for TestAnchor {
        fn read(&self) -> Result<Option<AnchorState>, String> { Ok(self.0.borrow().clone()) }
        fn write(&self, value: &AnchorState) -> Result<(), String> { *self.0.borrow_mut() = Some(value.clone()); Ok(()) }
    }
    fn fixture() -> (tempfile::TempDir, super::super::settings_generation_ownership::SettingsGenerationLock) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".settings-generation.lock"), b"").unwrap();
        let guard = super::super::settings_generation_ownership::SettingsGenerationLock::try_acquire(root.path()).unwrap();
        (root, guard)
    }
    fn verify(bytes: &[u8]) -> Result<(), String> { if bytes.starts_with(b"valid:") { Ok(()) } else { Err("invalid generation".into()) } }
    #[test]
    fn initial_cutover_keeps_legacy_and_recovers_only_verified_candidate() {
        let (root, guard) = fixture();
        let legacy = root.path().join("f6-keystore.dat");
        std::fs::write(&legacy, b"original encrypted signing key").unwrap();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Legacy)));
        select_initial(&guard, &anchor, &file, b"valid:signing-key", &verify).unwrap();
        assert_eq!(read_active(&guard, &anchor, &file, &verify).unwrap(), b"valid:signing-key");
        assert_eq!(std::fs::read(&legacy).unwrap(), b"original encrypted signing key");
        std::fs::remove_file(&file.path).unwrap();
        assert!(read_active(&guard, &anchor, &file, &verify).is_err());
        *anchor.0.borrow_mut() = None;
        assert!(select_initial(&guard, &anchor, &file, b"valid:other", &verify).is_err());
    }
    #[test]
    fn pending_initial_requires_fresh_source_and_exact_candidate() {
        let (root, guard) = fixture();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Preparing { next: digest(b"valid:original") })));
        assert!(read_active(&guard, &anchor, &file, &verify).is_err());
        assert!(select_initial(&guard, &anchor, &file, b"valid:other", &verify).is_err());
        assert!(select_initial(&guard, &anchor, &file, b"valid:original", &|_| Err("source changed".into())).is_err());
        assert!(file.read().unwrap().is_none());
        select_initial(&guard, &anchor, &file, b"valid:original", &verify).unwrap();
    }
    #[test]
    fn interrupted_update_only_recovers_matching_valid_old_or_new() {
        let (root, guard) = fixture();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Legacy)));
        select_initial(&guard, &anchor, &file, b"valid:old", &verify).unwrap();
        let state = AnchorState::Updating { prior: digest(b"valid:old"), next: digest(b"valid:new") };
        *anchor.0.borrow_mut() = Some(state.clone());
        recover_update(&guard, &anchor, &file, &verify).unwrap();
        assert_eq!(read_active(&guard, &anchor, &file, &verify).unwrap(), b"valid:old");
        *anchor.0.borrow_mut() = Some(state.clone());
        file.replace(b"valid:new").unwrap();
        recover_update(&guard, &anchor, &file, &verify).unwrap();
        assert_eq!(read_active(&guard, &anchor, &file, &verify).unwrap(), b"valid:new");
        *anchor.0.borrow_mut() = Some(state);
        file.replace(b"valid:other").unwrap();
        assert!(recover_update(&guard, &anchor, &file, &verify).is_err());
        assert!(read_active(&guard, &anchor, &file, &verify).is_err());
    }
    #[test]
    fn file_anchor_survives_reopen_and_missing_or_corrupt_fails_closed() {
        let (root, guard) = fixture();
        let scope = super::super::settings_rotation_trial::StoreScope::Machine;
        let anchor = ProtectedFileAnchor::new(root.path(), scope);
        let file = SelectedFile::new(root.path());
        assert!(select_initial(&guard, &anchor, &file, b"valid:key", &verify).is_err());
        anchor.provision_legacy().unwrap();
        assert!(anchor.provision_legacy().is_err());
        select_initial(&guard, &anchor, &file, b"valid:key", &verify).unwrap();
        let reopened = ProtectedFileAnchor::new(root.path(), scope);
        assert_eq!(read_active(&guard, &reopened, &file, &verify).unwrap(), b"valid:key");
        std::fs::write(&anchor.path, b"corrupt").unwrap();
        assert!(read_active(&guard, &reopened, &file, &verify).is_err());
        assert!(reopened.provision_legacy().is_err());
        std::fs::remove_file(&anchor.path).unwrap();
        assert!(read_active(&guard, &reopened, &file, &verify).is_err());
        assert!(reopened.write(&AnchorState::Legacy).is_err());
    }

    #[test]
    fn shared_guard_cannot_start_or_repair_cutover() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".settings-generation.lock"), b"").unwrap();
        let shared = super::super::settings_generation_ownership::SettingsGenerationLock::try_acquire_shared(root.path()).unwrap();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Legacy)));
        assert!(select_initial(&shared, &anchor, &file, b"valid:one", &verify).is_err());
        assert!(file.read().unwrap().is_none());
        assert_eq!(anchor.read().unwrap(), Some(AnchorState::Legacy));
    }

    #[test]
    fn first_migration_preserves_existing_key_and_rejects_ambiguous_artifacts() {
        let (root, guard) = fixture();
        let key = root.path().join("f6-keystore.dat");
        std::fs::write(&key, b"existing key").unwrap();
        let anchor = ProtectedFileAnchor::new(root.path(), super::super::settings_rotation_trial::StoreScope::Machine);
        assert!(provision_first_migration(&guard, &anchor, root.path(), &|| Err("writer running".into())).is_err());
        assert!(anchor.read().unwrap().is_none());
        let stage = root.path().join(".settings-cutover-candidate.v1");
        std::fs::write(&stage, b"ambiguous prior state").unwrap();
        assert!(provision_first_migration(&guard, &anchor, root.path(), &|| Ok(())).is_err());
        assert_eq!(std::fs::read(&stage).unwrap(), b"ambiguous prior state");
        std::fs::remove_file(stage).unwrap();
        provision_first_migration(&guard, &anchor, root.path(), &|| Ok(())).unwrap();
        assert_eq!(anchor.read().unwrap(), Some(AnchorState::Legacy));
        assert_eq!(std::fs::read(key).unwrap(), b"existing key");
    }

