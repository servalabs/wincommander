// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static PENDING: Mutex<Option<Source>> = Mutex::new(None);
const MAX_LEGACY_BYTES: u64 = 16 * 1024 * 1024;
const JOURNAL_FILENAME: &str = "legacy-settings-migration.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Source {
    #[serde(skip)]
    path: PathBuf,
    digest: [u8; 32],
    identity: (u32, u64),
    expected: Option<([u8; 32], [u8; 32])>,
}

fn open_source(path: &Path, delete: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::GENERIC_READ;
        use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_FLAG_OPEN_REPARSE_POINT};
        options
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        if delete {
            options.access_mode(GENERIC_READ | DELETE);
        }
    }
    #[cfg(not(windows))]
    if delete {
        return Err("Verified legacy cleanup is unavailable on this platform".into());
    }
    let file = options
        .open(path)
        .map_err(|_| "Could not open legacy settings")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Could not inspect legacy settings")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Legacy settings cannot be redirected".into());
        }
    }
    if !metadata.is_file() || metadata.len() > MAX_LEGACY_BYTES {
        return Err("Legacy settings are not a bounded regular file".into());
    }
    Ok(file)
}

#[cfg(windows)]
fn identity(file: &File) -> Result<(u32, u64), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err("Could not identify legacy settings".into());
    }
    Ok((
        information.dwVolumeSerialNumber,
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ))
}

#[cfg(not(windows))]
fn identity(_: &File) -> Result<(u32, u64), String> {
    Ok((0, 0))
}

fn read_source(file: &mut File) -> Result<String, String> {
    let mut text = String::new();
    file.take(MAX_LEGACY_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "Could not read legacy settings")?;
    if text.len() as u64 > MAX_LEGACY_BYTES {
        return Err("Legacy settings exceed the size limit".into());
    }
    Ok(text)
}

pub(super) fn read(path: &Path) -> Result<(String, Source), String> {
    let mut file = open_source(path, false)?;
    let identity = identity(&file)?;
    let text = read_source(&mut file)?;
    let source = Source {
        path: path.to_path_buf(),
        digest: Sha256::digest(text.as_bytes()).into(),
        identity,
        expected: None,
    };
    Ok((text, source))
}

pub(super) fn remember(source: Source) -> Result<(), String> {
    *PENDING.lock().map_err(|_| "Legacy migration lock failed")? = Some(source);
    Ok(())
}

fn clear_pending(pending: &mut Option<Source>) {
    *pending = None;
}

pub(super) fn clear() -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(|_| "Legacy migration lock failed")?;
    clear_pending(&mut pending);
    Ok(())
}

fn fingerprints(machine: &Value, user: &Value) -> Result<([u8; 32], [u8; 32]), String> {
    fn fingerprint(value: &Value, metadata: &str) -> Result<[u8; 32], String> {
        let mut value = value.clone();
        if let Some(object) = value.as_object_mut() {
            object.remove(metadata);
        }
        let encoded =
            serde_json::to_vec(&value).map_err(|_| "Could not fingerprint migrated settings")?;
        Ok(Sha256::digest(encoded).into())
    }
    Ok((
        fingerprint(machine, "appVersion")?,
        fingerprint(user, "lastSeenAt")?,
    ))
}

fn journal_path() -> Result<PathBuf, String> {
    Ok(crate::paths::user_data_dir()?.join(JOURNAL_FILENAME))
}

fn prepare_journal(
    pending: &mut Option<Source>,
    path: &Path,
    machine: &Value,
    user: &Value,
) -> Result<(), String> {
    if let Some(source) = pending {
        source.expected = Some(fingerprints(machine, user)?);
        let encoded =
            serde_json::to_vec(source).map_err(|_| "Could not encode migration journal")?;
        crate::datastore_io::atomic_write(path, &encoded)?;
    }
    Ok(())
}

pub(super) fn before_persistence(machine: &Value, user: &Value) -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(|_| "Legacy migration lock failed")?;
    if pending.is_some() {
        prepare_journal(&mut pending, &journal_path()?, machine, user)?;
    }
    Ok(())
}

fn resume_journal(path: &Path, legacy: &Path, machine: &Value, user: &Value) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Could not inspect migration journal".into()),
        Ok(metadata) if metadata.len() > 4096 => {
            return Err("Migration journal exceeds size limit".into())
        }
        Ok(_) => {}
    }
    let mut journal = open_source(path, false)?;
    let mut encoded = String::new();
    (&mut journal)
        .take(4097)
        .read_to_string(&mut encoded)
        .map_err(|_| "Could not read migration journal")?;
    if encoded.len() > 4096 {
        return Err("Migration journal exceeds size limit".into());
    }
    let mut source: Source =
        serde_json::from_str(&encoded).map_err(|_| "Migration journal is invalid")?;
    drop(journal);
    source.path = legacy.to_path_buf();
    if source.expected == Some(fingerprints(machine, user)?) {
        cleanup(&source)?;
        std::fs::remove_file(path).map_err(|_| "Could not remove completed migration journal")?;
    }
    Ok(())
}

pub(super) fn resume_committed(machine: &Value, user: &Value) {
    let result = journal_path().and_then(|path| {
        resume_journal(&path, &crate::paths::user_settings_path()?, machine, user)
    });
    if result.is_err() {
        crate::log_message(
            "warn",
            "[Settings] Legacy migration cleanup deferred; source preserved",
        );
    }
}

fn cleanup(source: &Source) -> Result<(), String> {
    let mut file = open_source(&source.path, true)?;
    if identity(&file)? != source.identity {
        return Err("Legacy settings identity changed; original preserved".into());
    }
    let digest: [u8; 32] = Sha256::digest(read_source(&mut file)?.as_bytes()).into();
    if digest != source.digest {
        return Err("Legacy settings changed; original preserved".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
        };
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                std::ptr::from_ref(&disposition).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err("Could not remove migrated legacy settings".into());
        }
    }
    Ok(())
}

fn complete(pending: &mut Option<Source>, committed: bool) -> Result<(), String> {
    if committed {
        if let Some(source) = pending.as_ref() {
            cleanup(source)?;
            *pending = None;
        }
    }
    Ok(())
}

pub(super) fn after_persistence(committed: bool, machine: &Value, user: &Value) {
    let result = PENDING
        .lock()
        .map_err(|_| "Legacy migration lock failed".to_string())
        .and_then(|mut pending| {
            let had_source = pending.is_some();
            complete(&mut pending, committed)?;
            if committed && had_source {
                match std::fs::remove_file(journal_path()?) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err("Could not remove completed migration journal".into())
                    }
                    _ => {}
                }
            }
            Ok(())
        });
    if result.is_err() {
        crate::log_message(
            "warn",
            "[Settings] Legacy plaintext cleanup deferred; source preserved",
        );
    }
    if committed {
        resume_committed(machine, user);
    }
}

#[cfg(all(test, windows))]
#[path = "settings_legacy_migration_tests.rs"]
mod tests;
