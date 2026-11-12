// SPDX-License-Identifier: AGPL-3.0-or-later
//! Staged service-owned trial custody. There is intentionally no transport or
//! IPC entry point until the Worker reservation and settlement are paired.
use crate::fleet_credential::{MachineDpapi, Protector};
use crate::license_cache::{licence_device_anchor::Anchor, release_gate};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::TryRng;
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

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Retirement { id: String, #[serde(default)] receipt: Option<String>, retired: bool }

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
    issued_accepted_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    settlement: Option<String>,
    #[serde(default)]
    completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retirement: Option<Retirement>,
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
pub(crate) struct TrialEnvelope { pub(crate) ok: bool, pub(crate) payload: String, pub(crate) signature: String, pub(crate) hybrid: String }

struct VerifiedTrial { payload: String, hybrid: String, operation_id: String, device_hash: String, credential_id: [u8; 32], credential_commitment: String, expires_at: u64 }

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
    Ok(VerifiedTrial { payload: envelope.payload, hybrid: envelope.hybrid, operation_id: pending.operation_id.clone(),
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
    #[serde(rename_all = "camelCase")]
    struct Request<'a> { payload: &'a str, proof: &'a str, trial_payload: &'a str, hybrid_trial: &'a str }
    let digest = Sha256::digest(signed_payload.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let payload = serde_json::to_string(&Claims { kind: "licence-trial-settlement-v1", operation_id: &pending.operation_id,
        device_hash: &pending.device_hash, trial_payload_digest: digest, credential_id: URL_SAFE_NO_PAD.encode(pending.credential_id) })
        .map_err(|_| INVALID)?;
    let proof = Zeroizing::new(wincmd_pq_experiment::licence_device_proof::sign_trial_settlement(&pending.credential_secret, payload.as_bytes())?);
    let proof_text = Zeroizing::new(URL_SAFE_NO_PAD.encode(proof.as_ref()));
    let mut body = Zeroizing::new(Vec::new());
    serde_json::to_writer(&mut *body, &Request { payload: &payload, proof: &proof_text,
        trial_payload: signed_payload, hybrid_trial: &trial.hybrid }).map_err(|_| INVALID)?;
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
    #[serde(rename_all = "camelCase")]
    struct Request { payload: String, proof: String, #[serde(default)] trial_payload: Option<String>,
        #[serde(default)] hybrid_trial: Option<String> }
    let sent: Request = serde_json::from_slice(request).map_err(|_| INVALID)?;
    if sent.payload.is_empty() || sent.proof.len() != 43 { return Err(INVALID); }
    if sent.trial_payload.is_some() != sent.hybrid_trial.is_some()
        || (purpose == wincmd_pq_experiment::Purpose::LicenceTrialRetirement && sent.trial_payload.is_some()) {
        return Err(INVALID);
    }
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
        || (pending.issued.is_some() != pending.issued_accepted_at.is_some())
        || (pending.settlement.is_some() && pending.issued.is_none())
        || (pending.completed && (pending.issued.is_none() || pending.settlement.is_none()))
        || pending.retirement.as_ref().is_some_and(|value| pending.issued.is_none() || !valid_hex(&value.id)
            || value.retired != value.receipt.is_some()
            || value.receipt.as_ref().is_some_and(|receipt| receipt.is_empty() || receipt.len() > 32 * 1024)) {
        return Err(INVALID);
    }
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
        || saved.credential_id != pending.credential_id || saved.issued != pending.issued
        || saved.issued_accepted_at != pending.issued_accepted_at || saved.settlement != pending.settlement
        || saved.completed != pending.completed
        || saved.retirement != pending.retirement
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
    if !matches(&saved, pending) || saved.retirement.is_some() { return Err(INVALID); }
    if let Some(existing) = &saved.issued {
        let bytes = URL_SAFE_NO_PAD.decode(existing).map_err(|_| INVALID)?;
        let prior = verify_response_with(&saved, &bytes, now, verify)?;
        return if prior.payload == received.payload { Ok(()) } else { Err(INVALID) };
    }
    saved.issued = Some(URL_SAFE_NO_PAD.encode(response));
    saved.issued_accepted_at = Some(now);
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
    if !matches(&saved, pending) || saved.retirement.is_some() { return Err(INVALID); }
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

/// Caller must hold cache WRITE_LOCK followed by the shared security lock.
/// A crash after cache persistence keeps the intent fenced until exact retry.
fn finalize_at(store: &Anchor, protector: &dyn Protector, pending: &PendingTrial, now: u64,
    trust: &wincmd_pq_experiment::jws::Trust<'_>, verify: impl Copy + Fn(&str, &str, &str) -> Result<(), &'static str>,
    write_and_verify_cache: impl FnOnce(&TrialEnvelope) -> Result<(), &'static str>) -> Result<(), &'static str> {
    let mut saved = load_at(store, protector)?.ok_or(INVALID)?;
    if !matches(&saved, pending) || saved.retirement.is_some() { return Err(INVALID); }
    let issued = URL_SAFE_NO_PAD.decode(saved.issued.as_deref().ok_or(INVALID)?).map_err(|_| INVALID)?;
    let trial = verify_response_with(&saved, &issued, now, verify)?;
    let request = settlement_request(&saved, &trial, now)?;
    verify_receipt(&request, saved.settlement.as_deref().ok_or(INVALID)?, trust,
        wincmd_pq_experiment::Purpose::LicenceTrialSettlement)?;
    let envelope: TrialEnvelope = serde_json::from_slice(&issued).map_err(|_| INVALID)?;
    write_and_verify_cache(&envelope)?;
    if !saved.completed { saved.completed = true; save_at(store, protector, &saved)?; }
    Ok(())
}

/// Unwired cache bridge. The supplied callback must refuse a conflicting paid
/// cache, commit and re-read protected history and cache under the same locks.
pub(crate) fn finalize_locked(pending: &PendingTrial,
    write_and_verify_cache: impl FnOnce(&TrialEnvelope) -> Result<(), &'static str>) -> Result<(), &'static str> {
    let trust = crate::license_cache::configured_device_receipt_trust()?;
    let clock = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs()).map_err(|_| INVALID);
    let now = clock()?;
    finalize_at(&machine(), &MachineDpapi, pending, now, &trust.policy(),
        crate::license_cache::verify_activation_envelope, |envelope| {
            let claims: TrialClaims = serde_json::from_str(&envelope.payload).map_err(|_| INVALID)?;
            if clock()? >= claims.exp { return Err(INVALID); }
            write_and_verify_cache(envelope)?;
            if clock()? >= claims.exp { return Err(INVALID); }
            Ok(())
        })
}

fn historical_retirement_evidence(saved: &PendingTrial, now: u64, trust: &wincmd_pq_experiment::jws::Trust<'_>,
    verify: impl Copy + Fn(&str, &str, &str) -> Result<(), &'static str>) -> Result<TrialEnvelope, &'static str> {
    let issued = URL_SAFE_NO_PAD.decode(saved.issued.as_deref().ok_or(INVALID)?).map_err(|_| INVALID)?;
    let envelope: TrialEnvelope = serde_json::from_slice(&issued).map_err(|_| INVALID)?;
    let claims: TrialClaims = serde_json::from_str(&envelope.payload).map_err(|_| INVALID)?;
    let accepted = saved.issued_accepted_at.ok_or(INVALID)?;
    if accepted > now || (!saved.completed && now < claims.exp) { return Err(INVALID); }
    // Historic verification lets a settled trial be retired even after it
    // expires; it cannot grant fresh entitlement or extend its expiry.
    let trial = verify_response_with(saved, &issued, accepted, verify)?;
    if let Some(receipt) = saved.settlement.as_deref() {
        let request = settlement_request(saved, &trial, accepted)?;
        verify_receipt(&request, receipt, trust, wincmd_pq_experiment::Purpose::LicenceTrialSettlement)?;
    } else if saved.completed { return Err(INVALID); }
    Ok(envelope)
}

/// Exact committed-secret enrolment for an already-expired, fenced trial.
/// This can only support retirement; it never changes the entitlement cache.
fn expired_custody_request(pending: &PendingTrial, now: u64, trust: &wincmd_pq_experiment::jws::Trust<'_>,
    verify: impl Copy + Fn(&str, &str, &str) -> Result<(), &'static str>) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if pending.retirement.as_ref().map_or(true, |value| value.retired) { return Err(INVALID); }
    let envelope = historical_retirement_evidence(pending, now, trust, verify)?;
    let claims: TrialClaims = serde_json::from_str(&envelope.payload).map_err(|_| INVALID)?;
    if now < claims.exp || pending.completed { return Err(INVALID); }
    let accepted = pending.issued_accepted_at.ok_or(INVALID)?;
    let issued = URL_SAFE_NO_PAD.decode(pending.issued.as_deref().ok_or(INVALID)?).map_err(|_| INVALID)?;
    let trial = verify_response_with(pending, &issued, accepted, verify)?;
    pending.custody_request(&trial, accepted)
}

/// Persist a fresh terminal fence before requesting remote retirement. A
/// pending fence blocks local cache use, paid activation and release.
fn begin_retirement_at(store: &Anchor, protector: &dyn Protector, pending: &PendingTrial, now: u64,
    trust: &wincmd_pq_experiment::jws::Trust<'_>, verify: impl Copy + Fn(&str, &str, &str) -> Result<(), &'static str>)
    -> Result<String, &'static str> {
    let _lock = crate::licence_security_lock()?;
    let mut saved = load_at(store, protector)?.ok_or(INVALID)?;
    if !matches(&saved, pending) { return Err(INVALID); }
    historical_retirement_evidence(&saved, now, trust, verify)?;
    if let Some(retirement) = &saved.retirement { return Ok(retirement.id.clone()); }
    let mut random = [0u8; 32];
    rand::rngs::SysRng.try_fill_bytes(&mut random).map_err(|_| INVALID)?;
    let id = random.iter().map(|byte| format!("{byte:02x}")).collect();
    random.zeroize();
    saved.retirement = Some(Retirement { id, receipt: None, retired: false });
    save_at(store, protector, &saved)?;
    Ok(saved.retirement.as_ref().ok_or(INVALID)?.id.clone())
}

fn begin_retirement(pending: &PendingTrial) -> Result<String, &'static str> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| INVALID)?.as_secs();
    let trust = crate::license_cache::configured_device_receipt_trust()?;
    begin_retirement_at(&machine(), &MachineDpapi, pending, now, &trust.policy(),
        crate::license_cache::verify_activation_envelope)
}

/// Caller holds cache WRITE_LOCK then the security lock. The cache callback
/// must remove only this exact trial and verify absence before returning.
/// Remote copies of a previously issued offline token cannot be recalled and
/// may remain usable until the signed token expires.
fn finish_retirement_at(store: &Anchor, protector: &dyn Protector, pending: &PendingTrial, receipt: &str, now: u64,
    trust: &wincmd_pq_experiment::jws::Trust<'_>, verify: impl Copy + Fn(&str, &str, &str) -> Result<(), &'static str>,
    remove_exact_cache: impl FnOnce(&TrialEnvelope) -> Result<(), &'static str>) -> Result<(), &'static str> {
    let mut saved = load_at(store, protector)?.ok_or(INVALID)?;
    if !matches(&saved, pending) { return Err(INVALID); }
    let envelope = historical_retirement_evidence(&saved, now, trust, verify)?;
    let retirement = saved.retirement.as_ref().ok_or(INVALID)?;
    let request = retirement_request(&saved, &retirement.id)?;
    verify_receipt(&request, receipt, trust, wincmd_pq_experiment::Purpose::LicenceTrialRetirement)?;
    if let Some(existing) = &retirement.receipt {
        verify_receipt(&request, existing, trust, wincmd_pq_experiment::Purpose::LicenceTrialRetirement)?;
    }
    remove_exact_cache(&envelope)?;
    let retirement = saved.retirement.as_mut().ok_or(INVALID)?;
    if !retirement.retired {
        retirement.receipt = Some(receipt.into());
        retirement.retired = true;
        save_at(store, protector, &saved)?;
    }
    Ok(())
}

/// Unwired protected-cache bridge; caller holds cache and security locks.
pub(crate) fn finish_retirement_locked(pending: &PendingTrial, receipt: &str, now: u64,
    remove_exact_cache: impl FnOnce(&TrialEnvelope) -> Result<(), &'static str>) -> Result<(), &'static str> {
    let trust = crate::license_cache::configured_device_receipt_trust()?;
    finish_retirement_at(&machine(), &MachineDpapi, pending, receipt, now, &trust.policy(),
        crate::license_cache::verify_activation_envelope, remove_exact_cache)
}

/// Called only while holding the shared cross-process licence security lock.
pub(crate) fn ensure_idle_locked() -> Result<(), &'static str> {
    ensure_idle_at(&machine(), &MachineDpapi)
}

fn ensure_idle_at(store: &Anchor, protector: &dyn Protector) -> Result<(), &'static str> {
    match load_at(store, protector)? {
        None => Ok(()),
        Some(saved) if saved.retirement.as_ref().is_some_and(|value| value.retired)
            || (saved.completed && saved.retirement.is_none()) => Ok(()),
        _ => Err(INVALID),
    }
}

/// A retired device trial must not be restored by replaying an older signed
/// token, including a legacy token without a v4 operation ID.
pub(crate) fn guard_cache_payload_locked(payload: &str) -> Result<(), &'static str> {
    guard_cache_payload_at(&machine(), &MachineDpapi, payload)
}

fn guard_cache_payload_at(store: &Anchor, protector: &dyn Protector, payload: &str) -> Result<(), &'static str> {
    let Some(saved) = load_at(store, protector)? else { return Ok(()); };
    if saved.retirement.is_none() { return Ok(()); }
    let claims: serde_json::Value = serde_json::from_str(payload).map_err(|_| INVALID)?;
    if claims.get("plan").and_then(|v| v.as_str()) == Some("trial")
        && claims.get("device_hash").and_then(|v| v.as_str()) == Some(saved.device_hash.as_str()) {
        return Err(INVALID);
    }
    Ok(())
}

/// This scaffold cannot be exposed yet: the eventual entry point must also
/// check the protected licence cache and retained paid history before starting.
pub(crate) fn begin_or_resume() -> Result<PendingTrial, &'static str> {
    let identity = crate::licence_machine_identity::read_machine_identity()?;
    if identity.portable { return Err(INVALID); }
    begin_at(&machine(), &release_gate::machine(), &MachineDpapi, &identity.device_hash,
        crate::licence_activation_intent::ensure_no_paid_intent_locked)
}

fn recover_pending() -> Result<Option<PendingTrial>, &'static str> {
    let _lock = crate::licence_security_lock()?;
    load_at(&machine(), &MachineDpapi)
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
        return if saved.device_hash == device_hash && saved.retirement.is_none() { Ok(saved) } else { Err(INVALID) };
    }
    let mut operation = [0u8; 32];
    let mut credential_id = [0u8; 32];
    let mut credential_secret = [0u8; 32];
    rand::rngs::SysRng.try_fill_bytes(&mut operation).map_err(|_| INVALID)?;
    rand::rngs::SysRng.try_fill_bytes(&mut credential_id).map_err(|_| INVALID)?;
    rand::rngs::SysRng.try_fill_bytes(&mut credential_secret).map_err(|_| INVALID)?;
    let operation_id = operation.iter().map(|byte| format!("{byte:02x}")).collect();
    operation.zeroize();
    let pending = PendingTrial { operation_id, device_hash: device_hash.into(), credential_id, credential_secret,
        issued: None, issued_accepted_at: None, settlement: None, completed: false, retirement: None };
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
