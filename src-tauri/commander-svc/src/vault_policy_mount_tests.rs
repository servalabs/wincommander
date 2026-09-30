// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
fn policy_mount_guard_allows_changes_to_a_different_unmounted_container() {
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    let live = active_mount_for_owner(7, "S-1-5-21-owner");
    let live_identity = live.container_identity.clone();
    broker.active.lock().unwrap().insert("mounted".into(), live);
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(
            &HashSet::from(["different".into()]),
            &HashSet::from(["different-file".into()])
        ),
        Ok(())
    );
    // Changing the ID or path spelling cannot evade the exact file identity.
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(
            &HashSet::from(["different".into()]),
            &HashSet::from([live_identity])
        ),
        Err(VaultMountReason::AlreadyMounted)
    );
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(
            &HashSet::from(["mounted".into()]),
            &HashSet::new()
        ),
        Err(VaultMountReason::AlreadyMounted)
    );
    assert!(events.lock().unwrap().dismounted.is_empty());
    assert!(events.lock().unwrap().recovered.is_empty());
}

#[test]
fn policy_mount_guard_requires_trusted_complete_driver_observation() {
    let changed = HashSet::from(["new-entry".into()]);
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    // A driver mount with no service-owned identity cannot be guessed unrelated.
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(&changed, &HashSet::new()),
        Err(VaultMountReason::MountStateUnknown)
    );
    let mut live = active_mount_for_owner(7, "S-1-5-21-owner");
    live.engine_mount_identity = Some("replaced-slot".into());
    broker.active.lock().unwrap().insert("other".into(), live);
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(&changed, &HashSet::new()),
        Err(VaultMountReason::MountStateUnknown)
    );
    broker.engine_snapshot = Some(|| Err("observer unavailable".into()));
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(&changed, &HashSet::new()),
        Err(VaultMountReason::MountStateUnknown)
    );
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(&changed, &HashSet::new()),
        Ok(())
    );
    broker.mark_registry_untrusted();
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(&changed, &HashSet::new()),
        Err(VaultMountReason::MountStateUnknown)
    );
}

#[test]
fn policy_mount_guard_rejects_unpersisted_cleanup_even_when_driver_is_empty() {
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(
        BrokerEvents::default(),
    )))));
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    let changed = HashSet::from(["new-entry".into()]);
    broker.mark_persistence_pending("old-entry");
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(&changed, &HashSet::new()),
        Err(VaultMountReason::MountStateUnknown)
    );
    broker.recovery.lock().unwrap().persistence_pending.clear();
    broker.mark_removal_pending("old-entry");
    assert_eq!(
        broker.reject_policy_changes_while_mounted_locked(&changed, &HashSet::new()),
        Err(VaultMountReason::MountStateUnknown)
    );
}

#[test]
fn authorized_cross_session_slot_dismount_returns_the_exact_confirmed_receipt() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.policy_authorizer = |_, _, _| wincmd_shared::vault_access::VaultAuthorizeMountResponse {
        allowed: true,
        launch_ready: true,
        denial_reason: None,
        mode: Some(VaultAccess::Read),
        presentation: Some(VaultPresentation::Machine),
        preferred_letter: None,
    };
    broker.active.lock().unwrap().insert(
        "managed".into(),
        active_mount_for_owner(7, "S-1-5-21-owner"),
    );
    let result = broker.dismount_personal_for_caller(
        &store,
        1,
        12,
        std::ptr::null_mut(),
        8,
        "S-1-5-21-member",
        true,
    );
    let wire = serde_json::to_value(result).unwrap();
    assert!(wincmd_shared::vault_inventory::confirmed_slot_dismount(wire, 12).is_ok());
    assert!(broker.snapshot().unwrap().is_empty());
    assert_eq!(events.lock().unwrap().recovered, [12]);
    assert!(broker.active.lock().unwrap().is_empty());
}

#[test]
fn authorized_cross_session_dismount_does_not_trust_process_success_without_closed_slot() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.policy_authorizer = |_, _, _| wincmd_shared::vault_access::VaultAuthorizeMountResponse {
        allowed: true,
        launch_ready: true,
        denial_reason: None,
        mode: Some(VaultAccess::Read),
        presentation: Some(VaultPresentation::Machine),
        preferred_letter: None,
    };
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "test-mount:12".into())])));
    broker.active.lock().unwrap().insert(
        "managed".into(),
        active_mount_for_owner(7, "S-1-5-21-owner"),
    );
    let result = broker.dismount_personal_for_caller(
        &store,
        1,
        12,
        std::ptr::null_mut(),
        8,
        "S-1-5-21-member",
        true,
    );
    assert_eq!(result.state, VaultMountState::Failed);
    assert_eq!(result.reason, Some(VaultMountReason::DismountFailed));
    assert!(wincmd_shared::vault_inventory::confirmed_slot_dismount(
        serde_json::to_value(result).unwrap(),
        12
    )
    .is_err());
    assert_eq!(events.lock().unwrap().recovered, [12]);
    assert!(broker.active.lock().unwrap().contains_key("managed"));
}
