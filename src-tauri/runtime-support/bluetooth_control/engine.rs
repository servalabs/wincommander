// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothStatus {
    pub blocked: bool,
    pub state: String,
    pub adapter_count: u32,
    pub owned_adapter_count: u32,
    pub can_change: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub version: u32,
    pub blocked: bool,
    pub records: Vec<Record>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub id: String,
    pub identity: String,
    pub confirmed: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Adapter {
    pub id: String,
    pub identity: String,
    pub problem: u32,
}

pub(super) trait Platform {
    fn admin(&self) -> bool;
    fn read(&mut self) -> Result<Journal, String>;
    fn write(&mut self, journal: &Journal) -> Result<(), String>;
    fn adapters(&mut self) -> Result<Vec<Adapter>, String>;
    fn externally_managed(&mut self) -> Result<bool, String>;
    fn disable(&mut self, adapter: &Adapter) -> Result<(), String>;
    fn enable(&mut self, adapter: &Adapter) -> Result<(), String>;
}

fn checked_read(platform: &mut impl Platform) -> Result<Journal, String> {
    let journal = platform.read()?;
    if journal.version != 1 || journal.records.len() > 128 {
        return Err("[BT_STATE_UNVERIFIED] Bluetooth recovery record is invalid. Leave Bluetooth unchanged and contact your administrator.".into());
    }
    let mut ids = std::collections::HashSet::new();
    for record in &journal.records {
        if record.id.is_empty()
            || record.id.len() > 1024
            || record.identity.is_empty()
            || record.identity.len() > 8192
            || !ids.insert(record.id.to_uppercase())
        {
            return Err("[BT_STATE_UNVERIFIED] Bluetooth recovery record is invalid. Leave Bluetooth unchanged and contact your administrator.".into());
        }
    }
    Ok(journal)
}

pub(super) fn status(platform: &mut impl Platform) -> Result<BluetoothStatus, String> {
    let journal = checked_read(platform)?;
    snapshot(platform, &journal)
}

fn snapshot(platform: &mut impl Platform, journal: &Journal) -> Result<BluetoothStatus, String> {
    let adapters = platform.adapters()?;
    let external = platform.externally_managed()?;
    let uncertain = journal.records.iter().any(|record| {
        !record.confirmed
            || adapters.iter().any(|adapter| {
                adapter.id.eq_ignore_ascii_case(&record.id) && adapter.identity != record.identity
            })
    });
    let not_disabled = adapters.iter().any(|adapter| adapter.problem != 22);
    let (state, message) = if uncertain {
        ("recovery_required", "An interrupted or changed Bluetooth adapter needs administrator review. WinCommander will not enable an uncertain adapter.")
    } else if journal.blocked && not_disabled {
        ("partial", "The Bluetooth restriction is saved, but Windows has not confirmed every adapter disabled. Retry or check device status.")
    } else if journal.blocked && adapters.is_empty() {
        ("no_adapter", "The Bluetooth restriction is saved for this PC. No Bluetooth adapter is currently detected.")
    } else if journal.blocked {
        (
            "blocked",
            "Bluetooth is blocked for this PC. Paired devices are preserved.",
        )
    } else if !journal.records.is_empty() {
        ("recovery_required", "Bluetooth restoration is incomplete. Reconnect any removed adapter and retry Allow Bluetooth.")
    } else if external {
        (
            "externally_blocked",
            "Bluetooth has an external device policy. Ask your administrator to review it.",
        )
    } else if adapters.is_empty() {
        ("no_adapter", "No Bluetooth adapter detected.")
    } else if adapters.iter().any(|adapter| adapter.problem != 0) {
        ("externally_blocked", "Windows reports an unavailable or externally disabled Bluetooth adapter. WinCommander has left it unchanged.")
    } else {
        (
            "allowed",
            "WinCommander allows Bluetooth on this PC. Windows radio settings still apply.",
        )
    };
    Ok(BluetoothStatus {
        blocked: journal.blocked,
        state: state.into(),
        adapter_count: adapters.len() as u32,
        owned_adapter_count: journal.records.iter().filter(|r| r.confirmed).count() as u32,
        can_change: platform.admin(),
        message: message.into(),
    })
}

pub(super) fn set(platform: &mut impl Platform, blocked: bool) -> Result<BluetoothStatus, String> {
    if !platform.admin() {
        return Err("[BT_ADMIN_REQUIRED] Run WinCommander as administrator to change Bluetooth for this PC.".into());
    }
    let mut journal = checked_read(platform)?;
    // Persist intent before any device mutation. A failed commit changes no device.
    journal.blocked = blocked;
    platform.write(&journal)?;
    if blocked {
        block(platform, &mut journal, true)?;
    } else {
        restore(platform, &mut journal)?;
    }
    snapshot(platform, &journal)
}

