// SPDX-License-Identifier: AGPL-3.0-or-later

fn custom_per_user_policy_store(shared: bool, group_owner: bool) -> VaultAccessStore {
    struct Resolver;
    impl PrincipalResolver for Resolver {
        fn resolve_sid(&self, name: &str) -> Result<String, crate::vault_access::VaultError> {
            Ok(match name { "Member" => "S-1-5-21-1-2-3-1002", "Group" => "S-1-5-21-1-2-3-1100", _ => "S-1-5-21-1-2-3-1001" }.into())
        }
        fn resolve_principal(&self, name: &str) -> Result<crate::vault_access::ResolvedPrincipal, crate::vault_access::VaultError> {
            Ok(crate::vault_access::ResolvedPrincipal { sid: self.resolve_sid(name)?, kind: if name == "Group" { crate::vault_access::PrincipalKind::Group } else { crate::vault_access::PrincipalKind::User } })
        }
    }
    let store = VaultAccessStore::open(Box::new(MountFs {
        files: Arc::new(Mutex::new(HashMap::new())), fail_next_active_write: Arc::new(AtomicBool::new(false)),
    }), Box::new(Resolver), Box::new(MountAcl), PathBuf::from("/policy"));
    let mut entry = serde_json::json!({ "id":"managed", "label":"Test", "container_path":"C:\\vaults\\managed.hc",
        "owner_account":"Owner", "primary_owner_sid":"S-1-5-21-1-2-3-1001",
        "grants":[{"principal_name":"Owner","access":"write"}],
        "mount":{"presentation":VaultPresentation::PerUser,"preferred_letter":"V"} });
    if group_owner {
        entry["owner_account"] = serde_json::json!("Group");
        entry["primary_owner_sid"] = serde_json::Value::Null;
        entry["grants"] = serde_json::json!([{"principal_name":"Group","access":"write"}]);
    } else if shared {
        entry["grants"].as_array_mut().unwrap().push(serde_json::json!({"principal_name":"Member","access":"read"}));
    } else {
        entry["access_pattern"] = serde_json::json!("private");
    }
    store.apply(serde_json::from_value(serde_json::json!({ "schema_version":1,"policy_id":"policy","version":1,"expected_previous_version":0,"entries":[entry] })).unwrap(), 1).unwrap();
    store
}

fn custom_shared_policy_token(store: &VaultAccessStore, entry: &str, token: windows_sys::Win32::Foundation::HANDLE) -> wincmd_shared::vault_access::VaultAuthorizeMountResponse {
    let sids = match token as usize {
        1 => vec!["S-1-5-21-1-2-3-1001".into(), "S-1-5-21-1-2-3-1100".into()],
        2 => vec!["S-1-5-21-1-2-3-1002".into(), "S-1-5-21-1-2-3-1100".into()],
        _ => vec!["S-1-5-21-1-2-3-1003".into()],
    };
    store.authorize_mount(entry, &sids)
}

fn assert_original_shared_alias_scope(mount: &ActiveMount) -> Result<bool, ()> {
    assert_eq!(mount.caller_sid, "S-1-5-21-1-2-3-1001");
    assert_eq!(mount.session_id, 7);
    assert_eq!(mount.authentication_id, Some((42, 0)));
    assert_eq!(mount.internal_drive, 12);
    Ok(true)
}

