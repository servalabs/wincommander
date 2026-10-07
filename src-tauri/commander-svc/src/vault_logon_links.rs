// SPDX-License-Identifier: AGPL-3.0-or-later
//! Remove dead encrypted aliases only from Windows logons proven to have ended.
use super::{Api, GlobalEncryptedLinkCleanup};

#[path = "vault_logon_liveness.rs"]
mod liveness;
pub(crate) use liveness::logon_ended;
#[path = "vault_logon_links_native.rs"]
mod native;
#[path = "vault_logon_links_scope.rs"]
mod scope;
#[cfg(test)]
#[path = "vault_logon_links_tests.rs"]
mod tests;

type LogonId = (u32, i32);

fn parse_logon_name(name: &str) -> Option<LogonId> {
    let bytes = name.as_bytes();
    if bytes.len() != 17
        || bytes[8] != b'-'
        || !bytes[..8]
            .iter()
            .chain(&bytes[9..])
            .all(u8::is_ascii_hexdigit)
    {
        return None;
    }
    Some((
        u32::from_str_radix(&name[9..], 16).ok()?,
        u32::from_str_radix(&name[..8], 16).ok()? as i32,
    ))
}

fn link_path(logon: LogonId, letter: char) -> Result<String, ()> {
    if !letter.is_ascii_uppercase() {
        return Err(());
    }
    Ok(format!(
        r"\Sessions\0\DosDevices\{:08x}-{:08x}\{letter}:",
        logon.1 as u32, logon.0
    ))
}

fn encrypted_slot(target: &str) -> Option<u8> {
    let lower = target.to_ascii_lowercase();
    let suffix = lower
        .strip_prefix(r"\device\veracryptvolume")
        .or_else(|| lower.strip_prefix(r"\device\truecryptvolume"))?;
    let bytes = suffix.as_bytes();
    (bytes.len() == 1 && bytes[0].is_ascii_lowercase()).then(|| bytes[0] - b'a')
}

