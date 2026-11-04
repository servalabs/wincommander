// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental replay check for the currently retained hybrid licence.
use super::*;
use std::{io::Read, sync::Mutex};

#[path = "../../commander-free/src/updater/hybrid_free_anchor.rs"]
pub(crate) mod anchor;
#[path = "license_cache_history.rs"]
pub(super) mod history;
#[path = "license_cache_ledger.rs"]
mod ledger;

const CACHE_LIMIT: u64 = 64 * 1024;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
struct ExistingCache {
    token: ExistingToken,
    #[serde(default)]
    last_verified_at: Option<u64>,
    #[serde(default)]
    seats_used: Option<u32>,
    #[serde(default)]
    seat_limit: Option<u32>,
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
    let _process_lock = crate::licence_security_lock()?;
    let existing = read_cache(path)?;
    if let Some(existing) = existing.as_deref() {
        check_previous_with_history(existing, candidate, public_key, trust, old_trust.as_ref())?;
    }
    let retained = anchor
        .read()
        .map_err(|_| "license security record is unavailable")?;
    let retained = ledger::advance(
        retained.as_deref(),
        existing.as_deref(),
        candidate,
        bytes,
        public_key,
        trust,
        old_trust.as_ref(),
    )?;
    // Record the authenticated candidate first. If the cache write is interrupted,
    // a retry of the same candidate is allowed while an older one remains blocked.
    anchor
        .write(&retained)
        .map_err(|_| "license security record is unavailable")?;
    atomic_write(path, bytes).map_err(|_| "machine license cache is unavailable")
}

/// Save only if the exact cache and protected history still match the request.
pub(super) fn write_renewal(
    path: &Path,
    expected: &StoreLicenseCacheRequest,
    expected_time: u64,
    candidate: &StoreLicenseCacheRequest,
    now: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
) -> Result<(), &'static str> {
    let anchor = anchor::Anchor::machine_named(r"SOFTWARE\ServaLabs\WinCommander\HybridLicence");
    write_renewal_with_anchor(
        path,
        (expected, expected_time),
        candidate,
        now,
        public_key,
        trust,
        &anchor,
    )
}

fn write_renewal_with_anchor(
    path: &Path,
    previous: (&StoreLicenseCacheRequest, u64),
    candidate: &StoreLicenseCacheRequest,
    now: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    anchor: &anchor::Anchor,
) -> Result<(), &'static str> {
    let (expected, expected_time) = previous;
    let old_trust = history::configured(trust)?;
    let _lock = WRITE_LOCK
        .lock()
        .map_err(|_| "license cache lock is unavailable")?;
    let _process_lock = crate::licence_security_lock()?;
    let existing = read_cache(path)?.ok_or("license cache is unavailable")?;
    let retained = anchor
        .read()
        .map_err(|_| "license security record is unavailable")?
        .ok_or("license security record is unavailable")?;
    let prior: ExistingCache =
        serde_json::from_slice(&existing).map_err(|_| "license cache is invalid")?;
    if prior.token.payload != expected.payload
        || prior.token.signature != expected.signature
        || prior.token.hybrid != expected.hybrid
        || prior.last_verified_at != Some(expected_time)
    {
        return Err("license cache changed during renewal");
    }
    validate_request(expected, public_key)?;
    let old_is_current = ledger::verify_current(
        &retained,
        expected,
        expected_time,
        &existing,
        public_key,
        trust,
        old_trust.as_ref(),
    )
    .is_ok();
    validate_dual_request(candidate, public_key, trust)?;
    let old: RefreshClaims =
        serde_json::from_str(&expected.payload).map_err(|_| "license cache is invalid")?;
    let new: RefreshClaims =
        serde_json::from_str(&candidate.payload).map_err(|_| "license renewal is invalid")?;
    if old.license_id != new.license_id
        || old.device_hash != new.device_hash
        || old.is_portable != new.is_portable
        || new.iss != "wincommander-license-worker"
        || new.sub != MAINLINE_APP_ID
    {
        return Err("license renewal does not match device");
    }
    check_previous_with_history(&existing, candidate, public_key, trust, old_trust.as_ref())?;
    let persisted = PersistedLicenseCache {
        token: PersistedTokenEnvelope {
            payload: &candidate.payload,
            signature: &candidate.signature,
            hybrid: candidate
                .hybrid
                .as_deref()
                .ok_or("hybrid license is required")?,
        },
        last_verified_at: signed_verification_time(&candidate.payload, now)?,
        seats_used: prior.seats_used,
        seat_limit: prior.seat_limit,
    };
    let bytes =
        serde_json::to_vec_pretty(&persisted).map_err(|_| "license cache could not be encoded")?;
    if old_is_current {
        let next = ledger::advance(
            Some(&retained),
            Some(&existing),
            candidate,
            &bytes,
            public_key,
            trust,
            old_trust.as_ref(),
        )?;
        anchor
            .write(&next)
            .map_err(|_| "license security record is unavailable")?;
    } else {
        // A prior attempt may have committed the exact candidate to the anchor
        // before its file write failed. Only that exact authenticated candidate
        // and timestamp may complete the interrupted update.
        ledger::verify_current(
            &retained,
            candidate,
            persisted.last_verified_at,
            &bytes,
            public_key,
            trust,
            old_trust.as_ref(),
        )?;
    }
    atomic_write(path, &bytes).map_err(|_| "machine license cache is unavailable")
}

