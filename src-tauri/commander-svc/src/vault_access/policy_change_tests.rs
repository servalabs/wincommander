// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
fn status_projection_does_not_reveal_other_owners_entry_ids() {
    struct SidResolver;
    impl PrincipalResolver for SidResolver {
        fn resolve_sid(&self, name: &str) -> Result<String, VaultError> { Ok(name.into()) }
    }
    let store = VaultAccessStore::open(
        Box::new(Fs(Arc::new(Mutex::new(HashMap::new())))),
        Box::new(SidResolver), Box::new(Acl), PathBuf::from("/policy"),
    );
    let owner = "S-1-5-21-1-2-3-1001";
    let other = "S-1-5-21-1-2-3-1002";
    let mut requested = policy(1, 0);
    requested.entries[0].primary_owner_sid = Some(owner.into());
    requested.entries[0].owner_account = owner.into();
    requested.entries[0].grants = vec![wincmd_shared::vault_access::VaultGrantInput {
        principal_name: owner.into(), access: VaultAccess::Write,
    }];
    requested.entries[0].mount.presentation = VaultPresentation::PerUser;
    let mut foreign = requested.entries[0].clone();
    foreign.id = "other-private".into();
    foreign.container_path = r"C:\other\private.hc".into();
    foreign.primary_owner_sid = Some(other.into());
    foreign.owner_account = other.into();
    foreign.grants[0].principal_name = other.into();
    foreign.mount.preferred_letter = Some("W".into());
    requested.entries.push(foreign);
    store.apply(requested, 7).unwrap();
    let status = store.caller_status(owner, false);
    assert_eq!(status.entries.len(), 1);
    assert_eq!(status.entries[0].id, "shared");
    assert_eq!(status.version, 1);
    assert_eq!(store.caller_status(other, false).entries[0].id, "other-private");
    assert!(store.caller_status("S-1-5-21-1-2-3-1003", false).entries.is_empty());
    assert_eq!(store.caller_status(owner, true).entries.len(), 2);
    assert_eq!(store.caller_status("", false), empty_status());
    assert_eq!(store.caller_status("invalid", true), empty_status());
    store.state.lock().unwrap().status.validation_state = VaultValidationState::Degraded;
    assert_eq!(store.caller_status(owner, false), empty_status());
    assert_eq!(store.caller_status(owner, true).entries.len(), 2);
}

#[test]
fn outsider_remove_cannot_atomically_recreate_the_same_file_under_a_new_policy_id() {
    let store = store(Arc::new(Mutex::new(HashMap::new())));
    store.apply(policy(1, 0), 1).unwrap();
    let mut replacement = policy(2, 1);
    replacement.entries[0].id = "new-owner-entry".into();
    replacement.entries[0].container_path = r"c:\VAULTS\shared.hc".into();
    replacement.entries[0].container_identity = Some("renderer-forged-identity".into());
    let protected = HashSet::from(["shared".to_owned()]);
    assert_eq!(
        store.reject_removed_entry_reidentification(&replacement, &protected),
        Err(VaultError::Forbidden)
    );
    replacement.entries[0].container_path = r"C:\another\distinct.hc".into();
    assert!(store
        .reject_removed_entry_reidentification(&replacement, &protected)
        .is_ok());
    replacement.entries.clear();
    assert!(store
        .reject_removed_entry_reidentification(&replacement, &protected)
        .is_ok());
    assert!(store
        .reject_removed_entry_reidentification(&policy(2, 1), &HashSet::new())
        .is_ok());
}

