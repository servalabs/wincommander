// SPDX-License-Identifier: AGPL-3.0-or-later
//! Isolated settings-key rotation experiment; not a live migration protocol.
//! Existing shared installation material remains unchanged for logs and Pro.
use super::{
    decode_section, derive_section_key, dpapi_protect, dpapi_unprotect, encode_section,
    validate_section_name,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
use zeroize::Zeroizing;

const FORMAT: &str = "wincommander-settings-generation-v1";
const SCOPE: &str = "settings-generation-v1";
const MAX_SECTIONS: usize = 256;
const MAX_TOTAL: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    protected_material: String,
    ciphertext: String,
}

/// Snapshot input is supplied by a caller holding all source/key ownership.
/// It may include any general datastore section, not just the UI settings.
/// No original file is changed. A caller must establish directory and old-writer
/// fencing and use a separately authenticated cutover decision before use.
fn prepare(old_material: &[u8; 32], sections: &[(String, String)]) -> Result<Vec<u8>, String> {
    if sections.len() > MAX_SECTIONS {
        return Err("too many settings sections".into());
    }
    let mut total = 0usize;
    let old_key = Zeroizing::new(derive_section_key(old_material, None)?);
    let mut values = BTreeMap::<String, Value>::new();
    for (name, encoded) in sections {
        if name.is_empty() || name.len() > 128 {
            return Err("invalid settings section".into());
        }
        validate_section_name(name)?;
        total = total
            .checked_add(encoded.len())
            .ok_or("settings snapshot too large")?;
        if total > MAX_TOTAL {
            return Err("settings snapshot too large".into());
        }
        let bytes = Zeroizing::new(
            decode_section(&old_key, encoded.trim(), &format!("machine:{name}"))
                .map_err(|_| "settings section cannot be authenticated")?,
        );
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "settings section is invalid")?;
        if values.insert(name.clone(), value).is_some() {
            return Err("duplicate settings section".into());
        }
    }
    let plain =
        Zeroizing::new(serde_json::to_vec(&values).map_err(|_| "cannot encode settings snapshot")?);
    if plain.len() > MAX_TOTAL {
        return Err("settings snapshot too large".into());
    }
    let mut material = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(material.as_mut());
    let protected = Zeroizing::new(dpapi_protect(material.as_ref())?);
    let key = Zeroizing::new(derive_section_key(&material, None)?);
    let ciphertext = encode_section(&key, &plain, SCOPE)?;
    let bundle = Bundle {
        format: FORMAT.into(),
        protected_material: B64.encode(protected.as_slice()),
        ciphertext,
    };
    serde_json::to_vec(&bundle).map_err(|_| "cannot encode protected settings generation".into())
}

fn open(input: &[u8]) -> Result<BTreeMap<String, Value>, String> {
    if input.len() > MAX_TOTAL * 2 + 65536 {
        return Err("settings generation too large".into());
    }
    let bundle: Bundle =
        serde_json::from_slice(input).map_err(|_| "invalid settings generation")?;
    if bundle.format != FORMAT {
        return Err("unsupported settings generation".into());
    }
    if !bundle.ciphertext.starts_with("enc:v2:") {
        return Err("settings generation requires a context-bound ciphertext".into());
    }
    let protected = Zeroizing::new(
        B64.decode(&bundle.protected_material)
            .map_err(|_| "invalid settings material")?,
    );
    if protected.len() > 65536 {
        return Err("invalid settings material".into());
    }
    let material = Zeroizing::new(
        dpapi_unprotect(&protected).map_err(|_| "settings generation cannot be unlocked")?,
    );
    let array: &[u8; 32] = material
        .as_slice()
        .try_into()
        .map_err(|_| "invalid settings material")?;
    let key = Zeroizing::new(derive_section_key(array, None)?);
    let plain = Zeroizing::new(
        decode_section(&key, &bundle.ciphertext, SCOPE)
            .map_err(|_| "settings generation cannot be authenticated")?,
    );
    let values: BTreeMap<String, Value> =
        serde_json::from_slice(&plain).map_err(|_| "invalid settings generation")?;
    if values.len() > MAX_SECTIONS
        || values
            .keys()
            .any(|name| name.is_empty() || name.len() > 128 || validate_section_name(name).is_err())
    {
        return Err("invalid settings generation".into());
    }
    Ok(values)
}

/// A single file contains both protected key material and all encrypted
/// sections. Publication must be atomic; an ambiguous error requires readback.
fn publish(
    path: &Path,
    candidate: &[u8],
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    open(candidate)?;
    persist(path, candidate)
}

#[cfg(all(test, windows))]
#[path = "settings_rotation_trial_tests.rs"]
mod tests;
