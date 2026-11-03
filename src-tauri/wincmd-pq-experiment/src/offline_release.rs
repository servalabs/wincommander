// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline rehearsal only. Trust must be obtained separately from release files.
use crate::{jws, Purpose};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;

const MAX_RELEASE: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENVELOPE: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProRelease {
    format: String,
    version: String,
    url: String,
    sha256: String,
    size: u64,
}

fn canonical_version(version: &str) -> Option<String> {
    let parts: Vec<u64> = version
        .split('.')
        .map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        })
        .collect::<Option<_>>()?;
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let mut canonical = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    if parts.get(3).is_some_and(|r| *r != 0) {
        canonical.push_str(&format!(".{}", parts[3]));
    }
    Some(canonical)
}

/// Verify both signatures, the release's purpose and metadata, and the exact
/// file bytes. The caller must provide independent trust and version policy.
/// This experimental profile does not supply revocation or persistent rollback.
pub fn verify_artifact<R: Read>(
    envelope: &[u8],
    mut artifact: R,
    expected_version: &str,
    trust: &jws::Trust<'_>,
) -> Result<(), &'static str> {
    if envelope.len() > MAX_ENVELOPE
        || trust.purpose != Purpose::Release
        || trust.minimum_epoch == 0
    {
        return Err("invalid release policy or envelope");
    }
    let verified =
        jws::verify(envelope, trust).map_err(|_| "release signatures or policy are invalid")?;
    let release: ProRelease =
        serde_json::from_slice(&verified.payload).map_err(|_| "invalid release payload")?;
    let canonical = canonical_version(&release.version).ok_or("invalid release version")?;
    if canonical_version(expected_version).as_deref() != Some(canonical.as_str())
        || release.format != "wincommander-pro-release-v1"
        || release.url
            != format!("https://winupdates.servalabs.com/pro/v{canonical}/wincommander-pro.exe")
        || release.size == 0
        || release.size > MAX_RELEASE
        || release.sha256.len() != 64
        || !release
            .sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("release metadata does not match policy");
    }
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
    use crate::{jws::sign::assemble, VerifyError, ML_DSA_65_PUBLIC_KEY_LEN};
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
