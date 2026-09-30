// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use windows_sys::Win32::NetworkManagement::NetManagement::{
    NetApiBufferFree, NetUserEnum, NetUserGetInfo, FILTER_NORMAL_ACCOUNT, UF_ACCOUNTDISABLE,
    USER_INFO_0, USER_INFO_23,
};

struct Buffer(*mut u8);
impl Drop for Buffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { NetApiBufferFree(self.0.cast()) };
        }
    }
}

/// Discover local accounts even before an administrator creates a Fleet group.
pub fn local_user_principals() -> Result<Vec<VaultKnownPrincipal>, VaultError> {
    let unavailable = || VaultError::PrincipalResolution("local Windows users".to_owned());
    let administrators = local_administrator_principals()?;
    let mut users = HashMap::new();
    let mut disabled = HashSet::new();
    let mut resume = 0u32;
    loop {
        let mut buffer = std::ptr::null_mut();
        let mut read = 0;
        let mut total = 0;
        let previous_resume = resume;
        let status = unsafe {
            NetUserEnum(
                std::ptr::null(),
                0,
                FILTER_NORMAL_ACCOUNT,
                &mut buffer,
                64 * 1024,
                &mut read,
                &mut total,
                &mut resume,
            )
        };
        let _buffer = Buffer(buffer);
        if !matches!(status, 0 | 234) || (read > 0 && buffer.is_null()) || read > 4096 {
            return Err(unavailable());
        }
        for index in 0..read as usize {
            // NetAPI owns an array of exactly `read` level-0 records until `_buffer` drops.
            let account = unsafe { &*buffer.cast::<USER_INFO_0>().add(index) };
            if account.usri0_name.is_null() {
                continue;
            }
            let mut details = std::ptr::null_mut();
            let status =
                unsafe { NetUserGetInfo(std::ptr::null(), account.usri0_name, 23, &mut details) };
            let _details = Buffer(details);
            // An account may disappear after enumeration; other failures are not an empty account list.
            if status == 2221 {
                continue;
            }
            if status != 0 || details.is_null() {
                return Err(unavailable());
            }
            // A successful level-23 lookup owns this record and its SID until `_details` drops.
            let account = unsafe { &*details.cast::<USER_INFO_23>() };
            let Some(sid) = sid_to_string(account.usri23_user_sid) else {
                continue;
            };
            if !local_user_is_enabled(account.usri23_flags) {
                disabled.insert(sid);
                continue;
            }
            let Some(label) = lookup_account_by_sid(&sid) else {
                continue;
            };
            if label.kind != PrincipalKind::User {
                continue;
            }
            let is_local_administrator = administrators.iter().any(|admin| admin.sid == sid);
            users.insert(
                sid.clone(),
                VaultKnownPrincipal {
                    sid,
                    display_name: label.display_name,
                    is_local_administrator,
                },
            );
        }
        if users.len() > 4096 {
            return Err(unavailable());
        }
        if status == 0 {
            break;
        }
        if resume == previous_resume || read == 0 {
            return Err(unavailable());
        }
    }
    // Domain administrators may be direct local-group members, not local SAM users.
    for administrator in administrators {
        if disabled.contains(&administrator.sid) {
            continue;
        }
        users.insert(administrator.sid.clone(), administrator);
    }
    let mut users = users.into_values().collect::<Vec<_>>();
    users.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    Ok(users)
}

fn local_user_is_enabled(flags: u32) -> bool {
    flags & UF_ACCOUNTDISABLE == 0
}

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_windows_accounts_are_not_selectable_even_when_administrators() {
        assert!(super::local_user_is_enabled(0));
        assert!(!super::local_user_is_enabled(super::UF_ACCOUNTDISABLE));
        assert!(!super::local_user_is_enabled(
            super::UF_ACCOUNTDISABLE | 0x10000
        ));
    }
    #[test]
    fn windows_users_are_discoverable_without_a_saved_fleet_directory() {
        let users = super::local_user_principals().expect("read local SAM users");
        eprintln!(
            "Discovered enabled Windows users: {} (administrators: {}, standard: {})",
            users.len(),
            users
                .iter()
                .filter(|user| user.is_local_administrator)
                .count(),
            users
                .iter()
                .filter(|user| !user.is_local_administrator)
                .count()
        );
        assert!(!users.is_empty());
        assert!(users.iter().all(
            |user| super::valid_windows_sid(&user.sid) && !user.display_name.trim().is_empty()
        ));
        let unique = users
            .iter()
            .map(|user| &user.sid)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), users.len());
    }
}
