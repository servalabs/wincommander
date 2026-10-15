// SPDX-License-Identifier: AGPL-3.0-or-later
#[test]
fn group_error_reason_never_exposes_the_group_identity() {
    assert_eq!(
        group_error_reason(VaultError::PrincipalResolution(
            "sensitive-group-name".into()
        )),
        "windows local group operation failed"
    );
}
#[test]
fn transient_edit_failure_recovers_verified_prior_policy_without_restart_or_replay() {
    let (store, controls, files, edit) = fixture();
    let prior = files
        .lock()
        .unwrap()
        .get(&PathBuf::from("/policy").join(POLICY_FILE))
        .unwrap()
        .clone();
    controls.fail_apply.store(true, Ordering::SeqCst);
    assert_eq!(store.apply(edit.clone(), 8), Err(VaultError::AclApply));
    assert_eq!(controls.applies.load(Ordering::SeqCst), 2); // no replay
    assert_eq!(controls.restores.load(Ordering::SeqCst), 1);
    assert_eq!(
        *files
            .lock()
            .unwrap()
            .get(&PathBuf::from("/policy").join(POLICY_FILE))
            .unwrap(),
        prior
    );
    assert_eq!(
        store.status().validation_state,
        VaultValidationState::Current
    );
    assert_eq!(store.caller_projection(OWNER).version, 1);
    assert_eq!(store.caller_projection(OWNER).entries.len(), 1);
    assert!(store.ensure_policy_read_available(false).is_ok());
    assert!(store.authorize_mount("shared", &[OWNER.into()]).allowed);
    assert!(
        !store
            .authorize_mount("shared", &["S-1-5-21-1-2-3-1002".into()])
            .allowed
    );
    store.apply(edit, 9).unwrap();
    assert_eq!(store.caller_projection(OWNER).version, 2);
}
#[test]
fn failed_acl_or_group_restore_or_readback_keeps_policy_unavailable() {
    for mode in 0..4 {
        let (store, controls, _, edit) = fixture();
        controls.fail_apply.store(true, Ordering::SeqCst);
        match mode {
            0 => controls.fail_restore.store(true, Ordering::SeqCst),
            1 => controls.fail_group_restore.store(true, Ordering::SeqCst),
            2 => controls.fail_restore_readback.store(true, Ordering::SeqCst),
            _ => controls
                .fail_prior_acl_readback
                .store(true, Ordering::SeqCst),
        }
        let expected = if mode < 2 {
            VaultError::RollbackUnverified
        } else {
            VaultError::AclApply
        };
        assert_eq!(store.apply(edit, 8), Err(expected));
        assert_blocked(&store);
    }
}
#[test]
fn ambiguous_persistence_that_replaced_policy_cannot_restore_current() {
    let (store, controls, files, edit) = fixture();
    controls.ambiguous_write.store(true, Ordering::SeqCst);
    assert_eq!(
        store.apply(edit.clone(), 8),
        Err(VaultError::CommitUnconfirmed)
    );
    assert_blocked(&store);
    let saved: PersistedPolicy = serde_json::from_slice(
        files
            .lock()
            .unwrap()
            .get(&PathBuf::from("/policy").join(POLICY_FILE))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved.policy.version, 2);
    let calls = controls.applies.load(Ordering::SeqCst);
    assert_eq!(store.apply(edit, 9), Err(VaultError::Persistence));
    assert_eq!(controls.applies.load(Ordering::SeqCst), calls);
}
#[test]
fn changed_protected_directory_blocks_rollback_recovery() {
    let (store, controls, _, edit) = fixture();
    controls.fail_apply.store(true, Ordering::SeqCst);
    controls.drift_directory.store(true, Ordering::SeqCst);
    assert_eq!(store.apply(edit, 8), Err(VaultError::AclApply));
    assert_blocked(&store);
}
#[test]
fn failed_removal_recovers_prior_policy_then_explicit_retry_can_remove() {
    let (store, controls, _, mut removal) = fixture();
    removal.entries.clear();
    controls.fail_remove.store(true, Ordering::SeqCst);
    assert_eq!(store.clear(removal.clone()), Err(VaultError::Persistence));
    assert_eq!(
        store.status().validation_state,
        VaultValidationState::Current
    );
    assert_eq!(store.caller_projection(OWNER).version, 1);
    store.clear(removal).unwrap();
    assert!(store.caller_projection(OWNER).entries.is_empty());
    assert!(store.ensure_policy_read_available(false).is_ok());
}
#[test]
fn refresh_recovers_after_transient_verification_failure_without_replaying_restoration() {
    for mode in 0..3 {
        let (store, controls, _, edit) = fixture();
        controls.fail_apply.store(true, Ordering::SeqCst);
        match mode {
            0 => controls.fail_restore_readback.store(true, Ordering::SeqCst),
            1 => controls
                .fail_prior_acl_readback
                .store(true, Ordering::SeqCst),
            _ => controls.outage_after_restore.store(true, Ordering::SeqCst),
        }
        assert_eq!(store.apply(edit.clone(), 8), Err(VaultError::AclApply));
        assert!(store.ensure_policy_read_available(false).is_err());
        assert_eq!(store.policy().unwrap().version, 1);
        controls
            .fail_restore_readback
            .store(false, Ordering::SeqCst);
        controls
            .fail_prior_acl_readback
            .store(false, Ordering::SeqCst);
        controls.reads_unavailable.store(false, Ordering::SeqCst);
        assert!(store.ensure_policy_read_available(false).is_ok());
        assert_eq!(store.caller_projection(OWNER).version, 1);
        assert_eq!(controls.applies.load(Ordering::SeqCst), 2);
        assert_eq!(controls.restores.load(Ordering::SeqCst), 1);
        assert!(
            !store
                .authorize_mount("shared", &["S-1-5-21-1-2-3-1002".into()])
                .allowed
        );
        store.apply(edit, 9).unwrap();
    }
}
#[test]
fn failed_restore_is_not_later_accepted_from_dependency_health_alone() {
    let (store, controls, _, edit) = fixture();
    controls.fail_apply.store(true, Ordering::SeqCst);
    controls.fail_restore.store(true, Ordering::SeqCst);
    assert_eq!(store.apply(edit, 8), Err(VaultError::RollbackUnverified));
    controls.fail_restore.store(false, Ordering::SeqCst);
    assert_blocked(&store);
    assert_eq!(controls.restores.load(Ordering::SeqCst), 1);
}
#[test]
fn first_save_failed_edit_recovers_only_verified_absence_and_can_retry_without_restart() {
    let (store, controls, files, first) = fixture_with_initial_policy(false);
    controls.fail_apply.store(true, Ordering::SeqCst);
    controls.outage_after_restore.store(true, Ordering::SeqCst);
    assert_eq!(store.apply(first.clone(), 7), Err(VaultError::AclApply));
    assert!(store.ensure_policy_read_available(false).is_err());
    assert!(store.ensure_policy_read_available(true).is_err());
    assert!(!files
        .lock()
        .unwrap()
        .contains_key(&PathBuf::from("/policy").join(POLICY_FILE)));
    controls.reads_unavailable.store(false, Ordering::SeqCst);
    assert!(store.ensure_policy_read_available(false).is_ok());
    assert_eq!(
        store.status().validation_state,
        VaultValidationState::NeverApplied
    );
    assert!(store.caller_projection(OWNER).entries.is_empty());
    assert_eq!(controls.applies.load(Ordering::SeqCst), 1);
    assert_eq!(controls.restores.load(Ordering::SeqCst), 1);
    assert!(!store.authorize_mount("shared", &[OWNER.into()]).allowed);
    store.apply(first, 8).unwrap();
    assert_eq!(store.caller_projection(OWNER).version, 1);
    assert!(
        !store
            .authorize_mount("shared", &["S-1-5-21-1-2-3-1002".into()])
            .allowed
    );
}
#[test]
fn appeared_policy_or_replaced_target_cannot_be_treated_as_verified_prior_absence() {
    for replaced_target in [false, true] {
        let (store, controls, _, first) = fixture_with_initial_policy(false);
        if replaced_target {
            controls.fail_apply.store(true, Ordering::SeqCst);
            controls
                .replace_target_after_apply
                .store(true, Ordering::SeqCst);
            assert_eq!(store.apply(first, 7), Err(VaultError::RollbackUnverified));
            assert_eq!(controls.restores.load(Ordering::SeqCst), 0);
        } else {
            controls.ambiguous_write.store(true, Ordering::SeqCst);
            assert_eq!(store.apply(first, 7), Err(VaultError::CommitUnconfirmed));
        }
        assert_eq!(
            store.status().validation_state,
            VaultValidationState::Degraded
        );
        assert!(store.ensure_policy_read_available(false).is_err());
        assert!(store.ensure_policy_read_available(true).is_err());
        assert!(!store.authorize_mount("shared", &[OWNER.into()]).allowed);
    }
}
#[test]
fn degraded_legacy_repair_without_durable_checkpoint_still_refuses_replacement_restore() {
    let (store, controls, _, edit) = fixture();
    {
        let mut state = store.state.lock().unwrap();
        state.status.validation_state = VaultValidationState::Degraded;
        // Models the migrated cache of an explicit legacy repair: its
        // old durable bytes cannot be used as automatic-recovery proof.
        state.active.as_mut().unwrap().resolved[0]
            .authorization_grants
            .push(ResolvedGrantRecord {
                sid: "S-1-5-21-1-2-3-1002".into(),
                access: VaultAccess::Read,
            });
    }
    controls.fail_apply.store(true, Ordering::SeqCst);
    controls
        .replace_target_after_apply
        .store(true, Ordering::SeqCst);
    assert_eq!(store.apply(edit, 8), Err(VaultError::RollbackUnverified));
    assert_eq!(controls.restores.load(Ordering::SeqCst), 0);
    assert_blocked(&store);
}
