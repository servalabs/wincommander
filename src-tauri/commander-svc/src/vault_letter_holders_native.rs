// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native object-namespace, LSA and WTS reads for drive-letter holders.
use super::super::{
    encrypted_slot, link_path, native::LinkApi, parse_logon_name, remove_if_still_stale, Api,
};
use super::{
    build_holders, classify_state, device_component, holder_kind, release_dead_links, HolderKind,
    LetterHolder, LetterRelease, LogonId, LogonState, SessionFacts,
};
use std::collections::HashMap;
use std::collections::HashSet;
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, LUID},
    Security::{
        Authentication::Identity::{
            LsaFreeReturnBuffer, LsaGetLogonSessionData, SECURITY_LOGON_SESSION_DATA,
        },
        Authorization::ConvertSidToStringSidW,
        IsValidSid,
    },
    System::RemoteDesktop::{
        WTSActive, WTSEnumerateSessionsW, WTSFreeMemory, WTSQueryUserToken,
        WTS_CURRENT_SERVER_HANDLE,
    },
};

fn read_target(links: &LinkApi, path: &str) -> Option<Option<String>> {
    match links.open(path, false) {
        Ok(None) => None,
        Ok(Some(link)) => Some(links.target(&link).ok()),
        // An unreadable link is reported as an unknown holder and never removed.
        Err(()) => Some(None),
    }
}

fn device_names(api: &Api) -> Result<HashSet<String>, ()> {
    Ok(api
        .entries(r"\Device", false)?
        .into_iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect())
}

pub(crate) fn letter_holders(
    letters: &HashSet<char>,
) -> Result<HashMap<char, Vec<LetterHolder>>, ()> {
    if letters.iter().any(|letter| !('D'..='Z').contains(letter)) {
        return Err(());
    }
    let api = Api::load()?;
    let links = LinkApi::load()?;
    let mut found = Vec::new();
    for &letter in letters {
        if let Some(target) = read_target(&links, &format!(r"\GLOBAL??\{letter}:")) {
            found.push((letter, None, target));
        }
    }
    let root = r"\Sessions\0\DosDevices";
    for (name, kind) in api.entries(root, false)? {
        let Some(logon) = parse_logon_name(&name).filter(|_| kind == "Directory") else {
            continue;
        };
        for (entry, _) in api.entries(&format!(r"{root}\{name}"), true)? {
            let Some(letter) =
                super::super::super::drive_letter(&entry).filter(|l| letters.contains(l))
            else {
                continue;
            };
            if let Some(target) = read_target(&links, &link_path(logon, letter)?) {
                found.push((letter, Some(logon), target));
            }
        }
    }
    let needs_devices = found.iter().any(|(_, _, target)| {
        target.as_deref().is_some_and(|target| {
            matches!(
                holder_kind(target),
                HolderKind::VaultMount | HolderKind::RamDisk
            )
        })
    });
    let devices = if needs_devices {
        device_names(&api)?
    } else {
        HashSet::new()
    };
    let sessions = session_facts().ok();
    let mut owners = HashMap::new();
    Ok(build_holders(found, &devices, |logon| {
        owners
            .entry(logon)
            .or_insert_with(|| logon_facts(logon, sessions.as_ref()))
            .clone()
    }))
}

pub(crate) fn release_stale_letters(
    letters: &HashSet<char>,
) -> Result<HashMap<char, LetterRelease>, ()> {
    Ok(release_dead_links(
        letter_holders(letters)?,
        |letter, holder| remove_holder(letter, holder).unwrap_or(false),
    ))
}

fn remove_holder(letter: char, holder: &LetterHolder) -> Result<bool, ()> {
    if !holder.is_dead_device() {
        return Ok(false);
    }
    let path = match holder.logon {
        Some(logon) => link_path(logon, letter)?,
        None => format!(r"\GLOBAL??\{letter}:"),
    };
    let api = Api::load()?;
    let links = LinkApi::load()?;
    let Some(inspection) = links.open(&path, false)? else {
        return Ok(false);
    };
    let target = links.target(&inspection)?;
    if holder_kind(&target) != holder.kind {
        return Ok(false);
    }
    // Holding the inspection handle pins the name while requesting DELETE access.
    let link = links.open(&path, true)?.ok_or(())?;
    if links.target(&link)? != target {
        return Ok(false);
    }
    drop(inspection);
    let _guard = match encrypted_slot(&target) {
        Some(slot) => {
            Some(wincmd_volume::VolumeOperationGuard::acquire_slot(slot).map_err(|_| ())?)
        }
        None => None,
    };
    let device = device_component(&target).ok_or(())?.to_ascii_lowercase();
    let still_stale = || Ok(!device_names(&api)?.contains(&device));
    if !remove_if_still_stale(
        &target,
        still_stale,
        || Ok(true),
        || links.target(&link),
        || links.remove(&link),
    )? {
        return Ok(false);
    }
    drop(link);
    Ok(links.open(&path, false)?.is_none())
}

