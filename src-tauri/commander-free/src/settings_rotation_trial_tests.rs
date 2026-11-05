// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use serde_json::json;

fn sections(key: &[u8; 32]) -> Vec<(String, String)> {
    let old = derive_section_key(key, None).unwrap();
    [
        ("settings", json!({"private_value":"first secret"})),
        ("payments", json!({"token":"second secret"})),
    ]
    .into_iter()
    .map(|(name, value)| {
        (
            name.into(),
            encode_section(
                &old,
                &serde_json::to_vec(&value).unwrap(),
                &format!("machine:{name}"),
            )
            .unwrap(),
        )
    })
    .collect()
}

#[test]
fn full_generation_uses_fresh_protected_material_and_reopens_all_sections() {
    let old = [42; 32];
    let input = sections(&old);
    let before = input.clone();
    let first = prepare(&old, &input).unwrap();
    let second = prepare(&old, &input).unwrap();
    assert_ne!(first, second);
    assert_eq!(input, before);
    let raw = std::str::from_utf8(&first).unwrap();
    for secret in ["first secret", "second secret", "private_value", "payments"] {
        assert!(!raw.contains(secret));
    }
    let first_envelope: Bundle = serde_json::from_slice(&first).unwrap();
    let unwrapped = Zeroizing::new(
        dpapi_unprotect(&B64.decode(&first_envelope.protected_material).unwrap()).unwrap(),
    );
    assert_ne!(unwrapped.as_slice(), &old);
    let opened = open(&first).unwrap();
    assert_eq!(opened["settings"]["private_value"], "first secret");
    assert_eq!(opened["payments"]["token"], "second secret");
}
#[test]
fn invalid_or_ambiguous_sections_abort_before_publication() {
    let old = [42; 32];
    let mut input = sections(&old);
    assert!(prepare(&[43; 32], &input).is_err());
    input[1].1.push('X');
    assert!(prepare(&old, &input).is_err());
    let duplicate = vec![sections(&old)[0].clone(), sections(&old)[0].clone()];
    assert!(prepare(&old, &duplicate).is_err());
}
#[test]
fn failed_publication_retains_old_generation_and_success_reopens_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("generation");
    let old = [42; 32];
    let existing = prepare(&old, &sections(&old)).unwrap();
    std::fs::write(&destination, &existing).unwrap();
    let candidate = prepare(&old, &sections(&old)).unwrap();
    assert!(publish(&destination, &candidate, &|_, _| Err(
        "injected failure".into()
    ))
    .is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), existing);
    publish(&destination, &candidate, &crate::datastore_io::atomic_write).unwrap();
    assert_eq!(
        open(&std::fs::read(&destination).unwrap()).unwrap()["payments"]["token"],
        "second secret"
    );
    let mut damaged = candidate;
    let i = damaged.len() - 2;
    damaged[i] ^= 1;
    assert!(publish(&destination, &damaged, &crate::datastore_io::atomic_write).is_err());
    assert_ne!(std::fs::read(destination).unwrap(), damaged);
}

#[test]
fn unknown_commit_outcome_can_be_read_back_without_assuming_rollback() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("generation");
    let old = [42; 32];
    let before = prepare(&old, &sections(&old)).unwrap();
    std::fs::write(&destination, &before).unwrap();
    let candidate = prepare(&old, &sections(&old)).unwrap();
    let result = publish(&destination, &candidate, &|path, bytes| {
        crate::datastore_io::atomic_write(path, bytes)?;
        Err("commit outcome unknown".into())
    });
    assert!(result.is_err());
    let observed = std::fs::read(&destination).unwrap();
    assert_eq!(observed, candidate);
    assert_eq!(
        open(&observed).unwrap()["settings"]["private_value"],
        "first secret"
    );
}

#[test]
fn legacy_v1_section_is_preserved_in_fresh_generation() {
    use super::super::{nonce_from_bytes, Aead, Aes256Gcm, KeyInit, FORMAT_PREFIX_V1};
    let old = [42; 32];
    let key = derive_section_key(&old, None).unwrap();
    let nonce_bytes = [7u8; 12];
    let cipher = Aes256Gcm::new((&key).into());
    let encrypted = cipher
        .encrypt(
            nonce_from_bytes(&nonce_bytes).unwrap(),
            b"{\"old\":true}".as_ref(),
        )
        .unwrap();
    let mut bytes = nonce_bytes.to_vec();
    bytes.extend(encrypted);
    let encoded = format!("{FORMAT_PREFIX_V1}{}", B64.encode(bytes));
    let bundle = prepare(&old, &[("settings".into(), encoded)]).unwrap();
    assert_eq!(open(&bundle).unwrap()["settings"]["old"], true);
}

#[test]
fn unscoped_legacy_ciphertext_is_rejected_for_new_generation() {
    use super::super::{nonce_from_bytes, Aead, Aes256Gcm, KeyInit, FORMAT_PREFIX_V1};
    let old = [42; 32];
    let candidate = prepare(&old, &sections(&old)).unwrap();
    let mut bundle: Bundle = serde_json::from_slice(&candidate).unwrap();
    let protected = B64.decode(&bundle.protected_material).unwrap();
    let material = Zeroizing::new(dpapi_unprotect(&protected).unwrap());
    let material: &[u8; 32] = material.as_slice().try_into().unwrap();
    let key = derive_section_key(material, None).unwrap();
    let nonce_bytes = [9u8; 12];
    let cipher = Aes256Gcm::new((&key).into());
    let payload = cipher
        .encrypt(
            nonce_from_bytes(&nonce_bytes).unwrap(),
            br#"{"settings":{}}"#.as_ref(),
        )
        .unwrap();
    let mut raw = nonce_bytes.to_vec();
    raw.extend(payload);
    bundle.ciphertext = format!("{FORMAT_PREFIX_V1}{}", B64.encode(raw));
    let substituted = serde_json::to_vec(&bundle).unwrap();
    assert!(open(&substituted).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generation");
    std::fs::write(&path, &candidate).unwrap();
    assert!(publish(&path, &substituted, &crate::datastore_io::atomic_write).is_err());
    assert_eq!(std::fs::read(path).unwrap(), candidate);
}
