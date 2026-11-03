//! Narrow SYSTEM-service writer for the shared WinCommander licence cache.
//!
//! The desktop remains responsible for talking to the licensing Worker and
//! checking the returned device binding. This module lets an ordinary active
//! Windows session persist that already-verified response without giving that
//! account write access to `%ProgramData%\WinCommander`. It accepts only a
//! bounded Ed25519-signed envelope and writes only one fixed file.

use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "experimental-pq-licence")]
#[path = "../../runtime-support/license_hybrid_trust.rs"]
mod hybrid_trust;
#[cfg(feature = "experimental-pq-licence")]
#[path = "license_cache_rollback.rs"]
mod rollback;

const DEFAULT_LICENSE_PUBLIC_KEY_B64: &str = "Z4ulYtrFLpOZYVLGpNo_PlegZsitTmJx2JwfyIqSJpY";
const MAINLINE_APP_ID: &str = "com.servalabs.wincommander";
const MAX_SIGNED_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_SIGNATURE_BYTES: usize = 256;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

/// This is intentionally the sole JSON shape accepted by the service. It is
/// not a path, a user identity, a permission, or an arbitrary file payload.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoreLicenseCacheRequest {
    payload: String,
    signature: String,
    #[serde(default)]
    seats_used: Option<u32>,
    #[serde(default)]
    seat_limit: Option<u32>,
    #[cfg(feature = "experimental-pq-licence")]
    #[serde(default)]
    hybrid: Option<String>,
}

/// The parts of the signed worker claim needed to reject malformed envelopes.
/// Unknown fields deliberately remain forward-compatible with future worker
/// claims; trust comes from the Ed25519 signature, not this projection.
#[derive(Debug, Deserialize)]
struct SignedClaims {
    #[cfg(feature = "experimental-pq-licence")]
    #[serde(default)]
    license_id: Option<String>,
    device_hash: String,
    iat: u64,
    exp: u64,
}

#[derive(Debug, Serialize)]
struct PersistedLicenseCache<'a> {
    token: PersistedTokenEnvelope<'a>,
    last_verified_at: u64,
    seats_used: Option<u32>,
    seat_limit: Option<u32>,
}

#[derive(Debug, Serialize)]
struct PersistedTokenEnvelope<'a> {
    payload: &'a str,
    signature: &'a str,
    #[cfg(feature = "experimental-pq-licence")]
    hybrid: &'a str,
}

/// Validate and persist a signed Worker envelope at the one machine-owned
/// location. Callers never choose a path and the service assigns the refresh
/// timestamp itself.
pub fn store(args: serde_json::Value) -> Result<(), &'static str> {
    let request: StoreLicenseCacheRequest =
        serde_json::from_value(args).map_err(|_| "license cache request is invalid")?;
    let public_key = service_public_key().ok_or("license verification is unavailable")?;
    #[cfg(not(feature = "experimental-pq-licence"))]
    validate_request(&request, public_key)?;
    #[cfg(feature = "experimental-pq-licence")]
    let trust = hybrid_trust::configured()?;
    #[cfg(feature = "experimental-pq-licence")]
    validate_dual_request(&request, public_key, &trust.policy())?;

    let persisted = PersistedLicenseCache {
        token: PersistedTokenEnvelope {
            payload: &request.payload,
            signature: &request.signature,
            #[cfg(feature = "experimental-pq-licence")]
            hybrid: request.hybrid.as_deref().ok_or("hybrid license is required")?,
        },
        // The caller can replay a signed token. Only its signed issuance time
        // proves when the server last issued it; storing the current time
        // would let a replay repeatedly restart the offline grace period.
        last_verified_at: signed_verification_time(&request.payload, now_unix())?,
        seats_used: request.seats_used,
        seat_limit: request.seat_limit,
    };
    let bytes =
        serde_json::to_vec_pretty(&persisted).map_err(|_| "license cache could not be encoded")?;
    let path = machine_license_cache_path()?;
    #[cfg(feature = "experimental-pq-licence")]
    {
        rollback::write(&path, &request, &bytes, public_key, &trust.policy())
    }
    #[cfg(not(feature = "experimental-pq-licence"))]
    {
        atomic_write(&path, &bytes).map_err(|_| "machine license cache is unavailable")
    }
}

