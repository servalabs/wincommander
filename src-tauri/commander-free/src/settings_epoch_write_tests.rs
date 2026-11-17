// SPDX-License-Identifier: AGPL-3.0-or-later
use super::super::{create_default_settings, GLOBAL_STATE_TEST_LOCK, SETTINGS_CACHE};
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::cell::Cell;

struct Fixture {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<AppSettings>,
    key: SigningKey,
}
impl Fixture {
    fn new() -> Self {
        let lock = GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let key = SigningKey::from_bytes(&[7; 32]); // Public deterministic test material only.
        let mut settings = create_default_settings();
        settings.policy.fleet_signing_key = Some(STANDARD.encode(key.verifying_key().to_bytes()));
        settings.policy.master_config_version = Some(10);
        settings.policy.managed = true;
        settings.app.fleet.enabled = true;
        settings.app.fleet.server_url = "https://fleet.example.invalid".into();
        let previous = SETTINGS_CACHE.lock().unwrap().replace(settings);
        Self {
            _lock: lock,
            previous,
            key,
        }
    }
    fn epoch(&self, version: u32, strategy: &str) -> Epoch {
        let config = json!({"privacy":{"telemetry":{"windowsDisabled":true}}});
        let locks = vec!["privacy.telemetry".into()];
        let preimage =
            wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
                version: i64::from(version),
                config: &config,
                locked_paths: &locks,
                managed: true,
                target_kind: "org",
                target_id: None,
            });
        Epoch {
            config,
            locked_paths: locks,
            strategy: strategy.into(),
            version,
            signature: Some(STANDARD.encode(self.key.sign(&preimage).to_bytes())),
            signer_key: Some(STANDARD.encode(self.key.verifying_key().to_bytes())),
            target_kind: Some("org".into()),
            target_id: None,
            managed: true,
            generation: None,
            floor_reset: false,
            keep_transaction_hash: None,
        }
    }
    fn value(&self) -> Value {
        serde_json::to_value(SETTINGS_CACHE.lock().unwrap().as_ref().unwrap()).unwrap()
    }
    fn deny(&self, epoch: &Epoch) {
        let before = self.value();
        let writes = Cell::new(0);
        let result = apply_with(epoch, |_| {
            writes.set(writes.get() + 1);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(writes.get(), 0);
        assert_eq!(self.value(), before);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        *SETTINGS_CACHE.lock().unwrap() = self.previous.take();
    }
}

#[test]
fn signed_merge_commits_configuration_locks_and_version_together() {
    let f = Fixture::new();
    let epoch = f.epoch(11, "merge");
    let result = apply_with(&epoch, |settings| {
        assert_eq!(settings.policy.master_config_version, Some(11));
        assert_eq!(settings.policy.locked_paths, vec!["privacy.telemetry"]);
        assert_eq!(
            serde_json::to_value(settings).unwrap()["ideal"]["privacy"]["telemetry"]
                ["windowsDisabled"],
            true
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(result.policy.master_config_version, Some(11));
}

#[test]
fn recovery_equal_epoch_readback_verifies_signed_desired_settings_only() {
    let f=Fixture::new(); let epoch=f.epoch(11,"merge");
    let current=SETTINGS_CACHE.lock().unwrap().as_ref().unwrap().clone();
    let applied=prepare(&current,&epoch).unwrap();
    assert!(readback_equal(&applied,&epoch).unwrap());
    let mut altered=serde_json::to_value(&applied).unwrap();
    altered["ideal"]["privacy"]["telemetry"]["windowsDisabled"]=json!(false);
    let altered:AppSettings=serde_json::from_value(altered).unwrap();
    assert!(!readback_equal(&altered,&epoch).unwrap());
    let mut forged=f.epoch(11,"merge"); forged.config["privacy"]["telemetry"]["windowsDisabled"]=json!(false);
    assert!(readback_equal(&applied,&forged).is_err());
    assert!(readback_equal(&current,&epoch).is_err());
    assert!(prepare(&applied,&epoch).is_err(), "ordinary equal-version writes stay blocked");
}

#[test]
fn fleet_epoch_cannot_enable_or_disable_account_incident_media_delivery() {
    let f = Fixture::new();
    let signed = |version: u32, strategy: &str, remote: bool| {
        let mut epoch = f.epoch(version, strategy);
        epoch.config = json!({"privacy":{"privacyShield":{"sendIncidentCapturesToFleet":remote}}});
        let preimage = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
            version: i64::from(epoch.version), config: &epoch.config,
            locked_paths: &epoch.locked_paths, managed: epoch.managed,
            target_kind: epoch.target_kind.as_deref().unwrap_or("org"),
            target_id: epoch.target_id.as_deref(),
        });
        epoch.signature = Some(STANDARD.encode(f.key.sign(&preimage).to_bytes()));
        epoch
    };
    let result = apply_with(&signed(11, "merge", true), |_| Ok(())).unwrap();
    assert!(!result.ideal.privacy.privacy_shield.send_incident_captures_to_fleet);
    SETTINGS_CACHE.lock().unwrap().as_mut().unwrap().ideal.privacy.privacy_shield
        .send_incident_captures_to_fleet = true;
    let result = apply_with(&signed(12, "overwrite", false), |_| Ok(())).unwrap();
    assert!(result.ideal.privacy.privacy_shield.send_incident_captures_to_fleet);
}

#[test]
fn signed_look_away_delay_is_preserved_and_out_of_range_is_rejected_before_write() {
    let fixture = Fixture::new();
    let mut current = SETTINGS_CACHE.lock().unwrap().as_ref().unwrap().clone();
    current.app.local_privacy_shield_look_away_delay_seconds = Some(7);
    for (value, expected) in [(json!(0), Some(0)), (json!(10), Some(10)), (json!(300), Some(300)), (json!(301), None), (json!(null), None), (json!("10"), None)] {
        let mut epoch = fixture.epoch(11, "merge");
        epoch.config = json!({"privacy":{"privacyShield":{"gazeDetectionEnabled":false,"lookAwayDelaySeconds":value}}});
        let preimage = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
            version: i64::from(epoch.version), config: &epoch.config, locked_paths: &epoch.locked_paths,
            managed: epoch.managed, target_kind: "org", target_id: None,
        });
        epoch.signature = Some(STANDARD.encode(fixture.key.sign(&preimage).to_bytes()));
        match expected {
            Some(seconds) => {
                let candidate = prepare(&current, &epoch).unwrap();
                assert_eq!(candidate.ideal.privacy.privacy_shield.look_away_delay_seconds, Some(seconds));
                assert_eq!(candidate.ideal.privacy.privacy_shield.gaze_detection_enabled, Some(false));
                assert_eq!(candidate.app.local_privacy_shield_look_away_delay_seconds, Some(7));
            }
            None => assert!(prepare(&current, &epoch).is_err()),
        }
    }
    current.ideal.privacy.privacy_shield.look_away_delay_seconds = Some(25);
    current.ideal.privacy.privacy_shield.fleet_capture_on_phone = Some(true);
    current.ideal.privacy.privacy_shield.fleet_capture_on_multi_face = Some(true);
    let mut omitted = fixture.epoch(11, "merge");
    omitted.config = json!({"privacy":{"privacyShield":{"gazeDetectionEnabled":false}}});
    let preimage = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
        version: i64::from(omitted.version), config: &omitted.config, locked_paths: &omitted.locked_paths,
        managed: omitted.managed, target_kind: "org", target_id: None,
    });
    omitted.signature = Some(STANDARD.encode(fixture.key.sign(&preimage).to_bytes()));
    let candidate = prepare(&current, &omitted).unwrap();
    assert_eq!(candidate.ideal.privacy.privacy_shield.look_away_delay_seconds, None);
    assert_eq!(candidate.ideal.privacy.privacy_shield.fleet_capture_on_phone, None);
    assert_eq!(candidate.ideal.privacy.privacy_shield.fleet_capture_on_multi_face, None);
    assert_eq!(candidate.app.local_privacy_shield_look_away_delay_seconds, Some(7));
    let mut overwritten = fixture.epoch(11, "overwrite");
    overwritten.config = json!({"privacy":{"privacyShield":{"lookAwayDelaySeconds":10}}});
    let preimage = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
        version: i64::from(overwritten.version), config: &overwritten.config, locked_paths: &overwritten.locked_paths,
        managed: overwritten.managed, target_kind: "org", target_id: None,
    });
    overwritten.signature = Some(STANDARD.encode(fixture.key.sign(&preimage).to_bytes()));
    assert_eq!(prepare(&current, &overwritten).unwrap().app.local_privacy_shield_look_away_delay_seconds, Some(7));
    overwritten.config = json!({"privacy":{"privacyShield":{"localLookAwayDelaySeconds":55}}});
    let preimage = wincmd_shared::fleet::epoch_preimage(&wincmd_shared::fleet::EpochSigningInput {
        version: i64::from(overwritten.version), config: &overwritten.config, locked_paths: &overwritten.locked_paths,
        managed: overwritten.managed, target_kind: "org", target_id: None,
    });
    overwritten.signature = Some(STANDARD.encode(fixture.key.sign(&preimage).to_bytes()));
    assert!(prepare(&current, &overwritten).is_err());
}

