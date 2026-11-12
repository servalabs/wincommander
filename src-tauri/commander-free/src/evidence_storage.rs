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
const MAX_MACHINE_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

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
    #[serde(default)]
    machine_source_digest: Option<String>,
    #[serde(default)]
    machine_source_length: Option<u64>,
    #[serde(default)]
    machine_source_records: Option<Vec<[u8; 32]>>,
    #[serde(default)]
    pub(super) history_gap_unverified: bool,
    #[serde(skip)]
    pub(super) profile_import_pending: bool,
    #[serde(default)]
    clear_pending: bool,
    #[serde(default)]
    clear_scope: Option<ClearScope>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ClearScope {
    user: String,
    machine: PathBuf,
    profile: Option<PathBuf>,
    machine_snapshot: Option<super::clear_io::FileStamp>,
    profile_snapshot: Option<super::clear_io::FileStamp>,
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

fn parse_machine_legacy(raw: &str) -> Result<Vec<EvidenceEntry>, String> {
    if raw.lines().filter(|line| !line.trim().is_empty()).count() > MAX_ENTRIES {
        return Err("Old machine evidence exceeds the retention limit; original preserved".into());
    }
    parse_legacy(raw)
}

fn entry_hashes(entries: &[EvidenceEntry]) -> Result<Vec<[u8; 32]>, String> {
    entries
        .iter()
        .map(|entry| {
            let bytes = serde_json::to_vec(entry).map_err(|_| "Cannot compare evidence")?;
            let mut hash = [0; 32];
            hash.copy_from_slice(&Sha256::digest(bytes));
            Ok(hash)
        })
        .collect()
}

fn unique_rollover(previous: &[[u8; 32]], current: &[[u8; 32]]) -> Result<usize, String> {
    if previous.len() != MAX_ENTRIES || current.len() != MAX_ENTRIES {
        return Err("Old evidence was truncated or replaced; original preserved".into());
    }
    let mut overlap = None;
    for length in 1..=MAX_ENTRIES {
        if previous[MAX_ENTRIES - length..] == current[..length] {
            if overlap.replace(length).is_some() {
                return Err("Old evidence rollover is ambiguous; original preserved".into());
            }
        }
    }
    overlap
        .ok_or_else(|| "Old evidence rollover has no verifiable overlap; original preserved".into())
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
                machine_source_digest: None,
                machine_source_length: None,
                machine_source_records: None,
                history_gap_unverified: false,
                profile_import_pending: false,
                clear_pending: false,
                clear_scope: None,
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
    let mut old_profile_present = false;
    let candidate = (|| -> Result<Option<(PathBuf, Vec<EvidenceEntry>)>, String> {
        let Some(old) = legacy()? else {
            return Ok(None);
        };
        let identity = match fs::canonicalize(&old) {
            Ok(identity) => identity,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Cannot identify old evidence ledger".into()),
        };
        old_profile_present = true;
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
    ledger.profile_import_pending |= old_profile_present;
    let candidate = match candidate {
        Ok(value) => value,
        Err(_) => {
            #[cfg(not(test))]
            crate::log::log_message(
                "warn",
                "Legacy evidence import is pending; original preserved",
            );
            ledger.profile_import_pending = true;
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
            let recovered = reload_verified(path, key_path, persist);
            if let Ok(mut actual) = recovered {
                #[cfg(not(test))]
                crate::log::log_message(
                    "warn",
                    "Legacy evidence import could not be published; retry pending",
                );
                actual.profile_import_pending = true;
                return Ok(actual);
            }
            return Err(error);
        }
    }
    Ok(ledger)
}

fn reload_verified(
    path: &Path,
    key_path: &Path,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<Ledger, String> {
    let encoded = fs::read_to_string(path).map_err(|_| "Cannot reread evidence ledger")?;
    let state = key(key_path, None, persist)?;
    if state.pending_digest.is_some() {
        return Err("Evidence commit is incomplete".into());
    }
    let bytes = crate::datastore::decode_section(&state.material, encoded.trim(), SCOPE)
        .map_err(|_| "Cannot authenticate evidence ledger")?;
    serde_json::from_slice(&bytes).map_err(|_| "Evidence ledger is malformed".into())
}

fn machine_snapshot(path: &Path) -> Result<Option<String>, String> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Cannot read old machine evidence; original preserved: {error}"
            ))
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_MACHINE_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read old machine evidence; original preserved")?;
    if bytes.len() as u64 > MAX_MACHINE_SOURCE_BYTES {
        return Err("Old machine evidence is too large; original preserved".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "Old machine evidence is malformed; original preserved".into())
}

/// Inspect protected status without importing or changing either source.
pub(super) fn read_only_status(
    path: &Path,
    machine_source: &Path,
    legacy: &impl Fn() -> Result<Option<PathBuf>, String>,
    key_path: &Path,
    failure_marker: &Path,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<bool, String> {
    let ledger = match fs::symlink_metadata(path) {
        Ok(_) => reload_verified(path, key_path, persist)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            empty_or_missing(key_path, persist)?
        }
        Err(error) => return Err(format!("Cannot inspect evidence ledger: {error}")),
    };
    let mut needs_review = ledger.history_gap_unverified || ledger.clear_pending || ledger.clear_scope.is_some();
    match fs::symlink_metadata(failure_marker) {
        Ok(_) => needs_review = true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot inspect evidence recording status: {error}")),
    }
    if let Some(raw) = machine_snapshot(machine_source)? {
        needs_review |= ledger.skip_future_imports
            || ledger.machine_source_digest.as_deref() != Some(digest(&raw).as_str());
    }
    if let Some(profile) = legacy()? {
        match fs::symlink_metadata(profile) {
            Ok(_) => needs_review = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Cannot inspect old evidence profile: {error}")),
        }
    }
    Ok(needs_review)
}

/// The old filename remains available to old binaries, but can no longer
/// overwrite the protected destination. Changed or ambiguous sources stay pending.
pub(super) fn read_with_machine_source(
    path: &Path,
    machine_source: &Path,
    legacy: &impl Fn() -> Result<Option<PathBuf>, String>,
    key_path: &Path,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<Ledger, String> {
    if matches!(fs::symlink_metadata(path), Err(ref e) if e.kind() == std::io::ErrorKind::NotFound)
        && machine_snapshot(machine_source)?
            .as_ref()
            .is_some_and(|raw| raw.starts_with(PREFIX))
    {
        return Err("Old protected evidence requires manual recovery; original preserved".into());
    }
    let mut ledger = read(path, &|| Ok(None), key_path, persist)?;
    if ledger.clear_pending || ledger.clear_scope.is_some() {
        return Err("Evidence clear was interrupted or inconsistent; retry clear before using the timeline".into());
    }
    if !ledger.skip_future_imports || ledger.machine_source_digest.is_some() {
        let source = machine_snapshot(machine_source)?;
        if let Some(raw) = source {
            let fingerprint = digest(&raw);
            if ledger.machine_source_digest.as_deref() != Some(fingerprint.as_str()) {
                let candidate =
                    (|| -> Result<(Vec<EvidenceEntry>, Vec<[u8; 32]>, bool), String> {
                        if !raw.is_empty() && !raw.ends_with('\n') {
                            return Err("Machine source has an incomplete final record".into());
                        }
                        let full = parse_machine_legacy(&raw)?;
                        let hashes = entry_hashes(&full)?;
                        let mut rolled = false;
                        let prior = match (
                            &ledger.machine_source_digest,
                            ledger.machine_source_length,
                        ) {
                            (None, None) => {
                                let mut existing = std::collections::HashSet::<Vec<u8>>::new();
                                for entry in &ledger.entries {
                                    existing.insert(
                                        serde_json::to_vec(entry)
                                            .map_err(|_| "Cannot compare evidence")?,
                                    );
                                }
                                for entry in &full {
                                    if existing.contains(
                                        &serde_json::to_vec(entry)
                                            .map_err(|_| "Cannot compare evidence")?,
                                    ) {
                                        return Err(
                                            "Ambiguous evidence records require review".into()
                                        );
                                    }
                                }
                                full.clone()
                            }
                            (Some(expected), Some(length)) => {
                                let length =
                                    usize::try_from(length).map_err(|_| "Invalid source length")?;
                                let prefix = raw.get(..length);
                                if prefix.is_some_and(|prefix| {
                                    digest(prefix) == *expected
                                        && (prefix.is_empty() || prefix.ends_with('\n'))
                                }) {
                                    let suffix = raw
                                        .get(length..)
                                        .ok_or("Machine source boundary is invalid")?;
                                    parse_machine_legacy(suffix)?
                                } else {
                                    let previous = ledger.machine_source_records.as_deref()
                                    .ok_or("Machine source was truncated or replaced; prior snapshot unavailable")?;
                                    let overlap = unique_rollover(previous, &hashes)?;
                                    rolled = true;
                                    full[overlap..].to_vec()
                                }
                            }
                            _ => return Err("Machine source state requires manual recovery".into()),
                        };
                        if machine_snapshot(machine_source)?.as_deref() != Some(raw.as_str()) {
                            return Err("Machine source changed during import".into());
                        }
                        Ok((prior, hashes, rolled))
                    })();
                {
                    let (mut prior, hashes, rolled) = candidate.map_err(|error| {
                        format!("Old evidence import pending; original preserved: {error}")
                    })?;
                    let mut combined = std::mem::take(&mut ledger.entries);
                    combined.append(&mut prior);
                    if ledger.machine_source_digest.is_none() {
                        combined.sort_by(|a, b| a.time.cmp(&b.time));
                    }
                    if combined.len() > MAX_ENTRIES {
                        combined.drain(..combined.len() - MAX_ENTRIES);
                    }
                    ledger.entries = combined;
                    ledger.machine_source_digest = Some(fingerprint);
                    ledger.machine_source_length = Some(raw.len() as u64);
                    ledger.machine_source_records = Some(hashes);
                    ledger.history_gap_unverified |= rolled;
                    if rolled {
                        #[cfg(not(test))]
                        crate::log::log_message("warn", "Old evidence rolled over; continuity before the overlap cannot be verified");
                    }
                    write(path, key_path, &ledger, persist).map_err(|error| {
                        format!(
                            "Old evidence import could not be saved; original preserved: {error}"
                        )
                    })?;
                    if machine_snapshot(machine_source)?.as_deref() != Some(raw.as_str()) {
                        return Err("Old evidence changed during import; stop older clients and retry recovery".into());
                    }
                }
            }
        }
    }
    read(path, legacy, key_path, persist)
}

/// Clear the protected machine ledger, the old machine ledger, and at most the
/// calling profile's old ledger. Caller holds the same mutex as old writers.
/// A persisted intent prevents old plaintext from being re-imported on restart.
pub(super) fn clear_user_requested(
    path: &Path,
    machine_source: &Path,
    current_profile: Option<&Path>,
    key_path: &Path,
    user: &str,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
    snapshot: &impl Fn(&Path) -> Result<Option<super::clear_io::FileStamp>, String>,
    erase_old: &impl Fn(&Path, &Option<super::clear_io::FileStamp>) -> Result<(), String>,
) -> Result<(), String> {
    if user.is_empty() || path == machine_source || path == key_path || machine_source == key_path
        || current_profile.is_some_and(|p| p == path || p == key_path || p == machine_source) {
        return Err("Evidence clear locations are invalid".into());
    }
    let mut ledger = read(path, &|| Ok(None), key_path, persist)?;
    let scope = if ledger.clear_pending {
        let scope = ledger.clear_scope.clone().ok_or("Evidence clear has no verified owner; preserve and review")?;
        if scope.user != user || scope.machine != machine_source || scope.profile.as_deref() != current_profile {
            return Err("Evidence clear belongs to another account or profile; original user must retry".into());
        }
        scope
    } else {
        if ledger.clear_scope.is_some() { return Err("Evidence clear state is inconsistent; preserve and review".into()); }
        let scope = ClearScope {
            user: user.to_owned(), machine: machine_source.to_path_buf(),
            profile: current_profile.map(Path::to_path_buf),
            machine_snapshot: snapshot(machine_source)?,
            profile_snapshot: current_profile.map(snapshot).transpose()?.flatten(),
        };
        ledger.entries.clear();
        ledger.skip_future_imports = true;
        ledger.machine_source_digest = Some(digest(""));
        ledger.machine_source_length = Some(0);
        ledger.machine_source_records = Some(Vec::new());
        ledger.clear_pending = true;
        ledger.clear_scope = Some(scope.clone());
        write(path, key_path, &ledger, persist)
            .map_err(|e| format!("Evidence clear could not start: {e}"))?;
        scope
    };
    erase_old(machine_source, &scope.machine_snapshot)
        .map_err(|e| format!("Evidence clear incomplete; retry clear: {e}"))?;
    if let Some(profile) = current_profile {
        erase_old(profile, &scope.profile_snapshot)
            .map_err(|e| format!("Evidence clear incomplete for this user; retry clear: {e}"))?;
    }
    ledger.clear_pending = false;
    ledger.clear_scope = None;
    write(path, key_path, &ledger, persist)
        .map_err(|e| format!("Evidence clear needs finalization; retry clear: {e}"))?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn existing_protected_ledger_without_clear_fields_remains_readable() {
        let raw = r#"{"entries":[],"imported_profiles":[],"skip_future_imports":false,"machine_source_digest":null,"machine_source_length":null,"machine_source_records":null,"history_gap_unverified":false}"#;
        let ledger: Ledger = serde_json::from_str(raw).unwrap();
        assert!(!ledger.clear_pending);
        assert!(ledger.clear_scope.is_none());
    }

    #[test]
    fn inconsistent_clear_scope_never_reimports_legacy_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("protected");
        let machine = dir.path().join("machine");
        let key = dir.path().join("key");
        let failure_marker = dir.path().join("no-failure-marker");
        let mut ledger = Ledger::default();
        ledger.clear_scope = Some(ClearScope {
            user: "user-a".into(), machine: machine.clone(), profile: None,
            machine_snapshot: None, profile_snapshot: None,
        });
        write(&protected, &key, &ledger, &crate::datastore_io::atomic_write).unwrap();
        let old = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&machine, &old).unwrap();
        assert!(read_with_machine_source(&protected, &machine, &|| Ok(None), &key,
            &crate::datastore_io::atomic_write).err().unwrap().contains("inconsistent"));
        assert!(read_only_status(&protected, &machine, &|| Ok(None), &key, &failure_marker,
            &crate::datastore_io::atomic_write).unwrap());
        assert_eq!(fs::read_to_string(&machine).unwrap(), old);
    }

    #[test]
    fn user_clear_finishes_machine_and_current_profile_and_imports_later_old_events() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("protected");
        let machine = dir.path().join("machine");
        let profile = dir.path().join("profile");
        let other = dir.path().join("other-profile");
        let key = dir.path().join("key");
        let old = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        for path in [&machine, &profile, &other] { fs::write(path, &old).unwrap(); }
        let erase = super::super::clear_io::clear_file;
        clear_user_requested(&protected, &machine, Some(&profile), &key, "user-a",
            &crate::datastore_io::atomic_write, &super::super::clear_io::snapshot, &erase).unwrap();
        assert!(fs::read(&machine).unwrap().is_empty());
        assert!(fs::read(&profile).unwrap().is_empty());
        assert_eq!(fs::read_to_string(&other).unwrap(), old);
        let blank = read_with_machine_source(&protected, &machine, &|| Ok(Some(profile.clone())), &key,
            &crate::datastore_io::atomic_write).unwrap();
        assert!(blank.entries.is_empty());
        assert_eq!(blank.machine_source_records, Some(Vec::new()));
        let mut new_event = entry(); new_event.summary = "after clear".into();
        fs::write(&machine, format!("{}\n", serde_json::to_string(&new_event).unwrap())).unwrap();
        let later = read_with_machine_source(&protected, &machine, &|| Ok(Some(profile.clone())), &key,
            &crate::datastore_io::atomic_write).unwrap();
        assert_eq!(later.entries.len(), 1);
        assert_eq!(later.entries[0].summary, "after clear");
    }

    #[test]
    fn interrupted_clear_blocks_reimport_and_retry_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("protected");
        let machine = dir.path().join("machine");
        let profile = dir.path().join("profile");
        let key = dir.path().join("key");
        let old = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&machine, &old).unwrap(); fs::write(&profile, &old).unwrap();
        let erase_fail_profile = |p: &Path, stamp: &Option<super::super::clear_io::FileStamp>| -> Result<(), String> {
            if p == profile { return Err("injected profile failure".into()); }
            super::super::clear_io::clear_file(p, stamp)
        };
        assert!(clear_user_requested(&protected, &machine, Some(&profile), &key, "user-a",
            &crate::datastore_io::atomic_write, &super::super::clear_io::snapshot, &erase_fail_profile).is_err());
        assert!(fs::read(&machine).unwrap().is_empty());
        assert_eq!(fs::read_to_string(&profile).unwrap(), old);
        let error = read_with_machine_source(&protected, &machine, &|| Ok(None), &key,
            &crate::datastore_io::atomic_write).err().unwrap();
        assert!(error.contains("interrupted"));
        let erase = super::super::clear_io::clear_file;
        clear_user_requested(&protected, &machine, Some(&profile), &key, "user-a",
            &crate::datastore_io::atomic_write, &super::super::clear_io::snapshot, &erase).unwrap();
        assert!(fs::read(&profile).unwrap().is_empty());
        assert!(read_with_machine_source(&protected, &machine, &|| Ok(None), &key,
            &crate::datastore_io::atomic_write).unwrap().entries.is_empty());
    }

    #[test]
    fn finalization_failure_is_reported_and_clear_can_be_retried() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("protected");
        let machine = dir.path().join("machine");
        let key = dir.path().join("key");
        fs::write(&machine, format!("{}\n", serde_json::to_string(&entry()).unwrap())).unwrap();
        let writes = AtomicUsize::new(0);
        let persist = |target: &Path, bytes: &[u8]| {
            if target == protected && writes.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err("injected finalization failure".into());
            }
            crate::datastore_io::atomic_write(target, bytes)
        };
        let erase = super::super::clear_io::clear_file;
        assert!(clear_user_requested(&protected, &machine, None, &key, "user-a", &persist, &super::super::clear_io::snapshot, &erase).is_err());
        assert!(fs::read(&machine).unwrap().is_empty());
        assert!(read_with_machine_source(&protected, &machine, &|| Ok(None), &key,
            &crate::datastore_io::atomic_write).err().unwrap().contains("interrupted"));
        let missing_machine = dir.path().join("missing-machine");
        let missing_failure_marker = dir.path().join("missing-failure-marker");
        assert!(read_only_status(&protected, &missing_machine, &|| Ok(None), &key,
            &missing_failure_marker, &crate::datastore_io::atomic_write).unwrap());
        clear_user_requested(&protected, &machine, None, &key, "user-a", &crate::datastore_io::atomic_write, &super::super::clear_io::snapshot, &erase).unwrap();
        assert!(read_with_machine_source(&protected, &machine, &|| Ok(None), &key,
            &crate::datastore_io::atomic_write).unwrap().entries.is_empty());
    }

    #[test]
    fn failed_clear_intent_keeps_legacy_source_and_is_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("protected"); let machine = dir.path().join("machine");
        let key = dir.path().join("key");
        let old = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&machine, &old).unwrap();
        let persist = |target: &Path, bytes: &[u8]| {
            if target == protected { return Err("injected intent failure".into()); }
            crate::datastore_io::atomic_write(target, bytes)
        };
        let erase = super::super::clear_io::clear_file;
        assert!(clear_user_requested(&protected, &machine, None, &key, "user-a", &persist, &super::super::clear_io::snapshot, &erase).is_err());
        assert_eq!(fs::read_to_string(&machine).unwrap(), old);
        clear_user_requested(&protected, &machine, None, &key, "user-a", &crate::datastore_io::atomic_write, &super::super::clear_io::snapshot, &erase).unwrap();
        assert!(fs::read(&machine).unwrap().is_empty());
    }

    #[test]
    fn pending_clear_rejects_different_user_and_preserves_late_old_events() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("protected"); let machine = dir.path().join("machine");
        let a = dir.path().join("profile-a"); let b = dir.path().join("profile-b"); let key = dir.path().join("key");
        fs::write(&machine, b"old machine").unwrap(); fs::write(&a, b"user a").unwrap(); fs::write(&b, b"user b").unwrap();
        let fail_a = |p: &Path, expected: &Option<super::super::clear_io::FileStamp>| -> Result<(), String> {
            if p == a { return Err("injected failure".into()); }
            super::super::clear_io::clear_file(p, expected)
        };
        let snapshot = super::super::clear_io::snapshot; let erase = super::super::clear_io::clear_file;
        assert!(clear_user_requested(&protected, &machine, Some(&a), &key, "user-a",
            &crate::datastore_io::atomic_write, &snapshot, &fail_a).is_err());
        assert!(fs::read(&machine).unwrap().is_empty());
        assert!(clear_user_requested(&protected, &machine, Some(&b), &key, "user-b",
            &crate::datastore_io::atomic_write, &snapshot, &erase).err().unwrap().contains("another account"));
        assert!(clear_user_requested(&protected, &machine, Some(&a), &key, "user-b",
            &crate::datastore_io::atomic_write, &snapshot, &erase).is_err());
        assert!(clear_user_requested(&protected, &machine, Some(&b), &key, "user-a",
            &crate::datastore_io::atomic_write, &snapshot, &erase).is_err());
        assert_eq!(fs::read(&a).unwrap(), b"user a"); assert_eq!(fs::read(&b).unwrap(), b"user b");
        fs::write(&machine, b"new old-writer event").unwrap();
        assert!(clear_user_requested(&protected, &machine, Some(&a), &key, "user-a",
            &crate::datastore_io::atomic_write, &snapshot, &erase).is_err());
        assert_eq!(fs::read(&machine).unwrap(), b"new old-writer event");
        assert_eq!(fs::read(&a).unwrap(), b"user a");
    }

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
                machine_source_digest: None,
                machine_source_length: None,
                machine_source_records: None,
                history_gap_unverified: false,
                profile_import_pending: false,
                clear_pending: false,
                clear_scope: None,
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
            machine_source_digest: None,
            machine_source_length: None,
            machine_source_records: None,
            history_gap_unverified: false,
            profile_import_pending: false,
            clear_pending: false,
                clear_scope: None,
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
                machine_source_digest: None,
                machine_source_length: None,
                machine_source_records: None,
                history_gap_unverified: false,
                profile_import_pending: false,
                clear_pending: false,
                clear_scope: None,
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
                machine_source_digest: None,
                machine_source_length: None,
                machine_source_records: None,
                history_gap_unverified: false,
                profile_import_pending: false,
                clear_pending: false,
                clear_scope: None,
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
                machine_source_digest: None,
                machine_source_length: None,
                machine_source_records: None,
                history_gap_unverified: false,
                profile_import_pending: false,
                clear_pending: false,
                clear_scope: None,
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
                machine_source_digest: None,
                machine_source_length: None,
                machine_source_records: None,
                history_gap_unverified: false,
                profile_import_pending: false,
                clear_pending: false,
                clear_scope: None,
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
                machine_source_digest: None,
                machine_source_length: None,
                machine_source_records: None,
                history_gap_unverified: false,
                profile_import_pending: false,
                clear_pending: false,
                clear_scope: None,
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
        assert!(ledger.profile_import_pending);
        assert_eq!(fs::read(&legacy).unwrap(), b"malformed old data");
    }

    #[test]
    fn status_reports_retained_profile_and_never_writes_or_imports_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old_machine = dir.path().join("machine");
        let profile = dir.path().join("profile");
        let key = dir.path().join("key");
        write(
            &path,
            &key,
            &Ledger {
                entries: vec![entry()],
                ..Ledger::default()
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let original = fs::read(&path).unwrap();
        assert!(!read_only_status(
            &path,
            &old_machine,
            &|| Ok(None),
            &key,
            &dir.path().join("failure"),
            &|_, _| Err("status must not write".into())
        )
        .unwrap());
        let marker = dir.path().join("failure");
        fs::write(&marker, b"recording-incomplete").unwrap();
        assert!(
            read_only_status(&path, &old_machine, &|| Ok(None), &key, &marker, &|_, _| {
                Err("status must not write".into())
            })
            .unwrap()
        );
        fs::remove_file(&marker).unwrap();
        fs::write(&profile, b"malformed old profile").unwrap();
        assert!(read_only_status(
            &path,
            &old_machine,
            &|| Ok(Some(profile.clone())),
            &key,
            &dir.path().join("failure"),
            &|_, _| Err("status must not write".into())
        )
        .unwrap());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(profile).unwrap(), b"malformed old profile");
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

    #[test]
    fn old_machine_writer_cannot_overwrite_new_protected_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        let original = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&old, &original).unwrap();
        let imported = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(imported.entries.len(), 1);
        let protected = fs::read(&path).unwrap();
        let mut later = entry();
        later.summary = "later old-client write".into();
        fs::write(
            &old,
            format!("{}{}\n", original, serde_json::to_string(&later).unwrap()),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), protected);
        let reconciled = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(reconciled.entries.len(), 2);
        assert!(!String::from_utf8(fs::read(&path).unwrap())
            .unwrap()
            .contains("later old-client write"));
        assert!(fs::read_to_string(old)
            .unwrap()
            .contains("later old-client write"));
    }

    #[test]
    fn source_changed_during_publish_stays_pending_and_original_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        let original = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        let mut later = entry();
        later.summary = "new old-client event".into();
        let changed = format!("{}{}\n", original, serde_json::to_string(&later).unwrap());
        fs::write(&old, &original).unwrap();
        let first = read_with_machine_source(&path, &old, &|| Ok(None), &key, &|target, bytes| {
            if target == path {
                fs::write(&old, &changed).unwrap();
            }
            crate::datastore_io::atomic_write(target, bytes)
        })
        .err()
        .unwrap();
        assert!(first.contains("changed during import"));
        assert_eq!(fs::read_to_string(&old).unwrap(), changed);
        let later = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(later.entries.len(), 2);
        assert_eq!(
            later.machine_source_digest.as_deref(),
            Some(digest(&changed).as_str())
        );
        assert_eq!(fs::read_to_string(old).unwrap(), changed);
    }

    #[test]
    fn optional_old_machine_import_failure_is_visible_and_preserves_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        let mut current = entry();
        current.summary = "current".into();
        write(
            &path,
            &key,
            &Ledger {
                entries: vec![current],
                ..Ledger::default()
            },
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let before = fs::read(&path).unwrap();
        fs::write(
            &old,
            format!("{}\n", serde_json::to_string(&entry()).unwrap()),
        )
        .unwrap();
        let result = read_with_machine_source(&path, &old, &|| Ok(None), &key, &|target, bytes| {
            if target == path {
                Err("denied".into())
            } else {
                crate::datastore_io::atomic_write(target, bytes)
            }
        })
        .err()
        .unwrap();
        assert!(result.contains("could not be saved"));
        assert_eq!(
            read(
                &path,
                &|| Ok(None),
                &key,
                &crate::datastore_io::atomic_write
            )
            .unwrap()
            .entries[0]
                .summary,
            "current"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(fs::read_to_string(&old)
            .unwrap()
            .contains("private summary"));
    }

    #[test]
    fn old_machine_truncation_and_partial_append_fail_but_deletion_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        let original = format!("{}\n", serde_json::to_string(&entry()).unwrap());
        fs::write(&old, &original).unwrap();
        read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let protected = fs::read(&path).unwrap();
        fs::write(&old, numbered_line(8)).unwrap();
        assert!(read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write
        )
        .err()
        .unwrap()
        .contains("truncated or replaced"));
        fs::write(&old, format!("{original}{{")).unwrap();
        assert!(read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write
        )
        .err()
        .unwrap()
        .contains("incomplete"));
        assert_eq!(fs::read(&path).unwrap(), protected);
        fs::remove_file(&old).unwrap();
        assert_eq!(
            read_with_machine_source(
                &path,
                &old,
                &|| Ok(None),
                &key,
                &crate::datastore_io::atomic_write
            )
            .unwrap()
            .entries
            .len(),
            1
        );
    }

    fn numbered_line(number: usize) -> String {
        let mut value = entry();
        value.summary = format!("event-{number:04}");
        if number == 1000 {
            value.time = "2020-01-01T00:00:00Z".into();
        }
        format!("{}\n", serde_json::to_string(&value).unwrap())
    }

    #[test]
    fn normal_rolling_writer_reconciles_a_unique_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        let previous: String = (0..MAX_ENTRIES).map(numbered_line).collect();
        fs::write(&old, previous).unwrap();
        read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let rolled: String = (1..=MAX_ENTRIES).map(numbered_line).collect();
        fs::write(&old, &rolled).unwrap();
        let ledger = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        assert_eq!(ledger.entries.len(), MAX_ENTRIES);
        assert!(ledger
            .entries
            .iter()
            .any(|entry| entry.summary == "event-1000"));
        assert_eq!(
            ledger
                .entries
                .iter()
                .filter(|entry| entry.summary == "event-1000")
                .count(),
            1
        );
        assert!(ledger.history_gap_unverified);
        assert!(read_only_status(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &dir.path().join("failure"),
            &|_, _| Err("status must not write".into())
        )
        .unwrap());
        assert_eq!(fs::read_to_string(old).unwrap(), rolled);
    }

    #[test]
    fn ambiguous_rolling_overlap_and_replacement_preserve_originals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        let repeated = numbered_line(7).repeat(MAX_ENTRIES);
        fs::write(&old, repeated).unwrap();
        read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .unwrap();
        let protected = fs::read(&path).unwrap();
        let ambiguous = format!(
            "{}{}",
            numbered_line(7).repeat(MAX_ENTRIES - 1),
            numbered_line(8)
        );
        fs::write(&old, &ambiguous).unwrap();
        let error = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .err()
        .unwrap();
        assert!(error.contains("ambiguous"));
        assert_eq!(fs::read(&path).unwrap(), protected);
        assert_eq!(fs::read_to_string(&old).unwrap(), ambiguous);
        let replacement: String = (1000..2000).map(numbered_line).collect();
        fs::write(&old, &replacement).unwrap();
        let error = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .err()
        .unwrap();
        assert!(error.contains("no verifiable overlap"));
        assert_eq!(fs::read(&path).unwrap(), protected);
        assert_eq!(fs::read_to_string(old).unwrap(), replacement);
    }

    #[test]
    fn old_encrypted_prototype_source_returns_recoverable_error_without_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        fs::write(&old, "enc:v2:old-prototype").unwrap();
        let error = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .err()
        .unwrap();
        assert!(error.contains("manual recovery"));
        assert_eq!(fs::read_to_string(old).unwrap(), "enc:v2:old-prototype");
        assert!(!path.exists() && !key.exists());
    }

    #[test]
    fn oversized_old_machine_source_fails_without_changing_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected");
        let old = dir.path().join("old-machine");
        let key = dir.path().join("key");
        fs::File::create(&old)
            .unwrap()
            .set_len(MAX_MACHINE_SOURCE_BYTES + 1)
            .unwrap();
        let error = read_with_machine_source(
            &path,
            &old,
            &|| Ok(None),
            &key,
            &crate::datastore_io::atomic_write,
        )
        .err()
        .unwrap();
        assert!(error.contains("too large"));
        assert_eq!(
            fs::metadata(old).unwrap().len(),
            MAX_MACHINE_SOURCE_BYTES + 1
        );
        assert!(!path.exists() && !key.exists());
    }

    #[test]
    #[ignore = "explicit isolated multi-identity acceptance fixture"]
    fn multi_identity_acceptance_fixture() {
        use std::time::{Duration, Instant};
        let id = std::env::var("WINCMD_EVIDENCE_FIXTURE_ID").expect("fixture id required");
        assert!(id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()));
        let mode = std::env::var("WINCMD_EVIDENCE_FIXTURE_MODE").expect("fixture mode required");
        let directory = std::path::PathBuf::from(
            std::env::var_os("WINCMD_EVIDENCE_FIXTURE_DIR").expect("fixture directory required"),
        );
        let path = crate::paths::machine_state_file(&format!("evidence-fixture-{id}.dat")).unwrap();
        let key_path =
            crate::paths::machine_state_file(&format!("evidence-fixture-{id}.key")).unwrap();
        let _lock = super::super::mutex::acquire_named(
            &format!(r"Global\WinCommander_evidence-fixture-{id}_lock"),
            5_000,
        )
        .unwrap();
        if mode == "hold" {
            fs::write(directory.join("lock-held"), b"held").unwrap();
            let until = Instant::now() + Duration::from_secs(25);
            while !directory.join("release-lock").exists() {
                assert!(Instant::now() < until, "fixture lock timed out");
                std::thread::sleep(Duration::from_millis(50));
            }
            return;
        }
        assert!(matches!(
            mode.as_str(),
            "administrator" | "system" | "parth" | "wait-admin"
        ));
        let mut ledger = read(
            &path,
            &|| Ok(None),
            &key_path,
            &crate::paths::atomic_write_machine_state,
        )
        .unwrap();
        let mut item = entry();
        item.summary = format!("identity-{mode}");
        ledger.entries.push(item);
        write(
            &path,
            &key_path,
            &ledger,
            &crate::paths::atomic_write_machine_state,
        )
        .unwrap();
        let reopened = read(
            &path,
            &|| Ok(None),
            &key_path,
            &crate::paths::atomic_write_machine_state,
        )
        .unwrap();
        assert!(reopened
            .entries
            .iter()
            .any(|item| item.summary == format!("identity-{mode}")));
        let encoded = fs::read_to_string(path).unwrap();
        assert!(!encoded.contains("identity-"));
    }
}
