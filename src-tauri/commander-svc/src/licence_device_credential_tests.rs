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
    let credential = Credential::generate().unwrap();
    let second = Credential::generate().unwrap();
    assert_ne!(credential.id, second.id);
    assert_ne!(credential.secret(), second.secret());
    assert!(load_with(&anchor, &MachineDpapi, &binding).is_err());
    save_with(&anchor, &MachineDpapi, &binding, &credential).unwrap();
    let original = anchor.read().unwrap().unwrap();
    let record: Record = serde_json::from_slice(&original).unwrap();
    assert_ne!(record.sealed, credential.secret().as_slice());
    let loaded = load_with(&anchor, &MachineDpapi, &binding).unwrap();
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
    assert!(save_with(&anchor, &MachineDpapi, &binding, &second).is_err());
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
        assert!(load_with(&anchor, &MachineDpapi, &changed).is_err());
    }
    let mut altered: Record = serde_json::from_slice(&original).unwrap();
    altered.id[0] ^= 1;
    anchor
        .write(&serde_json::to_vec(&altered).unwrap())
        .unwrap();
    assert!(load_with(&anchor, &MachineDpapi, &binding).is_err());
    assert!(save_with(&anchor, &MachineDpapi, &binding, &second).is_err());
    altered.id = credential.id;
    let index = altered.sealed.len() / 2;
    altered.sealed[index] ^= 1;
    anchor
        .write(&serde_json::to_vec(&altered).unwrap())
        .unwrap();
    assert!(load_with(&anchor, &MachineDpapi, &binding).is_err());
    anchor.write(b"not a credential").unwrap();
    assert!(load_with(&anchor, &MachineDpapi, &binding).is_err());
    assert!(save_with(&anchor, &MachineDpapi, &binding, &second).is_err());
    anchor::remove_test_key(&path).unwrap();
}
