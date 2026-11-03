// SPDX-License-Identifier: AGPL-3.0-or-later
//! Replay resistance while the locally installed signed record is retained.
use super::*;
use std::io::Read;

pub(super) fn check_installed_floor(
    metadata_path: &std::path::Path,
    candidate: &ProReleaseProof,
    version: Option<&str>,
    sha256: &str,
) -> Result<(), String> {
    let file = match std::fs::File::open(metadata_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("validation:Cannot read current Pro release record".into()),
    };
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "validation:Cannot read current Pro release record")?;
    if bytes.len() > 65536 {
        return Err("validation:Current Pro release record is too large".into());
    }
    let previous: ProInstallMetadata = serde_json::from_slice(&bytes)
        .map_err(|_| "validation:Current Pro release record is invalid")?;
    check_previous(&previous, candidate, version, sha256, &hybrid_pro_trust()?)
}

fn check_previous(
    previous: &ProInstallMetadata,
    candidate: &ProReleaseProof,
    version: Option<&str>,
    sha256: &str,
    trust: &hybrid_release::HybridProTrust,
) -> Result<(), String> {
    // Existing classical records have no hybrid floor; allow the first migration.
    let Some(old_record) = previous.hybrid_manifest.as_deref() else {
        return Ok(());
    };
    let previous_epoch = verify_hybrid_pro(
        old_record.as_bytes(),
        trust,
        previous.version.as_deref(),
        &previous.sha256,
        None,
    )
    .map_err(|_| "validation:Current hybrid Pro release record cannot be trusted")?;
    let new_record = candidate
        .hybrid_manifest
        .as_deref()
        .ok_or("validation:Hybrid Pro release record is missing")?;
    let new_epoch = verify_hybrid_pro(new_record.as_bytes(), trust, version, sha256, None)?;
    let old_version = previous
        .version
        .as_deref()
        .and_then(parse_semver)
        .ok_or("validation:Current Pro version is invalid")?;
    let new_version = version
        .and_then(parse_semver)
        .ok_or("validation:New Pro version is invalid")?;
    if new_epoch.epoch < previous_epoch.epoch
        || new_version < old_version
        || (new_version == old_version && !previous.sha256.eq_ignore_ascii_case(sha256))
    {
        return Err("validation:Pro release would roll back or replace an existing version".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};
    use wincmd_pq_experiment::{jws, Purpose, VerifyError};

    #[test]
    fn missing_record_allows_bootstrap_but_damaged_or_oversized_record_fails_closed() {
        let dir = std::env::temp_dir().join(format!("hybrid-floor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("installed.json");
        let proof = ProReleaseProof::default();
        assert!(check_installed_floor(&path, &proof, Some("3.7.0"), &"a".repeat(64)).is_ok());
        std::fs::write(&path, b"{damaged").unwrap();
        assert!(check_installed_floor(&path, &proof, Some("3.7.0"), &"a".repeat(64)).is_err());
        std::fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(check_installed_floor(&path, &proof, Some("3.7.0"), &"a".repeat(64)).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn signed_previous_record_prevents_version_epoch_and_same_version_replays() {
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
        let make = |version: &str, epoch: u64, hash: &str| {
            let url =
                format!("https://winupdates.servalabs.com/pro/v{version}/wincommander-pro.exe");
            let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-pro-release-v1", "version":version, "url":url, "sha256":hash, "size":1234})).unwrap();
            let bytes = jws::sign::assemble(
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
            String::from_utf8(bytes).unwrap()
        };
        let hash = "a".repeat(64);
        let previous = ProInstallMetadata {
            version: Some("3.7.0".into()),
            sha256: hash.clone(),
            signed_manifest: None,
            hybrid_manifest: Some(make("3.7.0", 7, &hash)),
        };
        let candidate = |version, epoch, digest: &str| ProReleaseProof {
            signed_manifest: None,
            hybrid_manifest: Some(make(version, epoch, digest)),
            hybrid_signed_size: Some(1234),
        };
        assert!(check_previous(
            &previous,
            &candidate("3.7.0", 7, &hash),
            Some("3.7.0"),
            &hash,
            &trust
        )
        .is_ok());
        assert!(check_previous(
            &previous,
            &candidate("3.8.0", 8, &hash),
            Some("3.8.0"),
            &hash,
            &trust
        )
        .is_ok());
        assert!(check_previous(
            &previous,
            &candidate("3.6.9", 8, &hash),
            Some("3.6.9"),
            &hash,
            &trust
        )
        .is_err());
        assert!(check_previous(
            &previous,
            &candidate("3.8.0", 6, &hash),
            Some("3.8.0"),
            &hash,
            &trust
        )
        .is_err());
        let different = "b".repeat(64);
        assert!(check_previous(
            &previous,
            &candidate("3.7.0", 8, &different),
            Some("3.7.0"),
            &different,
            &trust
        )
        .is_err());
        let corrupt = ProInstallMetadata {
            hybrid_manifest: Some("not a signed record".into()),
            ..previous
        };
        assert!(check_previous(
            &corrupt,
            &candidate("3.8.0", 8, &hash),
            Some("3.8.0"),
            &hash,
            &trust
        )
        .is_err());
        let legacy = ProInstallMetadata {
            hybrid_manifest: None,
            ..corrupt
        };
        assert!(check_previous(
            &legacy,
            &candidate("3.8.0", 8, &hash),
            Some("3.8.0"),
            &hash,
            &trust
        )
        .is_ok());
    }
}
