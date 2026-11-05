// SPDX-License-Identifier: AGPL-3.0-or-later
//! Staged service-owned trial custody. There is intentionally no transport or
//! IPC entry point until the Worker reservation and settlement are paired.
use crate::fleet_credential::{MachineDpapi, Protector};
use crate::license_cache::{licence_device_anchor::Anchor, release_gate};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const INVALID: &str = "trial security state is unavailable";
const PATH: &str = r"SOFTWARE\ServaLabs\WinCommander\HybridTrialActivation";
const ENTROPY: &[u8] = b"wincommander:licence-trial-intent:v1";
const LIMIT: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope { version: u8, sealed: String }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingTrial {
    operation_id: String,
    device_hash: String,
    credential_id: [u8; 32],
    credential_secret: [u8; 32],
}

impl Drop for PendingTrial {
    fn drop(&mut self) { self.credential_secret.zeroize(); }
}

impl PendingTrial {
    /// The proposed v4 reservation commits to this credential before any
    /// secret is sent; the transport remains deliberately disconnected.
    fn credential_commitment(&self) -> String {
        crate::licence_activation_intent::credential_commitment(&self.credential_id, &self.credential_secret)
    }

    fn request_body(&self) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a> { device_hash: &'a str, app_id: &'static str, is_portable: bool,
            operation_id: &'a str, trial_credential_id: String, trial_credential_commitment: String }
        let mut body = Zeroizing::new(Vec::new());
        let id = self.credential_id.iter().map(|byte| format!("{byte:02x}")).collect();
        serde_json::to_writer(&mut *body, &Request { device_hash: &self.device_hash,
            app_id: "com.servalabs.wincommander", is_portable: false, operation_id: &self.operation_id,
            trial_credential_id: id, trial_credential_commitment: self.credential_commitment() }).map_err(|_| INVALID)?;
        Ok(body)
    }
}

#[derive(Deserialize)]
struct TrialEnvelope { ok: bool, payload: String, signature: String, hybrid: String }

struct VerifiedTrial { payload: String, operation_id: String, device_hash: String, credential_id: [u8; 32], credential_commitment: String, expires_at: u64 }

#[derive(Deserialize)]
struct TrialClaims {
    license_id: String, device_hash: String, plan: String, features: Vec<String>, base_features: Vec<String>,
    service_features: Vec<String>, service_exp: u64, iat: u64, exp: u64, license_exp: u64,
    iss: String, sub: String, is_portable: bool, trial_operation_id: String,
    trial_credential_id: String, trial_credential_commitment: String,
}

fn verify_response_with(pending: &PendingTrial, bytes: &[u8], now: u64,
    verify: impl FnOnce(&str, &str, &str) -> Result<(), &'static str>) -> Result<VerifiedTrial, &'static str> {
    if !valid_hex(&pending.device_hash) || !valid_hex(&pending.operation_id)
        || bytes.is_empty() || bytes.len() > 64 * 1024 { return Err(INVALID); }
    let envelope: TrialEnvelope = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if !envelope.ok || envelope.payload.is_empty() || envelope.signature.is_empty() || envelope.hybrid.is_empty() {
        return Err(INVALID);
    }
    verify(&envelope.payload, &envelope.signature, &envelope.hybrid)?;
    let claims: TrialClaims = serde_json::from_str(&envelope.payload).map_err(|_| INVALID)?;
    let id: String = pending.credential_id.iter().map(|byte| format!("{byte:02x}")).collect();
    if claims.license_id != format!("trial-{}", &pending.device_hash[..16]) || claims.device_hash != pending.device_hash
        || claims.plan != "trial" || claims.features != ["paid"] || !claims.base_features.is_empty()
        || claims.service_features != ["paid"] || claims.service_exp != claims.exp || claims.license_exp != claims.exp
        || claims.iss != "wincommander-license-worker" || claims.sub != "com.servalabs.wincommander"
        || claims.is_portable || claims.trial_operation_id != pending.operation_id || claims.trial_credential_id != id
        || claims.trial_credential_commitment != pending.credential_commitment() || claims.iat > now.saturating_add(300)
        || claims.exp <= now || claims.iat.checked_add(16 * 24 * 60 * 60) != Some(claims.exp) {
        return Err(INVALID);
    }
    Ok(VerifiedTrial { payload: envelope.payload, operation_id: pending.operation_id.clone(),
        device_hash: pending.device_hash.clone(), credential_id: pending.credential_id,
        credential_commitment: pending.credential_commitment(), expires_at: claims.exp })
}

/// This verifies a response but deliberately does not persist or grant trial
/// entitlement. A separately authenticated settlement is still required.
fn verify_response(pending: &PendingTrial, bytes: &[u8], now: u64) -> Result<VerifiedTrial, &'static str> {
    verify_response_with(pending, bytes, now, crate::license_cache::verify_activation_envelope)
}

