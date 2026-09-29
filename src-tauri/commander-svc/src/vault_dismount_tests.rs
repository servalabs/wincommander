// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
fn dismount_personal_cross_account_matrix_never_gives_admin_a_private_override() {
    for presentation in [VaultPresentation::Machine, VaultPresentation::PerUser] {
        for owner in [false, true] {
            for elevated in [false, true] {
                for session in [7, 8] {
                    let store = mount_store(
                        Arc::new(Mutex::new(HashMap::new())),
                        Arc::new(AtomicBool::new(false)),
                    );
                    let events = Arc::new(Mutex::new(BrokerEvents::default()));
                    let broker =
                        VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
                    let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
                    mount.personal = true;
                    mount.presentation = presentation;
                    broker
                        .active
                        .lock()
                        .unwrap()
                        .insert("secret-private-id".into(), mount);
                    let caller = if owner {
                        "S-1-5-21-owner"
                    } else {
                        "S-1-5-21-other"
                    };
                    let result = broker.dismount_personal_for_caller(
                        &store,
                        1,
                        12,
                        std::ptr::null_mut(),
                        session,
                        caller,
                        elevated,
                    );
                    let allowed = if presentation == VaultPresentation::Machine {
                        elevated
                    } else {
                        owner && session == 7
                    };
                    assert_eq!(result.state == VaultMountState::Unmounted, allowed);
                    let events = events.lock().unwrap();
                    assert_eq!(
                        events.dismounted.len() + events.recovered.len(),
                        usize::from(allowed)
                    );
                    if !allowed {
                        assert_eq!(
                            result.reason,
                            Some(if presentation == VaultPresentation::PerUser {
                                VaultMountReason::MountStateUnknown
                            } else {
                                VaultMountReason::AdministratorRequired
                            })
                        );
                        assert!(!serde_json::to_string(&result)
                            .unwrap()
                            .contains("secret-private-id"));
                    }
                }
            }
        }
    }
}

fn dismount_policy_fixture(store: &VaultAccessStore, presentation: VaultPresentation) {
    let grants = if presentation == VaultPresentation::Machine {
        serde_json::json!([{"principal_name": "Owner", "access": "write"}, {"principal_name": "Member", "access": "read"}])
    } else {
        serde_json::json!([{"principal_name": "Owner", "access": "write"}])
    };
    store.apply(serde_json::from_value(serde_json::json!({
        "schema_version": 1, "policy_id": "policy", "version": 1, "expected_previous_version": 0,
        "entries": [{ "id": "managed", "label": "Protected", "container_path": "C:\\vaults\\managed.hc",
            "primary_owner_sid": "S-1-5-21-1-2-3-1001", "owner_account": "Owner", "grants": grants,
            "mount": {"presentation": presentation, "preferred_letter": "V"} }]
    })).unwrap(), 1).unwrap();
}

fn dismount_test_policy_token(
    store: &VaultAccessStore,
    entry: &str,
    token: windows_sys::Win32::Foundation::HANDLE,
) -> wincmd_shared::vault_access::VaultAuthorizeMountResponse {
    let sids = if token.is_null() {
        vec![]
    } else {
        vec!["S-1-5-21-1-2-3-1001".into()]
    };
    store.authorize_mount(entry, &sids)
}