#[test]
fn shared_per_user_member_cleanup_preserves_private_denials_identity_pause_and_alias_guards() {
    for scenario in ["shared", "private", "personal", "no-grant", "identity", "empty-identity", "native-reused", "alias-unknown", "sync-unknown"] {
        for by_slot in [false, true] {
            for elevated in [false, true] {
                let store = custom_per_user_policy_store(scenario != "private", false);
                let events = Arc::new(Mutex::new(BrokerEvents::default()));
                let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
                broker.policy_authorizer = custom_shared_policy_token;
                if scenario == "native-reused" { broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "external-native-instance".into())]))); }
                broker.owned_private_alias_cleanup = if scenario == "alias-unknown" { |_| Err(()) } else { assert_original_shared_alias_scope };
                let mut mount = active_mount_for_owner(7, "S-1-5-21-1-2-3-1001");
                mount.presentation = VaultPresentation::PerUser;
                mount.authentication_id = Some((42, 0));
                mount.container_identity = if scenario == "identity" { "different-file" } else { "v:1:i:2" }.into();
                if scenario == "empty-identity" { mount.container_identity.clear(); }
                mount.personal = scenario == "personal";
                mount.syncthing_resume_unresolved = scenario == "sync-unknown";
                assert!(broker.retain_cleanup_mount(&store, "managed", mount));
                let before = store.read_active_mounts().unwrap();
                let caller = if scenario == "no-grant" { "S-1-5-21-1-2-3-1003" } else { "S-1-5-21-1-2-3-1002" };
                let token = if scenario == "no-grant" { 3usize } else { 2usize } as windows_sys::Win32::Foundation::HANDLE;
                if scenario == "shared" {
                    let policy = store.policy().unwrap();
                    assert!(custom_shared_policy_token(&store, "managed", token).allowed, "shared token grant");
                    assert!(store.is_shared_per_user_policy_for_authorized_caller("managed", Some("v:1:i:2"), caller),
                        "sharing classification pattern={:?} identity_matches={}", policy.entries[0].access_pattern,
                        policy.entries[0].container_identity.as_deref() == Some("v:1:i:2"));
                }
                let result = if by_slot {
                    broker.dismount_personal_for_caller(&store, 1, 12, token, 8, caller, elevated)
                } else {
                    broker.dismount_authorized(&store, AuthorizedDismount { operation_id:1, entry_id:"managed", caller_token:token,
                        caller_session:8, caller_sid:caller, caller_elevated:elevated, caller_authentication_id:Some((99, 0)) })
                };
                assert_eq!(result.state == VaultMountState::Unmounted, scenario == "shared", "{scenario} slot={by_slot} admin={elevated}");
                if scenario == "sync-unknown" { assert_eq!(result.reason, Some(VaultMountReason::SyncthingProfileUnavailable)); }
                let events = events.lock().unwrap();
                assert_eq!(events.recovered.len(), usize::from(matches!(scenario, "shared" | "alias-unknown")), "{scenario}");
                if matches!(scenario, "shared" | "alias-unknown") {
                    assert_eq!(events.cleanup_contexts[0], (12, None, VaultPresentation::PerUser, 7, "S-1-5-21-1-2-3-1001".into()));
                } else {
                    assert_eq!(store.read_active_mounts().unwrap(), before, "{scenario}");
                }
                if scenario == "alias-unknown" { assert!(broker.active.lock().unwrap()["managed"].cleanup_required); }
            }
        }
    }
}

#[test]
fn shared_per_user_members_can_cleanup_and_retry_without_cross_logon_browsing() {
    for group_owner in [false, true] {
        let store = custom_per_user_policy_store(true, group_owner);
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.policy_authorizer = custom_shared_policy_token;
        broker.owned_private_alias_cleanup = assert_original_shared_alias_scope;
        let mut mount = active_mount_for_owner(7, "S-1-5-21-1-2-3-1001");
        mount.presentation = VaultPresentation::PerUser;
        mount.authentication_id = Some((42, 0));
        mount.container_identity = "v:1:i:2".into();
        assert!(broker.retain_cleanup_mount(&store, "managed", mount));
        let token = 2usize as windows_sys::Win32::Foundation::HANDLE;
        let rows = broker.personal_mounts_for_caller(&store, token, 8, "S-1-5-21-1-2-3-1002", false).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].dismount_allowed);
        assert!(!rows[0].browse_allowed);
        for _ in 0..2 {
            let result = broker.dismount_authorized(&store, AuthorizedDismount { operation_id:1, entry_id:"managed", caller_token:token,
                caller_session:8, caller_sid:"S-1-5-21-1-2-3-1002", caller_elevated:false, caller_authentication_id:Some((99, 0)) });
            assert_eq!(result.state, VaultMountState::Unmounted, "group={group_owner}");
        }
        assert_eq!(events.lock().unwrap().recovered, vec![12]);
        assert!(broker.active.lock().unwrap().is_empty());
    }
}
