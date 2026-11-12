// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disabled authenticated broker boundary for a local recovery public key.
use super::{verify_from_store, FleetMachinePolicy, VerifiedContext, VerifiedLocalPublicKey};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::VerifyingKey;
use rand::{rngs::OsRng, RngCore};
use serde::Deserialize;

// No corresponding authenticated sidecar feature or protected read-only
// key-store API exists yet. Never enable based on this implementation alone.
const KEY_BROKER_READY: bool = false;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyProof { device_id: String, nonce: String, public_key: String, signature: String }

// Only a future service-owned, PID/hash-pinned broker can create this value.
// No production constructor exists, so renderer data cannot be promoted to it.
struct AuthenticatedBrokerReply { value: serde_json::Value, device: String, nonce: String }

fn verify_broker_reply(reply: AuthenticatedBrokerReply) -> Result<VerifiedLocalPublicKey, &'static str> {
    let AuthenticatedBrokerReply { value, device, nonce } = reply;
    let proof: KeyProof = serde_json::from_value(value).map_err(|_| "local recovery key unavailable")?;
    if device.is_empty() || device.len() > 128 || proof.device_id != device || proof.nonce != nonce
        || nonce.len() != 44 || proof.public_key.len() != 44 || proof.signature.len() != 88
        || STANDARD.decode(&nonce).ok().filter(|b| b.len() == 32 && STANDARD.encode(b) == nonce).is_none() {
        return Err("local recovery key unavailable");
    }
    let bytes = STANDARD.decode(&proof.public_key).map_err(|_| "local recovery key unavailable")?;
    if STANDARD.encode(&bytes) != proof.public_key { return Err("local recovery key unavailable"); }
    let key: [u8; 32] = bytes.try_into().map_err(|_| "local recovery key unavailable")?;
    if VerifyingKey::from_bytes(&key).map_or(true, |key| key.is_weak()) { return Err("local recovery key unavailable"); }
    let signature = STANDARD.decode(&proof.signature).map_err(|_| "local recovery key unavailable")?;
    if STANDARD.encode(&signature) != proof.signature { return Err("local recovery key unavailable"); }
    let preimage = serde_json::to_vec(&("wincommander:fleet:local-recovery-key:v1", &device, &nonce, &proof.public_key))
        .map_err(|_| "local recovery key unavailable")?;
    if !fleet_proto::verify_signature_b64(&proof.public_key, &preimage, &proof.signature) {
        return Err("local recovery key unavailable");
    }
    Ok(VerifiedLocalPublicKey(key))
}

fn unix_now() -> Result<i64, &'static str> {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "local recovery key unavailable")?.as_secs();
    i64::try_from(seconds).map_err(|_| "local recovery key unavailable")
}

pub(super) async fn fetch(context: &VerifiedContext, store: &FleetMachinePolicy,
    peer: &super::super::AuthenticatedPipePeer) -> Result<VerifiedLocalPublicKey, &'static str> {
    if !KEY_BROKER_READY { return Err("local recovery key broker is unavailable"); }
    let captured = super::super::Peer::capture(peer)?;
    let verified = verify_from_store(context.bytes(), store, unix_now()?)?;
    if verified.device_id() != context.device_id() { return Err("local recovery key unavailable"); }
    let mut nonce = [0u8; 32];
    OsRng.try_fill_bytes(&mut nonce).map_err(|_| "local recovery key unavailable")?;
    let nonce = STANDARD.encode(nonce);
    let result = crate::pro_broker::recovery_key_call(verified.device_id(), &nonce, &captured.sid).await?;
    let proof = AuthenticatedBrokerReply { value: result, device: verified.device_id().to_owned(), nonce };
    let key = verify_broker_reply(proof)?;
    if super::super::Peer::capture(peer).ok().as_ref() != Some(&captured) {
        return Err("local recovery key unavailable");
    }
    verify_from_store(context.bytes(), store, unix_now()?)?;
    Ok(key)
}
