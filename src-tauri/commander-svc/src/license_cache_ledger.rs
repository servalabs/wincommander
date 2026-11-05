// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded signed licence history; no entry is silently evicted.
use super::*;

const MAX_RECORDS: usize = 4;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u8,
    records: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    released: Vec<ReleasedGeneration>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleasedGeneration {
    license_id: String,
    device_hash: String,
    generation: u64,
}

fn validate_ledger(ledger: &Ledger) -> Result<(), &'static str> {
    if !matches!(ledger.version, 1 | 2) || (ledger.version == 1 && !ledger.released.is_empty())
        || (ledger.version == 2 && ledger.released.is_empty())
        || ledger.records.len() > MAX_RECORDS || ledger.released.len() > MAX_RECORDS {
        return Err("license security record is invalid");
    }
    for (i, entry) in ledger.released.iter().enumerate() {
        if entry.license_id.is_empty() || entry.license_id.len() > 512 || entry.device_hash.is_empty()
            || entry.device_hash.len() > 512 || entry.generation == 0 || entry.generation > 9_007_199_254_740_991
            || ledger.released[..i].iter().any(|previous| previous.license_id == entry.license_id && previous.device_hash == entry.device_hash) {
            return Err("license security record is invalid");
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyCache {
    token: ExistingToken,
    #[serde(default, rename = "last_verified_at")]
    _last_verified_at: Option<u64>,
    #[serde(default, rename = "seats_used")]
    _seats_used: Option<u32>,
    #[serde(default, rename = "seat_limit")]
    _seat_limit: Option<u32>,
}

fn parse_ledger(raw: &[u8]) -> Result<Ledger, &'static str> {
    let ledger = match serde_json::from_slice::<Ledger>(raw) {
        Ok(ledger) => ledger,
        Err(_) => {
            let legacy: LegacyCache = serde_json::from_slice(raw).map_err(|_| "license security record is invalid")?;
            if legacy.token.hybrid.is_none() { return Err("license security record is invalid"); }
            Ledger { version: 1, released: Vec::new(), records: vec![std::str::from_utf8(raw)
                .map_err(|_| "license security record is invalid")?.to_owned()] }
        }
    };
    validate_ledger(&ledger)?;
    if ledger.records.is_empty() { return Err("license security record is invalid"); }
    Ok(ledger)
}

/// Locate the same device's protected candidate; callers must verify it before use.
pub(super) fn pending_renewal(
    retained: &[u8],
    expected: &StoreLicenseCacheRequest,
) -> Result<Option<(StoreLicenseCacheRequest, u64)>, &'static str> {
    let ledger = parse_ledger(retained)?;
    let old: RefreshClaims =
        serde_json::from_str(&expected.payload).map_err(|_| "license cache is invalid")?;
    let mut found = None;
    for record in ledger.records {
        let cache: ExistingCache =
            serde_json::from_str(&record).map_err(|_| "license security record is invalid")?;
        let summary: SignedClaims = serde_json::from_str(&cache.token.payload)
            .map_err(|_| "license security record is invalid")?;
        if summary.license_id.as_deref() != Some(old.license_id.as_str()) {
            continue;
        }
        let claims: RefreshClaims = serde_json::from_str(&cache.token.payload)
            .map_err(|_| "license security record is invalid")?;
        if claims.device_hash != old.device_hash
            || claims.is_portable != old.is_portable
            || found.is_some()
        {
            return Err("license security record is invalid");
        }
        let time = cache
            .last_verified_at
            .ok_or("license security record is invalid")?;
        found = Some((
            StoreLicenseCacheRequest {
                payload: cache.token.payload,
                signature: cache.token.signature,
                hybrid: cache.token.hybrid,
                seats_used: cache.seats_used,
                seat_limit: cache.seat_limit,
            },
            time,
        ));
    }
    Ok(found.filter(|(candidate, _)| {
        candidate.payload != expected.payload
            || candidate.signature != expected.signature
            || candidate.hybrid != expected.hybrid
    }))
}

