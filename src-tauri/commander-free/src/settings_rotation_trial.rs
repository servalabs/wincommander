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

/// Unwired selected-generation store. Callers must authorize the root, fence
/// old binaries, and provide a trusted anti-rollback anchor before integration.
pub(super) struct LockedSelectedGeneration {
    _ownership: super::settings_generation_ownership::SettingsGenerationLock,
    #[cfg(windows)]
    _root_pins: Vec<std::fs::File>,
    path: PathBuf,
    authority: PathBuf,
    scope: StoreScope,
}

// No missing/error conflation. Windows holds a no-follow, no-write/delete
// handle for each read; the owner also holds directory pins for its lifetime.
pub(super) fn read_generation_file(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
        options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(not(windows))]
    if path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) { return Err("linked settings file refused".into()); }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("settings generation file unavailable".into()),
    };
    let metadata = file.metadata().map_err(|_| "cannot inspect settings file")?;
    if !metadata.is_file() || metadata.len() > limit as u64 { return Err("invalid settings file".into()); }
    #[cfg(windows)] {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT};
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file handle and output structure remain valid for the call.
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.nNumberOfLinks != 1 { return Err("invalid settings file".into()); }
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|_| "cannot read settings file")?;
    if bytes.len() > limit { return Err("settings file too large".into()); }
    Ok(Some(bytes))
}

#[derive(Debug, PartialEq, Eq)]
enum OwnerAcquireError { Busy, Unavailable }
impl OwnerAcquireError {
    fn message(&self) -> &'static str {
        match self {
            Self::Busy => "Settings are busy. Wait a moment and try again; existing data was preserved.",
            Self::Unavailable => "Settings recovery requires an authorized administrator or the owning Windows account. Existing data was preserved.",
        }
    }
    fn from_io(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::WouldBlock { Self::Busy } else { Self::Unavailable }
    }
}
impl LockedSelectedGeneration {
    fn acquire(root: &Path) -> Result<Self, String> { Self::acquire_for(root, StoreScope::Machine) }
    fn acquire_for(root: &Path, scope: StoreScope) -> Result<Self, String> {
        Self::acquire_for_detailed(root, scope).map_err(|e| e.message().to_string())
    }
    fn acquire_for_detailed(root: &Path, scope: StoreScope) -> Result<Self, OwnerAcquireError> {
        #[cfg(windows)]
        let root_pins = source_snapshot::pin_directories(root).map_err(|_| OwnerAcquireError::Unavailable)?;
        let ownership = match scope {
            StoreScope::Machine => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_machine(root, false),
            StoreScope::CurrentUser => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_user(root, false),
        }.map_err(OwnerAcquireError::from_io)?;
        Ok(Self { _ownership: ownership, #[cfg(windows)] _root_pins: root_pins,
            path: root.join(".settings-generation.v1"), authority: root.join(".settings-cutover-anchor.v1"), scope })
    }

    pub(super) fn try_open(root: &Path) -> Result<Self, String> { Self::try_open_for(root, StoreScope::Machine) }
    pub(super) fn try_open_for(root: &Path, scope: StoreScope) -> Result<Self, String> {
        let selected = Self::acquire_for(root, scope)?;
        selected.read_reconciled()?;
        Ok(selected)
    }

    /// Explicit initial selection. Legacy files stay in place and the source
    /// snapshot must be fresh and held until independent anchor publication.
    #[cfg(windows)]
    fn initialize(snapshot: &mut source_snapshot::Snapshot, candidate: &[u8]) -> Result<Self, String> {
        let root = snapshot.root()?;
        let selected = Self::acquire_for(&root, snapshot.scope())?;
        let anchor = ProtectedFileAnchor::new(&root, selected.scope);
        let file = SelectedFile::new(&root);
        let guarded = std::cell::RefCell::new(snapshot);
        cutover::select_initial(&selected._ownership, &anchor, &file, candidate, &|bytes| guarded.borrow_mut().verify_generation(bytes))?;
        Ok(selected)
    }

