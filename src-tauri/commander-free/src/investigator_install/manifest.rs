use base64::{engine::general_purpose, Engine as _};
use minisign_verify::{PublicKey, Signature};

const MANIFEST_PATH: &str = "/investigator/latest.json";
const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const INVESTIGATOR_BINARY_NAME: &str = "wincommander-investigator.exe";
pub const PRO_SIDECAR_NAME: &str = "wincommander-pro.exe";

#[derive(Clone, serde::Deserialize, serde::Serialize)]
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
    let key_text = String::from_utf8(decode_base64(public_key_b64)?)
        .map_err(|_| "validation:WinCommander updater public key is not UTF-8".to_string())?;
    let signature_text = String::from_utf8(decode_base64(&envelope.signature)?)
        .map_err(|_| "validation:Investigator manifest signature is not UTF-8".to_string())?;
    let public_key = PublicKey::decode(&key_text)
        .map_err(|error| format!("validation:invalid WinCommander updater public key: {error}"))?;
    let signature = Signature::decode(&signature_text)
        .map_err(|error| format!("validation:invalid Investigator manifest signature: {error}"))?;
    public_key
        .verify(envelope.payload.as_bytes(), &signature, false)
        .map_err(|_| "signature:Investigator manifest signature is invalid.".to_string())?;
    let manifest = serde_json::from_str(&envelope.payload)
        .map_err(|e| format!("parse:invalid signed manifest payload: {}", e))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn verify_manifest(envelope: &SignedManifestEnvelope) -> Result<InvestigatorManifest, String> {
    verify_manifest_with_key(envelope.clone(), &manifest_public_key_b64()?)
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
    let client = crate::net::doh_http_client().map_err(|e| format!("network:{}", e))?;
    let response = client
        .get(manifest_url())
        .send()
        .await
        .map_err(|e| format!("network:{}", e))?;
    if !response.status().is_success() {
        return Err(format!(
            "network:manifest returned HTTP {}",
            response.status()
        ));
    }
    let envelope = response
        .json::<SignedManifestEnvelope>()
        .await
        .map_err(|e| format!("parse:invalid manifest envelope: {}", e))?;
    let manifest = verify_manifest(&envelope)?;
    Ok((envelope, manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

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
