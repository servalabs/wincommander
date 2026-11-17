// SPDX-License-Identifier: AGPL-3.0-or-later
//! Captured pipe identity owns its verified token until the connection ends.
use anyhow::Result;
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE},
    Security::{
        GetTokenInformation, RevertToSelf, TokenSessionId, TokenStatistics, TokenUser,
        TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_STATISTICS, TOKEN_USER,
    },
    System::{
        Pipes::{GetNamedPipeClientProcessId, ImpersonateNamedPipeClient},
        Threading::{
            GetCurrentThread, GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
            OpenThreadToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

pub(crate) struct AuthenticatedPipePeer {
    pub(super) client_pid: u32,
    pub(super) token: HANDLE,
    pub(super) session_id: u32,
    pub(super) caller_sid: String,
    pub(super) authentication_id: (u32, i32),
    /// Query-limited handle to the exact client process observed at capture.
    /// While it is open the kernel cannot recycle `client_pid`, so a later
    /// `OpenProcess(client_pid)` can only ever name this instance.
    pub(super) process: HANDLE,
    /// Creation time (FILETIME ticks) of that process at capture.
    pub(super) process_creation_time: u64,
}

unsafe impl Send for AuthenticatedPipePeer {}
unsafe impl Sync for AuthenticatedPipePeer {}

impl Drop for AuthenticatedPipePeer {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.token);
            if !self.process.is_null() {
                CloseHandle(self.process);
            }
        }
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

    /// True only while the exact process instance captured with this peer is
    /// still running and `client_pid` still resolves to it. Pid-based peer
    /// re-resolution (SessionHelper and relay gates) must be preceded by this
    /// check: if the original client exited, a fresh process could otherwise
    /// inherit the pid and the image-path pin. Fails closed.
    pub(crate) fn pinned_process_is_live(&self) -> bool {
        if self.process.is_null() {
            return false;
        }
        unsafe {
            let mut code = 0u32;
            let running =
                GetExitCodeProcess(self.process, &mut code) != 0 && code == PROCESS_STILL_ACTIVE;
            let current = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, self.client_pid);
            let current_creation = if current.is_null() {
                None
            } else {
                let value = process_creation_time(current);
                CloseHandle(current);
                value
            };
            same_live_instance(running, self.process_creation_time, current_creation)
        }
    }
}

const PROCESS_STILL_ACTIVE: u32 = 259;

/// Pure decision for [`AuthenticatedPipePeer::pinned_process_is_live`].
fn same_live_instance(running: bool, pinned_creation: u64, current_creation: Option<u64>) -> bool {
    running && pinned_creation != 0 && current_creation == Some(pinned_creation)
}

fn process_creation_time(process: HANDLE) -> Option<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    let ok =
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } != 0;
    let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    (ok && ticks != 0).then_some(ticks)
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
            let (process_token, process, process_creation_time) = open_verified_client_process_token(
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
                process,
                process_creation_time,
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
) -> Result<(HANDLE, HANDLE, u64)> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, client_pid);
        if process.is_null() {
            anyhow::bail!("OpenProcess for pipe client failed");
        }
        let mut process_token = std::ptr::null_mut();
        let opened =
            OpenProcessToken(process, TOKEN_QUERY | TOKEN_DUPLICATE, &mut process_token) != 0;
        if !opened {
            CloseHandle(process);
            anyhow::bail!("OpenProcessToken for pipe client failed");
        }
        let Some(creation) = process_creation_time(process) else {
            CloseHandle(process);
            CloseHandle(process_token);
            anyhow::bail!("pipe client process creation time unavailable");
        };

        let matches = token_sid(process_token).as_deref() == Some(expected_sid)
            && token_session_id(process_token) == Some(expected_session_id)
            && token_authentication_id(process_token) == Some(expected_authentication_id);
        if !matches {
            CloseHandle(process);
            CloseHandle(process_token);
            anyhow::bail!("pipe and process token identities differ");
        }
        // The process handle stays open (owned by the peer) so the pid cannot
        // be recycled while this connection lives.
        Ok((process_token, process, creation))
    }
}

pub(super) fn token_session_id(token: HANDLE) -> Option<u32> {
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

pub(super) fn token_sid(token: HANDLE) -> Option<String> {
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

pub(super) fn token_authentication_id(token: HANDLE) -> Option<(u32, i32)> {
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

#[cfg(test)]
mod tests {
    use super::same_live_instance;

    #[test]
    fn pid_reuse_or_exit_fails_the_pinned_process_check() {
        assert!(same_live_instance(true, 7, Some(7)));
        assert!(!same_live_instance(false, 7, Some(7)));
        assert!(!same_live_instance(true, 7, Some(8)));
        assert!(!same_live_instance(true, 7, None));
        assert!(!same_live_instance(true, 0, Some(0)));
    }

    #[test]
    fn live_current_process_is_pinned_and_a_null_handle_fails_closed() {
        use windows_sys::Win32::Foundation::DuplicateHandle;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};
        let pid = unsafe { GetCurrentProcessId() };
        let me = unsafe { GetCurrentProcess() };
        let creation = super::process_creation_time(me).unwrap();
        let mut dup = std::ptr::null_mut();
        assert_ne!(unsafe { DuplicateHandle(me, me, me, &mut dup, 0, 0, 2) }, 0);
        let mut peer = super::AuthenticatedPipePeer {
            client_pid: pid,
            token: std::ptr::null_mut(),
            session_id: 0,
            caller_sid: String::new(),
            authentication_id: (0, 0),
            process: dup,
            process_creation_time: creation,
        };
        assert!(peer.pinned_process_is_live());
        peer.process_creation_time = creation + 1;
        assert!(!peer.pinned_process_is_live());
        peer.process_creation_time = creation;
        peer.process = std::ptr::null_mut();
        assert!(!peer.pinned_process_is_live());
        peer.process = dup;
    }
}