    /// Interrupted first selection requires a fresh guarded source and the
    /// exact selected candidate; it never falls back to the legacy path.
    #[cfg(windows)]
    fn recover_initial(snapshot: &mut source_snapshot::Snapshot) -> Result<Self, String> {
        let root = snapshot.root()?;
        let candidate = read_generation_file(&root.join(".settings-generation.v1"), MAX_TOTAL * 2 + 65_536)?
            .ok_or("initial selected generation unavailable; retry with original candidate")?;
        Self::initialize(snapshot, &candidate)
    }

    fn read_reconciled(&self) -> Result<Vec<u8>, String> {
        let root = self.path.parent().ok_or("settings root unavailable")?;
        let anchor = ProtectedFileAnchor::new(root, self.scope);
        let file = SelectedFile::new(root);
        let verify = |bytes: &[u8]| verify_scoped_generation(self.scope, bytes);
        if matches!(anchor.read()?, Some(AnchorState::Updating { .. })) {
            cutover::recover_update(&self._ownership, &anchor, &file, &verify)?;
        }
        cutover::read_active(&self._ownership, &anchor, &file, &verify)
    }

    pub(super) fn read_all(&self) -> Result<BTreeMap<String, Value>, String> {
        open(&self.read_reconciled()?)
    }

    /// The exclusive owner covers the read, generation, write and readback.
    fn get_or_create_f6(&self, create: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
        let current = open_generation(&self.read_reconciled()?)?;
        if let Some(name) = matching_section(&current.sections, "f6-keystore") {
            let value = current.sections[name].clone();
            if !valid_f6(&value) { return Err("device signing key is invalid; existing data preserved".into()); }
            return Ok(value);
        }
        if current.f6_initialized { return Err("device signing key is missing; existing data preserved".into()); }
        let created = create()?;
        if !valid_f6(&created) { return Err("new device signing key is invalid".into()); }
        self.write_section("f6-keystore", created.clone())?;
        let committed = open_generation(&self.read_reconciled()?)?;
        let name = matching_section(&committed.sections, "f6-keystore").ok_or("committed device signing key missing")?;
        if committed.sections[name] != created { return Err("committed device signing key did not verify".into()); }
        Ok(created)
    }

    pub(super) fn write_section(&self, section: &str, value: Value) -> Result<(), String> {
        let current = self.read_reconciled()?;
        let candidate = updated_generation(&current, section, value)?;
        let root = self.path.parent().ok_or("settings root unavailable")?;
        let anchor = ProtectedFileAnchor::new(root, self.scope);
        let file = SelectedFile::new(root);
        cutover::update(&self._ownership, &anchor, &file, &candidate, &|bytes| verify_scoped_generation(self.scope, bytes))
    }
}

/// Read-only seam for a pre-provisioned shared lock. No automatic repair is
/// permitted under a shared lock. Root/file ACLs and old-version fencing are
/// prerequisites. Only the explicitly opted-in Windows lab route uses it.
struct ReadOnlySelectedGeneration {
    _ownership: super::settings_generation_ownership::SettingsGenerationLock,
    #[cfg(windows)]
    _root_pins: Vec<std::fs::File>,
    path: PathBuf,
    authority: PathBuf,
    scope: StoreScope,
}
impl ReadOnlySelectedGeneration {
    fn acquire(root: &Path, scope: StoreScope) -> Result<Self, String> {
        #[cfg(windows)]
        let root_pins = source_snapshot::pin_directories(root)?;
        let ownership = match scope {
            StoreScope::Machine => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_machine(root, true),
            StoreScope::CurrentUser => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_user(root, true),
        }.map_err(|error| format!("settings generation shared ownership unavailable: {error}"))?;
        Ok(Self { _ownership: ownership, #[cfg(windows)] _root_pins: root_pins,
            path: root.join(".settings-generation.v1"), authority: root.join(".settings-cutover-anchor.v1"), scope })
    }
    fn try_open(root: &Path, scope: StoreScope) -> Result<Self, String> {
        let reader = Self::acquire(root, scope)?;
        reader.read_all()?;
        Ok(reader)
    }
    fn inspect_startup(&self) -> Result<StartupView, String> {
        inspect_generation_files(&self._ownership, &self.path, &self.authority, self.scope)
    }
    fn read_all(&self) -> Result<BTreeMap<String, Value>, String> {
        match self.inspect_startup()? {
            StartupView::Selected(sections) => Ok(sections),
            StartupView::Legacy => Err("settings generation has not been selected".into()),
            StartupView::RecoveryRequired => Err("settings generation requires authorized recovery".into()),
        }
    }
}

