// SPDX-License-Identifier: AGPL-3.0-or-later
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

use super::{EvidenceEntry, MAX_ENTRIES};

const PREFIX: &str = "enc:v2:";
const SCOPE: &str = "machine:evidence-ledger";

#[derive(Serialize, Deserialize)]
struct KeyState {
    material: [u8; 32],
    pending_digest: Option<String>,
}
impl Drop for KeyState {
    fn drop(&mut self) {
        self.material.fill(0);
    }
}

fn digest(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

fn save_key(path: &Path, state: &KeyState, new: bool) -> Result<(), String> {
    let bytes =
        Zeroizing::new(serde_json::to_vec(state).map_err(|_| "Cannot encode evidence key")?);
    let protected = crate::datastore::dpapi_protect(&bytes)?;
    if new {
        crate::datastore_io::publish_new(path, &protected)?;
    } else {
        crate::datastore_io::atomic_write(path, &protected)?;
    }
    Ok(())
}

fn key(path: &Path, initial: Option<String>) -> Result<KeyState, String> {
    let mut protected = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && initial.is_some() => {
            let mut material = [0u8; 32];
            OsRng.fill_bytes(&mut material);
            let pending = initial.unwrap();
            save_key(
                path,
                &KeyState {
                    material,
                    pending_digest: Some(pending),
                },
                true,
            )?;
            material.fill(0);
            fs::read(path).map_err(|e| format!("Cannot read evidence key: {e}"))?
        }
        Err(error) => {
            return Err(format!(
                "Cannot read evidence key; ledger preserved: {error}"
            ))
        }
    };
    let material = Zeroizing::new(
        crate::datastore::dpapi_unprotect(&protected)
            .map_err(|_| "Cannot unlock evidence key; ledger preserved".to_string())?,
    );
    protected.fill(0);
    serde_json::from_slice(&material).map_err(|_| "Invalid evidence key; ledger preserved".into())
}

fn finish(path: &Path, state: &mut KeyState) -> Result<(), String> {
    if state.pending_digest.is_some() {
        state.pending_digest = None;
        save_key(path, state, false)?;
    }
    Ok(())
}

