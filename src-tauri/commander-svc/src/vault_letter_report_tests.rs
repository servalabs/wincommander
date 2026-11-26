// SPDX-License-Identifier: AGPL-3.0-or-later
// "Check and free" reports and the Secure Storage list for held letters.

fn check_and_free(
    broker: &VaultMountBroker,
    store: &VaultAccessStore,
    names: Option<fn(&str) -> Option<String>>,
) -> serde_json::Value {
    broker
        .cleanup_owned_letters_with(
            store,
            "S-1-5-21-1-2-3-500",
            2,
            (7, 0),
            &[],
            |_| false,
            |_, _| panic!("no mount record exists"),
            |letters| broker.release_stale_letters_locked(letters, Some("S-1-5-21-1-2-3-500")),
            names,
        )
        .unwrap()
}

fn report_for<'a>(result: &'a serde_json::Value, letter: &str) -> &'a serde_json::Value {
    result["letters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|report| report["letter"] == letter)
        .unwrap()
}

#[test]
fn check_and_free_releases_kings_dead_t_link_and_names_him_only_to_administrators() {
    let store = letter_store();
    let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    let result = check_and_free(&broker, &store, Some(|_| Some(r"SERVAULT\King".into())));
    assert_eq!(result["released"], 1);
    let t = report_for(&result, "T");
    assert_eq!(t["category"], "stale_drive_mapping");
    assert_eq!(t["released"], true);
    assert_eq!(t["holder"]["kind"], "vault_mount");
    assert_eq!(t["holder"]["logon_state"], "disconnected");
    assert_eq!(t["holder"]["device_present"], false);
    assert_eq!(t["holder"]["owner_is_caller"], false);
    assert_eq!(t["holder"]["owner"], r"SERVAULT\King");
    let r = report_for(&result, "R");
    assert_eq!(r["released"], false);
    assert_eq!(r["holder"]["kind"], "ram_disk");
    assert_eq!(r["holder"]["scope"], "machine");
    assert!(!result["unavailable_letters"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("T")));

    let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    let result = check_and_free(&broker, &store, None);
    assert!(report_for(&result, "T")["holder"].get("owner").is_none());
}

#[test]
fn check_and_free_keeps_a_live_t_and_names_what_holds_it() {
    let store = letter_store();
    let broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    T_DEVICE_LIVE.set(true);
    let result = check_and_free(&broker, &store, Some(|_| Some(r"SERVAULT\King".into())));
    assert_eq!(result["released"], 0);
    let t = report_for(&result, "T");
    assert_eq!(t["category"], "external_or_unknown_owner");
    assert_eq!(t["released"], false);
    assert_eq!(t["holder"]["kind"], "vault_mount");
    assert_eq!(t["holder"]["device_present"], true);
    assert_eq!(t["holder"]["owner"], r"SERVAULT\King");
    assert!(!T_RELEASED.get());
}

#[test]
fn another_accounts_stale_vault_is_listed_without_its_path_and_cannot_be_dismounted() {
    let store = letter_store();
    let mut broker = servault_broker(Arc::new(Mutex::new(BrokerEvents::default())));
    broker.engine_snapshot = Some(|| Ok(HashMap::new()));
    broker.private_alias_absent = |_| Ok(false);
    let mut mount = expired_private_mount();
    mount.caller_sid = KING_SID.into();
    mount.drive_letter = "T:".into();
    mount.canonical_container_path = Some(r"C:\Users\King\vault.hc".into());
    assert!(broker.retain_cleanup_mount(&store, "king", mount));
    let rows = broker
        .personal_mounts_for_caller(&store, std::ptr::null_mut(), 2, "S-1-5-21-1-2-3-500", false)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].drive_letter, "T:");
    assert!(!rows[0].dismount_allowed && !rows[0].browse_allowed);
    assert_eq!(
        rows[0].dismount_reason,
        Some(VaultMountReason::PrivateOwnerRequired)
    );
    assert_eq!(rows[0].canonical_container_path, None);
}
