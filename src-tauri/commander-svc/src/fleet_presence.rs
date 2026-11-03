// SPDX-License-Identifier: AGPL-3.0-or-later
//! Request-bound service reachability only. This never authorizes actions.
use crate::fleet_machine_policy::FleetTrust;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) const PATH: &str = "/v1/agents/windows-service-presence";
pub(crate) const MAX_RESPONSE: usize = 4096;

pub(crate) fn make_request(
    trust: &FleetTrust,
    origin: &str,
    secret: &[u8],
    now: i64,
) -> Result<(String, Vec<u8>), &'static str> {
    if secret.len() != 32 {
        return Err("credential_unavailable");
    }
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "random_unavailable")?;
    let nonce = format!(
        "sp-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let mut body = json!({"protocol_version":1,"device_id":trust.device_id,"origin":origin,
        "enrollment_generation":trust.enrollment_generation,"ts":now,"nonce":nonce,
        "hmac_version":2,"hmac":""});
    let mac = fleet_agent_core::compute_request_hmac_v2(secret, "POST", PATH, &body)
        .map_err(|_| "credential_unavailable")?;
    body["hmac"] = Value::String(mac);
    let request = serde_json::to_vec(&body).map_err(|_| "invalid_request")?;
    Ok((nonce, request))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    protocol_version: i64,
    org_id: String,
    device_id: String,
    enrollment_generation: String,
    nonce: String,
    request_sha256: String,
    signature: String,
}

pub(crate) fn verify_response(
    trust: &FleetTrust,
    nonce: &str,
    request: &[u8],
    response: &[u8],
) -> bool {
    if response.len() > MAX_RESPONSE {
        return false;
    }
    let Ok(response) = serde_json::from_slice::<Response>(response) else {
        return false;
    };
    let hash = STANDARD.encode(Sha256::digest(request));
    if response.protocol_version != 1
        || response.org_id != trust.org_id
        || response.device_id != trust.device_id
        || response.enrollment_generation != trust.enrollment_generation
        || response.nonce != nonce
        || response.request_sha256 != hash
    {
        return false;
    }
    let Ok(key_bytes) = STANDARD.decode(&trust.signer_key) else {
        return false;
    };
    if STANDARD.encode(&key_bytes) != trust.signer_key {
        return false;
    }
    let Ok(key): Result<[u8; 32], _> = key_bytes.try_into() else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key) else {
        return false;
    };
    if key.is_weak() {
        return false;
    }
    let Ok(signature) = STANDARD.decode(&response.signature) else {
        return false;
    };
    if STANDARD.encode(&signature) != response.signature {
        return false;
    }
    let Ok(signature): Result<[u8; 64], _> = signature.try_into() else {
        return false;
    };
    let payload = fleet_proto::canonical_json_bytes(&json!([
        "fleet.windows.service-presence.v1",
        trust.org_id,
        trust.device_id,
        trust.enrollment_generation,
        nonce,
        hash
    ]));
    key.verify_strict(&payload, &Signature::from_bytes(&signature))
        .is_ok()
}

#[cfg(test)]
#[path = "fleet_presence_tests.rs"]
mod tests;
