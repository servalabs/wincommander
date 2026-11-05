// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protected, unexposed retry state for an experimental device release.
use super::{anchor, decode_confirmed, entropy, selected_machine_anchor_locked, wire, Binding, ConfirmedCredential, RefreshLicence, INVALID, LOCK};
use crate::fleet_credential::{MachineDpapi, Protector};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const LIMIT: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record { version: u8, sealed: String }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmedRecord { phase: String, request: String }

impl Drop for ConfirmedRecord {
    fn drop(&mut self) { use zeroize::Zeroize; self.request.zeroize(); }
}

pub(super) struct SavedRelease {
    pub request: Zeroizing<Vec<u8>>,
    pub confirmed: bool,
    pub acknowledged: bool,
}

fn release_entropy(binding: &Binding<'_>, credential: &ConfirmedCredential, body: &[u8]) -> Result<[u8; 32], &'static str> {
    // Validate the identity fields using the credential store's existing rules.
    entropy(binding, &credential.id)?;
    let body_hash: [u8; 32] = Sha256::digest(body).into();
    let value = serde_json::to_vec(&("wincommander:licence-release-retry:v1", binding.licence_id,
        binding.device_hash, binding.issuer_origin, credential.id, body_hash)).map_err(|_| INVALID)?;
    Ok(Sha256::digest(value).into())
}

fn release_anchor(binding: &Binding<'_>) -> Result<anchor::Anchor, &'static str> {
    use std::fmt::Write as _;
    let digest = entropy(binding, &[0; 32])?;
    let mut suffix = String::with_capacity(64);
    for byte in digest { write!(&mut suffix, "{byte:02x}").map_err(|_| INVALID)?; }
    Ok(anchor::Anchor::machine_named(&format!(r"SOFTWARE\ServaLabs\WinCommander\ExperimentalLicenceRelease-{suffix}")))
}

// Callers hold the credential/cache coordination locks when using machine state.
fn load_locked(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential, body: &[u8],
) -> Result<Option<SavedRelease>, &'static str> {
    let Some(bytes) = store.read().map_err(|_| INVALID)? else { return Ok(None); };
    if bytes.len() > LIMIT { return Err(INVALID); }
    let record: Record = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    if !matches!(record.version, 1 | 2) || record.sealed.is_empty() || record.sealed.len() > LIMIT { return Err(INVALID); }
    let sealed = URL_SAFE_NO_PAD.decode(&record.sealed).map_err(|_| INVALID)?;
    if URL_SAFE_NO_PAD.encode(&sealed) != record.sealed { return Err(INVALID); }
    let plaintext = protector.unprotect(&sealed, &release_entropy(binding, credential, body)?).map_err(|_| INVALID)?;
    let (request, confirmed, acknowledged) = if record.version == 1 { (plaintext, false, false) } else {
        let value: ConfirmedRecord = serde_json::from_slice(&plaintext).map_err(|_| INVALID)?;
        if !matches!(value.phase.as_str(), "confirmed" | "acknowledged") || value.request.len() > LIMIT { return Err(INVALID); }
        let decoded = Zeroizing::new(URL_SAFE_NO_PAD.decode(&value.request).map_err(|_| INVALID)?);
        if URL_SAFE_NO_PAD.encode(decoded.as_slice()) != value.request { return Err(INVALID); }
        (decoded, true, value.phase == "acknowledged")
    };
    wire::verify_stored_release_request(credential, body, &request)?;
    Ok(Some(SavedRelease { request, confirmed, acknowledged }))
}

fn save_locked(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    save_locked_replacing(store, protector, binding, credential, body, request, None)
}

fn save_locked_replacing(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], request: &[u8], previous: Option<&[u8]>,
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    wire::verify_stored_release_request(credential, body, request)?;
    if let Some(expected) = previous {
        if store.read().map_err(|_| INVALID)?.as_deref() != Some(expected) { return Err(INVALID); }
    } else if let Some(existing) = load_locked(store, protector, binding, credential, body)? {
        return if existing.request.as_slice() == request { Ok(existing.request) } else { Err(INVALID) };
    }
    let sealed = protector.protect(request, &release_entropy(binding, credential, body)?).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let bytes = serde_json::to_vec(&Record { version: 1, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    if bytes.len() > LIMIT { return Err(INVALID); }
    store.write(&bytes).map_err(|_| INVALID)?;
    let reopened = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if reopened.confirmed || reopened.request.as_slice() != request { return Err(INVALID); }
    Ok(reopened.request)
}

/// A failed decryption is not enough to replace a record: protected history
/// must identify these exact bytes as a completed older activation.
fn completed_previous_record(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], licence: &RefreshLicence<'_>,
) -> Result<Option<Vec<u8>>, &'static str> {
    completed_previous_record_with(store, protector, binding, credential, body, |bytes| {
        crate::license_cache::can_replace_completed_release_locked(licence.payload, licence.signature, licence.hybrid,
            licence.last_verified_at, &credential.id, bytes)
    })
}