/// Candidate settlement transcript. There is deliberately no transport or
/// completion transition until the server custody protocol is specified.
fn settlement_request(pending: &PendingTrial, trial: &VerifiedTrial, now: u64) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    use sha2::{Digest, Sha256};
    let signed_payload = &trial.payload;
    if signed_payload.is_empty() || signed_payload.len() > 16 * 1024 || trial.operation_id != pending.operation_id
        || trial.device_hash != pending.device_hash || trial.credential_id != pending.credential_id
        || trial.credential_commitment != pending.credential_commitment() || trial.expires_at <= now { return Err(INVALID); }
    #[derive(Serialize)]
    struct Claims<'a> { kind: &'static str, operation_id: &'a str, device_hash: &'a str,
        trial_payload_digest: String, credential_id: String }
    #[derive(Serialize)]
    struct Request<'a> { payload: &'a str, proof: &'a str }
    let digest = Sha256::digest(signed_payload.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let payload = serde_json::to_string(&Claims { kind: "licence-trial-settlement-v1", operation_id: &pending.operation_id,
        device_hash: &pending.device_hash, trial_payload_digest: digest, credential_id: URL_SAFE_NO_PAD.encode(pending.credential_id) })
        .map_err(|_| INVALID)?;
    let proof = Zeroizing::new(wincmd_pq_experiment::licence_device_proof::sign_trial_settlement(&pending.credential_secret, payload.as_bytes())?);
    let proof_text = Zeroizing::new(URL_SAFE_NO_PAD.encode(proof.as_ref()));
    let mut body = Zeroizing::new(Vec::new());
    serde_json::to_writer(&mut *body, &Request { payload: &payload, proof: &proof_text }).map_err(|_| INVALID)?;
    Ok(body)
}

/// Candidate transcript only: before this can be sent, a retirement ID and
/// the exact request must be durably recorded, with a verified receipt.
fn retirement_request(pending: &PendingTrial, retirement_id: &str) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if !valid_hex(retirement_id) { return Err(INVALID); }
    #[derive(Serialize)]
    struct Claims<'a> { kind: &'static str, retirement_id: &'a str, operation_id: &'a str,
        device_hash: &'a str, credential_id: String }
    #[derive(Serialize)]
    struct Request<'a> { payload: &'a str, proof: &'a str }
    let payload = serde_json::to_string(&Claims { kind: "licence-trial-retirement-v1", retirement_id,
        operation_id: &pending.operation_id, device_hash: &pending.device_hash,
        credential_id: URL_SAFE_NO_PAD.encode(pending.credential_id) }).map_err(|_| INVALID)?;
    let proof = Zeroizing::new(wincmd_pq_experiment::licence_device_proof::sign_trial_retirement(&pending.credential_secret, payload.as_bytes())?);
    let proof_text = Zeroizing::new(URL_SAFE_NO_PAD.encode(proof.as_ref()));
    let mut body = Zeroizing::new(Vec::new());
    serde_json::to_writer(&mut *body, &Request { payload: &payload, proof: &proof_text }).map_err(|_| INVALID)?;
    Ok(body)
}

fn valid_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn machine() -> Anchor { Anchor::machine_named(PATH) }

