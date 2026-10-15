// SPDX-License-Identifier: AGPL-3.0-or-later
mod rollback_recovery {
    use super::*;

    const OWNER: &str = "S-1-5-21-1-2-3-1001";
    #[derive(Default)]
    struct Controls {
        fail_apply: AtomicBool,
        fail_restore: AtomicBool,
        fail_restore_readback: AtomicBool,
        fail_prior_acl_readback: AtomicBool,
        fail_group_restore: AtomicBool,
        ambiguous_write: AtomicBool,
        fail_remove: AtomicBool,
        drift_directory: AtomicBool,
        outage_after_restore: AtomicBool,
        reads_unavailable: AtomicBool,
        replace_target_after_apply: AtomicBool,
        target_replaced: AtomicBool,
        applies: AtomicUsize,
        restores: AtomicUsize,
    }
    struct RecoveryFs {
        files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        controls: Arc<Controls>,
    }
    impl VaultFs for RecoveryFs {
        fn read(&self, p: &Path) -> std::io::Result<Vec<u8>> {
            if self.controls.reads_unavailable.load(Ordering::SeqCst) {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                Fs(Arc::clone(&self.files)).read(p)
            }
        }
        fn atomic_write(&self, p: &Path, b: &[u8]) -> std::io::Result<()> {
            Fs(Arc::clone(&self.files)).atomic_write(p, b)?;
            if self.controls.ambiguous_write.swap(false, Ordering::SeqCst) {
                Err(std::io::Error::from(std::io::ErrorKind::Other))
            } else {
                Ok(())
            }
        }
        fn remove(&self, p: &Path) -> std::io::Result<()> {
            if self.controls.fail_remove.swap(false, Ordering::SeqCst) {
                Err(std::io::Error::from(std::io::ErrorKind::Other))
            } else {
                Fs(Arc::clone(&self.files)).remove(p)
            }
        }
        fn stable_file_identity(&self, p: &Path) -> Result<String, VaultError> {
            if self.controls.target_replaced.load(Ordering::SeqCst) {
                return Ok("replaced-target-identity".into());
            }
            Fs(Arc::clone(&self.files)).stable_file_identity(p)
        }
        fn normalize_personal_creation_path(&self, p: &Path) -> Result<PathBuf, VaultError> {
            lexical_normalize_creation_path(p)
        }
        fn personal_creation_target_exists(&self, p: &Path) -> Result<bool, VaultError> {
            Ok(self.files.lock().unwrap().contains_key(p))
        }
        fn validate_dedicated_parent(&self, _: &Path, _: &Path) -> Result<(), VaultError> {
            Ok(())
        }
    }
    struct SidResolver;
    impl PrincipalResolver for SidResolver {
        fn resolve_sid(&self, name: &str) -> Result<String, VaultError> {
            Ok(name.into())
        }
    }
    struct RecoveryAcl {
        controls: Arc<Controls>,
        files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    }
    impl AclApplier for RecoveryAcl {
        fn apply_and_verify(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
            self.controls.applies.fetch_add(1, Ordering::SeqCst);
            if self
                .controls
                .replace_target_after_apply
                .load(Ordering::SeqCst)
            {
                self.controls.target_replaced.store(true, Ordering::SeqCst);
            }
            if self.controls.drift_directory.swap(false, Ordering::SeqCst) {
                self.files.lock().unwrap().insert(
                    PathBuf::from("/policy").join(ACCESS_DIRECTORY_FILE),
                    b"{}".to_vec(),
                );
            }
            if self.controls.fail_apply.swap(false, Ordering::SeqCst) {
                Err(VaultError::AclApply)
            } else {
                Ok(())
            }
        }
        fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
            Ok(vec![AclSnapshot {
                path: plan.container.clone(),
                descriptor: b"prior-dacl".to_vec(),
                dacl_protected: true,
            }])
        }
        fn restore(&self, snapshots: &[AclSnapshot]) -> Result<(), VaultError> {
            assert!(!snapshots.is_empty());
            self.controls.restores.fetch_add(1, Ordering::SeqCst);
            if self.controls.outage_after_restore.load(Ordering::SeqCst) {
                self.controls
                    .reads_unavailable
                    .store(true, Ordering::SeqCst);
            }
            if self.controls.fail_restore.load(Ordering::SeqCst) {
                Err(VaultError::AclApply)
            } else {
                Ok(())
            }
        }
        fn verify_restored(&self, _: &[AclSnapshot]) -> Result<(), VaultError> {
            if self.controls.fail_restore_readback.load(Ordering::SeqCst) {
                Err(VaultError::AclReadback)
            } else {
                Ok(())
            }
        }
        fn verify_exact(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
            if self.controls.fail_prior_acl_readback.load(Ordering::SeqCst) {
                Err(VaultError::AclReadback)
            } else {
                Ok(())
            }
        }
        fn decommission_and_snapshot(
            &self,
            plan: &VaultAclPlan,
            _: &str,
        ) -> Result<Vec<AclSnapshot>, VaultError> {
            self.snapshot(plan)
        }
    }
    struct RecoveryGroups {
        inner: Groups,
        controls: Arc<Controls>,
    }
    impl LocalGroupReconciler for RecoveryGroups {
        fn reconcile_exact_members(
            &self,
            group: &str,
            members: &[String],
        ) -> Result<(), VaultError> {
            self.inner.reconcile_exact_members(group, members)
        }
        fn snapshot(
            &self,
            plans: &[GroupMembershipPlan],
        ) -> Result<Vec<GroupMembershipSnapshot>, VaultError> {
            self.inner.snapshot(plans)
        }
        fn delete_service_owned_group(&self, group: &str) -> Result<(), VaultError> {
            self.inner.delete_service_owned_group(group)
        }
        fn restore(&self, snapshots: &[GroupMembershipSnapshot]) -> Result<(), VaultError> {
            if self.controls.fail_group_restore.load(Ordering::SeqCst) {
                Err(VaultError::Persistence)
            } else {
                self.inner.restore(snapshots)
            }
        }
    }
    fn fixture_with_initial_policy(
        installed: bool,
    ) -> (
        VaultAccessStore,
        Arc<Controls>,
        Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        VaultAccessPolicy,
    ) {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let controls = Arc::new(Controls::default());
        let store = VaultAccessStore::open_with_groups(
            Box::new(RecoveryFs {
                files: Arc::clone(&files),
                controls: Arc::clone(&controls),
            }),
            Box::new(SidResolver),
            Box::new(RecoveryAcl {
                files: Arc::clone(&files),
                controls: Arc::clone(&controls),
            }),
            Box::new(RecoveryGroups {
                inner: Groups::default(),
                controls: Arc::clone(&controls),
            }),
            PathBuf::from("/policy"),
        );
        let mut first = policy(1, 0);
        first.entries[0].primary_owner_sid = Some(OWNER.into());
        first.entries[0].owner_account = OWNER.into();
        first.entries[0].grants = vec![wincmd_shared::vault_access::VaultGrantInput {
            principal_name: OWNER.into(),
            access: VaultAccess::Write,
        }];
        first.entries[0].mount.presentation = VaultPresentation::PerUser;
        if !installed {
            return (store, controls, files, first);
        }
        store.apply(first.clone(), 7).unwrap();
        let mut edit = first;
        edit.version = 2;
        edit.expected_previous_version = 1;
        edit.entries[0].mount.preferred_letter = Some("W".into());
        (store, controls, files, edit)
    }
    fn fixture() -> (
        VaultAccessStore,
        Arc<Controls>,
        Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        VaultAccessPolicy,
    ) {
        fixture_with_initial_policy(true)
    }
    fn assert_blocked(store: &VaultAccessStore) {
        assert_eq!(
            store.status().validation_state,
            VaultValidationState::Degraded
        );
        assert!(store.ensure_policy_read_available(false).is_err());
        assert!(store.ensure_policy_read_available(true).is_ok());
        assert!(!store.authorize_mount("shared", &[OWNER.into()]).allowed);
        assert_eq!(store.administrator_projection().version, 1);
    }
    include!("rollback_state_tests.rs");
    include!("rollback_windows_tests.rs");
}
