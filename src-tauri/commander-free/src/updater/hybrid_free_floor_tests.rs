// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::{jws, VerifyError};

#[test]
fn record_reads_are_bounded_and_missing_is_explicit() {
    let path = std::env::temp_dir().join(format!("free-floor-test-{}", uuid::Uuid::new_v4()));
    assert!(read_record(&path).unwrap().is_none());
    std::fs::write(&path, b"record").unwrap();
    assert_eq!(read_record(&path).unwrap().as_deref(), Some(&b"record"[..]));
    std::fs::write(&path, vec![b'x'; LIMIT + 1]).unwrap();
    assert!(read_record(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn signed_record_rejects_replay_and_allows_retry_and_channel_promotion() {
    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let trust = Trust {
        ed: ed.verifying_key().to_bytes(),
        pq: pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "ed",
        pq_id: "pq",
        epoch: 1,
    };
    let make = |version: &str, channel: &str, epoch: u64, hash: &str| {
        let url = format!(
            "https://winupdates.servalabs.com/free/v{version}/WinCommander_{version}_x64-setup.exe"
        );
        let payload = serde_json::to_vec(
            &serde_json::json!({"format":"wincommander-free-release-v1", "version":version,
            "channel":channel, "platform":"windows-x86_64", "url":url, "size":2, "sha256":hash}),
        )
        .unwrap();
        let envelope = jws::sign::assemble(
            &payload,
            epoch,
            &trust.policy(),
            |message| Ok(ed.sign(message).to_bytes().to_vec()),
            |message| {
                let mut sig = vec![0; 3309];
                pq.sign(message, &mut sig)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(sig)
            },
        )
        .unwrap();
        Record {
            version: version.into(),
            channel: channel.into(),
            envelope: String::from_utf8(envelope).unwrap(),
        }
    };
    let hash = "a".repeat(64);
    let old = make("3.7.0", "beta", 7, &hash);
    let stored = serde_json::to_vec(&old).unwrap();
    assert!(check(None, &old, &trust).is_ok());
    assert!(check(Some(&stored), &old, &trust).is_ok()); // failed installation can retry
    assert!(check(Some(&stored), &make("3.7.0", "stable", 7, &hash), &trust).is_ok());
    assert!(check(Some(&stored), &make("3.8.0", "stable", 8, &hash), &trust).is_ok());
    assert!(check(Some(&stored), &make("3.6.9", "stable", 8, &hash), &trust).is_err());
    assert!(check(Some(&stored), &make("3.8.0", "stable", 6, &hash), &trust).is_err());
    assert!(check(
        Some(&stored),
        &make("3.7.0", "stable", 8, &"b".repeat(64)),
        &trust
    )
    .is_err());
    assert!(check(Some(b"corrupt"), &old, &trust).is_err());
    let oversized = vec![b' '; LIMIT + 1];
    assert!(check(Some(&oversized), &old, &trust).is_err());
    let mut corrupt: Record = serde_json::from_slice(&stored).unwrap();
    corrupt.envelope = "invalid".into();
    assert!(check(None, &corrupt, &trust).is_err());
    assert!(check(Some(&serde_json::to_vec(&corrupt).unwrap()), &old, &trust).is_err());
}
