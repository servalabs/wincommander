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
fn failure_does_not_advance_cached_policy_and_an_invalid_strategy_is_denied() {
    let f = Fixture::new();
    let before = f.value();
    assert!(apply_with(&f.epoch(11, "merge"), |_| Err("injected I/O error".into())).is_err());
    assert_eq!(f.value(), before);
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
