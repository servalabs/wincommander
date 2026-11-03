use super::*;

pub(super) struct HybridProTrust {
    ed_key: [u8; 32],
    pq_key: [u8; wincmd_pq_experiment::ML_DSA_65_PUBLIC_KEY_LEN],
    ed_id: String,
    pq_id: String,
    minimum_epoch: u64,
}

pub(super) fn hybrid_pro_trust() -> Result<HybridProTrust, String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let decode = |value: Option<&str>| -> Result<Vec<u8>, String> {
        URL_SAFE_NO_PAD
            .decode(value.ok_or("Hybrid Pro release trust is not configured")?)
            .map_err(|_| "Hybrid Pro release key is invalid".to_string())
    };
    let ed_key = decode(option_env!("WINCMD_PQ_PRO_ED_KEY"))?
        .try_into()
        .map_err(|_| "Hybrid Pro release key length is invalid")?;
    let pq_key = decode(option_env!("WINCMD_PQ_PRO_MLDSA_KEY"))?
        .try_into()
        .map_err(|_| "Hybrid Pro release key length is invalid")?;
    let ed_id = option_env!("WINCMD_PQ_PRO_ED_ID")
        .filter(|value| !value.is_empty())
        .ok_or("Hybrid Pro release trust is not configured")?
        .to_string();
    let pq_id = option_env!("WINCMD_PQ_PRO_MLDSA_ID")
        .filter(|value| !value.is_empty())
        .ok_or("Hybrid Pro release trust is not configured")?
        .to_string();
    let minimum_epoch = option_env!("WINCMD_PQ_PRO_EPOCH")
        .and_then(|value| value.parse().ok())
        .filter(|epoch: &u64| *epoch > 0)
        .ok_or("Hybrid Pro release epoch is invalid")?;
    Ok(HybridProTrust {
        ed_key,
        pq_key,
        ed_id,
        pq_id,
        minimum_epoch,
    })
}

pub(super) fn verify_hybrid_pro(
    envelope: &[u8],
    trust: &HybridProTrust,
    version: Option<&str>,
    sha256: &str,
    expected_url: Option<&str>,
) -> Result<(), String> {
    use wincmd_pq_experiment::{jws, Purpose};
    let policy = jws::Trust {
        purpose: Purpose::Release,
        minimum_epoch: trust.minimum_epoch,
        ed_key_id: &trust.ed_id,
        ed25519_public: &trust.ed_key,
        pq_key_id: &trust.pq_id,
        ml_dsa_65_public: &trust.pq_key,
    };
    let verified =
        jws::verify(envelope, &policy).map_err(|_| "Hybrid Pro release signature is invalid")?;
    let release: crate::investigator_install::manifest::SignedProRelease =
        serde_json::from_slice(&verified.payload)
            .map_err(|_| "Hybrid Pro release payload is invalid")?;
    crate::investigator_install::manifest::validate_signed_pro(&release)?;
    if !signed_pro_release_matches(&release, version, sha256)
        || expected_url.is_some_and(|url| release.url != url)
    {
        return Err("Hybrid Pro release does not match the requested artifact".into());
    }
    Ok(())
}

