// SPDX-License-Identifier: AGPL-3.0-or-later
//! Retired hybrid proof can request renewal only during an explicit overlap.
use super::*;
use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

pub(super) struct HistoricalTrust {
    ed: [u8; 32],
    pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN],
    ed_id: String,
    pq_id: String,
    maximum_epoch: u64,
    refresh_until: u64,
}

pub(super) fn configured(
    current: &jws::Trust<'_>,
) -> Result<Option<HistoricalTrust>, &'static str> {
    parse(
        current,
        [
            option_env!("WINCMD_PQ_LIC_OLD_ED_KEY"),
            option_env!("WINCMD_PQ_LIC_OLD_MLDSA_KEY"),
            option_env!("WINCMD_PQ_LIC_OLD_ED_ID"),
            option_env!("WINCMD_PQ_LIC_OLD_MLDSA_ID"),
            option_env!("WINCMD_PQ_LIC_OLD_MAX_EPOCH"),
            option_env!("WINCMD_PQ_LIC_OLD_REFRESH_UNTIL"),
        ],
    )
}

fn parse(
    current: &jws::Trust<'_>,
    values: [Option<&str>; 6],
) -> Result<Option<HistoricalTrust>, &'static str> {
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let [Some(ed), Some(pq), Some(ed_id), Some(pq_id), Some(maximum), Some(deadline)] = values
    else {
        return Err("historical license refresh trust is incomplete");
    };
    let maximum_epoch: u64 = maximum
        .parse()
        .map_err(|_| "historical license epoch is invalid")?;
    let refresh_until: u64 = deadline
        .parse()
        .map_err(|_| "historical license refresh deadline is invalid")?;
    if current.purpose != Purpose::Licence
        || maximum_epoch == 0
        || maximum_epoch >= current.minimum_epoch
        || refresh_until == 0
        || ed_id.is_empty()
        || pq_id.is_empty()
        || ed_id.len() > 128
        || pq_id.len() > 128
        || ed_id == pq_id
    {
        return Err("historical license refresh trust is invalid");
    }
    let decode = |value: &str| -> Result<Vec<u8>, &'static str> {
        let bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| "historical license key is invalid")?;
        if general_purpose::URL_SAFE_NO_PAD.encode(&bytes) != value {
            return Err("historical license key is invalid");
        }
        Ok(bytes)
    };
    let old = HistoricalTrust {
        ed: decode(ed)?
            .try_into()
            .map_err(|_| "historical license key is invalid")?,
        pq: decode(pq)?
            .try_into()
            .map_err(|_| "historical license key is invalid")?,
        ed_id: ed_id.to_owned(),
        pq_id: pq_id.to_owned(),
        maximum_epoch,
        refresh_until,
    };
    Ok(Some(old))
}

pub(super) fn verify(
    token: &SignedTokenEnvelope,
    public_key: &str,
    current: &jws::Trust<'_>,
    old: &HistoricalTrust,
    now: u64,
) -> Result<LicenseClaims, String> {
    if now == 0 || now > old.refresh_until || old.maximum_epoch >= current.minimum_epoch {
        return Err("Historical licence renewal window has ended. Reactivate your licence.".into());
    }
    let claims = parse_and_verify_claims(&token.payload, &token.signature, public_key)?;
    if claims.plan == "trial" {
        return Err("Reactivate this trial with current licence security.".into());
    }
    let envelope = token
        .hybrid
        .as_deref()
        .ok_or("Hybrid licence proof is required")?;
    if envelope.len() > 32 * 1024 {
        return Err("Invalid hybrid licence proof".into());
    }
    let policy = jws::Trust {
        purpose: Purpose::Licence,
        minimum_epoch: 1,
        ed_key_id: &old.ed_id,
        ed25519_public: &old.ed,
        pq_key_id: &old.pq_id,
        ml_dsa_65_public: &old.pq,
    };
    let verified = jws::verify(envelope.as_bytes(), &policy)
        .map_err(|_| "Invalid historical licence proof")?;
    if verified.epoch > old.maximum_epoch || verified.payload != token.payload.as_bytes() {
        return Err("Invalid historical licence proof".into());
    }
    Ok(claims)
}

#[cfg(test)]
#[path = "license_refresh_history_tests.rs"]
mod tests;
