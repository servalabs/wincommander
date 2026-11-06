// SPDX-License-Identifier: AGPL-3.0-or-later
//! Isolated settings-key rotation experiment; not a live migration protocol.
//! Existing shared installation material remains unchanged for logs and Pro.
use super::{
    decode_section, derive_section_key, dpapi_protect, dpapi_unprotect, encode_section,
    validate_section_name, user_dpapi_protect, user_dpapi_unprotect,
};
use super::settings_generation_cutover::{self as cutover, Anchor, AnchorState, ProtectedFileAnchor, SelectedFile};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::{Path, PathBuf}};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum StoreScope { Machine, CurrentUser }
impl StoreScope {
    pub(super) fn protect(self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        match self { Self::Machine => dpapi_protect(bytes), Self::CurrentUser => user_dpapi_protect(bytes).map_err(|_| Self::user_unavailable()) }
    }
    pub(super) fn unprotect(self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        match self { Self::Machine => dpapi_unprotect(bytes), Self::CurrentUser => user_dpapi_unprotect(bytes).map_err(|_| Self::user_unavailable()) }
    }
    fn user_unavailable() -> String {
        "Could not unlock or protect your settings. Sign in to the same Windows account with its user profile loaded, then retry; existing data has been preserved.".into()
    }
    fn access_error(self, error: String) -> String {
        match self { Self::Machine => error, Self::CurrentUser => Self::user_unavailable() }
    }
    fn section_scope(self, name: &str) -> String {
        format!("{}:{name}", match self { Self::Machine => "machine", Self::CurrentUser => "user-install" })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    scope: StoreScope,
    f6_initialized: bool,
    sections: BTreeMap<String, Value>,
    source_sections: BTreeMap<String, String>,
    source_key_digest: String,
}

fn matching_section<'a>(sections: &'a BTreeMap<String, Value>, name: &str) -> Option<&'a str> {
    let folded = name.to_lowercase();
    sections.keys().find(|existing| existing.to_lowercase() == folded).map(String::as_str)
}
pub(super) fn valid_f6(value: &Value) -> bool {
    value.get("signing_seed_hex").and_then(Value::as_str)
        .is_some_and(|seed| seed.len() == 64 && seed.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn source_digest(bytes: &[u8]) -> String {
    B64.encode(Sha256::digest(bytes))
}

/// A source proof is computed only from an already authenticated generation.
/// Length prefixes bind exact section names and bytes without delimiter
/// ambiguity; random resealing does not change this proof.
fn legacy_source_proof(generation: &Generation) -> Result<String, String> {
    let key = B64.decode(&generation.source_key_digest).map_err(|_| "invalid source key proof")?;
    if key.len() != 32 { return Err("invalid source key proof".into()); }
    let mut hash = Sha256::new();
    hash.update(b"wincommander-settings-source-proof-v1\0");
    hash.update([match generation.scope { StoreScope::Machine => 1, StoreScope::CurrentUser => 2 }]);
    hash.update(&key);
    hash.update((generation.source_sections.len() as u64).to_be_bytes());
    for (name, digest) in &generation.source_sections {
        let digest = B64.decode(digest).map_err(|_| "invalid source section proof")?;
        if digest.len() != 32 { return Err("invalid source section proof".into()); }
        hash.update((name.len() as u64).to_be_bytes());
        hash.update(name.as_bytes());
        hash.update(&digest);
    }
    Ok(hex::encode(hash.finalize()))
}

fn valid_digest(value: &str) -> bool {
    B64.decode(value).is_ok_and(|bytes| bytes.len() == 32)
}

// The machine snapshot can contain both historic shared-key sections and
// individually wrapped v3 sections produced by the separate envelope draft.
fn decode_snapshot_section(scope: StoreScope, old_key: &[u8; 32], name: &str, encoded: &str) -> Result<Vec<u8>, String> {
    let encoded = encoded.trim();
    if encoded.starts_with(super::settings_section_envelope::PREFIX) {
        super::settings_section_envelope::open(&scope.section_scope(name), encoded, |bytes| scope.unprotect(bytes))
    } else {
        decode_section(old_key, encoded, &format!("machine:{name}"))
    }
}

/// Snapshot input is supplied by a caller holding all source/key ownership.
/// It may include any general datastore section, not just the UI settings.
/// No original file is changed. A caller must establish directory and old-writer
/// fencing and use a separately authenticated cutover decision before use.
fn prepare(old_material: &[u8; 32], sections: &[(String, String)]) -> Result<Vec<u8>, String> {
    prepare_for(StoreScope::Machine, old_material, sections)
}
fn prepare_for(scope: StoreScope, old_material: &[u8; 32], sections: &[(String, String)]) -> Result<Vec<u8>, String> {
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
            decode_snapshot_section(scope, &old_key, name, encoded)
                .map_err(|_| "settings section cannot be authenticated")?,
        );
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "settings section is invalid")?;
        if values.insert(name.clone(), value).is_some() {
            return Err("duplicate settings section".into());
        }
        source_sections.insert(name.clone(), source_digest(encoded.as_bytes()));
    }
    let f6_initialized = match matching_section(&values, "f6-keystore") {
        Some(name) if valid_f6(&values[name]) => true,
        Some(_) => return Err("existing device signing key is empty or invalid; originals preserved".into()),
        None => false,
    };
    let generation = Generation {
        scope,
        f6_initialized,
        sections: values,
        source_sections,
        source_key_digest: source_digest(old_material),
    };
    seal_generation(&generation)
}

