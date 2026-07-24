// SPDX-License-Identifier: AGPL-3.0-or-later
//! Named-pipe server for the UI ↔ SYSTEM-service IPC channel (Windows-only).
//!
//! Serves [`wincmd_shared::svc::SVC_PIPE_NAME`] with an ACL that allows:
//!   - SYSTEM (SY) + Builtin Administrators (BA): full control
//!   - Interactive Users (IU): connect + read/write (0x12019b)
//!
//! The real authorization is the app-layer CapabilityClass check in
//! [`authorize`], NOT the DACL.  The DACL only prevents completely
//! unauthenticated lateral connections.

#![cfg(windows)]

use std::io;
use std::os::windows::io::AsRawHandle;

use anyhow::{Context, Result};
use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

use wincmd_shared::svc::{classify_verb, CapabilityClass, SVC_PIPE_NAME, SVC_PROTOCOL_VERSION};
use wincmd_shared::{read_envelope, write_envelope, Envelope, ErrorReply, Hello, Response};

use crate::settings_host;

use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
    Security::{
        AllocateAndInitializeSid, CheckTokenMembership, EqualSid, FreeSid, GetTokenInformation,
        TokenUser, PSECURITY_DESCRIPTOR, PSID, SECURITY_NT_AUTHORITY, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        Pipes::GetNamedPipeClientProcessId,
        SystemServices::{DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID},
        Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

// ── SDDL ────────────────────────────────────────────────────────────────────
//
// D:(A;;FA;;;SY)      — SYSTEM: full access
// (A;;FA;;;BA)        — Builtin Administrators: full access
// (A;;0x12019b;;;IU)  — Interactive Users: connect+read/write (no delete/rename)
//
// 0x12019b = FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE | READ_CONTROL |
//            FILE_READ_ATTRIBUTES — enough for named-pipe I/O, not file ops.

const PIPE_SDDL: &str = "D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x12019b;;;IU)";

/// Run the named-pipe accept loop forever.  Each accepted connection is
/// handled in a fresh tokio task.
pub async fn serve() -> Result<()> {
    // Build the SECURITY_ATTRIBUTES so the kernel creates the pipe object
    // with our explicit DACL rather than the default service-process DACL.
    let sa = build_security_attributes().context("build pipe SECURITY_ATTRIBUTES")?;

    let mut server = unsafe {
        ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(true)
            .create_with_security_attributes_raw(SVC_PIPE_NAME, sa.as_ptr() as *mut _)
            .context("create named pipe")?
    };

    // Free the local SECURITY_DESCRIPTOR we allocated.  The kernel has
    // already copied the DACL into the pipe object by now.
    drop(sa);

    loop {
        // Wait for the next client to connect.
        server.connect().await.context("pipe accept")?;

        // Swap in a fresh server instance so the next client can connect
        // immediately while we handle this one.
        let next_server = unsafe {
            ServerOptions::new()
                .pipe_mode(PipeMode::Byte)
                .create_with_security_attributes_raw(SVC_PIPE_NAME, std::ptr::null_mut())
                .context("create next pipe instance")?
        };

        let conn = std::mem::replace(&mut server, next_server);

        // Determine caller privilege from the connected pipe peer.
        let raw_handle = conn.as_raw_handle() as HANDLE;
        let caller_privileged = caller_is_privileged(raw_handle).unwrap_or(false);

        tokio::spawn(async move {
            if let Err(e) = handle_connection(conn, caller_privileged).await {
                // Non-fatal — just log and let the task exit cleanly.
                eprintln!("[svc::pipe] connection error: {:#}", e);
            }
        });
    }
}

// ── Per-connection handler ───────────────────────────────────────────────────

pub(crate) async fn handle_connection(
    mut conn: tokio::net::windows::named_pipe::NamedPipeServer,
    caller_privileged: bool,
) -> Result<()> {
    // (a) Require a valid Hello frame.
    let hello = read_envelope(&mut conn).await.context("read Hello")?;
    match hello {
        Envelope::Hello(Hello {
            protocol_version, ..
        }) if protocol_version == SVC_PROTOCOL_VERSION => {
            // Echo the handshake ack.
            let ack = Envelope::Hello(wincmd_shared::svc::hello_from_ui("svc-ack"));
            write_envelope(&mut conn, &ack)
                .await
                .context("write Hello ack")?;
        }
        _ => {
            let err = Envelope::Error(ErrorReply {
                request_id: 0,
                kind: "protocol_mismatch".to_string(),
                message: format!(
                    "expected Hello with protocol_version={}, got something else",
                    SVC_PROTOCOL_VERSION
                ),
            });
            let _ = write_envelope(&mut conn, &err).await;
            return Ok(());
        }
    }

    // (b)/(c) Request loop.
    loop {
        let env = match read_envelope(&mut conn).await {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };

        match env {
            Envelope::Bye => break,

            Envelope::Request(req) => {
                let request_id = req.request_id;
                match authorize(&req.feature_id, caller_privileged) {
                    Err(reason) => {
                        let reply = Envelope::Error(ErrorReply {
                            request_id,
                            kind: "forbidden".to_string(),
                            message: reason.to_string(),
                        });
                        write_envelope(&mut conn, &reply).await?;
                    }
                    Ok(()) => {
                        let result = match req.feature_id.as_str() {
                            "svc.status" => serde_json::to_value(settings_host::status())
                                .unwrap_or_else(|_| serde_json::json!({"ok": true})),
                            "svc.get_settings" => settings_host::get_settings(),
                            "svc.health" => settings_host::health(),
                            "svc.ping" => serde_json::json!({ "pong": true }),
                            // Other (privileged) verbs: stub until wired in later phases.
                            _ => serde_json::json!({
                                "ok": true,
                                "verb": req.feature_id
                            }),
                        };
                        let reply = Envelope::Response(Response { request_id, result });
                        write_envelope(&mut conn, &reply).await?;
                    }
                }
            }

            // Gracefully ignore unexpected frame types rather than
            // crashing the connection.
            _ => {}
        }
    }

    Ok(())
}

// ── Authorization (pure, testable) ──────────────────────────────────────────

/// Decide whether `caller` may invoke `verb`.
///
/// This is a **pure** function — no I/O, no OS calls.  All Windows security
/// calls happen before this point, producing the `caller_privileged` bool.
///
/// Returns `Ok(())` if the call is permitted, or `Err(reason)` if denied.
///
/// # Examples
///
/// ```ignore
/// // Read-only verb: always allowed, even for unprivileged callers.
/// assert!(authorize("svc.ping", false).is_ok());
/// // Privileged verb: requires admin/SYSTEM.
/// assert!(authorize("svc.dispatch", false).is_err());
/// assert!(authorize("svc.dispatch", true).is_ok());
/// ```
pub fn authorize(verb: &str, caller_privileged: bool) -> Result<(), &'static str> {
    if classify_verb(verb) == CapabilityClass::Privileged && !caller_privileged {
        Err("privileged verb requires SYSTEM/Admin caller")
    } else {
        Ok(())
    }
}