#[test]
fn overwrite_preserves_verification_key_connection_and_device_identity() {
    let f = Fixture::new();
    let before = f.value();
    let result = apply_with(&f.epoch(11, "overwrite"), |_| Ok(())).unwrap();
    let after = serde_json::to_value(result).unwrap();
    for path in [
        "/policy/fleetSigningKey",
        "/app/fleet",
        "/deviceId",
        "/createdAt",
    ] {
        assert_eq!(before.pointer(path), after.pointer(path));
    }
    let mut unsigned = f.epoch(12, "merge");
    unsigned.signature = None;
    f.deny(&unsigned);
}

#[test]
fn valid_signatures_do_not_permit_replay_or_rollback() {
    let f = Fixture::new();
    f.deny(&f.epoch(9, "merge"));
    f.deny(&f.epoch(10, "merge"));
    apply_with(&f.epoch(11, "merge"), |_| Ok(())).unwrap();
    f.deny(&f.epoch(11, "merge"));
}

#[test]
fn missing_key_signature_wrong_key_and_tampering_never_reach_persistence() {
    let f = Fixture::new();
    let mut unsigned = f.epoch(11, "merge");
    unsigned.signature = None;
    f.deny(&unsigned);
    let mut swapped = f.epoch(11, "merge");
    swapped.signer_key = Some("other".into());
    f.deny(&swapped);
    let mut changed = f.epoch(11, "merge");
    changed.managed = false;
    f.deny(&changed);
    SETTINGS_CACHE
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .policy
        .fleet_signing_key = None;
    f.deny(&f.epoch(11, "merge"));
}

