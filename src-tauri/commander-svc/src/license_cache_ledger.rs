// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded signed licence history; no entry is silently evicted.
use super::*;

const MAX_RECORDS: usize = 4;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u8,
    records: Vec<String>,
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

pub(super) fn advance(
    retained: Option<&[u8]>,
    existing: Option<&[u8]>,
    candidate: &StoreLicenseCacheRequest,
    bytes: &[u8],
    public_key: &str,
    trust: &wincmd_pq_experiment::jws::Trust<'_>,
    old_trust: Option<&history::HistoricalTrust>,
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
        },
        Some(raw) => match serde_json::from_slice::<Ledger>(raw) {
            Ok(ledger) => ledger,
            // The first experimental version stored one complete cache directly.
            Err(_) => {
                let legacy: LegacyCache = serde_json::from_slice(raw)
                    .map_err(|_| "license security record is invalid")?;
                if legacy.token.hybrid.is_none() {
                    return Err("license security record is invalid");
                }
                Ledger {
                    version: 1,
                    records: vec![std::str::from_utf8(raw)
                        .map_err(|_| "license security record is invalid")?
                        .to_owned()],
                }
            }
        },
    };
    if ledger.version != 1
        || ledger.records.len() > MAX_RECORDS
        || (retained.is_some() && ledger.records.is_empty())
    {
        return Err("license security record is invalid");
    }
    let new: SignedClaims =
        serde_json::from_str(&candidate.payload).map_err(|_| "license cache is invalid")?;
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