// ── Peer-SID classifier ──────────────────────────────────────────────────────

/// Return `true` if the process connected to `pipe_handle` is running as
/// LocalSystem or a member of Builtin\Administrators.
///
/// # Safety
/// `pipe_handle` must be a valid, open named-pipe server handle.
pub fn caller_is_privileged(pipe_handle: HANDLE) -> Result<bool> {
    unsafe {
        // 1. Get the client PID from the pipe.
        let mut client_pid: u32 = 0;
        if GetNamedPipeClientProcessId(pipe_handle, &mut client_pid) == 0 {
            anyhow::bail!("GetNamedPipeClientProcessId failed");
        }

        // 2. Open the client process.
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, client_pid);
        if proc.is_null() {
            anyhow::bail!("OpenProcess failed for PID {}", client_pid);
        }
        let _guard_proc = HandleGuard(proc);

        // 3. Open the process token.
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(proc, TOKEN_QUERY, &mut token) == 0 {
            anyhow::bail!("OpenProcessToken failed");
        }
        let _guard_token = HandleGuard(token);

        // 4. Check Builtin\Administrators membership.
        let is_admin = is_admin_token(token)?;
        if is_admin {
            return Ok(true);
        }

        // 5. Check LocalSystem (S-1-5-18).
        let is_system = is_local_system_token(token)?;
        Ok(is_system)
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

/// Holds the SECURITY_DESCRIPTOR allocated by
/// `ConvertStringSecurityDescriptorToSecurityDescriptorW`.  The raw pointer is
/// valid for the lifetime of this struct and freed on drop via `LocalFree`.
struct SecurityAttributes {
    sa: windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    sd: PSECURITY_DESCRIPTOR,
}

impl SecurityAttributes {
    fn as_ptr(&self) -> *const windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        &self.sa
    }
}

impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        if !self.sd.is_null() {
            unsafe { LocalFree(self.sd as *mut _) };
        }
    }
}

fn build_security_attributes() -> Result<SecurityAttributes> {
    let sddl_wide: Vec<u16> = PIPE_SDDL
        .encode_utf16()
        .chain(std::iter::once(0u16))
        .collect();

    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let mut sd_size: u32 = 0;

    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl_wide.as_ptr(),
            1, // SDDL_REVISION_1
            &mut sd,
            &mut sd_size,
        )
    };
    if ok == 0 {
        anyhow::bail!("ConvertStringSecurityDescriptorToSecurityDescriptorW failed");
    }

    let sa = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };

    Ok(SecurityAttributes { sa, sd })
}

// ── RAII guards ──────────────────────────────────────────────────────────────

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

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::authorize;
    use wincmd_shared::svc::SVC_PROTOCOL_VERSION;

    // ── authorize: unit tests (pure function, no OS) ─────────────────────

    #[test]
    fn privileged_verb_from_unprivileged_caller_is_denied() {
        let result = authorize("svc.dispatch", false);
        assert!(
            result.is_err(),
            "expected Err for privileged verb + unprivileged caller"
        );
        assert_eq!(
            result.unwrap_err(),
            "privileged verb requires SYSTEM/Admin caller"
        );
    }

    #[test]
    fn privileged_verb_from_privileged_caller_is_allowed() {
        let result = authorize("svc.dispatch", true);
        assert!(
            result.is_ok(),
            "expected Ok for privileged verb + privileged caller"
        );
    }

    #[test]
    fn read_only_verb_from_unprivileged_caller_is_allowed() {
        let result = authorize("svc.ping", false);
        assert!(
            result.is_ok(),
            "expected Ok for read-only verb + unprivileged caller"
        );
    }

    #[test]
    fn read_only_verb_from_privileged_caller_is_allowed() {
        let result = authorize("svc.status", true);
        assert!(
            result.is_ok(),
            "expected Ok for read-only verb + privileged caller"
        );
    }

    #[test]
    fn unknown_verb_from_unprivileged_caller_is_denied_fail_closed() {
        // fail-closed: unknown verbs are Privileged
        let result = authorize("svc.totally_unknown_verb", false);
        assert!(
            result.is_err(),
            "expected Err for unknown verb + unprivileged caller"
        );
    }

    // ── SVC_PROTOCOL_VERSION is exported and non-empty ───────────────────

    #[test]
    fn protocol_version_non_empty() {
        assert!(!SVC_PROTOCOL_VERSION.is_empty());
    }

    // ── peer-SID smoke test (Windows, requires pipe infrastructure) ──────

    #[test]
    fn caller_is_privileged_on_pseudo_handle_does_not_panic() {
        use super::caller_is_privileged;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        // GetCurrentProcess() returns a pseudo-handle — not a real named-pipe
        // handle, so GetNamedPipeClientProcessId will fail.  The contract says
        // "does not panic", which is satisfied by returning Err rather than
        // unwrapping.
        let pseudo = unsafe { GetCurrentProcess() };
        // We only assert no panic; result may be Ok or Err.
        let _ = caller_is_privileged(pseudo);
    }
}