#[test]
fn editing_one_policy_does_not_reapply_an_unchanged_container_acl() {
    struct Paths(Arc<Mutex<Vec<PathBuf>>>);
    impl AclApplier for Paths {
        fn apply_and_verify(&self, plan: &VaultAclPlan) -> Result<(), VaultError> {
            self.0.lock().unwrap().push(plan.container.clone());
            Ok(())
        }
        fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
            Acl.snapshot(plan)
        }
        fn restore(&self, _: &[AclSnapshot]) -> Result<(), VaultError> {
            Ok(())
        }
    }
    let paths = Arc::new(Mutex::new(Vec::new()));
    let store = VaultAccessStore::open(
        Box::new(Fs(Arc::new(Mutex::new(HashMap::new())))),
        Box::new(Resolver),
        Box::new(Paths(paths.clone())),
        PathBuf::from("/policy"),
    );
    let mut first = policy(1, 0);
    let mut second = first.entries[0].clone();
    second.id = "other".into();
    second.container_path = r"C:\other\shared.hc".into();
    second.mount.preferred_letter = Some("W".into());
    first.entries.push(second);
    store.apply(first, 1).unwrap();
    paths.lock().unwrap().clear();
    let mut update = store.policy().unwrap();
    update.version = 2;
    update.expected_previous_version = 1;
    update.entries[0].label = "Only this label changed".into();
    let (ids, identities) = store.policy_change_targets(&update).unwrap();
    assert_eq!(ids, HashSet::from(["shared".to_owned()]));
    assert_eq!(
        identities,
        HashSet::from([r"volume:1:c:\vaults\shared.hc".to_owned()])
    );
    store.apply(update, 2).unwrap();
    assert_eq!(
        *paths.lock().unwrap(),
        vec![PathBuf::from(r"C:\vaults\shared.hc")]
    );
    paths.lock().unwrap().clear();
    let mut unchanged = store.policy().unwrap();
    unchanged.version = 3;
    unchanged.expected_previous_version = 2;
    assert_eq!(
        store.policy_change_targets(&unchanged).unwrap(),
        (HashSet::new(), HashSet::new())
    );
    store.apply(unchanged, 3).unwrap();
    assert!(paths.lock().unwrap().is_empty());
}

#[test]
fn changed_target_identity_is_resolved_not_trusted_from_renderer() {
    let store = store(Arc::new(Mutex::new(HashMap::new())));
    store.apply(policy(1, 0), 1).unwrap();
    let mut update = policy(2, 1);
    update.entries[0].container_identity = Some("forged-unmounted-identity".into());
    assert_eq!(
        store.policy_change_targets(&update),
        Err(VaultError::ContainerIdentity)
    );
}

#[test]
fn changed_resolved_principal_is_included_in_mount_gate_even_when_entry_is_unchanged() {
    struct ChangingPrincipal(Arc<AtomicBool>);
    impl PrincipalResolver for ChangingPrincipal {
        fn resolve_sid(&self, name: &str) -> Result<String, VaultError> {
            Ok(format!("S-1-test-{name}-{}", self.0.load(Ordering::SeqCst)))
        }
    }
    let changed = Arc::new(AtomicBool::new(false));
    let store = VaultAccessStore::open(
        Box::new(Fs(Arc::new(Mutex::new(HashMap::new())))),
        Box::new(ChangingPrincipal(changed.clone())),
        Box::new(Acl),
        PathBuf::from("/policy"),
    );
    store.apply(policy(1, 0), 1).unwrap();
    let unchanged = store.policy().unwrap();
    assert!(store
        .policy_change_targets(&unchanged)
        .unwrap()
        .0
        .is_empty());
    changed.store(true, Ordering::SeqCst);
    assert_eq!(
        store.policy_change_targets(&unchanged).unwrap().0,
        HashSet::from(["shared".to_owned()])
    );
}

#[test]
fn recovery_target_lookup_does_not_resolve_missing_unrelated_principals() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let original = store(files.clone());
    let mut first = policy(1, 0);
    let mut second = first.entries[0].clone();
    second.id = "other".into();
    second.container_path = r"C:\other\shared.hc".into();
    second.mount.preferred_letter = Some("W".into());
    first.entries.push(second);
    original.apply(first, 1).unwrap();
    let restarted = VaultAccessStore::open(
        Box::new(Fs(files)),
        Box::new(RejectingResolver),
        Box::new(Acl),
        PathBuf::from("/policy"),
    );
    restarted.load_at_startup();
    let (ids, identities) = restarted.policy_removal_targets("shared").unwrap();
    assert_eq!(ids, HashSet::from(["shared".to_owned()]));
    assert_eq!(
        identities,
        HashSet::from([r"volume:1:c:\vaults\shared.hc".to_owned()])
    );
    assert_eq!(
        restarted.policy_removal_targets("unknown"),
        Err(VaultError::Validation)
    );
}