#[test]
fn dismount_policy_matrix_requires_shared_elevation_and_grant_on_both_routes() {
    struct Resolver;
    impl PrincipalResolver for Resolver {
        fn resolve_sid(&self, name: &str) -> Result<String, crate::vault_access::VaultError> {
            Ok(if name == "Member" {
                "S-1-5-21-1-2-3-1002"
            } else {
                "S-1-5-21-1-2-3-1001"
            }
            .into())
        }
    }
    for presentation in [VaultPresentation::Machine, VaultPresentation::PerUser] {
        for owner in [false, true] {
            for elevated in [false, true] {
                for granted in [false, true] {
                    for by_slot in [false, true] {
                        let store = VaultAccessStore::open(
                            Box::new(MountFs {
                                files: Arc::new(Mutex::new(HashMap::new())),
                                fail_next_active_write: Arc::new(AtomicBool::new(false)),
                            }),
                            Box::new(Resolver),
                            Box::new(MountAcl),
                            PathBuf::from("/policy"),
                        );
                        dismount_policy_fixture(&store, presentation);
                        let events = Arc::new(Mutex::new(BrokerEvents::default()));
                        let mut broker =
                            VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
                        broker.policy_authorizer = dismount_test_policy_token;
                        let mut mount = active_mount_for_owner(7, "S-1-5-21-1-2-3-1001");
                        mount.presentation = presentation;
                        broker
                            .active
                            .lock()
                            .unwrap()
                            .insert("managed".into(), mount);
                        let caller = if owner {
                            "S-1-5-21-1-2-3-1001"
                        } else {
                            "S-1-5-21-other"
                        };
                        let token = if granted {
                            1usize as windows_sys::Win32::Foundation::HANDLE
                        } else {
                            std::ptr::null_mut()
                        };
                        let result = if by_slot {
                            broker.dismount_personal_for_caller(
                                &store, 1, 12, token, 7, caller, elevated,
                            )
                        } else {
                            broker.dismount_authorized(
                                &store,
                                AuthorizedDismount {
                                    operation_id: 1,
                                    entry_id: "managed",
                                    caller_token: token,
                                    caller_session: 7,
                                    caller_sid: caller,
                                    caller_elevated: elevated,
                                },
                            )
                        };
                        let allowed = granted
                            && if presentation == VaultPresentation::Machine {
                                elevated
                            } else {
                                owner
                            };
                        assert_eq!(result.state == VaultMountState::Unmounted, allowed,
                            "{presentation:?} owner={owner} elevated={elevated} granted={granted} slot={by_slot}");
                        let events = events.lock().unwrap();
                        assert_eq!(
                            events.dismounted.len() + events.recovered.len(),
                            usize::from(allowed)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dismount_refuses_reused_slots_legacy_unknown_and_unconfirmed_engine_success() {
    for proof in [
        None,
        Some("other-volume:12"),
        Some("test-mount:old-generation"),
    ] {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let store = mount_store(files, Arc::new(AtomicBool::new(false)));
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
        mount.personal = true;
        mount.engine_mount_identity = proof.map(str::to_owned);
        assert!(broker.retain_cleanup_mount(&store, "legacy", mount));
        let saved = store.read_active_mounts().unwrap();
        let result = broker.dismount_personal_for_caller(
            &store,
            1,
            12,
            std::ptr::null_mut(),
            7,
            "S-1-5-21-owner",
            true,
        );
        assert_eq!(result.reason, Some(VaultMountReason::MountStateUnknown));
        broker.dismount_session(&store, 7);
        assert!(broker.dismount_all(&store).is_err());
        assert_eq!(
            broker.load_and_cleanup(&store),
            Err(VaultMountReason::MountStateUnknown)
        );
        assert_eq!(store.read_active_mounts().unwrap(), saved);
        assert!(events.lock().unwrap().dismounted.is_empty());
        assert!(events.lock().unwrap().recovered.is_empty());
    }
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events)));
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "test-mount:12".into())])));
    let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
    mount.personal = true;
    broker.active.lock().unwrap().insert("mount".into(), mount);
    let result = broker.dismount_personal_for_caller(
        &store,
        1,
        12,
        std::ptr::null_mut(),
        7,
        "S-1-5-21-owner",
        true,
    );
    assert_eq!(result.state, VaultMountState::Failed);
    assert_eq!(broker.projection("mount").0, VaultMountState::Mounted);
}

#[test]
fn inventory_prunes_only_proven_absent_slots_and_never_invents_empty_on_error() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(
        BrokerEvents::default(),
    )))));
    let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
    mount.personal = true;
    mount.canonical_container_path = Some(r"\??\D:\Vaults\example.hc".into());
    broker.active.lock().unwrap().insert("mount".into(), mount);
    let rows = broker
        .personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", false)
        .unwrap();
    assert_eq!(
        rows[0].canonical_container_path.as_deref(),
        Some(r"D:\Vaults\example.hc")
    );
    assert!(!rows[0].dismount_allowed);
    assert!(rows[0].browse_allowed);
    assert_eq!(
        rows[0].dismount_reason,
        Some(VaultMountReason::AdministratorRequired)
    );
    broker.engine_snapshot = Some(|| Err("query unavailable".into()));
    assert_eq!(
        broker.personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true),
        Err(VaultMountReason::MountStateUnknown)
    );
    assert_eq!(broker.projection("mount").0, VaultMountState::Mounted);
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "different-generation".into())])));
    assert!(broker
        .personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true)
        .is_err());
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    assert!(broker
        .personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true)
        .unwrap()
        .is_empty());
    assert_eq!(broker.projection("mount").0, VaultMountState::Unmounted);
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(2, "unknown-volume".into())])));
    assert_eq!(
        broker.personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true),
        Err(VaultMountReason::MountStateUnknown)
    );
}