/// Confirm that the exact signed token is already in the protected history.
/// This does not create or update the history and does not authorize a device.
pub(super) fn verify_current(
    retained: &[u8],
    candidate: &StoreLicenseCacheRequest,
    expected_time: u64,
    bytes: &[u8],
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    old_trust: Option<&history::HistoricalTrust>,
) -> Result<(), &'static str> {
    verify_current_inner(retained, candidate, expected_time, bytes, public_key, trust, old_trust, false)
}

/// Only release recovery may inspect the exact token whose floor is retired.
pub(super) fn verify_current_for_release(
    retained: &[u8], candidate: &StoreLicenseCacheRequest, expected_time: u64, bytes: &[u8], public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>, old_trust: Option<&history::HistoricalTrust>,
) -> Result<(), &'static str> {
    verify_current_inner(retained, candidate, expected_time, bytes, public_key, trust, old_trust, true)
}

fn verify_current_inner(
    retained: &[u8], candidate: &StoreLicenseCacheRequest, expected_time: u64, bytes: &[u8], public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>, old_trust: Option<&history::HistoricalTrust>, allow_released: bool,
) -> Result<(), &'static str> {
    // This validates every retained proof, the candidate and rollback ordering.
    advance_inner(
        Some(retained),
        None,
        candidate,
        bytes,
        public_key,
        trust,
        old_trust,
        allow_released,
    )?;
    let current: ExistingCache =
        serde_json::from_slice(bytes).map_err(|_| "license security record is invalid")?;
    let verified_at = current
        .last_verified_at
        .ok_or("license cache verification time is missing")?;
    if verified_at != expected_time {
        return Err("license cache changed during verification");
    }
    let records = parse_ledger(retained)?.records;
    for record in records {
        let old: ExistingCache =
            serde_json::from_str(&record).map_err(|_| "license security record is invalid")?;
        if old.token.payload == candidate.payload
            && old.token.signature == candidate.signature
            && old.token.hybrid == candidate.hybrid
            && old.last_verified_at == Some(verified_at)
        {
            return Ok(());
        }
    }
    Err("license cache does not match security history")
}

pub(super) fn advance(
    retained: Option<&[u8]>,
    existing: Option<&[u8]>,
    candidate: &StoreLicenseCacheRequest,
    bytes: &[u8],
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    old_trust: Option<&history::HistoricalTrust>,
) -> Result<Vec<u8>, &'static str> {
    advance_inner(retained, existing, candidate, bytes, public_key, trust, old_trust, false)
}

