// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded historical trust for retained licence evidence, not current grants.
use super::*;
use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

pub(super) struct HistoricalTrust {
    ed: [u8; 32],
    pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN],
    ed_id: String,
    pq_id: String,
    maximum_epoch: u64,
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
        ],
    )
}

pub(super) fn parse(
    current: &jws::Trust<'_>,
    values: [Option<&str>; 5],
) -> Result<Option<HistoricalTrust>, &'static str> {
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let [Some(ed), Some(pq), Some(ed_id), Some(pq_id), Some(maximum)] = values else {
        return Err("historical license trust is incomplete");
    };
    let maximum_epoch: u64 = maximum
        .parse()
        .map_err(|_| "historical license epoch is invalid")?;
    if current.purpose != Purpose::Licence
        || maximum_epoch == 0
        || maximum_epoch >= current.minimum_epoch
        || ed_id.is_empty()
        || pq_id.is_empty()
        || ed_id.len() > 128
        || pq_id.len() > 128
        || ed_id == pq_id
    {
        return Err("historical license trust is invalid");
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
    let historical = HistoricalTrust {
        ed: decode(ed)?
            .try_into()
            .map_err(|_| "historical license key is invalid")?,
        pq: decode(pq)?
            .try_into()
            .map_err(|_| "historical license key is invalid")?,
        ed_id: ed_id.to_owned(),
        pq_id: pq_id.to_owned(),
        maximum_epoch,
    };
    Ok(Some(historical))
}

pub(super) fn verify_previous(
    previous: &StoreLicenseCacheRequest,
    current: &jws::Trust<'_>,
    historical: Option<&HistoricalTrust>,
) -> Result<u64, &'static str> {
    let envelope = previous
        .hybrid
        .as_deref()
        .ok_or("hybrid license is required")?;
    let current_history = jws::Trust {
        purpose: current.purpose,
        minimum_epoch: 1,
        ed_key_id: current.ed_key_id,
        ed25519_public: current.ed25519_public,
        pq_key_id: current.pq_key_id,
        ml_dsa_65_public: current.ml_dsa_65_public,
    };
    let verified = match validate_hybrid_request(envelope.as_bytes(), &current_history) {
        Ok(verified) => verified,
        Err(_) => {
            let old = historical.ok_or("historical license trust is unavailable")?;
            if old.maximum_epoch == 0 || old.maximum_epoch >= current.minimum_epoch {
                return Err("historical license trust is invalid");
            }
            let policy = jws::Trust {
                purpose: Purpose::Licence,
                minimum_epoch: 1,
                ed_key_id: &old.ed_id,
                ed25519_public: &old.ed,
                pq_key_id: &old.pq_id,
                ml_dsa_65_public: &old.pq,
            };
            let verified = validate_hybrid_request(envelope.as_bytes(), &policy)?;
            if verified.epoch > old.maximum_epoch {
                return Err("historical license epoch exceeds its limit");
            }
            verified
        }
    };
    if verified.payload != previous.payload.as_bytes() {
        return Err("hybrid license does not match the classical license");
    }
    Ok(verified.epoch)
}
