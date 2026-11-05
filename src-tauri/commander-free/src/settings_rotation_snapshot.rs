// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only source snapshot. This does not provide a production cutover.
use super::{prepare, validate_section_name, MAX_SECTIONS, MAX_TOTAL};
use crate::datastore::dpapi_unprotect;
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
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
    _key_file: File,
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
    /// `root` must be the machine datastore's already authorized directory.
    /// The key and every section are read once while write/delete sharing is denied.
    pub(super) fn capture(root: &Path) -> Result<Self, String> {
        let store = root.join("store");
        let pins = pin_directories(&store)?;
        let mut key_file = open_locked(&root.join(".install.material"), MAX_KEY)?;
        let raw = Zeroizing::new(read_locked(&mut key_file, MAX_KEY)?);
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
        let mut snapshot = Self { store, _directories: pins, _key_file: key_file, material, sections };
        snapshot.verify_unchanged()?;
        Ok(snapshot)
    }

    pub(super) fn verify_unchanged(&mut self) -> Result<(), String> {
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

    /// Stage only an unactivated candidate while all source guards stay alive.
    /// The callback must not overwrite an active generation. Even success is
    /// not cutover authorization: a later old writer can still change sources.
    /// On any error the staged destination may exist and needs inspection.
    pub(super) fn stage_unactivated(
        &mut self,
        stage: &impl Fn(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        let output = self.prepare()?;
        self.verify_unchanged()?;
        stage(&output)?;
        self.verify_unchanged()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "settings_rotation_snapshot_tests.rs"]
mod tests;