/// Integration test: spin up the pipe server, connect a client, perform the
/// Hello handshake, send a Request, and verify ACL enforcement with a
/// forced privilege flag (injected so we don't depend on the test process's
/// real SID).
#[cfg(test)]
mod integration {
    use tokio::net::windows::named_pipe::{ClientOptions, PipeMode, ServerOptions};
    use wincmd_shared::svc::hello_from_ui;
    use wincmd_shared::{read_envelope, write_envelope, Envelope, ErrorReply, Request};

    use super::handle_connection;

    /// Spin up one server instance on a uniquely-named test pipe, inject
    /// `forced_privilege`, connect a client, do the Hello handshake, send
    /// `verb`, return the reply envelope.
    ///
    /// Each caller passes a distinct `pipe_suffix` so concurrent tests use
    /// different pipe names and don't race on `first_pipe_instance`.
    async fn run_one_request(pipe_suffix: &str, verb: &str, forced_privilege: bool) -> Envelope {
        let pipe_name = format!(r"\\.\pipe\wincmd-svc-test-{}", pipe_suffix);

        // Create one server instance on the test pipe.
        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("create test pipe server");

        let pipe_name2 = pipe_name.clone();
        // Spawn the server handler task (injected privilege — no real SID query).
        let server_task = tokio::spawn(async move {
            let s = server;
            s.connect().await.expect("pipe connect");
            handle_connection(s, forced_privilege)
                .await
                .expect("handle_connection");
        });

        // Give the server a tick to enter the connect wait.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Connect as the client.
        let mut client = ClientOptions::new()
            .pipe_mode(PipeMode::Byte)
            .open(&pipe_name2)
            .expect("open test pipe client");

        // Send Hello.
        let hello = Envelope::Hello(hello_from_ui("test-session-token"));
        write_envelope(&mut client, &hello)
            .await
            .expect("write Hello");

        // Read Hello ack.
        let ack = read_envelope(&mut client).await.expect("read Hello ack");
        assert!(
            matches!(ack, Envelope::Hello(_)),
            "expected Hello ack, got {:?}",
            ack
        );

        // Send the request.
        let req = Envelope::Request(Request {
            request_id: 1,
            feature_id: verb.to_string(),
            args: serde_json::json!({}),
        });
        write_envelope(&mut client, &req)
            .await
            .expect("write Request");

        // Read the response/error.
        let reply = read_envelope(&mut client).await.expect("read reply");

        // Send Bye to let the server task finish cleanly.
        let _ = write_envelope(&mut client, &Envelope::Bye).await;

        server_task.await.ok();
        reply
    }

    #[tokio::test]
    async fn privileged_verb_with_forced_unprivileged_returns_forbidden() {
        let reply = run_one_request("forbidden", "svc.dispatch", false).await;
        match reply {
            Envelope::Error(ErrorReply { kind, .. }) => {
                assert_eq!(kind, "forbidden", "expected kind=forbidden, got {:?}", kind);
            }
            other => panic!("expected Envelope::Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn privileged_verb_with_forced_privileged_returns_response() {
        let reply = run_one_request("priv-ok", "svc.dispatch", true).await;
        match reply {
            Envelope::Response(r) => {
                assert_eq!(r.result["ok"], true);
                assert_eq!(r.result["verb"], "svc.dispatch");
            }
            other => panic!("expected Envelope::Response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn read_only_verb_with_forced_unprivileged_returns_response() {
        let reply = run_one_request("ro-ok", "svc.ping", false).await;
        match reply {
            Envelope::Response(r) => {
                assert_eq!(r.result["pong"], true);
            }
            other => panic!("expected Envelope::Response, got {:?}", other),
        }
    }
}
