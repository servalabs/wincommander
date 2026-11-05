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
const LIMIT: usize = 64 * 1024;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    issued: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    settlement: Option<String>,
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

    /// Candidate secret-custody request; usable only after verification of
    /// this exact reservation. It is not connected to a call path.
    fn custody_request(&self, trial: &VerifiedTrial, now: u64) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        if trial.operation_id != self.operation_id || trial.device_hash != self.device_hash
            || trial.credential_id != self.credential_id || trial.credential_commitment != self.credential_commitment()
            || trial.expires_at <= now { return Err(INVALID); }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a> { app_id: &'static str, device_hash: &'a str, operation_id: &'a str,
            credential_id: String, device_secret: &'a str }
        let secret = Zeroizing::new(URL_SAFE_NO_PAD.encode(self.credential_secret));
        let mut body = Zeroizing::new(Vec::new());
        serde_json::to_writer(&mut *body, &Request { app_id: "com.servalabs.wincommander", device_hash: &self.device_hash,
            operation_id: &self.operation_id, credential_id: URL_SAFE_NO_PAD.encode(self.credential_id), device_secret: &secret })
            .map_err(|_| INVALID)?;
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

/// Both signatures and the exact locally generated transcript are mandatory.
/// This primitive performs no persistence or entitlement transition.
fn verify_receipt(request: &[u8], receipt: &str, trust: &wincmd_pq_experiment::jws::Trust<'_>,
    purpose: wincmd_pq_experiment::Purpose) -> Result<(), &'static str> {
    if !matches!(purpose, wincmd_pq_experiment::Purpose::LicenceTrialSettlement |
        wincmd_pq_experiment::Purpose::LicenceTrialRetirement) || request.is_empty() || request.len() > 16 * 1024
        || receipt.is_empty() || receipt.len() > 32 * 1024 { return Err(INVALID); }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request { payload: String, proof: String }
    let sent: Request = serde_json::from_slice(request).map_err(|_| INVALID)?;
    if sent.payload.is_empty() || sent.proof.len() != 43 { return Err(INVALID); }
    let policy = wincmd_pq_experiment::jws::Trust { purpose, minimum_epoch: trust.minimum_epoch,
        ed_key_id: trust.ed_key_id, ed25519_public: trust.ed25519_public,
        pq_key_id: trust.pq_key_id, ml_dsa_65_public: trust.ml_dsa_65_public };
    let verified = wincmd_pq_experiment::jws::verify(receipt.as_bytes(), &policy).map_err(|_| INVALID)?;
    if verified.payload != sent.payload.as_bytes() { return Err(INVALID); }
    Ok(())
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
    if pending.issued.as_ref().is_some_and(|value| {
        URL_SAFE_NO_PAD.decode(value).ok().filter(|bytes| !bytes.is_empty() && bytes.len() <= 16 * 1024
            && URL_SAFE_NO_PAD.encode(bytes) == *value).is_none()
    }) || pending.settlement.as_ref().is_some_and(|value| value.is_empty() || value.len() > 32 * 1024)
        || (pending.settlement.is_some() && pending.issued.is_none()) { return Err(INVALID); }
    Ok(Some(pending))
}

