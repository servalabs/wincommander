// SPDX-License-Identifier: AGPL-3.0-or-later
//! Historical contact observations only; no policy installation or action authority.
//! The receipt authenticates body bytes, not HTTP status or transport metadata.
use super::{verify_policy, FleetTrust};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use rand::TryRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize)]
pub struct ContactChallenge {
    pub nonce: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactObservation {
    /// Local result-observation time, not proof of clock continuity or current availability.
    pub observed_unix_ms: u64,
}

#[derive(Clone)]
pub(super) struct PendingContact {
    pub nonce: String,
    pub trust: FleetTrust,
    issued: Instant,
}

#[derive(Deserialize)]
struct RequestBinding {
    protocol_version: i64,
    capabilities: Vec<String>,
    device_id: String,
    nonce: String,
    hmac_version: i64,
    hmac: String,
    ts: i64,
    #[serde(default)]
    decoy: bool,
}

#[derive(Deserialize)]
struct ResponseBinding {
    protocol_version: i64,
    #[serde(default)]
    pending_approval: bool,
    policy: fleet_proto::PolicyEnvelope,
}

fn key(trust: &FleetTrust) -> Result<VerifyingKey, &'static str> {
    let bytes: [u8; 32] = STANDARD
        .decode(&trust.signer_key)
        .map_err(|_| "contact_invalid_key")?
        .try_into()
        .map_err(|_| "contact_invalid_key")?;
    let key = VerifyingKey::from_bytes(&bytes).map_err(|_| "contact_invalid_key")?;
    if key.is_weak() {
        return Err("contact_invalid_key");
    }
    Ok(key)
}

impl PendingContact {
    pub fn issue(trust: FleetTrust, now: Instant) -> Result<Self, &'static str> {
        key(&trust)?;
        let mut random = [0u8; 16];
        rand::rngs::SysRng
            .try_fill_bytes(&mut random)
            .map_err(|_| "random_unavailable")?;
        let nonce = format!(
            "ci-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        Ok(Self {
            nonce,
            trust,
            issued: now,
        })
    }

    pub fn fresh(&self, now: Instant) -> bool {
        now.checked_duration_since(self.issued)
            .is_some_and(|age| age <= MAX_AGE)
    }

    pub fn verify(
        &self,
        request: &[u8],
        response: &[u8],
        receipt: &str,
    ) -> Result<(), &'static str> {
        if request.len() > MAX_REQUEST_BYTES || response.len() > MAX_RESPONSE_BYTES {
            return Err("contact_invalid_response");
        }
        let signature = receipt
            .strip_prefix("v1.")
            .filter(|_| receipt.len() == 91)
            .ok_or("contact_invalid_receipt")?;
        let raw: [u8; 64] = STANDARD
            .decode(signature)
            .map_err(|_| "contact_invalid_receipt")?
            .try_into()
            .map_err(|_| "contact_invalid_receipt")?;
        let signing_bytes = fleet_proto::canonical_json_bytes(&serde_json::json!([
            "fleet.windows.contact.v1",
            self.trust.org_id,
            self.trust.device_id,
            "wincommander",
            self.nonce,
            STANDARD.encode(Sha256::digest(request)),
            STANDARD.encode(Sha256::digest(response))
        ]));
        key(&self.trust)?
            .verify_strict(&signing_bytes, &Signature::from_bytes(&raw))
            .map_err(|_| "contact_invalid_signature")?;
        // Typed decoding rejects duplicate binding fields instead of silently choosing one.
        let request: RequestBinding =
            serde_json::from_slice(request).map_err(|_| "contact_invalid_request")?;
        let hmac = STANDARD
            .decode(&request.hmac)
            .map_err(|_| "contact_invalid_request")?;
        if request.protocol_version != fleet_proto::CHECKIN_PROTOCOL_VERSION
            || request.hmac_version != 2
            || hmac.len() != 32
            || STANDARD.encode(hmac) != request.hmac
            || request.device_id != self.trust.device_id
            || request.nonce != self.nonce
            || request.decoy
            || !request
                .capabilities
                .iter()
                .any(|c| c == "windows_contact_receipt_v1")
        {
            return Err("contact_invalid_request");
        }
        let _untrusted_request_time = request.ts;
        let response: ResponseBinding =
            serde_json::from_slice(response).map_err(|_| "contact_invalid_response")?;
        if response.protocol_version != fleet_proto::CHECKIN_PROTOCOL_VERSION
            || response.pending_approval
        {
            return Err("contact_invalid_response");
        }
        verify_policy(&self.trust, &response.policy)?;
        Ok(())
    }
}

pub(super) fn observed_time() -> Result<u64, &'static str> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "contact_clock_unavailable")?
        .as_millis()
        .try_into()
        .map_err(|_| "contact_clock_unavailable")
}
