// SPDX-License-Identifier: AGPL-3.0-or-later

fn expired_private_mount() -> ActiveMount {
    let mut mount = active_mount_for_owner(7, "S-1-5-21-owner");
    mount.personal = true;
    mount.presentation = VaultPresentation::PerUser;
    mount.authentication_id = Some((42, 0));
    mount.syncthing_managed = false;
    mount.syncthing_resume_unresolved = false;
    mount
}

#[test]
fn ended_logon_and_native_absence_retire_only_unbound_private_records() {
    for bound in [false, true] {
        let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.owner_logon_probe = |_| Ok(true);
        broker.engine_snapshot = Some(|| Ok(HashMap::new()));
        let mut mount = expired_private_mount();
        mount.syncthing_managed = bound;
        assert!(broker.retain_cleanup_mount(&store, "old", mount));
        let rows = broker.personal_mounts_for_caller(&store, std::ptr::null_mut(), 8, "S-1-5-21-owner", false).unwrap();
        assert_eq!(rows.len(), usize::from(bound));
        if bound { assert!(rows[0].cleanup_required && rows[0].dismount_allowed && !rows[0].browse_allowed); }
        let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
        assert_eq!(saved.mounts.len(), usize::from(bound));
        assert!(events.lock().unwrap().dismounted.is_empty());
        assert!(events.lock().unwrap().recovered.is_empty());
    }
}

#[test]
fn relogged_owner_can_close_verified_private_mount_without_using_old_alias() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
    assert_eq!(broker.projection("old").0, VaultMountState::Failed);
    let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-owner", false);
    assert_eq!(result.state, VaultMountState::Unmounted);
    let events = events.lock().unwrap();
    assert_eq!(events.recovered, vec![12]);
    assert_eq!(events.cleanup_contexts[0].1, None);
    assert!(broker.active.lock().unwrap().is_empty());
}

#[test]
fn another_logon_cannot_claim_live_disconnected_or_unknown_owner_mount() {
    for probe in [Ok(false), Err(())] {
        for caller in ["S-1-5-21-owner", "S-1-5-21-outsider"] {
            if probe == Ok(true) && caller == "S-1-5-21-owner" { continue; }
            let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
            let events = Arc::new(Mutex::new(BrokerEvents::default()));
            let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
            broker.owner_logon_probe = match probe { Ok(true) => |_| Ok(true), Ok(false) => |_| Ok(false), Err(()) => |_| Err(()) };
            assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
            let saved = store.read_active_mounts().unwrap();
            let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, caller, true);
            assert_eq!(result.reason, Some(VaultMountReason::MountStateUnknown));
            assert_eq!(store.read_active_mounts().unwrap(), saved);
            assert!(events.lock().unwrap().recovered.is_empty());
            assert!(events.lock().unwrap().dismounted.is_empty());
        }
    }
}

#[test]
fn valid_boot_journal_recovers_after_mismatched_native_instance_really_disappears() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
    let saved = store.read_active_mounts().unwrap();
    broker.engine_snapshot = Some(|| Ok(HashMap::from([(12, "new-generation".into())])));
    assert_eq!(broker.load_and_cleanup(&store), Err(VaultMountReason::MountStateUnknown));
    assert!(broker.recovery.lock().unwrap().registry_untrusted);
    assert!(broker.active.lock().unwrap().contains_key("old"));
    let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-owner", false);
    assert_eq!(result.reason, Some(VaultMountReason::MountStateUnknown));
    assert_eq!(store.read_active_mounts().unwrap(), saved);
    assert!(events.lock().unwrap().recovered.is_empty());
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    assert!(broker.personal_mounts_for_caller(&store, std::ptr::null_mut(), 8, "S-1-5-21-owner", false).unwrap().is_empty());
    assert!(!broker.recovery.lock().unwrap().registry_untrusted);
    assert!(broker.recovery_allows_entry("next", &store));
}

#[test]
fn missing_native_query_or_failed_journal_save_never_discards_recovery_authority() {
    for fail_write in [false, true] {
        let fail = Arc::new(AtomicBool::new(false));
        let store = mount_store(Arc::new(Mutex::new(HashMap::new())), fail.clone());
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(BrokerEvents::default())))));
        broker.owner_logon_probe = |_| Ok(true);
        assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
        let saved = store.read_active_mounts().unwrap();
        broker.engine_snapshot = if fail_write { Some(|| Ok(HashMap::new())) } else { Some(|| Err("unavailable".into())) };
        fail.store(fail_write, Ordering::SeqCst);
        assert!(broker.personal_mounts_for_caller(&store, std::ptr::null_mut(), 8, "S-1-5-21-owner", false).is_err());
        assert!(broker.active.lock().unwrap().contains_key("old"));
        assert_eq!(store.read_active_mounts().unwrap(), saved);
    }
}