/// Read-only confirmation that the current signed cache matches protected history.
#[cfg(feature = "experimental-pq-licence")]
pub fn verify_current(args: serde_json::Value) -> Result<(), &'static str> {
    let request: StoreLicenseCacheRequest =
        serde_json::from_value(args).map_err(|_| "license cache request is invalid")?;
    let public_key = service_public_key().ok_or("license verification is unavailable")?;
    let trust = hybrid_trust::configured()?;
    rollback::verify_current(&machine_license_cache_path()?, &request, public_key, &trust.policy())
}

#[cfg(not(feature = "experimental-pq-licence"))]
pub fn verify_current(_args: serde_json::Value) -> Result<(), &'static str> {
    Err("hybrid license verification is unavailable")
}

fn signed_verification_time(payload: &str, now: u64) -> Result<u64, &'static str> {
    let claims: SignedClaims =
        serde_json::from_str(payload).map_err(|_| "license cache payload is invalid")?;
    // A server clock ahead of the device must not create a future grace start.
    Ok(claims.iat.min(now))
}

fn validate_request(
    request: &StoreLicenseCacheRequest,
    public_key_b64: &str,
) -> Result<(), &'static str> {
    if request.payload.is_empty() || request.payload.len() > MAX_SIGNED_PAYLOAD_BYTES {
        return Err("license cache request is invalid");
    }
    if request.signature.is_empty() || request.signature.len() > MAX_SIGNATURE_BYTES {
        return Err("license cache request is invalid");
    }

    let key_bytes =
        decode_b64_any(public_key_b64).map_err(|_| "license verification is unavailable")?;
    let signature_bytes =
        decode_b64_any(&request.signature).map_err(|_| "license cache signature is invalid")?;
    let key_arr: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "license verification is unavailable")?;
    let signature_arr: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "license cache signature is invalid")?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_arr).map_err(|_| "license verification is unavailable")?;
    let signature = Signature::from_bytes(&signature_arr);
    verifying_key
        .verify(request.payload.as_bytes(), &signature)
        .map_err(|_| "license cache signature is invalid")?;

    validate_claims(request.payload.as_bytes())
}

fn validate_claims(payload: &[u8]) -> Result<(), &'static str> {
    if payload.is_empty() || payload.len() > MAX_SIGNED_PAYLOAD_BYTES {
        return Err("license cache payload is invalid");
    }
    let claims: SignedClaims =
        serde_json::from_slice(payload).map_err(|_| "license cache payload is invalid")?;
    if claims.device_hash.trim().is_empty() || claims.exp == 0 || claims.iat > claims.exp {
        return Err("license cache payload is invalid");
    }
    Ok(())
}

/// Validate the experimental hybrid proof before the service persists it.
/// Rollback protection and every licence reader must be implemented before use.
#[cfg(feature = "experimental-pq-licence")]
fn validate_hybrid_request(
    envelope: &[u8],
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
) -> Result<wincmd_pq_experiment::jws::Verified, &'static str> {
    if envelope.is_empty()
        || envelope.len() > 32 * 1024
        || trust.purpose != wincmd_pq_experiment::Purpose::Licence
    {
        return Err("license cache request is invalid");
    }
    let verified = wincmd_pq_experiment::jws::verify(envelope, trust)
        .map_err(|_| "license cache signature is invalid")?;
    validate_claims(&verified.payload)?;
    Ok(verified)
}

#[cfg(feature = "experimental-pq-licence")]
fn validate_dual_request(
    request: &StoreLicenseCacheRequest,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
) -> Result<(), &'static str> {
    validate_request(request, public_key)?;
    let hybrid = request.hybrid.as_deref().ok_or("hybrid license is required")?;
    let verified = validate_hybrid_request(hybrid.as_bytes(), trust)?;
    if verified.payload != request.payload.as_bytes() {
        return Err("hybrid license does not match the classical license");
    }
    Ok(())
}

