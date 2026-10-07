// SPDX-License-Identifier: AGPL-3.0-or-later

#[test]
#[ignore = "read-only installed registry and driver comparison; no live operations"]
fn live_registry_projection_diagnostic_reads_only() {
    let path = crate::policy_store::default_policy_dir().join("vault-active-mounts-v1.json");
    let registry: DurableMountRegistry = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("installed_registry=absent");
            DurableMountRegistry { mounts: HashMap::new() }
        }
        Err(error) => panic!("installed registry read failed: {error}"),
    };
    let slots = wincmd_volume::mounted_slot_identities().expect("physical observer failed");
    let matching = registry
        .mounts
        .values()
        .filter(|mount| mount.engine_mount_identity.as_ref().is_some_and(|identity| {
            slots.get(&mount.internal_drive) == Some(identity)
        }))
        .count();
    let untracked = slots
        .keys()
        .filter(|slot| {
            !registry
                .mounts
                .values()
                .any(|mount| mount.internal_drive == **slot)
        })
        .count();
    println!(
        "registered={} physically_observed={} exact_matches={} untracked={}",
        registry.mounts.len(),
        slots.len(),
        matching,
        untracked
    );
    // Projection persistence uses an in-memory test filesystem, never the installed store.
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(
        BrokerEvents::default(),
    )))));
    broker.engine_snapshot = Some(wincmd_volume::mounted_slot_identities);
    *broker.active.lock().unwrap() = registry.mounts;
    println!("untracked_cleanup_required={:?}", broker.has_untracked_mounts());
    match broker.personal_mounts_for_caller(
        &store,
        std::ptr::null_mut(),
        1,
        "diagnostic-no-private-owner",
        false,
    ) {
        Ok(rows) => println!("ordinary_projection_ok={} rows={}", true, rows.len()),
        Err(reason) => println!("ordinary_projection_error={}", reason.as_str()),
    }
}

#[test]
fn unknown_registry_mount_denials_are_not_reported_as_a_failed_dismount() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    assert_eq!(
        broker.recovery_failure_reason(),
        VaultMountReason::DismountFailed
    );
    broker.mark_registry_untrusted();
    let record = personal_record();
    let mut request = personal_request();
    assert_eq!(
        broker.mount_personal_authorized(
            1,
            &store,
            &record,
            &mut request,
            std::ptr::null_mut(),
            7,
            &record.owner_sid,
            (0, 0)
        ),
        Err(VaultMountReason::MountStateUnknown)
    );
    assert!(request.password.is_empty());
    let mut password = "secret".to_owned();
    let mut hidden_password = Some("hidden-secret".to_owned());
    let result = broker.mount_authorized_locked(
        2,
        &store,
        "managed",
        &mut password,
        &mut hidden_password,
        VaultVolumeRole::Outer,
        std::ptr::null_mut(),
        7,
        &record.owner_sid,
        (0, 0),
        wincmd_shared::vault_access::VaultAccess::Write,
    );
    assert_eq!(result.reason, Some(VaultMountReason::MountStateUnknown));
    assert!(password.is_empty());
    assert!(hidden_password.is_none());
    assert_eq!(events.lock().unwrap().mounted, 0);
    assert!(events.lock().unwrap().dismounted.is_empty());
    assert!(events.lock().unwrap().recovered.is_empty());
}

#[test]
fn ordinary_machine_dismount_allows_admin_but_private_mounts_never_have_admin_override() {
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
                    let allowed = (owner && session == 7)
                        || (presentation == VaultPresentation::Machine && elevated);
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
fn dismount_policy_matrix_requires_grant_and_private_owner_or_shared_elevation_on_both_routes() {
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
                    caller_authentication_id: None,
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
        assert!(broker.recovery.lock().unwrap().registry_untrusted);
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
    assert_eq!(broker.projection("mount").0, VaultMountState::Failed);
}

#[test]
fn inventory_retains_absent_slot_cleanup_authority_without_reporting_a_mounted_drive() {
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
    assert!(rows[0].dismount_allowed);
    assert!(rows[0].browse_allowed);
    assert_eq!(
        rows[0].dismount_reason,
        None
    );
    broker.engine_snapshot = Some(|| Err("query unavailable".into()));
    assert_eq!(
        broker.personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true),
        Err(VaultMountReason::MountStateUnknown)
    );
    assert_eq!(broker.projection("mount").0, VaultMountState::Failed);
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "different-generation".into())])));
    assert!(broker
        .personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true)
        .is_err());
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    let rows = broker
        .personal_mounts_for_caller(&store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].cleanup_required && rows[0].dismount_allowed);
    assert!(!rows[0].browse_allowed);
    assert_eq!(broker.projection("mount").0, VaultMountState::Failed);
    let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
    assert!(saved.mounts["mount"].driver_slot_absent);
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(2, "unknown-volume".into())])));
    let rows = broker.personal_mounts_for_caller(
        &store, std::ptr::null_mut(), 7, "S-1-5-21-owner", true,
    ).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].cleanup_required);
    assert!(!rows[0].browse_allowed);
}

