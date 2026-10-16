// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{test_support::*, *};

#[test]
fn status_observation_never_writes_or_applies() {
    let mut fake = Fake::default();
    assert_eq!(status(&mut fake).unwrap().state, "allowed");
    assert_eq!((fake.writes, fake.applies), (0, 0));
}
#[test]
fn schema_v2_has_no_adapter_or_monitoring_claims() {
    let value = serde_json::to_value(status(&mut Fake::default()).unwrap()).unwrap();
    assert_eq!(value["schemaVersion"], 2);
    assert_eq!(value.as_object().unwrap().len(), 5);
    assert!(value.get("adapterCount").is_none());
    assert!(value.get("monitoring").is_none());
}
#[test]
fn confirmed_policy_block_and_neutral_allow_keep_paired_devices_out_of_contract() {
    let mut fake = Fake::default();
    assert_eq!(set(&mut fake, true).unwrap().state, "blocked");
    assert_eq!(fake.journal.phase, Phase::Confirmed);
    assert_eq!(fake.journal.baseline, None);
    assert_eq!(set(&mut fake, false).unwrap().state, "allowed");
    assert_eq!(fake.observation.configured, Some(2));
    assert_eq!(fake.journal, Journal::default());
    assert_eq!(fake.applies, 2);
}
#[test]
fn already_allowed_explicit_baseline_is_recorded_and_restored_neutrally() {
    let mut fake = Fake::default();
    fake.observation.configured = Some(2);
    set(&mut fake, true).unwrap();
    assert_eq!(fake.journal.baseline, Some(2));
    assert_eq!(set(&mut fake, false).unwrap().state, "allowed");
}
#[test]
fn confirmed_idempotent_requests_do_not_rewrite_policy() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    assert_eq!(set(&mut fake, true).unwrap().state, "blocked");
    assert_eq!((fake.writes, fake.applies), (2, 1));
    set(&mut fake, false).unwrap();
    set(&mut fake, false).unwrap();
    assert_eq!((fake.writes, fake.applies), (4, 2));
}
#[test]
fn unsupported_server_home_or_old_windows_does_not_save_intent() {
    let mut fake = Fake::default();
    fake.observation.supported = false;
    assert_eq!(status(&mut fake).unwrap().state, "unsupported");
    assert!(set(&mut fake, true)
        .unwrap_err()
        .contains("BT_POLICY_UNSUPPORTED"));
    assert_eq!((fake.writes, fake.applies), (0, 0));
}
#[test]
fn legacy_receipt_is_reported_before_unsupported_provider_and_preserved() {
    let mut fake = Fake {
        legacy: true,
        ..Default::default()
    };
    fake.observation.supported = false;
    assert_eq!(status(&mut fake).unwrap().state, "legacy_recovery_required");
    assert!(set(&mut fake, false)
        .unwrap_err()
        .contains("BT_LEGACY_RECOVERY"));
    assert_eq!((fake.writes, fake.applies, fake.observes), (0, 0, 0));
    assert!(fake.legacy);
}
#[test]
fn ordinary_user_can_observe_but_cannot_mutate_policy() {
    let mut fake = Fake {
        admin: false,
        ..Default::default()
    };
    assert!(!status(&mut fake).unwrap().can_change);
    assert!(set(&mut fake, true)
        .unwrap_err()
        .contains("BT_ACCESS_DENIED"));
    assert_eq!((fake.writes, fake.applies), (0, 0));
}
#[test]
fn domain_mdm_or_azure_management_refuses_block_and_allow() {
    let mut fake = Fake::default();
    fake.observation.managed = true;
    assert_eq!(status(&mut fake).unwrap().state, "externally_managed");
    for blocked in [true, false] {
        assert!(set(&mut fake, blocked)
            .unwrap_err()
            .contains("BT_POLICY_MANAGED"));
    }
    assert_eq!((fake.writes, fake.applies), (0, 0));
}
#[test]
fn unowned_configured_or_effective_block_cannot_be_released_or_claimed() {
    for (configured, effective) in [(Some(0), Some(0)), (None, Some(0)), (Some(0), Some(2))] {
        let mut fake = Fake::default();
        fake.observation.configured = configured;
        fake.observation.effective = effective;
        assert_eq!(status(&mut fake).unwrap().state, "externally_managed");
        for blocked in [true, false] {
            assert!(set(&mut fake, blocked).is_err());
        }
        assert_eq!((fake.writes, fake.applies), (0, 0));
    }
}
#[test]
fn external_result_block_after_neutral_release_is_not_successful_allow() {
    let mut fake = Fake::default();
    set(&mut fake, true).unwrap();
    let mut after = fake.observation.clone();
    after.configured = Some(2);
    after.effective = Some(0);
    fake.after = Some(after);
    let result = set(&mut fake, false).unwrap();
    assert_eq!(result.state, "externally_managed");
    assert!(!result.blocked && !result.can_change);
    assert_eq!(fake.journal, Journal::default());
}
