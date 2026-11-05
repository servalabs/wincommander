// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only source snapshot. This does not provide a production cutover.
use super::{prepare, stage_new_with_hook, validate_section_name, verify_source, MAX_SECTIONS, MAX_TOTAL};
use crate::datastore::dpapi_unprotect;
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
use zeroize::Zeroizing;

const MAX_KEY: u64 = 65536;
const UNAVAILABLE: &str = "settings source unavailable or changed; originals preserved";

struct Section {
    name: String,
    file: File,
    digest: [u8; 32],
    encoded: String,
}

/// Retains every source handle until the caller drops the prepared snapshot.
/// It rejects writes to held files, but does not guarantee that another process
/// cannot create a new file or write after this guard is released.
pub(super) struct Snapshot {
    store: PathBuf,
    _directories: Vec<File>,
    key_file: File,
    key_digest: [u8; 32],
    material: Zeroizing<[u8; 32]>,
    sections: Vec<Section>,
}

fn pin_directories(path: &Path) -> Result<Vec<File>, String> {
    if !path.is_absolute() { return Err(UNAVAILABLE.into()); }
    let mut ancestors: Vec<_> = path.ancestors().collect();
    ancestors.reverse();
    let mut pins = Vec::with_capacity(ancestors.len());
    for ancestor in ancestors {
        let file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(ancestor).map_err(|_| UNAVAILABLE)?;
        let metadata = file.metadata().map_err(|_| UNAVAILABLE)?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(UNAVAILABLE.into());
        }
        pins.push(file);
    }
    Ok(pins)
}

fn open_locked(path: &Path, limit: u64) -> Result<File, String> {
    let file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path).map_err(|_| UNAVAILABLE)?;
    let metadata = file.metadata().map_err(|_| UNAVAILABLE)?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || metadata.len() > limit {
        return Err(UNAVAILABLE.into());
    }
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the file and output buffer stay live for this API call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.nNumberOfLinks != 1 {
        return Err(UNAVAILABLE.into());
    }
    Ok(file)
}

fn read_locked(file: &mut File, limit: u64) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(0)).map_err(|_| UNAVAILABLE)?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|_| UNAVAILABLE)?;
    if bytes.len() as u64 > limit { return Err(UNAVAILABLE.into()); }
    Ok(bytes)
}