fn parse_legacy(raw: &str) -> Result<Vec<EvidenceEntry>, String> {
    let mut entries = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|_| "Evidence ledger is malformed; original preserved".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > MAX_ENTRIES {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    Ok(entries)
}

pub(super) fn write(
    path: &Path,
    key_path: &Path,
    entries: &[EvidenceEntry],
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let existing = match fs::read_to_string(path) {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Cannot read evidence ledger: {error}")),
    };
    let initial = match &existing {
        Some(raw) if raw.starts_with(PREFIX) => None,
        Some(raw) => Some(digest(raw)),
        None => Some(digest("")),
    };
    let mut key = key(key_path, initial)?;
    if let Some(raw) = existing.as_deref() {
        if raw.starts_with(PREFIX) {
            crate::datastore::decode_section(&key.material, raw.trim(), SCOPE)
                .map_err(|_| "Cannot authenticate evidence ledger; original preserved")?;
            finish(key_path, &mut key)?;
        } else if key.pending_digest.as_deref() != Some(digest(raw).as_str()) {
            return Err("Unexpected plaintext evidence; original preserved".into());
        }
    } else if key.pending_digest.as_deref() != Some(digest("").as_str()) {
        return Err("Evidence ledger is missing; existing key preserved".into());
    }
    let bytes = serde_json::to_vec(entries).map_err(|e| format!("Cannot encode evidence: {e}"))?;
    let sealed = crate::datastore::encode_section(&key.material, &bytes, SCOPE)?;
    persist(path, sealed.as_bytes())?;
    finish(key_path, &mut key)
}

pub(super) fn read(
    path: &Path,
    legacy: &impl Fn() -> Result<PathBuf, String>,
    key_path: &Path,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<Vec<EvidenceEntry>, String> {
    let (raw, imported) = match fs::read_to_string(path) {
        Ok(raw) => (raw, None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let old = legacy()?;
            match fs::read_to_string(&old) {
                Ok(raw) => (raw, Some(old)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match fs::symlink_metadata(key_path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            return Ok(Vec::new())
                        }
                        Err(error) => return Err(format!("Cannot inspect evidence key: {error}")),
                        Ok(_) => {
                            let state = key(key_path, None)?;
                            if state.pending_digest.as_deref() == Some(digest("").as_str()) {
                                return Ok(Vec::new());
                            }
                            return Err("Evidence ledger is missing; existing key preserved".into());
                        }
                    }
                }
                Err(error) => return Err(format!("Cannot read old evidence ledger: {error}")),
            }
        }
        Err(error) => return Err(format!("Cannot read evidence ledger: {error}")),
    };
    if raw.starts_with(PREFIX) {
        if imported.is_some() {
            return Err("Unexpected encrypted legacy evidence; original preserved".into());
        }
        let mut key = key(key_path, None)?;
        let bytes = crate::datastore::decode_section(&key.material, raw.trim(), SCOPE)
            .map_err(|_| "Cannot authenticate evidence ledger; original preserved".to_string())?;
        let entries = serde_json::from_slice(&bytes)
            .map_err(|_| "Evidence ledger is malformed; original preserved")?;
        finish(key_path, &mut key)?;
        return Ok(entries);
    }
    let entries = parse_legacy(&raw)?;
    // A profile-owned source stays in place until ownership-safe cleanup is available.
    let fingerprint = digest(&raw);
    let mut state = key(key_path, Some(fingerprint.clone()))?;
    if state.pending_digest.as_deref() != Some(fingerprint.as_str()) {
        return Err("Unexpected plaintext evidence; original preserved".into());
    }
    let bytes = serde_json::to_vec(&entries).map_err(|_| "Cannot encode evidence")?;
    let sealed = crate::datastore::encode_section(&state.material, &bytes, SCOPE)?;
    persist(path, sealed.as_bytes())?;
    finish(key_path, &mut state)?;
    Ok(entries)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn entry() -> EvidenceEntry {
        EvidenceEntry {
            time: "2026-01-01T00:00:00Z".into(),
            source: "monitor".into(),
            severity: "warn".into(),
            summary: "private summary".into(),
            detail: Some("private detail".into()),
        }
    }

    #[test]
    fn existing_plaintext_is_migrated_and_reopened_without_exposing_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let legacy = dir.path().join("old");
        let key_path = dir.path().join("key");
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&entry()).unwrap()),
        )
        .unwrap();
        let entries = read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(entries[0].detail.as_deref(), Some("private detail"));
        let disk = fs::read_to_string(&path).unwrap();
        assert!(disk.starts_with(PREFIX));
        assert!(!disk.contains("private summary") && !disk.contains("private detail"));
        let reopened = read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(reopened[0].summary, "private summary");
    }

    #[test]
    fn failed_migration_preserves_old_ledger_and_authentication_failure_does_not_clear_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let legacy = dir.path().join("old");
        let key_path = dir.path().join("key");
        let original = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&legacy, &original).unwrap();
        assert!(read(&path, &|| Ok(legacy.clone()), &key_path, &|_, _| Err(
            "injected failure".into()
        ))
        .is_err());
        assert_eq!(fs::read_to_string(&legacy).unwrap(), original);
        assert!(!path.exists());
        read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert!(legacy.exists());
        let mut bytes = fs::read(&path).unwrap();
        let i = bytes.len() - 2;
        bytes[i] = if bytes[i] == b'A' { b'B' } else { b'A' };
        fs::write(&path, &bytes).unwrap();
        assert!(read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn missing_key_never_replaced_while_protected_ledger_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let legacy = dir.path().join("old");
        let key_path = dir.path().join("key");
        write(
            &path,
            &key_path,
            &[entry()],
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let protected = fs::read(&path).unwrap();
        fs::remove_file(&key_path).unwrap();
        assert!(read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .is_err());
        assert!(write(&path, &key_path, &[], &crate::datastore_io::atomic_write).is_err());
        assert!(!key_path.exists());
        assert_eq!(fs::read(&path).unwrap(), protected);
    }

    #[test]
    fn migration_resume_requires_exact_original_and_commits_before_accepting_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let legacy = dir.path().join("old");
        let key_path = dir.path().join("key");
        let original = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&path, &original).unwrap();
        assert!(read(&path, &|| Ok(legacy.clone()), &key_path, &|_, _| Err(
            "failed".into()
        ))
        .is_err());
        fs::write(&path, "{}\n").unwrap();
        assert!(read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .is_err());
        fs::write(&path, &original).unwrap();
        let state = key(&key_path, None).unwrap();
        let bytes = serde_json::to_vec(&vec![entry()]).unwrap();
        let sealed = crate::datastore::encode_section(&state.material, &bytes, SCOPE).unwrap();
        fs::write(&path, sealed).unwrap();
        read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        fs::write(&path, &original).unwrap();
        assert!(read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .is_err());
    }

    #[test]
    fn protected_history_is_not_treated_as_empty_when_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let legacy = dir.path().join("old");
        let key_path = dir.path().join("key");
        write(
            &path,
            &key_path,
            &[entry()],
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        fs::remove_file(&path).unwrap();
        assert!(read(
            &path,
            &|| Ok(legacy.clone()),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .is_err());
    }

    #[test]
    fn existing_machine_ledger_never_requires_a_user_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        write(
            &path,
            &key_path,
            &[entry()],
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let entries = read(
            &path,
            &|| Err("profile unavailable".into()),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(entries[0].summary, "private summary");
    }
}
