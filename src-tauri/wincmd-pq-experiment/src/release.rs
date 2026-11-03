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
) -> Result<ProRelease, &'static str> {
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
