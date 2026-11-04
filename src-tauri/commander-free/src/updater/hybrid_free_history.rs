// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded retired-key trust for retained Free update records only.
use super::Trust;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

pub(super) struct HistoricalTrust {
    ed: [u8; 32],
    pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN],
    ed_id: String,
    pq_id: String,
    pub(super) maximum_epoch: u64,
}

impl HistoricalTrust {
    pub(super) fn policy(&self) -> jws::Trust<'_> {
        jws::Trust {
            purpose: Purpose::Release,
            minimum_epoch: 1,
            ed_key_id: &self.ed_id,
            ed25519_public: &self.ed,
            pq_key_id: &self.pq_id,
            ml_dsa_65_public: &self.pq,
        }
    }
}

pub(super) fn configured(current: &Trust) -> Result<Option<HistoricalTrust>, String> {
    parse(
        current,
        [
            option_env!("WINCMD_PQ_FREE_OLD_ED_KEY"),
            option_env!("WINCMD_PQ_FREE_OLD_MLDSA_KEY"),
            option_env!("WINCMD_PQ_FREE_OLD_ED_ID"),
            option_env!("WINCMD_PQ_FREE_OLD_MLDSA_ID"),
            option_env!("WINCMD_PQ_FREE_OLD_MAX_EPOCH"),
        ],
    )
}

pub(super) fn parse(
    current: &Trust,
    values: [Option<&str>; 5],
) -> Result<Option<HistoricalTrust>, String> {
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let [Some(ed), Some(pq), Some(ed_id), Some(pq_id), Some(maximum)] = values else {
        return Err("Incomplete historical Free update trust configuration".into());
    };
    let maximum_epoch: u64 = maximum
        .parse()
        .map_err(|_| "Invalid historical Free update epoch")?;
    if maximum_epoch == 0
        || maximum_epoch >= current.epoch
        || ed_id.is_empty()
        || pq_id.is_empty()
        || ed_id.len() > 128
        || pq_id.len() > 128
        || ed_id == pq_id
    {
        return Err("Invalid historical Free update trust policy".into());
    }
    let decode = |value: &str| {
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| "Invalid historical Free update key".to_string())
    };
    let old = HistoricalTrust {
        ed: decode(ed)?
            .try_into()
            .map_err(|_| "Invalid historical Free update key length")?,
        pq: decode(pq)?
            .try_into()
            .map_err(|_| "Invalid historical Free update key length")?,
        ed_id: ed_id.to_owned(),
        pq_id: pq_id.to_owned(),
        maximum_epoch,
    };
    if old.ed == current.ed && old.pq == current.pq {
        return Err("Historical Free update keys must differ from current keys".into());
    }
    Ok(Some(old))
}