fn completed_previous_record_with(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], authorize: impl FnOnce(&[u8]) -> Result<bool, &'static str>,
) -> Result<Option<Vec<u8>>, &'static str> {
    let Some(bytes) = store.read().map_err(|_| INVALID)? else { return Ok(None); };
    match load_locked(store, protector, binding, credential, body) {
        Ok(Some(_)) => return Ok(None),
        Ok(None) => return Err(INVALID),
        Err(_) => {}
    }
    if authorize(&bytes)? { Ok(Some(bytes)) } else { Err(INVALID) }
}

fn confirm_locked(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], request: &[u8], response: &[u8],
) -> Result<(), &'static str> {
    wire::confirmed_release(response)?;
    let saved = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if saved.request.as_slice() != request { return Err(INVALID); }
    if saved.confirmed { return Ok(()); }
    save_confirmed_phase(store, protector, binding, credential, body, request, "confirmed")
}

fn save_confirmed_phase(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], request: &[u8], phase: &str,
) -> Result<(), &'static str> {
    if !matches!(phase, "confirmed" | "acknowledged") { return Err(INVALID); }
    let value = ConfirmedRecord { phase: phase.into(), request: URL_SAFE_NO_PAD.encode(request) };
    let plaintext = Zeroizing::new(serde_json::to_vec(&value).map_err(|_| INVALID)?);
    let sealed = protector.protect(&plaintext, &release_entropy(binding, credential, body)?).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let bytes = serde_json::to_vec(&Record { version: 2, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    if bytes.len() > LIMIT { return Err(INVALID); }
    store.write(&bytes).map_err(|_| INVALID)?;
    let reopened = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if !reopened.confirmed || reopened.acknowledged != (phase == "acknowledged") || reopened.request.as_slice() != request { return Err(INVALID); }
    Ok(())
}

/// Persist the acknowledgement only after the confirmed request is reopened
/// under the exact current credential/cache locks. A failed write retains the
/// previous state or can be confirmed by reopening on a retry.
fn acknowledge_locked(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], request: &[u8], confirmation: &wire::AcknowledgementConfirmation,
) -> Result<(), &'static str> {
    let saved = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if !saved.confirmed || saved.request.as_slice() != request { return Err(INVALID); }
    let acknowledgement = wire::release_acknowledgement_request(credential, body, request)?;
    if !confirmation.matches(&acknowledgement) { return Err(INVALID); }
    if saved.acknowledged { return Ok(()); }
    save_confirmed_phase(store, protector, binding, credential, body, request, "acknowledged")
}

fn with_current<T>(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
    op: impl FnOnce(&anchor::Anchor, &ConfirmedCredential, &[u8]) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    if binding.issuer_origin != wire::LICENCE_ORIGIN { return Err(INVALID); }
    // Same lock order as credential confirmation: credential, cache, process.
    let _guard = LOCK.lock().map_err(|_| INVALID)?;
    crate::license_cache::with_verified_release_wire(licence.payload, licence.signature, licence.hybrid,
        licence.last_verified_at, binding.licence_id, binding.device_hash, |body| {
            let source = selected_machine_anchor_locked(binding, trust, now)?;
            let bytes = source.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
            let credential = decode_confirmed(&MachineDpapi, binding, trust, &bytes, now)?;
            op(&release_anchor(binding)?, &credential, &body)
        })
}

