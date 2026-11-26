// SPDX-License-Identifier: AGPL-3.0-or-later
// Dead drive-letter links are swept automatically and by the cleanup verb.

type MountingAlias = (String, u8, Option<(u32, i32)>);

thread_local! {
    static SWEEPS: std::cell::RefCell<Vec<Vec<char>>> = const { std::cell::RefCell::new(Vec::new()) };
    static T_RELEASED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static T_DEVICE_LIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static MOUNTING_ALIASES: std::cell::RefCell<Vec<MountingAlias>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

const KING_SID: &str = "S-1-5-21-1-2-3-1105";

fn swept_letters() -> Vec<Vec<char>> {
    SWEEPS.with(|sweeps| sweeps.borrow().clone())
}

fn t_until_released() -> Result<HashSet<String>, ()> {
    Ok(if T_RELEASED.get() {
        HashSet::from(["R".into()])
    } else {
        HashSet::from(["R".into(), "T".into()])
    })
}

/// SERVAULT: King's disconnected logon links T: to VeraCryptVolumeT; R: is the live RAM disk.
fn servault_holders(letter: char) -> Vec<LetterHolder> {
    let king = LetterHolder {
        logon: Some((0x0ceed27f, 0)),
        owner_sid: Some(KING_SID.into()),
        session: Some(4),
        state: crate::vault_drive_letters::LogonState::Disconnected,
        kind: crate::vault_drive_letters::HolderKind::VaultMount,
        device: Some("VeraCryptVolumeT".into()),
        device_present: T_DEVICE_LIVE.get(),
    };
    let ram_disk = LetterHolder {
        logon: None,
        owner_sid: None,
        session: None,
        state: crate::vault_drive_letters::LogonState::Machine,
        kind: crate::vault_drive_letters::HolderKind::RamDisk,
        device: Some("ImDisk0".into()),
        device_present: true,
    };
    match letter {
        'T' => vec![king],
        'R' => vec![ram_disk],
        _ => Vec::new(),
    }
}

fn servault_probe(letters: &HashSet<char>) -> Result<HashMap<char, Vec<LetterHolder>>, ()> {
    Ok(letters
        .iter()
        .map(|letter| (*letter, servault_holders(*letter)))
        .filter(|(_, holders)| !holders.is_empty())
        .collect())
}

fn servault_release(letters: &HashSet<char>) -> Result<HashMap<char, LetterRelease>, ()> {
    let mut sorted = letters.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable();
    SWEEPS.with(|sweeps| sweeps.borrow_mut().push(sorted));
    Ok(letters
        .iter()
        .filter(|letter| !(**letter == 'T' && T_RELEASED.get()))
        .map(|letter| {
            let checked = servault_holders(*letter)
                .into_iter()
                .map(|holder| {
                    let released = holder.is_dead_device();
                    if released && *letter == 'T' {
                        T_RELEASED.set(true);
                    }
                    (holder, released)
                })
                .collect();
            (*letter, LetterRelease { checked })
        })
        .filter(|(_, release)| !release.checked.is_empty())
        .collect())
}

fn record_mounting_alias(mount: &ActiveMount) -> Result<bool, ()> {
    MOUNTING_ALIASES.with(|calls| {
        calls.borrow_mut().push((
            mount.drive_letter.clone(),
            mount.internal_drive,
            mount.authentication_id,
        ))
    });
    Ok(true)
}

fn servault_broker(events: Arc<Mutex<BrokerEvents>>) -> VaultMountBroker {
    SWEEPS.with(|sweeps| sweeps.borrow_mut().clear());
    MOUNTING_ALIASES.with(|calls| calls.borrow_mut().clear());
    T_RELEASED.set(false);
    T_DEVICE_LIVE.set(false);
    let mut broker = VaultMountBroker::with_broker_and_attestor(
        Box::new(MountBroker(events)),
        caller_can_list_root,
    );
    broker.drive_letter_probe = t_until_released;
    broker.letter_holder_probe = servault_probe;
    broker.stale_letter_release = servault_release;
    broker.mounted_alias_cleanup = record_mounting_alias;
    broker
}

fn letter_store() -> VaultAccessStore {
    mount_store(
        Arc::new(Mutex::new(HashMap::new())),
        Arc::new(AtomicBool::new(false)),
    )
}

#[test]
fn a_dead_vault_link_does_not_make_its_letter_unavailable_but_a_live_one_does() {
    let store = letter_store();
    let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    let unavailable = broker.unavailable_letters_locked(&store, None).unwrap();
    assert!(!unavailable.contains(&"T".to_string()));
    assert!(
        unavailable.contains(&"R".to_string()),
        "the RAM disk stays unavailable"
    );
    T_DEVICE_LIVE.set(true);
    assert!(broker
        .unavailable_letters_locked(&store, None)
        .unwrap()
        .contains(&"T".to_string()));
}

#[test]
fn mounting_on_t_releases_the_dead_link_before_the_engine_runs() {
    let store = letter_store();
    let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    assert_eq!(
        broker.select_mount_letter(&store, Some("T"), "container", KING_SID),
        Ok("T".into())
    );
    assert!(T_RELEASED.get());
    assert_eq!(swept_letters(), vec![vec!['T']]);
}

#[test]
fn mounting_on_t_is_refused_while_its_vault_device_is_live_or_the_link_stays() {
    let store = letter_store();
    let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    T_DEVICE_LIVE.set(true);
    assert_eq!(
        broker.select_mount_letter(&store, Some("T"), "container", KING_SID),
        Err(VaultMountReason::EngineDriveLetterUnavailable)
    );
    assert!(
        swept_letters().is_empty(),
        "a live link is never offered for removal"
    );
    let mut broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    broker.stale_letter_release = |_| Ok(HashMap::new());
    assert_eq!(
        broker.select_mount_letter(&store, Some("T"), "container", KING_SID),
        Err(VaultMountReason::EngineDriveLetterUnavailable)
    );
}

#[test]
fn a_mount_on_another_letter_sweeps_every_other_mapped_letter() {
    let store = letter_store();
    let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    let mut record = personal_record();
    record.scope = VaultPresentation::Machine;
    let mut request = personal_request();
    request.presentation = VaultPresentation::Machine;
    let result = broker.mount_unmanaged_recovery_authorized_locked(
        41,
        &store,
        &record,
        &mut request,
        std::ptr::null_mut(),
        7,
        "S-1-5-21-owner",
        (0, 0),
        false,
    );
    assert_eq!(result, Ok(("P:".into(), 12, true, None)));
    assert_eq!(swept_letters(), vec![vec!['R', 'T']]);
    assert!(T_RELEASED.get());
}

#[test]
fn every_dismount_path_sweeps_including_failed_logoff_and_service_start() {
    for path in [
        "dismount_all",
        "dismount_all_failed",
        "session",
        "owner_failed",
        "startup",
    ] {
        let store = letter_store();
        let events = Arc::new(Mutex::new(BrokerEvents::default()));
        events.lock().unwrap().cleanup_fails = path.ends_with("failed");
        let mut broker = servault_broker(events);
        broker.owner_logon_probe = |_| Ok(true);
        broker.private_alias_absent = |_| Ok(false);
        broker.private_alias_cleanup = |_| Ok(false);
        assert!(broker.retain_cleanup_mount(&store, "old", expired_private_mount()));
        match path {
            "dismount_all" | "dismount_all_failed" => {
                let _ = broker.dismount_all(&store);
            }
            "session" => broker.dismount_session(&store, 7),
            "owner_failed" => {
                let result = broker.dismount_personal_for_caller(
                    &store,
                    1,
                    12,
                    std::ptr::null_mut(),
                    8,
                    "S-1-5-21-owner",
                    false,
                );
                assert_ne!(result.state, VaultMountState::Unmounted);
            }
            _ => broker.sweep_dead_letters(),
        }
        assert!(
            swept_letters().iter().any(|letters| letters.contains(&'T')),
            "{path}"
        );
        assert!(T_RELEASED.get(), "{path}");
    }
}

#[test]
fn dismount_removes_the_letter_link_in_the_logon_that_mounted_it() {
    for presentation in [VaultPresentation::PerUser, VaultPresentation::Machine] {
        let store = letter_store();
        let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
        let mut mount = active_mount_for_owner(7, KING_SID);
        mount.presentation = presentation;
        mount.drive_letter = "T:".into();
        mount.authentication_id = Some((0x0ceed27f, 0));
        assert!(broker.retain_cleanup_mount(&store, "king", mount));
        // A logoff/disconnect dismount runs without the owner's token.
        broker.dismount_session(&store, 7);
        assert!(!broker.active.lock().unwrap().contains_key("king"));
        let calls = MOUNTING_ALIASES.with(|calls| calls.borrow().clone());
        if presentation == VaultPresentation::PerUser {
            assert_eq!(calls, vec![("T:".into(), 12, Some((0x0ceed27f, 0)))]);
        } else {
            assert!(
                calls.is_empty(),
                "a machine-wide letter is the engine's to remove"
            );
        }
    }
}