/// Reconcile an interrupted write using only the protected candidate and cache.
pub(super) fn recover_pending_renewal(
    path: &Path,
    now: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
) -> Result<bool, &'static str> {
    let anchor = anchor::Anchor::machine_named(r"SOFTWARE\ServaLabs\WinCommander\HybridLicence");
    recover_pending_with_anchor(path, now, public_key, trust, &anchor)
}

fn recover_pending_with_anchor(
    path: &Path,
    now: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    anchor: &anchor::Anchor,
) -> Result<bool, &'static str> {
    // This preliminary read selects a candidate only. The writer rechecks both
    // records under its lock before performing any mutation.
    let bytes = read_cache(path)?.ok_or("license cache is unavailable")?;
    let existing: ExistingCache =
        serde_json::from_slice(&bytes).map_err(|_| "license cache is invalid")?;
    let time = existing
        .last_verified_at
        .ok_or("license cache verification time is missing")?;
    let expected = StoreLicenseCacheRequest {
        payload: existing.token.payload,
        signature: existing.token.signature,
        hybrid: existing.token.hybrid,
        seats_used: existing.seats_used,
        seat_limit: existing.seat_limit,
    };
    let retained = anchor
        .read()
        .map_err(|_| "license security record is unavailable")?
        .ok_or("license security record is unavailable")?;
    match ledger::pending_renewal(&retained, &expected)? {
        Some((candidate, accepted_at)) => {
            if accepted_at > now {
                return Err("license security record is from the future");
            }
            write_renewal_with_anchor(
                path,
                (&expected, time),
                &candidate,
                accepted_at,
                public_key,
                trust,
                anchor,
            )?;
            Ok(true)
        }
        None => {
            verify_with_anchor(path, &expected, time, public_key, trust, anchor)?;
            Ok(false)
        }
    }
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
    with_verified_current(path, candidate, expected_time, public_key, trust, || Ok(()))
}

/// Run a short, non-blocking operation while the verified cache cannot change.
pub(super) fn with_verified_current<T>(
    path: &Path,
    candidate: &StoreLicenseCacheRequest,
    expected_time: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    operation: impl FnOnce() -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let anchor = anchor::Anchor::machine_named(r"SOFTWARE\ServaLabs\WinCommander\HybridLicence");
    with_verified_anchor(path, candidate, expected_time, public_key, trust, &anchor, operation)
}

fn verify_with_anchor(
    path: &Path,
    candidate: &StoreLicenseCacheRequest,
    expected_time: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    anchor: &anchor::Anchor,
) -> Result<(), &'static str> {
    with_verified_anchor(path, candidate, expected_time, public_key, trust, anchor, || Ok(()))
}

fn with_verified_anchor<T>(
    path: &Path,
    candidate: &StoreLicenseCacheRequest,
    expected_time: u64,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    anchor: &anchor::Anchor,
    operation: impl FnOnce() -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let old_trust = history::configured(trust)?;
    let _lock = WRITE_LOCK
        .lock()
        .map_err(|_| "license cache lock is unavailable")?;
    let _process_lock = crate::licence_security_lock()?;
    let bytes = read_cache(path)?.ok_or("license cache is unavailable")?;
    let retained = anchor
        .read()
        .map_err(|_| "license security record is unavailable")?
        .ok_or("license security record is unavailable")?;
    ledger::verify_current(
        &retained,
        candidate,
        expected_time,
        &bytes,
        public_key,
        trust,
        old_trust.as_ref(),
    )?;
    operation()
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
