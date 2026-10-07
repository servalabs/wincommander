// SPDX-License-Identifier: AGPL-3.0-or-later
use super::LogonId;
use std::collections::HashSet;
use windows_sys::Win32::{
    Foundation::LUID,
    Security::Authentication::Identity::{
        LsaFreeReturnBuffer, LsaGetLogonSessionData, SECURITY_LOGON_SESSION_DATA,
    },
    System::RemoteDesktop::{WTSEnumerateSessionsW, WTSFreeMemory, WTS_CURRENT_SERVER_HANDLE},
};

fn present_sessions() -> Result<HashSet<u32>, ()> {
    let mut rows = std::ptr::null_mut();
    let mut count = 0;
    unsafe {
        if WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut rows, &mut count) == 0 {
            return Err(());
        }
        let result = if count > 65_536 || (count > 0 && rows.is_null()) {
            Err(())
        } else if count == 0 {
            Ok(HashSet::new())
        } else {
            Ok(std::slice::from_raw_parts(rows, count as usize)
                .iter()
                .map(|row| row.SessionId)
                .collect())
        };
        if !rows.is_null() {
            WTSFreeMemory(rows.cast());
        }
        result
    }
}

pub(super) fn interactive_logon_ended(
    logon_type: u32,
    session: u32,
    present: &HashSet<u32>,
) -> bool {
    session != 0 && matches!(logon_type, 2 | 7 | 10 | 11 | 12 | 13) && !present.contains(&session)
}

fn classify_logon(
    data: Result<Option<(u32, u32)>, ()>,
    present: impl FnOnce() -> Result<HashSet<u32>, ()>,
) -> Result<bool, ()> {
    let Some((logon_type, session)) = data? else {
        return Ok(true);
    };
    if session == 0 || !matches!(logon_type, 2 | 7 | 10 | 11 | 12 | 13) {
        return Ok(false);
    }
    // A present session can contain primary, linked UAC and secondary RunAs tokens.
    // A different primary token alone is not proof that another logon has ended.
    Ok(interactive_logon_ended(logon_type, session, &present()?))
}

fn logon_data(logon: LogonId) -> Result<Option<(u32, u32)>, ()> {
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
        let result =
            if (*data).Size as usize >= std::mem::offset_of!(SECURITY_LOGON_SESSION_DATA, Sid) {
                Ok(Some(((*data).LogonType, (*data).Session)))
            } else {
                Err(())
            };
        LsaFreeReturnBuffer(data.cast());
        result
    }
}

pub(crate) fn logon_ended(logon: LogonId) -> Result<bool, ()> {
    // A retained token keeps LSA data alive after Windows has ended the session.
    classify_logon(logon_data(logon), present_sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lsa_retained_after_signout_does_not_keep_logon_alive() {
        assert_eq!(
            classify_logon(Ok(Some((10, 9))), || Ok(HashSet::from([11]))),
            Ok(true)
        );
        assert_eq!(
            classify_logon(Ok(None), || panic!("missing LSA proves ended")),
            Ok(true)
        );
    }
    #[test]
    fn present_sessions_keep_primary_linked_secondary_and_disconnected_logons() {
        for logon_type in [2, 7, 10, 11, 12, 13] {
            assert_eq!(
                classify_logon(Ok(Some((logon_type, 9))), || Ok(HashSet::from([9]))),
                Ok(false)
            );
        }
    }
    #[test]
    fn unknown_session_state_never_proves_signout() {
        assert_eq!(classify_logon(Err(()), || panic!()), Err(()));
        assert_eq!(classify_logon(Ok(Some((10, 9))), || Err(())), Err(()));
    }
    #[test]
    fn service_and_unknown_logon_types_cannot_be_reclaimed() {
        for (kind, session) in [(5, 9), (3, 9), (10, 0)] {
            assert_eq!(
                classify_logon(Ok(Some((kind, session))), || panic!()),
                Ok(false)
            );
        }
    }
}
