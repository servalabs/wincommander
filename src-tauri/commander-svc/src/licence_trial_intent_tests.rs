// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn paths() -> (String, String) {
    let id = uuid::Uuid::new_v4();
    (format!(r"SOFTWARE\WinCommanderTrialIntentTest-{id}"), format!(r"SOFTWARE\WinCommanderTrialGateTest-{id}"))
}

#[test]
fn trial_credential_commitment_matches_activation_vector() {
    let pending = PendingTrial { operation_id: "b".repeat(64), device_hash: "a".repeat(64),
        credential_id: [4; 32], credential_secret: [5; 32] };
    assert_eq!(pending.credential_commitment(), "fdef1206908c31b0f3b9ecce79bfb437e32deabe702d7f6fbf529b14f44a136f");
}

#[test]
fn protected_trial_intent_is_stable_and_fails_closed() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path.clone());
    let gate = Anchor::test_user(gate_path.clone());
    let device = "a".repeat(64);
    assert!(ensure_idle_at(&store, &MachineDpapi).is_ok());
    assert!(begin_at(&store, &gate, &MachineDpapi, &device, || Err(INVALID)).is_err());
    assert!(!store.exists().unwrap());
    let first = begin_at(&store, &gate, &MachineDpapi, &device, || Ok(())).unwrap();
    assert!(ensure_idle_at(&store, &MachineDpapi).is_err());
    let bytes = store.read().unwrap().unwrap();
    assert!(!bytes.windows(32).any(|part| part == first.credential_secret));
    let reopened = Anchor::test_user(path.clone());
    let second = begin_at(&reopened, &gate, &MachineDpapi, &device, || Ok(())).unwrap();
    assert_eq!(first.operation_id, second.operation_id);
    assert_eq!(first.credential_id, second.credential_id);
    assert_eq!(first.credential_secret, second.credential_secret);
    assert!(begin_at(&reopened, &gate, &MachineDpapi, &"b".repeat(64), || Ok(())).is_err());
    assert_eq!(store.read().unwrap().unwrap(), bytes);
    reopened.write(b"corrupt").unwrap();
    assert!(ensure_idle_at(&reopened, &MachineDpapi).is_err());
    assert!(begin_at(&reopened, &gate, &MachineDpapi, &device, || Ok(())).is_err());
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
}

#[test]
fn trial_intent_does_not_begin_during_release() {
    let (path, gate_path) = paths();
    let store = Anchor::test_user(path);
    let gate = Anchor::test_user(gate_path.clone());
    release_gate::begin_at(&gate, &[3; 32]).unwrap();
    assert!(begin_at(&store, &gate, &MachineDpapi, &"a".repeat(64), || Ok(())).is_err());
    assert!(!store.exists().unwrap());
    super::super::license_cache::licence_device_anchor::remove_test_key(&gate_path).unwrap();
}

#[test]
fn trial_retry_after_process_restart_uses_the_original_intent() {
    const STORE: &str = "WINCMD_TEST_TRIAL_INTENT_PATH";
    const GATE: &str = "WINCMD_TEST_TRIAL_GATE_PATH";
    const EXPECTED: &str = "WINCMD_TEST_TRIAL_OPERATION";
    if let (Ok(path), Ok(gate), Ok(expected)) = (std::env::var(STORE), std::env::var(GATE), std::env::var(EXPECTED)) {
        let pending = begin_at(&Anchor::test_user(path), &Anchor::test_user(gate), &MachineDpapi, &"a".repeat(64), || Ok(())).unwrap();
        assert_eq!(pending.operation_id, expected);
        return;
    }
    let (path, gate_path) = paths();
    let pending = begin_at(&Anchor::test_user(path.clone()), &Anchor::test_user(gate_path.clone()),
        &MachineDpapi, &"a".repeat(64), || Ok(())).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "licence_trial_intent::tests::trial_retry_after_process_restart_uses_the_original_intent"])
        .env(STORE, &path).env(GATE, &gate_path).env(EXPECTED, &pending.operation_id).output().unwrap();
    super::super::license_cache::licence_device_anchor::remove_test_key(&path).unwrap();
    assert!(child.status.success());
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}
