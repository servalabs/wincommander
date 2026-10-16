// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{test_support::*, *};

#[test]
fn initial_ownership_commit_failure_prevents_policy_mutation() {
    let mut fake = Fake {
        fail_write_at: 1,
        ..Default::default()
    };
    assert!(set(&mut fake, true).is_err());
    assert_eq!(fake.applies, 0);
    assert_eq!(fake.journal, Journal::default());
}
#[test]
fn failed_confirmation_commit_never_adopts_effective_zero_as_ownership() {
    let mut fake = Fake {
        fail_write_at: 2,
        ..Default::default()
    };
    assert!(set(&mut fake, true).is_err());
    assert_eq!(fake.journal.phase, Phase::PendingBlock);
    assert_eq!(fake.observation.effective, Some(0));
    assert_eq!(status(&mut fake).unwrap().state, "pending");
    for blocked in [true, false] {
        assert!(set(&mut fake, blocked)
            .unwrap_err()
            .contains("BT_POLICY_PENDING"));
    }
    assert_eq!(fake.applies, 1);
}
#[test]
fn rejected_provider_keeps_pending_evidence_without_automatic_replay() {
    let mut fake = Fake {
        fail_apply: true,
        ..Default::default()
    };
    assert!(set(&mut fake, true)
        .unwrap_err()
        .contains("BT_POLICY_REJECTED"));
    assert_eq!(status(&mut fake).unwrap().state, "pending");
    status(&mut fake).unwrap();
    assert_eq!(fake.applies, 1);
}
#[test]
fn matching_config_without_effective_confirmation_is_not_blocked() {
    let mut fake = Fake::default();
    let mut after = fake.observation.clone();
    after.configured = Some(0);
    fake.after = Some(after);
    assert!(set(&mut fake, true)
        .unwrap_err()
        .contains("BT_POLICY_UNVERIFIED"));
    assert_eq!(status(&mut fake).unwrap().state, "pending");
}
#[test]
fn unknown_managed_context_read_failure_does_not_write_policy() {
    let mut fake = Fake {
        fail_observe: true,
        ..Default::default()
    };
    assert!(set(&mut fake, true).is_err());
    assert_eq!((fake.writes, fake.applies), (0, 0));
}
#[test]
fn context_changes_after_restart_refuse_restoration_without_enabling() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.observation.context = "b".repeat(64);
    assert_eq!(status(&mut fake).unwrap().state, "externally_managed");
    assert!(set(&mut fake, false).is_err());
    assert_eq!(fake.applies, 1);
}
#[test]
fn externally_changed_owned_value_is_not_reasserted_or_overridden() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.observation.configured = Some(2);
    fake.observation.effective = Some(2);
    assert!(set(&mut fake, false).is_err());
    assert!(set(&mut fake, true).is_err());
    assert_eq!(fake.applies, 1);
}
#[test]
fn failed_release_receipt_commit_keeps_recovery_evidence_after_config_changed() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    fake.fail_write_at = 4;
    assert!(set(&mut fake, false).is_err());
    assert_eq!(fake.observation.configured, Some(2));
    assert_eq!(fake.journal.phase, Phase::PendingAllow);
    assert_eq!(status(&mut fake).unwrap().state, "pending");
    assert!(set(&mut fake, false).is_err());
    assert_eq!(fake.applies, 2);
}
#[test]
fn invalid_version_context_and_policy_values_fail_closed() {
    let mut fake = Fake::default();
    fake.journal.version = 1;
    assert!(set(&mut fake, true).is_err());
    fake.journal = Journal::default();
    fake.observation.configured = Some(1);
    assert!(set(&mut fake, true).is_err());
    fake.observation.configured = None;
    fake.observation.effective = None;
    assert!(set(&mut fake, true).is_err());
    assert_eq!((fake.writes, fake.applies), (0, 0));
}
