// SPDX-License-Identifier: AGPL-3.0-or-later
use super::super::history;
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use wincmd_pq_experiment::{jws, Purpose, VerifyError};

#[test]
fn retired_keys_only_verify_bounded_free_update_history() {
    let old_ed = SigningKey::from_bytes(&[31; 32]);
    let old_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let new_ed = SigningKey::from_bytes(&[42; 32]);
    let new_pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let current = Trust {
        ed: new_ed.verifying_key().to_bytes(),
        pq: new_pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "new-ed",
        pq_id: "new-pq",
        epoch: 8,
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
    let old_trust = history::parse(&current, fields).unwrap().unwrap();
    assert!(history::parse(&current, [None; 5]).unwrap().is_none());
    assert!(history::parse(&current, [Some("x"), None, None, None, None]).is_err());
    assert!(history::parse(
        &current,
        [fields[0], fields[1], fields[2], fields[3], Some("8")]
    )
    .is_err());
    let new_ed_public = URL_SAFE_NO_PAD.encode(current.ed);
    let new_pq_public = URL_SAFE_NO_PAD.encode(current.pq);
    assert!(history::parse(
        &current,
        [
            Some(&new_ed_public),
            Some(&new_pq_public),
            fields[2],
            fields[3],
            fields[4]
        ]
    )
    .is_err());
    let make = |version: &str,
                epoch: u64,
                ed: &SigningKey,
                pq: &PqdsaKeyPair,
                ed_id: &str,
                pq_id: &str| {
        let ed_public = ed.verifying_key().to_bytes();
        let pq_public = pq.public_key().as_ref().try_into().unwrap();
        let policy = jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: 1,
            ed_key_id: ed_id,
            ed25519_public: &ed_public,
            pq_key_id: pq_id,
            ml_dsa_65_public: pq_public,
        };
        let url = format!(
            "https://winupdates.servalabs.com/free/v{version}/WinCommander_{version}_x64-setup.exe"
        );
        let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-free-release-v1", "version":version, "channel":"stable", "platform":"windows-x86_64", "url":url, "size":2, "sha256":"a".repeat(64)})).unwrap();
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
        Record {
            version: version.into(),
            channel: "stable".into(),
            envelope: String::from_utf8(envelope).unwrap(),
        }
    };
    let previous = make("3.7.0", 7, &old_ed, &old_pq, "old-ed", "old-pq");
    let stored = serde_json::to_vec(&previous).unwrap();
    let next = make("3.8.0", 8, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(check_with_history(Some(&stored), &next, &current, Some(&old_trust)).is_ok());
    assert!(check_with_history(Some(&stored), &next, &current, None).is_err());
    let retired_candidate = make("3.8.0", 8, &old_ed, &old_pq, "old-ed", "old-pq");
    assert!(check_with_history(
        Some(&stored),
        &retired_candidate,
        &current,
        Some(&old_trust)
    )
    .is_err());
    let too_late =
        serde_json::to_vec(&make("3.7.0", 8, &old_ed, &old_pq, "old-ed", "old-pq")).unwrap();
    assert!(check_with_history(Some(&too_late), &next, &current, Some(&old_trust)).is_err());
    let mut damaged = previous;
    damaged.envelope.push('x');
    assert!(check_with_history(
        Some(&serde_json::to_vec(&damaged).unwrap()),
        &next,
        &current,
        Some(&old_trust)
    )
    .is_err());
    let rollback = make("3.6.9", 8, &new_ed, &new_pq, "new-ed", "new-pq");
    assert!(check_with_history(Some(&stored), &rollback, &current, Some(&old_trust)).is_err());
}