#[test]
fn persistence_failure_invalidates_uncertain_policy_and_an_invalid_strategy_is_denied() {
    let f = Fixture::new();
    let before = f.value();
    assert!(apply_with(&f.epoch(11, "merge"), |_| Err("injected I/O error".into())).is_err());
    assert!(SETTINGS_CACHE.lock().unwrap().is_none());
    // Model an authoritative reload without touching the operator's real settings.
    *SETTINGS_CACHE.lock().unwrap() = Some(serde_json::from_value(before).unwrap());
    f.deny(&f.epoch(11, "typo"));
}

#[test]
fn concurrent_identical_epochs_commit_at_most_once() {
    let f = Fixture::new();
    let epoch = f.epoch(11, "merge");
    let writes = std::sync::atomic::AtomicUsize::new(0);
    let successes = std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    apply_with(&epoch, |_| {
                        writes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    })
                    .is_ok()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(successes, 1);
    assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}

const GENERATION_OLD: &str = "df372735-0000-4000-8000-000000000001";
const GENERATION_NEW: &str = "98455ed7-0000-4000-8000-000000000002";

fn in_generation(f: &Fixture, version: u32, generation: &str, floor_reset: bool) -> Epoch {
    let mut epoch = f.epoch(version, "merge");
    epoch.generation = Some(generation.into());
    epoch.floor_reset = floor_reset;
    epoch
}

fn record_floor(generation: Option<&str>) {
    SETTINGS_CACHE.lock().unwrap().as_mut().unwrap().policy.master_config_generation =
        generation.map(str::to_owned);
}

#[test]
fn a_lower_epoch_inside_the_same_generation_is_always_refused() {
    let f = Fixture::new();
    record_floor(Some(GENERATION_OLD));
    // Floor 10 under the old generation: replaying older epochs of that very
    // generation is a rollback, with or without the reset flag.
    f.deny(&in_generation(&f, 3, GENERATION_OLD, false));
    f.deny(&in_generation(&f, 3, GENERATION_OLD, true));
    f.deny(&in_generation(&f, 10, GENERATION_OLD, true));
}

#[test]
fn a_confirmed_new_generation_restarts_below_the_old_floor_then_floor_is_monotonic() {
    let f = Fixture::new();
    record_floor(Some(GENERATION_OLD));
    let adopted = apply_with(&in_generation(&f, 1, GENERATION_NEW, true), |_| Ok(())).unwrap();
    assert_eq!(adopted.policy.master_config_version, Some(1));
    assert_eq!(adopted.policy.master_config_generation.as_deref(), Some(GENERATION_NEW));
    *SETTINGS_CACHE.lock().unwrap() = Some(adopted);
    // The floor now belongs to the new generation and only moves up.
    f.deny(&in_generation(&f, 1, GENERATION_NEW, true));
    f.deny(&in_generation(&f, 1, GENERATION_NEW, false));
    let next = apply_with(&in_generation(&f, 2, GENERATION_NEW, false), |_| Ok(())).unwrap();
    assert_eq!(next.policy.master_config_version, Some(2));
    *SETTINGS_CACHE.lock().unwrap() = Some(next);
    f.deny(&in_generation(&f, 1, GENERATION_NEW, true));
    // The old generation's epochs are now the other generation: still below
    // its own recorded floor of 2 only if the reset is proven again.
    f.deny(&in_generation(&f, 1, GENERATION_OLD, false));
}