fn session_facts() -> Result<HashMap<u32, SessionFacts>, ()> {
    let mut rows = std::ptr::null_mut();
    let mut count = 0;
    // SAFETY: the buffer is borrowed only after the bounds/null checks and freed once.
    let sessions = unsafe {
        if WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut rows, &mut count) == 0 {
            return Err(());
        }
        let result = if count > 65_536 || (count > 0 && rows.is_null()) {
            Err(())
        } else if count == 0 {
            Ok(Vec::new())
        } else {
            Ok(std::slice::from_raw_parts(rows, count as usize)
                .iter()
                .map(|row| (row.SessionId, row.State == WTSActive))
                .collect::<Vec<_>>())
        };
        if !rows.is_null() {
            WTSFreeMemory(rows.cast());
        }
        result?
    };
    Ok(sessions
        .into_iter()
        .map(|(id, active)| {
            (
                id,
                SessionFacts {
                    active,
                    user_logons: session_user_logons(id),
                },
            )
        })
        .collect())
}

fn session_user_logons(session: u32) -> Vec<LogonId> {
    let mut token = std::ptr::null_mut();
    // SAFETY: the service holds SeTcbPrivilege; the returned token is closed below.
    if unsafe { WTSQueryUserToken(session, &mut token) } == 0 || token.is_null() {
        return Vec::new();
    }
    let logons = [
        crate::vault_mount::authenticated_logon_id(token),
        crate::vault_mount::linked_authentication_id(token),
    ]
    .into_iter()
    .flatten()
    .collect();
    unsafe { CloseHandle(token) };
    logons
}

fn logon_record(logon: LogonId) -> Result<Option<(u32, u32, String)>, ()> {
    let id = LUID {
        LowPart: logon.0,
        HighPart: logon.1,
    };
    let mut data = std::ptr::null_mut();
    // SAFETY: LSA owns `data` until LsaFreeReturnBuffer; its size is checked before reading Sid.
    unsafe {
        let status = LsaGetLogonSessionData(&id, &mut data);
        if status as u32 == 0xc000005f {
            return Ok(None);
        }
        if status != 0 || data.is_null() {
            return Err(());
        }
        let result = (|| {
            let required = std::mem::offset_of!(SECURITY_LOGON_SESSION_DATA, Sid)
                + std::mem::size_of::<*mut std::ffi::c_void>();
            if ((*data).Size as usize) < required
                || (*data).Sid.is_null()
                || IsValidSid((*data).Sid) == 0
            {
                return Err(());
            }
            let mut text = std::ptr::null_mut();
            if ConvertSidToStringSidW((*data).Sid, &mut text) == 0 || text.is_null() {
                return Err(());
            }
            let length = (0..256).find(|&index| *text.add(index) == 0);
            let sid = length.and_then(|length| {
                String::from_utf16(std::slice::from_raw_parts(text, length)).ok()
            });
            LocalFree(text.cast());
            Ok(Some(((*data).LogonType, (*data).Session, sid.ok_or(())?)))
        })();
        LsaFreeReturnBuffer(data.cast());
        result
    }
}

fn logon_facts(
    logon: LogonId,
    sessions: Option<&HashMap<u32, SessionFacts>>,
) -> (Option<String>, Option<u32>, LogonState) {
    match logon_record(logon) {
        Ok(None) => (None, None, LogonState::SignedOut),
        Ok(Some((logon_type, session, sid))) => (
            Some(sid),
            Some(session),
            classify_state(logon, Some((logon_type, session)), sessions),
        ),
        Err(()) => (None, None, LogonState::Unknown),
    }
}
