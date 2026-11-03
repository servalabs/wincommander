use base64::{engine::general_purpose, Engine as _};
use minisign_verify::{PublicKey, Signature};

const MANIFEST_PATH: &str = "/investigator/latest.json";
const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const INVESTIGATOR_BINARY_NAME: &str = "wincommander-investigator.exe";
pub const PRO_SIDECAR_NAME: &str = "wincommander-pro.exe";

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedManifestEnvelope {
    pub payload: String,
    pub signature: String,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigatorArtifact {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigatorManifest {
    pub version: String,
    pub artifacts: Vec<InvestigatorArtifact>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignedProRelease {
    pub format: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

fn manifest_url() -> String {
    format!(
        "https://{}{}",
        crate::pro_install::ALLOWED_UPDATE_HOST,
        MANIFEST_PATH
    )
}

fn manifest_public_key_b64() -> Result<String, String> {
    let config: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json"))
        .map_err(|error| format!("configuration:invalid embedded Tauri config: {error}"))?;
    config
        .pointer("/plugins/updater/pubkey")
        .and_then(serde_json::Value::as_str)
        .filter(|key| !key.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| "configuration:WinCommander updater public key is missing".to_string())
}

fn decode_base64(value: &str) -> Result<Vec<u8>, String> {
    general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| general_purpose::STANDARD.decode(value))
        .map_err(|e| format!("validation:invalid base64: {}", e))
}

fn verify_manifest_with_key(
    envelope: SignedManifestEnvelope,
    public_key_b64: &str,
) -> Result<InvestigatorManifest, String> {
    verify_signature_with_key(&envelope, public_key_b64)?;
    let manifest = serde_json::from_str(&envelope.payload)
        .map_err(|e| format!("parse:invalid signed manifest payload: {}", e))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn verify_signature_with_key(
    envelope: &SignedManifestEnvelope,
    public_key_b64: &str,
) -> Result<(), String> {
    let key_text = String::from_utf8(decode_base64(public_key_b64)?)
        .map_err(|_| "validation:WinCommander updater public key is not UTF-8".to_string())?;
    let signature_text = String::from_utf8(decode_base64(&envelope.signature)?)
        .map_err(|_| "validation:release manifest signature is not UTF-8".to_string())?;
    let public_key = PublicKey::decode(&key_text)
        .map_err(|error| format!("validation:invalid WinCommander updater public key: {error}"))?;
    let signature = Signature::decode(&signature_text)
        .map_err(|error| format!("validation:invalid release manifest signature: {error}"))?;
    public_key
        .verify(envelope.payload.as_bytes(), &signature, false)
        .map_err(|_| "signature:release manifest signature is invalid.".to_string())?;
    Ok(())
}

pub fn verify_manifest(envelope: &SignedManifestEnvelope) -> Result<InvestigatorManifest, String> {
    verify_manifest_with_key(envelope.clone(), &manifest_public_key_b64()?)
}

pub(crate) fn verify_signed_pro(
    envelope: &SignedManifestEnvelope,
) -> Result<SignedProRelease, String> {
    verify_signed_pro_with_key(envelope, &manifest_public_key_b64()?)
}

fn verify_signed_pro_with_key(
    envelope: &SignedManifestEnvelope,
    key: &str,
) -> Result<SignedProRelease, String> {
    verify_signature_with_key(envelope, key)?;
    let release: SignedProRelease =
        serde_json::from_str(&envelope.payload).map_err(|_| "parse:invalid signed Pro release")?;
    validate_signed_pro(&release)?;
    Ok(release)
}

pub(crate) fn validate_signed_pro(release: &SignedProRelease) -> Result<(), String> {
    let canonical = canonical_version(&release.version)?;
    let expected_url = format!(
        "https://{}/pro/v{canonical}/wincommander-pro.exe",
        crate::pro_install::ALLOWED_UPDATE_HOST
    );
    if release.format != "wincommander-pro-release-v1"
        || release.url != expected_url
        || release.size == 0
        || release.size > MAX_ARTIFACT_BYTES
        || release.sha256.len() != 64
        || !release
            .sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("validation:invalid signed Pro release".into());
    }
    Ok(())
}

pub fn validate_manifest(manifest: &InvestigatorManifest) -> Result<(), String> {
    if manifest.version.trim().is_empty() || manifest.version.len() > 128 {
        return Err("validation:manifest version is invalid".to_string());
    }
    if manifest.artifacts.len() != 2 {
        return Err("validation:manifest must contain exactly two artifacts".to_string());
    }
    if manifest.artifacts[0].name != INVESTIGATOR_BINARY_NAME
        || manifest.artifacts[1].name != PRO_SIDECAR_NAME
    {
        return Err("validation:manifest artifact order is invalid".to_string());
    }
    for expected_name in [INVESTIGATOR_BINARY_NAME, PRO_SIDECAR_NAME] {
        if manifest
            .artifacts
            .iter()
            .filter(|artifact| artifact.name == expected_name)
            .count()
            != 1
        {
            return Err(format!(
                "validation:manifest must contain one {} artifact",
                expected_name
            ));
        }
    }
    for artifact in &manifest.artifacts {
        if artifact.size == 0 || artifact.size > MAX_ARTIFACT_BYTES {
            return Err(format!("validation:{} has an invalid size", artifact.name));
        }
        if artifact.sha256.len() != 64
            || !artifact
                .sha256
                .chars()
                .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
        {
            return Err(format!(
                "validation:{} has an invalid sha256",
                artifact.name
            ));
        }
        let parsed = reqwest::Url::parse(&artifact.url)
            .map_err(|e| format!("validation:{} has an invalid url: {}", artifact.name, e))?;
        if parsed.scheme() != "https"
            || !parsed.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case(crate::pro_install::ALLOWED_UPDATE_HOST)
            })
            || !parsed.path().starts_with("/investigator/")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(format!(
                "validation:{} url is outside the Investigator channel",
                artifact.name
            ));
        }
    }
    Ok(())
}

