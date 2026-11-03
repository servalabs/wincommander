// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental Free installer profile. Not connected to the production release pipeline.
use crate::{jws, Purpose};
use aws_lc_rs::digest::{digest, SHA256};
use serde::Deserialize;
#[cfg(any(
    feature = "experimental-signer",
    feature = "experimental-offline-verifier"
))]
use std::io::Read;

const MAX_ENVELOPE: usize = 32 * 1024;
const MAX_INSTALLER: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreeRelease {
    pub format: String,
    pub version: String,
    pub channel: String,
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// Require both signatures and bind the result to locally chosen update context.
/// Persistent rollback and key lifecycle are the caller's responsibility.
pub fn verify_metadata(
    envelope: &[u8],
    expected_version: &str,
    expected_channel: &str,
    expected_url: &str,
    trust: &jws::Trust<'_>,
) -> Result<(FreeRelease, u64), &'static str> {
    if envelope.len() > MAX_ENVELOPE
        || trust.purpose != Purpose::Release
        || trust.minimum_epoch == 0
    {
        return Err("invalid Free release policy or envelope");
    }
    let verified = jws::verify(envelope, trust).map_err(|_| "invalid Free release signatures")?;
    let release = validate_payload(
        &verified.payload,
        expected_version,
        expected_channel,
        expected_url,
    )?;
    Ok((release, verified.epoch))
}

fn validate_payload(
    payload: &[u8],
    version: &str,
    channel: &str,
    url: &str,
) -> Result<FreeRelease, &'static str> {
    let release: FreeRelease =
        serde_json::from_slice(payload).map_err(|_| "invalid Free release payload")?;
    // The publisher currently produces exactly three numeric components.
    let components: Vec<_> = version.split('.').collect();
    let valid_version = components.len() == 3
        && components.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 10
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
                && part.parse::<u32>().is_ok()
        });
    let canonical_url = format!(
        "https://winupdates.servalabs.com/free/v{version}/WinCommander_{version}_x64-setup.exe"
    );
    if !valid_version
        || !matches!(channel, "stable" | "beta")
        || release.format != "wincommander-free-release-v1"
        || release.version != version
        || release.channel != channel
        || release.platform != "windows-x86_64"
        || release.url != canonical_url
        || release.url != url
        || release.size == 0
        || release.size > MAX_INSTALLER
        || release.sha256.len() != 64
        || !release
            .sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("Free release metadata does not match policy");
    }
    Ok(release)
}

/// Check the bytes that will actually be installed against the signed metadata.
pub fn verify_artifact(release: &FreeRelease, bytes: &[u8]) -> Result<(), &'static str> {
    if u64::try_from(bytes.len()).ok() != Some(release.size) {
        return Err("Free installer size mismatch");
    }
    let actual: String = digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != release.sha256 {
        return Err("Free installer hash mismatch");
    }
    Ok(())
}

/// Prepare signing inputs only after checking the intended installer and policy.
#[cfg(feature = "experimental-signer")]
pub fn prepare_release(
    payload: &[u8],
    bytes: &[u8],
    version: &str,
    channel: &str,
    url: &str,
    epoch: u64,
    trust: &jws::Trust<'_>,
) -> Result<jws::sign::SigningInputs, &'static str> {
    prepare_release_from_reader(payload, bytes, version, channel, url, epoch, trust)
}

#[cfg(any(
    feature = "experimental-signer",
    feature = "experimental-offline-verifier"
))]
fn read_artifact(mut reader: impl Read) -> Result<(u64, String), &'static str> {
    let mut ctx = aws_lc_rs::digest::Context::new(&SHA256);
    let mut buf = [0u8; 65536];
    let mut size = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|_| "cannot read Free installer")?;
        if n == 0 {
            break;
        }
        size = size
            .checked_add(n as u64)
            .ok_or("Free installer is too large")?;
        if size > MAX_INSTALLER {
            return Err("Free installer is too large");
        }
        ctx.update(&buf[..n]);
    }
    let hash = ctx
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((size, hash))
}

/// Verify an independently obtained record and stream the exact Free installer.
#[cfg(feature = "experimental-offline-verifier")]
pub fn verify_offline(
    envelope: &[u8],
    artifact: impl Read,
    version: &str,
    channel: &str,
    trust: &jws::Trust<'_>,
) -> Result<(), &'static str> {
    let url = format!(
        "https://winupdates.servalabs.com/free/v{version}/WinCommander_{version}_x64-setup.exe"
    );
    let (release, _) = verify_metadata(envelope, version, channel, &url, trust)?;
    let (size, hash) = read_artifact(artifact)?;
    if size != release.size || hash != release.sha256 {
        return Err("Free installer does not match signed metadata");
    }
    Ok(())
}

/// Derive an unsigned Free payload from the staged Tauri manifest and file.
#[cfg(feature = "experimental-signer")]
pub fn stage_payload(
    manifest: &[u8],
    artifact: impl Read,
    version: &str,
    channel: &str,
) -> Result<Vec<u8>, &'static str> {
    if manifest.len() > 32768 {
        return Err("Free manifest is too large");
    }
    #[derive(Deserialize)]
    struct Manifest {
        version: String,
        platforms: std::collections::HashMap<String, Platform>,
    }
    #[derive(Deserialize)]
    struct Platform {
        url: String,
        signature: String,
    }
    let manifest: Manifest =
        serde_json::from_slice(manifest).map_err(|_| "invalid Free manifest")?;
    let platform = manifest
        .platforms
        .get("windows-x86_64")
        .ok_or("Free Windows platform missing")?;
    if manifest.version != version || platform.signature.is_empty() {
        return Err("Free manifest does not match release");
    }
    let (size, sha256) = read_artifact(artifact)?;
    let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-free-release-v1", "version": version,
        "channel":channel, "platform":"windows-x86_64", "url":platform.url, "size":size, "sha256":sha256}))
        .map_err(|_| "cannot encode Free release")?;
    validate_payload(&payload, version, channel, &platform.url)?;
    Ok(payload)
}