fn verify_scoped_generation(scope: StoreScope, bytes: &[u8]) -> Result<(), String> {
    if open_generation(bytes).map_err(|error| scope.access_error(error))?.scope != scope {
        return Err("settings generation scope mismatch".into());
    }
    Ok(())
}
fn inspect_generation_files(guard: &super::settings_generation_ownership::SettingsGenerationLock, path: &Path, authority_path: &Path, scope: StoreScope) -> Result<StartupView, String> {
    let root = authority_path.parent().ok_or("settings root unavailable")?;
    let anchor = ProtectedFileAnchor::new(root, scope);
    let file = SelectedFile::new(root);
    match anchor.read()? {
        Some(AnchorState::Legacy) => {
            if read_generation_file(path, MAX_TOTAL * 2 + 65_536)?.is_some() { return Err("unexpected selected settings before cutover".into()); }
            Ok(StartupView::Legacy)
        },
        Some(AnchorState::Active { .. }) => {
            let bytes = cutover::read_active(guard, &anchor, &file, &|value| verify_scoped_generation(scope, value))?;
            Ok(StartupView::Selected(open_generation(&bytes).map_err(|error| scope.access_error(error))?.sections))
        },
        Some(AnchorState::Preparing { .. } | AnchorState::Updating { .. }) => Ok(StartupView::RecoveryRequired),
        None => Err("settings cutover anchor is missing; existing data preserved".into()),
    }
}

/// Keep shared ownership through a legacy read or selected-generation read.
pub(super) fn read_with_legacy(root: &Path, scope: StoreScope, section: &str, legacy: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    let owner = ReadOnlySelectedGeneration::acquire(root, scope)?;
    match owner.inspect_startup()? {
        StartupView::Legacy => legacy(),
        StartupView::Selected(sections) => Ok(selected_section(sections, section)),
        StartupView::RecoveryRequired => {
            // Never upgrade in place: release shared ownership before asking
            // for exclusive repair, then re-read the anchor under that guard.
            drop(owner);
            let exclusive = LockedSelectedGeneration::acquire_for_detailed(root, scope)
                .map_err(|error| error.message().to_string())?;
            let anchor = ProtectedFileAnchor::new(root, scope);
            match anchor.read()? {
                Some(AnchorState::Updating { .. } | AnchorState::Active { .. }) => {
                    let sections = exclusive.read_all()?;
                    Ok(selected_section(sections, section))
                },
                _ => Err("Settings migration requires authorized recovery from the original source. Existing data was preserved.".into()),
            }
        },
    }
}
fn selected_section(mut sections: BTreeMap<String, Value>, section: &str) -> Value {
    let name = matching_section(&sections, section).map(str::to_owned);
    // Missing F6 is safe only because the authenticated generation has an
    // explicit never-initialized state; whole-anchor rollback remains open.
    name.and_then(|name| sections.remove(&name)).unwrap_or_else(|| Value::Object(Default::default()))
}

/// Read an existing F6 key with shared ownership. Only authenticated absence
/// permits the caller to attempt exclusive creation; that caller must re-read
/// under exclusive ownership before generating anything.
pub(super) fn read_f6_or_create(
    root: &Path,
    scope: StoreScope,
    read_legacy: impl FnOnce() -> Result<Value, String>,
    create_exclusive: impl FnOnce() -> Result<Value, String>,
) -> Result<Value, String> {
    let value = read_with_legacy(root, scope, "f6-keystore", read_legacy)?;
    if valid_f6(&value) { return Ok(value); }
    if value != Value::Object(Default::default()) {
        return Err("device signing key is invalid; existing data preserved".into());
    }
    create_exclusive()
}