fn target_absent(api: &Api, target: &str) -> Result<bool, ()> {
    if encrypted_slot(target).is_none() {
        return Ok(false);
    }
    let name = &target[r"\Device\".len()..];
    Ok(!api
        .entries(r"\Device", false)?
        .iter()
        .any(|(entry, _)| entry.eq_ignore_ascii_case(name)))
}

fn remove_if_still_stale(
    expected: &str,
    mut ended: impl FnMut() -> Result<bool, ()>,
    mut absent: impl FnMut() -> Result<bool, ()>,
    mut target: impl FnMut() -> Result<String, ()>,
    mut remove: impl FnMut() -> Result<(), ()>,
) -> Result<bool, ()> {
    for _ in 0..2 {
        if target()? != expected || !ended()? || !absent()? {
            return Ok(false);
        }
    }
    remove()?;
    Ok(true)
}

fn cleanup_link(
    path: &str,
    expected_slot: Option<u8>,
    authorized: impl FnMut() -> Result<bool, ()>,
) -> Result<GlobalEncryptedLinkCleanup, ()> {
    use GlobalEncryptedLinkCleanup::{Absent, Removed, Retained};
    let api = Api::load()?;
    let native = native::LinkApi::load()?;
    let Some(inspection) = native.open(path, false)? else {
        return Ok(Absent);
    };
    let target = native.target(&inspection)?;
    let Some(slot) = encrypted_slot(&target) else {
        return Ok(Retained);
    };
    if expected_slot.is_some_and(|expected| {
        expected != slot
            || !target.eq_ignore_ascii_case(&format!(
                r"\Device\VeraCryptVolume{}",
                char::from(b'A' + expected)
            ))
    }) {
        return Ok(Retained);
    }
    // Holding the inspection handle pins the name while requesting DELETE access.
    let link = native.open(path, true)?.ok_or(())?;
    if native.target(&link)? != target {
        return Ok(Retained);
    }
    drop(inspection);
    let _guard = wincmd_volume::VolumeOperationGuard::acquire_slot(slot).map_err(|_| ())?;
    // Delete the pinned object, never a name looked up again after inspection.
    // A replacement alias is preserved even if an external program races cleanup.
    if !remove_if_still_stale(
        &target,
        authorized,
        || target_absent(&api, &target),
        || native.target(&link),
        || native.remove(&link),
    )? {
        return Ok(Retained);
    }
    drop(link);
    Ok(if native.open(path, false)?.is_none() {
        Removed
    } else {
        Retained
    })
}

pub(crate) fn cleanup_ended_logon_encrypted_link(
    logon: LogonId,
    letter: char,
    internal_drive: u8,
    caller_sid: &str,
    session: u32,
) -> Result<GlobalEncryptedLinkCleanup, ()> {
    if internal_drive > 25 {
        return Err(());
    }
    let related = scope::related_logons(logon, caller_sid, session)?;
    let mut result = cleanup_link(&link_path(logon, letter)?, Some(internal_drive), || {
        logon_ended(logon)
    })?;
    if result == GlobalEncryptedLinkCleanup::Retained {
        return Ok(result);
    }
    let expected = format!(
        r"\Device\VeraCryptVolume{}",
        char::from(b'A' + internal_drive)
    );
    let native = native::LinkApi::load()?;
    for other in related.into_iter().filter(|other| *other != logon) {
        let path = link_path(other, letter)?;
        let Some(link) = native.open(&path, false)? else {
            continue;
        };
        if !native.target(&link)?.eq_ignore_ascii_case(&expected) {
            continue;
        }
        drop(link);
        let cleanup = cleanup_link(&path, Some(internal_drive), || logon_ended(other))?;
        if cleanup == GlobalEncryptedLinkCleanup::Retained {
            return Ok(cleanup);
        }
        if cleanup == GlobalEncryptedLinkCleanup::Removed {
            result = cleanup;
        }
    }
    Ok(result)
}

pub(crate) fn ended_logon_encrypted_link_absent(
    logon: LogonId,
    letter: char,
    internal_drive: u8,
    caller_sid: &str,
    session: u32,
) -> Result<bool, ()> {
    if internal_drive > 25 {
        return Err(());
    }
    if !logon_ended(logon)? {
        return Ok(false);
    }
    let native = native::LinkApi::load()?;
    let expected = format!(
        r"\Device\VeraCryptVolume{}",
        char::from(b'A' + internal_drive)
    );
    related_links_absent(
        logon,
        &scope::related_logons(logon, caller_sid, session)?,
        &expected,
        |other| {
            let Some(link) = native.open(&link_path(other, letter)?, false)? else {
                return Ok(None);
            };
            native.target(&link).map(Some)
        },
    )
}

fn related_links_absent(
    primary: LogonId,
    related: &[LogonId],
    expected: &str,
    mut query: impl FnMut(LogonId) -> Result<Option<String>, ()>,
) -> Result<bool, ()> {
    for &logon in related {
        if let Some(target) = query(logon)? {
            if logon == primary || target.eq_ignore_ascii_case(expected) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(crate) fn release_orphaned_logon_encrypted_links() -> Result<usize, ()> {
    let api = Api::load()?;
    let mut removed = 0;
    for (name, kind) in api.entries(r"\Sessions\0\DosDevices", false)? {
        if kind != "Directory" {
            continue;
        }
        let Some(logon) = parse_logon_name(&name) else {
            continue;
        };
        if logon_ended(logon) != Ok(true) {
            continue;
        }
        let Ok(entries) = api.entries(
            &format!(
                r"\Sessions\0\DosDevices\{:08x}-{:08x}",
                logon.1 as u32, logon.0
            ),
            true,
        ) else {
            continue;
        };
        for (name, kind) in entries {
            let bytes = name.as_bytes();
            if kind != "SymbolicLink"
                || bytes.len() != 2
                || bytes[1] != b':'
                || !bytes[0].is_ascii_alphabetic()
            {
                continue;
            }
            // An unreadable or racing alias remains blocked; it must not prevent
            // independent, provably stale aliases from being repaired.
            if cleanup_link(
                &link_path(logon, char::from(bytes[0].to_ascii_uppercase()))?,
                None,
                || logon_ended(logon),
            ) == Ok(GlobalEncryptedLinkCleanup::Removed)
            {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

pub(crate) fn release_orphaned_caller_encrypted_links(logon: LogonId) -> Result<usize, ()> {
    let mut removed = 0;
    // The service supplies the pipe peer's authenticated LUID, never a request argument.
    for letter in 'A'..='Z' {
        if cleanup_link(&link_path(logon, letter)?, None, || Ok(true))
            == Ok(GlobalEncryptedLinkCleanup::Removed)
        {
            removed += 1;
        }
    }
    Ok(removed)
}

pub(super) fn cleanup_global_link(letter: char) -> Result<GlobalEncryptedLinkCleanup, ()> {
    if !letter.is_ascii_uppercase() {
        return Err(());
    }
    cleanup_link(&format!(r"\GLOBAL??\{letter}:"), None, || Ok(true))
}
