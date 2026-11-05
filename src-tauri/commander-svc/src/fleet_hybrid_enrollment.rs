// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service-owned, one-use reservation for the exact signed enrollment body.
//! No network request or session publication is authorized by this module.
use super::*;
use sha2::{Digest, Sha256};

const MAX_REQUEST: usize = 32 * 1024;
const INVALID: &str = "hybrid_enrollment_invalid";

#[derive(Clone)]
pub(super) struct ClaimMarker {
    id: String,
    caller_sid: String,
    session_id: u32,
    trust: FleetTrust,
    connection_revision: u64,
    request_hash: [u8; 32],
    expires: std::time::Instant,
}

impl ClaimMarker {
    pub(super) fn matches_owner(&self, id: &str, actor: &str, session: u32) -> bool {
        self.id == id && self.caller_sid == actor && self.session_id == session
    }
}

/// Not cloneable or constructible outside the service. Carry the original
/// bytes, never reserialized JSON, into a future service-owned transport.
pub(crate) struct ClaimedHybridEnrollment {
    marker: ClaimMarker,
    origin: String,
    challenge: String,
    outcome_key: String,
    request: zeroize::Zeroizing<Vec<u8>>,
}

impl ClaimedHybridEnrollment {
    pub(crate) fn request(&self) -> &[u8] {
        &self.request
    }
    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }
    pub(crate) fn challenge(&self) -> &str {
        &self.challenge
    }
    pub(crate) fn outcome_key(&self) -> &str {
        &self.outcome_key
    }
}

fn verified_request(
    bytes: &[u8],
    pending: &PendingHybridEnrollment,
    expected_outcome: &str,
) -> Result<String, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_REQUEST {
        return Err(INVALID);
    }
    let mut value: serde_json::Value =
        fleet_proto::windows_v2::decode_windows_enrollment_safe(bytes).map_err(|_| INVALID)?;
    let object = value.as_object_mut().ok_or(INVALID)?;
    let string = |name: &str| {
        object
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or(INVALID)
    };
    let device = string("device_id")?;
    let kind = string("device_kind")?;
    let challenge = string("shield_progress_challenge")?;
    let signer = string("enrollment_server_key_b64")?;
    let nonce = string("enrollment_nonce")?;
    let outcome = string("outcome_pubkey_b64")?;
    let signature = string("enrollment_signature_b64")?;
    let version = object
        .get("enrollment_proof_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(INVALID)?;
    let timestamp = object
        .get("enrollment_ts")
        .and_then(serde_json::Value::as_i64)
        .ok_or(INVALID)?;
    if device != pending.trust.device_id
        || kind != "wincommander"
        || challenge != pending.challenge
        || signer != pending.trust.signer_key
        || outcome != expected_outcome
        || version != 2
        || timestamp <= 0
        || !uuid::Uuid::parse_str(&nonce)
            .is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == nonce)
    {
        return Err(INVALID);
    }
    let key = STANDARD.decode(&outcome).map_err(|_| INVALID)?;
    let key: [u8; 32] = key.try_into().map_err(|_| INVALID)?;
    if STANDARD.encode(key) != outcome
        || !ed25519_dalek::VerifyingKey::from_bytes(&key).is_ok_and(|key| !key.is_weak())
    {
        return Err(INVALID);
    }
    let raw_signature = STANDARD.decode(&signature).map_err(|_| INVALID)?;
    if raw_signature.len() != 64 || STANDARD.encode(&raw_signature) != signature {
        return Err(INVALID);
    }
    object.remove("enrollment_signature_b64");
    let mut preimage =
        b"WinCommander Fleet enrollment proof v2\nPOST\n/v1/windows/enroll-v2\n".to_vec();
    preimage.extend(fleet_proto::canonical_json_bytes(&value));
    if !fleet_proto::verify_signature_b64(&outcome, &preimage, &signature) {
        return Err(INVALID);
    }
    Ok(outcome)
}

impl FleetMachinePolicy {
    /// Exactly one caller can reserve this grant. Matching attempts consume it
    /// even when malformed, stale, or signed incorrectly. A foreign caller
    /// cannot consume another user's grant.
    pub(crate) fn claim_hybrid_enrollment(
        &self,
        id: &str,
        actor: &str,
        session: u32,
        bytes: &[u8],
    ) -> Result<ClaimedHybridEnrollment, &'static str> {
        let mut guard = self.state.lock().map_err(|_| "store_unavailable")?;
        let pending = guard
            .hybrid_enrollment
            .as_ref()
            .ok_or("hybrid_enrollment_not_authorized")?;
        if pending.id != id
            || pending.caller_sid != actor
            || session == 0
            || pending.session_id != session
        {
            return Err("hybrid_enrollment_not_authorized");
        }
        let pending = guard
            .hybrid_enrollment
            .take()
            .ok_or("hybrid_enrollment_not_authorized")?;
        let record = guard.record.as_ref().ok_or("not_provisioned")?;
        if pending.trust != record.trust
            || pending.connection_revision != record.connection_revision
            || std::time::Instant::now() >= pending.expires
            || record.service_enabled
            || record.credential.is_some()
            || record.hybrid_unconfirmed
            || record.hybrid_trust.is_none()
        {
            return Err("hybrid_enrollment_not_authorized");
        }
        let expected_outcome = record
            .hybrid_trust
            .as_ref()
            .and_then(|trust| trust.outcome_key_b64.as_deref())
            .ok_or("hybrid_enrollment_not_authorized")?;
        let outcome_key = verified_request(bytes, &pending, expected_outcome)?;
        if std::time::Instant::now() >= pending.expires {
            return Err("hybrid_enrollment_expired");
        }
        let marker = ClaimMarker {
            id: pending.id,
            caller_sid: pending.caller_sid,
            session_id: pending.session_id,
            trust: pending.trust,
            connection_revision: pending.connection_revision,
            request_hash: Sha256::digest(bytes).into(),
            expires: pending.expires,
        };
        guard.hybrid_claim = Some(marker.clone());
        Ok(ClaimedHybridEnrollment {
            marker,
            origin: pending.origin,
            challenge: pending.challenge,
            outcome_key,
            request: zeroize::Zeroizing::new(bytes.to_vec()),
        })
    }

    /// A check is only a snapshot; a future transport must serialize its
    /// actual effects with revocation and recheck before publishing a session.
    pub(crate) fn hybrid_claim_is_current(&self, claim: &ClaimedHybridEnrollment) -> bool {
        let Ok(guard) = self.state.lock() else {
            return false;
        };
        let Some(record) = guard.record.as_ref() else {
            return false;
        };
        let Some(saved) = guard.hybrid_claim.as_ref() else {
            return false;
        };
        saved.matches_owner(
            &claim.marker.id,
            &claim.marker.caller_sid,
            claim.marker.session_id,
        ) && saved.request_hash == claim.marker.request_hash
            && saved.trust == claim.marker.trust
            && saved.connection_revision == claim.marker.connection_revision
            && saved.trust == record.trust
            && saved.connection_revision == record.connection_revision
            && !record.service_enabled
            && record.credential.is_none()
            && !record.hybrid_unconfirmed
            && record.hybrid_trust.is_some()
            && std::time::Instant::now() < saved.expires
    }
}
