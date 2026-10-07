// SPDX-License-Identifier: AGPL-3.0-or-later

fn idempotent_policy_store() -> VaultAccessStore {
    struct Resolver;
    impl PrincipalResolver for Resolver {
        fn resolve_sid(&self, _: &str) -> Result<String, crate::vault_access::VaultError> {
            Ok("S-1-5-21-1-2-3-1001".into())
        }
    }
    let store = VaultAccessStore::open(
        Box::new(MountFs {
            files: Arc::new(Mutex::new(HashMap::new())),
            fail_next_active_write: Arc::new(AtomicBool::new(false)),
        }),
        Box::new(Resolver),
        Box::new(MountAcl),
        PathBuf::from("/policy"),
    );
    dismount_policy_fixture(&store, VaultPresentation::PerUser);
    store
}

fn policy_cleanup_request() -> AuthorizedDismount<'static> {
    AuthorizedDismount {
        caller_authentication_id: Some((99, 0)),
        operation_id: 1,
        entry_id: "managed",
        caller_token: 1usize as windows_sys::Win32::Foundation::HANDLE,
        caller_session: 11,
        caller_sid: "S-1-5-21-1-2-3-1001",
        caller_elevated: false,
    }
}

#[test]
fn authorized_retry_after_startup_cleanup_returns_unmounted_without_another_dismount() {
    let store = idempotent_policy_store();
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    broker.policy_authorizer = dismount_test_policy_token;
    let mut mount = expired_private_mount();
    mount.personal = false;
    mount.caller_sid = "S-1-5-21-1-2-3-1001".into();
    assert!(broker.retain_cleanup_mount(&store, "managed", mount));
    assert!(broker.load_and_cleanup(&store).is_ok());
    assert!(broker.active.lock().unwrap().is_empty());
    let result = broker.dismount_authorized(&store, policy_cleanup_request());
    assert_eq!(result.state, VaultMountState::Unmounted);
    assert_eq!(result.reason, None);
    assert_eq!(result.presentation, Some(VaultPresentation::PerUser));
    assert_eq!(events.lock().unwrap().recovered, vec![12]);
    assert!(events.lock().unwrap().dismounted.is_empty());
}

#[test]
fn unmounted_retry_requires_existing_policy_grant_and_private_owner() {
    for scenario in [
        "missing-entry",
        "no-grant",
        "wrong-owner",
        "invalid-session",
    ] {
        let store = idempotent_policy_store();
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.engine_snapshot = Some(|| Ok(HashMap::new()));
        broker.policy_authorizer = dismount_test_policy_token;
        let mut request = policy_cleanup_request();
        match scenario {
            "missing-entry" => request.entry_id = "unknown",
            "no-grant" => request.caller_token = std::ptr::null_mut(),
            "wrong-owner" => request.caller_sid = "S-1-5-21-1-2-3-1002",
            _ => request.caller_session = 0,
        }
        assert_eq!(
            broker.dismount_authorized(&store, request).reason,
            Some(VaultMountReason::MountStateUnknown)
        );
        assert!(events.lock().unwrap().recovered.is_empty());
        assert!(events.lock().unwrap().dismounted.is_empty());
    }
}

#[test]
fn unmounted_retry_preserves_uncertain_journal_and_driver_state() {
    for scenario in [
        "untrusted",
        "pending-save",
        "pending-remove",
        "driver-failed",
        "untracked",
        "reused",
    ] {
        let store = idempotent_policy_store();
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.engine_snapshot = Some(|| Ok(HashMap::new()));
        broker.policy_authorizer = dismount_test_policy_token;
        match scenario {
            "untrusted" => broker.mark_registry_untrusted(),
            "pending-save" => {
                broker
                    .recovery
                    .lock()
                    .unwrap()
                    .persistence_pending
                    .insert("other".into());
            }
            "pending-remove" => {
                broker
                    .recovery
                    .lock()
                    .unwrap()
                    .removal_pending
                    .insert("other".into());
            }
            "driver-failed" => broker.engine_snapshot = Some(|| Err("unavailable".into())),
            "untracked" => {
                broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "unknown".into())])))
            }
            _ => {
                broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "reused".into())])));
                broker
                    .active
                    .lock()
                    .unwrap()
                    .insert("other".into(), expired_private_mount());
            }
        }
        assert_eq!(
            broker
                .dismount_authorized(&store, policy_cleanup_request())
                .reason,
            Some(VaultMountReason::MountStateUnknown)
        );
        assert!(events.lock().unwrap().recovered.is_empty());
        assert!(events.lock().unwrap().dismounted.is_empty());
    }
}

#[test]
fn unmounted_retry_accepts_exactly_accounted_for_unrelated_live_mount() {
    let store = idempotent_policy_store();
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.policy_authorizer = dismount_test_policy_token;
    broker
        .active
        .lock()
        .unwrap()
        .insert("other".into(), expired_private_mount());
    let result = broker.dismount_authorized(&store, policy_cleanup_request());
    assert_eq!(result.state, VaultMountState::Unmounted);
    assert!(broker.active.lock().unwrap().contains_key("other"));
    assert!(events.lock().unwrap().recovered.is_empty());
    assert!(events.lock().unwrap().dismounted.is_empty());
}

#[test]
fn unmounted_retry_rejects_a_driver_change_during_confirmation() {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let store = idempotent_policy_store();
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.policy_authorizer = dismount_test_policy_token;
    CALLS.store(0, Ordering::SeqCst);
    broker.engine_snapshot = Some(|| {
        if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(HashMap::new())
        } else {
            Ok(HashMap::from([(12, "new-external-mount".into())]))
        }
    });
    assert_eq!(
        broker
            .dismount_authorized(&store, policy_cleanup_request())
            .reason,
        Some(VaultMountReason::MountStateUnknown)
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    assert!(events.lock().unwrap().recovered.is_empty());
    assert!(events.lock().unwrap().dismounted.is_empty());
}
