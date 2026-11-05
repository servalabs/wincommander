// SPDX-License-Identifier: AGPL-3.0-or-later
//! Durable, machine-protected intent before any paid activation request.
//! No caller or network path is connected until identity and settlement are qualified.
use crate::fleet_credential::{MachineDpapi, Protector};
use crate::license_cache::{licence_device_anchor::Anchor, release_gate};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const INVALID: &str = "licence activation is in progress or unavailable";
const ALREADY_COMPLETED: &str = "licence activation already completed; check status";
const PATH: &str = r"SOFTWARE\ServaLabs\WinCommander\HybridLicenceActivation";
const ENTROPY: &[u8] = b"wincommander:licence-activation-intent:v1";
const LIMIT: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope { version: u8, sealed: String }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion { operation_id: String, device_hash: String, portable: bool,
    key_digest: String, payload: String, receipt: String }

pub(crate) struct CompletedEvidence {
    pub pending: PendingActivation,
    pub payload: Vec<u8>,
    pub receipt: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingActivation {
    operation_id: String,
    licence_key: String,
    device_hash: String,
    portable: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivationEnvelope {
    ok: bool,
    payload: String,
    signature: String,
    hybrid: String,
    #[serde(default)]
    seats_used: Option<u32>,
    #[serde(default)]
    seat_limit: Option<u32>,
}

#[derive(Deserialize)]
struct ActivationClaims {
    license_id: String,
    device_hash: String,
    activation_operation_id: String,
    activation_generation: u64,
    is_portable: bool,
    plan: String,
    iss: String,
    sub: String,
    iat: u64,
    exp: u64,
}

pub(crate) struct VerifiedActivation {
    pub payload: String,
    pub signature: String,
    pub hybrid: String,
    pub seats_used: Option<u32>,
    pub seat_limit: Option<u32>,
    pub licence_id: String,
    pub generation: u64,
}

impl Drop for PendingActivation {
    fn drop(&mut self) { self.licence_key.zeroize(); }
}

impl PendingActivation {
    pub(crate) fn operation_id(&self) -> &str { &self.operation_id }
    pub(crate) fn licence_key(&self) -> &str { &self.licence_key }
    pub(crate) fn device_hash(&self) -> &str { &self.device_hash }
    pub(crate) fn portable(&self) -> bool { self.portable }

    /// The same protected intent always yields the same idempotent request.
    pub(crate) fn request_body(&self) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a> {
            license_key: &'a str,
            device_hash: &'a str,
            app_id: &'static str,
            is_portable: bool,
            operation_id: &'a str,
        }
        let mut output = Zeroizing::new(Vec::new());
        serde_json::to_writer(&mut *output, &Request { license_key: &self.licence_key,
            device_hash: &self.device_hash, app_id: "com.servalabs.wincommander",
            is_portable: self.portable, operation_id: &self.operation_id }).map_err(|_| INVALID)?;
        Ok(output)
    }
}

fn valid_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn valid_request(key: &str, device: &str) -> bool {
    !key.is_empty() && key.len() <= 256 && key.trim() == key && valid_hex(device)
}

/// The response must bind both signatures to the exact persisted operation.
/// Verification alone neither saves the cache nor completes the intent.
pub(crate) fn verify_response(pending: &PendingActivation, bytes: &[u8], now: u64)
    -> Result<VerifiedActivation, &'static str> {
    verify_response_with(pending, bytes, now, crate::license_cache::verify_activation_envelope)
}

