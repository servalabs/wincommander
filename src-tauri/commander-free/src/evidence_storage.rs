// SPDX-License-Identifier: AGPL-3.0-or-later
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

use super::{EvidenceEntry, MAX_ENTRIES};

const PREFIX: &str = "enc:v2:";
const SCOPE: &str = "machine:evidence-ledger";
const MAX_IMPORTED_PROFILES: usize = 256;

#[derive(Serialize, Deserialize)]
struct KeyState {
    material: [u8; 32],
    pending_digest: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Ledger {
    pub entries: Vec<EvidenceEntry>,
    imported_profiles: Vec<PathBuf>,
    pub skip_future_imports: bool,
}
impl Drop for KeyState {
    fn drop(&mut self) {
        self.material.zeroize();
    }
}

fn digest(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

fn save_key(
    path: &Path,
    state: &KeyState,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let bytes =
        Zeroizing::new(serde_json::to_vec(state).map_err(|_| "Cannot encode evidence key")?);
    let protected = crate::datastore::dpapi_protect(&bytes)?;
    persist(path, &protected)
}

fn key(
    path: &Path,
    initial: Option<String>,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<KeyState, String> {
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
                persist,
            )?;
            material.zeroize();
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

fn finish(
    path: &Path,
    state: &mut KeyState,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if state.pending_digest.is_some() {
        state.pending_digest = None;
        save_key(path, state, persist)?;
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
    ledger: &Ledger,
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
    let mut key = key(key_path, initial, persist)?;
    if let Some(raw) = existing.as_deref() {
        if raw.starts_with(PREFIX) {
            crate::datastore::decode_section(&key.material, raw.trim(), SCOPE)
                .map_err(|_| "Cannot authenticate evidence ledger; original preserved")?;
            finish(key_path, &mut key, persist)?;
        } else if key.pending_digest.as_deref() != Some(digest(raw).as_str()) {
            return Err("Unexpected plaintext evidence; original preserved".into());
        }
    } else if key.pending_digest.as_deref() != Some(digest("").as_str()) {
        return Err("Evidence ledger is missing; existing key preserved".into());
    }
    let bytes = serde_json::to_vec(ledger).map_err(|e| format!("Cannot encode evidence: {e}"))?;
    let sealed = crate::datastore::encode_section(&key.material, &bytes, SCOPE)?;
    persist(path, sealed.as_bytes())?;
    finish(key_path, &mut key, persist)
}

fn empty_or_missing(
    key_path: &Path,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<Ledger, String> {
    match fs::symlink_metadata(key_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
        Err(error) => Err(format!("Cannot inspect evidence key: {error}")),
        Ok(_) => {
            let state = key(key_path, None, persist)?;
            if state.pending_digest.as_deref() == Some(digest("").as_str()) {
                Ok(Ledger::default())
            } else {
                Err("Evidence ledger is missing; existing key preserved".into())
            }
        }
    }
}

pub(super) fn read(
    path: &Path,
    legacy: &impl Fn() -> Result<Option<PathBuf>, String>,
    key_path: &Path,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<Ledger, String> {
    let mut ledger = match fs::read_to_string(path) {
        Ok(raw) if raw.starts_with(PREFIX) => {
            let mut key = key(key_path, None, persist)?;
            let bytes = crate::datastore::decode_section(&key.material, raw.trim(), SCOPE)
                .map_err(|_| {
                    "Cannot authenticate evidence ledger; original preserved".to_string()
                })?;
            let value = serde_json::from_slice(&bytes)
                .map_err(|_| "Evidence ledger is malformed; original preserved")?;
            finish(key_path, &mut key, persist)?;
            value
        }
        Ok(raw) => {
            let entries = parse_legacy(&raw)?;
            let fingerprint = digest(&raw);
            let mut state = key(key_path, Some(fingerprint.clone()), persist)?;
            if state.pending_digest.as_deref() != Some(fingerprint.as_str()) {
                return Err("Unexpected plaintext evidence; original preserved".into());
            }
            let value = Ledger {
                entries,
                imported_profiles: Vec::new(),
                skip_future_imports: false,
            };
            let bytes = serde_json::to_vec(&value).map_err(|_| "Cannot encode evidence")?;
            let sealed = crate::datastore::encode_section(&state.material, &bytes, SCOPE)?;
            persist(path, sealed.as_bytes())?;
            finish(key_path, &mut state, persist)?;
            value
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            empty_or_missing(key_path, persist)?
        }
        Err(error) => return Err(format!("Cannot read evidence ledger: {error}")),
    };
    if ledger.skip_future_imports {
        return Ok(ledger);
    }
    let candidate = (|| -> Result<Option<(PathBuf, Vec<EvidenceEntry>)>, String> {
        let Some(old) = legacy()? else {
            return Ok(None);
        };
        let identity = match fs::canonicalize(&old) {
            Ok(identity) => identity,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Cannot identify old evidence ledger".into()),
        };
        if ledger.imported_profiles.contains(&identity) {
            return Ok(None);
        }
        if ledger.imported_profiles.len() >= MAX_IMPORTED_PROFILES {
            return Err("Too many evidence profile imports".into());
        }
        let raw = fs::read_to_string(&identity).map_err(|_| "Cannot read old evidence ledger")?;
        let prior = parse_legacy(&raw)?;
        let mut existing = std::collections::HashSet::<Vec<u8>>::new();
        for entry in &ledger.entries {
            existing.insert(serde_json::to_vec(entry).map_err(|_| "Cannot compare evidence")?);
        }
        for entry in &prior {
            if existing.contains(&serde_json::to_vec(entry).map_err(|_| "Cannot compare evidence")?)
            {
                return Err("Ambiguous evidence records require review".into());
            }
        }
        Ok(Some((identity, prior)))
    })();
    let candidate = match candidate {
        Ok(value) => value,
        Err(_) => {
            #[cfg(not(test))]
            crate::log::log_message(
                "warn",
                "Legacy evidence import is pending; original preserved",
            );
            None
        }
    };
    if let Some((identity, mut prior)) = candidate {
        prior.append(&mut ledger.entries);
        prior.sort_by(|a, b| a.time.cmp(&b.time));
        if prior.len() > MAX_ENTRIES {
            prior.drain(..prior.len() - MAX_ENTRIES);
        }
        ledger.entries = prior;
        ledger.imported_profiles.push(identity);
        if let Err(error) = write(path, key_path, &ledger, persist) {
            let recovered = (|| -> Result<Ledger, String> {
                let encoded =
                    fs::read_to_string(path).map_err(|_| "Cannot reread evidence ledger")?;
                let state = key(key_path, None, persist)?;
                if state.pending_digest.is_some() {
                    return Err("Evidence commit is incomplete".into());
                }
                let bytes =
                    crate::datastore::decode_section(&state.material, encoded.trim(), SCOPE)
                        .map_err(|_| "Cannot authenticate evidence ledger")?;
                serde_json::from_slice(&bytes).map_err(|_| "Evidence ledger is malformed".into())
            })();
            if let Ok(actual) = recovered {
                #[cfg(not(test))]
                crate::log::log_message(
                    "warn",
                    "Legacy evidence import could not be published; retry pending",
                );
                return Ok(actual);
            }
            return Err(error);
        }
    }
    Ok(ledger)
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
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(entries.entries[0].detail.as_deref(), Some("private detail"));
        let disk = fs::read_to_string(&path).unwrap();
        assert!(disk.starts_with(PREFIX));
        assert!(!disk.contains("private summary") && !disk.contains("private detail"));
        let reopened = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(reopened.entries[0].summary, "private summary");
    }

    #[test]
    fn failed_migration_preserves_old_ledger_and_authentication_failure_does_not_clear_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let legacy = dir.path().join("old");
        let key_path = dir.path().join("key");
        let original = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&legacy, &original).unwrap();
        assert!(
            read(&path, &|| Ok(Some(legacy.clone())), &key_path, &|_, _| Err(
                "injected failure".into()
            ))
            .is_err()
        );
        assert_eq!(fs::read_to_string(&legacy).unwrap(), original);
        assert!(!path.exists());
        read(
            &path,
            &|| Ok(Some(legacy.clone())),
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
            &|| Ok(Some(legacy.clone())),
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
            &Ledger {
                entries: vec![entry()],
                imported_profiles: Vec::new(),
                skip_future_imports: false,
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let protected = fs::read(&path).unwrap();
        fs::remove_file(&key_path).unwrap();
        assert!(read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .is_err());
        assert!(write(
            &path,
            &key_path,
            &Ledger::default(),
            &crate::datastore_io::atomic_write
        )
        .is_err());
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
        assert!(read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &|target, bytes| {
                if target == path {
                    Err("failed".into())
                } else {
                    crate::datastore_io::atomic_write(target, bytes)
                }
            },
        )
        .is_err());
        fs::write(&path, "{}\n").unwrap();
        assert!(read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .is_err());
        fs::write(&path, &original).unwrap();
        let state = key(&key_path, None, &crate::datastore_io::atomic_write).unwrap();
        let bytes = serde_json::to_vec(&Ledger {
            entries: vec![entry()],
            imported_profiles: Vec::new(),
            skip_future_imports: false,
        })
        .unwrap();
        let sealed = crate::datastore::encode_section(&state.material, &bytes, SCOPE).unwrap();
        fs::write(&path, sealed).unwrap();
        read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        fs::write(&path, &original).unwrap();
        assert!(read(
            &path,
            &|| Ok(Some(legacy.clone())),
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
            &Ledger {
                entries: vec![entry()],
                imported_profiles: Vec::new(),
                skip_future_imports: false,
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        fs::remove_file(&path).unwrap();
        assert!(read(
            &path,
            &|| Ok(Some(legacy.clone())),
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
            &Ledger {
                entries: vec![entry()],
                imported_profiles: Vec::new(),
                skip_future_imports: false,
            },
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
        assert_eq!(entries.entries[0].summary, "private summary");
    }

    #[test]
    fn no_profile_first_run_creates_and_reads_machine_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        assert!(read(
            &path,
            &|| Ok(None),
            &key_path,
            &crate::datastore_io::atomic_write
        )
        .unwrap()
        .entries
        .is_empty());
        write(
            &path,
            &key_path,
            &Ledger {
                entries: vec![entry()],
                imported_profiles: Vec::new(),
                skip_future_imports: false,
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(
            read(
                &path,
                &|| Ok(None),
                &key_path,
                &crate::datastore_io::atomic_write
            )
            .unwrap()
            .entries[0]
                .summary,
            "private summary"
        );
    }

    #[test]
    fn later_profile_import_is_atomic_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        let legacy = dir.path().join("profile-ledger");
        let mut current = entry();
        current.time = "2026-02-01T00:00:00Z".into();
        current.summary = "current".into();
        write(
            &path,
            &key_path,
            &Ledger {
                entries: vec![current],
                imported_profiles: Vec::new(),
                skip_future_imports: false,
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let before = fs::read(&path).unwrap();
        fs::write(
            &legacy,
            format!("{}\n", serde_json::to_string(&entry()).unwrap()),
        )
        .unwrap();
        let deferred = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &|target, bytes| {
                if target == path {
                    Err("injected failure".into())
                } else {
                    crate::datastore_io::atomic_write(target, bytes)
                }
            },
        )
        .unwrap();
        assert_eq!(deferred.entries.len(), 1);
        assert_eq!(fs::read(&path).unwrap(), before);
        let loaded = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].summary, "private summary");
        assert_eq!(loaded.entries[1].summary, "current");
        let reloaded = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(reloaded.entries.len(), 2);
        assert!(legacy.exists());
    }

    #[test]
    fn initial_key_and_commit_use_the_same_ordered_publisher_as_the_ledger() {
        use std::sync::Mutex;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        let published = Mutex::new(Vec::new());
        write(
            &path,
            &key_path,
            &Ledger {
                entries: vec![entry()],
                imported_profiles: Vec::new(),
                skip_future_imports: false,
            },
            &|target, bytes| {
                published.lock().unwrap().push(target.to_path_buf());
                crate::datastore_io::atomic_write(target, bytes)
            },
        )
        .unwrap();
        assert_eq!(
            *published.lock().unwrap(),
            vec![key_path.clone(), path.clone(), key_path]
        );
    }

    #[test]
    fn old_profile_already_present_is_not_imported_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        let legacy = dir.path().join("profile-ledger");
        fs::write(
            &legacy,
            format!("{}\n", serde_json::to_string(&entry()).unwrap()),
        )
        .unwrap();
        write(
            &path,
            &key_path,
            &Ledger {
                entries: vec![entry()],
                ..Ledger::default()
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let ledger = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(ledger.entries.len(), 1);
        assert!(ledger.imported_profiles.is_empty());
    }

    #[test]
    fn cleared_ledger_never_resurrects_a_later_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        let legacy = dir.path().join("profile-ledger");
        fs::write(
            &legacy,
            format!("{}\n", serde_json::to_string(&entry()).unwrap()),
        )
        .unwrap();
        write(
            &path,
            &key_path,
            &Ledger {
                skip_future_imports: true,
                ..Ledger::default()
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let ledger = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert!(ledger.entries.is_empty());
        assert!(legacy.exists());
    }

    #[test]
    fn damaged_profile_does_not_hide_healthy_machine_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        let legacy = dir.path().join("profile-ledger");
        fs::write(&legacy, b"malformed old data").unwrap();
        write(
            &path,
            &key_path,
            &Ledger {
                entries: vec![entry()],
                ..Ledger::default()
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let ledger = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(ledger.entries.len(), 1);
        assert!(ledger.imported_profiles.is_empty());
        assert_eq!(fs::read(&legacy).unwrap(), b"malformed old data");
    }

    #[test]
    fn uncertain_import_outcome_is_reread_and_never_duplicated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        let legacy = dir.path().join("profile-ledger");
        let mut current = entry();
        current.summary = "current".into();
        write(
            &path,
            &key_path,
            &Ledger {
                entries: vec![current],
                ..Ledger::default()
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        fs::write(
            &legacy,
            format!("{}\n", serde_json::to_string(&entry()).unwrap()),
        )
        .unwrap();
        let actual = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &|target, bytes| {
                crate::datastore_io::atomic_write(target, bytes)?;
                if target == path {
                    Err("unknown outcome".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!(actual.entries.len(), 2);
        assert_eq!(actual.imported_profiles.len(), 1);
        assert_eq!(
            read(
                &path,
                &|| Ok(Some(legacy.clone())),
                &key_path,
                &crate::datastore_io::atomic_write
            )
            .unwrap()
            .entries
            .len(),
            2
        );
    }

    #[test]
    fn malformed_old_profile_does_not_block_fresh_machine_recording() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger");
        let key_path = dir.path().join("key");
        let legacy = dir.path().join("profile-ledger");
        fs::write(&legacy, b"malformed old data").unwrap();
        let mut fresh = read(
            &path,
            &|| Ok(Some(legacy.clone())),
            &key_path,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        fresh.entries.push(entry());
        write(&path, &key_path, &fresh, &crate::datastore_io::atomic_write).unwrap();
        assert_eq!(
            read(
                &path,
                &|| Ok(Some(legacy.clone())),
                &key_path,
                &crate::datastore_io::atomic_write
            )
            .unwrap()
            .entries
            .len(),
            1
        );
        assert_eq!(fs::read(legacy).unwrap(), b"malformed old data");
    }
}
