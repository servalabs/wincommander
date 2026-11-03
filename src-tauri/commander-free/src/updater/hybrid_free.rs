// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in Free updater rehearsal. No production keys or publisher are configured.
use crate::settings::UpdateChannel;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use wincmd_pq_experiment::{free_release, jws, Purpose};

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
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Free update download failed: {e}"))?;
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
) -> Result<Vec<u8>, String> {
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
    free_release::verify_artifact(&release, &bytes)
        .map_err(|e| format!("Free update verification failed: {e}"))?;
    // The plugin verifies its original signature as an additional required check.
    update
        .verify(&bytes)
        .map_err(|e| format!("Free update signature failed: {e}"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    #[test]
    fn rejects_bad_context_signatures_and_installer() {
        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let trust = Trust {
            ed: ed.verifying_key().to_bytes(),
            pq: pq.public_key().as_ref().try_into().unwrap(),
            ed_id: "ed",
            pq_id: "pq",
            epoch: 7,
        };
        let bytes = b"installer";
        let url = "https://winupdates.servalabs.com/free/v3.7.0/WinCommander_3.7.0_x64-setup.exe";
        let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-free-release-v1", "version":"3.7.0", "channel":"stable", "platform":"windows-x86_64", "url":url, "sha256":hex::encode(Sha256::digest(bytes)), "size":bytes.len()})).unwrap();
        let envelope = jws::sign::assemble(
            &payload,
            7,
            &trust.policy(),
            |m| Ok(ed.sign(m).to_bytes().to_vec()),
            |m| {
                let mut s = vec![0; 3309];
                pq.sign(m, &mut s).unwrap();
                Ok(s)
            },
        )
        .unwrap();
        let release = metadata(&envelope, "3.7.0", UpdateChannel::Stable, url, &trust).unwrap();
        assert!(free_release::verify_artifact(&release, bytes).is_ok());
        assert!(free_release::verify_artifact(&release, b"malicious").is_err());
        assert!(metadata(&envelope, "3.7.0", UpdateChannel::Beta, url, &trust).is_err());
        assert!(metadata(
            &envelope,
            "3.7.0",
            UpdateChannel::Stable,
            "https://evil.example/x",
            &trust
        )
        .is_err());
        for i in 0..2 {
            let mut bad: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
            bad["signatures"][i]["signature"] = serde_json::json!("bad");
            assert!(metadata(
                &serde_json::to_vec(&bad).unwrap(),
                "3.7.0",
                UpdateChannel::Stable,
                url,
                &trust
            )
            .is_err());
        }
        assert!(metadata_url("../evil", UpdateChannel::Stable).is_err());
        assert!(metadata_url("3.07.0", UpdateChannel::Beta).is_err());
        assert!(metadata_url("3.7.0", UpdateChannel::Beta)
            .unwrap()
            .ends_with("/hybrid-beta.json"));
        let mut bounded = Vec::new();
        append_bounded(&mut bounded, b"123", 4).unwrap();
        assert!(append_bounded(&mut bounded, b"45", 4).is_err());
        assert_eq!(&bounded, b"123");
    }
}