fn verify_response_with(pending: &PendingActivation, bytes: &[u8], now: u64,
    verify: impl FnOnce(&str, &str, &str) -> Result<(), &'static str>)
    -> Result<VerifiedActivation, &'static str> {
    if bytes.is_empty() || bytes.len() > 64 * 1024 { return Err(INVALID); }
    let envelope: ActivationEnvelope = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if !envelope.ok || envelope.payload.is_empty() || envelope.signature.is_empty() || envelope.hybrid.is_empty() {
        return Err(INVALID);
    }
    verify(&envelope.payload, &envelope.signature, &envelope.hybrid)?;
    let claims: ActivationClaims = serde_json::from_str(&envelope.payload).map_err(|_| INVALID)?;
    if claims.license_id.is_empty() || claims.license_id.len() > 256 || claims.device_hash != pending.device_hash
        || claims.activation_operation_id != pending.operation_id || claims.is_portable != pending.portable
        || claims.activation_generation == 0 || claims.activation_generation > 9_007_199_254_740_991
        || claims.plan.is_empty() || claims.plan == "trial" || claims.iss != "wincommander-license-worker"
        || claims.sub != "com.servalabs.wincommander" || claims.iat > now.saturating_add(300)
        || claims.exp <= now || claims.iat > claims.exp {
        return Err(INVALID);
    }
    Ok(VerifiedActivation { payload: envelope.payload, signature: envelope.signature, hybrid: envelope.hybrid,
        seats_used: envelope.seats_used, seat_limit: envelope.seat_limit,
        licence_id: claims.license_id, generation: claims.activation_generation })
}

/// Exact cross-language receipt payload; the licence-key hash is never exposed.
pub(crate) fn acknowledgement_payload(pending: &PendingActivation, verified: &VerifiedActivation,
    credential_id: &[u8; 32]) -> Result<Vec<u8>, &'static str> {
    use sha2::{Digest, Sha256};
    #[derive(Serialize)]
    struct Payload<'a> {
        kind: &'static str,
        operation_id: &'a str,
        activation_digest: String,
        licence_id: &'a str,
        device_hash: &'a str,
        issuer_origin: &'static str,
        credential_id: String,
        generation: u64,
    }
    let digest = Sha256::digest(verified.payload.as_bytes());
    let digest = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    serde_json::to_vec(&Payload { kind: "licence-activation-ack-v1", operation_id: &pending.operation_id,
        activation_digest: digest, licence_id: &verified.licence_id, device_hash: &pending.device_hash,
        issuer_origin: "https://wincommander-licensing.servalabs.com", credential_id: URL_SAFE_NO_PAD.encode(credential_id),
        generation: verified.generation }).map_err(|_| INVALID)
}

fn machine() -> Anchor { Anchor::machine_named(PATH) }

fn unseal_at(store: &Anchor, protector: &dyn Protector) -> Result<Option<(u8, Zeroizing<Vec<u8>>)>, &'static str> {
    if !store.exists().map_err(|_| INVALID)? { return Ok(None); }
    let bytes = store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
    if bytes.len() > LIMIT { return Err(INVALID); }
    let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    if !matches!(envelope.version, 1 | 2) || envelope.sealed.len() > LIMIT { return Err(INVALID); }
    let sealed = URL_SAFE_NO_PAD.decode(&envelope.sealed).map_err(|_| INVALID)?;
    if sealed.is_empty() || URL_SAFE_NO_PAD.encode(&sealed) != envelope.sealed { return Err(INVALID); }
    let plaintext = protector.unprotect(&sealed, ENTROPY).map_err(|_| INVALID)?;
    Ok(Some((envelope.version, plaintext)))
}

fn completed_at(store: &Anchor, protector: &dyn Protector) -> Result<Option<Completion>, &'static str> {
    let Some((version, plaintext)) = unseal_at(store, protector)? else { return Ok(None); };
    if version == 1 { return Ok(None); }
    let completed: Completion = serde_json::from_slice(&plaintext).map_err(|_| INVALID)?;
    let payload = URL_SAFE_NO_PAD.decode(&completed.payload).map_err(|_| INVALID)?;
    if !valid_hex(&completed.operation_id) || !valid_hex(&completed.device_hash) || !valid_hex(&completed.key_digest)
        || payload.is_empty() || payload.len() > 16 * 1024 || URL_SAFE_NO_PAD.encode(payload) != completed.payload
        || completed.receipt.is_empty() || completed.receipt.len() > 32 * 1024 { return Err(INVALID); }
    Ok(Some(completed))
}

