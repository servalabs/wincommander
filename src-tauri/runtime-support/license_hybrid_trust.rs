// SPDX-License-Identifier: AGPL-3.0-or-later
//! Build-time trust for the opt-in hybrid licence cache writer.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use wincmd_pq_experiment::{jws, Purpose, ML_DSA_65_PUBLIC_KEY_LEN};

pub(crate) struct Trust {
    ed: [u8; 32],
    pq: [u8; ML_DSA_65_PUBLIC_KEY_LEN],
    ed_id: &'static str,
    pq_id: &'static str,
    minimum_epoch: u64,
}
impl Trust {
    pub(crate) fn policy(&self) -> jws::Trust<'_> {
        jws::Trust {
            purpose: Purpose::Licence,
            minimum_epoch: self.minimum_epoch,
            ed_key_id: self.ed_id,
            ed25519_public: &self.ed,
            pq_key_id: self.pq_id,
            ml_dsa_65_public: &self.pq,
        }
    }
}
pub(super) fn configured() -> Result<Trust, &'static str> {
    let decode = |value: Option<&str>| -> Result<Vec<u8>, &'static str> {
        let value = value.ok_or("hybrid license trust is unavailable")?;
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| "hybrid license trust is invalid")?;
        if URL_SAFE_NO_PAD.encode(&bytes) != value {
            return Err("hybrid license trust is invalid");
        }
        Ok(bytes)
    };
    let ed = decode(option_env!("WINCMD_PQ_LIC_ED_KEY"))?
        .try_into()
        .map_err(|_| "hybrid license trust is invalid")?;
    let pq = decode(option_env!("WINCMD_PQ_LIC_MLDSA_KEY"))?
        .try_into()
        .map_err(|_| "hybrid license trust is invalid")?;
    let ed_id = option_env!("WINCMD_PQ_LIC_ED_ID")
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or("hybrid license trust is unavailable")?;
    let pq_id = option_env!("WINCMD_PQ_LIC_MLDSA_ID")
        .filter(|v| !v.is_empty() && v.len() <= 128 && *v != ed_id)
        .ok_or("hybrid license trust is unavailable")?;
    let minimum_epoch = option_env!("WINCMD_PQ_LIC_EPOCH")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .ok_or("hybrid license trust is unavailable")?;
    Ok(Trust {
        ed,
        pq,
        ed_id,
        pq_id,
        minimum_epoch,
    })
}
