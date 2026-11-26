// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who holds a drive letter, and removal of links whose device no longer exists.
use super::{encrypted_slot, LogonId};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HolderKind {
    VaultMount,
    RamDisk,
    WindowsVolume,
    NetworkDrive,
    Subst,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LogonState {
    Machine,
    Active,
    Disconnected,
    SignedOut,
    Service,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LetterHolder {
    /// `None` is the machine-wide `\GLOBAL??` namespace.
    pub(crate) logon: Option<LogonId>,
    pub(crate) owner_sid: Option<String>,
    pub(crate) session: Option<u32>,
    pub(crate) state: LogonState,
    pub(crate) kind: HolderKind,
    pub(crate) device: Option<String>,
    pub(crate) device_present: bool,
}

impl LetterHolder {
    pub(crate) fn is_dead_device(&self) -> bool {
        matches!(self.kind, HolderKind::VaultMount | HolderKind::RamDisk) && !self.device_present
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LetterRelease {
    pub(crate) checked: Vec<(LetterHolder, bool)>,
}

impl LetterRelease {
    pub(crate) fn released(&self) -> usize {
        self.checked
            .iter()
            .filter(|(_, released)| *released)
            .count()
    }

    pub(crate) fn reported(&self) -> Option<&LetterHolder> {
        let kept = self.checked.iter().find(|(_, released)| !*released);
        kept.or_else(|| self.checked.first())
            .map(|(holder, _)| holder)
    }
}

fn device_component(target: &str) -> Option<&str> {
    let rest = target
        .get(..8)
        .filter(|prefix| prefix.eq_ignore_ascii_case(r"\Device\"))
        .map(|_| &target[8..])?;
    (!rest.is_empty() && !rest.contains('\\')).then_some(rest)
}

pub(crate) fn holder_kind(target: &str) -> HolderKind {
    if encrypted_slot(target).is_some() {
        return HolderKind::VaultMount;
    }
    if super::drive_report::network_target(target) {
        return HolderKind::NetworkDrive;
    }
    if target.starts_with(r"\??\") {
        return HolderKind::Subst;
    }
    let Some(device) = device_component(target).map(str::to_ascii_lowercase) else {
        return HolderKind::Other;
    };
    if device
        .strip_prefix("imdisk")
        .is_some_and(|unit| !unit.is_empty() && unit.bytes().all(|byte| byte.is_ascii_digit()))
    {
        HolderKind::RamDisk
    } else if ["harddiskvolume", "cdrom", "floppy"]
        .iter()
        .any(|prefix| device.starts_with(prefix))
    {
        HolderKind::WindowsVolume
    } else {
        HolderKind::Other
    }
}

pub(super) struct SessionFacts {
    pub(super) active: bool,
    pub(super) user_logons: Vec<LogonId>,
}

pub(super) fn classify_state(
    logon: LogonId,
    record: Option<(u32, u32)>,
    sessions: Option<&HashMap<u32, SessionFacts>>,
) -> LogonState {
    let Some((logon_type, session)) = record else {
        return LogonState::SignedOut;
    };
    if session == 0 || !matches!(logon_type, 2 | 7 | 10 | 11 | 12 | 13) {
        return LogonState::Service;
    }
    let Some(sessions) = sessions else {
        return LogonState::Unknown;
    };
    let Some(facts) = sessions.get(&session) else {
        return LogonState::SignedOut;
    };
    // A reused RDP session id hosts exactly one remote logon pair; any other is a previous user's.
    if matches!(logon_type, 10 | 12)
        && !facts.user_logons.is_empty()
        && !facts.user_logons.contains(&logon)
    {
        return LogonState::SignedOut;
    }
    if facts.active {
        LogonState::Active
    } else {
        LogonState::Disconnected
    }
}

fn kind_name(kind: HolderKind) -> &'static str {
    match kind {
        HolderKind::VaultMount => "vault_mount",
        HolderKind::RamDisk => "ram_disk",
        HolderKind::WindowsVolume => "windows_volume",
        HolderKind::NetworkDrive => "network_drive",
        HolderKind::Subst => "subst",
        HolderKind::Other => "other",
    }
}

fn state_name(state: LogonState) -> &'static str {
    match state {
        LogonState::Machine => "machine",
        LogonState::Active => "active",
        LogonState::Disconnected => "disconnected",
        LogonState::SignedOut => "signed_out",
        LogonState::Service => "service",
        LogonState::Unknown => "unknown",
    }
}

/// Identity-free diagnostic fields: no SID, account name, share or folder path.
pub(crate) fn holder_diagnostics(
    letter: char,
    release: &LetterRelease,
    caller_sid: Option<&str>,
) -> BTreeMap<String, String> {
    release
        .checked
        .iter()
        .take(8)
        .enumerate()
        .map(|(index, (holder, released))| {
            let owner = match (&holder.owner_sid, caller_sid) {
                (None, _) => "none",
                (Some(owner), Some(caller)) if owner == caller => "caller",
                _ => "other",
            };
            let logon = holder.logon.map_or("-".to_owned(), |(low, high)| {
                format!("{:08x}-{low:08x}", high as u32)
            });
            (
                format!("{letter}{index}"),
                format!(
                    "kind={} device={} present={} logon={logon} state={} session={} owner={owner} action={}",
                    kind_name(holder.kind),
                    holder.device.as_deref().unwrap_or("-"),
                    holder.device_present,
                    state_name(holder.state),
                    holder.session.map_or("-".to_owned(), |session| session.to_string()),
                    if *released { "released" } else { "kept" },
                ),
            )
        })
        .collect()
}

pub(super) type OwnerFacts = (Option<String>, Option<u32>, LogonState);

/// `found` is `(letter, logon or machine-wide, link target if readable)`.
pub(super) fn build_holders(
    found: Vec<(char, Option<LogonId>, Option<String>)>,
    devices: &HashSet<String>,
    mut owner: impl FnMut(LogonId) -> OwnerFacts,
) -> HashMap<char, Vec<LetterHolder>> {
    let mut holders: HashMap<char, Vec<LetterHolder>> = HashMap::new();
    for (letter, logon, target) in found {
        let kind = target.as_deref().map_or(HolderKind::Other, holder_kind);
        let device = target
            .as_deref()
            .and_then(device_component)
            .map(str::to_owned);
        let device_present = !matches!(kind, HolderKind::VaultMount | HolderKind::RamDisk)
            || device
                .as_ref()
                .is_none_or(|name| devices.contains(&name.to_ascii_lowercase()));
        let (owner_sid, session, state) = match logon {
            None => (None, None, LogonState::Machine),
            Some(logon) => owner(logon),
        };
        holders.entry(letter).or_default().push(LetterHolder {
            logon,
            owner_sid,
            session,
            state,
            kind,
            device,
            device_present,
        });
    }
    holders
}

/// Only a link to a device that no longer exists is removed, whichever logon holds it.
pub(super) fn release_dead_links(
    holders: HashMap<char, Vec<LetterHolder>>,
    mut remove: impl FnMut(char, &LetterHolder) -> bool,
) -> HashMap<char, LetterRelease> {
    holders
        .into_iter()
        .map(|(letter, holders)| {
            let checked = holders
                .into_iter()
                .map(|holder| {
                    let released = holder.is_dead_device() && remove(letter, &holder);
                    (holder, released)
                })
                .collect();
            (letter, LetterRelease { checked })
        })
        .collect()
}

#[cfg_attr(test, allow(dead_code))]
#[path = "vault_letter_holders_native.rs"]
mod native;
pub(crate) use native::letter_holders;
#[cfg(not(test))]
pub(crate) use native::release_stale_letters;

#[cfg(test)]
#[path = "vault_letter_holders_tests.rs"]
mod tests;
