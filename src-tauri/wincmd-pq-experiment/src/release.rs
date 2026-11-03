// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared experimental hybrid Pro release metadata policy.
use crate::{jws, Purpose};
use serde::Deserialize;

const MAX_RELEASE: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENVELOPE: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProRelease {
    pub format: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
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

/// Verify both signatures and validate the release against local policy.
/// This experimental policy does not implement revocation or persistent rollback.
pub fn verify_metadata(
    envelope: &[u8],
    expected_version: &str,
    trust: &jws::Trust<'_>,
) -> Result<(ProRelease, u64), &'static str> {
    if envelope.len() > MAX_ENVELOPE
        || trust.purpose != Purpose::Release
        || trust.minimum_epoch == 0
    {
        return Err("invalid release policy or envelope");
    }
    let verified =
        jws::verify(envelope, trust).map_err(|_| "release signatures or policy are invalid")?;
    let release = validate_payload(&verified.payload, expected_version)?;
    Ok((release, verified.epoch))
}

fn validate_payload(payload: &[u8], expected_version: &str) -> Result<ProRelease, &'static str> {
    let release: ProRelease =
        serde_json::from_slice(payload).map_err(|_| "invalid release payload")?;
    let canonical = canonical_version(&release.version).ok_or("invalid release version")?;
    let expected_version = expected_version.trim();
    let expected_version = expected_version
        .strip_prefix('v')
        .or_else(|| expected_version.strip_prefix('V'))
        .unwrap_or(expected_version);
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
    Ok(release)
}

#[cfg(any(
    feature = "experimental-signer",
    feature = "experimental-offline-verifier"
))]
pub(crate) fn check_artifact<R: std::io::Read>(
    release: &ProRelease,
    mut artifact: R,
) -> Result<(), &'static str> {
    use sha2::{Digest, Sha256};
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

/// Experimental issuer boundary. Validate the exact artifact before invoking either signer.
/// Caller must keep the artifact immutable through publication. Private-key custody,
/// revocation, persistent rollback and publication are separate.
#[cfg(feature = "experimental-signer")]
pub fn sign_release<R, E, P>(
    payload: &[u8],
    artifact: R,
    expected_version: &str,
    epoch: u64,
    trust: &jws::Trust<'_>,
    sign_ed: E,
    sign_pq: P,
) -> Result<Vec<u8>, &'static str>
where
    R: std::io::Read,
    E: FnOnce(&[u8]) -> Result<Vec<u8>, crate::VerifyError>,
    P: FnOnce(&[u8]) -> Result<Vec<u8>, crate::VerifyError>,
{
    if payload.len() > crate::MAX_PAYLOAD_LEN
        || epoch == 0
        || epoch < trust.minimum_epoch
        || trust.purpose != Purpose::Release
        || trust.minimum_epoch == 0
    {
        return Err("invalid release policy");
    }
    let release = validate_payload(payload, expected_version)?;
    check_artifact(&release, artifact)?;
    let signed = jws::sign::assemble(payload, epoch, trust, sign_ed, sign_pq)
        .map_err(|_| "release signing failed")?;
    verify_metadata(&signed, expected_version, trust)?;
    Ok(signed)
}