fn save_at(store: &Anchor, protector: &dyn Protector, pending: &PendingTrial) -> Result<(), &'static str> {
    let plain = Zeroizing::new(serde_json::to_vec(pending).map_err(|_| INVALID)?);
    let sealed = protector.protect(&plain, ENTROPY).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let bytes = serde_json::to_vec(&Envelope { version: 1, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    store.write(&bytes).map_err(|_| INVALID)?;
    let saved = load_at(store, protector)?.ok_or(INVALID)?;
    if saved.operation_id != pending.operation_id || saved.device_hash != pending.device_hash
        || saved.credential_id != pending.credential_id || saved.issued != pending.issued || saved.settlement != pending.settlement
        || !bool::from(saved.credential_secret.ct_eq(&pending.credential_secret)) { return Err(INVALID); }
    Ok(())
}

fn matches(a: &PendingTrial, b: &PendingTrial) -> bool {
    a.operation_id == b.operation_id && a.device_hash == b.device_hash && a.credential_id == b.credential_id
        && bool::from(a.credential_secret.ct_eq(&b.credential_secret))
}

/// Persist the verified Worker response before custody or settlement. Exact
/// retries reopen the retained response; no trial entitlement is granted.
fn record_issued_at(store: &Anchor, protector: &dyn Protector, pending: &PendingTrial, response: &[u8], now: u64,
    verify: impl Copy + Fn(&str, &str, &str) -> Result<(), &'static str>) -> Result<(), &'static str> {
    if response.len() > 16 * 1024 { return Err(INVALID); }
    let received = verify_response_with(pending, response, now, verify)?;
    let _lock = crate::licence_security_lock()?;
    let mut saved = load_at(store, protector)?.ok_or(INVALID)?;
    if !matches(&saved, pending) { return Err(INVALID); }
    if let Some(existing) = &saved.issued {
        let bytes = URL_SAFE_NO_PAD.decode(existing).map_err(|_| INVALID)?;
        let prior = verify_response_with(&saved, &bytes, now, verify)?;
        return if prior.payload == received.payload { Ok(()) } else { Err(INVALID) };
    }
    saved.issued = Some(URL_SAFE_NO_PAD.encode(response));
    save_at(store, protector, &saved)
}

fn record_issued(pending: &PendingTrial, response: &[u8], now: u64) -> Result<(), &'static str> {
    record_issued_at(&machine(), &MachineDpapi, pending, response, now,
        crate::license_cache::verify_activation_envelope)
}

/// Record only a receipt over the retained verified response and the exact
/// locally computed settlement request. No cache or entitlement is changed.
fn record_settlement_at(store: &Anchor, protector: &dyn Protector, pending: &PendingTrial, receipt: &str,
    now: u64, trust: &wincmd_pq_experiment::jws::Trust<'_>,
    verify: impl Copy + Fn(&str, &str, &str) -> Result<(), &'static str>) -> Result<(), &'static str> {
    let _lock = crate::licence_security_lock()?;
    let mut saved = load_at(store, protector)?.ok_or(INVALID)?;
    if !matches(&saved, pending) { return Err(INVALID); }
    let issued = URL_SAFE_NO_PAD.decode(saved.issued.as_deref().ok_or(INVALID)?).map_err(|_| INVALID)?;
    let trial = verify_response_with(&saved, &issued, now, verify)?;
    let request = settlement_request(&saved, &trial, now)?;
    verify_receipt(&request, receipt, trust, wincmd_pq_experiment::Purpose::LicenceTrialSettlement)?;
    if let Some(existing) = &saved.settlement {
        verify_receipt(&request, existing, trust, wincmd_pq_experiment::Purpose::LicenceTrialSettlement)?;
        return Ok(());
    }
    saved.settlement = Some(receipt.into());
    save_at(store, protector, &saved)
}

fn record_settlement(pending: &PendingTrial, receipt: &str, now: u64) -> Result<(), &'static str> {
    let trust = crate::license_cache::configured_device_receipt_trust()?;
    record_settlement_at(&machine(), &MachineDpapi, pending, receipt, now, &trust.policy(),
        crate::license_cache::verify_activation_envelope)
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
    let pending = PendingTrial { operation_id, device_hash: device_hash.into(), credential_id, credential_secret,
        issued: None, settlement: None };
    credential_secret.zeroize();
    save_at(store, protector, &pending)?;
    let saved = load_at(store, protector)?.ok_or(INVALID)?;
    if saved.operation_id != pending.operation_id || saved.device_hash != pending.device_hash
        || saved.credential_id != pending.credential_id
        || !bool::from(saved.credential_secret.ct_eq(&pending.credential_secret)) { return Err(INVALID); }
    Ok(saved)
}

#[cfg(test)]
#[path = "licence_trial_intent_tests.rs"]
mod tests;
