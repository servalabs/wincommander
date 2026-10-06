// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
fn expired_fallback_never_uses_a_reused_legacy_session_namespace() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    let mut mount = expired_private_mount();
    mount.authentication_id = None;
    assert!(broker.retain_cleanup_mount(&store, "legacy", mount));
    broker.owner_logon_probe = |_| Ok(true);
    let expired = broker.expired_session_cleanup_targets(7);
    assert_eq!(expired.mounts.len(), 1);
    broker.owner_logon_probe = |_| Ok(false);
    let saved = store.read_active_mounts().unwrap();
    broker.dismount_session_targets(&store, &expired);
    assert_eq!(store.read_active_mounts().unwrap(), saved);
    assert!(events.lock().unwrap().recovered.is_empty());
    assert!(events.lock().unwrap().dismounted.is_empty());
}

#[test]
fn ended_logon_cleanup_keeps_its_proof_when_a_later_probe_would_fail() {
    static PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    PROBES.store(0, Ordering::SeqCst);
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| if PROBES.fetch_add(1, Ordering::SeqCst) == 0 { Ok(true) } else { Err(()) };
    let mut mount = expired_private_mount();
    mount.syncthing_managed = true;
    assert!(broker.retain_cleanup_mount(&store, "old", mount));
    let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-admin", true);
    assert_eq!(result.state, VaultMountState::Failed);
    assert_eq!(PROBES.load(Ordering::SeqCst), 1);
    let events = events.lock().unwrap();
    assert_eq!(events.recovered, vec![12]);
    assert_eq!(events.cleanup_contexts[0].1, None);
    let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
    assert!(saved.mounts["old"].driver_slot_absent && saved.mounts["old"].syncthing_managed);
}

#[test]
fn administrator_recovers_only_ended_standalone_private_mounts() {
    for personal in [false, true] {
        for elevated in [false, true] {
            for ended in [false, true] {
                let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
                let events = Arc::new(Mutex::new(BrokerEvents::default()));
                let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
                broker.owner_logon_probe = if ended { |_| Ok(true) } else { |_| Ok(false) };
                let mut mount = expired_private_mount();
                mount.personal = personal;
                assert!(broker.retain_cleanup_mount(&store, "old", mount));
                let allowed = personal && elevated && ended;
                let rows = broker.personal_mounts_for_caller(&store, std::ptr::null_mut(), 8, "S-1-5-21-admin", elevated).unwrap();
                assert_eq!(rows.len(), usize::from(allowed));
                if allowed { assert!(rows[0].dismount_allowed && !rows[0].browse_allowed); }
                let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-admin", elevated);
                assert_eq!(result.state == VaultMountState::Unmounted, allowed);
                let events = events.lock().unwrap();
                assert_eq!(events.recovered.len(), usize::from(allowed));
                assert!(events.dismounted.is_empty());
                if allowed { assert_eq!(events.cleanup_contexts[0].1, None); }
            }
        }
    }
}

#[test]
fn administrator_cleanup_never_adopts_a_different_live_generation() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "different-live-generation".into())])));
    assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
    let saved = store.read_active_mounts().unwrap();
    let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-admin", true);
    assert_eq!(result.reason, Some(VaultMountReason::MountStateUnknown));
    assert_eq!(store.read_active_mounts().unwrap(), saved);
    assert!(events.lock().unwrap().recovered.is_empty());
    assert!(events.lock().unwrap().dismounted.is_empty());
}

#[test]
fn administrator_cleanup_preserves_cross_owner_sync_pause_obligation() {
    for absent in [false, true] {
        for unresolved in [false, true] {
            let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
            let events = Arc::new(Mutex::new(BrokerEvents::default()));
            let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
            broker.owner_logon_probe = |_| Ok(true);
            if absent { broker.engine_snapshot = Some(|| Ok(HashMap::new())); }
            let mut mount = expired_private_mount();
            mount.syncthing_managed = !unresolved;
            mount.syncthing_resume_unresolved = unresolved;
            assert!(broker.retain_cleanup_mount(&store, "old", mount));
            let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-admin", true);
            assert_eq!(result.state, VaultMountState::Failed);
            let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
            let pending = &saved.mounts["old"];
            assert!(pending.cleanup_required && pending.driver_slot_absent);
            assert_eq!(pending.syncthing_managed, !unresolved);
            assert_eq!(pending.syncthing_resume_unresolved, unresolved);
            broker.reconcile_absent_mounts_locked(&store).unwrap();
            assert!(broker.active.lock().unwrap()["old"].cleanup_required);
        }
    }
}

#[test]
fn administrator_can_retire_confirmed_absent_unbound_standalone_record() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
    let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-admin", true);
    assert_eq!(result.state, VaultMountState::Unmounted);
    assert!(broker.recovery_allows_entry("old", &store));
    assert!(events.lock().unwrap().recovered.is_empty());
}
