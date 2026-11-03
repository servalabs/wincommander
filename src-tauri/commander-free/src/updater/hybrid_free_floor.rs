// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental replay check; deletion, restoration and privileged attackers remain open.
use super::{channel_name, Trust, VerifiedUpdate};
use serde::{Deserialize, Serialize};
use std::io::Read;

const LIMIT: usize = 65536;
const NAME: &str = "hybrid-free-update-floor.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: String,
    channel: String,
    envelope: String,
}

struct Floor {
    version: [u32; 3],
    epoch: u64,
    hash: String,
}

fn verify(record: &Record, trust: &Trust) -> Result<Floor, String> {
    if !matches!(record.channel.as_str(), "stable" | "beta") {
        return Err("Free update record has an invalid channel".into());
    }
    let url = format!(
        "https://winupdates.servalabs.com/free/v{0}/WinCommander_{0}_x64-setup.exe",
        record.version
    );
    let (release, epoch) = wincmd_pq_experiment::free_release::verify_metadata(
        record.envelope.as_bytes(),
        &record.version,
        &record.channel,
        &url,
        &trust.policy(),
    )
    .map_err(|_| "Free update record is not trusted")?;
    let parts: Vec<u32> = release
        .version
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| "Free update record version is invalid")?;
    let version = parts
        .try_into()
        .map_err(|_| "Free update record version is invalid")?;
    Ok(Floor {
        version,
        epoch,
        hash: release.sha256,
    })
}

fn check(previous: Option<&[u8]>, candidate: &Record, trust: &Trust) -> Result<(), String> {
    let next = verify(candidate, trust)?;
    let Some(previous) = previous else {
        return Ok(());
    };
    if previous.len() > LIMIT {
        return Err("Free update record is too large".into());
    }
    let previous: Record =
        serde_json::from_slice(previous).map_err(|_| "Free update record is invalid")?;
    let old = verify(&previous, trust)?;
    if next.version < old.version
        || next.epoch < old.epoch
        || (next.version == old.version && next.hash != old.hash)
    {
        return Err("Free update would roll back or replace a recorded release".into());
    }
    Ok(())
}

fn read_record(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::File::open(path) {
        Ok(file) => {
            let mut data = Vec::new();
            file.take((LIMIT + 1) as u64)
                .read_to_end(&mut data)
                .map_err(|_| "Cannot read Free update record")?;
            if data.len() > LIMIT {
                return Err("Free update record is too large".into());
            }
            Ok(Some(data))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Cannot read Free update record".into()),
    }
}

/// Advance before launch so a cancelled or failed installer can retry the same release.
pub(super) fn record(
    candidate: &VerifiedUpdate,
    trust: &Trust,
) -> Result<crate::paths::MachineStateLock, String> {
    record_with_lock(candidate, trust, "hybrid-free-update")
}

fn record_with_lock(
    candidate: &VerifiedUpdate,
    trust: &Trust,
    lock_name: &str,
) -> Result<crate::paths::MachineStateLock, String> {
    let lock = crate::paths::acquire_machine_state_lock(lock_name)?;
    let path = crate::paths::machine_state_file(NAME)?;
    let old = read_record(&path)?;
    let record = Record {
        version: candidate.version.clone(),
        channel: channel_name(candidate.channel).into(),
        envelope: String::from_utf8(candidate.envelope.clone())
            .map_err(|_| "Free update proof is invalid")?,
    };
    check(old.as_deref(), &record, trust)?;
    let encoded = serde_json::to_vec(&record).map_err(|_| "Cannot encode Free update record")?;
    if encoded.len() > LIMIT {
        return Err("Free update record is too large".into());
    }
    crate::paths::atomic_write_machine_state(&path, &encoded)
        .map_err(|_| "Cannot save Free update record".to_string())?;
    if read_record(&path)?.as_deref() != Some(encoded.as_slice()) {
        return Err("Free update record readback failed".into());
    }
    Ok(lock)
}

#[cfg(test)]
#[path = "hybrid_free_floor_tests.rs"]
mod tests;
