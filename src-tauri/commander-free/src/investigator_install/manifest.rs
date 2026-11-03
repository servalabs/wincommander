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

fn validate_signed_pro(release: &SignedProRelease) -> Result<(), String> {
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

fn canonical_version(version: &str) -> Result<String, String> {
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn signed_manifest_rejects_oversized_chunked_response() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request);
            let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10001\r\n");
            let _ = socket.write_all(&vec![b'x'; MAX_MANIFEST_BYTES + 1]);
            let _ = socket.write_all(b"\r\n0\r\n\r\n");
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        assert!(read_signed_envelope(response)
            .await
            .unwrap_err()
            .contains("size limit"));
        thread.join().unwrap();
    }

    #[test]
    fn signed_pro_record_verifies_and_rejects_tampering() {
        let mut envelope: SignedManifestEnvelope =
            serde_json::from_str(include_str!("../../tests/fixtures/signed-pro-test.json"))
                .unwrap();
        let public_key = include_str!("../../tests/fixtures/signed-pro-test.pub").trim();
        let release = verify_signed_pro_with_key(&envelope, public_key).unwrap();
        assert_eq!(release.version, "3.7.0");
        assert_eq!(release.sha256, "a".repeat(64));
        envelope.payload = envelope.payload.replace(&"a".repeat(64), &"b".repeat(64));
        assert!(verify_signed_pro_with_key(&envelope, public_key).is_err());
        envelope.signature.clear();
        assert!(verify_signed_pro_with_key(&envelope, public_key).is_err());
    }

    #[test]
    fn signed_pro_record_rejects_wrong_scope_and_invalid_fields() {
        let envelope: SignedManifestEnvelope =
            serde_json::from_str(include_str!("../../tests/fixtures/signed-pro-test.json"))
                .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&envelope.payload).unwrap();
        for (field, invalid) in [
            ("format", serde_json::json!("other-product")),
            (
                "url",
                serde_json::json!("https://evil.example/pro/v3.7.0/wincommander-pro.exe"),
            ),
            (
                "url",
                serde_json::json!(
                    "https://winupdates.servalabs.com/pro/v3.7.1/wincommander-pro.exe"
                ),
            ),
            ("sha256", serde_json::json!("not-a-hash")),
            ("size", serde_json::json!(0)),
        ] {
            let old = value[field].clone();
            value[field] = invalid;
            let release: SignedProRelease = serde_json::from_value(value.clone()).unwrap();
            assert!(validate_signed_pro(&release).is_err());
            value[field] = old;
        }
    }

    #[test]
    fn versioned_pro_manifest_url_cannot_escape_approved_path() {
        assert_eq!(
            versioned_manifest_url("3.6.7.0").unwrap(),
            "https://winupdates.servalabs.com/investigator/v3.6.7/latest.json"
        );
        assert_eq!(
            versioned_manifest_url("3.6.7.2").unwrap(),
            "https://winupdates.servalabs.com/investigator/v3.6.7.2/latest.json"
        );
        for invalid in [
            "",
            "3.6",
            "3.6.7/../../pro",
            "3.6.7?x=1",
            "3.6.7.0.1",
            "9999999999999999999999.1.1",
        ] {
            assert!(versioned_manifest_url(invalid).is_err());
        }
    }

    #[test]
    fn wincommander_minisign_envelope_verifies_before_manifest_parsing() {
        let public_key = "untrusted comment: minisign public key\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n";
        let signature = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";
        let envelope = SignedManifestEnvelope {
            payload: "test".to_string(),
            signature: general_purpose::STANDARD.encode(signature),
        };
        let encoded_key = general_purpose::STANDARD.encode(public_key);

        let error = match verify_manifest_with_key(envelope, &encoded_key) {
            Err(error) => error,
            Ok(_) => panic!("non-JSON fixture unexpectedly parsed as a manifest"),
        };
        assert!(error.starts_with("parse:invalid signed manifest payload:"));
    }

    #[test]
    fn manifest_requires_both_expected_artifacts() {
        let manifest: InvestigatorManifest = serde_json::from_str(
            r#"{"version":"1.0.0","artifacts":[{"name":"wincommander-investigator.exe","url":"https://winupdates.servalabs.com/investigator/wincommander-investigator.exe","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1},{"name":"wincommander-pro.exe","url":"https://winupdates.servalabs.com/investigator/wincommander-pro.exe","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","size":1}]}"#,
        )
        .unwrap();
        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn manifest_rejects_urls_outside_investigator_channel() {
        let manifest: InvestigatorManifest = serde_json::from_str(
            r#"{"version":"1.0.0","artifacts":[{"name":"wincommander-investigator.exe","url":"https://winupdates.servalabs.com/pro/wincommander-investigator.exe","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1},{"name":"wincommander-pro.exe","url":"https://winupdates.servalabs.com/investigator/wincommander-pro.exe","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","size":1}]}"#,
        )
        .unwrap();
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn manifest_rejects_reordered_or_noncanonical_artifacts() {
        let mut manifest: InvestigatorManifest = serde_json::from_str(
            r#"{"version":"1.0.0","artifacts":[{"name":"wincommander-investigator.exe","url":"https://winupdates.servalabs.com/investigator/wincommander-investigator.exe","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1},{"name":"wincommander-pro.exe","url":"https://winupdates.servalabs.com/investigator/wincommander-pro.exe","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","size":1}]}"#,
        )
        .unwrap();
        manifest.artifacts.swap(0, 1);
        assert!(validate_manifest(&manifest).is_err());

        manifest.artifacts.swap(0, 1);
        manifest.artifacts[0].sha256.replace_range(0..1, "A");
        assert!(validate_manifest(&manifest).is_err());
    }
}