#[test]
fn ended_owner_pause_uses_authenticated_new_session_and_preserves_failure() {
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(Arc::new(Mutex::new(BrokerEvents::default())))));
    broker.owner_logon_probe = |_| Ok(true);
    let mut mount = expired_private_mount();
    mount.syncthing_managed = true;
    let request = AuthorizedDismount {
                    caller_authentication_id: None, operation_id: 1, entry_id: "old", caller_token: std::ptr::null_mut(),
        caller_session: 8, caller_sid: "S-1-5-21-owner", caller_elevated: false };
    assert_eq!(broker.pause_expired_owner_binding(&request, &mount, true, |context| {
        assert_eq!(context.session_id, 8);
        assert_eq!(context.caller_sid, mount.caller_sid);
        assert_eq!(context.container_identity, mount.container_identity);
        Err(VaultMountReason::BrokerUnavailable)
    }), Err(VaultMountReason::BrokerUnavailable));
    assert_eq!(mount.session_id, 7);
}

#[test]
fn signoff_retains_bound_pause_obligation_after_native_cleanup() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    let mut mount = expired_private_mount();
    mount.syncthing_managed = true;
    assert!(broker.retain_cleanup_mount(&store, "old", mount));
    broker.dismount_session(&store, 7);
    assert_eq!(events.lock().unwrap().recovered, vec![12]);
    let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
    assert!(saved.mounts["old"].syncthing_managed);
    assert!(saved.mounts["old"].cleanup_required && saved.mounts["old"].driver_slot_absent);
}

#[test]
fn native_logon_probe_keeps_the_current_logon_and_legacy_session_alive() {
    use windows_sys::Win32::{Foundation::CloseHandle, Security::{GetTokenInformation,
        TokenStatistics, TOKEN_QUERY, TOKEN_STATISTICS}, System::{Threading::{GetCurrentProcess,
        GetCurrentProcessId, OpenProcessToken}, RemoteDesktop::ProcessIdToSessionId}};
    let mut token = std::ptr::null_mut();
    assert_ne!(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }, 0);
    let mut statistics: TOKEN_STATISTICS = unsafe { std::mem::zeroed() };
    let mut size = 0;
    let queried = unsafe { GetTokenInformation(token, TokenStatistics,
        (&mut statistics as *mut TOKEN_STATISTICS).cast(), std::mem::size_of_val(&statistics) as u32, &mut size) };
    unsafe { CloseHandle(token); }
    assert_ne!(queried, 0);
    let mut mount = expired_private_mount();
    mount.authentication_id = Some((statistics.AuthenticationId.LowPart, statistics.AuthenticationId.HighPart));
    assert_eq!(recovery::owner_logon_ended(&mount), Ok(false));
    assert_ne!(unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut mount.session_id) }, 0);
    mount.authentication_id = None;
    assert_eq!(recovery::owner_logon_ended(&mount), Ok(false));
}

#[test]
fn reused_session_number_requires_original_luid_or_proven_old_logon_end() {
    for ended in [false, true] {
        let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
        broker.owner_logon_probe = if ended { |_| Ok(true) } else { |_| Err(()) };
        assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
        let saved = store.read_active_mounts().unwrap();
        let result = broker.dismount_authorized(&store, AuthorizedDismount {
            operation_id: 1, entry_id: "old", caller_token: std::ptr::null_mut(),
            caller_session: 7, caller_sid: "S-1-5-21-owner", caller_elevated: true,
            caller_authentication_id: Some((43, 0)),
        });
        if ended { assert_eq!(result.state, VaultMountState::Unmounted); }
        else {
            assert_eq!(result.reason, Some(VaultMountReason::MountStateUnknown));
            assert_eq!(store.read_active_mounts().unwrap(), saved);
            assert!(events.lock().unwrap().recovered.is_empty());
            assert!(events.lock().unwrap().dismounted.is_empty());
        }
    }
}