fn prepare_finalization_locked(
    binding: &Binding<'_>, credential: &ConfirmedCredential, body: &[u8], request: &[u8], licence: &RefreshLicence<'_>,
) -> Result<(), &'static str> {
    let context = release_entropy(binding, credential, body)?;
    let gate = crate::license_cache::release_gate::machine();
    crate::license_cache::release_gate::check_request_at(&gate, &context, Some(request))?;
    crate::license_cache::mark_acknowledged_release_locked(licence.payload, licence.signature, licence.hybrid, licence.last_verified_at)?;
    let (cache, generation) = crate::license_cache::released_snapshot_locked(licence.payload, licence.signature, licence.hybrid, licence.last_verified_at)?;
    crate::license_cache::release_gate::prepare_finalization_at(&gate, &context, request, binding.licence_id,
        binding.device_hash, &credential.id, generation, &cache)
}

/// Reverify all protected evidence, then remove only the exact cached licence.
/// The gate, credential and acknowledgement remain for retry and finalization.
pub(crate) fn remove_prepared_cache(
    trust: &wincmd_pq_experiment::jws::Trust<'_>, now: u64,
) -> Result<bool, &'static str> {
    let _guard = LOCK.lock().map_err(|_| INVALID)?;
    let verified = crate::license_cache::with_prepared_release_recovery(|journal, snapshot, body| {
        let binding = Binding { licence_id: &journal.licence_id, device_hash: &journal.device_hash,
            issuer_origin: wire::LICENCE_ORIGIN };
        let credential_anchor = selected_machine_anchor_locked(&binding, trust, now)?;
        let bytes = credential_anchor.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
        let credential = decode_confirmed(&MachineDpapi, &binding, trust, &bytes, now)?;
        let release_store = release_anchor(&binding)?;
        let before = release_store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
        let saved = verify_prepared_ack(&release_store, &crate::license_cache::release_gate::machine(),
            &MachineDpapi, &binding, &credential, &body, journal)?;
        let after = release_store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
        if before != after { return Err(INVALID); }
        crate::license_cache::remove_prepared_cache_locked(journal)?;
        crate::license_cache::mark_release_completed_locked(journal, snapshot, &saved.request, &after)?;
        crate::license_cache::release_gate::finish_at(&crate::license_cache::release_gate::machine(), journal, &saved.request)
    })?;
    Ok(verified.is_some())
}

/// Resolve an uncertain completion response only from an exact completed
/// journal, released floor and retained acknowledged credential, with no cache.
pub(crate) fn verify_completed_recovery(
    trust: &wincmd_pq_experiment::jws::Trust<'_>, now: u64,
) -> Result<bool, &'static str> {
    let _guard = LOCK.lock().map_err(|_| INVALID)?;
    let verified = crate::license_cache::with_completed_release_recovery(|journal, snapshot, body| {
        let binding = Binding { licence_id: &journal.licence_id, device_hash: &journal.device_hash,
            issuer_origin: wire::LICENCE_ORIGIN };
        let credential_anchor = selected_machine_anchor_locked(&binding, trust, now)?;
        let bytes = credential_anchor.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
        let credential = decode_confirmed(&MachineDpapi, &binding, trust, &bytes, now)?;
        let release_store = release_anchor(&binding)?;
        let before = release_store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
        let saved = verify_prepared_ack(&release_store, &crate::license_cache::release_gate::machine(),
            &MachineDpapi, &binding, &credential, &body, journal)?;
        let after = release_store.read().map_err(|_| INVALID)?.ok_or(INVALID)?;
        if before != after { return Err(INVALID); }
        crate::license_cache::verify_release_completed_locked(journal, snapshot, &saved.request, &after)
    })?;
    Ok(verified.is_some())
}

fn verify_prepared_ack(
    store: &anchor::Anchor, gate: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>,
    credential: &ConfirmedCredential, body: &[u8], journal: &crate::license_cache::release_gate::PreparedRelease,
) -> Result<SavedRelease, &'static str> {
    if credential.id != journal.credential_id || binding.licence_id != journal.licence_id
        || binding.device_hash != journal.device_hash || release_entropy(binding, credential, body)? != journal.context {
        return Err(INVALID);
    }
    let saved = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if !saved.acknowledged { return Err(INVALID); }
    crate::license_cache::release_gate::check_request_proof_at(gate, &journal.context, &saved.request)?;
    Ok(saved)
}