fn load_at(store: &Anchor, protector: &dyn Protector) -> Result<Option<PendingActivation>, &'static str> {
    let Some((version, plaintext)) = unseal_at(store, protector)? else { return Ok(None); };
    if version == 2 { completed_at(store, protector)?.ok_or(INVALID)?; return Ok(None); }
    let pending: PendingActivation = serde_json::from_slice(&plaintext).map_err(|_| INVALID)?;
    if !valid_hex(&pending.operation_id) || !valid_request(&pending.licence_key, &pending.device_hash) {
        return Err(INVALID);
    }
    Ok(Some(pending))
}

/// Caller holds the shared cross-process licence lock. A damaged or empty
/// activation key is never treated as absent.
pub(crate) fn ensure_idle_locked() -> Result<(), &'static str> {
    ensure_idle_at(&machine())
}

fn ensure_idle_at(store: &Anchor) -> Result<(), &'static str> {
    if load_at(store, &MachineDpapi)?.is_some() { Err(INVALID) } else { Ok(()) }
}

/// Caller holds the cross-process licence lock and has reverified the exact
/// cache, credential and hybrid-signed acknowledgement. Never stores the key.
pub(crate) fn complete_locked(pending: &PendingActivation, payload: &[u8], receipt: &str)
    -> Result<(), &'static str> {
    complete_at(&machine(), &MachineDpapi, pending, payload, receipt)
}

fn complete_at(store: &Anchor, protector: &dyn Protector, pending: &PendingActivation,
    payload: &[u8], receipt: &str) -> Result<(), &'static str> {
    ensure_matches_at(store, protector, pending)?;
    if payload.is_empty() || payload.len() > 16 * 1024 || receipt.is_empty() || receipt.len() > 32 * 1024 {
        return Err(INVALID);
    }
    use sha2::{Digest, Sha256};
    let key_digest: String = Sha256::digest(pending.licence_key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let value = Completion { operation_id: pending.operation_id.clone(), device_hash: pending.device_hash.clone(),
        portable: pending.portable, key_digest, payload: URL_SAFE_NO_PAD.encode(payload), receipt: receipt.into() };
    let plaintext = Zeroizing::new(serde_json::to_vec(&value).map_err(|_| INVALID)?);
    let sealed = protector.protect(&plaintext, ENTROPY).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let record = serde_json::to_vec(&Envelope { version: 2, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    store.write(&record).map_err(|_| INVALID)?;
    if store.read().map_err(|_| INVALID)?.as_deref() != Some(record.as_slice()) || load_at(store, protector)?.is_some() {
        return Err(INVALID);
    }
    Ok(())
}

/// Create before a network request, or resume only the exact previous request.
/// Neither timeout nor process restart clears this protected state.
pub(crate) fn begin_or_resume(key: &str, device: &str, portable: bool) -> Result<PendingActivation, &'static str> {
    begin_at(&machine(), &release_gate::machine(), &MachineDpapi, key, device, portable)
}

/// Service restart recovery needs no desktop-supplied key or operation ID.
pub(crate) fn recover_pending() -> Result<Option<PendingActivation>, &'static str> {
    let _lock = crate::licence_security_lock()?;
    release_gate::ensure_idle()?;
    load_at(&machine(), &MachineDpapi)
}

fn ensure_current(pending: &PendingActivation) -> Result<(), &'static str> {
    let _lock = crate::licence_security_lock()?;
    release_gate::ensure_idle()?;
    ensure_matches_locked(pending)
}

/// Caller holds the cross-process licence lock.
pub(crate) fn ensure_matches_locked(pending: &PendingActivation) -> Result<(), &'static str> {
    ensure_matches_at(&machine(), &MachineDpapi, pending)
}

/// Recheck completed evidence under the same lock as cache and credential.
pub(crate) fn ensure_completed_matches_locked(pending: &PendingActivation, payload: &[u8], receipt: &str)
    -> Result<(), &'static str> {
    let completed = completed_at(&machine(), &MachineDpapi)?.ok_or(INVALID)?;
    if !completed_matches(&completed, pending.licence_key(), pending.device_hash(), pending.portable())
        || completed.operation_id != pending.operation_id
        || completed.payload != URL_SAFE_NO_PAD.encode(payload) || completed.receipt != receipt { return Err(INVALID); }
    Ok(())
}

