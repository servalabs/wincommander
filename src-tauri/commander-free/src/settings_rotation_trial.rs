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
use sha2::{Digest, Sha256};
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    sections: BTreeMap<String, Value>,
    source_sections: BTreeMap<String, String>,
    source_key_digest: String,
}

fn source_digest(bytes: &[u8]) -> String {
    B64.encode(Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    B64.decode(value).is_ok_and(|bytes| bytes.len() == 32)
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
    let mut source_sections = BTreeMap::<String, String>::new();
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
        source_sections.insert(name.clone(), source_digest(encoded.as_bytes()));
    }
    let generation = Generation {
        sections: values,
        source_sections,
        source_key_digest: source_digest(old_material),
    };
    let plain = Zeroizing::new(
        serde_json::to_vec(&generation).map_err(|_| "cannot encode settings snapshot")?,
    );
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

fn open_generation(input: &[u8]) -> Result<Generation, String> {
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
    let generation: Generation =
        serde_json::from_slice(&plain).map_err(|_| "invalid settings generation")?;
    if generation.sections.len() > MAX_SECTIONS
        || generation.sections.keys().any(|name| {
            name.is_empty() || name.len() > 128 || validate_section_name(name).is_err()
        })
        || generation.sections.len() != generation.source_sections.len()
        || generation.sections.keys().ne(generation.source_sections.keys())
        || generation.source_sections.values().any(|digest| !valid_digest(digest))
        || !valid_digest(&generation.source_key_digest)
    {
        return Err("invalid settings generation".into());
    }
    Ok(generation)
}

fn open(input: &[u8]) -> Result<BTreeMap<String, Value>, String> {
    Ok(open_generation(input)?.sections)
}

/// Diagnose whether a newly enumerated snapshot matches the source used for
/// this generation. Matching is not an old-writer fence: ownership must be
/// held through activation, and a trusted anti-rollback policy is still needed.
fn verify_source(input: &[u8], old_material: &[u8; 32], sections: &[(String, String)]) -> Result<(), String> {
    let generation = open_generation(input)?;
    if generation.source_key_digest != source_digest(old_material) {
        return Err("settings source key changed; do not activate generation".into());
    }
    let mut observed = BTreeMap::new();
    for (name, encoded) in sections {
        if observed.insert(name.clone(), source_digest(encoded.as_bytes())).is_some() {
            return Err("duplicate settings section".into());
        }
    }
    if generation.source_sections != observed {
        return Err("settings source sections changed; do not activate generation".into());
    }
    // The encrypted manifest binds source bytes but cannot by itself prove the
    // candidate values came from them. Re-decrypt every original and compare.
    let old_key = Zeroizing::new(derive_section_key(old_material, None)?);
    for (name, encoded) in sections {
        let bytes = Zeroizing::new(
            decode_section(&old_key, encoded.trim(), &format!("machine:{name}"))
                .map_err(|_| "settings source section cannot be authenticated")?,
        );
        let original: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "settings source section is invalid")?;
        if generation.sections.get(name) != Some(&original) {
            return Err("settings generation differs from original values".into());
        }
    }
    Ok(())
}

/// A single file contains both protected key material and all encrypted
/// sections. Publication must be atomic; an ambiguous error requires readback.
fn publish(
    path: &Path,
    candidate: &[u8],
    old_material: &[u8; 32],
    sections: &[(String, String)],
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    verify_source(candidate, old_material, sections)?;
    persist(path, candidate)
}

#[cfg(all(test, windows))]
#[path = "settings_rotation_trial_tests.rs"]
mod tests;

#[cfg(windows)]
#[path = "settings_rotation_snapshot.rs"]
mod source_snapshot;
