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
        || generation.source_sections.keys().any(|name| !generation.sections.contains_key(name))
        || {
            let mut names = std::collections::HashSet::new();
            generation.sections.keys().any(|name| !names.insert(name.to_lowercase()))
        }
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
    if generation.source_sections != observed || generation.sections.keys().ne(observed.keys()) {
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

/// Holds the created stage handle and an independent reader while the caller
/// verifies. Reopening the path each time detects observed replacement. Root
/// authorization and activation fencing remain production requirements.
struct StagedFile {
    path: PathBuf,
    written: std::fs::File,
    _reader: std::fs::File,
}

fn open_stage_reader(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    if std::fs::symlink_metadata(path).map_err(|_| "cannot inspect settings stage")?.file_type().is_symlink() {
        return Err("linked settings stage refused".into());
    }
    options.open(path).map_err(|_| "cannot reopen settings stage".into())
}

impl StagedFile {
    fn verify(&self, candidate: &[u8], old_material: &[u8; 32], sections: &[(String, String)]) -> Result<(), String> {
        use std::io::Read;
        let limit = MAX_TOTAL * 2 + 65536;
        let readback = open_stage_reader(&self.path)?;
        if !same_stage_file(&self.written, &readback)? { return Err("settings stage identity changed".into()); }
        let metadata = readback.metadata().map_err(|_| "cannot inspect settings stage")?;
        if !metadata.is_file() || metadata.len() > limit as u64 { return Err("invalid settings stage".into()); }
        let mut actual = Vec::new();
        readback.take(limit as u64 + 1).read_to_end(&mut actual).map_err(|_| "cannot read settings stage")?;
        if actual.len() > limit || actual != candidate { return Err("settings stage readback mismatch".into()); }
        verify_source(&actual, old_material, sections)
    }
}

/// Reopen an existing unactivated stage after restart, without modifying it.
/// The caller must hold and authenticate the complete source snapshot and
/// authorize the containing directory before considering any activation.
fn recover_stage(path: &Path, old_material: &[u8; 32], sections: &[(String, String)]) -> Result<(StagedFile, Vec<u8>), String> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
        options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    if std::fs::symlink_metadata(path).map_err(|_| "cannot inspect settings stage")?.file_type().is_symlink() {
        return Err("linked settings stage refused".into());
    }
    let written = options.open(path).map_err(|_| "cannot open settings stage for recovery")?;
    let reader = open_stage_reader(path)?;
    if !same_stage_file(&written, &reader)? { return Err("settings stage identity changed".into()); }
    let limit = MAX_TOTAL * 2 + 65536;
    let metadata = reader.metadata().map_err(|_| "cannot inspect settings stage")?;
    if !metadata.is_file() || metadata.len() > limit as u64 { return Err("invalid settings stage".into()); }
    let mut candidate = Vec::new();
    (&reader).take(limit as u64 + 1).read_to_end(&mut candidate).map_err(|_| "cannot read settings stage")?;
    if candidate.len() > limit { return Err("invalid settings stage".into()); }
    verify_source(&candidate, old_material, sections)?;
    let guard = StagedFile { path: path.to_path_buf(), written, _reader: reader };
    guard.verify(&candidate, old_material, sections)?;
    Ok((guard, candidate))
}

/// Create a new stage without overwrite and hold it through readback. A failed
/// attempt may leave an unactivated file that must be inspected, not activated.
fn stage_new_with_hook(
    path: &Path,
    candidate: &[u8],
    old_material: &[u8; 32],
    sections: &[(String, String)],
    after_write: &impl Fn(&Path, &mut std::fs::File) -> Result<(), String>,
) -> Result<StagedFile, String> {
    use std::io::Write;
    let limit = MAX_TOTAL * 2 + 65536;
    if candidate.len() > limit { return Err("settings generation too large".into()); }
    verify_source(candidate, old_material, sections)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    let mut written = options.open(path).map_err(|_| "settings stage already exists or is unavailable")?;
    written.write_all(candidate).and_then(|_| written.sync_all())
        .map_err(|_| "settings stage write failed; inspect destination")?;
    after_write(path, &mut written)?;
    let reader = open_stage_reader(path)?;
    if !same_stage_file(&written, &reader)? { return Err("settings stage identity changed".into()); }
    let guard = StagedFile { path: path.to_path_buf(), written, _reader: reader };
    guard.verify(candidate, old_material, sections)?;
    Ok(guard)
}

#[cfg(windows)]
fn same_stage_file(first: &std::fs::File, second: &std::fs::File) -> Result<bool, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT};
    let mut a = BY_HANDLE_FILE_INFORMATION::default();
    let mut b = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: both handles and output structs remain valid throughout the calls.
    if unsafe { GetFileInformationByHandle(first.as_raw_handle(), &mut a) } == 0
        || unsafe { GetFileInformationByHandle(second.as_raw_handle(), &mut b) } == 0 {
        return Err("cannot verify settings stage identity".into());
    }
    Ok(a.nNumberOfLinks == 1 && b.nNumberOfLinks == 1
        && a.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
        && b.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
        && a.dwVolumeSerialNumber == b.dwVolumeSerialNumber
        && a.nFileIndexHigh == b.nFileIndexHigh && a.nFileIndexLow == b.nFileIndexLow)
}