/// Keep exclusive ownership through either a legacy save or a selected write.
pub(super) fn write_with_legacy(root: &Path, scope: StoreScope, section: &str, value: &Value, legacy: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let owner = LockedSelectedGeneration::acquire_for(root, scope)?;
    match inspect_generation_files(&owner._ownership, &owner.path, &owner.authority, scope)? {
        StartupView::Legacy => legacy(),
        StartupView::Selected(_) => owner.write_section(section, value.clone()),
        StartupView::RecoveryRequired => Err("settings generation requires authorized recovery".into()),
    }
}

/// Holds exclusive ownership across settings preflight and the final write.
/// Root/lock ACL and old-version fencing remain activation prerequisites.
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(super) struct RuntimeWriteGuard {
    owner: LockedSelectedGeneration,
    _root_pins: Vec<std::fs::File>,
    selected: bool,
}
#[cfg(all(windows, feature = "experimental-settings-generation"))]
impl RuntimeWriteGuard {
    pub(super) fn selected(&self) -> bool { self.selected }
    pub(super) fn get_or_create_f6(&self, create: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
        if !self.selected { return Err("settings generation is not selected".into()); }
        self.owner.get_or_create_f6(create)
    }
    pub(super) fn write(&self, section: &str, value: &Value) -> Result<(), String> {
        if !self.selected { return Err("settings generation is not selected".into()); }
        self.owner.write_section(section, value.clone())
    }
}
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(super) fn begin_experimental_write() -> Result<RuntimeWriteGuard, String> {
    let (root, pins, scope) = experimental_runtime_root()?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let selected = match inspect_generation_files(&owner._ownership, &owner.path, &owner.authority, owner.scope)? {
        StartupView::Legacy => false,
        StartupView::Selected(_) => true,
        StartupView::RecoveryRequired => {
            let anchor = ProtectedFileAnchor::new(&root, scope);
            if matches!(anchor.read()?, Some(AnchorState::Updating { .. })) {
                owner.read_reconciled()?;
                true
            } else {
                return Err("Settings migration requires authorized recovery from the original source. Existing data was preserved.".into());
            }
        },
    };
    Ok(RuntimeWriteGuard { owner, _root_pins: pins, selected })
}

/// Resolve and pin the selected store root for the explicit lab route.
/// ACL and cross-user/noninteractive behavior remain unvalidated on Windows.
#[cfg(windows)]
pub(super) fn experimental_runtime_root() -> Result<(PathBuf, Vec<std::fs::File>, StoreScope), String> {
    let scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    experimental_runtime_root_for(scope)
}

#[cfg(windows)]
fn experimental_runtime_root_for(scope: StoreScope) -> Result<(PathBuf, Vec<std::fs::File>, StoreScope), String> {
    let root = source_snapshot::known_root(scope)?;
    let pins = source_snapshot::pin_matching_roots(&root, scope)?;
    let root_handle = pins.last().ok_or("settings root unavailable")?;
    match scope {
        StoreScope::Machine => super::settings_generation_acl_windows::verify_machine_root_handle(root_handle),
        StoreScope::CurrentUser => super::settings_generation_acl_windows::verify_current_user_root_handle(root_handle),
    }.map_err(|_| "settings root permissions are not authorized")?;
    Ok((root, pins, scope))
}

/// Hold the search-maintenance fence for the lifetime of the installing
/// process. The parent handle is acquired before readiness is sent, so normal
/// installer exit and crash both release this cooperative user-profile lock.
#[cfg(windows)]
pub(super) fn hold_search_fence_for_installer(pid: u32) -> Result<(), String> {
    let (root, _root_pins, _) = experimental_runtime_root_for(StoreScope::CurrentUser)?;
    hold_search_fence_at(&root.join("file-search"), pid)
}