fn service_public_key() -> Option<&'static str> {
    option_env!("WINCMD_LICENSE_PUBLIC_KEY")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("LICENSE_PUBLIC_KEY_B64").filter(|value| !value.trim().is_empty()))
        .or_else(|| {
            (option_env!("WINCMD_APP_ID").unwrap_or(MAINLINE_APP_ID) == MAINLINE_APP_ID)
                .then_some(DEFAULT_LICENSE_PUBLIC_KEY_B64)
        })
}

fn decode_b64_any(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .or_else(|_| general_purpose::URL_SAFE.decode(input))
        .or_else(|_| general_purpose::STANDARD.decode(input))
}

fn machine_license_cache_path() -> Result<PathBuf, &'static str> {
    let root = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .ok_or("machine license cache is unavailable")?
        .join("WinCommander");
    let root_metadata =
        fs::symlink_metadata(&root).map_err(|_| "machine license cache is unavailable")?;
    if !root_metadata.is_dir()
        || root_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err("machine license cache is unavailable");
    }

    let path = root.join("license_cache.json");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("machine license cache is unavailable");
        }
    }
    Ok(path)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let temp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
    ));

    let write_result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Source and destination are siblings on the same protected volume.
        // Windows replaces the existing file atomically for this operation.
        fs::rename(&temp, path)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[cfg(feature = "experimental-pq-licence")]
    #[test]
    fn worker_issued_hybrid_licence_is_accepted_by_service() {
        use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};
        let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/workers-hybrid-licence.json")).unwrap();
        let ed: [u8; 32] = general_purpose::URL_SAFE_NO_PAD.decode(fixture["publicEd"].as_str().unwrap()).unwrap().try_into().unwrap();
        let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = general_purpose::URL_SAFE_NO_PAD.decode(fixture["publicPq"].as_str().unwrap()).unwrap().try_into().unwrap();
        let trust = jws::Trust { purpose: Purpose::Licence, minimum_epoch: 7, ed_key_id: "ed-key", ed25519_public: &ed, pq_key_id: "pq-key", ml_dsa_65_public: &pq };
        let mut request = StoreLicenseCacheRequest {
            payload: fixture["payload"].as_str().unwrap().to_owned(),
            signature: fixture["signature"].as_str().unwrap().to_owned(),
            hybrid: Some(fixture["hybrid"].as_str().unwrap().to_owned()),
            seats_used: None, seat_limit: None,
        };
        let public = fixture["publicClassical"].as_str().unwrap();
        assert!(validate_dual_request(&request, public, &trust).is_ok());
        let claims: SignedClaims = serde_json::from_str(&request.payload).unwrap();
        assert_eq!(
            signed_verification_time(&request.payload, claims.iat + 1_000_000).unwrap(),
            claims.iat
        );
        request.hybrid = None;
        assert!(validate_dual_request(&request, public, &trust).is_err());
        request.hybrid = Some(fixture["hybrid"].as_str().unwrap().to_owned());
        request.payload.push(' ');
        assert!(validate_dual_request(&request, public, &trust).is_err());
    }

    fn signed_request() -> (StoreLicenseCacheRequest, String) {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let payload = serde_json::json!({
            "license_id": "test-license",
            "device_hash": "device-test",
            "plan": "pro",
            "features": ["paid"],
            "iat": 1_000_u64,
            "exp": 2_000_u64,
        })
        .to_string();
        let signature = general_purpose::URL_SAFE_NO_PAD
            .encode(signing_key.sign(payload.as_bytes()).to_bytes());
        let public_key =
            general_purpose::URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes());
        (
            StoreLicenseCacheRequest {
                payload,
                signature,
                seats_used: Some(1),
                seat_limit: Some(3),
                #[cfg(feature = "experimental-pq-licence")]
                hybrid: None,
            },
            public_key,
        )
    }

    #[test]
    fn accepts_only_a_valid_signed_worker_envelope() {
        let (request, public_key) = signed_request();
        assert!(validate_request(&request, &public_key).is_ok());

        let mut forged = request;
        forged.payload.push('x');
        assert_eq!(
            validate_request(&forged, &public_key),
            Err("license cache signature is invalid")
        );
    }

    #[test]
    fn signed_issuance_bounds_cache_freshness_for_replay_and_clock_skew() {
        let (request, public_key) = signed_request();
        validate_request(&request, &public_key).unwrap();
        let claims: SignedClaims = serde_json::from_str(&request.payload).unwrap();
        // Replaying the same authenticated token later never refreshes it.
        assert_eq!(
            signed_verification_time(&request.payload, claims.iat + 10).unwrap(),
            claims.iat
        );
        assert_eq!(
            signed_verification_time(&request.payload, claims.iat + 1_000_000).unwrap(),
            claims.iat
        );
        // A behind-clock device cannot grant future-dated offline grace.
        assert_eq!(
            signed_verification_time(&request.payload, claims.iat - 1).unwrap(),
            claims.iat - 1
        );
        // A normally issued replacement advances freshness to its signed time.
        let mut renewal = request;
        renewal.payload = format!(
            r#"{{"device_hash":"device","iat":{},"exp":{}}}"#,
            claims.iat + 20,
            claims.iat + 100
        );
        renewal.signature = general_purpose::URL_SAFE_NO_PAD.encode(
            SigningKey::from_bytes(&[7; 32]).sign(renewal.payload.as_bytes()).to_bytes(),
        );
        validate_request(&renewal, &public_key).unwrap();
        assert_eq!(
            signed_verification_time(&renewal.payload, claims.iat + 21).unwrap(),
            claims.iat + 20
        );
    }

    #[test]
    fn rejects_malformed_or_unbounded_cache_requests() {
        let (mut request, public_key) = signed_request();
        request.payload = "{}".repeat(MAX_SIGNED_PAYLOAD_BYTES);
        assert_eq!(
            validate_request(&request, &public_key),
            Err("license cache request is invalid")
        );

        let malformed = serde_json::json!({
            "payload": "x",
            "signature": "y",
            "path": "C:\\\\not-accepted"
        });
        assert!(serde_json::from_value::<StoreLicenseCacheRequest>(malformed).is_err());
    }

    #[cfg(feature = "experimental-pq-licence")]
    #[test]
    fn hybrid_validation_checks_both_signatures_and_legacy_claim_constraints() {
        use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/hybrid-licence-test.json")).unwrap();
        let ed: [u8; 32] = general_purpose::URL_SAFE_NO_PAD
            .decode(fixture["publicEd"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = general_purpose::URL_SAFE_NO_PAD
            .decode(fixture["publicPq"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let trust = jws::Trust {
            purpose: Purpose::Licence,
            minimum_epoch: 7,
            ed_key_id: "ed-key",
            ed25519_public: &ed,
            pq_key_id: "pq-key",
            ml_dsa_65_public: &pq,
        };
        let token = serde_json::to_vec(&fixture["jws"]).unwrap();
        assert_eq!(validate_hybrid_request(&token, &trust).unwrap().epoch, 7);
        for index in 0..2 {
            let mut changed = fixture["jws"].clone();
            let mut signature = general_purpose::URL_SAFE_NO_PAD
                .decode(changed["signatures"][index]["signature"].as_str().unwrap())
                .unwrap();
            signature[0] ^= 1;
            changed["signatures"][index]["signature"] =
                general_purpose::URL_SAFE_NO_PAD.encode(signature).into();
            assert_eq!(
                validate_hybrid_request(&serde_json::to_vec(&changed).unwrap(), &trust).err(),
                Some("license cache signature is invalid")
            );
        }
        let stale = jws::Trust {
            minimum_epoch: 8,
            ..trust
        };
        assert_eq!(
            validate_hybrid_request(&token, &stale).err(),
            Some("license cache signature is invalid")
        );
        assert_eq!(
            validate_hybrid_request(&vec![b' '; 32 * 1024 + 1], &stale).err(),
            Some("license cache request is invalid")
        );
        assert_eq!(
            validate_claims(br#"{"device_hash":"","iat":1,"exp":2}"#),
            Err("license cache payload is invalid")
        );
        assert_eq!(
            validate_claims(br#"{"device_hash":"x","iat":3,"exp":2}"#),
            Err("license cache payload is invalid")
        );
    }

    #[cfg(feature = "experimental-pq-licence")]
    #[test]
    fn missing_hybrid_trust_cannot_write_a_classical_only_cache() {
        if option_env!("WINCMD_PQ_LIC_ED_KEY").is_some() { return; }
        assert!(hybrid_trust::configured().is_err());
        let (request, _) = signed_request();
        let args = serde_json::json!({"payload":request.payload,"signature":request.signature});
        assert!(store(args).is_err());
    }

    #[cfg(feature = "experimental-pq-licence")]
    #[test]
    fn service_requires_matching_classical_and_hybrid_licences_before_cache_write() {
        use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};
        let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/hybrid-licence-test.json")).unwrap();
        let ed: [u8; 32] = general_purpose::URL_SAFE_NO_PAD.decode(fixture["publicEd"].as_str().unwrap()).unwrap().try_into().unwrap();
        let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = general_purpose::URL_SAFE_NO_PAD.decode(fixture["publicPq"].as_str().unwrap()).unwrap().try_into().unwrap();
        let trust = jws::Trust { purpose: Purpose::Licence, minimum_epoch: 7, ed_key_id: "ed-key", ed25519_public: &ed, pq_key_id: "pq-key", ml_dsa_65_public: &pq };
        let classic_key = SigningKey::from_bytes(&[7;32]);
        let public = general_purpose::URL_SAFE_NO_PAD.encode(classic_key.verifying_key().to_bytes());
        let payload = String::from_utf8(general_purpose::URL_SAFE_NO_PAD.decode(fixture["jws"]["payload"].as_str().unwrap()).unwrap()).unwrap();
        let mut request = StoreLicenseCacheRequest { signature: general_purpose::URL_SAFE_NO_PAD.encode(classic_key.sign(payload.as_bytes()).to_bytes()), payload, seats_used: None, seat_limit: None, hybrid: Some(serde_json::to_string(&fixture["jws"]).unwrap()) };
        assert!(validate_dual_request(&request, &public, &trust).is_ok());
        let persisted = PersistedLicenseCache { token: PersistedTokenEnvelope { payload: &request.payload, signature: &request.signature, hybrid: request.hybrid.as_deref().unwrap() }, last_verified_at: 123, seats_used: None, seat_limit: None };
        let json = serde_json::to_value(persisted).unwrap();
        assert_eq!(json["token"]["hybrid"].as_str(), request.hybrid.as_deref());
        let original = request.hybrid.take().unwrap();
        assert_eq!(validate_dual_request(&request, &public, &trust), Err("hybrid license is required"));
        request.hybrid = Some(original.clone());
        request.payload.push(' ');
        request.signature = general_purpose::URL_SAFE_NO_PAD.encode(classic_key.sign(request.payload.as_bytes()).to_bytes());
        assert_eq!(validate_dual_request(&request, &public, &trust), Err("hybrid license does not match the classical license"));
        request.payload.pop();
        request.signature = general_purpose::URL_SAFE_NO_PAD.encode(classic_key.sign(request.payload.as_bytes()).to_bytes());
        request.hybrid = Some(format!("{original}x"));
        assert!(validate_dual_request(&request, &public, &trust).is_err());
    }

    #[cfg(feature = "experimental-pq-licence")]
    #[test]
    fn hybrid_signature_does_not_make_malformed_claims_valid() {
        use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/hybrid-licence-invalid-test.json"))
                .unwrap();
        let ed: [u8; 32] = general_purpose::URL_SAFE_NO_PAD
            .decode(fixture["publicEd"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN] = general_purpose::URL_SAFE_NO_PAD
            .decode(fixture["publicPq"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let trust = jws::Trust {
            purpose: Purpose::Licence,
            minimum_epoch: 7,
            ed_key_id: "ed-key",
            ed25519_public: &ed,
            pq_key_id: "pq-key",
            ml_dsa_65_public: &pq,
        };
        let token = serde_json::to_vec(&fixture["jws"]).unwrap();
        assert!(jws::verify(&token, &trust).is_ok());
        assert_eq!(
            validate_hybrid_request(&token, &trust).err(),
            Some("license cache payload is invalid")
        );
    }
}