pub(super) async fn fetch_hybrid_pro(version: &str) -> Result<String, String> {
    let version = crate::investigator_install::manifest::canonical_version(version)?;
    let url = format!("https://{ALLOWED_UPDATE_HOST}/pro/v{version}/hybrid.json");
    let client = crate::net::required_hybrid_http_client(std::time::Duration::from_secs(15))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("network:{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "network:hybrid release returned HTTP {}",
            response.status()
        ));
    }
    const LIMIT: usize = 32 * 1024;
    if response
        .content_length()
        .is_some_and(|length| length > LIMIT as u64)
    {
        return Err("Hybrid Pro release exceeds size limit".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("network:{e}"))? {
        if chunk.len() > LIMIT.saturating_sub(body.len()) {
            return Err("Hybrid Pro release exceeds size limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| "Hybrid Pro release is not UTF-8".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hybrid_pro_requires_both_signatures_and_binds_release() {
        use super::*;
        use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        use ed25519_dalek::{Signer, SigningKey};

        let ed = SigningKey::from_bytes(&[42; 32]);
        let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
        let trust = HybridProTrust {
            ed_key: ed.verifying_key().to_bytes(),
            pq_key: pq.public_key().as_ref().try_into().unwrap(),
            ed_id: "ed-key".into(),
            pq_id: "pq-key".into(),
            minimum_epoch: 7,
        };
        let url = "https://winupdates.servalabs.com/pro/v3.7.0/wincommander-pro.exe";
        let hash = "a".repeat(64);
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "format":"wincommander-pro-release-v1", "version":"3.7.0",
                "url":url, "sha256":hash, "size":1234
            }))
            .unwrap(),
        );
        let header = |alg: &str, kid: &str| {
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&serde_json::json!({
            "alg":alg, "kid":kid, "typ":"application/wincmd-experimental+jws",
            "crit":["profile","purpose","epoch"], "profile":"experimental-ed25519-mldsa65-v1",
            "purpose":"release", "epoch":7
        })).unwrap())
        };
        let ed_header = header("Ed25519", "ed-key");
        let pq_header = header("ML-DSA-65", "pq-key");
        let ed_signature = ed.sign(format!("{ed_header}.{payload}").as_bytes());
        let mut pq_signature = vec![0; 3309];
        pq.sign(
            format!("{pq_header}.{payload}").as_bytes(),
            &mut pq_signature,
        )
        .unwrap();
        let envelope = serde_json::json!({"payload":payload, "signatures":[
            {"protected":ed_header,"signature":URL_SAFE_NO_PAD.encode(ed_signature.to_bytes())},
            {"protected":pq_header,"signature":URL_SAFE_NO_PAD.encode(pq_signature)}
        ]});
        let verify =
            |value: &serde_json::Value, policy: &HybridProTrust, version, digest: &str, target| {
                verify_hybrid_pro(
                    &serde_json::to_vec(value).unwrap(),
                    policy,
                    version,
                    digest,
                    target,
                )
            };
        assert!(verify(&envelope, &trust, Some("3.7.0"), &hash, Some(url)).is_ok());
        for index in 0..2 {
            let mut changed = envelope.clone();
            changed["signatures"][index]["signature"] = serde_json::json!("invalid");
            assert!(verify(&changed, &trust, Some("3.7.0"), &hash, Some(url)).is_err());
        }
        let mut missing = envelope.clone();
        missing["signatures"].as_array_mut().unwrap().pop();
        assert!(verify(&missing, &trust, Some("3.7.0"), &hash, Some(url)).is_err());
        assert!(verify(&envelope, &trust, Some("3.7.1"), &hash, Some(url)).is_err());
        assert!(verify(&envelope, &trust, Some("3.7.0"), &"b".repeat(64), Some(url)).is_err());
        assert!(verify(
            &envelope,
            &trust,
            Some("3.7.0"),
            &hash,
            Some("https://evil.example/")
        )
        .is_err());
        let stale = HybridProTrust {
            minimum_epoch: 8,
            ..trust
        };
        assert!(verify(&envelope, &stale, Some("3.7.0"), &hash, Some(url)).is_err());
    }

    #[test]
    fn hybrid_mode_does_not_accept_classical_only_metadata() {
        use super::*;
        let envelope = serde_json::from_str(include_str!(
            "../../tests/fixtures/pro-signed-release-3.6.7.json"
        ))
        .unwrap();
        let hash = "1681e4a9005ef22b4bf011510ac7a7d1cc6a3f8f16ae57cb31dbb1153279c45e";
        let metadata = ProInstallMetadata {
            version: Some("3.6.7".into()),
            sha256: hash.into(),
            signed_manifest: Some(envelope),
            hybrid_manifest: None,
        };
        assert!(!metadata_has_signed_hash(&metadata, hash));
    }

    #[test]
    fn hybrid_fixture_is_rechecked_from_persisted_metadata() {
        use super::*;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/hybrid-pro-disposable.json"
        ))
        .unwrap();
        let hash = "a".repeat(64);
        let mut metadata = ProInstallMetadata {
            version: Some("3.7.0".into()),
            sha256: hash.clone(),
            signed_manifest: None,
            hybrid_manifest: Some(serde_json::to_string(&fixture["jws"]).unwrap()),
        };
        // CI sets these explicitly from the disposable fixture. A feature build
        // without provisioned trust must fail closed rather than accept legacy.
        let fixture_trusted = option_env!("WINCMD_PQ_PRO_ED_KEY") == fixture["publicEd"].as_str()
            && option_env!("WINCMD_PQ_PRO_MLDSA_KEY") == fixture["publicPq"].as_str()
            && option_env!("WINCMD_PQ_PRO_ED_ID") == Some("ed-key")
            && option_env!("WINCMD_PQ_PRO_MLDSA_ID") == Some("pq-key")
            && option_env!("WINCMD_PQ_PRO_EPOCH")
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|v| (1..=7).contains(&v));
        if !fixture_trusted {
            assert!(!metadata_has_signed_hash(&metadata, &hash));
            return;
        }
        let restored: ProInstallMetadata =
            serde_json::from_slice(&serde_json::to_vec(&metadata).unwrap()).unwrap();
        assert!(metadata_has_signed_hash(&restored, &hash));
        metadata.hybrid_manifest.as_mut().unwrap().push('x');
        assert!(!metadata_has_signed_hash(&metadata, &hash));
    }
}
