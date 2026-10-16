// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{test_support::*, *};

#[test]
fn unreadable_external_policy_never_authorizes_restore() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.fail_policy = true;
    assert!(set(&mut fake, false).is_err());
    assert_eq!(fake.enables, 0);
    assert!(!fake.journal.records.is_empty());
    assert_eq!(fake.adapters[0].problem, 22);
}

#[test]
fn pending_intent_commit_failure_does_not_disable_adapter() {
    let mut fake = Fake {
        fail_write_at: 2,
        ..Default::default()
    };
    assert!(set(&mut fake, true).is_err());
    assert_eq!(fake.disables, 0);
    assert!(fake.journal.blocked);
    assert!(fake.journal.records.is_empty());
    assert_eq!(status(&mut fake).unwrap().state, "partial");
}

#[test]
fn failed_ownership_commit_after_disable_preserves_pending_and_refuses_enable() {
    let mut fake = Fake {
        fail_write_at: 3,
        ..Default::default()
    };
    assert!(set(&mut fake, true).is_err());
    assert_eq!(fake.disables, 1);
    assert_eq!(fake.adapters[0].problem, 22);
    assert!(!fake.journal.records[0].confirmed);
    fake.fail_write_at = 0;
    assert_eq!(set(&mut fake, false).unwrap().state, "recovery_required");
    assert_eq!(fake.enables, 0);
}

#[test]
fn failed_cleanup_commit_after_enable_retries_without_enabling_twice() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.fail_write_at = fake.writes + 2;
    assert!(set(&mut fake, false).is_err());
    assert_eq!(fake.enables, 1);
    assert_eq!(fake.adapters[0].problem, 0);
    assert_eq!(fake.journal.records.len(), 1);
    fake.fail_write_at = 0;
    assert_eq!(set(&mut fake, false).unwrap().state, "allowed");
    assert_eq!(fake.enables, 1);
    assert!(fake.journal.records.is_empty());
}