#[test]
fn untracked_driver_mount_does_not_block_or_authorize_caller_inventory() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.engine_snapshot = Some(|| Ok(HashMap::from([
        (12, "test-mount:12".into()), (13, "untracked-mount".into()),
    ])));
    assert_eq!(broker.has_untracked_mounts(), Ok(true));
    for elevated in [false, true] {
        assert!(broker.personal_mounts_for_caller(
            &store, std::ptr::null_mut(), 7, "S-1-5-21-owner", elevated,
        ).unwrap().is_empty());
    }
    let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
    mount.personal = true;
    broker.active.lock().unwrap().insert("owned".into(), mount);
    let rows = broker.personal_mounts_for_caller(
        &store, std::ptr::null_mut(), 7, "S-1-5-21-owner", false,
    ).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].internal_drive, 12);
    let result = broker.dismount_personal_for_caller(
        &store, 1, 13, std::ptr::null_mut(), 7, "S-1-5-21-owner", true,
    );
    assert_ne!(result.state, VaultMountState::Unmounted);
    assert!(events.lock().unwrap().dismounted.is_empty());
    assert!(events.lock().unwrap().recovered.is_empty());
    broker.engine_snapshot = Some(|| Err("query unavailable".into()));
    assert_eq!(broker.has_untracked_mounts(), Err(VaultMountReason::MountStateUnknown));
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    assert_eq!(broker.has_untracked_mounts(), Ok(false));
}

#[test]
fn absent_slot_retains_exact_cleanup_context_until_alias_removal_is_confirmed() {
    for presentation in [VaultPresentation::Machine, VaultPresentation::PerUser] {
        let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
        let events = Arc::new(Mutex::new(BrokerEvents { cleanup_fails: true, ..Default::default() }));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.engine_snapshot = Some(|| Ok(HashMap::new()));
        let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
        mount.personal = true;
        mount.presentation = presentation;
        mount.drive_letter = "W:".into();
        assert!(broker.retain_cleanup_mount(&store, "stale", mount));
        let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 7, "S-1-5-21-owner", false);
        assert_eq!(result.state, VaultMountState::Failed);
        let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        assert!(saved.mounts["stale"].cleanup_required && saved.mounts["stale"].driver_slot_absent);
        assert_eq!(events.lock().unwrap().cleanup_contexts[0],
            (12, Some("W:".into()), presentation, 7, "S-1-5-21-owner".into()));
        broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "test-mount:12".into())])));
        let reused = broker.dismount_personal_for_caller(&store, 2, 12, std::ptr::null_mut(), 7, "S-1-5-21-owner", false);
        assert_eq!(reused.reason, Some(VaultMountReason::MountStateUnknown));
        assert_eq!(events.lock().unwrap().cleanup_contexts.len(), 1);
        broker.engine_snapshot = Some(|| Ok(HashMap::new()));
        events.lock().unwrap().cleanup_fails = false;
        let result = broker.dismount_personal_for_caller(&store, 2, 12, std::ptr::null_mut(), 7, "S-1-5-21-owner", false);
        assert_eq!(result.state, VaultMountState::Unmounted);
        let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        assert!(saved.mounts.is_empty());
    }
}

#[test]
fn startup_recovery_checks_aliases_even_if_the_driver_slot_is_already_absent() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents { cleanup_fails: true, ..Default::default() }));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
    mount.drive_letter = "Y:".into();
    assert!(broker.retain_cleanup_mount(&store, "stale", mount));
    assert_eq!(broker.load_and_cleanup(&store), Ok(HashSet::new()));
    let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
    assert!(saved.mounts["stale"].cleanup_required && saved.mounts["stale"].driver_slot_absent);
    assert!(!broker.recovery.lock().unwrap().registry_untrusted);
    assert!(broker.recovery_allows_entry("unrelated", &store));
    assert_eq!(broker.projection("stale").0, VaultMountState::Failed);
    assert_eq!(events.lock().unwrap().cleanup_contexts[0].1, Some("Y:".into()));
    assert_eq!(events.lock().unwrap().recovered, vec![12]);
}

#[test]
fn broker_success_requires_exact_slot_and_verified_presentation_cleanup() {
    for reply in [serde_json::json!({}), serde_json::json!({"status":"dismounted","internalDrive":12}),
        serde_json::json!({"status":"dismounted","internalDrive":12,"presentationCleanup":false}),
        serde_json::json!({"status":"dismounted","internalDrive":13,"presentationCleanup":true})] {
        assert_eq!(confirmed_broker_dismount(&reply, 12), Err(VaultMountReason::DismountFailed));
    }
    assert!(confirmed_broker_dismount(&serde_json::json!({"status":"dismounted","internalDrive":12,"presentationCleanup":true}), 12).is_ok());
}

#[test]
fn ended_private_policy_owner_can_retry_cleanup_from_a_new_logon() {
    let store = mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    // This models an old Test2 logon whose K: presentation disappeared while
    // its exact encrypted-driver slot is still live. A current policy grant is
    // deliberately unavailable: the recovery must close the stale recorded
    // mount, but it must not make the account eligible for a fresh mount.
    broker.owner_logon_probe = |_| Ok(true);
    broker.policy_authorizer = |_, _, _| wincmd_shared::vault_access::VaultAuthorizeMountResponse {
        allowed: false,
        launch_ready: false,
        denial_reason: Some(wincmd_shared::vault_access::VaultMountDenial::NotAuthorized),
        mode: None,
        presentation: None,
        preferred_letter: None,
    };
    let mut mount = active_mount_for_owner(9, "S-1-5-21-owner");
    mount.personal = false;
    mount.presentation = VaultPresentation::PerUser;
    assert!(broker.retain_cleanup_mount(&store, "private-policy", mount));

    let result = broker.dismount_authorized(
        &store,
        AuthorizedDismount {
            caller_authentication_id: Some((99, 1)),
            operation_id: 42,
            entry_id: "private-policy",
            caller_token: std::ptr::null_mut(),
            caller_session: 10,
            caller_sid: "S-1-5-21-owner",
            caller_elevated: false,
        },
    );

    assert_eq!(result.state, VaultMountState::Unmounted);
    assert_eq!(events.lock().unwrap().recovered, vec![12]);
    assert!(broker.active.lock().unwrap().is_empty());
}
