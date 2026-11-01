// SPDX-License-Identifier: AGPL-3.0-or-later
//! Captured pipe identity owns its verified token until the connection ends.
use anyhow::Result;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        GetTokenInformation, RevertToSelf, TokenSessionId, TokenStatistics, TokenUser,
        TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_STATISTICS, TOKEN_USER,
    },
    System::{
        Pipes::{GetNamedPipeClientProcessId, ImpersonateNamedPipeClient},
        Threading::{
            GetCurrentThread, OpenProcess, OpenProcessToken, OpenThreadToken,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

pub(crate) struct AuthenticatedPipePeer {
    pub(super) client_pid: u32,
    pub(super) token: HANDLE,
    pub(super) session_id: u32,
    pub(super) caller_sid: String,
    pub(super) authentication_id: (u32, i32),
}

unsafe impl Send for AuthenticatedPipePeer {}
unsafe impl Sync for AuthenticatedPipePeer {}

impl Drop for AuthenticatedPipePeer {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.token) };
    }
}

impl AuthenticatedPipePeer {
    pub(crate) fn client_pid(&self) -> u32 {
        self.client_pid
    }

    pub(crate) fn token(&self) -> HANDLE {
        self.token
    }

    pub(crate) fn session_id(&self) -> u32 {
        self.session_id
    }

    pub(crate) fn caller_sid(&self) -> &str {
        &self.caller_sid
    }

    pub(crate) fn authentication_id(&self) -> (u32, i32) {
        self.authentication_id
    }
}

/// Capture one authenticated token directly from the connected named-pipe
/// client. The token remains owned for the whole connection, so later Vault
/// authorization and broker launch cannot race a reused process ID.
pub(crate) fn capture_authenticated_pipe_peer(
    pipe_handle: HANDLE,
) -> Result<AuthenticatedPipePeer> {
    unsafe {
        let mut client_pid = 0u32;
        if GetNamedPipeClientProcessId(pipe_handle, &mut client_pid) == 0 || client_pid == 0 {
            anyhow::bail!("GetNamedPipeClientProcessId failed");
        }
        if ImpersonateNamedPipeClient(pipe_handle) == 0 {
            anyhow::bail!("ImpersonateNamedPipeClient failed");
        }
        let mut token = std::ptr::null_mut();
        let opened = OpenThreadToken(
            GetCurrentThread(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            1,
            &mut token,
        ) != 0;
        let reverted = RevertToSelf() != 0;
        if !reverted {
            if opened {
                CloseHandle(token);
            }
            // Returning to Tokio while still impersonating an untrusted
            // client would corrupt the service's authority. Fail-stop.
            std::process::abort();
        }
        if !opened {
            anyhow::bail!("OpenThreadToken failed");
        }

        let result = (|| {
            let session_id = token_session_id(token)
                .ok_or_else(|| anyhow::anyhow!("token session unavailable"))?;
            let caller_sid =
                token_sid(token).ok_or_else(|| anyhow::anyhow!("token SID unavailable"))?;
            let authentication_id = token_authentication_id(token)
                .ok_or_else(|| anyhow::anyhow!("token authentication ID unavailable"))?;
            let process_token = open_verified_client_process_token(
                client_pid,
                &caller_sid,
                session_id,
                authentication_id,
            )?;
            Ok(AuthenticatedPipePeer {
                client_pid,
                token: process_token,
                session_id,
                caller_sid,
                authentication_id,
            })
        })();
        CloseHandle(token);
        result
    }
}

fn open_verified_client_process_token(
    client_pid: u32,
    expected_sid: &str,
    expected_session_id: u32,
    expected_authentication_id: (u32, i32),
) -> Result<HANDLE> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, client_pid);
        if process.is_null() {
            anyhow::bail!("OpenProcess for pipe client failed");
        }
        let mut process_token = std::ptr::null_mut();
        let opened =
            OpenProcessToken(process, TOKEN_QUERY | TOKEN_DUPLICATE, &mut process_token) != 0;
        CloseHandle(process);
        if !opened {
            anyhow::bail!("OpenProcessToken for pipe client failed");
        }

        let matches = token_sid(process_token).as_deref() == Some(expected_sid)
            && token_session_id(process_token) == Some(expected_session_id)
            && token_authentication_id(process_token) == Some(expected_authentication_id);
        if !matches {
            CloseHandle(process_token);
            anyhow::bail!("pipe and process token identities differ");
        }
        Ok(process_token)
    }
}

fn token_session_id(token: HANDLE) -> Option<u32> {
    let mut session_id = 0u32;
    let mut returned = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenSessionId,
            &mut session_id as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        ) != 0
    };
    ok.then_some(session_id)
}

fn token_sid(token: HANDLE) -> Option<String> {
    let mut size = 0u32;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size) };
    let mut buffer = vec![0u8; size as usize];
    let ok = size != 0
        && unsafe {
            GetTokenInformation(token, TokenUser, buffer.as_mut_ptr() as _, size, &mut size)
        } != 0;
    let sid = ok.then(|| unsafe {
        crate::vault_access::sid_to_string((buffer.as_ptr() as *const TOKEN_USER).read().User.Sid)
    })?;
    use zeroize::Zeroize;
    buffer.zeroize();
    sid
}

fn token_authentication_id(token: HANDLE) -> Option<(u32, i32)> {
    let mut statistics = TOKEN_STATISTICS::default();
    let mut returned = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenStatistics,
            &mut statistics as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_STATISTICS>() as u32,
            &mut returned,
        ) != 0
    };
    ok.then_some((
        statistics.AuthenticationId.LowPart,
        statistics.AuthenticationId.HighPart,
    ))
}