#[cfg(unix)]
fn same_stage_file(first: &std::fs::File, second: &std::fs::File) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    let a = first.metadata().map_err(|_| "cannot verify settings stage identity")?;
    let b = second.metadata().map_err(|_| "cannot verify settings stage identity")?;
    Ok(a.nlink() == 1 && b.nlink() == 1 && a.dev() == b.dev() && a.ino() == b.ino())
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

/// Gated production seam: keeps cooperative ownership for every operation on
/// a selected generation. No runtime route constructs this yet. Before wiring,
/// callers must authorize and pin the root, fence old executables, and protect
/// the selected file and its authoritative state against replacement.
pub(super) struct LockedSelectedGeneration {
    _ownership: super::settings_generation_ownership::SettingsGenerationLock,
    path: PathBuf,
}

impl LockedSelectedGeneration {
    pub(super) fn try_open(root: &Path) -> Result<Self, String> {
        let ownership = super::settings_generation_ownership::SettingsGenerationLock::try_acquire(root)
            .map_err(|_| "settings generation ownership unavailable")?;
        let path = root.join(".settings-generation.v1");
        let selected = Self { _ownership: ownership, path };
        // Presence alone is not authorization, but the candidate must at least
        // be readable and authenticated before this owner is returned.
        selected.read_all()?;
        Ok(selected)
    }

    pub(super) fn read_all(&self) -> Result<BTreeMap<String, Value>, String> {
        use std::io::Read;
        let file = std::fs::File::open(&self.path).map_err(|_| "selected settings generation unavailable")?;
        let mut bytes = Vec::new();
        file.take((MAX_TOTAL * 2 + 65537) as u64).read_to_end(&mut bytes)
            .map_err(|_| "cannot read selected settings generation")?;
        open(&bytes)
    }

    pub(super) fn write_section(&self, section: &str, value: Value) -> Result<(), String> {
        update_selected_generation(&self.path, section, value, &crate::datastore_io::atomic_write)
    }
}

/// Trial reader/writer for an already selected generation. The caller must
/// serialize all readers and writers and keep trusted selection state; neither
/// condition is supplied by this experiment. Never falls back to the legacy
/// store if the selected generation is unreadable or corrupt.
fn update_selected_generation(
    path: &Path,
    section: &str,
    value: Value,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| "selected settings generation unavailable")?;
    let mut input = Vec::new();
    file.take((MAX_TOTAL * 2 + 65537) as u64).read_to_end(&mut input)
        .map_err(|_| "cannot read selected settings generation")?;
    let mut generation = open_generation(&input)?;
    if section.is_empty() || section.len() > 128 { return Err("invalid settings section".into()); }
    validate_section_name(section)?;
    if !generation.sections.contains_key(section)
        && (generation.sections.len() >= MAX_SECTIONS || generation.sections.keys().any(|name| name.to_lowercase() == section.to_lowercase())) {
        return Err("duplicate or excessive settings sections".into());
    }
    generation.sections.insert(section.to_owned(), value);
    let candidate = seal_generation(&generation)?;
    // Reject a self-inconsistent output before any persistent modification.
    let reopened = open_generation(&candidate)?;
    if reopened.sections != generation.sections { return Err("settings generation readback mismatch".into()); }
    persist(path, &candidate)
}

/// Test-only single-file generation selection. The caller must hold a complete,
/// authorized source snapshot and exclusive migration ownership through this
/// call. Existing legacy files are never renamed or deleted. No production
/// reader or writer selects this file yet.
fn select_initial_generation(
    path: &Path,
    candidate: &[u8],
    old_material: &[u8; 32],
    sections: &[(String, String)],
    create: &impl Fn(&Path, &[u8]) -> Result<bool, String>,
) -> Result<(), String> {
    use std::io::Read;
    verify_source(candidate, old_material, sections)?;
    let publication = create(path, candidate);
    let file = std::fs::File::open(path).map_err(|_| "selected settings generation unavailable; legacy preserved")?;
    let mut observed = Vec::new();
    file.take((MAX_TOTAL * 2 + 65537) as u64).read_to_end(&mut observed)
        .map_err(|_| "cannot read selected settings generation; legacy preserved")?;
    if observed.len() > MAX_TOTAL * 2 + 65536 { return Err("selected settings generation too large; legacy preserved".into()); }
    // If our publish succeeded, or its result is ambiguous, only our exact
    // bytes can establish that it committed. An existing other winner must
    // independently authenticate against the exact guarded original values.
    if !matches!(publication, Ok(false)) && observed != candidate {
        return Err("settings generation publication ambiguous; legacy preserved".into());
    }
    verify_source(&observed, old_material, sections)?;
    Ok(())
}

#[cfg(all(test, windows))]
#[path = "settings_rotation_trial_tests.rs"]
mod tests;

#[cfg(windows)]
#[path = "settings_rotation_snapshot.rs"]
mod source_snapshot;