/// Stream and verify the installer before generating either signing message.
#[cfg(feature = "experimental-signer")]
pub fn prepare_release_from_reader(
    payload: &[u8],
    artifact: impl Read,
    version: &str,
    channel: &str,
    url: &str,
    epoch: u64,
    trust: &jws::Trust<'_>,
) -> Result<jws::sign::SigningInputs, &'static str> {
    if payload.len() > crate::MAX_PAYLOAD_LEN
        || trust.purpose != Purpose::Release
        || trust.minimum_epoch == 0
    {
        return Err("invalid Free release policy");
    }
    let release = validate_payload(payload, version, channel, url)?;
    let (size, hash) = read_artifact(artifact)?;
    if size != release.size || hash != release.sha256 {
        return Err("Free installer does not match signed metadata");
    }
    jws::sign::signing_inputs(payload, epoch, trust).map_err(|_| "invalid Free signing parameters")
}

/// Recheck the file, assemble the supplied signatures and verify both before returning.
#[cfg(feature = "experimental-signer")]
pub fn finish_release(
    payload: &[u8],
    artifact: impl Read,
    version: &str,
    channel: &str,
    epoch: u64,
    trust: &jws::Trust<'_>,
    signatures: (Vec<u8>, Vec<u8>),
) -> Result<Vec<u8>, &'static str> {
    let url = format!(
        "https://winupdates.servalabs.com/free/v{version}/WinCommander_{version}_x64-setup.exe"
    );
    prepare_release_from_reader(payload, artifact, version, channel, &url, epoch, trust)?;
    let envelope = jws::sign::assemble(
        payload,
        epoch,
        trust,
        |_| Ok(signatures.0),
        |_| Ok(signatures.1),
    )
    .map_err(|_| "Free release signing failed")?;
    verify_metadata(&envelope, version, channel, &url, trust)?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn binds_both_signatures_and_installer_context() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let ed_public = ed.verifying_key().to_bytes();
        let pq_public = pq.public_key().as_ref().try_into().unwrap();
        let trust = jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: 7,
            ed_key_id: "ed",
            ed25519_public: &ed_public,
            pq_key_id: "pq",
            ml_dsa_65_public: &pq_public,
        };
        let bytes = b"test installer";
        let hash: String = digest(&SHA256, bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let url = "https://winupdates.servalabs.com/free/v3.7.0/WinCommander_3.7.0_x64-setup.exe";
        let payload =
            serde_json::to_vec(&serde_json::json!({"format":"wincommander-free-release-v1",
            "version":"3.7.0", "channel":"stable", "platform":"windows-x86_64", "url":url,
            "sha256":hash, "size":bytes.len()}))
            .unwrap();
        let envelope = jws::sign::assemble(
            &payload,
            7,
            &trust,
            |m| Ok(ed.sign(m).to_bytes().to_vec()),
            |m| {
                let mut s = vec![0; 3309];
                pq.sign(m, &mut s).unwrap();
                Ok(s)
            },
        )
        .unwrap();
        let verify = |e: &[u8], v, c, u| verify_metadata(e, v, c, u, &trust);
        let (release, epoch) = verify(&envelope, "3.7.0", "stable", url).unwrap();
        assert_eq!(epoch, 7);
        assert!(verify_artifact(&release, bytes).is_ok());
        #[cfg(feature = "experimental-signer")]
        {
            assert!(prepare_release(&payload, bytes, "3.7.0", "stable", url, 7, &trust).is_ok());
            assert!(
                prepare_release(&payload, b"wrong", "3.7.0", "stable", url, 7, &trust).is_err()
            );
            assert!(prepare_release(&payload, bytes, "3.7.0", "stable", url, 6, &trust).is_err());
        }
        assert!(verify_artifact(&release, b"test installex").is_err());
        assert!(verify_artifact(&release, b"short").is_err());
        assert!(verify(&envelope, "3.7.1", "stable", url).is_err());
        assert!(verify(&envelope, "3.7.0", "beta", url).is_err());
        assert!(verify(
            &envelope,
            "3.7.0",
            "stable",
            "https://evil.example/setup.exe"
        )
        .is_err());
        for index in 0..2 {
            let mut altered: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
            altered["signatures"][index]["signature"] = serde_json::json!("invalid");
            assert!(verify(
                &serde_json::to_vec(&altered).unwrap(),
                "3.7.0",
                "stable",
                url
            )
            .is_err());
        }
        let mut wrong = serde_json::from_slice::<serde_json::Value>(&payload).unwrap();
        wrong["platform"] = serde_json::json!("windows-aarch64");
        assert!(
            validate_payload(&serde_json::to_vec(&wrong).unwrap(), "3.7.0", "stable", url).is_err()
        );
        wrong["platform"] = serde_json::json!("windows-x86_64");
        wrong["extra"] = serde_json::json!(true);
        assert!(
            validate_payload(&serde_json::to_vec(&wrong).unwrap(), "3.7.0", "stable", url).is_err()
        );
    }
}