#[cfg(windows)]
fn hold_search_fence_at(parent_dir: &std::path::Path, pid: u32) -> Result<(), String> {
    use std::{fs::OpenOptions, io::Write, os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle}};
    use windows_sys::Win32::{Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, INFINITE}};
    struct Process(*mut std::ffi::c_void);
    impl Drop for Process { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    if pid == 0 { return Err("Installer process is unavailable.".into()); }
    let parent = Process(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) });
    if parent.0.is_null() { return Err("Cannot verify installer process lifetime.".into()); }
    if unsafe { WaitForSingleObject(parent.0, 0) } != WAIT_TIMEOUT { return Err("Installer already exited.".into()); }
    match std::fs::create_dir(parent_dir) {
        Ok(()) => {},
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(_) => return Err("Cannot prepare search maintenance directory.".into()),
    }
    let _search_pins = source_snapshot::pin_directories(parent_dir)?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(parent_dir.join(".encrypted-search-maintenance.lock")).map_err(|_| "Cannot open search maintenance fence.")?;
    let metadata = file.metadata().map_err(|_| "Cannot inspect search maintenance fence.")?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 || info.nNumberOfLinks != 1 {
        return Err("Search maintenance fence is not a safe file.".into());
    }
    file.try_lock().map_err(|_| "Close WinCommander search before installing the update.")?;
    if unsafe { WaitForSingleObject(parent.0, 0) } != WAIT_TIMEOUT { return Err("Installer exited before maintenance began.".into()); }
    println!("WINCOMMANDER_SEARCH_MAINTENANCE_READY_V1");
    std::io::stdout().flush().map_err(|_| "Cannot confirm search maintenance ownership.")?;
    if unsafe { WaitForSingleObject(parent.0, INFINITE) } != WAIT_OBJECT_0 { return Err("Cannot track installer process lifetime.".into()); }
    Ok(())
}

/// Read-only startup decision; callers must not fall back to legacy when this
/// reports recovery or an error. The Windows lab route uses the same decision.
#[derive(Debug, PartialEq)]
pub(super) enum StartupView {
    Legacy,
    Selected(BTreeMap<String, Value>),
    RecoveryRequired,
}
pub(super) fn inspect_startup_once(root: &Path, scope: StoreScope) -> Result<StartupView, String> {
    ReadOnlySelectedGeneration::acquire(root, scope)?.inspect_startup()
}

/// Open and release shared ownership for this read only. It cannot be cached by
/// callers, avoiding long-lived reader locks that would block policy writers.
pub(super) fn read_selected_once(root: &Path, scope: StoreScope) -> Result<BTreeMap<String, Value>, String> {
    ReadOnlySelectedGeneration::acquire(root, scope)?.read_all()
}