/// Save and read back before any network send. Existing state cannot be replaced.
pub(crate) fn stage(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64, request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body| {
        let previous = completed_previous_record(store, &MachineDpapi, binding, credential, body, licence)?;
        stage_guarded_replacing(store, &crate::license_cache::release_gate::machine(), &MachineDpapi,
            binding, credential, body, request, previous.as_deref())
    })
}

fn stage_guarded(store: &anchor::Anchor, gate: &anchor::Anchor, protector: &dyn Protector,
    binding: &Binding<'_>, credential: &ConfirmedCredential, body: &[u8], request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    stage_guarded_replacing(store, gate, protector, binding, credential, body, request, None)
}

fn stage_guarded_replacing(store: &anchor::Anchor, gate: &anchor::Anchor, protector: &dyn Protector,
    binding: &Binding<'_>, credential: &ConfirmedCredential, body: &[u8], request: &[u8], previous: Option<&[u8]>,
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    wire::verify_stored_release_request(credential, body, request)?;
    let context = release_entropy(binding, credential, body)?;
    crate::license_cache::release_gate::begin_at(gate, &context)?;
    let saved = save_locked_replacing(store, protector, binding, credential, body, request, previous)?;
    crate::license_cache::release_gate::bind_request_at(gate, &context, &saved)?;
    Ok(saved)
}

/// A caller may retry these exact bytes; the local licence is never cleared here.
pub(crate) fn load(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
) -> Result<Option<SavedRelease>, &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body| {
        if completed_previous_record(store, &MachineDpapi, binding, credential, body, licence)?.is_some() {
            let context = release_entropy(binding, credential, body)?;
            crate::license_cache::release_gate::check_request_at(&crate::license_cache::release_gate::machine(), &context, None)?;
            return Ok(None);
        }
        let saved = load_guarded(store, &crate::license_cache::release_gate::machine(), &MachineDpapi,
            binding, credential, body)?;
        if let Some(item) = saved.as_ref().filter(|item| item.acknowledged) {
            prepare_finalization_locked(binding, credential, body, &item.request, licence)?;
        }
        Ok(saved)
    })
}

fn load_guarded(store: &anchor::Anchor, gate: &anchor::Anchor, protector: &dyn Protector,
    binding: &Binding<'_>, credential: &ConfirmedCredential, body: &[u8],
) -> Result<Option<SavedRelease>, &'static str> {
    let saved = load_locked(store, protector, binding, credential, body)?;
    let context = release_entropy(binding, credential, body)?;
    if let Some(item) = &saved {
        crate::license_cache::release_gate::bind_request_at(gate, &context, &item.request)?;
    } else {
        crate::license_cache::release_gate::check_request_at(gate, &context, None)?;
    }
    Ok(saved)
}

/// Record the exact confirmed request and read it back before acknowledgement.
pub(crate) fn confirm(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
    request: &[u8], response: &[u8],
) -> Result<(), &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body| {
        let context = release_entropy(binding, credential, body)?;
        crate::license_cache::release_gate::check_request(&context, Some(request))?;
        confirm_locked(store, &MachineDpapi, binding, credential, body, request, response)
    })
}

/// No acknowledgement is generated until the protected confirmed state exists.
pub(crate) fn prepare_acknowledgement(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body| {
        let saved = load_locked(store, &MachineDpapi, binding, credential, body)?.ok_or(INVALID)?;
        let context = release_entropy(binding, credential, body)?;
        crate::license_cache::release_gate::check_request(&context, Some(&saved.request))?;
        prepare_ack_locked(store, &MachineDpapi, binding, credential, body)
    })
}

/// Retain the acknowledged phase before any future local finalization. The
/// caller must have sent the acknowledgement for this same confirmed request.
/// This does not delete the licence or authorize an IPC caller.
pub(crate) fn record_acknowledgement(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
    request: &[u8], confirmation: &wire::AcknowledgementConfirmation,
) -> Result<(), &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body| {
        let context = release_entropy(binding, credential, body)?;
        crate::license_cache::release_gate::check_request(&context, Some(request))?;
        acknowledge_locked(store, &MachineDpapi, binding, credential, body, request, confirmation)?;
        prepare_finalization_locked(binding, credential, body, request, licence)
    })
}