fn advance_inner(
    retained: Option<&[u8]>, existing: Option<&[u8]>, candidate: &StoreLicenseCacheRequest, bytes: &[u8], public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>, old_trust: Option<&history::HistoricalTrust>, allow_released: bool,
) -> Result<Vec<u8>, &'static str> {
    validate_dual_request(candidate, public_key, trust)?;
    let current: ExistingCache =
        serde_json::from_slice(bytes).map_err(|_| "license security record is invalid")?;
    if current.token.payload != candidate.payload
        || current.token.signature != candidate.signature
        || current.token.hybrid != candidate.hybrid
    {
        return Err("license security record does not match request");
    }
    let record = std::str::from_utf8(bytes).map_err(|_| "license security record is invalid")?;
    let mut ledger = match retained {
        None => Ledger {
            version: 1,
            records: Vec::new(),
            released: Vec::new(),
        },
        Some(raw) => parse_ledger(raw)?,
    };
    validate_ledger(&ledger)?;
    if retained.is_some() && ledger.records.is_empty() {
        return Err("license security record is invalid");
    }
    let new: SignedClaims =
        serde_json::from_str(&candidate.payload).map_err(|_| "license cache is invalid")?;
    if let Some(released) = ledger.released.iter().find(|entry| new.license_id.as_deref() == Some(entry.license_id.as_str())
        && new.device_hash == entry.device_hash) {
        let generation = checked_activation_generation(new.activation_generation)?
            .ok_or("license cache would restore a released activation")?;
        if generation < released.generation || (generation == released.generation && !allow_released) {
            return Err("license cache would restore a released activation");
        }
    }
    let cached = if let Some(bytes) = existing {
        check_previous_with_history(bytes, candidate, public_key, trust, old_trust)?;
        let cache: ExistingCache =
            serde_json::from_slice(bytes).map_err(|_| "license cache is invalid")?;
        if cache.token.hybrid.is_some() {
            let claims: SignedClaims = serde_json::from_str(&cache.token.payload)
                .map_err(|_| "license cache is invalid")?;
            Some((bytes, cache.token.payload, claims))
        } else {
            None
        }
    } else {
        None
    };
    let mut replaced = None;
    let mut cached_match = false;
    for (index, old) in ledger.records.iter().enumerate() {
        let old_cache: ExistingCache =
            serde_json::from_str(old).map_err(|_| "license security record is invalid")?;
        if old_cache.token.hybrid.is_none() {
            return Err("license security record is invalid");
        }
        check_previous_with_history(old.as_bytes(), candidate, public_key, trust, old_trust)?;
        let old_claims: SignedClaims = serde_json::from_str(&old_cache.token.payload)
            .map_err(|_| "license security record is invalid")?;
        if let Some((_, payload, claims)) = &cached {
            if same_licence(&old_claims, claims) {
                if cached_match {
                    return Err("license security record is invalid");
                }
                cached_match = true;
                // Do not discard a newer or conflicting cache when switching
                // licences. Refresh that same licence to reconcile both copies.
                if !same_licence(claims, &new) && old_cache.token.payload != *payload {
                    return Err("license security history needs reconciliation");
                }
            }
        }
        if same_licence(&old_claims, &new) && replaced.replace(index).is_some() {
            return Err("license security record is invalid");
        }
    }
    if let Some((bytes, _, claims)) = cached {
        if !cached_match && !same_licence(&claims, &new) {
            if ledger.records.len() >= MAX_RECORDS {
                return Err("license security history is full; recovery is required");
            }
            ledger.records.push(
                std::str::from_utf8(bytes)
                    .map_err(|_| "license security record is invalid")?
                    .to_owned(),
            );
        }
    }
    match replaced {
        Some(index) => ledger.records[index] = record.to_owned(),
        None if ledger.records.len() < MAX_RECORDS => ledger.records.push(record.to_owned()),
        None => return Err("license security history is full; recovery is required"),
    }
    let output = serde_json::to_vec(&ledger).map_err(|_| "license security record is invalid")?;
    if output.len() > CACHE_LIMIT as usize {
        return Err("license security history is full; recovery is required");
    }
    Ok(output)
}

/// Record the exact authenticated current generation as released. The caller
/// must separately prove a durable acknowledgement and hold the licence locks.
/// This never clears the release gate or removes the local cache.
pub(super) fn mark_released(
    retained: &[u8], candidate: &StoreLicenseCacheRequest, expected_time: u64, bytes: &[u8], public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>, old_trust: Option<&history::HistoricalTrust>,
) -> Result<Vec<u8>, &'static str> {
    verify_current_inner(retained, candidate, expected_time, bytes, public_key, trust, old_trust, true)?;
    let mut ledger = parse_ledger(retained)?;
    let claims: SignedClaims = serde_json::from_str(&candidate.payload).map_err(|_| "license cache is invalid")?;
    let id = claims.license_id.filter(|value| !value.is_empty() && value.len() <= 512)
        .ok_or("license security record is invalid")?;
    if claims.device_hash.is_empty() || claims.device_hash.len() > 512 { return Err("license security record is invalid"); }
    let generation = checked_activation_generation(claims.activation_generation)?
        .ok_or("license activation generation is invalid")?;
    if let Some(entry) = ledger.released.iter_mut().find(|entry| entry.license_id == id && entry.device_hash == claims.device_hash) {
        if generation < entry.generation { return Err("license cache would restore a released activation"); }
        entry.generation = generation;
    } else {
        if ledger.released.len() >= MAX_RECORDS { return Err("license security history is full; recovery is required"); }
        ledger.released.push(ReleasedGeneration { license_id: id, device_hash: claims.device_hash, generation });
    }
    ledger.version = 2;
    let output = serde_json::to_vec(&ledger).map_err(|_| "license security record is invalid")?;
    if output.len() > CACHE_LIMIT as usize { return Err("license security history is full; recovery is required"); }
    Ok(output)
}