/// Prepare a reusable encrypted candidate before recording the cutover.
/// Existing stage bytes are never overwritten; source verification detects a
/// stale or incomplete stage. The caller holds exclusive ownership and pins.
fn stage_and_select<A: Anchor>(
    guard: &super::settings_generation_ownership::SettingsGenerationLock,
    anchor: &A,
    root: &Path,
    prepare: impl FnOnce() -> Result<Vec<u8>, String>,
    verify: &impl Fn(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    guard.require_exclusive().map_err(|e| e.to_string())?;
    let stage_path = root.join(".settings-cutover-candidate.v1");
    let state = anchor.read()?.ok_or("settings cutover anchor is missing")?;
    let candidate = match state {
        AnchorState::Legacy => match read_generation_file(&stage_path, MAX_TOTAL * 2 + 65_536)? {
            Some(existing) => existing,
            None => {
                let candidate = prepare()?;
                verify(&candidate)?;
                if !crate::datastore_io::publish_new(&stage_path, &candidate)? {
                    return Err("settings cutover candidate already exists; retry recovery".into());
                }
                let observed = read_generation_file(&stage_path, MAX_TOTAL * 2 + 65_536)?
                    .ok_or("settings cutover candidate is missing")?;
                if observed != candidate { return Err("settings cutover candidate did not verify".into()); }
                observed
            }
        },
        AnchorState::Preparing { .. } => read_generation_file(&stage_path, MAX_TOTAL * 2 + 65_536)?
            .ok_or("prepared settings candidate is missing; restore it before recovery")?,
        _ => return Err("settings cutover is not awaiting initial selection".into()),
    };
    verify(&candidate)?;
    cutover::select_initial(guard, anchor, &SelectedFile::new(root), &candidate, verify)
}

/// Explicit lab-only migration entry point. No application or installer route
/// invokes this function. Root ACL, old-writer exclusion and independent anchor
/// authorization remain mandatory external prerequisites.
#[cfg(windows)]
pub(super) fn migrate_explicitly() -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let (root, _pins, scope) = experimental_runtime_root()?;
    match scope {
        StoreScope::Machine => super::settings_generation_acl_windows::provision_or_verify(&root),
        StoreScope::CurrentUser => super::settings_generation_acl_windows::provision_user_or_verify(&root),
    }.map_err(|_| "cannot provision authorized settings lock; existing data preserved")?;
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    let initial = anchor.read()?;
    match initial {
        Some(AnchorState::Active { .. }) => { owner.read_reconciled()?; return Ok(()); },
        Some(AnchorState::Legacy | AnchorState::Preparing { .. }) | None => {},
        Some(AnchorState::Updating { .. }) => return Err("settings update requires authorized recovery before migration".into()),
    }
    let snapshot = match scope {
        StoreScope::Machine => source_snapshot::Snapshot::capture_machine()?,
        StoreScope::CurrentUser => source_snapshot::Snapshot::capture_user()?,
    };
    let held = std::cell::RefCell::new(snapshot);
    if initial.is_none() {
        // Authenticate every original, including F6, before provisioning the
        // first marker. This assumes the first rollout has never selected a
        // prior generation; losing all old cutover artifacts is undetectable.
        let probe = held.borrow_mut().prepare()?;
        cutover::provision_first_migration_with_stage(&owner._ownership, &anchor, &root, &|stage| {
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            held.borrow_mut().verify_generation(stage.unwrap_or(&probe))
        })?;
    }
    stage_and_select(&owner._ownership, &anchor, &root,
        || held.borrow_mut().prepare(),
        &|bytes| {
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            held.borrow_mut().verify_generation(bytes)
        })
}