// Reseal the complete in-memory generation as one authenticated unit.
fn seal_generation(generation: &Generation) -> Result<Vec<u8>, String> {
    let plain = Zeroizing::new(
        serde_json::to_vec(generation).map_err(|_| "cannot encode settings snapshot")?,
    );
    if plain.len() > MAX_TOTAL {
        return Err("settings snapshot too large".into());
    }
    let mut material = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(material.as_mut());
    let protected = Zeroizing::new(generation.scope.protect(material.as_ref())?);
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
        || generation.source_sections.keys().any(|name| !generation.sections.contains_key(name))
        || {
            let mut names = std::collections::HashSet::new();
            generation.sections.keys().any(|name| !names.insert(name.to_lowercase()))
        }
        || generation.source_sections.values().any(|digest| !valid_digest(digest))
        || !valid_digest(&generation.source_key_digest)
        || match matching_section(&generation.sections, "f6-keystore") {
            Some(name) => !generation.f6_initialized || !valid_f6(&generation.sections[name]),
            None => generation.f6_initialized,
        }
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
    verify_source_for(StoreScope::Machine, input, old_material, sections)
}
fn verify_source_for(scope: StoreScope, input: &[u8], old_material: &[u8; 32], sections: &[(String, String)]) -> Result<(), String> {
    let generation = open_generation(input)?;
    if generation.scope != scope { return Err("settings generation scope mismatch".into()); }
    if generation.source_key_digest != source_digest(old_material) {
        return Err("settings source key changed; do not activate generation".into());
    }
    let mut observed = BTreeMap::new();
    for (name, encoded) in sections {
        if observed.insert(name.clone(), source_digest(encoded.as_bytes())).is_some() {
            return Err("duplicate settings section".into());
        }
    }
    if generation.source_sections != observed || generation.sections.keys().ne(observed.keys()) {
        return Err("settings source sections changed; do not activate generation".into());
    }
    // The encrypted manifest binds source bytes but cannot by itself prove the
    // candidate values came from them. Re-decrypt every original and compare.
    let old_key = Zeroizing::new(derive_section_key(old_material, None)?);
    for (name, encoded) in sections {
        let bytes = Zeroizing::new(
            decode_snapshot_section(scope, &old_key, name, encoded)
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

include!("settings_rotation_staging.rs");
include!("settings_rotation_runtime.rs");
include!("settings_rotation_migration.rs");
include!("settings_rotation_fresh.rs");

#[cfg(all(test, windows))]
#[path = "settings_rotation_trial_tests.rs"]
mod tests;

#[cfg(windows)]
#[path = "settings_rotation_snapshot.rs"]
mod source_snapshot;