fn load_at(store: &Anchor, protector: &dyn Protector) -> Result<Option<PendingTrial>, &'static str> {
    if !store.exists().map_err(|_| INVALID)? { return Ok(None); }
    let raw = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    if raw.len() > LIMIT { return Err(INVALID); }
    let envelope: Envelope = serde_json::from_slice(&raw).map_err(|_| INVALID)?;
    if envelope.version != 1 || envelope.sealed.len() > LIMIT { return Err(INVALID); }
    let sealed = URL_SAFE_NO_PAD.decode(&envelope.sealed).map_err(|_| INVALID)?;
    if sealed.is_empty() || URL_SAFE_NO_PAD.encode(&sealed) != envelope.sealed { return Err(INVALID); }
    let plaintext = protector.unprotect(&sealed, ENTROPY).map_err(|_| INVALID)?;
    let pending: PendingTrial = serde_json::from_slice(&plaintext).map_err(|_| INVALID)?;
    if !valid_hex(&pending.operation_id) || !valid_hex(&pending.device_hash) { return Err(INVALID); }
    Ok(Some(pending))
}

/// Called only while holding the shared cross-process licence security lock.
pub(crate) fn ensure_idle_locked() -> Result<(), &'static str> {
    ensure_idle_at(&machine(), &MachineDpapi)
}

fn ensure_idle_at(store: &Anchor, protector: &dyn Protector) -> Result<(), &'static str> {
    if load_at(store, protector)?.is_some() { Err(INVALID) } else { Ok(()) }
}

/// This scaffold cannot be exposed yet: the eventual entry point must also
/// check the protected licence cache and retained paid history before starting.
pub(crate) fn begin_or_resume() -> Result<PendingTrial, &'static str> {
    let identity = crate::licence_machine_identity::read_machine_identity()?;
    if identity.portable { return Err(INVALID); }
    begin_at(&machine(), &release_gate::machine(), &MachineDpapi, &identity.device_hash,
        crate::licence_activation_intent::ensure_no_paid_intent_locked)
}

/// A resumed trial always retains the exact operation and device credential.
/// No caller can replace a damaged or unfinished intent.
fn begin_at(store: &Anchor, gate: &Anchor, protector: &dyn Protector, device_hash: &str,
    check_paid: impl FnOnce() -> Result<(), &'static str>) -> Result<PendingTrial, &'static str> {
    if !valid_hex(device_hash) { return Err(INVALID); }
    let _lock = crate::licence_security_lock()?;
    release_gate::ensure_idle_at(gate)?;
    check_paid()?;
    if let Some(saved) = load_at(store, protector)? {
        return if saved.device_hash == device_hash { Ok(saved) } else { Err(INVALID) };
    }
    let mut operation = [0u8; 32];
    let mut credential_id = [0u8; 32];
    let mut credential_secret = [0u8; 32];
    rand::rngs::OsRng.try_fill_bytes(&mut operation).map_err(|_| INVALID)?;
    rand::rngs::OsRng.try_fill_bytes(&mut credential_id).map_err(|_| INVALID)?;
    rand::rngs::OsRng.try_fill_bytes(&mut credential_secret).map_err(|_| INVALID)?;
    let operation_id = operation.iter().map(|byte| format!("{byte:02x}")).collect();
    operation.zeroize();
    let pending = PendingTrial { operation_id, device_hash: device_hash.into(), credential_id, credential_secret };
    credential_secret.zeroize();
    let plain = Zeroizing::new(serde_json::to_vec(&pending).map_err(|_| INVALID)?);
    let sealed = protector.protect(&plain, ENTROPY).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let bytes = serde_json::to_vec(&Envelope { version: 1, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    store.write(&bytes).map_err(|_| INVALID)?;
    let saved = load_at(store, protector)?.ok_or(INVALID)?;
    if saved.operation_id != pending.operation_id || saved.device_hash != pending.device_hash
        || saved.credential_id != pending.credential_id
        || !bool::from(saved.credential_secret.ct_eq(&pending.credential_secret)) { return Err(INVALID); }
    Ok(saved)
}

#[cfg(test)]
#[path = "licence_trial_intent_tests.rs"]
mod tests;