/// Pre-replacement staging only. It never provisions a cutover anchor or
/// selects a generation, so the original installation remains authoritative.
/// User staging additionally requires an expected SID checked by the caller.
/// Installer handoff and reliable identity capture are not yet implemented.
#[cfg(windows)]
pub(super) fn stage_existing_explicitly(expected_scope: StoreScope) -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let actual_scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    if expected_scope == StoreScope::Machine && actual_scope != expected_scope {
        return Err("Settings installation scope does not match the running application; existing data preserved.".into());
    }
    let root = source_snapshot::known_root(expected_scope)?;
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && expected_scope == StoreScope::Machine => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err("Original user settings root is missing; no migration was performed.".into()),
        Err(_) => return Err("Cannot inspect settings root; existing data preserved.".into()),
        Ok(_) => {},
    }
    let (root, _pins, scope) = experimental_runtime_root_for(expected_scope)?;
    let _installed_binary = if scope == StoreScope::CurrentUser {
        Some(source_snapshot::pin_user_installed_binary(&root)?)
    } else { None };
    let key_name = match scope { StoreScope::Machine => ".install.material", StoreScope::CurrentUser => ".user-store.material" };
    let key = read_generation_file(&root.join(key_name), 65_536)?;
    if key.is_none() {
        let store = root.join("store");
        match std::fs::read_dir(&store) {
            Ok(mut entries) => {
                if entries.next().is_none() { return Ok(()); }
                return Err("Settings key is missing for existing store; existing data preserved.".into());
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            _ => return Err("Settings key is missing or store cannot be inspected; existing data preserved.".into()),
        }
    }
    match scope {
        StoreScope::Machine => super::settings_generation_acl_windows::provision_or_verify(&root),
        StoreScope::CurrentUser => super::settings_generation_acl_windows::provision_user_or_verify(&root),
    }.map_err(|_| "Cannot provision settings lock; existing data preserved.")?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    match anchor.read()? {
        Some(AnchorState::Active { .. }) => { owner.read_reconciled()?; return Ok(()); },
        Some(AnchorState::Updating { .. }) => return Err("Settings update requires recovery; existing data preserved.".into()),
        Some(AnchorState::Legacy | AnchorState::Preparing { .. }) | None => {},
    }
    let mut snapshot = match scope {
        StoreScope::Machine => source_snapshot::Snapshot::capture_machine()?,
        StoreScope::CurrentUser => source_snapshot::Snapshot::capture_user_from_verified_helper()?,
    };
    let stage = root.join(".settings-cutover-candidate.v1");
    let candidate = match read_generation_file(&stage, MAX_TOTAL * 2 + 65_536)? {
        Some(existing) => existing,
        None => {
            let prepared = snapshot.prepare()?;
            snapshot.verify_generation(&prepared)?;
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            if !crate::datastore_io::publish_new(&stage, &prepared)? { return Err("Settings staging changed; retry with originals preserved.".into()); }
            let observed = read_generation_file(&stage, MAX_TOTAL * 2 + 65_536)?.ok_or("Settings stage unavailable")?;
            if observed != prepared { return Err("Settings stage readback failed; originals preserved.".into()); }
            observed
        },
    };
    if let Some(AnchorState::Preparing { next }) = anchor.read()? {
        if cutover::digest(&candidate) != next { return Err("Settings stage does not match pending recovery; originals preserved.".into()); }
    }
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    snapshot.verify_generation(&candidate)
}

/// Installer decision under the same maintenance checks. A protected Active
/// generation is reused; an existing legacy key is migrated; otherwise the
/// conservative fresh path rejects any unexpected existing file or directory.
#[cfg(windows)]
pub(super) fn install_migration_explicitly() -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    let root = source_snapshot::known_root(scope)?;
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return provision_verified_fresh_install(),
        Err(_) => return Err("Cannot inspect settings root; existing data was preserved.".into()),
        Ok(_) => {},
    }
    let (root, _pins, scope) = experimental_runtime_root()?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    if matches!(anchor.read()?, Some(AnchorState::Active { .. } | AnchorState::Updating { .. })) {
        let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
        owner.read_reconciled()?;
        return Ok(());
    }
    let key_name = match scope { StoreScope::Machine => ".install.material", StoreScope::CurrentUser => ".user-store.material" };
    if read_generation_file(&root.join(key_name), 65_536)?.is_some() {
        migrate_explicitly()
    } else {
        provision_verified_fresh_install()
    }
}

// Fresh installs have no legacy key or sections to migrate. The marker and
// candidate still go through the same interrupted-selection protocol.
fn prepare_fresh_generation(scope: StoreScope) -> Result<Vec<u8>, String> {
    seal_generation(&Generation { scope, f6_initialized: false, sections: BTreeMap::new(),
        source_sections: BTreeMap::new(), source_key_digest: source_digest(b"wincommander-fresh-install-v1") })
}
fn verify_fresh_generation(root: &Path, scope: StoreScope, candidate: &[u8]) -> Result<(), String> {
    super::settings_generation_bootstrap::ensure_fresh_contents(root)?;
    let generation = open_generation(candidate).map_err(|error| scope.access_error(error))?;
    if generation.scope != scope || generation.f6_initialized || !generation.sections.is_empty()
        || !generation.source_sections.is_empty() || generation.source_key_digest != source_digest(b"wincommander-fresh-install-v1") {
        return Err("fresh settings candidate does not match an empty source".into());
    }
    Ok(())
}
fn select_fresh_generation(
    guard: &super::settings_generation_ownership::SettingsGenerationLock,
    root: &Path, scope: StoreScope, verify_maintenance: &impl Fn() -> Result<(), String>,
) -> Result<(), String> {
    guard.require_exclusive().map_err(|e| e.to_string())?;
    verify_maintenance()?;
    let anchor = ProtectedFileAnchor::new(root, scope);
    if matches!(anchor.read()?, Some(AnchorState::Active { .. })) {
        return cutover::read_active(guard, &anchor, &SelectedFile::new(root), &|bytes| verify_scoped_generation(scope, bytes)).map(|_| ());
    }
    stage_and_select(guard, &anchor, root, || prepare_fresh_generation(scope),
        &|bytes| { verify_maintenance()?; verify_fresh_generation(root, scope, bytes) })
}

