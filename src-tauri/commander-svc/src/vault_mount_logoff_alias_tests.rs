// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
fn ended_session_inventory_retains_record_while_private_alias_is_present_or_unknown() {
    for alias in [Ok(false), Err(())] {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.owner_logon_probe = |_| Ok(true);
        broker.engine_snapshot = Some(|| Ok(HashMap::new()));
        broker.private_alias_absent = if alias.is_ok() {
            |_| Ok(false)
        } else {
            |_| Err(())
        };
        broker.private_alias_cleanup = |_| panic!("inventory must not remove private aliases");
        assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
        let rows = broker
            .personal_mounts_for_caller(&store, std::ptr::null_mut(), 8, "S-1-5-21-owner", false)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].cleanup_required && rows[0].dismount_allowed && !rows[0].browse_allowed);
        assert!(broker.active.lock().unwrap().contains_key("old"));
        assert!(events.lock().unwrap().recovered.is_empty());
    }
}

#[test]
fn ended_owner_retry_requires_confirmed_alias_cleanup_after_driver_dismount() {
    for alias in [Ok(false), Err(())] {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.owner_logon_probe = |_| Ok(true);
        broker.private_alias_absent = |_| Ok(false);
        broker.private_alias_cleanup = if alias.is_ok() {
            |_| Ok(false)
        } else {
            |_| Err(())
        };
        assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
        let result = broker.dismount_personal_for_caller(
            &store,
            1,
            12,
            std::ptr::null_mut(),
            8,
            "S-1-5-21-owner",
            false,
        );
        assert_eq!(result.reason, Some(VaultMountReason::DismountFailed));
        assert_eq!(events.lock().unwrap().recovered, vec![12]);
        let saved: DurableMountRegistry =
            serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        assert!(saved.mounts["old"].cleanup_required && saved.mounts["old"].driver_slot_absent);
        broker.private_alias_cleanup = |mount| {
            assert_eq!(mount.authentication_id, Some((42, 0)));
            assert_eq!(mount.internal_drive, 12);
            assert_eq!(mount.drive_letter, "V:");
            Ok(true)
        };
        let retried = broker.dismount_personal_for_caller(
            &store,
            2,
            12,
            std::ptr::null_mut(),
            8,
            "S-1-5-21-owner",
            false,
        );
        assert_eq!(retried.state, VaultMountState::Unmounted);
        assert!(broker.active.lock().unwrap().is_empty());
    }
}

#[test]
fn ended_owner_can_clear_absent_driver_alias_without_an_available_pro_broker() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents {
        cleanup_fails: true,
        ..Default::default()
    }));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events)));
    broker.owner_logon_probe = |_| Ok(true);
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    broker.private_alias_absent = |_| Ok(false);
    broker.private_alias_cleanup = |_| Ok(true);
    assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
    let result = broker.dismount_personal_for_caller(
        &store,
        1,
        12,
        std::ptr::null_mut(),
        8,
        "S-1-5-21-owner",
        false,
    );
    assert_eq!(result.state, VaultMountState::Unmounted);
    assert!(broker.active.lock().unwrap().is_empty());
}

#[test]
fn ended_owner_never_cleans_alias_when_driver_slot_is_reused_or_query_fails() {
    for unavailable in [false, true] {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.owner_logon_probe = |_| Ok(true);
        broker.engine_snapshot = if unavailable {
            Some(|| Err("query failed".into()))
        } else {
            Some(|| Ok(HashMap::from([(12, "different-generation".into())])))
        };
        broker.private_alias_cleanup =
            |_| panic!("unknown driver state cannot authorize alias cleanup");
        assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
        let result = broker.dismount_personal_for_caller(
            &store,
            1,
            12,
            std::ptr::null_mut(),
            8,
            "S-1-5-21-owner",
            false,
        );
        assert_eq!(result.reason, Some(VaultMountReason::MountStateUnknown));
        assert!(broker.active.lock().unwrap().contains_key("old"));
        assert!(events.lock().unwrap().recovered.is_empty());
    }
}

#[test]
fn startup_keeps_closed_driver_record_until_ended_owner_alias_is_cleaned() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    broker.private_alias_cleanup = |_| Ok(false);
    assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
    assert!(broker.load_and_cleanup(&store).unwrap().is_empty());
    assert!(broker.active.lock().unwrap()["old"].cleanup_required);
    assert_eq!(events.lock().unwrap().recovered, vec![12]);
}

#[test]
fn private_alias_recovery_requires_recorded_logon_and_one_exact_drive_letter() {
    let mut mount = expired_private_mount();
    for letter in ["", "V:\\other", "VV", "7:", "V:::"] {
        mount.drive_letter = letter.into();
        assert_eq!(recovery::private_alias_absent(&mount), Err(()));
        assert_eq!(recovery::cleanup_private_alias(&mount), Err(()));
    }
    mount.drive_letter = "V:".into();
    mount.authentication_id = None;
    assert_eq!(recovery::private_alias_absent(&mount), Err(()));
    assert_eq!(recovery::cleanup_private_alias(&mount), Err(()));
}
