// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{parse_logon_name, Api, LogonId};
use windows_sys::Win32::{
    Foundation::{LocalFree, LUID},
    Security::{
        Authentication::Identity::{
            LsaFreeReturnBuffer, LsaGetLogonSessionData, SECURITY_LOGON_SESSION_DATA,
        },
        Authorization::ConvertStringSidToSidW,
        GetLengthSid, IsValidSid,
    },
};

#[derive(PartialEq, Eq)]
struct OwnerSession {
    sid: Vec<u8>,
    session: u32,
}

fn owner_session(logon: LogonId) -> Result<Option<OwnerSession>, ()> {
    if logon == (0x3e7, 0) {
        return Ok(Some(OwnerSession {
            sid: Vec::new(),
            session: 0,
        }));
    }
    let id = LUID {
        LowPart: logon.0,
        HighPart: logon.1,
    };
    let mut data = std::ptr::null_mut();
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
            let length = GetLengthSid((*data).Sid) as usize;
            if !(8..=68).contains(&length) {
                return Err(());
            }
            Ok(Some(OwnerSession {
                sid: std::slice::from_raw_parts((*data).Sid.cast::<u8>(), length).to_vec(),
                session: (*data).Session,
            }))
        })();
        LsaFreeReturnBuffer(data.cast());
        result
    }
}

fn saved_owner(sid: &str, session: u32) -> Result<OwnerSession, ()> {
    if session == 0 || sid.len() > 256 || sid.contains('\0') {
        return Err(());
    }
    let wide: Vec<u16> = sid.encode_utf16().chain(Some(0)).collect();
    let mut native = std::ptr::null_mut();
    unsafe {
        if ConvertStringSidToSidW(wide.as_ptr(), &mut native) == 0 {
            return Err(());
        }
        let length = GetLengthSid(native) as usize;
        let result = if IsValidSid(native) != 0 && (8..=68).contains(&length) {
            Ok(OwnerSession {
                sid: std::slice::from_raw_parts(native.cast::<u8>(), length).to_vec(),
                session,
            })
        } else {
            Err(())
        };
        LocalFree(native);
        result
    }
}

fn could_share_namespace(expected: &OwnerSession, actual: Option<&OwnerSession>) -> bool {
    // Missing LSA data cannot exclude an old paired alias. Keep it in the
    // inspection scope; removal still needs the exact dead encrypted target.
    actual.is_none_or(|actual| actual == expected)
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn verified_owner(logon: LogonId, caller_sid: &str, session: u32) -> Result<bool, ()> {
    Ok(positively_matches_owner(&saved_owner(caller_sid, session)?, owner_session(logon)?.as_ref()))
}

fn positively_matches_owner(expected: &OwnerSession, actual: Option<&OwnerSession>) -> bool {
    actual == Some(expected)
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn owned_logons(logon: LogonId, caller_sid: &str, session: u32) -> Result<Vec<LogonId>, ()> {
    if !verified_owner(logon, caller_sid, session)? { return Err(()); }
    let mut owned = vec![logon];
    for (name, kind) in Api::load()?.entries(r"\Sessions\0\DosDevices", false)? {
        if kind != "Directory" { continue; }
        if let Some(other) = parse_logon_name(&name) {
            if other != logon && verified_owner(other, caller_sid, session)? { owned.push(other); }
        }
    }
    Ok(owned)
}

pub(super) fn related_logons(
    logon: LogonId,
    caller_sid: &str,
    session: u32,
) -> Result<Vec<LogonId>, ()> {
    let mut related = vec![logon];
    let expected = saved_owner(caller_sid, session)?;
    if !could_share_namespace(&expected, owner_session(logon)?.as_ref()) {
        return Err(());
    }
    for (name, kind) in Api::load()?.entries(r"\Sessions\0\DosDevices", false)? {
        if kind != "Directory" {
            continue;
        }
        let Some(other) = parse_logon_name(&name) else {
            continue;
        };
        if other != logon && could_share_namespace(&expected, owner_session(other)?.as_ref()) {
            related.push(other);
        }
    }
    Ok(related)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_owner_scope_requires_positive_identity_and_original_session() {
        let expected = OwnerSession { sid: vec![1, 2, 3], session: 9 };
        assert!(positively_matches_owner(&expected, Some(&expected)));
        assert!(!positively_matches_owner(&expected, None));
        assert!(!positively_matches_owner(&expected, Some(&OwnerSession { sid: vec![1, 2, 4], session: 9 })));
        assert!(!positively_matches_owner(&expected, Some(&OwnerSession { sid: vec![1, 2, 3], session: 10 })));
    }
    #[test]
    fn paired_alias_scope_requires_both_same_account_and_same_session() {
        let owner = OwnerSession {
            sid: vec![1, 2, 3],
            session: 9,
        };
        assert!(could_share_namespace(&owner, None));
        assert!(could_share_namespace(&owner, Some(&owner)));
        assert!(!could_share_namespace(
            &owner,
            Some(&OwnerSession {
                sid: vec![1, 2, 4],
                session: 9
            })
        ));
        assert!(!could_share_namespace(
            &owner,
            Some(&OwnerSession {
                sid: vec![1, 2, 3],
                session: 11
            })
        ));
        assert!(
            owner
                == OwnerSession {
                    sid: vec![1, 2, 3],
                    session: 9
                }
        );
        assert!(
            owner
                != OwnerSession {
                    sid: vec![1, 2, 4],
                    session: 9
                }
        );
        assert!(
            owner
                != OwnerSession {
                    sid: vec![1, 2, 3],
                    session: 11
                }
        );
    }
    #[test]
    fn missing_lsa_uses_validated_saved_owner_without_omitting_unknown_aliases() {
        let owner = saved_owner("S-1-5-21-123-456-789-1001", 9).unwrap();
        assert!(could_share_namespace(&owner, None));
        assert!(saved_owner("not a Windows SID", 9).is_err());
        assert!(saved_owner("S-1-5-21-123-456-789-1001", 0).is_err());
    }
}