pub async fn fetch_manifest() -> Result<(SignedManifestEnvelope, InvestigatorManifest), String> {
    fetch_verified_manifest(None).await
}

/// Fetch the matching version first. Older published releases only have a
/// latest pointer, which is usable only after the caller checks its version.
pub(crate) async fn fetch_manifest_for_pro(
    version: &str,
) -> Result<(SignedManifestEnvelope, InvestigatorManifest), String> {
    let url = versioned_manifest_url(version)?;
    fetch_verified_manifest(Some(url)).await
}

fn versioned_manifest_url(version: &str) -> Result<String, String> {
    let canonical = canonical_version(version)?;
    Ok(format!(
        "https://{}/investigator/v{canonical}/latest.json",
        crate::pro_install::ALLOWED_UPDATE_HOST
    ))
}

pub(crate) fn canonical_version(version: &str) -> Result<String, String> {
    let parts: Vec<_> = version.split('.').collect();
    if !(parts.len() == 3 || parts.len() == 4)
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("validation:invalid Pro release version".into());
    }
    let numbers: Vec<u64> = parts
        .iter()
        .map(|part| part.parse())
        .collect::<Result<_, _>>()
        .map_err(|_| "validation:invalid Pro release version")?;
    let mut canonical = format!("{}.{}.{}", numbers[0], numbers[1], numbers[2]);
    if numbers.get(3).is_some_and(|revision| *revision != 0) {
        canonical.push_str(&format!(".{}", numbers[3]));
    }
    Ok(canonical)
}

pub(crate) async fn fetch_signed_pro(
    version: &str,
) -> Result<Option<SignedManifestEnvelope>, String> {
    let canonical = canonical_version(version)?;
    let url = format!(
        "https://{}/pro/v{canonical}/signed.json",
        crate::pro_install::ALLOWED_UPDATE_HOST
    );
    let client = crate::net::required_hybrid_http_client(std::time::Duration::from_secs(15))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("network:{e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!(
            "network:signed Pro release returned HTTP {}",
            response.status()
        ));
    }
    let envelope = read_signed_envelope(response).await?;
    verify_signed_pro(&envelope)?;
    Ok(Some(envelope))
}

async fn fetch_verified_manifest(
    versioned_url: Option<String>,
) -> Result<(SignedManifestEnvelope, InvestigatorManifest), String> {
    let client = crate::net::required_hybrid_http_client(std::time::Duration::from_secs(15))
        .map_err(|e| format!("network:{}", e))?;
    let url = versioned_url.clone().unwrap_or_else(manifest_url);
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("network:{}", e))?;
    let response = if versioned_url.is_some() && response.status() == reqwest::StatusCode::NOT_FOUND
    {
        client
            .get(manifest_url())
            .send()
            .await
            .map_err(|e| format!("network:{}", e))?
    } else {
        response
    };
    if !response.status().is_success() {
        return Err(format!(
            "network:manifest returned HTTP {}",
            response.status()
        ));
    }
    let envelope = read_signed_envelope(response).await?;
    let manifest = verify_manifest(&envelope)?;
    Ok((envelope, manifest))
}

async fn read_signed_envelope(
    mut response: reqwest::Response,
) -> Result<SignedManifestEnvelope, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MANIFEST_BYTES as u64)
    {
        return Err("validation:signed manifest exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("network:{e}"))? {
        if chunk.len() > MAX_MANIFEST_BYTES.saturating_sub(bytes.len()) {
            return Err("validation:signed manifest exceeds size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("parse:invalid manifest envelope: {e}"))
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