fn ensure_matches_at(store: &Anchor, protector: &dyn Protector, pending: &PendingActivation) -> Result<(), &'static str> {
    let saved = load_at(store, protector)?.ok_or(INVALID)?;
    if saved.operation_id != pending.operation_id || saved.device_hash != pending.device_hash
        || saved.portable != pending.portable
        || !bool::from(saved.licence_key.as_bytes().ct_eq(pending.licence_key.as_bytes())) {
        return Err(INVALID);
    }
    Ok(())
}

/// Send only after reopening the exact protected intent. All failures preserve
/// that intent; a successful verification is not yet a durable cache handoff.
pub(crate) async fn send_pending(pending: &PendingActivation) -> Result<VerifiedActivation, &'static str> {
    ensure_current(pending)?;
    let body = pending.request_body()?;
    let response = crate::licence_device_credential::post_activation(body).await?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| INVALID)?.as_secs();
    verify_response(pending, &response, now)
}

/// The service owns identity, intent, transport, protected cache and enrolment.
/// No failure clears intent; only a verified signed settlement completes it.
pub(crate) async fn activate_from_key(key: &str) -> Result<(), &'static str> {
    let identity = tokio::task::spawn_blocking(crate::licence_machine_identity::read_machine_identity)
        .await.map_err(|_| INVALID)??;
    let pending = match begin_or_resume(key, &identity.device_hash, identity.portable) {
        Ok(pending) => pending,
        Err(ALREADY_COMPLETED) => {
            let evidence = completed_for_key(key, &identity.device_hash, identity.portable)?.ok_or(INVALID)?;
            if crate::licence_device_credential::verify_completed_activation(&evidence).is_ok() {
                return Ok(());
            }
            let pending = crate::licence_device_release_pending::restart_after_completed_release(&evidence)?;
            let verified = send_pending(&pending).await?;
            crate::license_cache::store_activation(&pending, &verified)?;
            crate::licence_device_credential::enrol_from_machine_cache(pending.licence_key()).await?;
            return crate::licence_device_credential::acknowledge_activation(&pending, &verified).await;

        }
        Err(error) => return Err(error),
    };
    let verified = send_pending(&pending).await?;
    crate::license_cache::store_activation(&pending, &verified)?;
    crate::licence_device_credential::enrol_from_machine_cache(pending.licence_key()).await?;
    crate::licence_device_credential::acknowledge_activation(&pending, &verified).await
}

fn begin_at(store: &Anchor, gate: &Anchor, protector: &dyn Protector,
    key: &str, device: &str, portable: bool) -> Result<PendingActivation, &'static str> {
    if !valid_request(key, device) { return Err(INVALID); }
    let _lock = crate::licence_security_lock()?;
    begin_locked(store, gate, protector, key, device, portable, false)
}

