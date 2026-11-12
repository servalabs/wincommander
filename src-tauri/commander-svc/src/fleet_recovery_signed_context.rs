// SPDX-License-Identifier: AGPL-3.0-or-later
//! Verification of a bounded server-signed context against service-held trust.
use super::native_confirmation::canonical_uuid;
use crate::fleet_machine_policy::{FleetMachinePolicy, FleetTrust};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::VerifyingKey;
use serde::Deserialize;

#[path = "fleet_recovery_key_broker.rs"]
mod key_broker;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryContext {
    recovery_id: String, org_id: String, device_id: String, old_generation: String,
    server_key: String, old_key: String, challenge: String, issued_at: i64,
    expires_at: i64, server_signature: String,
}

pub(super) struct VerifiedContext { recovery_id: String, device_id: String, bytes: Vec<u8> }
impl VerifiedContext {
    pub(super) fn recovery_id(&self) -> &str { &self.recovery_id }
    pub(super) fn bytes(&self) -> &[u8] { &self.bytes }
    fn device_id(&self) -> &str { &self.device_id }
}

// No production constructor exists until an authenticated, protected local
// broker can provide the actual replacement public key to this service.
pub(super) struct VerifiedLocalPublicKey([u8; 32]);
impl VerifiedLocalPublicKey { pub(super) fn bytes(&self) -> &[u8; 32] { &self.0 } }

fn valid_key(value: &str) -> bool {
    let Ok(bytes) = STANDARD.decode(value) else { return false; };
    if STANDARD.encode(&bytes) != value { return false; }
    let Ok(bytes): Result<[u8; 32], _> = bytes.try_into() else { return false; };
    VerifyingKey::from_bytes(&bytes).is_ok_and(|key| !key.is_weak())
}

pub(super) fn verify_with_trust(bytes: &[u8], trust: &FleetTrust, now: i64) -> Result<VerifiedContext, &'static str> {
    if bytes.is_empty() || bytes.len() > 4096 { return Err("invalid recovery context"); }
    let context: RecoveryContext = serde_json::from_slice(bytes).map_err(|_| "invalid recovery context")?;
    if context.org_id.is_empty() || context.org_id.len() > 128 || context.device_id.len() > 128
        || context.org_id != trust.org_id || context.device_id != trust.device_id
        || context.old_generation != trust.enrollment_generation || context.server_key != trust.signer_key
        || !canonical_uuid(&context.recovery_id) || !canonical_uuid(&context.device_id)
        || !canonical_uuid(&context.old_generation) || !canonical_uuid(&context.challenge)
        || !valid_key(&context.server_key) || !valid_key(&context.old_key)
        || context.issued_at > now || context.expires_at <= now
        || context.expires_at.checked_sub(context.issued_at).is_none_or(|n| !(1..=300).contains(&n)) {
        return Err("invalid recovery context");
    }
    let signature = STANDARD.decode(&context.server_signature).map_err(|_| "invalid recovery context")?;
    if STANDARD.encode(&signature) != context.server_signature { return Err("invalid recovery context"); }
    let payload = serde_json::to_vec(&("wincommander:fleet:device-key-recovery-context:v2", &context.recovery_id,
        &context.org_id, &context.device_id, &context.old_generation, &context.server_key,
        &context.old_key, &context.challenge, context.issued_at, context.expires_at)).map_err(|_| "invalid recovery context")?;
    if !fleet_proto::verify_signature_b64(&trust.signer_key, &payload, &context.server_signature) {
        return Err("invalid recovery context");
    }
    Ok(VerifiedContext { recovery_id: context.recovery_id, device_id: context.device_id, bytes: bytes.to_vec() })
}

pub(super) fn verify_from_store(bytes: &[u8], store: &FleetMachinePolicy, now: i64) -> Result<VerifiedContext, &'static str> {
    let (trust, _) = store.trust_snapshot().map_err(|_| "protected Fleet trust is unavailable")?
        .ok_or("protected Fleet trust is unavailable")?;
    verify_with_trust(bytes, &trust, now)
}