fn enumerate(store: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut found = Vec::new();
    let mut identities = std::collections::HashSet::new();
    for entry in fs::read_dir(store).map_err(|_| UNAVAILABLE)? {
        let entry = entry.map_err(|_| UNAVAILABLE)?;
        let path = entry.path();
        // Rust's Path::extension treats `.dat` as a stem with no extension,
        // but the existing section validator can create this empty-name file.
        if entry.file_name().to_str().is_some_and(|name| name.eq_ignore_ascii_case(".dat")) {
            return Err(UNAVAILABLE.into());
        }
        if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("dat")) { continue; }
        let name = path.file_stem().and_then(|s| s.to_str()).ok_or(UNAVAILABLE)?.to_owned();
        if name.is_empty() || name.len() > 128 || validate_section_name(&name).is_err()
            || !identities.insert(name.to_lowercase()) { return Err(UNAVAILABLE.into()); }
        found.push((name, path));
        if found.len() > MAX_SECTIONS { return Err(UNAVAILABLE.into()); }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

impl Snapshot {
    /// Machine-only entry point. The inherited machine-data resolver uses the
    /// process environment; trustworthy root authorization remains required
    /// before any production integration.
    pub(super) fn capture_machine() -> Result<Self, String> {
        if crate::paths::current_user_install_uses_local_datastore() {
            return Err("machine settings rotation is unavailable for a current-user install".into());
        }
        let root = crate::paths::machine_data_dir().map_err(|_| UNAVAILABLE)?;
        Self::capture(&root)
    }

    /// Test fixture / internal seam. Caller must independently authorize root.
    /// The key and every section are read once while write/delete sharing is denied.
    fn capture(root: &Path) -> Result<Self, String> {
        let store = root.join("store");
        let pins = pin_directories(&store)?;
        let mut key_file = open_locked(&root.join(".install.material"), MAX_KEY)?;
        let raw = Zeroizing::new(read_locked(&mut key_file, MAX_KEY)?);
        let key_digest = Sha256::digest(&raw[..]).into();
        let plain = if raw.len() == 32 { Zeroizing::new(raw.to_vec()) }
            else { Zeroizing::new(dpapi_unprotect(&raw).map_err(|_| UNAVAILABLE)?) };
        let mut material = Zeroizing::new([0u8; 32]);
        if plain.len() != material.len() { return Err(UNAVAILABLE.into()); }
        material.copy_from_slice(&plain);
        let mut sections = Vec::new();
        let mut total = 0usize;
        for (name, path) in enumerate(&store)? {
            let mut file = open_locked(&path, MAX_TOTAL as u64)?;
            let bytes = read_locked(&mut file, MAX_TOTAL as u64)?;
            total = total.checked_add(bytes.len()).ok_or(UNAVAILABLE)?;
            if total > MAX_TOTAL { return Err(UNAVAILABLE.into()); }
            let digest = Sha256::digest(&bytes).into();
            let encoded = String::from_utf8(bytes).map_err(|_| UNAVAILABLE)?;
            sections.push(Section { name, file, digest, encoded });
        }
        let mut snapshot = Self { store, _directories: pins, key_file, key_digest, material, sections };
        snapshot.verify_unchanged()?;
        Ok(snapshot)
    }

    pub(super) fn verify_unchanged(&mut self) -> Result<(), String> {
        let raw_key = Zeroizing::new(read_locked(&mut self.key_file, MAX_KEY)?);
        let observed_key: [u8; 32] = Sha256::digest(&raw_key[..]).into();
        if observed_key != self.key_digest { return Err(UNAVAILABLE.into()); }
        let current = enumerate(&self.store)?;
        if current.len() != self.sections.len()
            || current.iter().zip(&self.sections).any(|((name, _), section)| name != &section.name) {
            return Err(UNAVAILABLE.into());
        }
        for section in &mut self.sections {
            let bytes = read_locked(&mut section.file, MAX_TOTAL as u64)?;
            let observed: [u8; 32] = Sha256::digest(&bytes).into();
            if observed != section.digest { return Err(UNAVAILABLE.into()); }
        }
        Ok(())
    }

    /// Build the complete new generation while the guard remains alive.
    /// A second verification narrows source-change races, but is not a
    /// substitute for deployment-wide fencing or crash-safe activation.
    pub(super) fn prepare(&mut self) -> Result<Vec<u8>, String> {
        self.verify_unchanged()?;
        let sections: Vec<_> = self.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        let output = prepare(&self.material, &sections)?;
        self.verify_unchanged()?;
        Ok(output)
    }

    /// Compare a reopened generation with a freshly guarded, complete source
    /// snapshot. This detects observed changes, not writers racing afterward.
    pub(super) fn verify_generation(&mut self, generation: &[u8]) -> Result<(), String> {
        self.verify_unchanged()?;
        let sections: Vec<_> = self.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        verify_source(generation, &self.material, &sections)?;
        self.verify_unchanged()
    }

    /// Stage at a new path and independently verify its contents while all
    /// source guards stay alive. It does not activate or fence old writers.
    pub(super) fn stage_unactivated(&mut self, path: &Path) -> Result<(), String> {
        self.stage_unactivated_with_hook(path, &|_, _| Ok(()))
    }

    fn stage_unactivated_with_hook(
        &mut self,
        path: &Path,
        after_write: &impl Fn(&Path, &mut File) -> Result<(), String>,
    ) -> Result<(), String> {
        let output = self.prepare()?;
        self.verify_generation(&output)?;
        let sections: Vec<_> = self.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        stage_new_with_hook(path, &output, &self.material, &sections, after_write)?;
        self.verify_generation(&output)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "settings_rotation_snapshot_tests.rs"]
mod tests;
