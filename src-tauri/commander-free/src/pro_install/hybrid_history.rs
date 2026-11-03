// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded historical trust for retained Pro rollback evidence only.
use super::hybrid_release::{verify_hybrid_pro, HybridProTrust, VerifiedHybridPro};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

#[cfg(test)]
#[path = "hybrid_history_tests.rs"]
mod tests;

pub(super) struct HistoricalTrust {
    trust: HybridProTrust,
    maximum_epoch: u64,
}

pub(super) fn configured(current: &HybridProTrust) -> Result<Option<HistoricalTrust>, String> {
    parse(
        current,
        [
            option_env!("WINCMD_PQ_PRO_OLD_ED_KEY"),
            option_env!("WINCMD_PQ_PRO_OLD_MLDSA_KEY"),
            option_env!("WINCMD_PQ_PRO_OLD_ED_ID"),
            option_env!("WINCMD_PQ_PRO_OLD_MLDSA_ID"),
            option_env!("WINCMD_PQ_PRO_OLD_MAX_EPOCH"),
        ],
    )
}

fn parse(
    current: &HybridProTrust,
    values: [Option<&str>; 5],
) -> Result<Option<HistoricalTrust>, String> {
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let [Some(ed), Some(pq), Some(ed_id), Some(pq_id), Some(maximum)] = values else {
        return Err("validation:Incomplete historical Pro trust configuration".into());
    };
    let decode = |value: &str| {
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| "validation:Invalid historical Pro key".to_string())
    };
    let maximum_epoch: u64 = maximum
        .parse()
        .map_err(|_| "validation:Invalid historical Pro epoch")?;
    if maximum_epoch == 0
        || maximum_epoch >= current.minimum_epoch
        || ed_id.is_empty()
        || pq_id.is_empty()
        || ed_id.len() > 128
        || pq_id.len() > 128
        || ed_id == pq_id
    {
        return Err("validation:Invalid historical Pro trust policy".into());
    }
    let trust = HybridProTrust {
        ed_key: decode(ed)?
            .try_into()
            .map_err(|_| "validation:Invalid historical Pro key length")?,
        pq_key: decode(pq)?
            .try_into()
            .map_err(|_| "validation:Invalid historical Pro key length")?,
        ed_id: ed_id.to_owned(),
        pq_id: pq_id.to_owned(),
        minimum_epoch: 1,
    };
    if trust.ed_key == current.ed_key && trust.pq_key == current.pq_key {
        return Err("validation:Historical Pro keys must differ from current keys".into());
    }
    Ok(Some(HistoricalTrust {
        trust,
        maximum_epoch,
    }))
}

pub(super) fn verify_previous(
    envelope: &[u8],
    current: &HybridProTrust,
    version: Option<&str>,
    hash: &str,
    historical: Option<&HistoricalTrust>,
) -> Result<VerifiedHybridPro, String> {
    let mut current_history = current.clone();
    current_history.minimum_epoch = 1;
    if let Ok(verified) = verify_hybrid_pro(envelope, &current_history, version, hash, None) {
        return Ok(verified);
    }
    let old = historical.ok_or("validation:Historical Pro trust is not configured")?;
    if old.maximum_epoch == 0 || old.maximum_epoch >= current.minimum_epoch {
        return Err("validation:Invalid historical Pro trust policy".into());
    }
    let verified = verify_hybrid_pro(envelope, &old.trust, version, hash, None)?;
    if verified.epoch > old.maximum_epoch {
        return Err("validation:Historical Pro signature exceeds its epoch limit".into());
    }
    Ok(verified)
}
