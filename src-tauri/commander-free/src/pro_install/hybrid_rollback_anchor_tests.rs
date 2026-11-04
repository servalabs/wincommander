// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::{jws, Purpose, VerifyError};

#[test]
fn independent_anchor_rejects_replays_and_allows_retry() {
    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let trust = hybrid_release::HybridProTrust {
        ed_key: ed.verifying_key().to_bytes(),
        pq_key: pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "ed".into(),
        pq_id: "pq".into(),
        minimum_epoch: 1,
    };
    let policy = jws::Trust {
        purpose: Purpose::Release,
        minimum_epoch: 1,
        ed_key_id: &trust.ed_id,
        ed25519_public: &trust.ed_key,
        pq_key_id: &trust.pq_id,
        ml_dsa_65_public: &trust.pq_key,
    };
    let hash = "a".repeat(64);
    let candidate = |version: &str, epoch| {
        let url = format!("https://winupdates.servalabs.com/pro/v{version}/wincommander-pro.exe");
        let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-pro-release-v1", "version":version, "url":url, "sha256":hash, "size":1234})).unwrap();
        let envelope = jws::sign::assemble(
            &payload,
            epoch,
            &policy,
            |message| Ok(ed.sign(message).to_bytes().to_vec()),
            |message| {
                let mut signature = vec![0; 3309];
                pq.sign(message, &mut signature)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(signature)
            },
        )
        .unwrap();
        ProReleaseProof {
            signed_manifest: None,
            hybrid_manifest: Some(String::from_utf8(envelope).unwrap()),
            hybrid_signed_size: Some(1234),
        }
    };
    let path = format!(
        r"Software\ServaLabs\WinCommanderTest\{}",
        uuid::Uuid::new_v4()
    );
    let anchor = crate::hybrid_update_anchor::Anchor::test_user(path.clone());
    let directory = tempfile::tempdir().unwrap();
    let metadata_path = directory.path().join("installed.json");
    let current = candidate("3.7.0", 7);
    let newer = candidate("3.8.0", 8);
    let older = candidate("3.6.9", 7);
    let check = |proof: &ProReleaseProof, version| {
        check_and_record_with_anchor(&metadata_path, &anchor, proof, Some(version), &hash, &trust)
    };
    assert!(check(&current, "3.7.0").is_ok());
    // Deleting or replacing the install metadata does not remove the anchor.
    assert!(check(&older, "3.6.9").is_err());
    let classical = ProInstallMetadata {
        version: Some("3.6.9".into()),
        sha256: hash.clone(),
        signed_manifest: None,
        hybrid_manifest: None,
    };
    std::fs::write(&metadata_path, serde_json::to_vec(&classical).unwrap()).unwrap();
    assert!(check(&older, "3.6.9").is_err());
    assert!(check(&newer, "3.8.0").is_ok());
    assert!(check(&newer, "3.8.0").is_ok());
    assert!(check(&current, "3.7.0").is_err());
    anchor.write(br#"{"sha256":"old"}"#).unwrap();
    assert!(record_anchor(&anchor, &newer, Some("3.8.0"), &hash, &trust).is_err());
    anchor.write(b"damaged").unwrap();
    assert!(record_anchor(&anchor, &newer, Some("3.8.0"), &hash, &trust).is_err());
    crate::hybrid_update_anchor::remove_test_key(&path).unwrap();
}
