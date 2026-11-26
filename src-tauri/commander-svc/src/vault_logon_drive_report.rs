// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only mapping evidence. Paths, SIDs and logon IDs stay in the service.
use super::{
    native::LinkApi,
    parse_logon_name,
    scope::{logon_relationship, LogonRelationship},
    Api, LogonId,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionDriveMapping {
    CurrentSession,
    CurrentSessionNetwork,
    OtherSession,
    OtherAccount,
}

pub(super) fn network_target(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    [
        r"\device\mup",
        r"\device\lanmanredirector",
        r"\device\webdavredirector",
    ]
    .into_iter()
    .any(|prefix| {
        lower
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('\\'))
    })
}

pub(crate) fn session_drive_mappings(
    caller: LogonId,
    sid: &str,
    session: u32,
    requested: &HashSet<char>,
) -> Result<HashMap<char, SessionDriveMapping>, ()> {
    let api = Api::load()?;
    let links = LinkApi::load()?;
    let root = r"\Sessions\0\DosDevices";
    let mut mappings = HashMap::new();
    for (name, kind) in api.entries(root, false)? {
        if kind != "Directory" {
            continue;
        }
        let Some(logon) = parse_logon_name(&name) else {
            continue;
        };
        let namespace = format!("{root}\\{name}");
        let Ok(entries) = api.entries(&namespace, true) else {
            continue;
        };
        let mut observed = entries
            .iter()
            .filter_map(|(name, kind)| {
                if kind != "SymbolicLink" {
                    return None;
                }
                let letter = super::super::drive_letter(name)?;
                requested.contains(&letter).then_some((letter, name))
            })
            .peekable();
        if observed.peek().is_none() {
            continue;
        }
        let relationship = if logon == caller {
            Some(LogonRelationship::CurrentSession)
        } else {
            logon_relationship(logon, sid, session)
        };
        let Some(relationship) = relationship else {
            continue;
        };
        for (letter, name) in observed {
            let path = format!("{namespace}\\{name}");
            let Ok(Some(link)) = links.open(&path, false) else {
                continue;
            };
            let mapping = match relationship {
                LogonRelationship::CurrentSession => {
                    if links
                        .target(&link)
                        .is_ok_and(|target| network_target(&target))
                    {
                        SessionDriveMapping::CurrentSessionNetwork
                    } else {
                        SessionDriveMapping::CurrentSession
                    }
                }
                LogonRelationship::OtherSession => SessionDriveMapping::OtherSession,
                LogonRelationship::OtherAccount => SessionDriveMapping::OtherAccount,
            };
            mappings
                .entry(letter)
                .and_modify(|existing| *existing = prefer_mapping(*existing, mapping))
                .or_insert(mapping);
        }
    }
    Ok(mappings)
}

fn prefer_mapping(first: SessionDriveMapping, second: SessionDriveMapping) -> SessionDriveMapping {
    fn rank(mapping: SessionDriveMapping) -> u8 {
        match mapping {
            SessionDriveMapping::CurrentSession => 3,
            SessionDriveMapping::CurrentSessionNetwork => 4,
            SessionDriveMapping::OtherSession => 1,
            SessionDriveMapping::OtherAccount => 2,
        }
    }
    if rank(first) >= rank(second) {
        first
    } else {
        second
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_mapping_uses_exact_windows_device_family() {
        assert!(network_target(r"\Device\Mup\server\share"));
        assert!(network_target(r"\Device\LanmanRedirector\server\share"));
        assert!(!network_target(r"\Device\MupOther\server\share"));
        assert!(!network_target(r"\Device\HarddiskVolume3\data"));
    }

    #[test]
    fn reports_a_current_network_drive_when_another_account_also_has_the_letter() {
        use SessionDriveMapping::*;
        assert_eq!(
            prefer_mapping(CurrentSessionNetwork, OtherAccount),
            CurrentSessionNetwork
        );
        assert_eq!(prefer_mapping(OtherSession, OtherAccount), OtherAccount);
    }
}