#[test]
fn an_unconfirmed_or_unknown_generation_cannot_go_below_the_floor() {
    let f = Fixture::new();
    record_floor(Some(GENERATION_OLD));
    // A different generation the service has not confirmed (no reset flag).
    f.deny(&in_generation(&f, 1, GENERATION_NEW, false));
    // No generation at all (an unbound epoch) never resets, even if flagged.
    let mut unbound = f.epoch(1, "merge");
    unbound.floor_reset = true;
    f.deny(&unbound);
    // A newer number in the new generation needs no reset.
    let newer = apply_with(&in_generation(&f, 11, GENERATION_NEW, false), |_| Ok(())).unwrap();
    assert_eq!(newer.policy.master_config_generation.as_deref(), Some(GENERATION_NEW));
}

#[test]
fn a_floor_recorded_before_generations_were_tracked_can_be_reset_only_when_confirmed() {
    let f = Fixture::new();
    record_floor(None);
    f.deny(&in_generation(&f, 1, GENERATION_NEW, false));
    let adopted = apply_with(&in_generation(&f, 1, GENERATION_NEW, true), |_| Ok(())).unwrap();
    assert_eq!(adopted.policy.master_config_generation.as_deref(), Some(GENERATION_NEW));
    // Epoch 0 is never valid, even with a proven new generation.
    record_floor(Some(GENERATION_OLD));
    f.deny(&in_generation(&f, 0, GENERATION_NEW, true));
}

fn transaction_for(epoch_hash: &str) -> super::super::ShieldPolicyTransaction {
    serde_json::from_value(json!({
        "epochVersion": 10, "epochSignatureHash": epoch_hash, "resolutionRevision": 3,
        "shieldRevision": 24, "attemptId": "attempt", "wasRunning": true,
        "wasFleetOwned": true, "expectedHash": "tuning", "prePauseMode": "balanced",
    }))
    .unwrap()
}

#[test]
fn a_floor_reset_voids_the_old_shield_transaction_in_the_same_write_but_keeps_this_epochs() {
    let f = Fixture::new();
    record_floor(Some(GENERATION_OLD));
    SETTINGS_CACHE.lock().unwrap().as_mut().unwrap().app.fleet.privacy_shield_policy_transaction =
        Some(transaction_for("old-generation-epoch"));
    let mut reset = in_generation(&f, 1, GENERATION_NEW, true);
    reset.keep_transaction_hash = Some("new-generation-epoch".into());
    let adopted = apply_with(&reset, |settings| {
        // The persisted candidate already has the stale transaction gone.
        assert!(settings.app.fleet.privacy_shield_policy_transaction.is_none());
        assert_eq!(settings.policy.master_config_version, Some(1));
        Ok(())
    })
    .unwrap();
    assert!(adopted.app.fleet.privacy_shield_policy_transaction.is_none());

    // A transaction for the very epoch being adopted carries crash-recovery
    // data (it was running and paused): it survives the same reset.
    record_floor(Some(GENERATION_OLD));
    {
        let mut cache = SETTINGS_CACHE.lock().unwrap();
        let settings = cache.as_mut().unwrap();
        settings.policy.master_config_version = Some(10);
        settings.app.fleet.privacy_shield_policy_transaction =
            Some(transaction_for("new-generation-epoch"));
    }
    let mut reset = in_generation(&f, 1, GENERATION_NEW, true);
    reset.keep_transaction_hash = Some("new-generation-epoch".into());
    let kept = apply_with(&reset, |_| Ok(())).unwrap();
    let transaction = kept.app.fleet.privacy_shield_policy_transaction.unwrap();
    assert!(transaction.was_running);
    assert_eq!(transaction.pre_pause_mode.as_deref(), Some("balanced"));
}

#[test]
fn an_ordinary_epoch_never_touches_a_pending_shield_transaction() {
    let f = Fixture::new();
    record_floor(Some(GENERATION_OLD));
    SETTINGS_CACHE.lock().unwrap().as_mut().unwrap().app.fleet.privacy_shield_policy_transaction =
        Some(transaction_for("in-flight"));
    let next = apply_with(&in_generation(&f, 11, GENERATION_OLD, false), |_| Ok(())).unwrap();
    assert!(next.app.fleet.privacy_shield_policy_transaction.is_some());
}
