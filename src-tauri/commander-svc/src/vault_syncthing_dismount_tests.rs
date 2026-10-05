// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
fn sync_enrollment_preserves_recovery_notice_and_rejects_malformed_flags() {
    let legacy = serde_json::json!({ "managed": true, "gui_url": "http://127.0.0.1:8385" });
    let result = syncthing_enrollment_result(&legacy).unwrap();
    assert_eq!(result.gui_url, "http://127.0.0.1:8385");
    assert!(!result.recovery_required);
    for flag in [false, true] {
        let mut response = legacy.clone();
        response["recovery_required"] = serde_json::json!(flag);
        assert_eq!(syncthing_enrollment_result(&response).unwrap().recovery_required, flag);
    }
    for invalid in [serde_json::Value::Null, serde_json::json!("true"), serde_json::json!(1)] {
        let mut response = legacy.clone();
        response["recovery_required"] = invalid;
        assert_eq!(syncthing_enrollment_result(&response), Err(VaultMountReason::BrokerRejected));
    }
    for invalid_url in ["http://example.com:8385", "http://127.0.0.1:0"] {
        let mut response = legacy.clone();
        response["gui_url"] = serde_json::json!(invalid_url);
        response["recovery_required"] = serde_json::json!(true);
        assert_eq!(syncthing_enrollment_result(&response), Err(VaultMountReason::BrokerRejected));
    }
}

#[test]
fn sync_pause_failure_preserves_its_cause_and_allows_a_confirmed_retry() {
    for reason in [
        VaultMountReason::BrokerUnavailable,
        VaultMountReason::BrokerRejected,
        VaultMountReason::SyncthingProfileUnavailable,
        VaultMountReason::SyncthingRootConflict,
    ] {
        for personal in [false, true] {
            let store = mount_store(
                Arc::new(Mutex::new(HashMap::new())),
                Arc::new(AtomicBool::new(false)),
            );
            let events = Arc::new(Mutex::new(BrokerEvents::default()));
            let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
            let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
            mount.presentation = VaultPresentation::PerUser;
            mount.personal = personal;
            mount.syncthing_managed = true;
            let before = serde_json::to_value(&mount).unwrap();
            broker.active.lock().unwrap().insert("managed".into(), mount);

            let failed = broker.dismount_entry_locked_for_client_inner(
                42, &store, "managed", Some(std::ptr::null_mut()), |feature, _| {
                    assert_eq!(feature, "vault.syncthing.pause");
                    Err(reason)
                },
            );
            assert_eq!(failed.state, VaultMountState::Failed);
            assert_eq!(failed.reason, Some(reason));
            assert!(events.lock().unwrap().dismounted.is_empty());
            assert_eq!(
                serde_json::to_value(&broker.active.lock().unwrap()["managed"]).unwrap(),
                before,
            );
            assert!(broker.recovery_allows_entry("managed", &store));
            assert!(broker.recovery_allows_entry("another-vault", &store));

            let recovered = broker.dismount_entry_locked_for_client_inner(
                43, &store, "managed", Some(std::ptr::null_mut()), |feature, _| {
                    assert_eq!(feature, "vault.syncthing.pause");
                    Ok(syncthing_lifecycle_result(&serde_json::json!({
                        "managed": true, "paused": true,
                    })))
                },
            );
            assert_eq!(recovered.state, VaultMountState::Unmounted);
            assert_eq!(recovered.reason, None);
            assert_eq!(events.lock().unwrap().dismounted, vec![12]);
            assert!(!broker.active.lock().unwrap().contains_key("managed"));
        }
    }
}

#[test]
fn missing_sync_binding_acknowledgement_does_not_dismount_a_previously_managed_vault() {
    for reply in [
        serde_json::json!({}),
        serde_json::json!({ "managed": false }),
        serde_json::json!({ "managed": "true" }),
    ] {
        let store = mount_store(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
        mount.presentation = VaultPresentation::PerUser;
        mount.syncthing_managed = true;
        broker.active.lock().unwrap().insert("managed".into(), mount);
        let result = broker.dismount_entry_locked_for_client_inner(
            42, &store, "managed", Some(std::ptr::null_mut()), |_, _| {
                Ok(syncthing_lifecycle_result(&reply))
            },
        );
        assert_eq!(result.state, VaultMountState::Failed);
        assert_eq!(result.reason, Some(VaultMountReason::BrokerRejected));
        assert!(events.lock().unwrap().dismounted.is_empty());
        assert!(broker.active.lock().unwrap().contains_key("managed"));
        assert!(broker.recovery_allows_entry("managed", &store));
        assert!(broker.recovery_allows_entry("another-vault", &store));
    }
}
