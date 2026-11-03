// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in Free updater rehearsal. No production keys or publisher are configured.
use crate::settings::UpdateChannel;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use wincmd_pq_experiment::{free_release, jws, Purpose};

#[path = "hybrid_free_floor.rs"]
mod floor;

pub(super) struct VerifiedUpdate {
    pub(super) bytes: Vec<u8>,
    envelope: Vec<u8>,
    version: String,
    channel: UpdateChannel,
}

pub(super) fn record_before_install(
    verified: &VerifiedUpdate,
) -> Result<crate::paths::MachineStateLock, String> {
    let trust = Trust::configured()?;
    floor::record(verified, &trust)
}

struct Trust {
    ed: [u8; 32],
    pq: [u8; wincmd_pq_experiment::ML_DSA_65_PUBLIC_KEY_LEN],
    ed_id: &'static str,
    pq_id: &'static str,
    epoch: u64,
}

impl Trust {
    fn configured() -> Result<Self, String> {
        let decode = |value: Option<&str>| -> Result<Vec<u8>, String> {
            URL_SAFE_NO_PAD
                .decode(value.ok_or("Free update trust is not configured")?)
                .map_err(|_| "Free update trust key is invalid".into())
        };
        Ok(Self {
            ed: decode(option_env!("WINCMD_PQ_FREE_ED_KEY"))?
                .try_into()
                .map_err(|_| "Free Ed25519 key has invalid length")?,
            pq: decode(option_env!("WINCMD_PQ_FREE_MLDSA_KEY"))?
                .try_into()
                .map_err(|_| "Free ML-DSA key has invalid length")?,
            ed_id: option_env!("WINCMD_PQ_FREE_ED_ID")
                .filter(|s| !s.is_empty())
                .ok_or("Free Ed25519 key ID is missing")?,
            pq_id: option_env!("WINCMD_PQ_FREE_MLDSA_ID")
                .filter(|s| !s.is_empty())
                .ok_or("Free ML-DSA key ID is missing")?,
            epoch: option_env!("WINCMD_PQ_FREE_EPOCH")
                .and_then(|s| s.parse().ok())
                .filter(|n| *n > 0)
                .ok_or("Free update epoch is invalid")?,
        })
    }
    fn policy(&self) -> jws::Trust<'_> {
        jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: self.epoch,
            ed_key_id: self.ed_id,
            ed25519_public: &self.ed,
            pq_key_id: self.pq_id,
            ml_dsa_65_public: &self.pq,
        }
    }
}

fn channel_name(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
    }
}

// Reject attacker-controlled path characters before using an unsigned version in a URL.
fn metadata_url(version: &str, channel: UpdateChannel) -> Result<String, String> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || !parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 10
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
                && p.parse::<u32>().is_ok()
        })
    {
        return Err("Free update version is invalid".into());
    }
    Ok(format!(
        "https://winupdates.servalabs.com/free/v{version}/hybrid-{}.json",
        channel_name(channel)
    ))
}

async fn fetch_metadata(version: &str, channel: UpdateChannel) -> Result<Vec<u8>, String> {
    let url = metadata_url(version, channel)?;
    let client = crate::net::required_hybrid_http_client(std::time::Duration::from_secs(15))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Free update metadata network error: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Free update metadata HTTP {}", response.status()));
    }
    const LIMIT: usize = 32 * 1024;
    if response.content_length().is_some_and(|n| n > LIMIT as u64) {
        return Err("Free update metadata is too large".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Free update metadata network error: {e}"))?
    {
        if chunk.len() > LIMIT.saturating_sub(body.len()) {
            return Err("Free update metadata is too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn download_bounded(url: &str, size: u64) -> Result<Vec<u8>, String> {
    let client = crate::net::required_hybrid_http_client(std::time::Duration::from_secs(900))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Free update download failed: {e}"))?;
    read_bounded_response(response, size).await
}

async fn read_bounded_response(
    mut response: reqwest::Response,
    size: u64,
) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(format!("Free update download HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length != size)
    {
        return Err("Free update size mismatch".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Free update download failed: {e}"))?
    {
        append_bounded(&mut bytes, &chunk, size)?;
    }
    Ok(bytes)
}

fn append_bounded(bytes: &mut Vec<u8>, chunk: &[u8], size: u64) -> Result<(), String> {
    if (chunk.len() as u64) > size.saturating_sub(bytes.len() as u64) {
        return Err("Free update size exceeds signed limit".into());
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

fn metadata(
    envelope: &[u8],
    version: &str,
    channel: UpdateChannel,
    url: &str,
    trust: &Trust,
) -> Result<free_release::FreeRelease, String> {
    free_release::verify_metadata(
        envelope,
        version,
        channel_name(channel),
        url,
        &trust.policy(),
    )
    .map(|(release, _)| release)
    .map_err(|e| format!("Free update verification failed: {e}"))
}

pub(super) async fn verified_bytes(
    update: &tauri_plugin_updater::Update,
    channel: UpdateChannel,
    staged: Option<Vec<u8>>,
) -> Result<VerifiedUpdate, String> {
    let trust = Trust::configured()?;
    let envelope = fetch_metadata(&update.version, channel).await?;
    let release = metadata(
        &envelope,
        &update.version,
        channel,
        update.download_url.as_str(),
        &trust,
    )?;
    let bytes = match staged {
        Some(bytes) => bytes,
        None => download_bounded(&release.url, release.size).await?,
    };
    verify_package(update, &release, &bytes)?;
    Ok(VerifiedUpdate {
        bytes,
        envelope,
        version: update.version.clone(),
        channel,
    })
}

fn verify_package(
    update: &tauri_plugin_updater::Update,
    release: &free_release::FreeRelease,
    bytes: &[u8],
) -> Result<(), String> {
    free_release::verify_artifact(release, bytes)
        .map_err(|e| format!("Free update verification failed: {e}"))?;
    // The plugin verifies its original signature as an additional required check.
    update
        .verify(bytes)
        .map_err(|e| format!("Free update signature failed: {e}"))
}

#[cfg(test)]
#[path = "hybrid_free_tests.rs"]
mod tests;