struct ForeignUncertainBroker(MountBroker);
impl AuthenticatedVaultBroker for ForeignUncertainBroker {
    fn observed_slots(&self) -> Result<HashMap<u8, String>, String> {
        let mut slots = self.0.observed_slots()?;
        slots.insert(13, "foreign-new-generation".into());
        Ok(slots)
    }
    fn mount(&self, request: &mut InternalMountRequest) -> Result<InternalMountReply, VaultMountReason> { self.0.mount(request) }
    fn dismount(&self, request: BrokerDismountRequest<'_>) -> Result<(), VaultMountReason> { self.0.dismount(request) }
    fn cleanup_orphans(&self) -> Result<(), VaultMountReason> { Ok(()) }
    fn recover_dismount(&self, request: BrokerDismountRequest<'_>) -> Result<(), VaultMountReason> { self.0.recover_dismount(request) }
}

#[test]
fn own_verified_cleanup_preserves_foreign_uncertainty_and_never_adopts_its_slot() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(ForeignUncertainBroker(MountBroker(events.clone()))));
    broker.owner_logon_probe = |_| Ok(true);
    assert!(broker.retain_cleanup_mount(&store, "own", expired_private_mount()));
    let mut foreign = expired_private_mount();
    foreign.internal_drive = 13;
    foreign.caller_sid = "S-1-5-21-other".into();
    foreign.engine_mount_identity = Some("foreign-old-generation".into());
    assert!(broker.retain_cleanup_mount(&store, "foreign", foreign));
    assert_eq!(broker.load_and_cleanup(&store), Err(VaultMountReason::MountStateUnknown));
    let result = broker.dismount_personal_for_caller(&store, 1, 12, std::ptr::null_mut(), 8, "S-1-5-21-owner", false);
    assert_eq!(result.state, VaultMountState::Unmounted);
    assert_eq!(events.lock().unwrap().recovered, vec![12]);
    assert!(broker.recovery.lock().unwrap().registry_untrusted);
    let saved: DurableMountRegistry = serde_json::from_slice(&store.read_active_mounts().unwrap()).unwrap();
    assert_eq!(saved.mounts.len(), 1);
    assert_eq!(saved.mounts["foreign"].engine_mount_identity.as_deref(), Some("foreign-old-generation"));
}

#[test]
fn delayed_logoff_retry_cannot_close_a_new_mount_in_the_reused_session() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(true);
    assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
    let targets = broker.session_cleanup_targets(7);
    let mut new_mount = expired_private_mount();
    new_mount.authentication_id = Some((43, 0));
    assert!(broker.retain_cleanup_mount(&store, "old", new_mount));
    let saved = store.read_active_mounts().unwrap();
    broker.dismount_session_targets(&store, &targets);
    assert!(events.lock().unwrap().recovered.is_empty());
    assert!(events.lock().unwrap().dismounted.is_empty());
    assert_eq!(store.read_active_mounts().unwrap(), saved);
}

#[test]
fn logoff_callback_never_waits_on_busy_mount_map_and_fallback_skips_live_logons() {
    let store = mount_store(Arc::new(Mutex::new(HashMap::new())), Arc::new(AtomicBool::new(false)));
    let events = Arc::new(Mutex::new(BrokerEvents::default()));
    let mut broker = VaultMountBroker::with_broker(Box::new(MountBroker(events.clone())));
    broker.owner_logon_probe = |_| Ok(false);
    assert!(broker.retain_cleanup_mount(&store, "own", expired_private_mount()));
    let operation = broker.operation.lock().unwrap();
    assert!(broker.try_session_cleanup_targets(7).is_none());
    drop(operation);
    let busy = broker.active.lock().unwrap();
    assert!(broker.try_session_cleanup_targets(7).is_none());
    drop(busy);
    let targets = broker.expired_session_cleanup_targets(7);
    assert!(targets.mounts.is_empty());
    broker.dismount_session_targets(&store, &targets);
    assert!(events.lock().unwrap().dismounted.is_empty());
    assert!(events.lock().unwrap().recovered.is_empty());
    broker.owner_logon_probe = |_| Ok(true);
    let expired = broker.expired_session_cleanup_targets(7);
    broker.owner_logon_probe = |_| Err(());
    broker.dismount_session_targets(&store, &expired);
    assert!(events.lock().unwrap().dismounted.is_empty());
    assert!(events.lock().unwrap().recovered.is_empty());
    broker.owner_logon_probe = |_| Ok(true);
    broker.dismount_session_targets(&store, &expired);
    assert_eq!(events.lock().unwrap().recovered, vec![12]);
}
