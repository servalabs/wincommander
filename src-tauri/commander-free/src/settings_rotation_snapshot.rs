// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only source snapshot. This does not provide a production cutover.
use super::{prepare_for, recover_stage, stage_new_with_hook, validate_section_name, verify_source_for, StoreScope, StagedFile, MAX_SECTIONS, MAX_TOTAL};
use crate::datastore::dpapi_unprotect;
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
use zeroize::Zeroizing;

const MAX_KEY: u64 = 65536;
const STAGED_NAME: &str = ".settings-generation.v1.staged";
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
    scope: StoreScope,
    _directories: Vec<File>,
    key_file: File,
    key_digest: [u8; 32],
    material: Zeroizing<[u8; 32]>,
    sections: Vec<Section>,
}

pub(super) fn pin_directories(path: &Path) -> Result<Vec<File>, String> {
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

// Both directory trees stay pinned by the returned handles. This detects a
// disagreement between the known-folder root and the legacy live resolver;
// it does not authorize ACLs or prevent an old process using another path.
fn live_base(scope: StoreScope) -> Result<PathBuf, String> {
    let value = match scope {
        StoreScope::Machine => std::env::var("ProgramData").or_else(|_| std::env::var("ALLUSERSPROFILE")),
        StoreScope::CurrentUser => std::env::var("LOCALAPPDATA"),
    }.map_err(|_| UNAVAILABLE)?;
    Ok(PathBuf::from(value))
}
fn pin_matching_paths(trusted: &Path, live: &Path) -> Result<Vec<File>, String> {
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    let mut trusted_pins = pin_directories(trusted)?;
    let live_pins = pin_directories(live)?;
    let mut first = BY_HANDLE_FILE_INFORMATION::default();
    let mut second = BY_HANDLE_FILE_INFORMATION::default();
    let a = trusted_pins.last().ok_or(UNAVAILABLE)?;
    let b = live_pins.last().ok_or(UNAVAILABLE)?;
    // SAFETY: pinned directory handles and both result buffers remain live.
    if unsafe { GetFileInformationByHandle(a.as_raw_handle(), &mut first) } == 0
        || unsafe { GetFileInformationByHandle(b.as_raw_handle(), &mut second) } == 0
        || first.dwVolumeSerialNumber != second.dwVolumeSerialNumber
        || first.nFileIndexHigh != second.nFileIndexHigh
        || first.nFileIndexLow != second.nFileIndexLow {
        return Err(UNAVAILABLE.into());
    }
    trusted_pins.extend(live_pins);
    Ok(trusted_pins)
}
/// Pin matching product roots once they exist.
pub(super) fn pin_matching_roots(trusted: &Path, scope: StoreScope) -> Result<Vec<File>, String> {
    pin_matching_paths(trusted, &live_base(scope)?.join(crate::paths::APP_DIR_NAME))
}
/// Pin matching known-folder parents before a new product root is created.
pub(super) fn pin_matching_parents(root: &Path, scope: StoreScope) -> Result<Vec<File>, String> {
    pin_matching_paths(root.parent().ok_or(UNAVAILABLE)?, &live_base(scope)?)
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

/// Pin the expected user installation's binary throughout staging. The root
/// itself is independently checked against the current user's known folder.
pub(super) fn pin_user_installed_binary(root: &Path) -> Result<File, String> {
    open_locked(&root.join("wincommander-free.exe"), 1024 * 1024 * 1024)
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

// Resolve the machine root independently of process environment variables. This
// does not establish directory ACL ownership or authorize activation.
pub(super) fn known_root(scope: StoreScope) -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::{FOLDERID_LocalAppData, FOLDERID_ProgramData, SHGetKnownFolderPath}};
    let app = Path::new(crate::paths::APP_DIR_NAME);
    let mut components = app.components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_))) || components.next().is_some() {
        return Err(UNAVAILABLE.into());
    }
    let mut raw = std::ptr::null_mut();
    // SAFETY: SHGetKnownFolderPath initializes an allocated null-terminated path.
    let folder = match scope { StoreScope::Machine => &FOLDERID_ProgramData, StoreScope::CurrentUser => &FOLDERID_LocalAppData };
    let result = unsafe { SHGetKnownFolderPath(folder, 0, std::ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        if !raw.is_null() { unsafe { CoTaskMemFree(raw.cast()) }; }
        return Err(UNAVAILABLE.into());
    }
    let mut len = 0usize;
    // SAFETY: Windows returned a null-terminated allocation, freed below.
    let base = unsafe {
        while *raw.add(len) != 0 { len += 1; }
        let value = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw.cast());
        PathBuf::from(value)
    };
    if !base.is_absolute() { return Err(UNAVAILABLE.into()); }
    Ok(base.join(app))
}

include!("settings_rotation_snapshot_impl.rs");

#[cfg(test)]
#[path = "settings_rotation_snapshot_tests.rs"]
mod tests;