#[cfg(windows)]
pub(super) fn pin_root_for_acl(root: &Path) -> Result<Vec<std::fs::File>, String> {
    source_snapshot::pin_directories(root)
}

/// Create or verify a fresh install's data root while holding both the known
/// and configured parent paths pinned. Its DACL is verified before any store
/// operation. Fresh-install proof and old-process fencing remain external.
#[cfg(windows)]
fn prepare_fresh_root() -> Result<(PathBuf, Vec<std::fs::File>, StoreScope), String> {
    let scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    let root = source_snapshot::known_root(scope)?;
    let mut pins = source_snapshot::pin_matching_parents(&root, scope)?;
    pins.extend(super::settings_generation_acl_windows::create_or_verify_root(&root, scope == StoreScope::CurrentUser)
        .map_err(|_| "cannot create or verify authorized settings root")?);
    Ok((root, pins, scope))
}

/// Explicit installer-lab entry for a verified fresh install only. Never
/// invoked automatically; the caller must establish fresh-install authority
/// and prevent old executable launches before calling it.
#[cfg(windows)]
pub(super) fn provision_verified_fresh_install() -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let (root, _pins, scope) = prepare_fresh_root()?;
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    super::settings_generation_bootstrap::provision_fresh(&root, scope, |path| {
        match scope {
            StoreScope::Machine => super::settings_generation_acl_windows::provision_or_verify(path),
            StoreScope::CurrentUser => super::settings_generation_acl_windows::provision_user_or_verify(path),
        }.map_err(|_| "cannot provision authorized settings lock".into())
    })?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    select_fresh_generation(&owner._ownership, &root, scope, &|| super::settings_generation_maintenance::verify_known_writers_stopped())
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
    let input = read_generation_file(path, MAX_TOTAL * 2 + 65_536)?.ok_or("selected settings generation unavailable")?;
    let candidate = updated_generation(&input, section, value)?;
    persist(path, &candidate)
}

fn updated_generation(input: &[u8], section: &str, value: Value) -> Result<Vec<u8>, String> {
    let mut generation = open_generation(input)?;
    if section.is_empty() || section.len() > 128 { return Err("invalid settings section".into()); }
    validate_section_name(section)?;
    let existing = matching_section(&generation.sections, section).map(str::to_owned);
    if existing.is_none() && generation.sections.len() >= MAX_SECTIONS { return Err("excessive settings sections".into()); }
    if section.eq_ignore_ascii_case("f6-keystore") {
        if !valid_f6(&value) { return Err("device signing key is empty or invalid; existing key preserved".into()); }
        if let Some(name) = existing.as_ref() {
            let old_seed = generation.sections[name]["signing_seed_hex"].as_str().ok_or("device signing key is invalid")?;
            let new_seed = value["signing_seed_hex"].as_str().ok_or("device signing key is invalid")?;
            if !old_seed.eq_ignore_ascii_case(new_seed) { return Err("replacing an existing device signing key is not allowed".into()); }
        }
        generation.f6_initialized = true;
    }
    generation.sections.insert(existing.unwrap_or_else(|| section.to_owned()), value);
    let candidate = seal_generation(&generation)?;
    let reopened = open_generation(&candidate)?;
    if reopened.sections != generation.sections { return Err("settings generation readback mismatch".into()); }
    Ok(candidate)
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