pub(super) fn reconcile(platform: &mut impl Platform) -> Result<BluetoothStatus, String> {
    let mut journal = checked_read(platform)?;
    if journal.blocked {
        if !platform.admin() {
            return Err(
                "[BT_ADMIN_REQUIRED] Administrator access is required to enforce the Bluetooth restriction.".into(),
            );
        }
        block(platform, &mut journal, false)?;
    }
    snapshot(platform, &journal)
}

fn block(
    platform: &mut impl Platform,
    journal: &mut Journal,
    explicit_retry: bool,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut failure = None;
    for adapter in platform.adapters()? {
        if started.elapsed() >= std::time::Duration::from_secs(30) {
            break;
        }
        let existing = journal
            .records
            .iter()
            .position(|r| r.id.eq_ignore_ascii_case(&adapter.id));
        if existing.is_some_and(|index| journal.records[index].identity != adapter.identity) {
            continue;
        }
        // An unconfirmed operation needs an explicit retry, not an endless
        // background driver call that can delay the user's recovery request.
        if !explicit_retry && existing.is_some_and(|index| !journal.records[index].confirmed) {
            continue;
        }
        if adapter.problem != 0 {
            continue;
        } // Never claim pre-disabled/faulted devices.
        if existing.is_none() && journal.records.len() >= 128 {
            return Err(
                "Bluetooth adapter limit reached. Ask your administrator to review device history."
                    .into(),
            );
        }
        let index = if let Some(index) = existing {
            // A currently-enabled matching radio permits a fresh disable attempt.
            // An interrupted, already-disabled radio never reaches this branch.
            journal.records[index].confirmed = false;
            index
        } else {
            journal.records.push(Record {
                id: adapter.id.clone(),
                identity: adapter.identity.clone(),
                confirmed: false,
            });
            journal.records.len() - 1
        };
        platform.write(journal)?;
        if let Err(error) = platform.disable(&adapter) {
            // Even a failed native call may have changed state; retain pending evidence.
            failure.get_or_insert(error);
            continue;
        }
        let verified = platform.adapters()?.iter().any(|current| {
            current.id.eq_ignore_ascii_case(&adapter.id)
                && current.identity == adapter.identity
                && current.problem == 22
        });
        if verified {
            journal.records[index].confirmed = true;
            platform.write(journal)?;
        } else {
            failure.get_or_insert_with(|| "[BT_DISABLE_TRANSITION_TIMEOUT] Windows did not confirm the Bluetooth adapter disabled.".to_string());
        }
    }
    failure.map_or(Ok(()), Err)
}

fn restore(platform: &mut impl Platform, journal: &mut Journal) -> Result<(), String> {
    if !journal.records.is_empty() && platform.externally_managed()? {
        return Err("[BT_POLICY_UNVERIFIED] Bluetooth restoration is blocked by another device policy. Ask your administrator to review it; no adapter was enabled.".into());
    }
    let adapters = platform.adapters()?;
    let started = std::time::Instant::now();
    let mut failure = None;
    for adapter in adapters {
        if started.elapsed() >= std::time::Duration::from_secs(30) {
            break;
        }
        let Some(index) = journal
            .records
            .iter()
            .position(|r| r.id.eq_ignore_ascii_case(&adapter.id))
        else {
            continue;
        };
        let record = &journal.records[index];
        if record.identity != adapter.identity {
            continue;
        }
        if adapter.problem == 0 {
            journal.records.remove(index);
            platform.write(journal)?;
            continue;
        }
        if !record.confirmed {
            continue;
        }
        if adapter.problem != 22 {
            continue;
        }
        if let Err(error) = platform.enable(&adapter) {
            failure.get_or_insert(error);
            continue;
        }
        let verified = platform.adapters()?.iter().any(|current| {
            current.id.eq_ignore_ascii_case(&adapter.id)
                && current.identity == adapter.identity
                && current.problem == 0
        });
        if verified {
            journal.records.remove(index);
            platform.write(journal)?;
        } else {
            failure.get_or_insert_with(|| "[BT_ENABLE_TRANSITION_TIMEOUT] Windows did not confirm the Bluetooth adapter restored.".to_string());
        }
    }
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod failure_tests;
#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
