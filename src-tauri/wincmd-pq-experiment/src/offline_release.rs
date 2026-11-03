// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline rehearsal only. Trust must be obtained separately from release files.
use crate::{jws, release};
use sha2::{Digest, Sha256};
use std::io::Read;

/// Verify both signatures, the release's purpose and metadata, and the exact
/// file bytes. The caller must provide independent trust and version policy.
/// This experimental profile does not supply revocation or persistent rollback.
pub fn verify_artifact<R: Read>(
    envelope: &[u8],
    mut artifact: R,
    expected_version: &str,
    trust: &jws::Trust<'_>,
) -> Result<(), &'static str> {
    let (release, _) = release::verify_metadata(envelope, expected_version, trust)?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 65536];
    loop {
        let count = artifact
            .read(&mut buffer)
            .map_err(|_| "cannot read release file")?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or("release file too large")?;
        if total > release.size {
            return Err("release file size mismatch");
        }
        digest.update(&buffer[..count]);
    }
    if total != release.size || hex::encode(digest.finalize()) != release.sha256 {
        return Err("release file does not match signed size and hash");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{jws::sign::assemble, Purpose, VerifyError, ML_DSA_65_PUBLIC_KEY_LEN};
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn verifies_signed_file_and_rejects_tampering_and_wrong_policy() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ed_public = ed.verifying_key().to_bytes();
        let pq_public: [u8; ML_DSA_65_PUBLIC_KEY_LEN] =
            pq.public_key().as_ref().try_into().unwrap();
        let trust = jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: 7,
            ed_key_id: "ed",
            ed25519_public: &ed_public,
            pq_key_id: "pq",
            ml_dsa_65_public: &pq_public,
        };
        let bytes = b"test release bytes";
        let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-pro-release-v1", "version":"3.7.0", "url":"https://winupdates.servalabs.com/pro/v3.7.0/wincommander-pro.exe", "sha256":hex::encode(Sha256::digest(bytes)), "size":bytes.len()})).unwrap();
        let signed = assemble(
            &payload,
            7,
            &trust,
            |m| Ok(ed.sign(m).to_bytes().to_vec()),
            |m| {
                let mut s = vec![0; 3309];
                pq.sign(m, &mut s)
                    .map_err(|_| VerifyError::InvalidSignature)?;
                Ok(s)
            },
        )
        .unwrap();
        assert!(verify_artifact(&signed, &bytes[..], "3.7.0", &trust).is_ok());
        assert!(verify_artifact(&signed, &bytes[..], " V3.7.0.0 ", &trust).is_ok());
        // Even correctly signed records must satisfy the shared release policy.
        const MAX_RELEASE_TEST: u64 = 4 * 1024 * 1024 * 1024;
        for (field, value) in [
            ("format", serde_json::json!("unexpected")),
            (
                "url",
                serde_json::json!("https://example.com/wincommander-pro.exe"),
            ),
            ("sha256", serde_json::json!("A".repeat(64))),
            ("size", serde_json::json!(0)),
            ("size", serde_json::json!(MAX_RELEASE_TEST + 1)),
            ("version", serde_json::json!("3.7.1")),
            ("unexpected", serde_json::json!(true)),
            ("epoch", serde_json::json!(8)),
        ] {
            let mut bad: serde_json::Value = serde_json::from_slice(&payload).unwrap();
            bad[field] = value;
            let forged = assemble(
                &serde_json::to_vec(&bad).unwrap(),
                7,
                &trust,
                |m| Ok(ed.sign(m).to_bytes().to_vec()),
                |m| {
                    let mut s = vec![0; 3309];
                    pq.sign(m, &mut s)
                        .map_err(|_| VerifyError::InvalidSignature)?;
                    Ok(s)
                },
            )
            .unwrap();
            assert!(
                verify_artifact(&forged, &bytes[..], "3.7.0", &trust).is_err(),
                "accepted invalid field {field}"
            );
        }

        assert!(verify_artifact(&signed, &bytes[..], "3.7.1", &trust).is_err());
        assert!(verify_artifact(&signed, &b"test release byteX"[..], "3.7.0", &trust).is_err());
        assert!(verify_artifact(&signed, &bytes[..bytes.len() - 1], "3.7.0", &trust).is_err());
        let stale = jws::Trust {
            minimum_epoch: 8,
            ..trust
        };
        assert!(verify_artifact(&signed, &bytes[..], "3.7.0", &stale).is_err());
        let mut changed = signed.clone();
        changed[20] ^= 1;
        assert!(verify_artifact(&changed, &bytes[..], "3.7.0", &trust).is_err());
    }
}
