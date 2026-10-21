// SPDX-License-Identifier: AGPL-3.0-or-later
//! Membership checks retain the exact captured peer token and local group policy.
use super::VAULT_POLICY_ADMIN_GROUP;
use anyhow::Result;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        AllocateAndInitializeSid, CheckTokenMembership, DuplicateToken, EqualSid, FreeSid,
        GetTokenInformation, LookupAccountNameW, SecurityIdentification, TokenUser, PSID,
        SECURITY_NT_AUTHORITY, SID_NAME_USE, TOKEN_USER,
    },
    System::SystemServices::{DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID},
};

pub(super) fn token_is_privileged(token: HANDLE) -> Result<bool> {
    unsafe {
        // CheckTokenMembership accepts an impersonation token, not a primary
        // token. Duplicate the exact authenticated peer token; never reopen
        // a process by PID on the live connection path.
        let mut membership_token: HANDLE = std::ptr::null_mut();
        if DuplicateToken(token, SecurityIdentification, &mut membership_token) == 0 {
            anyhow::bail!("DuplicateToken failed");
        }
        let _guard_membership_token = HandleGuard(membership_token);
        if is_admin_token(membership_token)? {
            return Ok(true);
        }
        is_local_system_token(membership_token)
    }
}

pub(super) fn caller_has_vault_policy_capability_token(token: HANDLE) -> Result<bool> {
    unsafe {
        let mut membership_token = std::ptr::null_mut();
        if DuplicateToken(token, SecurityIdentification, &mut membership_token) == 0 {
            anyhow::bail!("DuplicateToken failed");
        }
        let _membership_token = HandleGuard(membership_token);
        let name = std::ffi::OsStr::new(VAULT_POLICY_ADMIN_GROUP)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut sid_len = 0u32;
        let mut domain_len = 0u32;
        let mut use_type = 0i32 as SID_NAME_USE;
        LookupAccountNameW(
            std::ptr::null(),
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut sid_len,
            std::ptr::null_mut(),
            &mut domain_len,
            &mut use_type,
        );
        if sid_len == 0 {
            return Ok(false);
        }
        let mut sid = vec![0u8; sid_len as usize];
        let mut domain = vec![0u16; domain_len as usize + 1];
        if LookupAccountNameW(
            std::ptr::null(),
            name.as_ptr(),
            sid.as_mut_ptr() as PSID,
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut use_type,
        ) == 0
        {
            return Ok(false);
        }
        let mut member = 0;
        if CheckTokenMembership(membership_token, sid.as_mut_ptr() as PSID, &mut member) == 0 {
            return Ok(false);
        }
        Ok(member != 0)
    }
}

unsafe fn is_admin_token(token: HANDLE) -> Result<bool> {
    let mut admin_sid: PSID = std::ptr::null_mut();
    // SECURITY_NT_AUTHORITY + SECURITY_BUILTIN_DOMAIN_RID + DOMAIN_ALIAS_RID_ADMINS
    let nt_authority = SECURITY_NT_AUTHORITY;
    let result = AllocateAndInitializeSid(
        &nt_authority,
        2,
        SECURITY_BUILTIN_DOMAIN_RID as u32,
        DOMAIN_ALIAS_RID_ADMINS as u32,
        0,
        0,
        0,
        0,
        0,
        0,
        &mut admin_sid,
    );
    if result == 0 {
        anyhow::bail!("AllocateAndInitializeSid (Admins) failed");
    }
    let _sid_guard = SidGuard(admin_sid);

    let mut is_member: windows_sys::core::BOOL = 0;
    let ok = CheckTokenMembership(token, admin_sid, &mut is_member);
    if ok == 0 {
        anyhow::bail!("CheckTokenMembership failed");
    }
    Ok(is_member != 0)
}

unsafe fn is_local_system_token(token: HANDLE) -> Result<bool> {
    // Query TOKEN_USER to get the user SID from the token.
    let mut needed: u32 = 0;
    // First call: get required buffer size.
    GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);

    let mut buf = vec![0u8; needed as usize];
    let ok = GetTokenInformation(
        token,
        TokenUser,
        buf.as_mut_ptr() as *mut _,
        needed,
        &mut needed,
    );
    if ok == 0 {
        anyhow::bail!("GetTokenInformation(TokenUser) failed");
    }

    let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
    let user_sid = token_user.User.Sid;

    // Build LocalSystem SID (S-1-5-18) for comparison.
    let mut system_sid: PSID = std::ptr::null_mut();
    let nt_authority = SECURITY_NT_AUTHORITY;
    let result = AllocateAndInitializeSid(
        &nt_authority,
        1,
        18u32, // SECURITY_LOCAL_SYSTEM_RID
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        &mut system_sid,
    );
    if result == 0 {
        anyhow::bail!("AllocateAndInitializeSid (LocalSystem) failed");
    }
    let _sid_guard = SidGuard(system_sid);

    let equal = EqualSid(user_sid, system_sid) != 0;
    Ok(equal)
}

// ── SECURITY_ATTRIBUTES builder ──────────────────────────────────────────────

struct HandleGuard(HANDLE);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct SidGuard(PSID);
impl Drop for SidGuard {
    fn drop(&mut self) {
        unsafe { FreeSid(self.0) };
    }
}

// ── Shared test fixtures (used by both `tests` and `integration` below) ────
