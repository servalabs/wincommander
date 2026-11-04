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
) -> Result<Option<Zeroizing<Vec<u8>>>, &'static str> {
    let Some(bytes) = store.read().map_err(|_| INVALID)? else { return Ok(None); };
    if bytes.len() > LIMIT { return Err(INVALID); }
    let record: Record = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    if record.version != 1 || record.sealed.is_empty() || record.sealed.len() > LIMIT { return Err(INVALID); }
    let sealed = URL_SAFE_NO_PAD.decode(&record.sealed).map_err(|_| INVALID)?;
    if URL_SAFE_NO_PAD.encode(&sealed) != record.sealed { return Err(INVALID); }
    let request = protector.unprotect(&sealed, &release_entropy(binding, credential, body)?).map_err(|_| INVALID)?;
    wire::verify_stored_release_request(credential, body, &request)?;
    Ok(Some(request))
}

fn save_locked(
    store: &anchor::Anchor, protector: &dyn Protector, binding: &Binding<'_>, credential: &ConfirmedCredential,
    body: &[u8], request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    wire::verify_stored_release_request(credential, body, request)?;
    if let Some(existing) = load_locked(store, protector, binding, credential, body)? {
        return if existing.as_slice() == request { Ok(existing) } else { Err(INVALID) };
    }
    let sealed = protector.protect(request, &release_entropy(binding, credential, body)?).map_err(|_| INVALID)?;
    if sealed.is_empty() || sealed.len() > LIMIT / 2 { return Err(INVALID); }
    let bytes = serde_json::to_vec(&Record { version: 1, sealed: URL_SAFE_NO_PAD.encode(sealed) }).map_err(|_| INVALID)?;
    if bytes.len() > LIMIT { return Err(INVALID); }
    store.write(&bytes).map_err(|_| INVALID)?;
    let reopened = load_locked(store, protector, binding, credential, body)?.ok_or(INVALID)?;
    if reopened.as_slice() != request { return Err(INVALID); }
    Ok(reopened)
}

fn with_current<T>(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
    op: impl FnOnce(&anchor::Anchor, &ConfirmedCredential, &[u8]) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    if binding.issuer_origin != wire::LICENCE_ORIGIN { return Err(INVALID); }
    // Same lock order as credential confirmation: credential, cache, process.
    let _guard = LOCK.lock().map_err(|_| INVALID)?;
    crate::license_cache::with_verified_refresh_wire(licence.payload, licence.signature, licence.hybrid,
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
        save_locked(store, &MachineDpapi, binding, credential, body, request))
}

/// A caller may retry these exact bytes; the local licence is never cleared here.
pub(crate) fn load(
    binding: &Binding<'_>, trust: &wincmd_pq_experiment::jws::Trust<'_>, licence: &RefreshLicence<'_>, now: u64,
) -> Result<Option<Zeroizing<Vec<u8>>>, &'static str> {
    with_current(binding, trust, licence, now, |store, credential, body|
        load_locked(store, &MachineDpapi, binding, credential, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_retry_is_sealed_bound_and_never_replaced() {
        let path = format!(r"SOFTWARE\WinCommanderReleaseRetryTest-{}", uuid::Uuid::new_v4());
        let store = anchor::Anchor::test_user(path.clone());
        let binding = Binding { licence_id: "licence", device_hash: "device", issuer_origin: "https://license.example" };
        let credential = ConfirmedCredential { id: [7; 32], secret: Zeroizing::new([9; 32]) };
        let body = b"verified licence request";
        let digest: [u8; 32] = Sha256::digest(body).into();
        let response = serde_json::to_vec(&serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(credential.id),
            "nonce": URL_SAFE_NO_PAD.encode([8; 32]), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": 1060 })).unwrap();
        let request = wire::release_request(&credential, body.to_vec(), &response, 1000).unwrap();
        assert!(load_locked(&store, &MachineDpapi, &binding, &credential, body).unwrap().is_none());
        let mut forged: serde_json::Value = serde_json::from_slice(&request).unwrap();
        forged["proof"] = serde_json::json!(URL_SAFE_NO_PAD.encode([0; 32]));
        assert!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &serde_json::to_vec(&forged).unwrap()).is_err());
        assert!(store.read().unwrap().is_none());
        assert_eq!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &request).unwrap().as_slice(), request);
        let sealed = store.read().unwrap().unwrap();
        assert!(!sealed.windows(request.len()).any(|part| part == request));
        assert_eq!(load_locked(&anchor::Anchor::test_user(path.clone()), &MachineDpapi, &binding, &credential, body).unwrap().unwrap().as_slice(), request);
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
        store.write(b"invalid").unwrap();
        assert!(load_locked(&store, &MachineDpapi, &binding, &credential, body).is_err());
        assert!(save_locked(&store, &MachineDpapi, &binding, &credential, body, &request).is_err());
        anchor::remove_test_key(&path).unwrap();
    }
}
