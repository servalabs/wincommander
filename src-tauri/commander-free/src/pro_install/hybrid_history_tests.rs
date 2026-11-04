// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::pro_install::{hybrid_rollback, ProInstallMetadata, ProReleaseProof};
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::{jws, Purpose, VerifyError};

#[test]
fn retired_keys_verify_only_bounded_history_and_never_new_updates() {
    let old_ed = SigningKey::from_bytes(&[31; 32]);
    let old_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let new_ed = SigningKey::from_bytes(&[42; 32]);
    let new_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let current = HybridProTrust {
        ed_key: new_ed.verifying_key().to_bytes(),
        pq_key: new_pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "new-ed".into(),
        pq_id: "new-pq".into(),
        minimum_epoch: 8,
    };
    let old_ed_public = URL_SAFE_NO_PAD.encode(old_ed.verifying_key().to_bytes());
    let old_pq_public = URL_SAFE_NO_PAD.encode(old_pq.public_key().as_ref());
    let fields = [
        Some(old_ed_public.as_str()),
        Some(old_pq_public.as_str()),
        Some("old-ed"),
        Some("old-pq"),
        Some("7"),
    ];
    let history = parse(&current, fields).unwrap().unwrap();
    assert!(parse(&current, [None; 5]).unwrap().is_none());
    assert!(parse(&current, [Some("x"), None, None, None, None]).is_err());
    let current_ed = URL_SAFE_NO_PAD.encode(current.ed_key);
    let current_pq = URL_SAFE_NO_PAD.encode(current.pq_key);
    assert!(parse(
        &current,
        [
            Some(&current_ed),
            Some(&current_pq),
            Some("old-ed"),
            Some("old-pq"),
            Some("7")
        ]
    )
    .is_err());
    let long_id = "x".repeat(129);
    assert!(parse(
        &current,
        [fields[0], fields[1], Some(&long_id), fields[3], fields[4]]
    )
    .is_err());
    assert!(parse(
        &current,
        [fields[0], fields[1], fields[2], fields[3], Some("8")]
    )
    .is_err());
    assert!(parse(
        &current,
        [fields[0], fields[1], fields[2], fields[3], Some("0")]
    )
    .is_err());
    let hash = "a".repeat(64);
    let sign = |version: &str,
                epoch: u64,
                ed: &SigningKey,
                pq: &PqdsaKeyPair,
                ed_id: &str,
                pq_id: &str| {
        let ed_public = ed.verifying_key().to_bytes();
        let pq_public: &[u8; wincmd_pq_experiment::ML_DSA_65_PUBLIC_KEY_LEN] =
            pq.public_key().as_ref().try_into().unwrap();
        let policy = jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: 1,
            ed_key_id: ed_id,
            ed25519_public: &ed_public,
            pq_key_id: pq_id,
            ml_dsa_65_public: pq_public,
        };
        let url = format!("https://winupdates.servalabs.com/pro/v{version}/wincommander-pro.exe");
        let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-pro-release-v1", "version":version, "url":url, "sha256":hash, "size":1234})).unwrap();
        String::from_utf8(
            jws::sign::assemble(
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
            .unwrap(),
        )
        .unwrap()
    };
    let old = sign("3.7.0", 7, &old_ed, &old_pq, "old-ed", "old-pq");
    let previous = ProInstallMetadata {
        version: Some("3.7.0".into()),
        sha256: hash.clone(),
        signed_manifest: None,
        hybrid_manifest: Some(old),
    };
    let make_proof = |manifest| ProReleaseProof {
        signed_manifest: None,
        hybrid_manifest: Some(manifest),
        hybrid_signed_size: Some(1234),
    };
    let next = make_proof(sign("3.8.0", 8, &new_ed, &new_pq, "new-ed", "new-pq"));
    assert!(hybrid_rollback::check_previous_with_history(
        &previous,
        &next,
        Some("3.8.0"),
        &hash,
        &current,
        Some(&history)
    )
    .is_ok());
    assert!(hybrid_rollback::check_previous_with_history(
        &previous,
        &next,
        Some("3.8.0"),
        &hash,
        &current,
        None
    )
    .is_err());
    let stale_candidate = make_proof(sign("3.8.0", 8, &old_ed, &old_pq, "old-ed", "old-pq"));
    assert!(hybrid_rollback::check_previous_with_history(
        &previous,
        &stale_candidate,
        Some("3.8.0"),
        &hash,
        &current,
        Some(&history)
    )
    .is_err());
    let too_late = ProInstallMetadata {
        version: previous.version.clone(),
        sha256: hash.clone(),
        signed_manifest: None,
        hybrid_manifest: Some(sign("3.7.0", 8, &old_ed, &old_pq, "old-ed", "old-pq")),
    };
    assert!(hybrid_rollback::check_previous_with_history(
        &too_late,
        &next,
        Some("3.8.0"),
        &hash,
        &current,
        Some(&history)
    )
    .is_err());
    let mut damaged = previous;
    damaged.hybrid_manifest.as_mut().unwrap().push('x');
    assert!(hybrid_rollback::check_previous_with_history(
        &damaged,
        &next,
        Some("3.8.0"),
        &hash,
        &current,
        Some(&history)
    )
    .is_err());
}
