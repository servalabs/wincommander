// SPDX-License-Identifier: AGPL-3.0-or-later
use super::test_support::*;
use super::*;

#[test]
fn unauthorized_machine_changes_do_not_write_or_touch_devices() {
    let mut fake = Fake {
        admin: false,
        ..Default::default()
    };
    assert!(set(&mut fake, true).is_err());
    assert!(set(&mut fake, false).is_err());
    assert_eq!((fake.writes, fake.disables, fake.enables), (0, 0, 0));
}

#[test]
fn block_and_allow_survive_a_new_controller_without_losing_ownership() {
    let mut fake = Fake::default();
    assert_eq!(set(&mut fake, true).unwrap().state, "blocked");
    assert!(fake.journal.records[0].confirmed);
    let mut restarted = Fake {
        journal: fake.journal.clone(),
        adapters: fake.adapters.clone(),
        ..Default::default()
    };
    assert_eq!(status(&mut restarted).unwrap().state, "blocked");
    assert_eq!(set(&mut restarted, false).unwrap().state, "allowed");
    assert_eq!(restarted.enables, 1);
    assert!(restarted.journal.records.is_empty());
}

#[test]
fn predisabled_or_faulted_external_adapters_are_never_claimed_or_enabled() {
    let mut fake = Fake {
        adapters: vec![adapter("EXTERNAL", 22), adapter("FAULT", 10)],
        ..Default::default()
    };
    set(&mut fake, true).unwrap();
    set(&mut fake, false).unwrap();
    assert!(fake.journal.records.is_empty());
    assert_eq!((fake.disables, fake.enables), (0, 0));
    assert_eq!(status(&mut fake).unwrap().state, "externally_blocked");
}

#[test]
fn journal_commit_failure_prevents_device_mutations() {
    let mut fake = Fake {
        fail_write: true,
        ..Default::default()
    };
    assert!(set(&mut fake, true).is_err());
    assert_eq!((fake.disables, fake.enables), (0, 0));
}

#[test]
fn failed_disable_retains_pending_evidence_and_never_authorizes_restore() {
    let mut fake = Fake {
        fail_disable: true,
        ..Default::default()
    };
    assert_eq!(set(&mut fake, true).unwrap_err(), "disable failed");
    assert_eq!(status(&mut fake).unwrap().state, "recovery_required");
    assert!(!fake.journal.records[0].confirmed);
    set(&mut fake, false).unwrap();
    assert_eq!(fake.enables, 0);
    assert!(fake.journal.records.is_empty());
}

#[test]
fn interrupted_pending_disables_cannot_be_upgraded_to_confirmed_ownership() {
    let mut fake = Fake::default();
    fake.journal.blocked = true;
    fake.journal.records.push(Record {
        id: "RADIO1".into(),
        identity: "RADIO1-hardware".into(),
        confirmed: false,
    });
    fake.adapters[0].problem = 22;
    assert_eq!(reconcile(&mut fake).unwrap().state, "recovery_required");
    set(&mut fake, false).unwrap();
    assert_eq!(fake.enables, 0);
}

#[test]
fn changed_driver_or_hardware_identity_refuses_restore() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.adapters[0].identity = "replacement-hardware".into();
    assert_eq!(set(&mut fake, false).unwrap().state, "recovery_required");
    assert_eq!(fake.enables, 0);
}

#[test]
fn external_policy_prevents_all_restoration_and_is_not_modified() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.external = true;
    assert!(set(&mut fake, false).is_err());
    assert_eq!(fake.enables, 0);
    assert!(!fake.journal.records.is_empty());
    assert!(fake.external);
}

#[test]
fn new_hotplug_adapter_is_disabled_only_when_saved_intent_is_active() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.adapters.push(adapter("NEW", 0));
    assert_eq!(reconcile(&mut fake).unwrap().state, "blocked");
    assert_eq!(fake.disables, 2);
    set(&mut fake, false).unwrap();
    fake.adapters.push(adapter("ALLOWED", 0));
    reconcile(&mut fake).unwrap();
    assert_eq!(fake.disables, 2);
}

#[test]
fn failed_enable_reports_recovery_instead_of_claiming_bluetooth_allowed() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.fail_enable = true;
    assert_eq!(set(&mut fake, false).unwrap_err(), "enable failed");
    assert_eq!(status(&mut fake).unwrap().state, "recovery_required");
    assert!(!fake.journal.records.is_empty());
}

#[test]
fn removed_owned_adapter_keeps_restore_record_until_it_returns() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    let removed = fake.adapters.remove(0);
    assert_eq!(set(&mut fake, false).unwrap().state, "recovery_required");
    fake.adapters.push(removed);
    assert_eq!(set(&mut fake, false).unwrap().state, "allowed");
    assert_eq!(fake.enables, 1);
}

#[test]
fn no_hardware_can_save_machine_intent_without_claiming_an_adapter() {
    let mut fake = Fake {
        adapters: vec![],
        ..Default::default()
    };
    assert_eq!(status(&mut fake).unwrap().state, "no_adapter");
    assert_eq!(set(&mut fake, true).unwrap().state, "no_adapter");
    assert!(fake.journal.records.is_empty());
    assert_eq!(set(&mut fake, false).unwrap().state, "no_adapter");
}

#[test]
fn duplicate_or_unknown_version_records_fail_closed_before_mutation() {
    let mut fake = Fake::default();
    fake.journal.version = 2;
    assert!(set(&mut fake, true).is_err());
    fake.journal.version = 1;
    fake.journal.records = vec![
        Record {
            id: "RADIO1".into(),
            identity: "hardware".into(),
            confirmed: true
        };
        2
    ];
    assert!(set(&mut fake, false).is_err());
    assert_eq!((fake.disables, fake.enables), (0, 0));
}

#[test]
fn already_enabled_owned_adapter_can_release_record_without_enabling_again() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.adapters[0].problem = 0;
    assert_eq!(set(&mut fake, false).unwrap().state, "allowed");
    assert_eq!(fake.enables, 0);
}

#[test]
fn manually_enabled_pending_adapter_can_be_released_without_mutation() {
    let mut fake = Fake::default();
    fake.journal.records.push(Record {
        id: "RADIO1".into(),
        identity: "RADIO1-hardware".into(),
        confirmed: false,
    });
    assert_eq!(set(&mut fake, false).unwrap().state, "allowed");
    assert_eq!((fake.enables, fake.disables), (0, 0));
    assert!(fake.journal.records.is_empty());
}

#[test]
fn enabled_pending_adapter_requires_explicit_retry_before_fresh_disable_ownership() {
    let mut fake = Fake::default();
    fake.journal.blocked = true;
    fake.journal.records.push(Record {
        id: "RADIO1".into(),
        identity: "RADIO1-hardware".into(),
        confirmed: false,
    });
    assert_eq!(reconcile(&mut fake).unwrap().state, "recovery_required");
    assert_eq!(fake.disables, 0);
    assert!(!fake.journal.records[0].confirmed);
    assert_eq!(set(&mut fake, true).unwrap().state, "blocked");
    assert!(fake.journal.records[0].confirmed);
    assert_eq!(fake.disables, 1);
}
