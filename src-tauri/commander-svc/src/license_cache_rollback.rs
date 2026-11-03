// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental replay check for the currently retained hybrid licence.
use super::*;
use std::{io::Read, sync::Mutex};

const CACHE_LIMIT: u64 = 64 * 1024;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
struct ExistingCache {
    token: ExistingToken,
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
    let _lock = WRITE_LOCK
        .lock()
        .map_err(|_| "license cache lock is unavailable")?;
    let existing = match fs::File::open(path) {
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
    };
    if let Some(existing) = existing {
        check_previous(&existing, candidate, public_key, trust)?;
    }
    atomic_write(path, bytes).map_err(|_| "machine license cache is unavailable")
}

fn check_previous(
    bytes: &[u8],
    candidate: &StoreLicenseCacheRequest,
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
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
    validate_dual_request(&previous, public_key, trust)?;
    let old: SignedClaims =
        serde_json::from_str(&previous.payload).map_err(|_| "license cache is invalid")?;
    let new: SignedClaims =
        serde_json::from_str(&candidate.payload).map_err(|_| "license cache is invalid")?;
    let same_licence = match (&old.license_id, &new.license_id) {
        (Some(a), Some(b)) => !a.is_empty() && a == b,
        (None, None) => old.device_hash == new.device_hash,
        _ => false,
    };
    if same_licence && new.iat < old.iat {
        return Err("license cache would restore an older signed licence");
    }
    Ok(())
}

#[cfg(test)]
#[path = "license_cache_rollback_tests.rs"]
mod tests;
