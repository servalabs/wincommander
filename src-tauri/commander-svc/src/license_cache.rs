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
}

/// The parts of the signed worker claim needed to reject malformed envelopes.
/// Unknown fields deliberately remain forward-compatible with future worker
/// claims; trust comes from the Ed25519 signature, not this projection.
#[derive(Debug, Deserialize)]
struct SignedClaims {
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
}

/// Validate and persist a signed Worker envelope at the one machine-owned
/// location. Callers never choose a path and the service assigns the refresh
/// timestamp itself.
pub fn store(args: serde_json::Value) -> Result<(), &'static str> {
    let request: StoreLicenseCacheRequest =
        serde_json::from_value(args).map_err(|_| "license cache request is invalid")?;
    validate_request(
        &request,
        service_public_key().ok_or("license verification is unavailable")?,
    )?;

    let persisted = PersistedLicenseCache {
        token: PersistedTokenEnvelope {
            payload: &request.payload,
            signature: &request.signature,
        },
        last_verified_at: now_unix(),
        seats_used: request.seats_used,
        seat_limit: request.seat_limit,
    };
    let bytes =
        serde_json::to_vec_pretty(&persisted).map_err(|_| "license cache could not be encoded")?;
    atomic_write(&machine_license_cache_path()?, &bytes)
        .map_err(|_| "machine license cache is unavailable")
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

    let claims: SignedClaims =
        serde_json::from_str(&request.payload).map_err(|_| "license cache payload is invalid")?;
    if claims.device_hash.trim().is_empty() || claims.exp == 0 || claims.iat > claims.exp {
        return Err("license cache payload is invalid");
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
}