fn begin_locked(store: &Anchor, gate: &Anchor, protector: &dyn Protector,
    key: &str, device: &str, portable: bool, released: bool) -> Result<PendingActivation, &'static str> {
    release_gate::ensure_idle_at(gate)?;
    if let Some(completed) = completed_at(store, protector)? {
        if !released && completed_matches(&completed, key, device, portable) {
            // A successful acknowledgement may have been followed by a lost IPC
            // reply. Never allocate a new server generation for that retry.
            return Err(ALREADY_COMPLETED);
        }
        // Another key must not allocate a seat while the completed generation
        // has not been proven released. Cross-key handoff is fail-closed.
        if !released { return Err(INVALID); }
    }
    if let Some(pending) = load_at(store, protector)? {
        if pending.device_hash != device || pending.portable != portable
            || !bool::from(pending.licence_key.as_bytes().ct_eq(key.as_bytes())) {
            return Err(INVALID);
        }
        return Ok(pending);
    }
    let mut random = [0u8; 32];
    rand::rngs::OsRng.try_fill_bytes(&mut random).map_err(|_| INVALID)?;
    use std::fmt::Write as _;
    let mut operation_id = String::with_capacity(64);
    for byte in random { write!(&mut operation_id, "{byte:02x}").map_err(|_| INVALID)?; }
    random.zeroize();
    let pending = PendingActivation { operation_id, licence_key: key.into(), device_hash: device.into(), portable };
    let plaintext = Zeroizing::new(serde_json::to_vec(&pending).map_err(|_| INVALID)?);
    let sealed = protector.protect(&plaintext, ENTROPY).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let record = serde_json::to_vec(&Envelope { version: 1, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    store.write(&record).map_err(|_| INVALID)?;
    let saved = load_at(store, protector)?.ok_or(INVALID)?;
    if saved.operation_id != pending.operation_id || saved.device_hash != pending.device_hash
        || saved.portable != pending.portable
        || !bool::from(saved.licence_key.as_bytes().ct_eq(pending.licence_key.as_bytes())) {
        return Err(INVALID);
    }
    Ok(saved)
}


/// Only called while the completed release and credential are locked and verified.
/// Match the signed activation settlement to the exact released generation before
/// replacing its completion marker with a fresh, durable operation intent.
pub(crate) fn restart_after_release_locked(evidence: &CompletedEvidence,
    journal: &crate::license_cache::release_gate::PreparedRelease,
    trust: &wincmd_pq_experiment::jws::Trust<'_>) -> Result<PendingActivation, &'static str> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Ack { kind: String, operation_id: String, activation_digest: String,
        licence_id: String, device_hash: String, issuer_origin: String, credential_id: String, generation: u64 }
    ensure_completed_matches_locked(&evidence.pending, &evidence.payload, &evidence.receipt)?;
    let policy = wincmd_pq_experiment::jws::Trust {
        purpose: wincmd_pq_experiment::Purpose::LicenceActivationAck,
        minimum_epoch: trust.minimum_epoch, ed_key_id: trust.ed_key_id,
        ed25519_public: trust.ed25519_public, pq_key_id: trust.pq_key_id,
        ml_dsa_65_public: trust.ml_dsa_65_public,
    };
    let verified = wincmd_pq_experiment::jws::verify(evidence.receipt.as_bytes(), &policy).map_err(|_| INVALID)?;
    if verified.payload != evidence.payload { return Err(INVALID); }
    let ack: Ack = serde_json::from_slice(&evidence.payload).map_err(|_| INVALID)?;
    if ack.kind != "licence-activation-ack-v1" || ack.operation_id != evidence.pending.operation_id
        || !valid_hex(&ack.activation_digest) || ack.licence_id != journal.licence_id
        || ack.device_hash != journal.device_hash || ack.device_hash != evidence.pending.device_hash
        || ack.issuer_origin != "https://wincommander-licensing.servalabs.com"
        || ack.credential_id != URL_SAFE_NO_PAD.encode(journal.credential_id)
        || ack.generation != journal.generation { return Err(INVALID); }
    begin_locked(&machine(), &release_gate::machine(), &MachineDpapi, evidence.pending.licence_key(),
        evidence.pending.device_hash(), evidence.pending.portable(), true)
}

fn completed_matches(completed: &Completion, key: &str, device: &str, portable: bool) -> bool {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    completed.device_hash == device && completed.portable == portable
        && bool::from(completed.key_digest.as_bytes().ct_eq(digest.as_bytes()))
}

fn completed_for_key(key: &str, device: &str, portable: bool) -> Result<Option<CompletedEvidence>, &'static str> {
    let _lock = crate::licence_security_lock()?;
    release_gate::ensure_idle()?;
    let Some(completed) = completed_at(&machine(), &MachineDpapi)? else { return Ok(None); };
    if !completed_matches(&completed, key, device, portable) { return Err(INVALID); }
    let payload = URL_SAFE_NO_PAD.decode(&completed.payload).map_err(|_| INVALID)?;
    Ok(Some(CompletedEvidence { pending: PendingActivation { operation_id: completed.operation_id,
        licence_key: key.into(), device_hash: device.into(), portable }, payload, receipt: completed.receipt }))
}

#[cfg(test)]
#[path = "licence_activation_intent_tests.rs"]
mod tests;
