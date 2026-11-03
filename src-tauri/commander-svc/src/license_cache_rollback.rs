// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental replay check for the currently retained hybrid licence.
use super::*;
use std::{io::Read, sync::Mutex};

#[path = "license_cache_history.rs"]
mod history;
#[path = "license_cache_ledger.rs"]
mod ledger;
#[path = "../../commander-free/src/updater/hybrid_free_anchor.rs"]
mod anchor;

const CACHE_LIMIT: u64 = 64 * 1024;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
struct ExistingCache {
    token: ExistingToken,
    #[serde(default)]
    last_verified_at: Option<u64>,
}

#[derive(Deserialize)]
struct ExistingToken {
    payload: String,
    signature: String,
    #[serde(default)]
    hybrid: Option<String>,
}

pub(super) fn write(
    path: &Path,
    candidate: &StoreLicenseCacheRequest,
    bytes: &[u8],
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
) -> Result<(), &'static str> {
    let anchor = anchor::Anchor::machine_named(r"SOFTWARE\ServaLabs\WinCommander\HybridLicence");
    write_with_anchor(path, candidate, bytes, public_key, trust, &anchor)
}

fn write_with_anchor(
    path: &Path,
    candidate: &StoreLicenseCacheRequest,
    bytes: &[u8],
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    anchor: &anchor::Anchor,
) -> Result<(), &'static str> {
    let old_trust = history::configured(trust)?;
    let _lock = WRITE_LOCK
        .lock()
        .map_err(|_| "license cache lock is unavailable")?;
    let existing = read_cache(path)?;
    if let Some(existing) = existing.as_deref() {
        check_previous_with_history(existing, candidate, public_key, trust, old_trust.as_ref())?;
    }
    let retained = anchor.read().map_err(|_| "license security record is unavailable")?;
    let retained = ledger::advance(retained.as_deref(), existing.as_deref(), candidate, bytes, public_key, trust, old_trust.as_ref())?;
    // Record the authenticated candidate first. If the cache write is interrupted,
    // a retry of the same candidate is allowed while an older one remains blocked.
    anchor.write(&retained).map_err(|_| "license security record is unavailable")?;
    atomic_write(path, bytes).map_err(|_| "machine license cache is unavailable")
}

fn read_cache(path: &Path) -> Result<Option<Vec<u8>>, &'static str> {
    Ok(match fs::File::open(path) {
        Ok(file) => {
            let mut data = Vec::new();
            file.take(CACHE_LIMIT + 1)
                .read_to_end(&mut data)
                .map_err(|_| "license cache is unavailable")?;
            if data.len() as u64 > CACHE_LIMIT {
                return Err("license cache is invalid");
            }
            Some(data)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("license cache is unavailable"),
    })
}

pub(super) fn verify_current(
    path: &Path,
    candidate: &StoreLicenseCacheRequest,
    expected_time: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
) -> Result<(), &'static str> {
    let anchor = anchor::Anchor::machine_named(r"SOFTWARE\ServaLabs\WinCommander\HybridLicence");
    verify_with_anchor(path, candidate, expected_time, public_key, trust, &anchor)
}

fn verify_with_anchor(
    path: &Path,
    candidate: &StoreLicenseCacheRequest,
    expected_time: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    anchor: &anchor::Anchor,
) -> Result<(), &'static str> {
    let old_trust = history::configured(trust)?;
    let _lock = WRITE_LOCK.lock().map_err(|_| "license cache lock is unavailable")?;
    let bytes = read_cache(path)?.ok_or("license cache is unavailable")?;
    let retained = anchor.read().map_err(|_| "license security record is unavailable")?
        .ok_or("license security record is unavailable")?;
    ledger::verify_current(&retained, candidate, expected_time, &bytes, public_key, trust, old_trust.as_ref())
}

#[cfg(test)]
fn check_previous(
    bytes: &[u8],
    candidate: &StoreLicenseCacheRequest,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
) -> Result<(), &'static str> {
    let old_trust = history::configured(trust)?;
    check_previous_with_history(bytes, candidate, public_key, trust, old_trust.as_ref())
}

fn check_previous_with_history(
    bytes: &[u8],
    candidate: &StoreLicenseCacheRequest,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    old_trust: Option<&history::HistoricalTrust>,
) -> Result<(), &'static str> {
    let existing: ExistingCache =
        serde_json::from_slice(bytes).map_err(|_| "license cache is invalid")?;
    let previous = StoreLicenseCacheRequest {
        payload: existing.token.payload,
        signature: existing.token.signature,
        hybrid: existing.token.hybrid,
        seats_used: None,
        seat_limit: None,
    };
    validate_request(&previous, public_key)?;
    // A classical cache has no hybrid floor; permit the first migration.
    if previous.hybrid.is_none() {
        return Ok(());
    }
    let previous_epoch = history::verify_previous(&previous, trust, old_trust)?;
    let candidate_proof = candidate
        .hybrid
        .as_deref()
        .ok_or("hybrid license is required")?;
    let candidate_verified = validate_hybrid_request(candidate_proof.as_bytes(), trust)?;
    if candidate_verified.payload != candidate.payload.as_bytes() {
        return Err("hybrid license does not match the classical license");
    }
    if candidate_verified.epoch < previous_epoch {
        return Err("license cache would restore an older signing epoch");
    }
    let old: SignedClaims =
        serde_json::from_str(&previous.payload).map_err(|_| "license cache is invalid")?;
    let new: SignedClaims =
        serde_json::from_str(&candidate.payload).map_err(|_| "license cache is invalid")?;
    let same_licence = same_licence(&old, &new);
    if same_licence && new.iat < old.iat {
        return Err("license cache would restore an older signed licence");
    }
    // Issuance timestamps have one-second resolution. Different payloads at
    // the same timestamp have no trustworthy ordering; retain the existing
    // proof and allow the issuer to retry in a later second. Exact retries,
    // including signatures regenerated over identical claims, remain valid.
    if same_licence && new.iat == old.iat && candidate.payload != previous.payload {
        return Err("license cache has conflicting licences issued in the same second; retry");
    }
    Ok(())
}

fn same_licence(old: &SignedClaims, new: &SignedClaims) -> bool {
    match (&old.license_id, &new.license_id) {
        (Some(a), Some(b)) => !a.is_empty() && a == b,
        (None, None) => old.device_hash == new.device_hash,
        _ => false,
    }
}

#[cfg(test)]
#[path = "license_cache_rollback_tests.rs"]
mod tests;
