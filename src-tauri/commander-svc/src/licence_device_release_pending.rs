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
    wire::verify_stored_release_request(credential, body, request)?;
    if let Some(existing) = load_locked(store, protector, binding, credential, body)? {
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

/// Save and read back before any network send. Existing state cannot be replaced.
pub(crate) fn stage(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64, request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body|
        stage_guarded(store, &crate::license_cache::release_gate::machine(), &MachineDpapi,
            binding, credential, body, request))
}

fn stage_guarded(store: &anchor::Anchor, gate: &anchor::Anchor, protector: &dyn Protector,
    binding: &Binding<'_>, credential: &ConfirmedCredential, body: &[u8], request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    wire::verify_stored_release_request(credential, body, request)?;
    let context = release_entropy(binding, credential, body)?;
    crate::license_cache::release_gate::begin_at(gate, &context)?;
    let saved = save_locked(store, protector, binding, credential, body, request)?;
    crate::license_cache::release_gate::bind_request_at(gate, &context, &saved)?;
    Ok(saved)
}

/// A caller may retry these exact bytes; the local licence is never cleared here.
pub(crate) fn load(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
) -> Result<Option<SavedRelease>, &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body| {
        let saved = load_guarded(store, &crate::license_cache::release_gate::machine(), &MachineDpapi,
            binding, credential, body)?;
        if saved.as_ref().is_some_and(|item| item.acknowledged) {
            crate::license_cache::mark_acknowledged_release_locked(licence.payload, licence.signature, licence.hybrid, licence.last_verified_at)?;
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
        crate::license_cache::mark_acknowledged_release_locked(licence.payload, licence.signature, licence.hybrid, licence.last_verified_at)
    })
}

fn prepare_ack_locked(store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>,
    credential: &ConfirmedCredential, body: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let saved = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if !saved.confirmed { return Err(INVALID); }
    wire::release_acknowledgement_request(credential, body, &saved.request)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        acknowledge_locked(&store, &MachineDpapi, &binding, &credential, body, &request, &ack_confirmation).unwrap();
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
