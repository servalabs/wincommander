// SPDX-License-Identifier: AGPL-3.0-or-later
//! Password-independent, account-scoped preferences. Payloads confer no machine authority.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wincmd_shared::personal_settings::{
    PersonalSettingsRecord, ReadPersonalSettingsRequest, WritePersonalSettingsRequest,
    MAX_PERSONAL_SETTINGS_BYTES, READ_PERSONAL_SETTINGS_VERB, WRITE_PERSONAL_SETTINGS_VERB,
};
use zeroize::Zeroizing;

#[path = "personal_settings_windows.rs"]
mod platform;
#[path = "personal_settings_scheduler.rs"]
mod scheduler;
#[cfg(test)]
#[path = "personal_settings_tests.rs"]
mod tests;

type StoreResult<T> = Result<T, &'static str>;
const UNAVAILABLE: &str = "personal_settings_unavailable";
const INVALID: &str = "personal_settings_invalid";
const CORRUPT: &str = "personal_settings_corrupt";
const MAX_RECORD_BYTES: u64 = (MAX_PERSONAL_SETTINGS_BYTES + 64 * 1024) as u64;
fn scheduler() -> &'static scheduler::Scheduler {
    static SCHEDULER: OnceLock<scheduler::Scheduler> = OnceLock::new();
    SCHEDULER.get_or_init(scheduler::Scheduler::new)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRecord {
    version: u32,
    owner_sid: String,
    record: PersonalSettingsRecord,
}

pub(crate) async fn handle(
    verb: &str,
    args: serde_json::Value,
    peer: Option<&crate::pipe::AuthenticatedPipePeer>,
) -> StoreResult<serde_json::Value> {
    let sid = peer
        .map(|peer| peer.caller_sid())
        .filter(|sid| !sid.is_empty())
        .ok_or("personal_settings_unauthorized")?
        .to_owned();
    let verb = verb.to_owned();
    let owner = sid.clone();
    // Only the captured peer supplies ownership; no token impersonation crosses threads.
    scheduler()
        .run(&sid, move || {
            let root = platform::store_root()?;
            let record = execute(&root, &verb, args, &owner)?;
            serde_json::to_value(record).map_err(|_| UNAVAILABLE)
        })
        .await
}

fn execute(
    root: &Path,
    verb: &str,
    args: serde_json::Value,
    sid: &str,
) -> StoreResult<PersonalSettingsRecord> {
    enum Operation {
        Read,
        Write(WritePersonalSettingsRequest),
    }
    let operation = match verb {
        READ_PERSONAL_SETTINGS_VERB => {
            serde_json::from_value::<ReadPersonalSettingsRequest>(args).map_err(|_| INVALID)?;
            Operation::Read
        }
        WRITE_PERSONAL_SETTINGS_VERB => {
            let request: WritePersonalSettingsRequest =
                serde_json::from_value(args).map_err(|_| INVALID)?;
            validate_value(&request.value)?;
            Operation::Write(request)
        }
        _ => return Err(INVALID),
    };
    let _directory_guards = platform::secure_directory(root)?;
    let path = record_path(root, sid);
    let current = read_record(&path, sid)?;
    let Operation::Write(request) = operation else {
        return Ok(current);
    };
    if request.expected_revision != current.revision {
        return Err("personal_settings_conflict");
    }
    let record = PersonalSettingsRecord {
        revision: current
            .revision
            .checked_add(1)
            .ok_or("personal_settings_revision_exhausted")?,
        value: Some(request.value),
        legacy_recovery_required: current.legacy_recovery_required
            || request.legacy_recovery_required,
    };
    let payload = Zeroizing::new(
        serde_json::to_vec(&StoredRecord {
            version: 1,
            owner_sid: sid.to_owned(),
            record: record.clone(),
        })
        .map_err(|_| INVALID)?,
    );
    let encrypted = platform::dpapi(&payload, sid, true)?;
    atomic_write(&path, &encrypted)?;
    Ok(record)
}

fn validate_value(value: &serde_json::Value) -> StoreResult<()> {
    if !value.is_object()
        || serde_json::to_vec(value).map_err(|_| INVALID)?.len() > MAX_PERSONAL_SETTINGS_BYTES
    {
        return Err(INVALID);
    }
    Ok(())
}

fn record_path(root: &Path, sid: &str) -> PathBuf {
    let digest: String = Sha256::digest(sid.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    root.join(format!("{digest}.settings"))
}

fn read_record(path: &Path, sid: &str) -> StoreResult<PersonalSettingsRecord> {
    let file = match platform::open_record(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PersonalSettingsRecord::default())
        }
        Err(_) => return Err(UNAVAILABLE),
    };
    let mut ciphertext = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut ciphertext)
        .map_err(|_| UNAVAILABLE)?;
    if ciphertext.len() as u64 > MAX_RECORD_BYTES {
        return Err(CORRUPT);
    }
    let plaintext = Zeroizing::new(platform::dpapi(&ciphertext, sid, false).map_err(|_| CORRUPT)?);
    let stored: StoredRecord = serde_json::from_slice(&plaintext).map_err(|_| CORRUPT)?;
    if stored.version != 1 || stored.owner_sid != sid || stored.record.revision == 0 {
        return Err(CORRUPT);
    }
    validate_value(stored.record.value.as_ref().ok_or(CORRUPT)?).map_err(|_| CORRUPT)?;
    Ok(stored.record)
}

fn atomic_write(path: &Path, encrypted: &[u8]) -> StoreResult<()> {
    let temp = path.with_extension(format!("{:016x}.pending", rand::random::<u64>()));
    let mut created = false;
    let result = (|| {
        let mut file = platform::open_record(&temp, true).map_err(|_| UNAVAILABLE)?;
        created = true;
        file.write_all(encrypted).map_err(|_| UNAVAILABLE)?;
        file.sync_all().map_err(|_| UNAVAILABLE)?;
        drop(file);
        platform::replace(&temp, path)
    })();
    if result.is_err() && created {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