fn prepare_ack_locked(store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>,
    credential: &ConfirmedCredential, body: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let saved = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if !saved.confirmed { return Err(INVALID); }
    wire::release_acknowledgement_request(credential, body, &saved.request)
}

/// Disposable HKCU/DPAPI fixture for the signed-ledger integration test.
#[cfg(test)]
pub(crate) fn test_make_acknowledged_record(body: &[u8]) -> ([u8; 32], Vec<u8>, Vec<u8>) {
    let path = format!(r"SOFTWARE\WinCommanderReleaseIntegrationOld-{}", uuid::Uuid::new_v4());
    let gate_path = format!(r"SOFTWARE\WinCommanderReleaseIntegrationGate-{}", uuid::Uuid::new_v4());
    let store = anchor::Anchor::test_user(path.clone());
    let gate = anchor::Anchor::test_user(gate_path.clone());
    let binding = Binding { licence_id: "lic-a", device_hash: "device", issuer_origin: "https://license.example" };
    let credential = ConfirmedCredential { id: [3; 32], secret: Zeroizing::new([6; 32]) };
    let context = release_entropy(&binding, &credential, body).unwrap();
    let digest: [u8; 32] = Sha256::digest(body).into();
    let challenge = serde_json::to_vec(&serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(credential.id),
        "nonce": URL_SAFE_NO_PAD.encode([7; 32]), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": 1060 })).unwrap();
    let request = wire::release_request(&credential, body.to_vec(), &challenge, 1000).unwrap();
    stage_guarded(&store, &gate, &MachineDpapi, &binding, &credential, body, &request).unwrap();
    confirm_locked(&store, &MachineDpapi, &binding, &credential, body, &request, br#"{"ok":true}"#).unwrap();
    let ack = prepare_ack_locked(&store, &MachineDpapi, &binding, &credential, body).unwrap();
    let proof = wire::test_acknowledgement_confirmation(&ack, br#"{"ok":true}"#).unwrap();
    acknowledge_locked(&store, &MachineDpapi, &binding, &credential, body, &request, &proof).unwrap();
    let record = store.read().unwrap().unwrap();
    anchor::remove_test_key(&path).unwrap();
    anchor::remove_test_key(&gate_path).unwrap();
    (context, request.to_vec(), record)
}

/// Exercises real DPAPI and registry writes with the real ledger authorizer
/// supplied by the calling test; this does not use production HKLM keys.
#[cfg(test)]
pub(crate) fn test_rotate_completed_record(
    previous: &[u8], body: &[u8], authorize: impl Fn(&[u8]) -> Result<bool, &'static str>,
) {
    let path = format!(r"SOFTWARE\WinCommanderReleaseIntegrationNew-{}", uuid::Uuid::new_v4());
    let gate_path = format!(r"SOFTWARE\WinCommanderReleaseIntegrationGate-{}", uuid::Uuid::new_v4());
    let store = anchor::Anchor::test_user(path.clone());
    let gate = anchor::Anchor::test_user(gate_path.clone());
    let binding = Binding { licence_id: "lic-a", device_hash: "device", issuer_origin: "https://license.example" };
    let credential = ConfirmedCredential { id: [3; 32], secret: Zeroizing::new([6; 32]) };
    store.write(previous).unwrap();
    let allowed = completed_previous_record_with(&store, &MachineDpapi, &binding, &credential, body, |bytes| authorize(bytes)).unwrap().unwrap();
    assert_eq!(allowed, previous);
    let digest: [u8; 32] = Sha256::digest(body).into();
    let challenge = serde_json::to_vec(&serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(credential.id),
        "nonce": URL_SAFE_NO_PAD.encode([8; 32]), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": 1060 })).unwrap();
    let request = wire::release_request(&credential, body.to_vec(), &challenge, 1000).unwrap();
    let context = release_entropy(&binding, &credential, body).unwrap();
    // Crash after the gate, then after the new sealed record but before binding.
    crate::license_cache::release_gate::begin_at(&gate, &context).unwrap();
    assert_eq!(store.read().unwrap().unwrap(), previous);
    let reopened_store = anchor::Anchor::test_user(path.clone());
    let reopened_gate = anchor::Anchor::test_user(gate_path.clone());
    crate::license_cache::release_gate::check_request_at(&reopened_gate, &context, None).unwrap();
    let retry = completed_previous_record_with(&reopened_store, &MachineDpapi, &binding, &credential, body, |bytes| authorize(bytes)).unwrap().unwrap();
    assert_eq!(retry, allowed);
    save_locked_replacing(&reopened_store, &MachineDpapi, &binding, &credential, body, &request, Some(&retry)).unwrap();
    let resumed = load_guarded(&reopened_store, &reopened_gate, &MachineDpapi, &binding, &credential, body).unwrap().unwrap();
    assert_eq!(resumed.request.as_slice(), request);
    assert_eq!(stage_guarded(&reopened_store, &reopened_gate, &MachineDpapi, &binding, &credential, body, &request).unwrap().as_slice(), request);
    assert!(stage_guarded_replacing(&reopened_store, &reopened_gate, &MachineDpapi, &binding, &credential, body, &request, Some(previous)).is_err());
    anchor::remove_test_key(&path).unwrap();
    anchor::remove_test_key(&gate_path).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_preserves_old_record_until_exact_new_request_is_staged() {
        let path = format!(r"SOFTWARE\WinCommanderReleaseRotateTest-{}", uuid::Uuid::new_v4());
        let gate_path = format!(r"SOFTWARE\WinCommanderReleaseRotateGateTest-{}", uuid::Uuid::new_v4());
        let store = anchor::Anchor::test_user(path.clone());
        let gate = anchor::Anchor::test_user(gate_path.clone());
        let binding = Binding { licence_id: "licence", device_hash: "device", issuer_origin: "https://license.example" };
        let credential = ConfirmedCredential { id: [5; 32], secret: Zeroizing::new([6; 32]) };
        let old = b"old completed protected record";
        store.write(old).unwrap();
        let body = b"new activation body";
        assert!(completed_previous_record_with(&store, &MachineDpapi, &binding, &credential, body, |_| Ok(false)).is_err());
        assert!(completed_previous_record_with(&store, &MachineDpapi, &binding, &credential, body, |_| Err("history unavailable")).is_err());
        assert_eq!(completed_previous_record_with(&store, &MachineDpapi, &binding, &credential, body,
            |bytes| Ok(bytes == old)).unwrap().unwrap(), old);
        let digest: [u8; 32] = Sha256::digest(body).into();
        let challenge = serde_json::to_vec(&serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(credential.id),
            "nonce": URL_SAFE_NO_PAD.encode([7; 32]), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": 1060 })).unwrap();
        let request = wire::release_request(&credential, body.to_vec(), &challenge, 1000).unwrap();
        assert!(stage_guarded_replacing(&store, &gate, &MachineDpapi, &binding, &credential, body, &request, Some(b"different")).is_err());
        assert_eq!(store.read().unwrap().unwrap(), old);
        let reopened = anchor::Anchor::test_user(path.clone());
        assert_eq!(stage_guarded_replacing(&reopened, &anchor::Anchor::test_user(gate_path.clone()), &MachineDpapi,
            &binding, &credential, body, &request, Some(old)).unwrap().as_slice(), request);
        assert_ne!(store.read().unwrap().unwrap(), old);
        assert!(completed_previous_record_with(&store, &MachineDpapi, &binding, &credential, body,
            |_| panic!("a current record must not request replacement authorization")).unwrap().is_none());
        assert_eq!(stage_guarded(&store, &gate, &MachineDpapi, &binding, &credential, body, &request).unwrap().as_slice(), request);
        assert!(stage_guarded_replacing(&store, &gate, &MachineDpapi, &binding, &credential, body, &request, Some(old)).is_err());
        anchor::remove_test_key(&path).unwrap();
        anchor::remove_test_key(&gate_path).unwrap();
    }

    #[test]
    fn release_retry_is_sealed_bound_and_never_replaced() {
        let path = format!(r"SOFTWARE\WinCommanderReleaseRetryTest-{}", uuid::Uuid::new_v4());
        let store = anchor::Anchor::test_user(path.clone());
        let gate_path = format!(r"SOFTWARE\WinCommanderReleaseRetryGateTest-{}", uuid::Uuid::new_v4());
        let gate = anchor::Anchor::test_user(gate_path.clone());
        let binding = Binding { licence_id: "licence", device_hash: "device", issuer_origin: "https://license.example" };
        let credential = ConfirmedCredential { id: [7; 32], secret: Zeroizing::new([9; 32]) };
        let body = b"verified licence request";
        let digest: [u8; 32] = Sha256::digest(body).into();
        let response = serde_json::to_vec(&serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(credential.id),
            "nonce": URL_SAFE_NO_PAD.encode([8; 32]), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": 1060 })).unwrap();
        let request = wire::release_request(&credential, body.to_vec(), &response, 1000).unwrap();
        let expected_ack = wire::release_acknowledgement_request(&credential, body, &request).unwrap();
        let ack_confirmation = wire::test_acknowledgement_confirmation(&expected_ack, br#"{"ok":true}"#).unwrap();
        let release_confirmation = wire::test_acknowledgement_confirmation(&request, br#"{"ok":true}"#).unwrap();
        assert!(wire::test_acknowledgement_confirmation(&expected_ack, br#"{"ok":false}"#).is_err());
        assert!(load_locked(&store, &MachineDpapi, &binding, &credential, body).unwrap().is_none());
        let mut forged: serde_json::Value = serde_json::from_slice(&request).unwrap();
        forged["proof"] = serde_json::json!(URL_SAFE_NO_PAD.encode([0; 32]));
        assert!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &serde_json::to_vec(&forged).unwrap()).is_err());
        assert!(stage_guarded(&store, &gate, &MachineDpapi, &binding, &credential, body, &serde_json::to_vec(&forged).unwrap()).is_err());
        assert!(gate.read().unwrap().is_none());
        assert!(store.read().unwrap().is_none());
        // A crash after saving the request but before binding its digest must
        // recover only the exact protected request.
        let context = release_entropy(&binding, &credential, body).unwrap();
        crate::license_cache::release_gate::begin_at(&gate, &context).unwrap();
        save_locked(&store, &MachineDpapi, &binding, &credential, body, &request).unwrap();
        crate::license_cache::release_gate::check_request_at(&gate, &context, None).unwrap();
        assert_eq!(load_guarded(&store, &gate, &MachineDpapi, &binding, &credential, body)
            .unwrap().unwrap().request.as_slice(), request);
        assert_eq!(stage_guarded(&store, &gate, &MachineDpapi, &binding, &credential, body, &request).unwrap().as_slice(), request);
        assert!(crate::license_cache::release_gate::ensure_idle_at(&gate).is_err());
        assert_eq!(load_guarded(&anchor::Anchor::test_user(path.clone()), &anchor::Anchor::test_user(gate_path.clone()),
            &MachineDpapi, &binding, &credential, body).unwrap().unwrap().request.as_slice(), request);
        let sealed = store.read().unwrap().unwrap();
        assert!(!sealed.windows(request.len()).any(|part| part == request));
        let reopened = load_locked(&anchor::Anchor::test_user(path.clone()), &MachineDpapi, &binding, &credential, body).unwrap().unwrap();
        assert!(!reopened.confirmed);
        assert!(!reopened.acknowledged);
        assert_eq!(reopened.request.as_slice(), request);
        assert!(prepare_ack_locked(&store, &MachineDpapi, &binding, &credential, body).is_err());
        assert!(acknowledge_locked(&store, &MachineDpapi, &binding, &credential, body, &request, &ack_confirmation).is_err());
        assert_eq!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &request).unwrap().as_slice(), request);
        assert!(load_locked(&store, &MachineDpapi, &Binding { device_hash: "other", ..binding }, &credential, body).is_err());
        assert!(load_locked(&store, &MachineDpapi, &binding, &credential, b"different body").is_err());
        let other = ConfirmedCredential { id: [7; 32], secret: Zeroizing::new([10; 32]) };
        assert!(load_locked(&store, &MachineDpapi, &binding, &other, body).is_err());
        let changed = serde_json::to_vec(&serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(credential.id),
            "nonce": URL_SAFE_NO_PAD.encode([11; 32]), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": 1060 })).unwrap();
        let alternate = wire::release_request(&credential, body.to_vec(), &changed, 1000).unwrap();
        assert!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &alternate).is_err());
        assert_eq!(store.read().unwrap().unwrap(), sealed);
        assert!(confirm_locked(&store, &MachineDpapi, &binding, &credential, body, &request, br#"{"ok":false}"#).is_err());
        assert_eq!(store.read().unwrap().unwrap(), sealed);
        assert!(confirm_locked(&store, &MachineDpapi, &binding, &credential, body, &alternate, br#"{"ok":true}"#).is_err());
        confirm_locked(&store, &MachineDpapi, &binding, &credential, body, &request, br#"{"ok":true}"#).unwrap();
        let confirmed = load_locked(&anchor::Anchor::test_user(path.clone()), &MachineDpapi, &binding, &credential, body).unwrap().unwrap();
        assert!(confirmed.confirmed);
        assert!(!confirmed.acknowledged);
        assert_eq!(confirmed.request.as_slice(), request);
        let ack = prepare_ack_locked(&store, &MachineDpapi, &binding, &credential, body).unwrap();
        let ack_json: serde_json::Value = serde_json::from_slice(&ack).unwrap();
        let original: serde_json::Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(ack_json["nonce"], original["nonce"]);
        assert_ne!(ack_json["proof"], original["proof"]);
        assert!(acknowledge_locked(&store, &MachineDpapi, &binding, &credential, body, &alternate, &ack_confirmation).is_err());
        assert!(acknowledge_locked(&store, &MachineDpapi, &binding, &credential, body, &request, &release_confirmation).is_err());
        assert!(!load_locked(&store, &MachineDpapi, &binding, &credential, body).unwrap().unwrap().acknowledged);
        crate::license_cache::release_gate::prepare_finalization_at(&gate, &context, &request,
            binding.licence_id, binding.device_hash, &credential.id, 2, b"signed cache").unwrap();
        let before_ack = crate::license_cache::release_gate::prepared_at(&gate).unwrap().unwrap();
        assert!(verify_prepared_ack(&store, &gate, &MachineDpapi, &binding, &credential, body, &before_ack).is_err());
        acknowledge_locked(&store, &MachineDpapi, &binding, &credential, body, &request, &ack_confirmation).unwrap();
        crate::license_cache::release_gate::prepare_finalization_at(&gate, &context, &request,
            binding.licence_id, binding.device_hash, &credential.id, 2, b"signed cache").unwrap();
        let prepared = crate::license_cache::release_gate::prepared_at(&anchor::Anchor::test_user(gate_path.clone())).unwrap().unwrap();
        assert!(verify_prepared_ack(&store, &gate, &MachineDpapi, &binding, &credential, body, &prepared).is_ok());
        assert!(verify_prepared_ack(&store, &gate, &MachineDpapi, &binding, &credential, b"changed", &prepared).is_err());
        assert!(verify_prepared_ack(&store, &gate, &MachineDpapi, &binding, &other, body, &prepared).is_err());
        let wrong_binding = Binding { licence_id: "other", ..binding };
        assert!(verify_prepared_ack(&store, &gate, &MachineDpapi, &wrong_binding, &credential, body, &prepared).is_err());
        let acknowledged = load_locked(&anchor::Anchor::test_user(path.clone()), &MachineDpapi, &binding, &credential, body).unwrap().unwrap();
        assert!(acknowledged.confirmed && acknowledged.acknowledged);
        assert_eq!(acknowledged.request.as_slice(), request);
        acknowledge_locked(&store, &MachineDpapi, &binding, &credential, body, &request, &ack_confirmation).unwrap();
        confirm_locked(&store, &MachineDpapi, &binding, &credential, body, &request, br#"{"ok":true}"#).unwrap();
        assert!(load_locked(&store, &MachineDpapi, &binding, &credential, body).unwrap().unwrap().acknowledged);
        assert!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &alternate).is_err());
        assert!(stage_guarded(&store, &gate, &MachineDpapi, &binding, &credential, body, &alternate).is_err());
        gate.write(b"invalid").unwrap();
        assert!(load_guarded(&store, &gate, &MachineDpapi, &binding, &credential, body).is_err());
        store.write(b"invalid").unwrap();
        assert!(load_locked(&store, &MachineDpapi, &binding, &credential, body).is_err());
        assert!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &request).is_err());
        anchor::remove_test_key(&path).unwrap();
        anchor::remove_test_key(&gate_path).unwrap();
    }
}
