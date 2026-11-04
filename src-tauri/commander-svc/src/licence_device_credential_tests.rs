// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn disposable_registry_credential_is_encrypted_bound_and_not_replaced() {
    let path = format!(
        r"SOFTWARE\WinCommanderLicenceDeviceTest-{}",
        uuid::Uuid::new_v4()
    );
    let anchor = anchor::Anchor::test_user(path.clone());
    let binding = Binding {
        licence_id: "test-licence",
        device_hash: "test-device",
        issuer_origin: "https://license.example",
    };
    let credential = get_or_create_with(&anchor, &MachineDpapi, &binding).unwrap();
    let second = PendingCredential::generate().unwrap();
    assert_ne!(credential.id, second.id);
    assert_ne!(credential.secret(), second.secret());
    let original = anchor.read().unwrap().unwrap();
    let record: Record = serde_json::from_slice(&original).unwrap();
    assert_eq!(record.phase, "pending");
    assert_ne!(record.sealed, credential.secret().as_slice());
    let reopened = anchor::Anchor::test_user(path.clone());
    let loaded = get_or_create_with(&reopened, &MachineDpapi, &binding).unwrap();
    assert_eq!(loaded.id, credential.id);
    assert_eq!(loaded.secret(), credential.secret());
    let challenge = wincmd_pq_experiment::licence_device_proof::Challenge {
        operation: wincmd_pq_experiment::licence_device_proof::Operation::Refresh,
        credential_id: &credential.id,
        nonce: &[7; 32],
        request_digest: &[9; 32],
        expires_at: 2_000_000_000,
    };
    assert_eq!(
        wincmd_pq_experiment::licence_device_proof::sign(credential.secret(), &challenge),
        wincmd_pq_experiment::licence_device_proof::sign(loaded.secret(), &challenge),
    );
    assert!(save_locked(&anchor, &MachineDpapi, &binding, &second).is_err());
    assert_eq!(anchor.read().unwrap().unwrap(), original);
    for changed in [
        Binding {
            licence_id: "other",
            ..binding
        },
        Binding {
            device_hash: "other",
            ..binding
        },
        Binding {
            issuer_origin: "https://other.example",
            ..binding
        },
    ] {
        assert!(get_or_create_with(&anchor, &MachineDpapi, &changed).is_err());
    }
    let mut altered: Record = serde_json::from_slice(&original).unwrap();
    altered.id[0] ^= 1;
    anchor
        .write(&serde_json::to_vec(&altered).unwrap())
        .unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    assert!(save_locked(&anchor, &MachineDpapi, &binding, &second).is_err());
    altered.id = credential.id;
    let index = altered.sealed.len() / 2;
    altered.sealed[index] ^= 1;
    anchor
        .write(&serde_json::to_vec(&altered).unwrap())
        .unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    let mut false_confirmation: serde_json::Value = serde_json::from_slice(&original).unwrap();
    false_confirmation["phase"] = "confirmed".into();
    anchor
        .write(&serde_json::to_vec(&false_confirmation).unwrap())
        .unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    anchor.write(b"not a credential").unwrap();
    assert!(get_or_create_with(&anchor, &MachineDpapi, &binding).is_err());
    assert!(save_locked(&anchor, &MachineDpapi, &binding, &second).is_err());
    anchor::remove_test_key(&path).unwrap();
}

#[test]
fn concurrent_retries_reuse_one_pending_credential() {
    let path = format!(
        r"SOFTWARE\WinCommanderLicenceDeviceRace-{}",
        uuid::Uuid::new_v4()
    );
    let ids = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let anchor = anchor::Anchor::test_user(path.clone());
            let binding = Binding {
                licence_id: "licence",
                device_hash: "device",
                issuer_origin: "https://license.example",
            };
            get_or_create_with(&anchor, &MachineDpapi, &binding)
                .unwrap()
                .id
        });
        let second = scope.spawn(|| {
            let anchor = anchor::Anchor::test_user(path.clone());
            let binding = Binding {
                licence_id: "licence",
                device_hash: "device",
                issuer_origin: "https://license.example",
            };
            get_or_create_with(&anchor, &MachineDpapi, &binding)
                .unwrap()
                .id
        });
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(ids.0, ids.1);
    anchor::remove_test_key(&path).unwrap();
}
