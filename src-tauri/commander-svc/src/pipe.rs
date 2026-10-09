// SPDX-License-Identifier: AGPL-3.0-or-later
//! Named-pipe server for the UI ↔ SYSTEM-service IPC channel (Windows-only).
//!
//! Serves [`wincmd_shared::svc::SVC_PIPE_NAME`] with an ACL that allows:
//!   - SYSTEM (SY) + Builtin Administrators (BA): full control
//!   - Builtin Users (BU): connect + read/write (0x12019b)
//!
//! The real authorization is the app-layer CapabilityClass check in
//! [`authorize`], NOT the DACL.  The DACL only prevents completely
//! unauthenticated lateral connections.
//!
//! # Clipboard Guard verbs (plan §4.3, decision D-2)
//!
//! This module also implements the four `svc.clipboard.*` /
//! `svc.policy.install_epoch` verbs (Phase 2): `svc.clipboard.get_policy`
//! reads back [`crate::policy_store::PolicyStore`]'s resolved ruleset,
//! `svc.policy.install_epoch` re-verifies and atomically installs a signed
//! epoch, `svc.clipboard.report_event` accepts an already-locally-matched
//! [`wincmd_shared::fleet::ClipboardEventReport`] from a pinned
//! `SessionHelper` peer and queues it for a future outbound path, and
//! `svc.clipboard.set_enabled` is an admin-only local kill-switch. See each
//! handler's own doc comment for the exact contract.

#![cfg(windows)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

use wincmd_shared::fleet::{Action, ClipboardEventReport};
use wincmd_shared::vault_access::VaultMountReason;
use wincmd_shared::svc::{
    classify_verb, is_known_verb, ApplyMachineSettingRequest, CapabilityClass,
    APPLY_MACHINE_SETTING_VERB, STORE_LICENSE_CACHE_VERB, SVC_PIPE_NAME, SVC_PROTOCOL_VERSION,
};
use wincmd_shared::{Envelope, ErrorReply, Hello, Request, Response};

use crate::peer_auth::{SessionHelperGate, TrustOrigin};
// `PeerAuthError` is named directly only by test code (production code
// only ever calls `.to_string()` on values of this type, via `Display`,
// without spelling out the type name) — cfg-gated so the plain (non-test)
// build doesn't warn on an otherwise-unused import.
#[cfg(test)]
use crate::peer_auth::PeerAuthError;
use crate::policy_store::{EpochInstallInput, PolicyStore};
use crate::settings_host;
use crate::vault_access::VaultAccessStore;
use crate::vault_mount::VaultMountBroker;

#[path = "service_peer_query.rs"]
mod service_peer_query;

#[path = "vault_inventory_contract.rs"]
mod vault_inventory_contract;

#[path = "vault_create_staging.rs"]
mod vault_create_staging;

use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
    Security::{
        AllocateAndInitializeSid, CheckTokenMembership, DuplicateToken, EqualSid, FreeSid,
        GetTokenInformation, LookupAccountNameW, RevertToSelf, SecurityIdentification,
        TokenSessionId, TokenStatistics, TokenUser, PSECURITY_DESCRIPTOR, PSID,
        SECURITY_NT_AUTHORITY, SID_NAME_USE, TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_STATISTICS,
        TOKEN_USER,
    },
    System::{
        Pipes::{GetNamedPipeClientProcessId, ImpersonateNamedPipeClient},
        RemoteDesktop::{
            WTSActive, WTSConnectState, WTSFreeMemory, WTSQuerySessionInformationW,
            WTS_CONNECTSTATE_CLASS, WTS_CURRENT_SERVER_HANDLE,
        },
        SystemServices::{DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID},
        Threading::{
            GetCurrentThread, OpenProcess, OpenProcessToken, OpenThreadToken,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

// ── SDDL ────────────────────────────────────────────────────────────────────
//
// D:(A;;FA;;;SY)      — SYSTEM: full access
// (A;;FA;;;BA)        — Builtin Administrators: full access
// (A;;0x12019b;;;BU)  — Local users: connect+read/write (no delete/rename)
//
// 0x12019b = FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE | READ_CONTROL |
//            FILE_READ_ATTRIBUTES — enough for named-pipe I/O, not file ops.

// Tokio rejects remote pipe clients by default and we set that option
// explicitly below. BU therefore admits ordinary local desktop/SSH sessions
// without making this an SMB-accessible endpoint; app-layer authorization
// still derives the local client PID/token for every request.
const PIPE_SDDL: &str = "D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x12019b;;;BU)";
const VAULT_POLICY_ADMIN_GROUP: &str = "WinCommander Vault Policy Administrators";
#[cfg(test)]
const PERSONAL_VAULT_CONTAINER_UNWRITABLE: &str = "vault_container_not_writable";
const PERSONAL_VAULT_SESSION_ABSENT: &str = "vault_session_unavailable";
const PERSONAL_VAULT_DRIVER_STOPPED: &str = "vault_driver_unavailable";
const PERSONAL_VAULT_UNAUTHORIZED: &str = "vault_not_authorized";

pub(crate) struct AuthenticatedPipePeer {
    client_pid: u32,
    token: HANDLE,
    session_id: u32,
    caller_sid: String,
    authentication_id: (u32, i32),
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

fn is_vault_management_verb(verb: &str) -> bool {
    matches!(
        verb,
        "svc.vault.get_policy"
            | "svc.vault.get_status"
            | "svc.vault.apply_policy"
            | "svc.vault.forget_entry_policy_only"
    )
}

/// Run the named-pipe accept loop forever.  Each accepted connection is
/// handled in a fresh tokio task.  `policy_store`, `session_helper_gate`,
/// and `clipboard_state` are constructed ONCE by `main.rs` and shared
/// (via `Arc`) across every connection — in particular the gate's rate
/// limiter and the clipboard event queue must outlive any single
/// connection to mean anything.
pub async fn serve(
    policy_store: Arc<PolicyStore>,
    session_helper_gate: Arc<SessionHelperGate>,
    clipboard_state: Arc<ClipboardGuardState>,
    vault_access: Arc<VaultAccessStore>,
    vault_mount: Arc<VaultMountBroker>,
) -> Result<()> {
    // A named-pipe instance can be abandoned by Windows after an interrupted
    // client connection or a failed replacement. Do not leave the SYSTEM
    // service running without a usable listener: discard that instance and
    // construct a fresh, explicitly protected pipe. Authorization still
    // happens independently for every new connection.
    loop {
        if let Err(error) = serve_pipe_instance(
            Arc::clone(&policy_store),
            Arc::clone(&session_helper_gate),
            Arc::clone(&clipboard_state),
            Arc::clone(&vault_access),
            Arc::clone(&vault_mount),
        )
        .await
        {
            eprintln!("[svc::pipe] listener restarted after error: {error:#}");
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
}

async fn serve_pipe_instance(
    policy_store: Arc<PolicyStore>,
    session_helper_gate: Arc<SessionHelperGate>,
    clipboard_state: Arc<ClipboardGuardState>,
    vault_access: Arc<VaultAccessStore>,
    vault_mount: Arc<VaultMountBroker>,
) -> Result<()> {
    // Build the SECURITY_ATTRIBUTES so the kernel creates the pipe object
    // with our explicit DACL rather than the default service-process DACL.
    let sa = build_security_attributes().context("build pipe SECURITY_ATTRIBUTES")?;

    let mut server = unsafe {
        ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .reject_remote_clients(true)
            .first_pipe_instance(true)
            .create_with_security_attributes_raw(SVC_PIPE_NAME, sa.as_ptr() as *mut _)
            .context("create named pipe")?
    };

    // Free the local SECURITY_DESCRIPTOR we allocated.  The kernel has
    // already copied the DACL into the pipe object by now.
    drop(sa);
    let connection_slots = crate::pipe_transport::connection_slots();

    loop {
        // Wait for the next client to connect.
        server.connect().await.context("pipe accept")?;

        // Swap in a fresh server instance so the next client can connect
        // immediately while we handle this one.
        // Every pipe *instance* needs the same explicit DACL. Passing NULL
        // here silently falls back to the LocalSystem process default after
        // the first connection, locking ordinary users out intermittently.
        let next_sa = build_security_attributes().context("build next pipe SECURITY_ATTRIBUTES")?;
        let next_server = unsafe {
            ServerOptions::new()
                .pipe_mode(PipeMode::Byte)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(SVC_PIPE_NAME, next_sa.as_ptr() as *mut _)
                .context("create next pipe instance")?
        };
        drop(next_sa);

        let conn = std::mem::replace(&mut server, next_server);
        // Before the client sends Hello it verifies this SYSTEM process image.
        // Grant only its kernel-derived account SID metadata-query access; this
        // is separate from post-Hello impersonation/command authorization.
        if service_peer_query::allow_connected_account(conn.as_raw_handle()).is_err() {
            drop(conn);
            continue;
        }
        let Ok(permit) = Arc::clone(&connection_slots).try_acquire_owned() else {
            drop(conn);
            continue;
        };

        let policy_store = Arc::clone(&policy_store);
        let session_helper_gate = Arc::clone(&session_helper_gate);
        let clipboard_state = Arc::clone(&clipboard_state);
        let vault_access = Arc::clone(&vault_access);
        let vault_mount = Arc::clone(&vault_mount);

        tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) = handle_connection(
                conn,
                false,
                false,
                None,
                true,
                policy_store,
                session_helper_gate,
                clipboard_state,
                vault_access,
                vault_mount,
            )
            .await
            {
                // Non-fatal — just log and let the task exit cleanly.
                eprintln!("[svc::pipe] connection error: {:#}", e);
            }
        });
    }
}

// ── Per-connection handler ───────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_connection(
    mut conn: tokio::net::windows::named_pipe::NamedPipeServer,
    caller_privileged: bool,
    vault_policy_manager: bool,
    peer: Option<Arc<AuthenticatedPipePeer>>,
    capture_live_peer: bool,
    policy_store: Arc<PolicyStore>,
    session_helper_gate: Arc<SessionHelperGate>,
    clipboard_state: Arc<ClipboardGuardState>,
    vault_access: Arc<VaultAccessStore>,
    vault_mount: Arc<VaultMountBroker>,
) -> Result<()> {
    // (a) Require a valid Hello frame. Capture the peer's session token —
    // every frame after the handshake that arrives as `Envelope::Signed`
    // (the documented post-handshake shape; see `wincmd_shared::svc`'s
    // module doc) is verified against THIS token, matching the exact
    // Phase-9b HMAC contract `Envelope::sign`/`verify_and_unwrap` define.
    let session_token = match crate::pipe_transport::read_hello(&mut conn)
        .await
        .context("read Hello")?
    {
        Envelope::Hello(Hello {
            protocol_version,
            session_token,
            ..
        }) if protocol_version == SVC_PROTOCOL_VERSION => session_token,
        _ => {
            let err = Envelope::Error(ErrorReply {
                request_id: 0,
                kind: "protocol_mismatch".to_string(),
                message: format!(
                    "expected Hello with protocol_version={}, got something else",
                    SVC_PROTOCOL_VERSION
                ),
            });
            let _ = crate::pipe_transport::write_frame(&mut conn, &err).await;
            return Ok(());
        }
    };

    // A named-pipe server can impersonate only after its client has written.
    // Capture the exact peer after Hello, before the ack or authorization, and
    // retain it for the connection's full lifetime.
    let peer = if capture_live_peer {
        let raw_handle = conn.as_raw_handle() as HANDLE;
        Some(Arc::new(
            capture_authenticated_pipe_peer(raw_handle)
                .context("capture authenticated peer after Hello")?,
        ))
    } else {
        peer
    };
    let caller_privileged = caller_privileged
        || peer
            .as_deref()
            .and_then(|peer| token_is_privileged(peer.token()).ok())
            .unwrap_or(false);
    let vault_policy_manager = vault_policy_manager
        || peer
            .as_deref()
            .and_then(|peer| caller_has_vault_policy_capability_token(peer.token()).ok())
            .unwrap_or(false);
    let client_pid = peer
        .as_deref()
        .map(AuthenticatedPipePeer::client_pid)
        .unwrap_or(0);
    // Derived from the captured named-pipe token, never from renderer input.
    // It is used only by personal-Vault creation, whose file work stays in
    // this caller's token.
    let caller_has_interactive_session = peer
        .as_deref()
        .is_some_and(peer_has_active_interactive_session);
    let ack = Envelope::Hello(wincmd_shared::svc::hello_from_ui("svc-ack"));
    crate::pipe_transport::write_frame(&mut conn, &ack)
        .await
        .context("write Hello ack")?;

    // (b)/(c) Bound even continuously active clients; callers can reconnect.
    // This never interrupts a handler or discards its completed reply.
    for _ in 0..crate::pipe_transport::MAX_FRAMES_PER_CONNECTION {
        let env = match crate::pipe_transport::read_frame(&mut conn).await {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };

        // Unwrap a `Signed` frame into the `Request` it carries, verifying
        // the HMAC tag against this connection's session token. A bare
        // (unsigned) `Request` is still accepted too — today's Free/Pro
        // callers send `Signed`, but nothing about this loop requires it.
        let req = match env {
            Envelope::Bye => break,
            Envelope::Request(req) => req,
            Envelope::Signed(inner) => {
                match Envelope::Signed(inner).verify_and_unwrap(&session_token) {
                    Ok(Envelope::Request(req)) => req,
                    Ok(_other) => {
                        // A Signed frame wrapping something other than a
                        // Request (e.g. a stray Notification) — gracefully
                        // ignore, same stance as the frame-type wildcard below.
                        continue;
                    }
                    Err(reason) => {
                        let reply = Envelope::Error(ErrorReply {
                            request_id: 0,
                            kind: "signature_invalid".to_string(),
                            message: reason.to_string(),
                        });
                        crate::pipe_transport::write_frame(&mut conn, &reply).await?;
                        continue;
                    }
                }
            }
            // Gracefully ignore unexpected frame types rather than
            // crashing the connection.
            _ => continue,
        };

        let request_id = req.request_id;
        if !is_known_verb(&req.feature_id) {
            let reply = Envelope::Error(ErrorReply {
                request_id,
                kind: "unknown_verb".to_string(),
                message: "service verb is not recognized".to_string(),
            });
            crate::pipe_transport::write_frame(&mut conn, &reply).await?;
            continue;
        }
        match authorize_with_interactive_session(
            &req.feature_id,
            caller_privileged
                || (vault_policy_manager && is_vault_management_verb(&req.feature_id)),
            caller_has_interactive_session,
            client_pid,
            &session_helper_gate,
        )
        .await
        {
            Err(reason) => {
                let reply = Envelope::Error(ErrorReply {
                    request_id,
                    kind: "forbidden".to_string(),
                    message: reason,
                });
                crate::pipe_transport::write_frame(&mut conn, &reply).await?;
            }
            Ok(trust_origin) => {
                let reply = dispatch_verb(
                    req,
                    trust_origin,
                    &policy_store,
                    &clipboard_state,
                    &vault_access,
                    &vault_mount,
                    peer.as_deref(),
                    caller_privileged,
                )
                .await;
                crate::pipe_transport::write_frame(&mut conn, &reply).await?;
            }
        }
    }

    Ok(())
}

// ── Authorization (async: SessionHelper's check does blocking Win32/IO) ────

/// Decide whether `caller` may invoke `verb`, and — for `SessionHelper`
/// verbs — return the [`TrustOrigin`] a handler must persist alongside
/// whatever the call produces (D-2's "trust-origin marker on stored
/// receipts").
///
/// This match is intentionally **exhaustive** over [`CapabilityClass`] (no
/// wildcard arm) so that the compiler forces a decision here the day a
/// fourth class is ever added — this property was added deliberately after
/// a fail-open bug in an earlier version of this function and must not be
/// lost.
///
/// `SessionHelper`'s real check ([`SessionHelperGate::authorize`]) does
/// blocking Win32 calls and, on the signature-verification step, spawns
/// and waits on a `powershell.exe` child process — so this function is
/// `async` and offloads that work to `tokio::task::spawn_blocking` rather
/// than blocking the calling task's tokio worker thread.
///
/// # Examples
///
/// ```ignore
/// // Read-only verb: always allowed, even for unprivileged callers.
/// assert!(authorize("svc.ping", false, 0, &gate).await.is_ok());
/// // Privileged verb: requires admin/SYSTEM.
/// assert!(authorize("svc.dispatch", false, 0, &gate).await.is_err());
/// assert!(authorize("svc.dispatch", true, 0, &gate).await.is_ok());
/// // SessionHelper verb: admin/SYSTEM privilege is NOT a substitute for
/// // peer_auth confirmation (D-2) — only a pinned, in-session, correctly
/// // signed peer passes, regardless of `caller_privileged`.
/// ```
#[cfg(test)]
pub async fn authorize(
    verb: &str,
    caller_privileged: bool,
    pid: u32,
    session_helper_gate: &Arc<SessionHelperGate>,
) -> Result<Option<TrustOrigin>, String> {
    authorize_with_interactive_session(verb, caller_privileged, false, pid, session_helper_gate)
        .await
}

/// As [`authorize`], with the captured peer's active-session status. This is
/// kept separate so direct callers cannot accidentally claim interactive
/// status; the pipe loop derives it from `AuthenticatedPipePeer`.
async fn authorize_with_interactive_session(
    verb: &str,
    caller_privileged: bool,
    caller_has_interactive_session: bool,
    pid: u32,
    session_helper_gate: &Arc<SessionHelperGate>,
) -> Result<Option<TrustOrigin>, String> {
    match classify_verb(verb) {
        CapabilityClass::ReadOnly => Ok(None),
        // The handler enforces the captured token's SID; no admin bypass or
        // active-console requirement can cross account boundaries.
        CapabilityClass::UserScoped => Ok(None),

        // This is intentionally separate from read-only. The only current
        // user is personal Vault creation: caller-selected file I/O runs with
        // the authenticated user token, while the service is limited to its
        // fixed driver payload.
        CapabilityClass::InteractiveSession => {
            if caller_has_interactive_session {
                Ok(None)
            } else {
                Err("personal Vault creation requires an active Windows session".to_string())
            }
        }

        CapabilityClass::Privileged => {
            if caller_privileged {
                Ok(None)
            } else {
                Err("privileged verb requires SYSTEM/Admin caller".to_string())
            }
        }

        // D-2: no admin/SYSTEM bypass here — a SessionHelper verb is
        // granted ONLY on interactive-session membership + binary-path
        // pinning + the per-(session,
        // path, verb) rate limit, all enforced by `SessionHelperGate`.
        // Fail closed on every `PeerAuthError` — its `Display` is a fixed,
        // path-free string (see that type's own doc/tests), safe to hand
        // straight to `ErrorReply.message`.
        CapabilityClass::SessionHelper => {
            let gate = Arc::clone(session_helper_gate);
            let verb_owned = verb.to_string();
            let result =
                tokio::task::spawn_blocking(move || gate.authorize(pid, &verb_owned)).await;
            match result {
                Ok(Ok(trust_origin)) => Ok(Some(trust_origin)),
                Ok(Err(peer_err)) => Err(peer_err.to_string()),
                Err(_join_err) => {
                    Err("session-helper authorization task failed to complete".to_string())
                }
            }
        }
    }
}

// ── Verb dispatch (Clipboard Guard business logic) ──────────────────────────

/// A verb handler's failure: a stable, enumerable `kind` tag plus a
/// message that every constructor site has already checked against plan
/// §8's "never a path, a rule name, or clipboard text" rule.
#[derive(Debug)]
struct VerbError {
    kind: &'static str,
    message: String,
}

impl VerbError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Compute the reply for an already-authorized request. `trust_origin` is
/// `Some` only for `SessionHelper`-class verbs (see [`authorize`]);
/// `ReadOnly`/`Privileged` verbs always see `None`.
#[allow(clippy::too_many_arguments)]
async fn dispatch_verb(
    req: Request,
    trust_origin: Option<TrustOrigin>,
    policy_store: &PolicyStore,
    clipboard_state: &ClipboardGuardState,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    peer: Option<&AuthenticatedPipePeer>,
    caller_privileged: bool,
) -> Envelope {
    let Request {
        request_id,
        feature_id,
        diagnostic_operation_id,
        args,
    } = req;
    let diagnostic_operation_id = diagnostic_operation_id
        .filter(|value| valid_diagnostic_operation_id(value))
        .unwrap_or_else(|| format!("VLT-{request_id}"));

    let outcome: Result<serde_json::Value, VerbError> = match feature_id.as_str() {
        "svc.status" => Ok(serde_json::to_value(settings_host::status())
            .unwrap_or_else(|_| serde_json::json!({"ok": true}))),
        "svc.get_settings" => Ok(settings_host::get_settings()),
        "svc.health" => Ok(settings_host::health()),
        "svc.ping" => Ok(serde_json::json!({ "pong": true })),
        "svc.diagnostics.query" => handle_diagnostics_query(args, peer),
        "svc.personal_settings.read" | "svc.personal_settings.write" => {
            crate::personal_settings::handle(&feature_id, args, peer)
                .await
                .map_err(|kind| VerbError::new(kind, kind))
        }

        APPLY_MACHINE_SETTING_VERB => handle_apply_machine_setting(args),

        STORE_LICENSE_CACHE_VERB => crate::license_cache::store(args)
            .map(|_| serde_json::json!({ "stored": true }))
            .map_err(|message| VerbError::new("license_cache_rejected", message)),

        "svc.clipboard.get_policy" => Ok(clipboard_policy_response(policy_store)),

        "svc.policy.install_epoch" => handle_install_epoch(policy_store, args),

        "svc.clipboard.report_event" => match trust_origin {
            Some(origin) => handle_report_event(clipboard_state, args, origin),
            // `authorize()` only ever returns `Some` for the `SessionHelper`
            // class, and this verb IS classified `SessionHelper` — reaching
            // `None` here would mean that contract broke. Fail closed
            // rather than silently accepting an unattributed event.
            None => Err(VerbError::new(
                "internal_error",
                "missing trust attribution for a session-helper verb",
            )),
        },

        "svc.clipboard.set_enabled" => handle_set_enabled(clipboard_state, args),

        "svc.vault.get_policy" => Ok(serde_json::to_value(vault_policy_projection(
            vault_access,
            peer.map(AuthenticatedPipePeer::caller_sid).unwrap_or(""),
            caller_privileged,
        ))
        .unwrap_or(serde_json::Value::Null)),
        "svc.vault.list_principals" => handle_vault_list_principals(vault_access, args, peer),
        "svc.vault.get_status" => Ok(serde_json::to_value(vault_access.caller_status(
            peer.map(AuthenticatedPipePeer::caller_sid).unwrap_or(""),
            caller_privileged,
        ))
        .unwrap_or(serde_json::Value::Null)),
        // The historical whole-policy wire cannot safely be caller-scoped:
        // an owner would have to receive other owners' records or omission
        // would become deletion.  Keep it closed now that fragments exist.
        "svc.vault.apply_policy" => Err(VerbError::new(
            "vault_legacy_policy_wire_retired",
            "reload Fleet Vaults before saving policy changes",
        )),
        "svc.vault.apply_owner_fragment" => handle_vault_apply_owner_fragment(
            vault_access,
            vault_mount,
            args,
            peer,
            caller_privileged,
        ),
        "svc.vault.forget_entry_policy_only" => handle_vault_forget_entry_policy_only(
            vault_access,
            vault_mount,
            args,
            peer,
            caller_privileged,
        ),
        "svc.vault.authorize_mount" => handle_vault_authorize(vault_access, args, peer),
        "svc.vault.mount" if args.get("personal") == Some(&serde_json::Value::Bool(true)) => {
            handle_personal_vault_mount(
                request_id,
                vault_access,
                vault_mount,
                args,
                peer,
                caller_privileged,
            )
            .await
        }
        "svc.vault.mount" => {
            handle_vault_mount(
                request_id,
                &diagnostic_operation_id,
                vault_access,
                vault_mount,
                args,
                peer,
            )
            .await
        }
        "svc.vault.create_personal" => {
            handle_personal_vault_create(request_id, vault_access, args, peer).await
        }
        "svc.vault.unmount" => handle_vault_unmount(
            request_id,
            &diagnostic_operation_id,
            vault_access,
            vault_mount,
            args,
            peer,
        ),
        "svc.vault.dismount_personal" => {
            handle_personal_vault_dismount(request_id, vault_access, vault_mount, args, peer)
        }
        "svc.vault.manage_personal_syncthing" => handle_personal_vault_syncthing_manage(
            request_id,
            vault_access,
            vault_mount,
            args,
            peer,
        ),
        "svc.vault.enroll_personal_syncthing" => handle_personal_vault_syncthing_enroll(
            request_id,
            vault_access,
            vault_mount,
            args,
            peer,
        ),
        "svc.vault.release_orphaned_drive_letters" => {
            handle_release_orphaned_vault_drive_letters(vault_mount, args, peer)
        }
        "svc.vault.list_authorized" => {
            if args.get("personal").is_some() {
                handle_personal_vault_list(vault_access, vault_mount, &args, peer)
            } else {
                handle_vault_list_authorized(vault_access, vault_mount, peer)
            }
        }
        "svc.vault.capabilities" => {
            Ok(serde_json::json!({ "can_manage_policy": caller_privileged }))
        }
        "svc.vault.drive_letters" => {
            handle_vault_drive_letters(vault_access, vault_mount, args, peer, caller_privileged)
        }
        "svc.vault.reconcile_access_groups" => Err(VerbError::new(
            "vault_legacy_group_wire_retired",
            "Reload Fleet Access control before saving groups.",
        )),
        "svc.vault.get_access_directory" => handle_vault_get_access_directory(vault_access, args),
        "svc.vault.save_access_directory" => handle_vault_save_access_directory(
            vault_access,
            vault_mount,
            args,
            peer.map(|peer| peer.caller_sid()),
        ),
        "svc.vault.personal_status" => handle_personal_vault_status(vault_access, args, peer),

        // The connection loop checks `is_known_verb` before authorization.
        // Keep this second guard so direct tests or future internal callers of
        // `dispatch_verb` cannot turn an unrecognized string into success.
        _ => Err(VerbError::new(
            "unknown_verb",
            "service verb is not recognized",
        )),
    };

    match outcome {
        Ok(result) => Envelope::Response(Response { request_id, result }),
        Err(e) => Envelope::Error(ErrorReply {
            request_id,
            kind: e.kind.to_string(),
            message: e.message,
        }),
    }
}

fn valid_diagnostic_operation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Returns the encrypted service store's deliberately small support view.
/// The service writer owns decryption; the pipe never returns ciphertext,
/// context, raw errors, paths, identities, or filesystem details.
fn handle_diagnostics_query(
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    if peer.is_none() {
        return Err(VerbError::new(
            "diagnostics_forbidden",
            "diagnostic history requires an authenticated Windows session",
        ));
    }
    let object = args.as_object().ok_or_else(|| {
        VerbError::new(
            "diagnostics_validation_failed",
            "diagnostic query is invalid",
        )
    })?;
    if object
        .keys()
        .any(|key| key != "operation_id" && key != "limit")
    {
        return Err(VerbError::new(
            "diagnostics_validation_failed",
            "diagnostic query is invalid",
        ));
    }
    let operation_id = match object.get("operation_id") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value))
            if !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) =>
        {
            Some(value.as_str())
        }
        _ => {
            return Err(VerbError::new(
                "diagnostics_validation_failed",
                "diagnostic query is invalid",
            ))
        }
    };
    let limit = match object.get("limit") {
        None => 100,
        Some(value) => value
            .as_u64()
            .filter(|value| (1..=100).contains(value))
            .map(|value| value as usize)
            .ok_or_else(|| {
                VerbError::new(
                    "diagnostics_validation_failed",
                    "diagnostic query is invalid",
                )
            })?,
    };
    crate::diagnostics::recent_summaries(operation_id, limit)
        .and_then(|events| {
            serde_json::to_value(events)
                .map_err(|_| "encode diagnostic summaries failed".to_string())
        })
        .map_err(|_| {
            VerbError::new(
                "diagnostics_unavailable",
                "encrypted diagnostic history is unavailable",
            )
        })
}

/// Applies one explicitly allow-listed machine setting through the SYSTEM
/// service and returns the Windows read-back.  Deserialization is deliberately
/// into the shared typed contract rather than an open JSON object.
fn handle_apply_machine_setting(args: serde_json::Value) -> Result<serde_json::Value, VerbError> {
    let request: ApplyMachineSettingRequest = serde_json::from_value(args).map_err(|_| {
        VerbError::new(
            "machine_setting_validation_failed",
            "machine setting request is invalid",
        )
    })?;
    request.validate().map_err(|_| {
        VerbError::new(
            "machine_setting_validation_failed",
            "machine setting request is invalid",
        )
    })?;
    let observed = crate::machine_settings::apply(request)
        .map_err(|message| VerbError::new("machine_setting_apply_failed", message))?;
    serde_json::to_value(observed).map_err(|_| {
        VerbError::new(
            "machine_setting_apply_failed",
            "machine setting read-back could not be encoded",
        )
    })
}

/// Backs `svc.clipboard.get_policy` (`ReadOnly` — GROUNDING §7: safe for
/// any authenticated peer, since the resolved ruleset is already
/// observable by triggering it).
///
/// Wire shape is `{"policy_version": i64, "rules": [Rule, ...]}` — this
/// must deserialize into EXACTLY `clipboard_guard_helper::policy::
/// ClipboardPolicyResponse` (that type's own doc comment states the
/// contract this function must satisfy: "the `Response.result` JSON must
/// deserialize into exactly this shape"). Both fields are required there
/// (no `Option`/`#[serde(default)]`), so when nothing has been installed
/// yet this responds with the same sentinel `ClipboardPolicyResponse`'s
/// own `ActivePolicy::empty()` represents on the client side — version 0,
/// no rules — rather than a differently-shaped "not installed" marker a
/// required-field struct could never parse.
fn clipboard_policy_response(policy_store: &PolicyStore) -> serde_json::Value {
    match policy_store.get_clipboard_policy() {
        Some(view) => serde_json::json!({
            "policy_version": view.version,
            "rules": view.rules,
        }),
        None => serde_json::json!({
            "policy_version": 0,
            "rules": Vec::<wincmd_clip_rules::Rule>::new(),
        }),
    }
}

#[cfg(test)]
const VAULT_POLICY_MAX_ENTRIES: usize = 64;
#[cfg(test)]
const VAULT_POLICY_MAX_GRANTS: usize = 32;

/// Parse and structurally reject an untrusted policy before taking the mount
/// broker's exclusive operation.  That ordering is intentional: a malformed
/// request must not turn a harmless failed Save into a dismount of an active
/// Vault. `VaultAccessStore::apply` repeats these checks after filesystem
/// path normalization as the authoritative defence-in-depth boundary.
#[cfg(test)]
fn prepare_vault_apply_policy(
    args: serde_json::Value,
) -> Result<wincmd_shared::vault_access::VaultAccessPolicy, VerbError> {
    let mut policy: wincmd_shared::vault_access::VaultAccessPolicy = serde_json::from_value(args)
        .map_err(|_| {
        VerbError::new("vault_validation_failed", "vault policy request is invalid")
    })?;
    // The renderer echoes the version it edited.  The service alone advances
    // it, so an initial draft's zero and a current-version echo cannot cause
    // a same-version rewrite.
    if policy.version <= policy.expected_previous_version {
        policy.version = policy.expected_previous_version.saturating_add(1);
    }
    validate_vault_apply_structure(&policy)?;
    Ok(policy)
}

/// Mirrors the request-only portion of `vault_access::validate_policy`.
/// Filesystem normalization/identity and principal resolution stay in the
/// store because they require its protected Windows seams.  These checks are
/// deliberately enough to reject malformed and duplicate renderer requests
/// before a live mount is touched.
#[cfg(test)]
fn validate_vault_apply_structure(
    policy: &wincmd_shared::vault_access::VaultAccessPolicy,
) -> Result<(), VerbError> {
    use wincmd_shared::vault_access::{VaultPresentation, VAULT_ACCESS_SCHEMA_VERSION};

    let invalid = || VerbError::new("vault_validation_failed", "vault policy request is invalid");
    if policy.schema_version != VAULT_ACCESS_SCHEMA_VERSION
        || policy.policy_id.is_empty()
        || policy.policy_id.len() > 64
        || policy.version == 0
        || policy.entries.len() > VAULT_POLICY_MAX_ENTRIES
    {
        return Err(invalid());
    }

    // An empty request is the intentional policy-removal shape. Its exact
    // version/policy identity is checked against the active record below,
    // still before any cleanup can dismount a Vault.
    if policy.entries.is_empty() {
        return Ok(());
    }

    let mut ids = HashSet::new();
    let mut container_paths = HashSet::new();
    let mut reserved_letters = HashSet::new();
    for entry in &policy.entries {
        if !valid_vault_entry_id(&entry.id)
            || entry.label.trim().is_empty()
            || entry.label.len() > 128
            || entry.owner_account.trim().is_empty()
            || !Path::new(&entry.container_path).is_absolute()
            || entry.grants.is_empty()
            || entry.grants.len() > VAULT_POLICY_MAX_GRANTS
            || !ids.insert(entry.id.as_str())
        {
            return Err(invalid());
        }
        let container_key = entry
            .container_path
            .trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase();
        if container_key.is_empty() || !container_paths.insert(container_key) {
            return Err(invalid());
        }
        if let Some(letter) = &entry.mount.preferred_letter {
            if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_alphabetic() {
                return Err(invalid());
            }
            if !reserved_letters.insert(letter.to_ascii_uppercase()) {
                return Err(invalid());
            }
        }
        let mut principals = HashSet::new();
        for grant in &entry.grants {
            let principal_key = grant.principal_name.trim().to_ascii_lowercase();
            if principal_key.is_empty()
                || grant.principal_name.len() > 256
                || !principals.insert(principal_key)
            {
                return Err(invalid());
            }
        }
        if entry.mount.presentation == VaultPresentation::Machine && entry.grants.len() < 2 {
            return Err(invalid());
        }
    }
    Ok(())
}

fn valid_vault_entry_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn validate_vault_apply_version(
    vault_access: &VaultAccessStore,
    policy: &wincmd_shared::vault_access::VaultAccessPolicy,
) -> Result<(), VerbError> {
    let previous = vault_access.policy();
    let previous_version = previous.as_ref().map(|active| active.version).unwrap_or(0);
    let version_matches = policy.expected_previous_version == previous_version
        && policy.version == previous_version.saturating_add(1);
    let clear_matches = !policy.entries.is_empty()
        || previous
            .as_ref()
            .is_some_and(|active| active.policy_id == policy.policy_id);
    if version_matches && clear_matches {
        Ok(())
    } else {
        Err(VerbError::new(
            "vault_apply_failed",
            "vault policy was changed elsewhere since this draft was loaded — reload the Vault tab and reapply",
        ))
    }
}

fn handle_vault_apply(
    vault_access: &VaultAccessStore,
    policy: wincmd_shared::vault_access::VaultAccessPolicy,
) -> Result<serde_json::Value, VerbError> {
    if policy.entries.is_empty() {
        return vault_access
            .clear(policy)
            .and_then(|status| {
                serde_json::to_value(status)
                    .map_err(|_| crate::vault_access::VaultError::Persistence)
            })
            .map_err(|error| VerbError::new("vault_apply_failed", vault_error_message(error)));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    vault_access
        .apply(policy, now)
        .and_then(|status| {
            serde_json::to_value(status).map_err(|_| crate::vault_access::VaultError::Persistence)
        })
        .map_err(|error| VerbError::new("vault_apply_failed", vault_error_message(error)))
}

fn handle_vault_apply_owner_fragment(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
    caller_privileged: bool,
) -> Result<serde_json::Value, VerbError> {
    let fragment: wincmd_shared::vault_access::VaultOwnerPolicyFragment =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "vault owner policy request is invalid",
            )
        })?;
    let caller_sid = peer
        .map(AuthenticatedPipePeer::caller_sid)
        .filter(|sid| !sid.is_empty())
        .ok_or_else(|| VerbError::new("vault_not_authorized", "vault owner session unavailable"))?;
    vault_mount.with_exclusive_operation(|| {
        let policy = merge_owner_fragment(
            vault_access,
            fragment.clone(),
            caller_sid,
            caller_privileged,
        )?;
        validate_private_owner_users(&policy, caller_sid)?;
        // The fragment has already been merged with the service's complete
        // protected policy.  The ordinary apply path now performs its normal
        // identity/ACL read-back and atomic persistence.
        validate_vault_apply_version(vault_access, &policy)?;
        validate_vault_owner_mutation(vault_access, &policy, caller_sid, caller_privileged)?;
        validate_vault_changed_targets_unmounted(vault_access, vault_mount, &policy)?;
        let occupied = vault_mount.occupied_letters_locked().map_err(|_| {
            VerbError::new(
                "vault_drive_letters_unavailable",
                "Windows drive availability could not be checked. Refresh and try again.",
            )
        })?;
        validate_policy_drive_letters(&policy, vault_access.policy().as_ref(), &occupied)?;
        vault_access
            .preflight_apply(policy.clone())
            .map_err(|error| VerbError::new("vault_apply_failed", vault_error_message(error)))?;
        handle_vault_apply(vault_access, policy)?;
        serde_json::to_value(vault_access.caller_status(caller_sid, caller_privileged))
            .map_err(|_| VerbError::new("vault_apply_failed", "Vault status could not be encoded"))
    })
}

fn validate_vault_changed_targets_unmounted(
    store: &VaultAccessStore,
    broker: &VaultMountBroker,
    requested: &wincmd_shared::vault_access::VaultAccessPolicy,
) -> Result<(), VerbError> {
    let (ids, identities) = store
        .policy_change_targets(requested)
        .map_err(|error| VerbError::new("vault_apply_failed", vault_error_message(error)))?;
    validate_vault_targets_unmounted(broker, &ids, &identities)
}

fn validate_vault_targets_unmounted(
    broker: &VaultMountBroker,
    ids: &HashSet<String>,
    identities: &HashSet<String>,
) -> Result<(), VerbError> {
    broker.reject_policy_changes_while_mounted_locked(ids, identities).map_err(|reason| {
        if reason == wincmd_shared::vault_access::VaultMountReason::AlreadyMounted {
            VerbError::new("vault_mounted", "Dismount this Vault before editing or removing its policy. Other mounted Vaults do not block this change.")
        } else {
            VerbError::new("vault_mount_state_unknown", "Windows could not verify whether this Vault is mounted. Refresh its status before changing its policy.")
        }
    })
}

fn validate_policy_drive_letters(
    policy: &wincmd_shared::vault_access::VaultAccessPolicy,
    previous: Option<&wincmd_shared::vault_access::VaultAccessPolicy>,
    occupied: &HashSet<String>,
) -> Result<(), VerbError> {
    if policy
        .entries
        .iter()
        .filter(|entry| {
            !previous.is_some_and(|policy| policy.entries.iter().any(|old| old == *entry))
        })
        .filter_map(|entry| entry.mount.preferred_letter.as_ref())
        .any(|letter| occupied.contains(&letter.to_ascii_uppercase()))
    {
        return Err(VerbError::new("vault_engine_drive_letter_unavailable",
            "A selected drive letter is already in use on this PC. Choose a free letter before saving."));
    }
    Ok(())
}

fn handle_vault_drive_letters(
    store: &VaultAccessStore,
    broker: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
    caller_privileged: bool,
) -> Result<serde_json::Value, VerbError> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Query {
        exclude_entry_id: Option<String>,
    }
    let query: Query = serde_json::from_value(args).map_err(|_| {
        VerbError::new(
            "vault_validation_failed",
            "drive availability request is invalid",
        )
    })?;
    let peer = require_personal_mount_peer(peer)?;
    broker.with_exclusive_operation(|| {
        let mut exclude_entry_id = None;
        if let Some(entry_id) = query.exclude_entry_id.as_deref() {
            if !valid_vault_entry_id(entry_id) {
                return Err(VerbError::new(
                    "vault_validation_failed",
                    "drive availability request is invalid",
                ));
            }
            let projection = if caller_privileged {
                store.administrator_projection()
            } else {
                store.caller_projection(peer.caller_sid())
            };
            // A new draft has no reservation. Unknown and foreign opaque IDs both
            // exclude nothing, so this query cannot reveal whether either exists.
            if projection.entries.iter().any(|entry| {
                entry.entry.id == entry_id
                    && store
                        .fleet_group_access(&entry.entry, peer.caller_sid())
                        .is_ok_and(|access| access != Some(false))
            }) {
                exclude_entry_id = Some(entry_id);
            }
        }
        let letters = broker
            .unavailable_letters_locked(store, exclude_entry_id)
            .map_err(|_| {
                VerbError::new(
                    "vault_drive_letters_unavailable",
                    "Windows drive availability could not be checked. Refresh and try again.",
                )
            })?;
        Ok(serde_json::json!({ "unavailable_letters": letters }))
    })
}

/// The selected owner must be a Windows-resolved account, not a renderer label.
fn validate_private_owner_users(
    policy: &wincmd_shared::vault_access::VaultAccessPolicy,
    caller_sid: &str,
) -> Result<(), VerbError> {
    let users = crate::vault_access::local_user_principals().map_err(|_| {
        VerbError::new(
            "vault_directory_unavailable",
            "Windows user accounts are unavailable",
        )
    })?;
    let mut user_sids = users
        .iter()
        .map(|principal| principal.sid.as_str())
        .collect::<HashSet<_>>();
    // A domain account can have a valid authenticated local session without a local SAM row.
    if crate::vault_access::account_label_for_sid(caller_sid).is_some() {
        user_sids.insert(caller_sid);
    }
    validate_private_owner_sids(policy, &user_sids)
}

fn validate_private_owner_sids(
    policy: &wincmd_shared::vault_access::VaultAccessPolicy,
    user_sids: &HashSet<&str>,
) -> Result<(), VerbError> {
    for entry in policy.entries.iter().filter(|entry| {
        entry.mount.presentation == wincmd_shared::vault_access::VaultPresentation::PerUser
    }) {
        let Some(owner_sid) = entry.primary_owner_sid.as_deref() else {
            return Err(VerbError::new(
                "vault_not_authorized",
                "a private vault requires a Windows user as its primary owner",
            ));
        };
        if !user_sids.contains(owner_sid) {
            return Err(VerbError::new(
                "vault_not_authorized",
                "the selected primary owner is not an available Windows user",
            ));
        }
    }
    Ok(())
}

fn vault_policy_projection(
    store: &VaultAccessStore,
    caller_sid: &str,
    caller_privileged: bool,
) -> wincmd_shared::vault_access::VaultOwnerPolicyFragment {
    let mut projection = if caller_privileged && !caller_sid.is_empty() {
        store.administrator_projection()
    } else {
        store.caller_projection(caller_sid)
    };
    for owned in &mut projection.entries {
        let group_access = store.fleet_group_access(&owned.entry, caller_sid);
        let owns_entry = owned.entry.primary_owner_sid.as_deref() == Some(caller_sid);
        (owned.can_edit_policy, owned.can_remove_policy) =
            vault_policy_capabilities(owns_entry, caller_privileged, group_access);
    }
    projection
}

fn vault_policy_capabilities(
    owns_entry: bool,
    caller_privileged: bool,
    group_access: Result<Option<bool>, crate::vault_access::VaultError>,
) -> (bool, bool) {
    let can_edit = match group_access {
            Ok(Some(true)) => owns_entry || caller_privileged,
            Ok(None) => owns_entry,
            _ => false,
    };
    (can_edit, caller_privileged || can_edit)
}

fn merge_owner_fragment(
    vault_access: &VaultAccessStore,
    fragment: wincmd_shared::vault_access::VaultOwnerPolicyFragment,
    caller_sid: &str,
    caller_privileged: bool,
) -> Result<wincmd_shared::vault_access::VaultAccessPolicy, VerbError> {
    let previous = vault_access.policy();
    let mut group_scope = HashMap::new();
    let mut scoped_entry_ids = HashSet::new();
    for entry in previous
        .iter()
        .flat_map(|policy| &policy.entries)
        .chain(fragment.entries.iter().map(|owned| &owned.entry))
    {
        // A persisted absence of group authority must also override proposed groups.
        if !scoped_entry_ids.insert(entry.id.as_str()) {
            continue;
        }
        if let Some(allowed) = vault_access
            .fleet_group_access(entry, caller_sid)
            .map_err(|_| fleet_group_access_denied())?
        {
            group_scope.insert(entry.id.clone(), allowed);
        }
    }
    merge_owner_fragment_policy_with_scope(
        previous,
        fragment,
        caller_sid,
        caller_privileged,
        &group_scope,
    )
}

fn fleet_group_access_denied() -> VerbError {
    VerbError::new("vault_fleet_group_required", "This Vault belongs to a Fleet group that your Windows account does not belong to. No policy changes were made.")
}

fn validate_primary_owner_transfer(
    existing: &wincmd_shared::vault_access::VaultAccessEntry,
    requested: &wincmd_shared::vault_access::VaultAccessEntry,
    caller_privileged: bool,
) -> Result<(), VerbError> {
    if existing.primary_owner_sid != requested.primary_owner_sid && !caller_privileged {
        return Err(VerbError::new("vault_owner_transfer_requires_admin",
            "A standard Windows account cannot transfer Vault ownership. Remove the unmounted policy and ask an administrator to recreate it for the new owner."));
    }
    Ok(())
}

fn new_vault_owner_allowed(
    entry: &wincmd_shared::vault_access::VaultAccessEntry,
    caller_sid: &str,
    caller_privileged: bool,
    initial_policy: bool,
) -> bool {
    let private =
        entry.mount.presentation == wincmd_shared::vault_access::VaultPresentation::PerUser;
    entry.primary_owner_sid.as_deref() == Some(caller_sid)
        || (private && caller_privileged)
        || (!private && initial_policy)
}

#[cfg(test)]
fn merge_owner_fragment_policy(
    previous: Option<wincmd_shared::vault_access::VaultAccessPolicy>,
    fragment: wincmd_shared::vault_access::VaultOwnerPolicyFragment,
    caller_sid: &str,
    administrator_sids: Option<&HashSet<String>>,
) -> Result<wincmd_shared::vault_access::VaultAccessPolicy, VerbError> {
    merge_owner_fragment_policy_with_scope(
        previous,
        fragment,
        caller_sid,
        administrator_sids.is_some(),
        &HashMap::new(),
    )
}

fn merge_owner_fragment_policy_with_scope(
    previous: Option<wincmd_shared::vault_access::VaultAccessPolicy>,
    fragment: wincmd_shared::vault_access::VaultOwnerPolicyFragment,
    caller_sid: &str,
    caller_privileged: bool,
    group_scope: &HashMap<String, bool>,
) -> Result<wincmd_shared::vault_access::VaultAccessPolicy, VerbError> {
    use wincmd_shared::vault_access::VaultAccessPolicy;
    let denied = || {
        VerbError::new(
            "vault_owner_required",
            "Only the primary owner or an authorized Fleet group administrator can edit this vault policy. Other administrators may remove it only while unmounted.",
        )
    };
    if fragment.schema_version != wincmd_shared::vault_access::VAULT_ACCESS_SCHEMA_VERSION {
        return Err(VerbError::new(
            "vault_validation_failed",
            "vault owner policy request is invalid",
        ));
    }
    let mut removals = HashSet::new();
    for id in &fragment.remove_entry_ids {
        if !valid_vault_entry_id(id) || !removals.insert(id.as_str()) {
            return Err(VerbError::new(
                "vault_validation_failed",
                "vault removal request is invalid",
            ));
        }
        let existing = previous
            .as_ref()
            .and_then(|policy| policy.entries.iter().find(|entry| entry.id == *id))
            .ok_or_else(|| {
                VerbError::new(
                    "vault_validation_failed",
                    "vault removal request is invalid",
                )
            })?;
        if group_scope.get(id) == Some(&false) && !caller_privileged {
            return Err(fleet_group_access_denied());
        }
        if existing.primary_owner_sid.as_deref() != Some(caller_sid) && !caller_privileged {
            return Err(denied());
        }
    }
    let mut incoming = HashMap::new();
    for owned in fragment.entries {
        if group_scope.get(&owned.entry.id) == Some(&false) {
            let unchanged = previous
                .as_ref()
                .is_some_and(|policy| policy.entries.iter().any(|entry| *entry == owned.entry));
            if unchanged {
                continue;
            }
            return Err(fleet_group_access_denied());
        }
        if removals.contains(owned.entry.id.as_str()) {
            return Err(VerbError::new(
                "vault_validation_failed",
                "vault removal request is invalid",
            ));
        }
        match previous.as_ref().and_then(|policy| {
            policy
                .entries
                .iter()
                .find(|entry| entry.id == owned.entry.id)
        }) {
                Some(existing) if existing.primary_owner_sid.as_deref() != Some(caller_sid) => {
                let group_administrator =
                    caller_privileged && group_scope.get(&existing.id) == Some(&true);
                    if !group_administrator && owned.entry != *existing {
                        return Err(denied());
                    }
                }
            Some(existing) => {
                validate_primary_owner_transfer(existing, &owned.entry, caller_privileged)?
            }
            None if !new_vault_owner_allowed(
                &owned.entry,
                caller_sid,
                caller_privileged,
                previous.is_none(),
            ) =>
            {
                    return Err(denied());
                }
                _ => {}
        }
        if incoming.insert(owned.entry.id.clone(), owned).is_some() {
            return Err(VerbError::new(
                "vault_validation_failed",
                "vault owner policy request is invalid",
            ));
        }
    }
    let Some(previous) = previous else {
        let policy_id = fragment
            .policy_id
            .unwrap_or_else(|| format!("vault-owner-{caller_sid}"));
        return Ok(VaultAccessPolicy {
            schema_version: fragment.schema_version,
            policy_id,
            version: 1,
            expected_previous_version: 0,
            entries: incoming.into_values().map(|owned| owned.entry).collect(),
        });
    };
    if fragment.policy_id.as_deref() != Some(previous.policy_id.as_str())
        || fragment.expected_previous_version != previous.version
    {
        return Err(VerbError::new("vault_apply_failed", "vault policy was changed elsewhere since this draft was loaded — reload the Vault tab and reapply"));
    }
    let mut entries = Vec::with_capacity(previous.entries.len() + incoming.len());
    for existing in &previous.entries {
        if removals.contains(existing.id.as_str()) {
            continue;
        }
        let Some(replacement) = incoming.remove(&existing.id) else {
            // Omission is never deletion: the caller may only receive their
            // own fragment and must not be able to erase a hidden owner.
            entries.push(existing.clone());
            continue;
        };
        let group_administrator = caller_privileged && group_scope.get(&existing.id) == Some(&true);
        if existing.primary_owner_sid.as_deref() != Some(caller_sid)
            && !group_administrator
            && replacement.entry != *existing
        {
            return Err(denied());
        }
        let mut entry = replacement.entry;
        // A saved Vault is an opaque service identity. Preserve backing path,
        // stable identity, and container kind from the protected record; the
        // owner may change policy, not retarget it to an arbitrary file.
        entry.container_path = existing.container_path.clone();
        entry.container_identity = existing.container_identity.clone();
        entry.container_kind = existing.container_kind;
        entries.push(entry);
    }
    // Newly assigned private entries still undergo principal and file identity validation.
    entries.extend(incoming.into_values().map(|owned| owned.entry));
    Ok(VaultAccessPolicy {
        schema_version: previous.schema_version,
        policy_id: previous.policy_id,
        version: previous.version.saturating_add(1),
        expected_previous_version: previous.version,
        entries,
    })
}

/// Ownership is a service decision based on the authenticated Windows SID,
/// never a renderer's selected account name.  A privileged caller can create
/// a policy for another user or remove an unmounted entry, but cannot edit
/// its owner, access, or contents without policy membership.
fn validate_vault_owner_mutation(
    vault_access: &VaultAccessStore,
    requested: &wincmd_shared::vault_access::VaultAccessPolicy,
    caller_sid: &str,
    caller_privileged: bool,
) -> Result<(), VerbError> {
    let previous = vault_access.policy();
    let mut group_scope = HashMap::new();
    let mut scoped_entry_ids = HashSet::new();
    for entry in previous
        .iter()
        .flat_map(|policy| &policy.entries)
        .chain(requested.entries.iter())
    {
        if !scoped_entry_ids.insert(entry.id.as_str()) {
            continue;
        }
        if let Some(allowed) = vault_access
            .fleet_group_access(entry, caller_sid)
            .map_err(|_| fleet_group_access_denied())?
        {
            group_scope.insert(entry.id.clone(), allowed);
        }
    }
    validate_vault_owner_policy_mutation_with_scope(
        previous.as_ref(),
        requested,
        caller_sid,
        caller_privileged,
        &group_scope,
    )?;
    let protected_removals = previous
        .iter()
        .flat_map(|policy| &policy.entries)
        .filter(|old| requested.entries.iter().all(|entry| entry.id != old.id))
        .filter(|old| {
            !vault_policy_capabilities(
                old.primary_owner_sid.as_deref() == Some(caller_sid),
                caller_privileged,
            Ok(group_scope.get(&old.id).copied()),
            )
            .0
        })
        .map(|old| old.id.clone())
        .collect::<HashSet<_>>();
    vault_access.reject_removed_entry_reidentification(requested, &protected_removals)
        .map_err(|error| {
            if error == crate::vault_access::VaultError::Forbidden {
                VerbError::new("vault_owner_required",
                    "Removing a policy does not authorize replacing the same Vault's owner or access in that save. Remove it separately while unmounted.")
            } else {
                VerbError::new("vault_apply_failed", vault_error_message(error))
            }
        })
}

#[cfg(test)]
fn validate_vault_owner_policy_mutation(
    previous: Option<&wincmd_shared::vault_access::VaultAccessPolicy>,
    requested: &wincmd_shared::vault_access::VaultAccessPolicy,
    caller_sid: &str,
    caller_privileged: bool,
) -> Result<(), VerbError> {
    validate_vault_owner_policy_mutation_with_scope(
        previous,
        requested,
        caller_sid,
        caller_privileged,
        &HashMap::new(),
    )
}

fn validate_vault_owner_policy_mutation_with_scope(
    previous: Option<&wincmd_shared::vault_access::VaultAccessPolicy>,
    requested: &wincmd_shared::vault_access::VaultAccessPolicy,
    caller_sid: &str,
    caller_privileged: bool,
    group_scope: &HashMap<String, bool>,
) -> Result<(), VerbError> {
    let denied = || {
        VerbError::new(
            "vault_owner_required",
            "Only the primary owner or an authorized Fleet group administrator can edit this vault policy. Other administrators may remove it only while unmounted.",
        )
    };
    for entry in &requested.entries {
        if group_scope.get(&entry.id) == Some(&false)
            && !previous
                .is_some_and(|policy| policy.entries.iter().any(|existing| existing == entry))
        {
            return Err(fleet_group_access_denied());
        }
    }
    let Some(previous) = previous else {
        if requested
            .entries
            .iter()
            .any(|entry| !new_vault_owner_allowed(entry, caller_sid, caller_privileged, true))
        {
            return Err(denied());
        }
        return Ok(());
    };
    let requested_by_id = requested
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<HashMap<_, _>>();
    for existing in &previous.entries {
        if group_scope.get(&existing.id) == Some(&false) {
            if requested_by_id
                .get(existing.id.as_str())
                .is_some_and(|entry| **entry == *existing)
            {
                continue;
            }
            if caller_privileged && !requested_by_id.contains_key(existing.id.as_str()) {
                continue;
            }
            return Err(fleet_group_access_denied());
        }
        let owns_existing = existing.primary_owner_sid.as_deref() == Some(caller_sid);
        let manages_group = caller_privileged && group_scope.get(&existing.id) == Some(&true);
        match requested_by_id.get(existing.id.as_str()) {
            None if owns_existing || caller_privileged => {}
            None => return Err(denied()),
            Some(replacement) if !owns_existing && !manages_group => {
                if *replacement != existing {
                    return Err(denied());
                }
            }
            Some(replacement) => {
                validate_primary_owner_transfer(existing, replacement, caller_privileged)?;
                if replacement.primary_owner_sid != existing.primary_owner_sid
                    && !owns_existing
                    && !manages_group
                {
                    return Err(denied());
                }
            }
        }
    }
    for entry in &requested.entries {
        if previous.entries.iter().all(|old| old.id != entry.id)
            && !new_vault_owner_allowed(entry, caller_sid, caller_privileged, false)
        {
            return Err(denied());
        }
    }
    Ok(())
}

fn handle_vault_list_principals(
    vault_access: &VaultAccessStore,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    if !args.is_null() && args != serde_json::json!({}) {
        return Err(VerbError::new(
            "vault_validation_failed",
            "principal list request is invalid",
        ));
    }
    let peer = peer
        .ok_or_else(|| VerbError::new("vault_not_authorized", "vault owner session unavailable"))?;
    let directory = vault_access.access_directory().map_err(|_| {
        VerbError::new(
            "vault_directory_unavailable",
            "vault owner list is unavailable",
        )
    })?;
    let mut directory_principals = directory
        .users
        .into_iter()
        .map(|user| wincmd_shared::vault_access::VaultKnownPrincipal {
            sid: user.sid,
            display_name: user.display_name.unwrap_or(user.username),
            is_local_administrator: false,
        })
        .collect::<Vec<_>>();
    let discovered = crate::vault_access::local_user_principals().map_err(|_| {
        VerbError::new(
            "vault_directory_unavailable",
            "Windows user accounts are unavailable",
        )
    })?;
    let administrators = discovered
        .iter()
        .filter(|user| user.is_local_administrator)
        .cloned()
        .collect();
    directory_principals.extend(discovered);
    let caller_label = crate::vault_access::account_label_for_sid(peer.caller_sid())
        // A Windows token SID is still the only truthful identifier if its
        // account was removed between authentication and this request.
        .unwrap_or_else(|| peer.caller_sid().to_owned());
    serde_json::to_value(build_known_principals_response(
        peer.caller_sid(),
        caller_label,
        directory_principals,
        administrators,
    ))
    .map_err(|_| VerbError::new("vault_internal_error", "vault owner list is unavailable"))
}

fn build_known_principals_response(
    caller_sid: &str,
    caller_label: String,
    directory_principals: Vec<wincmd_shared::vault_access::VaultKnownPrincipal>,
    administrators: Vec<wincmd_shared::vault_access::VaultKnownPrincipal>,
) -> wincmd_shared::vault_access::VaultKnownPrincipalsResponse {
    let mut principals = HashMap::new();
    for principal in directory_principals {
        principals.insert(principal.sid.clone(), principal);
    }
    for administrator in administrators {
        // Windows' live account lookup wins over a directory display alias.
        principals.insert(administrator.sid.clone(), administrator);
    }
    let caller_is_administrator = principals
        .get(caller_sid)
        .is_some_and(|principal| principal.is_local_administrator);
    principals.insert(
        caller_sid.to_owned(),
        wincmd_shared::vault_access::VaultKnownPrincipal {
            sid: caller_sid.to_owned(),
            display_name: caller_label,
            is_local_administrator: caller_is_administrator,
        },
    );
    let mut principals = principals.into_values().collect::<Vec<_>>();
    principals.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.sid.cmp(&right.sid))
    });
    wincmd_shared::vault_access::VaultKnownPrincipalsResponse {
        current_caller_sid: caller_sid.to_owned(),
        principals,
    }
}

/// Removes only the selected service-owned policy record after a degraded ACL
/// application. Active entries are dismounted first so no live mount becomes
/// orphaned from its durable policy record; this path never invokes the
/// ACL/group layers, so Windows permissions remain exactly as they were.
/// Exact policy identity/version input prevents a stale renderer from
/// forgetting a policy which was subsequently changed by another admin.
fn handle_vault_forget_entry_policy_only(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
    caller_privileged: bool,
) -> Result<serde_json::Value, VerbError> {
    if !caller_privileged {
        return Err(VerbError::new(
            "vault_not_authorized",
            "only a local administrator may remove another user's vault policy",
        ));
    }
    let request: wincmd_shared::vault_access::VaultForgetEntryPolicyOnlyRequest =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "forget policy recovery request is invalid",
            )
        })?;
    vault_mount.with_exclusive_operation(|| {
        peer.map(AuthenticatedPipePeer::caller_sid)
            .filter(|sid| !sid.is_empty())
            .ok_or_else(fleet_group_access_denied)?;
        let (ids, identities) = vault_access
            .policy_removal_targets(&request.entry_id)
            .map_err(|error| {
                VerbError::new("vault_forget_policy_failed", vault_error_message(error))
            })?;
        validate_vault_targets_unmounted(vault_mount, &ids, &identities)?;
        vault_access
            .forget_entry_policy_only(
                &request.entry_id,
                &request.policy_id,
                request.expected_version,
            )
            .and_then(|status| {
                serde_json::to_value(status)
                    .map_err(|_| crate::vault_access::VaultError::Persistence)
            })
            .map_err(|error| {
                VerbError::new("vault_forget_policy_failed", vault_error_message(error))
            })
    })
}

/// Reads the durable, machine-owned Access control directory. This stays a
/// privileged operation because it contains local account SIDs and group
/// membership, rather than a Vault member's filtered mount projection.
fn handle_vault_get_access_directory(
    vault_access: &VaultAccessStore,
    args: serde_json::Value,
) -> Result<serde_json::Value, VerbError> {
    if !args.is_null() && args != serde_json::json!({}) {
        return Err(VerbError::new(
            "vault_validation_failed",
            "access directory request is invalid",
        ));
    }
    vault_access
        .access_directory()
        .and_then(|mut directory| {
            merge_discovered_directory_users(
                &mut directory,
                crate::vault_access::local_user_principals()?,
            );
            serde_json::to_value(directory)
                .map_err(|_| crate::vault_access::VaultError::Persistence)
        })
        .map_err(|error| VerbError::new("vault_directory_unavailable", vault_error_message(error)))
}

fn merge_discovered_directory_users(
    directory: &mut wincmd_shared::vault_access::VaultAccessDirectory,
    users: Vec<wincmd_shared::vault_access::VaultKnownPrincipal>,
) {
    for user in users {
        let record = wincmd_shared::vault_access::VaultAccessDirectoryUser {
            sid: user.sid.clone(),
            username: user.display_name.clone(),
            display_name: Some(user.display_name),
        };
        if let Some(existing) = directory
            .users
            .iter_mut()
            .find(|existing| existing.sid == user.sid)
        {
            *existing = record;
        } else {
            directory.users.push(record);
        }
    }
    directory
        .users
        .sort_by(|left, right| left.username.cmp(&right.username));
}

/// Authenticates the creator and verifies Windows membership before saving.
fn handle_vault_save_access_directory(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    caller_sid: Option<&str>,
) -> Result<serde_json::Value, VerbError> {
    let caller_sid = caller_sid.filter(|sid| !sid.is_empty()).ok_or_else(|| {
        VerbError::new(
            "vault_not_authorized",
            "Windows could not confirm the group creator. Reopen WinCommander and retry.",
        )
    })?;
    let request: wincmd_shared::vault_access::VaultSaveAccessDirectoryRequest =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "access directory request is invalid",
            )
        })?;
    let (directory, results) = vault_mount
        .with_exclusive_operation(|| {
            let ensure_unmounted = || {
                if vault_mount.has_active_mounts_locked() {
                    Err(crate::vault_access::VaultError::Mounted)
                } else {
                    Ok(())
                }
            };
            let (directory, results) = vault_access.save_access_directory_for_caller(
                request.directory,
                caller_sid,
                ensure_unmounted,
            )?;
            let membership_changed = results.iter().any(|result| {
                matches!(
                    result.state,
                    wincmd_shared::vault_access::VaultAccessGroupState::Created
                        | wincmd_shared::vault_access::VaultAccessGroupState::Updated
                )
            });
            if membership_changed
                || (!results.is_empty() && vault_access.active_policy_uses_access_directory_group())
            {
                ensure_unmounted()?;
                let applied_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs() as i64)
                    .unwrap_or(0);
                vault_access.refresh_active_policy_for_access_directory_change(applied_at)?;
            }
            Ok((directory, results))
        })
        .map_err(vault_group_update_error)?;
    serde_json::to_value(
        wincmd_shared::vault_access::VaultSaveAccessDirectoryResponse { directory, results },
    )
    .map_err(|_| {
        VerbError::new(
            "vault_internal_error",
            "access directory response could not be created",
        )
    })
}

/// Read-only, caller-scoped diagnostic for a manually selected personal Vault.
/// The response is a fixed state label: it never discloses an owner SID,
/// container identity, ACL, or secret.
fn handle_personal_vault_status(
    vault_access: &VaultAccessStore,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let path = args
        .get("container_path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| {
            VerbError::new("vault_validation_failed", "personal vault path is invalid")
        })?;
    let peer = peer.ok_or_else(|| {
        VerbError::new(
            PERSONAL_VAULT_SESSION_ABSENT,
            "no interactive Windows session",
        )
    })?;
    let state = vault_access
        .personal_registration_state(path, peer.caller_sid())
        .map_err(|_| {
            VerbError::new(
                "vault_personal_status_failed",
                "personal vault status unavailable",
            )
        })?;
    let state = match state {
        crate::vault_access::PersonalVaultRegistrationState::CallerOwned => "caller_owned",
        crate::vault_access::PersonalVaultRegistrationState::RegisteredElsewhere => {
            "registered_elsewhere"
        }
        crate::vault_access::PersonalVaultRegistrationState::IdentityChanged => "identity_changed",
        crate::vault_access::PersonalVaultRegistrationState::Unregistered => "unregistered",
    };
    Ok(serde_json::json!({ "state": state }))
}

fn handle_vault_authorize(
    vault_access: &VaultAccessStore,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let request: wincmd_shared::vault_access::VaultAuthorizeMountRequest =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "mount authorization request is invalid",
            )
        })?;
    // The SID/group decision comes from this connection's named-pipe client
    // token.  The renderer supplies only an opaque registered entry id.
    let authorization = peer
        .map(|peer| {
            crate::vault_access::authorize_mount_for_token(
                vault_access,
                &request.entry_id,
                peer.token(),
            )
        })
        .unwrap_or_else(vault_authorization_denied);
    Ok(serde_json::to_value(authorization)
        .unwrap_or_else(|_| serde_json::json!({"allowed":false,"launch_ready":false,"denial_reason":"not_authorized","mode":null,"presentation":null,"preferred_letter":null})))
}

/// Personal creation is intentionally service-mediated even though the native
/// engine is launched in the caller's session.  That gives the user normal
/// file-placement semantics while the SYSTEM service remains the source of
/// truth for the owner record and the protected container DACL.
async fn handle_personal_vault_create(
    operation_id: u64,
    vault_access: &VaultAccessStore,
    mut args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let started = Instant::now();
    let diagnostic_operation_id = format!("VLT-{operation_id}");
    let Some(peer) = peer else {
        zeroize_json(&mut args);
        crate::diagnostics::record_vault_failure(
            &diagnostic_operation_id,
            "create",
            "VLT.CREATE.SESSION_UNAVAILABLE",
            "sign_in_interactively",
            true,
            started,
        );
        return Err(VerbError::new(
            PERSONAL_VAULT_SESSION_ABSENT,
            "no interactive Windows session",
        ));
    };
    if peer.caller_sid().is_empty() || !peer_has_active_interactive_session(peer) {
        zeroize_json(&mut args);
        crate::diagnostics::record_vault_failure(
            &diagnostic_operation_id,
            "create",
            "VLT.CREATE.SESSION_UNAVAILABLE",
            "sign_in_interactively",
            true,
            started,
        );
        return Err(VerbError::new(
            PERSONAL_VAULT_SESSION_ABSENT,
            "no interactive Windows session",
        ));
    }
    let target = parse_personal_vault_create_target(&args).map_err(|(kind, message)| {
        zeroize_json(&mut args);
        crate::diagnostics::record_vault_failure(
            &diagnostic_operation_id,
            "create",
            kind,
            "review_selected_target",
            false,
            started,
        );
        VerbError::new("vault_validation_failed", message)
    })?;
    // Prepare only the fixed, service-owned driver. The native engine remains
    // in the authenticated caller's session so it never needs a UAC prompt to
    // create a file in that user's chosen location.
    if let Err(error) = ensure_vault_driver_for_personal_operation(operation_id).await {
        zeroize_json(&mut args);
        crate::diagnostics::record_vault_failure(
            &diagnostic_operation_id,
            "create",
            "VLT.CREATE.DRIVER_UNAVAILABLE",
            "repair_wincommander",
            true,
            started,
        );
        return Err(VerbError::new(
            PERSONAL_VAULT_DRIVER_STOPPED,
            error.public_message(),
        ));
    }
    // File containers receive the durable owner record and file identity
    // reservation that has always guarded their lifecycle.  A raw device has
    // no stable file identity or ACL to persist, so it deliberately avoids
    // that route.  It is admitted only with a complete reviewed identity;
    // the signed Pro broker then re-probes every identity field immediately
    // before it can format the partition.
    let registration = if let PersonalVaultCreateTarget::File { path } = target {
        let now = crate::vault_access::unix_time_seconds();
        let registration = vault_access
            .begin_personal_registration_as_caller(
                &path,
                crate::vault_access::PersonalCreationCaller {
                    owner_sid: peer.caller_sid(),
                    session_id: peer.session_id(),
                    client_pid: peer.client_pid(),
                    authentication_id: peer.authentication_id(),
                },
                peer.token(),
                operation_id,
                now,
            )
            .map_err(|_| {
                crate::diagnostics::record_vault_failure(
                    &diagnostic_operation_id,
                    "create",
                    "VLT.CREATE.OWNER_RECORD_FAILED",
                    "review_destination",
                    false,
                    started,
                );
                VerbError::new(
                    "vault_owner_record_failed",
                    "personal vault ownership could not be recorded",
                )
            })?;
        args["Path"] = serde_json::Value::String(registration.normalized_path().to_string());
        Some(registration)
    } else {
        None
    };
    args["TargetSessionId"] = serde_json::Value::from(peer.session_id());
    // Windows filesystem formatting requires privilege for true standard users.
    // SYSTEM receives only a protected staging target, never their chosen path.
    let mut staging = if let Some(registration) = registration
        .as_ref()
        .filter(|_| !token_is_privileged(peer.token()).unwrap_or(false))
    {
        Some(vault_create_staging::Creation::prepare(&mut args, registration.normalized_path(), peer.token()).map_err(|_| {
            zeroize_json(&mut args);
            vault_access.cancel_personal_registration(registration);
            VerbError::new("vault_creation_preparation_failed", "The new Vault could not be prepared. Choose a new writable filename and allow free disk space for two encrypted copies; keyfiles must be readable and no larger than 64 MB.")
        })?)
    } else {
        None
    };
    let mut result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
            crate::pro_broker::VaultCall {
                request_id: operation_id,
                target_session_id: if staging.is_some() {
                    0
                } else {
                    peer.session_id()
                },
                caller_sid: peer.caller_sid(),
                caller_token: Some(peer.token()),
                caller_authentication_id: Some(peer.authentication_id()),
                presentation: if staging.is_some() {
                    wincmd_shared::vault_access::VaultPresentation::Machine
                } else {
                    wincmd_shared::vault_access::VaultPresentation::PerUser
                },
                feature_id: "vault.broker.create_personal",
                args,
            },
        ))
    })
    .map_err(|reason| {
        if let Some(registration) = &registration {
            vault_access.cancel_personal_registration(registration);
        }
        crate::diagnostics::record_vault_failure(
            &diagnostic_operation_id,
            "create",
            "VLT.CREATE.BROKER_FAILED",
            "review_create_diagnostics",
            false,
            started,
        );
        personal_vault_creation_failure(reason)
    })?;
    if let Some(staging) = &mut staging {
        staging.publish(&mut result).map_err(|_| {
            if let Some(registration) = &registration { vault_access.cancel_personal_registration(registration); }
            VerbError::new("vault_creation_verification_failed", "The encrypted Vault could not be copied and verified at the selected destination. No completed Vault was saved.")
        })?;
    }
    if let Some(registration) = &registration {
        let broker_path = result
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if vault_access
            .record_personal_broker_completion_as_caller(
                registration,
                broker_path,
                peer.token(),
                crate::vault_access::unix_time_seconds(),
            )
            .is_err()
        {
            vault_access.cancel_personal_registration(registration);
            crate::diagnostics::record_vault_failure(
                &diagnostic_operation_id,
                "create",
                "VLT.CREATE.VERIFICATION_FAILED",
                "do_not_use_created_target",
                false,
                started,
            );
            return Err(VerbError::new(
                "vault_creation_verification_failed",
                "personal vault creation could not be verified",
            ));
        }
        vault_access
            .complete_personal_registration_as_caller(
                registration,
                peer.token(),
                crate::vault_access::unix_time_seconds(),
            )
            .map_err(|_| {
                vault_access.cancel_personal_registration(registration);
                crate::diagnostics::record_vault_failure(
                    &diagnostic_operation_id,
                    "create",
                    "VLT.CREATE.OWNER_RECORD_FAILED",
                    "review_destination",
                    false,
                    started,
                );
                VerbError::new(
                    "vault_owner_record_failed",
                    "personal vault ownership could not be recorded",
                )
            })?;
    }
    if let Some(staging) = &mut staging {
        staging.commit();
    }
    crate::diagnostics::record_vault_create_success(&diagnostic_operation_id, started);
    Ok(result)
}

/// The service does not accept an arbitrary raw path for a destructive
/// creation.  `TargetKind=device` must carry the complete partition identity
/// the user reviewed in the UI.  The trusted broker independently compares
/// these fields to the live partition before formatting it.
enum PersonalVaultCreateTarget {
    File { path: String },
    Device,
}

fn parse_personal_vault_create_target(
    args: &serde_json::Value,
) -> Result<PersonalVaultCreateTarget, (&'static str, &'static str)> {
    match args.get("TargetKind").and_then(serde_json::Value::as_str) {
        None | Some("") | Some("file") => {
            let path = args
                .get("Path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if path.is_empty() || !Path::new(path).is_absolute() {
                return Err((
                    "VLT.CREATE.REQUEST_INVALID",
                    "personal vault path is invalid",
                ));
            }
            Ok(PersonalVaultCreateTarget::File {
                path: path.to_string(),
            })
        }
        Some("device") => {
            if args
                .get("Path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| !path.trim().is_empty())
                || !complete_device_create_identity(args)
            {
                return Err((
                    "VLT.CREATE.DEVICE_IDENTITY_INVALID",
                    "selected partition identity is incomplete or invalid",
                ));
            }
            Ok(PersonalVaultCreateTarget::Device)
        }
        _ => Err((
            "VLT.CREATE.REQUEST_INVALID",
            "personal vault target is invalid",
        )),
    }
}

/// A renderer value is not enough to identify a raw partition.  Require the
/// immutable disk/partition tuple and its reviewed GUID, offset, size, and
/// disk ID.  Keep this structural check local; the signed Pro broker performs
/// the authoritative live Windows re-probe before it writes anything.
fn complete_device_create_identity(args: &serde_json::Value) -> bool {
    let unsigned = |key: &str, nonzero: bool, max: u64| {
        let value = match args.get(key) {
            Some(serde_json::Value::Number(value)) => value.as_u64(),
            // The desktop backend serializes invocation arguments as strings.
            // Admit only canonical decimal text; signs, whitespace, floats,
            // and exponent notation all fail closed.
            Some(serde_json::Value::String(value))
                if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                value.parse::<u64>().ok()
            }
            _ => None,
        };
        value.is_some_and(|value| value <= max && (!nonzero || value > 0))
    };
    let bounded_token = |key: &str, max: usize| {
        args.get(key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| {
                let value = value.trim();
                !value.is_empty()
                    && value.len() <= max
                    && value.is_ascii()
                    && !value.chars().any(char::is_control)
            })
    };
    unsigned("DeviceDiskNumber", false, u32::MAX.into())
        && unsigned("DevicePartitionNumber", true, u32::MAX.into())
        && unsigned("DeviceOffsetBytes", true, u64::MAX)
        && unsigned("DeviceSizeBytes", true, u64::MAX)
        && bounded_token("DevicePartitionGuid", 128)
        && bounded_token("DeviceDiskUniqueId", 512)
}

/// A fixed-payload repair may require the signed Pro helper to copy the
/// service-owned driver into its protected location. It receives neither a
/// caller path nor credentials; mounting is authorized separately against the
/// authenticated desktop user's Windows access.
async fn ensure_vault_driver_for_personal_operation(
    request_id: u64,
) -> Result<(), crate::encvol_driver::EnsureDriverError> {
    let mut driver_check =
        tokio::task::spawn_blocking(crate::encvol_driver::ensure_for_vault_mount)
            .await
            .unwrap_or(Err(
                crate::encvol_driver::EnsureDriverError::ServiceInspection,
            ));
    if matches!(
        &driver_check,
        Err(crate::encvol_driver::EnsureDriverError::PayloadValidation)
    ) {
        let prepared = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                crate::pro_broker::VaultCall {
                    request_id,
                    target_session_id: 0,
                    caller_sid: "S-1-5-18",
                    caller_token: None,
                    caller_authentication_id: None,
                    presentation: wincmd_shared::vault_access::VaultPresentation::Machine,
                    feature_id: "vault.broker.prepare_driver",
                    args: serde_json::json!({}),
                },
            ))
        });
        if prepared.is_ok() {
            driver_check =
                tokio::task::spawn_blocking(crate::encvol_driver::ensure_for_vault_mount)
                    .await
                    .unwrap_or(Err(
                        crate::encvol_driver::EnsureDriverError::ServiceInspection,
                    ));
        }
    }
    driver_check
}

fn personal_vault_creation_failure(
    reason: wincmd_shared::vault_access::VaultMountReason,
) -> VerbError {
    VerbError::new(
        VaultMountBroker::personal_mount_failure_code(reason),
        "personal vault engine could not create the container",
    )
}

/// Pre-flight failures are intentionally separate from native-engine failures:
/// operators can fix an owner ACL, a busy letter, a missing user session, or a
/// stopped driver without losing the generic engine diagnostic.
async fn handle_personal_vault_mount(
    operation_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    mut args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
    caller_privileged: bool,
) -> Result<serde_json::Value, VerbError> {
    let Some(object) = args.as_object_mut() else {
        zeroize_json(&mut args);
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal mount request is invalid",
        ));
    };
    if object.remove("personal") != Some(serde_json::Value::Bool(true)) {
        zeroize_json(&mut args);
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal mount request is invalid",
        ));
    }
    let mut request = parse_personal_mount_request(&mut args)?;
    let Some(peer) = peer else {
        zeroize_personal_mount(&mut request);
        return Err(VerbError::new(
            PERSONAL_VAULT_SESSION_ABSENT,
            "no interactive Windows session",
        ));
    };
    if peer.caller_sid().is_empty() || !peer_has_active_interactive_session(peer) {
        zeroize_personal_mount(&mut request);
        return Err(VerbError::new(
            PERSONAL_VAULT_SESSION_ABSENT,
            "no interactive Windows session",
        ));
    }
    // Recovery changes only the mounted root DACL of an ordinary, password
    // unlocked container. It is deliberately unavailable to a standard
    // account and to read-only mounts, where a DACL write cannot be honest.
    if request.repair_current_account_access && !caller_privileged {
        zeroize_personal_mount(&mut request);
        return Err(VerbError::new(
            VaultMountBroker::personal_mount_failure_code(VaultMountReason::AdministratorRequired),
            "recovering access to an ordinary Vault requires a local administrator",
        ));
    }
    if request.repair_current_account_access && request.read_only {
        zeroize_personal_mount(&mut request);
        return Err(VerbError::new(
            "vault_validation_failed",
            "a read-only Vault cannot repair Windows access",
        ));
    }
    let mut record = match vault_access.selected_container_mount_route(
        &request.container_path,
        peer.caller_sid(),
        peer.session_id(),
    ) {
        Ok(crate::vault_access::SelectedContainerMountRoute::Unmanaged { record }) => record,
        Ok(crate::vault_access::SelectedContainerMountRoute::Managed { entry_id }) => {
            let authorization = crate::vault_access::authorize_mount_for_token(
                vault_access,
                &entry_id,
                peer.token(),
            );
            zeroize_personal_mount(&mut request);
            if authorization.allowed {
                return Err(VerbError::new(
                    "vault_policy_managed",
                    "selected container is governed by Vault permissions",
                ));
            }
            return Err(VerbError::new(
                PERSONAL_VAULT_UNAUTHORIZED,
                "caller is not authorized for this Vault policy",
            ));
        }
        Err(crate::vault_access::VaultError::ContainerIdentity) => {
            zeroize_personal_mount(&mut request);
            return Err(VerbError::new(
                "vault_policy_identity_changed",
                "the Vault policy no longer matches the selected container",
            ));
        }
        Err(_) => {
            zeroize_personal_mount(&mut request);
            return Err(VerbError::new(
                "vault_policy_unavailable",
                "the selected Vault policy could not be verified",
            ));
        }
    };
    // ACL repair is an explicit recovery operation. An administrator's normal
    // personal mount must preserve the volume's existing permissions so FAT
    // and exFAT mounts do not enter the NTFS-only repair path. Policy routing
    // above keeps managed Vaults out of this personal route; the service later
    // validates an explicitly requested repair before it derives a repair SID.
    // Legacy clients omit presentation and retain their per-user scope.
    record.scope = request.presentation;
    if let Err(reason) = crate::pro_broker::vault_payload_readiness() {
        zeroize_personal_mount(&mut request);
        return Err(VerbError::new(
            VaultMountBroker::personal_mount_failure_code(reason),
            "The Vault engine module is unavailable. Check the Pro installation in License settings.",
        ));
    }
    let driver = tokio::task::spawn_blocking(crate::encvol_driver::ensure_for_vault_mount)
        .await
        .unwrap_or(Err(
            crate::encvol_driver::EnsureDriverError::ServiceInspection,
        ));
    if let Err(error) = driver {
        zeroize_personal_mount(&mut request);
        return Err(VerbError::new(
            PERSONAL_VAULT_DRIVER_STOPPED,
            error.public_message(),
        ));
    }
    // Unmanaged containers have no durable owner record. The caller's normal
    // Windows access and the engine's password/PIM/keyfile verification are
    // the authority checks; the short-lived record only pins this mount to the
    // authenticated session and identity.
    let (drive_letter, internal_drive, acl_attested, sync_warning) = vault_mount
        .with_exclusive_operation(|| {
            // Recheck under the policy/mount lock: a policy may have been saved
            // while the driver check was running.
            let current = vault_access.selected_container_mount_route(
                &request.container_path,
                peer.caller_sid(),
                peer.session_id(),
            );
            if !matches!(current,
                Ok(crate::vault_access::SelectedContainerMountRoute::Unmanaged { record: ref fresh })
                    if fresh.container_identity == record.container_identity
                        && fresh.container_path == record.container_path)
            {
                zeroize_personal_mount(&mut request);
                return Err(wincmd_shared::vault_access::VaultMountReason::NotAuthorized);
            }
            // Hold caller-authorized paths until the elevated broker finishes.
            let _mount_files = match crate::vault_access::hold_unmanaged_mount_files(
                &record, &request, peer.token(),
            ) {
                Ok(files) => files,
                Err(_) => {
                    zeroize_personal_mount(&mut request);
                    return Err(wincmd_shared::vault_access::VaultMountReason::NotAuthorized);
                }
            };
            vault_mount.mount_unmanaged_recovery_authorized_locked(
                operation_id,
                vault_access,
                &record,
                &mut request,
                peer.token(),
                peer.session_id(),
                peer.caller_sid(),
                peer.authentication_id(),
                caller_privileged,
            )
        })
        .map_err(|reason| {
            VerbError::new(
                VaultMountBroker::personal_mount_failure_code(reason),
                "unmanaged vault mount failed",
            )
        })?;
    Ok(serde_json::json!({
        "status": "mounted",
        "drive": drive_letter,
        "internalDrive": internal_drive,
        "scope": record.scope,
        "aclAttested": acl_attested,
        "syncWarning": sync_warning,
    }))
}

fn zeroize_personal_mount(request: &mut wincmd_shared::vault_access::PersonalVaultMountRequest) {
    request.zeroize_secrets();
}

fn parse_personal_mount_request(
    args: &mut serde_json::Value,
) -> Result<wincmd_shared::vault_access::PersonalVaultMountRequest, VerbError> {
    use zeroize::Zeroize;

    let invalid = || {
        VerbError::new(
            "vault_validation_failed",
            "personal mount request is invalid",
        )
    };
    let object = args.as_object_mut().ok_or_else(invalid)?;
    let mut password = match object.remove("password") {
        Some(serde_json::Value::String(value)) => value,
        Some(mut value) => {
            zeroize_json(&mut value);
            zeroize_json(args);
            return Err(invalid());
        }
        None => {
            zeroize_json(args);
            return Err(invalid());
        }
    };
    let mut hidden_password = match object.remove("hidden_protection_password") {
        Some(serde_json::Value::String(value)) => Some(value),
        Some(serde_json::Value::Null) | None => None,
        Some(mut value) => {
            password.zeroize();
            zeroize_json(&mut value);
            zeroize_json(args);
            return Err(invalid());
        }
    };
    let mut keyfiles = match take_secret_string_list(object.remove("keyfiles")) {
        Ok(values) => values,
        Err(mut value) => {
            password.zeroize();
            hidden_password.iter_mut().for_each(Zeroize::zeroize);
            zeroize_json(&mut value);
            zeroize_json(args);
            return Err(invalid());
        }
    };
    let mut hidden_keyfiles = match take_secret_string_list(object.remove("hidden_keyfiles")) {
        Ok(values) => values,
        Err(mut value) => {
            password.zeroize();
            hidden_password.iter_mut().for_each(Zeroize::zeroize);
            keyfiles.iter_mut().for_each(Zeroize::zeroize);
            zeroize_json(&mut value);
            zeroize_json(args);
            return Err(invalid());
        }
    };
    object.insert("password".into(), serde_json::Value::String(String::new()));
    let decoded = serde_json::from_value(args.take());
    let mut request: wincmd_shared::vault_access::PersonalVaultMountRequest = match decoded {
        Ok(request) => request,
        Err(_) => {
            password.zeroize();
            if let Some(value) = &mut hidden_password {
                value.zeroize();
            }
            keyfiles.iter_mut().for_each(Zeroize::zeroize);
            hidden_keyfiles.iter_mut().for_each(Zeroize::zeroize);
            return Err(invalid());
        }
    };
    request.password = password;
    request.hidden_protection_password = hidden_password;
    request.keyfiles = keyfiles;
    request.hidden_keyfiles = hidden_keyfiles;
    Ok(request)
}

fn take_secret_string_list(
    value: Option<serde_json::Value>,
) -> Result<Vec<String>, serde_json::Value> {
    use zeroize::Zeroize;

    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let serde_json::Value::Array(mut values) = value else {
        return Err(value);
    };
    let mut strings = Vec::with_capacity(values.len());
    while let Some(value) = values.pop() {
        match value {
            serde_json::Value::String(value) => strings.push(value),
            mut other => {
                strings.iter_mut().for_each(Zeroize::zeroize);
                values.iter_mut().for_each(zeroize_json);
                zeroize_json(&mut other);
                return Err(other);
            }
        }
    }
    Ok(strings)
}

fn peer_has_active_interactive_session(peer: &AuthenticatedPipePeer) -> bool {
    session_is_active(peer.session_id(), wts_connect_state(peer.session_id()))
}

fn session_is_active(session_id: u32, state: Option<WTS_CONNECTSTATE_CLASS>) -> bool {
    session_id != 0 && state == Some(WTSActive)
}

fn wts_connect_state(session_id: u32) -> Option<WTS_CONNECTSTATE_CLASS> {
    if session_id == 0 {
        return None;
    }
    unsafe {
        let mut buffer = std::ptr::null_mut();
        let mut byte_count = 0u32;
        if WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session_id,
            WTSConnectState,
            &mut buffer,
            &mut byte_count,
        ) == 0
            || buffer.is_null()
            || byte_count < std::mem::size_of::<WTS_CONNECTSTATE_CLASS>() as u32
        {
            if !buffer.is_null() {
                WTSFreeMemory(buffer.cast());
            }
            return None;
        }
        let state = (buffer as *const WTS_CONNECTSTATE_CLASS).read();
        WTSFreeMemory(buffer.cast());
        Some(state)
    }
}

async fn handle_vault_mount(
    request_id: u64,
    diagnostic_operation_id: &str,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    mut args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let started = std::time::Instant::now();
    let mut request = take_vault_mount_request(&mut args).map_err(|_| {
        crate::diagnostics::record_vault_failure(
            diagnostic_operation_id,
            "mount",
            "VLT.REQUEST.INVALID",
            "review_request",
            false,
            started,
        );
        VerbError::new("vault_validation_failed", "mount request is invalid")
    })?;
    // This must happen before a broker attempt: PID/token membership is
    // derived from the named-pipe peer, never from a renderer identity.
    let authorization = peer
        .map(|peer| {
            crate::vault_access::authorize_mount_for_token(
                vault_access,
                &request.entry_id,
                peer.token(),
            )
        })
        .unwrap_or_else(vault_authorization_denied);
    let result = if authorization.allowed {
        if let Err(reason) = crate::pro_broker::vault_payload_readiness() {
            use zeroize::Zeroize;
            request.password.zeroize();
            if let Some(password) = &mut request.hidden_protection_password {
                password.zeroize();
            }
            request.hidden_protection_password = None;
            let missing = reason == wincmd_shared::vault_access::VaultMountReason::ProNotInstalled;
            crate::diagnostics::record_vault_failure(
                diagnostic_operation_id,
                "mount",
                if missing {
                    "VLT.PRO.NOT_INSTALLED"
                } else {
                    "VLT.BROKER.UNAVAILABLE"
                },
                if missing {
                    "install_pro_module"
                } else {
                    "check_service_health"
                },
                false,
                started,
            );
            return Err(VerbError::new(
                VaultMountBroker::personal_mount_failure_code(reason),
                "The Vault engine module is unavailable. Check the Pro installation in License settings.",
            ));
        }
        // This internal guard has no pipe verb: an authenticated user may ask
        // for a policy-authorized mount, but can never name a driver, service,
        // or executable.  Validate/repair the fixed engine driver before the
        // privileged broker receives the password.
        let mut driver_check =
            tokio::task::spawn_blocking(crate::encvol_driver::ensure_for_vault_mount)
                .await
                .unwrap_or(Err(
                    crate::encvol_driver::EnsureDriverError::ServiceInspection,
                ));
        // A clean developer machine can have the authenticated Pro sidecar but
        // not yet its fixed ProgramData engine payload. Prepare that payload as
        // SYSTEM, with no password or caller-controlled path, then retry the
        // same pinned-driver validation. This also makes first vault use work
        // after a normal signed installation rather than requiring a manual
        // Settings repair first.
        if matches!(
            &driver_check,
            Err(crate::encvol_driver::EnsureDriverError::PayloadValidation)
        ) {
            // The broker owns a Windows process HANDLE and is deliberately
            // !Send. Run its short setup exchange in place, as the existing
            // mount/dismount broker paths do, so this pipe connection remains
            // safe to schedule on Tokio's multi-threaded runtime.
            let prepared = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
                    crate::pro_broker::VaultCall {
                        request_id,
                        target_session_id: 0,
                        caller_sid: "S-1-5-18",
                        caller_token: None,
                        caller_authentication_id: None,
                        presentation: wincmd_shared::vault_access::VaultPresentation::Machine,
                        feature_id: "vault.broker.prepare_driver",
                        args: serde_json::json!({}),
                    },
                ))
            });
            if prepared.is_ok() {
                driver_check =
                    tokio::task::spawn_blocking(crate::encvol_driver::ensure_for_vault_mount)
                        .await
                        .unwrap_or(Err(
                            crate::encvol_driver::EnsureDriverError::ServiceInspection,
                        ));
            }
        }
        if let Err(error) = driver_check {
            use zeroize::Zeroize;
            request.password.zeroize();
            if let Some(hidden_protection_password) = &mut request.hidden_protection_password {
                hidden_protection_password.zeroize();
            }
            request.hidden_protection_password = None;
            let error_code = match error {
                crate::encvol_driver::EnsureDriverError::PayloadValidation => {
                    "VLT.DRIVER.PAYLOAD_INVALID"
                }
                crate::encvol_driver::EnsureDriverError::ServiceInspection => {
                    "VLT.DRIVER.SERVICE_INSPECTION_FAILED"
                }
                crate::encvol_driver::EnsureDriverError::ServiceOwnership => {
                    "VLT.DRIVER.OWNERSHIP_REJECTED"
                }
                crate::encvol_driver::EnsureDriverError::ServiceCreate => {
                    "VLT.DRIVER.CREATE_FAILED"
                }
                crate::encvol_driver::EnsureDriverError::ServiceConfigure => {
                    "VLT.DRIVER.CONFIGURE_FAILED"
                }
                crate::encvol_driver::EnsureDriverError::ServiceStart => "VLT.DRIVER.START_FAILED",
            };
            crate::diagnostics::record_vault_failure(
                diagnostic_operation_id,
                "mount",
                error_code,
                "check_driver_health",
                true,
                started,
            );
            return Err(VerbError::new(
                "vault_driver_unavailable",
                error.public_message(),
            ));
        }
        let Some(peer) = peer else {
            crate::diagnostics::record_vault_failure(
                diagnostic_operation_id,
                "mount",
                "VLT.AUTH.DENIED",
                "request_authorization",
                false,
                started,
            );
            return Err(VerbError::new(
                "vault_not_authorized",
                "vault peer token unavailable",
            ));
        };
        vault_mount.mount_authorized(
            request_id,
            vault_access,
            &request.entry_id,
            &mut request.password,
            &mut request.hidden_protection_password,
            request.volume_role,
            peer.token(),
            peer.session_id(),
            peer.caller_sid(),
            peer.authentication_id(),
            authorization
                .mode
                .unwrap_or(wincmd_shared::vault_access::VaultAccess::Read),
        )
    } else {
        use zeroize::Zeroize;
        request.password.zeroize();
        if let Some(hidden_protection_password) = &mut request.hidden_protection_password {
            hidden_protection_password.zeroize();
        }
        request.hidden_protection_password = None;
        wincmd_shared::vault_access::VaultMountResult {
            entry_id: request.entry_id,
            state: wincmd_shared::vault_access::VaultMountState::Denied,
            presentation: None,
            drive_letter: None,
            reason: Some(wincmd_shared::vault_access::VaultMountReason::NotAuthorized),
            sync_warning: None,
        }
    };
    crate::diagnostics::record_vault_terminal(diagnostic_operation_id, "mount", &result, started);
    serde_json::to_value(result)
        .map_err(|_| VerbError::new("vault_internal_error", "mount result could not be created"))
}

/// Move the sole secret string out of its JSON object instead of cloning the
/// public payload. Every remaining string is overwritten before it drops.
fn take_vault_mount_request(
    args: &mut serde_json::Value,
) -> Result<wincmd_shared::vault_access::VaultMountRequest, ()> {
    use zeroize::Zeroize;
    let object = args.as_object_mut().ok_or(())?;
    if !(2..=4).contains(&object.len()) {
        zeroize_json(args);
        return Err(());
    }
    let mut entry_id = match object.remove("entry_id") {
        Some(serde_json::Value::String(value)) => value,
        _ => {
            zeroize_json(args);
            return Err(());
        }
    };
    let password = match object.remove("password") {
        Some(serde_json::Value::String(value)) => value,
        _ => {
            entry_id.zeroize();
            zeroize_json(args);
            return Err(());
        }
    };
    let volume_role = match object.remove("volume_role") {
        None => wincmd_shared::vault_access::VaultVolumeRole::Outer,
        Some(serde_json::Value::String(value)) if value == "outer" => {
            wincmd_shared::vault_access::VaultVolumeRole::Outer
        }
        Some(serde_json::Value::String(value)) if value == "hidden" => {
            wincmd_shared::vault_access::VaultVolumeRole::Hidden
        }
        _ => {
            entry_id.zeroize();
            let mut password = password;
            password.zeroize();
            zeroize_json(args);
            return Err(());
        }
    };
    let hidden_protection_password = match object.remove("hidden_protection_password") {
        None => None,
        Some(serde_json::Value::String(value)) => Some(value),
        _ => {
            entry_id.zeroize();
            let mut password = password;
            password.zeroize();
            zeroize_json(args);
            return Err(());
        }
    };
    if !object.is_empty() {
        entry_id.zeroize();
        let mut password = password;
        password.zeroize();
        if let Some(mut hidden_protection_password) = hidden_protection_password {
            hidden_protection_password.zeroize();
        }
        zeroize_json(args);
        return Err(());
    }
    zeroize_json(args);
    Ok(wincmd_shared::vault_access::VaultMountRequest {
        entry_id,
        password,
        volume_role,
        hidden_protection_password,
    })
}

fn zeroize_json(value: &mut serde_json::Value) {
    use zeroize::Zeroize;
    match value {
        serde_json::Value::String(text) => text.zeroize(),
        serde_json::Value::Array(values) => {
            for value in values {
                zeroize_json(value);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                zeroize_json(value);
            }
        }
        _ => {}
    }
}

fn vault_authorization_denied() -> wincmd_shared::vault_access::VaultAuthorizeMountResponse {
    wincmd_shared::vault_access::VaultAuthorizeMountResponse {
        allowed: false,
        launch_ready: false,
        denial_reason: Some(wincmd_shared::vault_access::VaultMountDenial::NotAuthorized),
        mode: None,
        presentation: None,
        preferred_letter: None,
    }
}

fn handle_vault_unmount(
    request_id: u64,
    diagnostic_operation_id: &str,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let started = std::time::Instant::now();
    let request: wincmd_shared::vault_access::VaultUnmountRequest = serde_json::from_value(args)
        .map_err(|_| {
            crate::diagnostics::record_vault_failure(
                diagnostic_operation_id,
                "dismount",
                "VLT.REQUEST.INVALID",
                "review_request",
                false,
                started,
            );
            VerbError::new("vault_validation_failed", "unmount request is invalid")
        })?;
    let result = if let Some(peer) = peer {
        vault_mount.dismount_authorized(
            vault_access,
            crate::vault_mount::AuthorizedDismount {
                caller_authentication_id: Some(peer.authentication_id()),
                operation_id: request_id,
                entry_id: &request.entry_id,
                caller_token: peer.token(),
                caller_session: peer.session_id(),
                caller_sid: peer.caller_sid(),
                caller_elevated: token_is_privileged(peer.token()).unwrap_or(false),
            },
        )
    } else {
        wincmd_shared::vault_access::VaultMountResult {
            entry_id: request.entry_id,
            state: wincmd_shared::vault_access::VaultMountState::Denied,
            presentation: None,
            drive_letter: None,
            reason: Some(wincmd_shared::vault_access::VaultMountReason::MountStateUnknown),
            sync_warning: None,
        }
    };
    crate::diagnostics::record_vault_terminal(
        diagnostic_operation_id,
        "dismount",
        &result,
        started,
    );
    serde_json::to_value(result).map_err(|_| {
        VerbError::new(
            "vault_internal_error",
            "unmount result could not be created",
        )
    })
}

fn require_personal_mount_peer(
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<&AuthenticatedPipePeer, VerbError> {
    peer.filter(|peer| !peer.caller_sid().is_empty() && peer_has_active_interactive_session(peer))
        .ok_or_else(|| {
            VerbError::new(
                PERSONAL_VAULT_SESSION_ABSENT,
                "no interactive Windows session",
            )
        })
}

fn valid_personal_mount_query(args: &serde_json::Value) -> bool {
    vault_inventory_contract::parse_query(args).is_some()
}

fn handle_personal_vault_list(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: &serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    if !valid_personal_mount_query(args) {
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal mount query is invalid",
        ));
    }
    let peer = require_personal_mount_peer(peer)?;
    if vault_inventory_contract::parse_query(args)
        == Some(vault_inventory_contract::InventoryVersion::VerifyCleanup)
        && vault_mount.has_untracked_mounts().map_err(|_| {
            VerbError::new("vault_mount_state_unknown", "cleanup status unavailable")
        })?
    {
        return Err(VerbError::new("vault_untracked_mounts", "untracked driver mounts remain"));
    }
    let mounts = vault_mount
        .personal_mounts_for_caller(
            vault_access,
            peer.token(),
            peer.session_id(),
            peer.caller_sid(),
            token_is_privileged(peer.token()).unwrap_or(false),
        )
        .map_err(|reason| {
            VerbError::new(
                VaultMountBroker::personal_mount_failure_code(reason),
                "personal mount list unavailable",
            )
        })?;
    vault_inventory_contract::reply(
        vault_inventory_contract::parse_query(args).expect("validated inventory request"),
        &mounts,
    )
        .map_err(|_| VerbError::new("vault_internal_error", "personal mount list unavailable"))
}

fn handle_personal_vault_dismount(
    request_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    mut args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let Some(object) = args.as_object_mut() else {
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal dismount request is invalid",
        ));
    };
    if object.remove("personal") != Some(serde_json::Value::Bool(true)) {
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal dismount request is invalid",
        ));
    }
    let request: wincmd_shared::vault_access::PersonalVaultDismountRequest =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "personal dismount request is invalid",
            )
        })?;
    if request.internal_drive > 25 {
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal dismount request is invalid",
        ));
    }
    let peer = require_personal_mount_peer(peer)?;
    let result = vault_mount.dismount_personal_for_caller(
        vault_access,
        request_id,
        request.internal_drive,
        peer.token(),
        peer.session_id(),
        peer.caller_sid(),
        token_is_privileged(peer.token()).unwrap_or(false),
    );
    serde_json::to_value(result)
        .map_err(|_| VerbError::new("vault_internal_error", "personal dismount unavailable"))
}

fn handle_personal_vault_syncthing_manage(
    request_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let request: wincmd_shared::vault_sync::VaultSyncManagementRequest =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "personal sync request is invalid",
            )
        })?;
    if !request.valid() {
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal sync request is invalid",
        ));
    }
    let peer = require_personal_mount_peer(peer)?;
    let result = vault_mount
        .manage_personal_syncthing(
            vault_access,
            request_id,
            &request,
            peer.token(),
            peer.session_id(),
            peer.caller_sid(),
        )
        .map_err(|reason| {
            VerbError::new(
                VaultMountBroker::personal_mount_failure_code(reason),
                "personal sync management could not be confirmed",
            )
        })?;
    serde_json::to_value(result)
        .map_err(|_| VerbError::new("vault_internal_error", "personal sync result unavailable"))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalVaultSyncthingEnrollmentRequest {
    personal: bool,
    internal_drive: u8,
    relative_path: String,
    #[serde(default)]
    recovery_action: Option<crate::vault_syncthing_recovery::RecoveryAction>,
    #[serde(default)]
    recovery_token: Option<String>,
    #[serde(default)]
    expected_mount_receipt: Option<String>,
}

fn handle_personal_vault_syncthing_enroll(
    request_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let request: PersonalVaultSyncthingEnrollmentRequest =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "personal sync enrollment request is invalid",
            )
    })?;
    if !request.personal
        || request.internal_drive > 25
        || request.relative_path.len() > 240
        || request
            .expected_mount_receipt
            .as_deref()
            .is_some_and(|value| value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal sync enrollment request is invalid",
        ));
    }
    let peer = require_personal_mount_peer(peer)?;
    let enrollment = vault_mount
        .enroll_personal_syncthing(
            vault_access,
            request_id,
            request.internal_drive,
            &request.relative_path,
            peer.token(),
            peer.session_id(),
            peer.caller_sid(),
            &crate::vault_syncthing_recovery::RecoveryOptions {
                action: request.recovery_action,
                token: request.recovery_token,
            },
            request.expected_mount_receipt.as_deref(),
        )
        .map_err(|reason| {
            VerbError::new(
                VaultMountBroker::personal_mount_failure_code(reason),
                "personal sync enrollment could not be confirmed",
            )
        })?;
    Ok(serde_json::json!({
        "enabled": enrollment.managed,
        "folder_id": enrollment.folder_id,
        "gui_url": enrollment.gui_url,
        "recovery_required": enrollment.recovery_required,
        "recovery_roots": enrollment.recovery_roots,
        "pairing_required": enrollment.pairing_required,
    }))
}

fn handle_release_orphaned_vault_drive_letters(
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    if args != serde_json::json!({}) {
        return Err(VerbError::new(
            "vault_validation_failed",
            "drive-letter repair request is invalid",
        ));
    }
    let peer = require_personal_mount_peer(peer)?;
    // The namespace comes from the authenticated token, never a selected user or letter.
    vault_mount.with_exclusive_operation(|| {
        let cleanup_error = |_| VerbError::new(
            "vault_drive_letter_cleanup_failed",
            "Unavailable Vault drive letters could not be checked.",
        );
        let released_old = crate::vault_drive_letters::release_orphaned_logon_encrypted_links()
            .map_err(cleanup_error)?;
        let released_global = crate::vault_drive_letters::release_orphaned_global_encrypted_links()
            .map_err(cleanup_error)?;
        let released_caller = crate::vault_drive_letters::release_orphaned_caller_encrypted_links(
            peer.authentication_id(),
        ).map_err(cleanup_error)?;
        Ok(serde_json::json!({ "released": released_old + released_global + released_caller }))
    })
}
fn handle_vault_list_authorized(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let entries: Vec<wincmd_shared::vault_access::VaultAuthorizedEntry> = vault_access
        .entry_summaries()
        .into_iter()
        .filter_map(|(entry_id, label, container_kind, preferred_letter)| {
            let authorization = peer
                .map(|peer| {
                    crate::vault_access::authorize_mount_for_token(
                        vault_access,
                        &entry_id,
                        peer.token(),
                    )
                })
                .unwrap_or_else(vault_authorization_denied);
            if !authorization.allowed {
                return None;
            }
            let (mount_state, drive_letter) = vault_mount.projection(&entry_id);
            Some(wincmd_shared::vault_access::VaultAuthorizedEntry {
                entry_id,
                label,
                access: authorization.mode?,
                presentation: authorization.presentation?,
                container_kind,
                mount_state,
                drive_letter,
                preferred_letter,
            })
        })
        .collect();
    serde_json::to_value(entries).map_err(|_| {
        VerbError::new(
            "vault_internal_error",
            "authorized vault list could not be created",
        )
    })
}

/// Renders a [`crate::vault_access::VaultError`] for an `ErrorReply.message`.
///
/// PRIVACY BOUNDARY: `PrincipalResolution`'s payload is the admin-supplied
/// principal or local-group name — the admin typed it, so echoing it back is
/// in scope. No other variant carries, and this function must never format
/// in, a resolved SID, a container path, or ACL/SDDL detail.
fn vault_error_message(error: crate::vault_access::VaultError) -> String {
    match error {
        crate::vault_access::VaultError::Forbidden => "You are not a member of this Fleet access group. Its settings were not changed.".to_string(),
        crate::vault_access::VaultError::Mounted => "Dismount the Vault before changing its access groups. No drive was dismounted automatically.".to_string(),
        crate::vault_access::VaultError::GroupInUse => "This group is assigned to a Vault policy. Remove its Vault assignments before deleting or renaming the Windows group.".to_string(),
        crate::vault_access::VaultError::GroupNameConflict => "That Windows group already exists. Choose a different Windows group name; its membership was not changed.".to_string(),
        crate::vault_access::VaultError::Validation => {
            "vault policy failed validation — check drive letters, container paths, and duplicate entries".to_string()
        }
        crate::vault_access::VaultError::VersionConflict => {
            "vault policy was changed elsewhere since this draft was loaded — reload the Vault tab and reapply".to_string()
        }
        crate::vault_access::VaultError::PrincipalResolution(name) => {
            format!("vault principal resolution failed for '{name}'")
        }
        crate::vault_access::VaultError::ContainerIdentity => {
            "vault container identity validation failed".to_string()
        }
        crate::vault_access::VaultError::PolicyPathReserved => {
            "the selected container is already governed by Vault permissions — use that Vault in Fleet or choose a different file".to_string()
        }
        crate::vault_access::VaultError::AclApply => {
            "vault access plan could not be applied".to_string()
        }
        crate::vault_access::VaultError::AclReadback => {
            "vault access plan read-back failed".to_string()
        }
        crate::vault_access::VaultError::Persistence => {
            "vault policy could not be persisted".to_string()
        }
    }
}

fn vault_group_update_error(error: crate::vault_access::VaultError) -> VerbError {
    let kind = match error {
        crate::vault_access::VaultError::Forbidden => "vault_not_authorized",
        crate::vault_access::VaultError::Mounted => "vault_policy_mounted",
        crate::vault_access::VaultError::GroupInUse => "vault_group_in_use",
        crate::vault_access::VaultError::GroupNameConflict => "vault_group_name_conflict",
        _ => "vault_access_group_apply_failed",
    };
    VerbError::new(kind, vault_error_message(error))
}

/// The two epoch subtree keys `EpochInstallInput.config` may carry (plan
/// §4.4 / `policy_store::SubtreeCompiler::CONFIG_KEY`). Hardcoded as
/// literals here (rather than imported) because that associated const is
/// a private implementation detail of `policy_store` — these two strings
/// are the public wire contract regardless of how that module names them
/// internally.
const CLIPBOARD_GUARD_CONFIG_KEY: &str = "clipboardGuard";
const INK_RECEIPT_CONFIG_KEY: &str = "inkReceipt";

/// Wire shape `svc.policy.install_epoch`'s `args` must deserialize into —
/// matches `commander-free/src/settings.rs`'s `InstallEpochArgs` field for
/// field (see that struct's doc comment). Field names differ from
/// [`EpochInstallInput`]'s own (`policy_version`/`signature`/`signer_key`
/// here vs. `version`/`signature_b64`/`signer_key_b64` there) because
/// `EpochInstallInput` is ALSO the on-disk persisted shape and predates
/// this wire adapter — this struct exists solely to bridge the two without
/// renaming either.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallEpochWireArgs {
    policy_version: i64,
    config: serde_json::Value,
    locked_paths: Vec<String>,
    managed: bool,
    target_kind: String,
    /// Free omits this key entirely when `None`
    /// (`#[serde(skip_serializing_if = "Option::is_none")]` on the sender
    /// side) — `#[serde(default)]` is required here so an absent key
    /// deserializes to `None` rather than a "missing field" error.
    #[serde(default)]
    target_id: Option<String>,
    signature: String,
    signer_key: String,
}

/// `svc.policy.install_epoch` (`SessionHelper`, D-2 caller 3). Free/Pro
/// relays the FULL verified epoch config regardless of which subtree
/// actually changed — there is no wire discriminator field naming a single
/// target subtree (see [`InstallEpochWireArgs`]'s doc). So this handler
/// attempts EVERY subtree whose key is present in `config`, independently:
/// each one is verified, version-gated, and compiled on its own by
/// [`PolicyStore`], exactly as if it had arrived alone. A subtree whose key
/// is simply ABSENT is not attempted at all (and is not an error) — this
/// mirrors Free's own "neither subtree mentioned -> nothing to do" stance.
///
/// `PolicyStore` re-verifies the signature and enforces the
/// monotonic-version guard itself (D-7's reasoning applied to this hop) —
/// this handler never trusts that a caller (or `authorize()`'s peer
/// pinning) already checked anything about the payload's validity.
fn handle_install_epoch(
    policy_store: &PolicyStore,
    args: serde_json::Value,
) -> Result<serde_json::Value, VerbError> {
    let wire: InstallEpochWireArgs = serde_json::from_value(args)
        .map_err(|_| VerbError::new("bad_request", "malformed install_epoch payload"))?;

    let clipboard_present = wire.config.get(CLIPBOARD_GUARD_CONFIG_KEY).is_some();
    let ink_receipt_present = wire.config.get(INK_RECEIPT_CONFIG_KEY).is_some();

    if !clipboard_present && !ink_receipt_present {
        return Ok(serde_json::json!({ "applied": Vec::<&str>::new() }));
    }

    let input = EpochInstallInput {
        version: wire.policy_version,
        config: wire.config,
        locked_paths: wire.locked_paths,
        managed: wire.managed,
        target_kind: wire.target_kind,
        target_id: wire.target_id,
        signature_b64: wire.signature,
        signer_key_b64: wire.signer_key,
    };

    let mut applied = Vec::new();
    let mut failures = Vec::new();

    if clipboard_present {
        match policy_store.install_clipboard_epoch(input.clone()) {
            Ok(()) => applied.push(CLIPBOARD_GUARD_CONFIG_KEY),
            // `PolicyStoreError`'s `Display` is content-free by
            // construction (see that type's doc) — prefixing it with the
            // fixed subtree-key literal above adds nothing path/rule/text-like.
            Err(e) => failures.push(format!("{CLIPBOARD_GUARD_CONFIG_KEY}: {e}")),
        }
    }
    if ink_receipt_present {
        match policy_store.install_ink_receipt_epoch(input) {
            Ok(()) => applied.push(INK_RECEIPT_CONFIG_KEY),
            Err(e) => failures.push(format!("{INK_RECEIPT_CONFIG_KEY}: {e}")),
        }
    }

    if failures.is_empty() {
        Ok(serde_json::json!({ "applied": applied }))
    } else {
        Err(VerbError::new("policy_rejected", failures.join("; ")))
    }
}

#[derive(serde::Deserialize)]
struct SetEnabledArgs {
    enabled: bool,
}

/// `svc.clipboard.set_enabled` (`Privileged`) — an admin-only local
/// kill-switch, in-memory only for this phase. GROUNDING §9 puts the
/// *authoritative* `clipboard_guard_enabled` toggle in Fleet's
/// `org_settings` (mirroring `fleet_privacy_shield_enabled`), which has no
/// established push path into `commander-svc` yet — see this task's
/// handoff note. This verb is a local override on top of whatever that
/// future channel eventually sets, not a replacement for it; it does not
/// persist across a service restart.
fn handle_set_enabled(
    state: &ClipboardGuardState,
    args: serde_json::Value,
) -> Result<serde_json::Value, VerbError> {
    let parsed: SetEnabledArgs = serde_json::from_value(args)
        .map_err(|_| VerbError::new("bad_request", "malformed set_enabled payload"))?;
    state.set_enabled(parsed.enabled);
    Ok(serde_json::json!({ "enabled": parsed.enabled }))
}

/// `svc.clipboard.report_event` (`SessionHelper`, D-2 caller 1). Accepts an
/// already-locally-matched [`ClipboardEventReport`] and queues it, stamped
/// with `trust_origin` (D-2's "trust-origin marker on stored receipts").
/// `ClipboardEventReport` is `#[serde(deny_unknown_fields)]` and carries
/// only scalars/closed enums/id-strings (see its own doc comment in
/// `fleet-proto`) — there is no clipboard text or rule name anywhere in
/// this type for a malformed-payload error to leak.
fn handle_report_event(
    state: &ClipboardGuardState,
    args: serde_json::Value,
    trust_origin: TrustOrigin,
) -> Result<serde_json::Value, VerbError> {
    if !state.is_enabled() {
        return Err(VerbError::new(
            "clipboard_guard_disabled",
            "clipboard guard is administratively disabled",
        ));
    }

    let report: ClipboardEventReport = serde_json::from_value(args)
        .map_err(|_| VerbError::new("bad_request", "malformed clipboard event report"))?;

    let now = Instant::now();
    state.mark_event_accepted(now);
    state.events.push(QueuedClipboardEvent {
        report,
        trust_origin,
        queued_at: now,
    });

    Ok(serde_json::json!({ "accepted": true }))
}

// ── Clipboard Guard shared state (queue + health + local toggle) ───────────

/// Bound on how many not-yet-picked-up clipboard events this service holds
/// in memory. FIFO eviction of the OLDEST entry once full — losing the
/// oldest unconfirmed telemetry is an acceptable degrade, unbounded growth
/// is not.
const CLIPBOARD_EVENT_QUEUE_CAPACITY: usize = 2000;

/// How long a queued clipboard event is retained without an outbound
/// consumer before `enforcement_tick` gives up on it. There is no real
/// fleet-upload path yet (Phase 3's `fleet_conn_loop` is still a stub) —
/// this only stops an indefinitely-absent consumer from pinning stale
/// telemetry in memory forever; it is independent of the count-based cap
/// above.
const CLIPBOARD_EVENT_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// How recently a `report_event` call must have been accepted for
/// `helper_running` to read `true`. See [`enforcement_tick`]'s doc for why
/// this is the best available proxy today.
const HELPER_LIVENESS_WINDOW: Duration = Duration::from_secs(5 * 60);

/// One clipboard-guard event accepted from a pinned `SessionHelper` peer,
/// held in memory until an outbound consumer picks it up (Phase 3's
/// `fleet_conn_loop`, not yet built) or it ages out.
#[derive(Debug, Clone)]
pub struct QueuedClipboardEvent {
    pub report: ClipboardEventReport,
    // Populated per D-2's "trust-origin marker on stored receipts"
    // requirement; not yet read back by any production code because the
    // outbound consumer that would need it (Phase 3's `fleet_conn_loop`)
    // is still a stub — see this file's module doc. Read today only by
    // tests confirming it's stamped correctly.
    #[allow(dead_code)]
    pub trust_origin: TrustOrigin,
    queued_at: Instant,
}

/// Bounded, in-memory FIFO of accepted clipboard events awaiting an
/// outbound consumer.
pub struct ClipboardEventQueue {
    inner: Mutex<VecDeque<QueuedClipboardEvent>>,
    capacity: usize,
}

impl ClipboardEventQueue {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
            capacity,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<QueuedClipboardEvent>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Queue one event, evicting the OLDEST entry first if already at
    /// capacity. Never panics on a poisoned mutex (recovers the guard
    /// instead, matching `peer_auth.rs`'s own rate-limiter precedent).
    pub fn push(&self, event: QueuedClipboardEvent) {
        let mut guard = self.lock();
        if guard.len() >= self.capacity {
            guard.pop_front();
        }
        guard.push_back(event);
    }

    /// Current queue depth — backs the `queued_events` health field.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Non-destructive copy of everything currently queued, FIFO order.
    /// Deliberately does NOT remove anything — a real outbound consumer
    /// added later (Phase 3) must still find every event still present;
    /// this is the "retain queued clipboard events for pickup" half of the
    /// enforcement loop's job.
    pub fn snapshot(&self) -> Vec<QueuedClipboardEvent> {
        self.lock().iter().cloned().collect()
    }

    /// Drop entries older than `max_age` as of `now`, returning how many
    /// were pruned. This is the "drain" half of "drain/retain": it bounds
    /// how long telemetry can sit waiting for an outbound path that
    /// doesn't exist yet, independent of the count-based cap in `push`.
    pub fn prune_older_than(&self, max_age: Duration, now: Instant) -> usize {
        let mut guard = self.lock();
        let before = guard.len();
        guard.retain(|e| now.duration_since(e.queued_at) < max_age);
        before - guard.len()
    }
}

/// Health snapshot for Clipboard Guard, refreshed once per
/// `enforcement_tick`. See that function's doc for exactly how each flag
/// is derived from what `commander-svc` can actually observe today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClipboardGuardHealth {
    pub policy_current: bool,
    pub rules_compiled: bool,
    pub helper_running: bool,
    pub listener_registered: bool,
    pub clear_failing: bool,
    pub queued_events: usize,
}

/// Shared, process-lifetime state for the Clipboard Guard `SessionHelper`
/// verbs, constructed once in `main.rs` and shared across every pipe
/// connection task and `enforcement_tick`.
pub struct ClipboardGuardState {
    pub events: ClipboardEventQueue,
    enabled: AtomicBool,
    /// Timestamp of the most recently accepted `report_event` call —
    /// `helper_running`'s proxy signal. There is no independent
    /// process-liveness probe for the per-user Clipboard Guard helper: its
    /// binary is a later-phase deliverable (see `peer_auth.rs`'s "known
    /// placeholder" note) with no heartbeat verb defined yet, so "have we
    /// heard from it recently" is the best signal available today.
    last_event_accepted: Mutex<Option<Instant>>,
    health: Mutex<ClipboardGuardHealth>,
}

impl ClipboardGuardState {
    pub fn new() -> Self {
        Self {
            events: ClipboardEventQueue::new(CLIPBOARD_EVENT_QUEUE_CAPACITY),
            enabled: AtomicBool::new(true),
            last_event_accepted: Mutex::new(None),
            health: Mutex::new(ClipboardGuardHealth::default()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn set_enabled(&self, value: bool) {
        self.enabled.store(value, Ordering::SeqCst);
    }

    fn mark_event_accepted(&self, now: Instant) {
        *self
            .last_event_accepted
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(now);
    }

    // Not yet called by any production code — a future Phase-3
    // health-reporter is the intended real caller (mirrors
    // `PolicyStore::health()`'s identical situation, per C2's handoff).
    // Exercised today by `enforcement_tick`'s own tests, which assert on
    // the flags this refreshes.
    #[allow(dead_code)]
    pub fn health(&self) -> ClipboardGuardHealth {
        *self.health.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set_health(&self, health: ClipboardGuardHealth) {
        *self.health.lock().unwrap_or_else(|p| p.into_inner()) = health;
    }
}

impl Default for ClipboardGuardState {
    fn default() -> Self {
        Self::new()
    }
}

/// One iteration of Clipboard Guard's real enforcement work: refresh the
/// policy-health flags from [`PolicyStore`], prune clipboard events that
/// have aged out with no outbound consumer, and derive the
/// clipboard-specific health flags from what's left. Extracted from
/// `main.rs::enforcement_loop`'s loop body so it's independently testable
/// without a real 30s sleep.
///
/// Never panics: every step here already returns a default/degraded value
/// rather than an error (poison-recovering locks, `PolicyStore`'s own
/// no-panic accessors) — there is deliberately nothing for a `?` to
/// propagate. `clear_failing` is derived from real data already flowing
/// through `svc.clipboard.report_event`: a queued
/// [`ClipboardEventReport`] whose `actions_attempted` names
/// `clear_clipboard`/`quarantine_clipboard` but whose `actions_succeeded`
/// does not is direct evidence that the endpoint's clear/quarantine action
/// is failing — commander-svc has no other way to observe that outcome,
/// since the clipboard content matching and the action itself both run in
/// the per-user helper, not this service.
pub(crate) fn enforcement_tick(
    policy_store: &PolicyStore,
    state: &ClipboardGuardState,
    now: Instant,
) {
    let policy_health = policy_store.clipboard_health();

    state.events.prune_older_than(CLIPBOARD_EVENT_MAX_AGE, now);
    let queued_events = state.events.len();
    let snapshot = state.events.snapshot();

    let helper_running = state
        .last_event_accepted
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .map(|at| now.duration_since(at) < HELPER_LIVENESS_WINDOW)
        .unwrap_or(false);

    state.set_health(ClipboardGuardHealth {
        policy_current: policy_health.policy_current,
        rules_compiled: policy_health.rules_compiled,
        helper_running,
        // No independent liveness signal for the listener specifically —
        // mirrors `helper_running` until a real per-helper heartbeat verb
        // exists (see the module doc's "known placeholder" reference).
        listener_registered: helper_running,
        clear_failing: clear_action_is_failing(&snapshot),
        queued_events,
    });
}

fn clear_action_is_failing(events: &[QueuedClipboardEvent]) -> bool {
    events.iter().any(|q| {
        [Action::ClearClipboard, Action::QuarantineClipboard]
            .into_iter()
            .any(|action| {
                q.report.actions_attempted.contains(&action)
                    && !q.report.actions_succeeded.contains(&action)
            })
    })
}

// ── Peer-SID classifier ──────────────────────────────────────────────────────

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

fn token_is_privileged(token: HANDLE) -> Result<bool> {
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

fn caller_has_vault_policy_capability_token(token: HANDLE) -> Result<bool> {
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

// ── Shared test fixtures (used by both `tests` and `integration` below) ────

#[cfg(test)]
mod test_support {
    use super::*;
    use crate::peer_auth::{PeerAuthProbe, PeerIdentitySnapshot};
    use crate::policy_store::{PolicyFs, PolicyStoreError, SystemClock};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    pub const TEST_ROOT: &str = r"C:\Program Files\WinCommander";

    /// Ed25519 fixtures generated once via a throwaway helper Cargo project
    /// (mirrors `policy_store.rs`'s own test-fixture provenance note) —
    /// distinct keypair from that module's fixtures, scoped to this file's
    /// own `handle_install_epoch` tests.
    pub const PIPE_PINNED_PUBKEY_B64: &str = "6kpsY+KcUgq+9VB7Ey7F+ZVHdq6+vnuSQh7qaRRG0iw=";
    /// Signature over version 1 of [`pipe_test_config`].
    pub const PIPE_SIG_V1_B64: &str =
        "/l+M1Grt0jhwOKLnWffMECByVMcIua5IOpeCreqHFxaItPpq5QinbSz8F8Cb8Q3hsEpVmk2SLh67gWMy0Q6DAQ==";
    /// Signature over version 2 of the SAME config (strictly greater).
    pub const PIPE_SIG_V2_B64: &str =
        "Ge2rkdBPC1mCXZUcyZ9W2ybNc5yUgHaTdSjYkh6XiOB89wUm/FD3cAp0xNJh6giR+8Eqr1pCZcVXw0mdJMEvBQ==";

    pub fn pipe_test_config() -> serde_json::Value {
        serde_json::json!({
            "clipboardGuard": {
                "rules": [{
                    "actions": ["notify_user"],
                    "cooldownSeconds": 30,
                    "enabled": true,
                    "id": "2e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4d",
                    "locked": false,
                    "matcher": {"kind": "phrase", "params": {"case_sensitive": false, "value": "wire-test"}},
                    "name": "pipe-test-rule",
                    "priority": 100,
                    "revision": 1,
                    "severity": "warn",
                    "snoozable": true
                }]
            }
        })
    }

    /// Build a full `svc.policy.install_epoch` wire payload (matches
    /// [`super::InstallEpochWireArgs`]) at `version`, signed with
    /// `signature` (pass a MISMATCHED version's signature to produce a
    /// forged-payload fixture, mirroring `policy_store.rs`'s own trick).
    pub fn pipe_wire_args(version: i64, signature: &str) -> serde_json::Value {
        serde_json::json!({
            "policy_version": version,
            "config": pipe_test_config(),
            "locked_paths": [],
            "managed": true,
            "target_kind": "org",
            "signature": signature,
            "signer_key": PIPE_PINNED_PUBKEY_B64,
        })
    }

    pub fn sample_clipboard_event_report() -> serde_json::Value {
        serde_json::json!({
            "event_id": "01890a5d-ac1d-7c3e-8b1a-0123456789ab",
            "occurred_at": "2026-08-18T00:00:00Z",
            "policy_version": 1,
            "rule_id": "2e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4d",
            "rule_revision": 1,
            "severity": "warn",
            "actions_attempted": ["clear_clipboard"],
            "actions_succeeded": [],
            "suppressed_count": 0,
        })
    }

    /// In-memory [`PolicyFs`], reimplemented here (not reusing
    /// `policy_store`'s own private `#[cfg(test)]` fake) so these tests
    /// stay independent of the REAL Windows ACL/file-write path.
    /// `WindowsPolicyFs` requires the calling process's token to actually
    /// be granted access under the very DACL it just wrote
    /// (SYSTEM/Administrators-only) — a non-elevated dev/CI shell's split
    /// token does not satisfy that even when the signed-in account is an
    /// administrator (`BUILTIN\Administrators` is present in the token but
    /// disabled/"deny only" outside an elevated process). That's a real
    /// environment property, not a defect in `WindowsPolicyFs` — see
    /// `policy_store.rs`'s own `windows_lock_down_smoke`, which only
    /// asserts the ACL-set call itself succeeds and deliberately never
    /// attempts a subsequent write.
    struct InMemoryPolicyFs {
        files: Mutex<HashMap<PathBuf, Vec<u8>>>,
    }

    impl InMemoryPolicyFs {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }
    }

    impl PolicyFs for InMemoryPolicyFs {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
        }

        fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn ensure_dir_secure(&self, _dir: &Path) -> Result<(), PolicyStoreError> {
            Ok(())
        }
    }

    /// Fresh, in-memory-backed `PolicyStore`, pinned to
    /// [`PIPE_PINNED_PUBKEY_B64`]. Each call gets its own store (no shared
    /// state across tests).
    pub fn test_policy_store() -> Arc<PolicyStore> {
        Arc::new(
            PolicyStore::open(
                Box::new(InMemoryPolicyFs::new()),
                Box::new(SystemClock),
                PathBuf::from("/fake/pipe-test-policy"),
                PIPE_PINNED_PUBKEY_B64.to_string(),
            )
            .expect("in-memory policy store always opens"),
        )
    }

    pub struct FakePeerProbe {
        pub identity: Result<PeerIdentitySnapshot, PeerAuthError>,
        pub active_session: Result<u32, PeerAuthError>,
    }

    impl PeerAuthProbe for FakePeerProbe {
        fn identity(&self, _pid: u32) -> Result<PeerIdentitySnapshot, PeerAuthError> {
            self.identity.clone()
        }
        fn active_interactive_session(&self) -> Result<u32, PeerAuthError> {
            self.active_session
        }
    }

    pub fn ok_identity() -> PeerIdentitySnapshot {
        PeerIdentitySnapshot {
            session_id: 7,
            canonical_image_path: PathBuf::from(TEST_ROOT).join("wincommander-free.exe"),
        }
    }

    /// A gate whose fake probe passes every check — used for "peer is
    /// pinned" test scenarios.
    pub fn passing_gate() -> Arc<SessionHelperGate> {
        Arc::new(SessionHelperGate::with_allowed_root(
            Arc::new(FakePeerProbe {
                identity: Ok(ok_identity()),
                active_session: Ok(7),
            }),
            Some(PathBuf::from(TEST_ROOT)),
        ))
    }

    /// A gate whose fake probe fails in exactly the way named by `err`, so
    /// each individual `PeerAuthError` reason can be exercised at the
    /// `authorize()` call boundary independently.
    pub fn gate_denying_with(err: PeerAuthError) -> Arc<SessionHelperGate> {
        let root = Some(PathBuf::from(TEST_ROOT));
        match err {
            PeerAuthError::IdentityUnavailable => Arc::new(SessionHelperGate::with_allowed_root(
                Arc::new(FakePeerProbe {
                    identity: Err(PeerAuthError::IdentityUnavailable),
                    active_session: Ok(7),
                }),
                root,
            )),
            PeerAuthError::NoInteractiveSession => Arc::new(SessionHelperGate::with_allowed_root(
                Arc::new(FakePeerProbe {
                    identity: Ok(ok_identity()),
                    active_session: Err(PeerAuthError::NoInteractiveSession),
                }),
                root,
            )),
            PeerAuthError::WrongSession => Arc::new(SessionHelperGate::with_allowed_root(
                Arc::new(FakePeerProbe {
                    identity: Ok(ok_identity()),
                    active_session: Ok(99),
                }),
                root,
            )),
            PeerAuthError::PathNotAllowed => {
                let mut identity = ok_identity();
                identity.canonical_image_path = PathBuf::from(r"C:\Users\attacker\evil.exe");
                Arc::new(SessionHelperGate::with_allowed_root(
                    Arc::new(FakePeerProbe {
                        identity: Ok(identity),
                        active_session: Ok(7),
                    }),
                    root,
                ))
            }
            // Not exercised via this helper — rate limiting is tested
            // separately by hammering a `passing_gate()` past its quota.
            PeerAuthError::RateLimited => passing_gate(),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use wincmd_shared::svc::SVC_PROTOCOL_VERSION;

    fn valid_vault_policy_args() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "policy_id": "policy-1",
            "version": 1,
            "expected_previous_version": 0,
            "entries": [{
                "id": "vault-1",
                "label": "Finance",
                "container_path": r"C:\Vaults\finance.hc",
                "owner_account": "Owner",
                "grants": [{ "principal_name": "Owner", "access": "write" }],
                "mount": { "presentation": "per-user", "preferred_letter": "V" }
            }]
        })
    }

    #[test]
    fn policy_save_rejects_an_occupied_letter_but_retains_unedited_reservations() {
        let mut policy = prepare_vault_apply_policy(valid_vault_policy_args()).unwrap();
        let occupied = HashSet::from(["V".to_owned()]);
        assert_eq!(
            validate_policy_drive_letters(&policy, None, &occupied)
                .unwrap_err()
                .kind,
            "vault_engine_drive_letter_unavailable"
        );
        let previous = policy.clone();
        assert!(validate_policy_drive_letters(&policy, Some(&previous), &occupied).is_ok());
        policy.entries[0].label = "Changed label".into();
        assert!(validate_policy_drive_letters(&policy, Some(&previous), &occupied).is_err());
        policy.entries[0].mount.preferred_letter = Some("W".into());
        assert!(validate_policy_drive_letters(&policy, Some(&previous), &occupied).is_ok());
    }

    #[test]
    fn new_free_letter_policy_saves_while_an_unchanged_other_vault_is_mounted() {
        let store = crate::vault_access::test_policy_store();
        let previous: wincmd_shared::vault_access::VaultAccessPolicy =
            serde_json::from_value(valid_vault_policy_args()).unwrap();
        store.apply(previous.clone(), 1).unwrap();
        let broker = crate::vault_mount::policy_edit_test_broker(
            "vault-1",
            r"volume:1:c:\vaults\finance.hc",
        );
        let mut requested = previous.clone();
        requested.version = 2;
        requested.expected_previous_version = 1;
        let mut added = previous.entries[0].clone();
        added.id = "new-vault".into();
        added.container_path = r"C:\Vaults\new-container.hc".into();
        added.mount.preferred_letter = Some("W".into());
        requested.entries.push(added);
        broker.with_exclusive_operation(|| {
            let occupied = broker.occupied_letters_locked().unwrap();
            assert!(occupied.contains("V"));
            assert!(!occupied.contains("W"));
            validate_vault_changed_targets_unmounted(&store, &broker, &requested).unwrap();
            validate_policy_drive_letters(&requested, Some(&previous), &occupied).unwrap();
            store.preflight_apply(requested.clone()).unwrap();
            let mut colliding = requested.clone();
            colliding.entries[1].mount.preferred_letter = Some("V".into());
            assert_eq!(
                validate_policy_drive_letters(&colliding, Some(&previous), &occupied)
                    .unwrap_err()
                    .kind,
                "vault_engine_drive_letter_unavailable"
            );
            store.apply(requested.clone(), 2).unwrap();
            assert!(
                broker.has_active_mounts_locked(),
                "saving must not dismount the other Vault"
            );
        });
        assert_eq!(store.policy().unwrap().entries, requested.entries);
        assert_eq!(store.policy().unwrap().entries[0], previous.entries[0]);
    }

    fn removal_test_policy() -> wincmd_shared::vault_access::VaultAccessPolicy {
        let mut value = valid_vault_policy_args();
        value["entries"][0]["primary_owner_sid"] = serde_json::json!("S-1-5-21-owner");
        let mut other = value["entries"][0].clone();
        other["id"] = serde_json::json!("vault-other");
        other["primary_owner_sid"] = serde_json::json!("S-1-5-21-other");
        value["entries"].as_array_mut().unwrap().push(other);
        serde_json::from_value(value).unwrap()
    }

    fn removal_test_fragment(
        ids: &[&str],
    ) -> wincmd_shared::vault_access::VaultOwnerPolicyFragment {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "policy_id": "policy-1",
            "version": 2,
            "expected_previous_version": 1,
            "entries": [],
            "remove_entry_ids": ids,
        }))
        .unwrap()
    }

    #[test]
    fn owner_fragment_removes_only_explicit_owned_entry_and_preserves_hidden_owner() {
        let previous = removal_test_policy();
        let merged = merge_owner_fragment_policy(
            Some(previous.clone()),
            removal_test_fragment(&["vault-1"]),
            "S-1-5-21-owner",
            None,
        )
        .unwrap();
        assert_eq!(merged.entries, vec![previous.entries[1].clone()]);
        assert_eq!((merged.version, merged.expected_previous_version), (2, 1));
        validate_vault_owner_policy_mutation(Some(&previous), &merged, "S-1-5-21-owner", false)
            .unwrap();
    }

    #[test]
    fn owner_fragment_omission_is_not_a_delete_and_old_wire_defaults_to_no_removals() {
        let previous = removal_test_policy();
        let mut value = serde_json::to_value(removal_test_fragment(&[])).unwrap();
        value.as_object_mut().unwrap().remove("remove_entry_ids");
        let fragment = serde_json::from_value(value).unwrap();
        let merged =
            merge_owner_fragment_policy(Some(previous.clone()), fragment, "S-1-5-21-owner", None)
                .unwrap();
        assert_eq!(merged.entries, previous.entries);
    }

    #[test]
    fn owner_fragment_cannot_delete_another_users_entry_without_admin_authority() {
        let previous = removal_test_policy();
        let error = merge_owner_fragment_policy(
            Some(previous.clone()),
            removal_test_fragment(&["vault-other"]),
            "S-1-5-21-owner",
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind, "vault_owner_required");
        let mut forged = previous.clone();
        forged.entries.pop();
        assert_eq!(
            validate_vault_owner_policy_mutation(Some(&previous), &forged, "S-1-5-21-owner", false)
                .unwrap_err()
                .kind,
            "vault_owner_required"
        );
    }

    #[test]
    fn administrator_explicit_removal_can_clear_last_entry_through_normal_policy_mutation() {
        let previous = removal_test_policy();
        let administrators = HashSet::new();
        let merged = merge_owner_fragment_policy(
            Some(previous.clone()),
            removal_test_fragment(&["vault-1", "vault-other"]),
            "S-1-5-21-admin",
            Some(&administrators),
        )
        .unwrap();
        assert!(merged.entries.is_empty());
        assert_eq!(merged.policy_id, previous.policy_id);
        validate_vault_owner_policy_mutation(Some(&previous), &merged, "S-1-5-21-admin", true)
            .unwrap();
    }

    #[test]
    fn outsider_administrator_can_only_remove_group_policy_even_as_owner() {
        let previous = removal_test_policy();
        let scope = HashMap::from([("vault-1".to_string(), false)]);
        for caller in ["S-1-5-21-admin", "S-1-5-21-owner"] {
            let removed = merge_owner_fragment_policy_with_scope(
                Some(previous.clone()),
                removal_test_fragment(&["vault-1"]),
                caller,
                true,
                &scope,
            )
            .unwrap();
            assert!(removed.entries.iter().all(|entry| entry.id != "vault-1"));
            validate_vault_owner_policy_mutation_with_scope(
                Some(&previous),
                &removed,
                caller,
                true,
                &scope,
            )
            .unwrap();
            for transfer in [false, true] {
                let mut entry = previous.entries[0].clone();
                if transfer {
                    entry.primary_owner_sid = Some(caller.into());
                } else {
                    entry.label = "unauthorized change".into();
                }
                let mut fragment = removal_test_fragment(&[]);
                fragment
                    .entries
                    .push(wincmd_shared::vault_access::VaultOwnedPolicyEntry {
                        can_edit_policy: false,
                        can_remove_policy: false,
                        entry,
                        canonical_container_path: None,
                        container_path_state:
                            wincmd_shared::vault_access::VaultContainerPathState::Available,
                });
                if transfer && caller == "S-1-5-21-owner" {
                    continue;
                }
                assert_eq!(
                    merge_owner_fragment_policy_with_scope(
                        Some(previous.clone()),
                        fragment,
                        caller,
                        true,
                        &scope
                    )
                    .unwrap_err()
                    .kind,
                    "vault_fleet_group_required"
                );
            }
        }
    }

    #[test]
    fn service_handlers_block_policy_edits_and_recovery_removal_while_mounted() {
        let store = crate::vault_access::test_policy_store();
        let policy: wincmd_shared::vault_access::VaultAccessPolicy =
            serde_json::from_value(valid_vault_policy_args()).unwrap();
        store.apply(policy, 1).unwrap();
        let broker =
            crate::vault_mount::policy_edit_test_broker("vault-1", "test-mounted-identity");
        let mut edited = store.policy().unwrap();
        edited.entries[0].label = "Changed".into();
        let error = broker
            .with_exclusive_operation(|| {
                validate_vault_changed_targets_unmounted(&store, &broker, &edited)
            })
            .unwrap_err();
        assert_eq!(error.kind, "vault_mounted");
        for (sid, elevated) in [
            ("S-1-5-21-owner", false),
            ("S-1-5-21-owner", true),
            ("S-1-5-21-outsider", true),
        ] {
            let peer = AuthenticatedPipePeer {
                client_pid: 1,
                token: std::ptr::null_mut(),
                session_id: 7,
                caller_sid: sid.into(),
                authentication_id: (1, 0),
            };
            if elevated {
                let request = serde_json::json!({ "entry_id": "vault-1", "policy_id": "policy-1", "expected_version": 1 });
                let error = handle_vault_forget_entry_policy_only(
                    &store,
                    &broker,
                    request,
                    Some(&peer),
                    true,
                )
                .unwrap_err();
                assert_eq!(error.kind, "vault_mounted");
            }
            assert!(broker.with_exclusive_operation(|| broker.has_active_mounts_locked()));
            assert_eq!(store.policy().unwrap().version, 1);
        }
    }

    #[test]
    fn member_administrator_can_edit_group_policy_without_deleting_hidden_entries() {
        let previous = removal_test_policy();
        let scope = HashMap::from([
            ("vault-1".to_string(), true),
            ("vault-other".to_string(), false),
        ]);
        let mut entry = previous.entries[0].clone();
        entry.label = "authorized change".into();
        let mut fragment = removal_test_fragment(&[]);
        fragment
            .entries
            .push(wincmd_shared::vault_access::VaultOwnedPolicyEntry {
                can_edit_policy: false,
                can_remove_policy: false,
                entry,
                canonical_container_path: None,
                container_path_state:
                    wincmd_shared::vault_access::VaultContainerPathState::Available,
        });
        let merged = merge_owner_fragment_policy_with_scope(
            Some(previous.clone()),
            fragment,
            "S-1-5-21-admin",
            true,
            &scope,
        )
        .unwrap();
        assert_eq!(merged.entries[0].label, "authorized change");
        assert_eq!(merged.entries[1], previous.entries[1]);
        validate_vault_owner_policy_mutation_with_scope(
            Some(&previous),
            &merged,
            "S-1-5-21-admin",
            true,
            &scope,
        )
        .unwrap();
        let mut forged_removal = merged;
        forged_removal.entries.pop();
        validate_vault_owner_policy_mutation_with_scope(
            Some(&previous),
            &forged_removal,
            "S-1-5-21-admin",
            true,
            &scope,
        )
        .unwrap();
        assert_eq!(
            validate_vault_owner_policy_mutation_with_scope(
                Some(&previous),
                &forged_removal,
                "S-1-5-21-owner",
                false,
                &scope
            )
            .unwrap_err()
            .kind,
            "vault_fleet_group_required"
        );
    }

    #[test]
    fn full_policy_validation_rejects_initial_and_added_outsider_group_entries() {
        let requested = removal_test_policy();
        let scope = HashMap::from([("vault-other".to_string(), false)]);
        assert_eq!(
            validate_vault_owner_policy_mutation_with_scope(
                None,
                &requested,
                "S-1-5-21-admin",
                true,
                &scope
            )
            .unwrap_err()
            .kind,
            "vault_fleet_group_required"
        );
        let mut previous = requested.clone();
        previous.entries.pop();
        assert_eq!(
            validate_vault_owner_policy_mutation_with_scope(
                Some(&previous),
                &requested,
                "S-1-5-21-admin",
                true,
                &scope
            )
            .unwrap_err()
            .kind,
            "vault_fleet_group_required"
        );
        assert!(
            validate_vault_owner_policy_mutation_with_scope(
                Some(&requested),
                &requested,
                "S-1-5-21-admin",
                true,
                &scope
            )
            .is_ok(),
            "unchanged foreign policy entries are preserved"
        );
    }

    #[test]
    fn owner_fragment_rejects_ambiguous_unknown_and_stale_removals() {
        let previous = removal_test_policy();
        for ids in [vec!["vault-1", "vault-1"], vec!["unknown"], vec![""]] {
            assert_eq!(
                merge_owner_fragment_policy(
                    Some(previous.clone()),
                    removal_test_fragment(&ids),
                    "S-1-5-21-owner",
                    None,
                )
                .unwrap_err()
                .kind,
                "vault_validation_failed"
            );
        }
        let mut fragment = removal_test_fragment(&["vault-1"]);
        fragment
            .entries
            .push(wincmd_shared::vault_access::VaultOwnedPolicyEntry {
                can_edit_policy: false,
                can_remove_policy: false,
                entry: previous.entries[0].clone(),
                container_path_state:
                    wincmd_shared::vault_access::VaultContainerPathState::Available,
                canonical_container_path: None,
            });
        assert_eq!(
            merge_owner_fragment_policy(Some(previous.clone()), fragment, "S-1-5-21-owner", None)
                .unwrap_err()
                .kind,
            "vault_validation_failed"
        );
        let mut stale = removal_test_fragment(&["vault-1"]);
        stale.expected_previous_version = 0;
        assert_eq!(
            merge_owner_fragment_policy(Some(previous), stale, "S-1-5-21-owner", None)
                .unwrap_err()
                .kind,
            "vault_apply_failed"
        );
        assert_eq!(
            merge_owner_fragment_policy(
                None,
                removal_test_fragment(&["vault-1"]),
                "S-1-5-21-owner",
                None
            )
            .unwrap_err()
            .kind,
            "vault_validation_failed"
        );
    }

    #[test]
    fn private_vault_accepts_enabled_standard_owners_and_rejects_unavailable_accounts() {
        let mut value = valid_vault_policy_args();
        value["entries"][0]["primary_owner_sid"] = serde_json::json!("S-1-5-21-standard");
        let policy = serde_json::from_value(value).expect("valid policy shape");
        let users = HashSet::from(["S-1-5-21-admin", "S-1-5-21-standard"]);
        validate_private_owner_sids(&policy, &users).unwrap();
        let users_without_disabled = HashSet::from(["S-1-5-21-admin"]);
        let error = validate_private_owner_sids(&policy, &users_without_disabled)
            .expect_err("disabled, deleted, or unknown owners must not be selected");

        assert_eq!(error.kind, "vault_not_authorized");
    }

    #[test]
    fn outsider_administrator_cannot_transfer_another_owners_policy() {
        let mut policy = valid_vault_policy_args();
        policy["entries"][0]["primary_owner_sid"] = serde_json::json!("S-1-5-21-owner");
        let existing: wincmd_shared::vault_access::VaultAccessEntry =
            serde_json::from_value(policy["entries"][0].clone()).expect("test entry");
        let mut transfer = existing.clone();
        transfer.primary_owner_sid = Some("S-1-5-21-admin-target".into());
        transfer.owner_account = "Admin target".into();
        let previous: wincmd_shared::vault_access::VaultAccessPolicy =
            serde_json::from_value(policy).unwrap();
        let mut requested = previous.clone();
        requested.entries[0] = transfer;
        assert_eq!(
            validate_vault_owner_policy_mutation(
                Some(&previous),
                &requested,
                "S-1-5-21-admin",
                true
            )
            .unwrap_err()
            .kind,
            "vault_owner_required"
        );
    }

    #[test]
    fn administrator_can_assign_new_private_entries_without_claiming_them_first() {
        use wincmd_shared::vault_access::{VaultContainerPathState, VaultOwnedPolicyEntry};
        let previous = removal_test_policy();
        for initial in [false, true] {
            for selected_owner in ["S-1-5-21-standard", "S-1-5-21-admin-target"] {
                let mut entry = previous.entries[0].clone();
                entry.id = "new-private".into();
                entry.container_path = r"C:\Vaults\new-private.hc".into();
                entry.primary_owner_sid = Some(selected_owner.into());
                let mut fragment = removal_test_fragment(&[]);
                fragment.entries.push(VaultOwnedPolicyEntry {
                    entry: entry.clone(),
                    canonical_container_path: None,
                    container_path_state: VaultContainerPathState::Available,
                    can_edit_policy: false,
                    can_remove_policy: false,
                });
                let base = (!initial).then_some(previous.clone());
                let merged = merge_owner_fragment_policy_with_scope(
                    base.clone(),
                    fragment,
                    "S-1-5-21-admin",
                    true,
                    &HashMap::new(),
                )
                .expect("admin may assign a new private vault");
                validate_vault_owner_policy_mutation(
                    base.as_ref(),
                    &merged,
                    "S-1-5-21-admin",
                    true,
                )
                .unwrap();
                assert!(merged.entries.contains(&entry));
                assert_eq!(
                    vault_policy_capabilities(false, true, Ok(None)),
                    (false, true)
                );
            }
        }
    }

    #[test]
    fn standard_user_can_create_private_entries_only_for_self() {
        use wincmd_shared::vault_access::{VaultContainerPathState, VaultOwnedPolicyEntry};
        let previous = removal_test_policy();
        for initial in [false, true] {
            for selected_owner in ["S-1-5-21-standard", "S-1-5-21-other"] {
                let mut requested = previous.clone();
                requested.entries.clear();
                let mut entry = previous.entries[0].clone();
                entry.id = "new-private".into();
                entry.primary_owner_sid = Some(selected_owner.into());
                requested.entries.push(entry.clone());
                let base = (!initial).then_some(&previous);
                if !initial {
                    requested.entries.extend(previous.entries.clone());
                }
                let result = validate_vault_owner_policy_mutation(
                    base,
                    &requested,
                    "S-1-5-21-standard",
                    false,
                );
                assert_eq!(result.is_ok(), selected_owner == "S-1-5-21-standard");
                let mut fragment = removal_test_fragment(&[]);
                fragment.entries.push(VaultOwnedPolicyEntry {
                    entry,
                    canonical_container_path: None,
                    container_path_state: VaultContainerPathState::Available,
                    can_edit_policy: true,
                    can_remove_policy: true,
                });
                assert_eq!(
                    merge_owner_fragment_policy_with_scope(
                        base.cloned(),
                        fragment,
                        "S-1-5-21-standard",
                        false,
                        &HashMap::new()
                    )
                    .is_ok(),
                    selected_owner == "S-1-5-21-standard"
                );
            }
        }
    }

    #[test]
    fn standard_private_owner_can_edit_but_cannot_transfer_ownership() {
        use wincmd_shared::vault_access::{VaultContainerPathState, VaultOwnedPolicyEntry};
        let previous = removal_test_policy();
        let mut requested = previous.clone();
        requested.entries[0].label = "Owner label update".into();
        validate_vault_owner_policy_mutation(Some(&previous), &requested, "S-1-5-21-owner", false)
            .unwrap();
        requested.entries[0].primary_owner_sid = Some("S-1-5-21-other".into());
        assert_eq!(
            validate_vault_owner_policy_mutation(
                Some(&previous),
                &requested,
                "S-1-5-21-owner",
                false
            )
            .unwrap_err()
            .kind,
            "vault_owner_transfer_requires_admin"
        );
        let mut fragment = removal_test_fragment(&[]);
        fragment.entries.push(VaultOwnedPolicyEntry {
            entry: requested.entries[0].clone(),
            canonical_container_path: None,
            container_path_state: VaultContainerPathState::Available,
            can_edit_policy: true,
            can_remove_policy: true,
        });
        assert_eq!(
            merge_owner_fragment_policy_with_scope(
                Some(previous.clone()),
                fragment,
                "S-1-5-21-owner",
                false,
                &HashMap::new()
            )
            .unwrap_err()
            .kind,
            "vault_owner_transfer_requires_admin"
        );
        validate_vault_owner_policy_mutation(Some(&previous), &requested, "S-1-5-21-owner", true)
            .unwrap();
    }

    #[test]
    fn standard_owner_cannot_transfer_after_switching_to_shared_presentation() {
        let mut previous = removal_test_policy();
        let mut requested = previous.clone();
        requested.entries[0].mount.presentation =
            wincmd_shared::vault_access::VaultPresentation::Machine;
        requested.entries[0].primary_owner_sid = Some("S-1-5-21-other".into());
        assert_eq!(
            validate_vault_owner_policy_mutation(
                Some(&previous),
                &requested,
                "S-1-5-21-owner",
                false
            )
            .unwrap_err()
            .kind,
            "vault_owner_transfer_requires_admin"
        );
        previous.entries[0].mount.presentation =
            wincmd_shared::vault_access::VaultPresentation::Machine;
        assert_eq!(
            validate_vault_owner_policy_mutation(
                Some(&previous),
                &requested,
                "S-1-5-21-owner",
                false
            )
            .unwrap_err()
            .kind,
            "vault_owner_transfer_requires_admin"
        );
        requested.entries[0].mount.presentation =
            wincmd_shared::vault_access::VaultPresentation::PerUser;
        assert_eq!(
            validate_vault_owner_policy_mutation(
                Some(&previous),
                &requested,
                "S-1-5-21-owner",
                false
            )
            .unwrap_err()
            .kind,
            "vault_owner_transfer_requires_admin"
        );
        requested.entries[0].primary_owner_sid = previous.entries[0].primary_owner_sid.clone();
        requested.entries[0].label = "Same owner update".into();
        validate_vault_owner_policy_mutation(Some(&previous), &requested, "S-1-5-21-owner", false)
            .unwrap();
    }

    #[test]
    fn outsider_administrator_cannot_hide_ownership_transfer_in_remove_and_recreate() {
        let store = crate::vault_access::test_policy_store();
        let policy: wincmd_shared::vault_access::VaultAccessPolicy =
            serde_json::from_value(valid_vault_policy_args()).unwrap();
        store.apply(policy.clone(), 1).unwrap();
        let mut recreated = policy.entries[0].clone();
        recreated.id = "new-id-for-same-container".into();
        recreated.primary_owner_sid = Some("S-1-5-21-admin".into());
        let mut fragment = removal_test_fragment(&["vault-1"]);
        fragment
            .entries
            .push(wincmd_shared::vault_access::VaultOwnedPolicyEntry {
                entry: recreated,
                canonical_container_path: None,
                container_path_state:
                    wincmd_shared::vault_access::VaultContainerPathState::Available,
                can_edit_policy: true,
                can_remove_policy: true,
        });
        let merged = merge_owner_fragment_policy(
            Some(policy),
            fragment,
            "S-1-5-21-admin",
            Some(&HashSet::new()),
        )
        .unwrap();
        assert_eq!(
            validate_vault_owner_mutation(&store, &merged, "S-1-5-21-admin", true)
                .unwrap_err()
                .kind,
            "vault_owner_required"
        );
        assert_eq!(store.policy().unwrap().entries[0].id, "vault-1");
    }

    #[test]
    fn outsider_admin_cannot_replace_private_authority_with_a_proposed_shared_group() {
        use wincmd_shared::vault_access::{
            VaultContainerPathState, VaultOwnedPolicyEntry, VaultPresentation,
        };
        let store = crate::vault_access::test_policy_store();
        let caller = "S-1-5-21-1-2-3-1002";
        let directory = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "users": [{ "sid": caller, "username": "Outsider", "display_name": null }],
            "groups": [{ "id": "outsider", "name": "Outsider", "local_group": "WC_Outsider", "member_sids": [caller] }]
        })).unwrap();
        store.save_access_directory(directory).unwrap();
        let policy = serde_json::from_value(valid_vault_policy_args()).unwrap();
        store.apply(policy, 1).unwrap();
        let previous = store.policy().unwrap();
        let mut requested = previous.clone();
        requested.entries[0].mount.presentation = VaultPresentation::Machine;
        requested.entries[0].primary_owner_sid = Some(caller.into());
        requested.entries[0].owner_account = "WC_Outsider".into();
        requested.entries[0].grants[0].principal_name = "WC_Outsider".into();
        assert_eq!(
            store
                .fleet_group_access(&previous.entries[0], caller)
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .fleet_group_access(&requested.entries[0], caller)
                .unwrap(),
            Some(true)
        );
        let mut fragment = removal_test_fragment(&[]);
        fragment.entries.push(VaultOwnedPolicyEntry {
            entry: requested.entries[0].clone(),
            canonical_container_path: None,
            container_path_state: VaultContainerPathState::Available,
            can_edit_policy: true,
            can_remove_policy: true,
        });
        assert_eq!(
            merge_owner_fragment(&store, fragment, caller, true)
                .unwrap_err()
                .kind,
            "vault_owner_required"
        );
        assert_eq!(
            validate_vault_owner_mutation(&store, &requested, caller, true)
                .unwrap_err()
                .kind,
            "vault_owner_required"
        );
        assert_eq!(store.policy().unwrap(), previous);
    }

    #[test]
    fn administrator_edit_of_another_owner_reports_ownership_not_stale_draft() {
        use wincmd_shared::vault_access::{VaultContainerPathState, VaultOwnedPolicyEntry};
        let previous = removal_test_policy();
        let mut fragment = removal_test_fragment(&[]);
        fragment.expected_previous_version = 0;
        let mut entry = previous.entries[0].clone();
        entry.label = "Edited by another administrator".into();
        fragment.entries.push(VaultOwnedPolicyEntry {
            can_edit_policy: false,
            can_remove_policy: false,
            entry,
            container_path_state: VaultContainerPathState::Available,
            canonical_container_path: None,
        });
        let administrators = HashSet::from(["S-1-5-21-admin".to_string()]);
        let error = merge_owner_fragment_policy(
            Some(previous),
            fragment,
            "S-1-5-21-admin",
            Some(&administrators),
        )
        .unwrap_err();
        assert_eq!(error.kind, "vault_owner_required");
    }

    #[test]
    fn known_principal_response_preserves_trusted_caller_label_and_admin_flag() {
        let response = build_known_principals_response(
            "S-1-5-21-caller",
            "WORKSTATION\\Parth".into(),
            vec![wincmd_shared::vault_access::VaultKnownPrincipal {
                sid: "S-1-5-21-directory-user".into(),
                display_name: "Fleet User".into(),
                is_local_administrator: false,
            }],
            vec![wincmd_shared::vault_access::VaultKnownPrincipal {
                sid: "S-1-5-21-caller".into(),
                display_name: "WORKSTATION\\Parth".into(),
                is_local_administrator: true,
            }],
        );

        assert_eq!(response.current_caller_sid, "S-1-5-21-caller");
        assert_eq!(response.principals.len(), 2);
        assert!(response.principals.iter().any(|principal| {
            principal.sid == "S-1-5-21-caller"
                && principal.display_name == "WORKSTATION\\Parth"
                && principal.is_local_administrator
        }));
    }

    #[test]
    fn policy_capabilities_distinguish_member_admin_from_outsider_remove_only() {
        assert_eq!(
            vault_policy_capabilities(false, true, Ok(Some(false))),
            (false, true)
        );
        assert_eq!(
            vault_policy_capabilities(true, true, Ok(Some(false))),
            (false, true)
        );
        assert_eq!(
            vault_policy_capabilities(false, true, Ok(Some(true))),
            (true, true)
        );
        assert_eq!(
            vault_policy_capabilities(true, false, Ok(None)),
            (true, true)
        );
        assert_eq!(
            vault_policy_capabilities(false, true, Ok(None)),
            (false, true)
        );
        assert_eq!(
            vault_policy_capabilities(false, false, Ok(Some(true))),
            (false, false)
        );
        assert_eq!(
            vault_policy_capabilities(
                true,
                false,
                Err(crate::vault_access::VaultError::Persistence)
            ),
            (false, false)
        );
    }

    #[test]
    fn machine_directory_discovery_keeps_groups_and_includes_users_without_groups() {
        let mut directory: wincmd_shared::vault_access::VaultAccessDirectory =
            serde_json::from_value(serde_json::json!({"schema_version":1,"users":[],"groups":[]}))
                .unwrap();
        let user = wincmd_shared::vault_access::VaultKnownPrincipal {
            sid: "S-1-5-21-1001".into(),
            display_name: "PC\\Standard".into(),
            is_local_administrator: false,
        };
        merge_discovered_directory_users(&mut directory, vec![user.clone()]);
        merge_discovered_directory_users(&mut directory, vec![user]);
        assert_eq!(directory.users.len(), 1);
        assert_eq!(directory.users[0].username, "PC\\Standard");
        assert!(directory.groups.is_empty());
    }

    #[test]
    fn vault_apply_preflight_rejects_malformed_request_before_cleanup() {
        let error = prepare_vault_apply_policy(serde_json::json!({ "not": "a policy" }))
            .expect_err("malformed policy must never enter mount cleanup");
        assert_eq!(error.kind, "vault_validation_failed");
    }

    #[test]
    fn vault_apply_preflight_rejects_duplicate_entry_ids_before_cleanup() {
        let mut args = valid_vault_policy_args();
        let duplicate = args["entries"][0].clone();
        args["entries"].as_array_mut().unwrap().push(duplicate);

        let error = prepare_vault_apply_policy(args)
            .expect_err("duplicate entry IDs must never enter mount cleanup");
        assert_eq!(error.kind, "vault_validation_failed");
    }

    #[test]
    fn vault_apply_preflight_rejects_duplicate_container_spellings_before_cleanup() {
        let mut args = valid_vault_policy_args();
        let mut duplicate = args["entries"][0].clone();
        duplicate["id"] = serde_json::json!("vault-2");
        duplicate["container_path"] = serde_json::json!(r"c:/vaults/FINANCE.hc\");
        args["entries"].as_array_mut().unwrap().push(duplicate);

        let error = prepare_vault_apply_policy(args)
            .expect_err("duplicate container paths must never enter mount cleanup");
        assert_eq!(error.kind, "vault_validation_failed");
    }

    #[test]
    fn vault_apply_preflight_rejects_duplicate_machine_drive_letters() {
        let mut args = valid_vault_policy_args();
        args["entries"][0]["mount"]["presentation"] = serde_json::json!("machine");
        args["entries"][0]["grants"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({ "principal_name": "Partner", "access": "read" }));
        let mut duplicate = args["entries"][0].clone();
        duplicate["id"] = serde_json::json!("vault-2");
        duplicate["container_path"] = serde_json::json!(r"C:\\Vaults\\second.hc");
        duplicate["mount"]["preferred_letter"] = serde_json::json!("v");
        args["entries"].as_array_mut().unwrap().push(duplicate);

        let error = prepare_vault_apply_policy(args)
            .expect_err("one machine drive letter must not be promised twice");
        assert_eq!(error.kind, "vault_validation_failed");
    }

    #[test]
    fn personal_mount_parser_preserves_only_explicit_repair_requests() {
        for repair in [None, Some(false), Some(true)] {
            let mut args = serde_json::json!({
                "container_path": "C:\\vaults\\personal.hc",
                "password": "test-only-password",
                "volume_kind": "standard",
                "volume_role": "outer"
            });
            if let Some(repair) = repair {
                args["repair_current_account_access"] = serde_json::json!(repair);
            }
            let result = parse_personal_mount_request(&mut args);
            assert!(args.is_null(), "request JSON must not retain credentials");
            let mut request = result.expect("ordinary, legacy, and explicit recovery mounts parse");
            assert_eq!(
                request.repair_current_account_access,
                repair.unwrap_or(false)
            );
            request.zeroize_secrets();
        }
    }

    #[test]
    fn personal_mount_preflight_failures_are_distinct_from_native_engine_failure() {
        let failures = [
            PERSONAL_VAULT_CONTAINER_UNWRITABLE,
            PERSONAL_VAULT_SESSION_ABSENT,
            PERSONAL_VAULT_DRIVER_STOPPED,
            PERSONAL_VAULT_UNAUTHORIZED,
        ];
        assert_eq!(
            failures
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            failures.len()
        );
        assert!(failures
            .iter()
            .all(|failure| *failure != "vault_engine_mount_failed"));
    }

    #[test]
    fn personal_vault_requires_an_active_wts_session_not_only_a_nonzero_id() {
        assert!(session_is_active(7, Some(WTSActive)));
        assert!(!session_is_active(
            7,
            Some(windows_sys::Win32::System::RemoteDesktop::WTSDisconnected)
        ));
        assert!(!session_is_active(7, None));
        assert!(!session_is_active(0, Some(WTSActive)));
    }

    // ── authorize: table-driven over CapabilityClass ─────────────────────

    #[tokio::test]
    async fn read_only_verb_allowed_regardless_of_privilege() {
        let gate = passing_gate();
        for caller_privileged in [false, true] {
            let result = authorize("svc.ping", caller_privileged, 1234, &gate).await;
            assert_eq!(result, Ok(None));
        }
    }

    #[tokio::test]
    async fn owner_fragment_does_not_depend_on_transient_wts_state() {
        let result = authorize_with_interactive_session(
            "svc.vault.apply_owner_fragment",
            false,
            false,
            1234,
            &passing_gate(),
        )
        .await;
        assert_eq!(result, Ok(None));
    }

    #[tokio::test]
    async fn vault_principal_list_requires_an_actual_local_administrator() {
        let gate = passing_gate();
        assert_eq!(
            authorize("svc.vault.list_principals", false, 1234, &gate).await,
            Err("privileged verb requires SYSTEM/Admin caller".to_string()),
        );
        assert_eq!(
            authorize("svc.vault.list_principals", true, 1234, &gate).await,
            Ok(None),
        );
    }

    #[tokio::test]
    async fn personal_vault_creation_allows_standard_user_only_in_active_session() {
        let gate = passing_gate();
        assert_eq!(
            authorize_with_interactive_session(
                "svc.vault.create_personal",
                false,
                true,
                1234,
                &gate,
            )
            .await,
            Ok(None),
        );
        assert_eq!(
            authorize_with_interactive_session(
                "svc.vault.create_personal",
                false,
                false,
                1234,
                &gate,
            )
            .await,
            Err("personal Vault creation requires an active Windows session".to_string()),
        );
    }

    #[test]
    fn personal_vault_creation_broker_failures_keep_the_stable_reason_code() {
        for (reason, expected) in [
            (
                wincmd_shared::vault_access::VaultMountReason::BrokerUnavailable,
                "vault_broker_unavailable",
            ),
            (
                wincmd_shared::vault_access::VaultMountReason::EntitlementDenied,
                "vault_entitlement_denied",
            ),
            (
                wincmd_shared::vault_access::VaultMountReason::BrokerRejected,
                "vault_broker_rejected",
            ),
        ] {
            assert_eq!(personal_vault_creation_failure(reason).kind, expected);
        }
    }

    #[test]
    fn personal_file_create_target_keeps_the_existing_absolute_path_contract() {
        let args = serde_json::json!({
            "TargetKind": "file",
            "Path": r"C:\Users\Parth\vault.hc",
        });
        let target = parse_personal_vault_create_target(&args).expect("file target is valid");
        assert!(matches!(
            target,
            PersonalVaultCreateTarget::File { ref path }
                if path == r"C:\Users\Parth\vault.hc"
        ));
    }

    #[test]
    fn personal_device_create_requires_the_complete_reviewed_identity() {
        let args = serde_json::json!({
            "TargetKind": "device",
            "Path": "",
            "DeviceDiskNumber": 4,
            "DevicePartitionNumber": 2,
            "DevicePartitionGuid": "{4a46e8c1-dac0-4b7a-8b5e-0f9f0e719a4a}",
            "DeviceOffsetBytes": 1_048_576,
            "DeviceSizeBytes": 5_368_709_120u64,
            "DeviceDiskUniqueId": "SCSI\\Disk&Ven_Test&Prod_Test",
        });
        assert!(matches!(
            parse_personal_vault_create_target(&args),
            Ok(PersonalVaultCreateTarget::Device)
        ));
    }

    #[test]
    fn personal_device_create_accepts_the_backend_decimal_string_wire_shape() {
        let args = serde_json::json!({
            "TargetKind": "device",
            "Path": "",
            "DeviceDiskNumber": "4",
            "DevicePartitionNumber": "2",
            "DevicePartitionGuid": "{4a46e8c1-dac0-4b7a-8b5e-0f9f0e719a4a}",
            "DeviceOffsetBytes": "1048576",
            "DeviceSizeBytes": "5368709120",
            "DeviceDiskUniqueId": "SCSI\\Disk&Ven_Test&Prod_Test",
        });
        assert!(matches!(
            parse_personal_vault_create_target(&args),
            Ok(PersonalVaultCreateTarget::Device)
        ));

        let malformed = serde_json::json!({
            "TargetKind": "device",
            "Path": "",
            "DeviceDiskNumber": "4.0",
            "DevicePartitionNumber": "2",
            "DevicePartitionGuid": "guid",
            "DeviceOffsetBytes": "1048576",
            "DeviceSizeBytes": "5368709120",
            "DeviceDiskUniqueId": "disk",
        });
        assert!(parse_personal_vault_create_target(&malformed).is_err());
    }

    #[test]
    fn personal_device_create_rejects_missing_identity_or_a_file_path() {
        let missing_disk_id = serde_json::json!({
            "TargetKind": "device",
            "Path": "",
            "DeviceDiskNumber": 4,
            "DevicePartitionNumber": 2,
            "DevicePartitionGuid": "guid",
            "DeviceOffsetBytes": 1_048_576,
            "DeviceSizeBytes": 5_368_709_120u64,
        });
        assert!(matches!(
            parse_personal_vault_create_target(&missing_disk_id),
            Err((
                "VLT.CREATE.DEVICE_IDENTITY_INVALID",
                "selected partition identity is incomplete or invalid",
            ))
        ));

        let device_with_path = serde_json::json!({
            "TargetKind": "device",
            "Path": r"C:\\not-a-device.hc",
            "DeviceDiskNumber": 4,
            "DevicePartitionNumber": 2,
            "DevicePartitionGuid": "guid",
            "DeviceOffsetBytes": 1_048_576,
            "DeviceSizeBytes": 5_368_709_120u64,
            "DeviceDiskUniqueId": "disk",
        });
        assert!(parse_personal_vault_create_target(&device_with_path).is_err());
    }

    #[test]
    fn capability_probe_is_not_a_policy_management_operation() {
        assert!(!is_vault_management_verb("svc.vault.capabilities"));
        assert!(is_vault_management_verb("svc.vault.get_policy"));
        assert!(is_vault_management_verb("svc.vault.get_status"));
        assert!(is_vault_management_verb("svc.vault.apply_policy"));
        assert!(is_vault_management_verb(
            "svc.vault.forget_entry_policy_only"
        ));
        // Task B: reconcile_access_groups is gated exactly like
        // apply_policy (Privileged / SYSTEM-Admin only) but is deliberately
        // NOT a "Vault Policy Administrator" capability-token verb — only
        // an actual SYSTEM/Admin caller may mutate real Windows local
        // groups, unlike the policy-document verbs above.
        assert!(!is_vault_management_verb(
            "svc.vault.reconcile_access_groups"
        ));
    }

    #[tokio::test]
    async fn vault_reconcile_access_groups_from_unprivileged_caller_is_denied() {
        let gate = passing_gate();
        let result = authorize("svc.vault.reconcile_access_groups", false, 1234, &gate).await;
        assert_eq!(
            result,
            Err("privileged verb requires SYSTEM/Admin caller".to_string())
        );
    }

    #[tokio::test]
    async fn vault_reconcile_access_groups_from_privileged_caller_is_allowed() {
        let gate = passing_gate();
        let result = authorize("svc.vault.reconcile_access_groups", true, 1234, &gate).await;
        assert_eq!(result, Ok(None));
    }

    #[tokio::test]
    async fn privileged_verb_from_unprivileged_caller_is_denied() {
        let gate = passing_gate();
        let result = authorize("svc.dispatch", false, 1234, &gate).await;
        assert_eq!(
            result,
            Err("privileged verb requires SYSTEM/Admin caller".to_string())
        );
    }

    #[tokio::test]
    async fn machine_setting_rpc_requires_a_privileged_caller() {
        let gate = passing_gate();
        let result = authorize(APPLY_MACHINE_SETTING_VERB, false, 1234, &gate).await;
        assert_eq!(
            result,
            Err("privileged verb requires SYSTEM/Admin caller".to_string())
        );
    }

    #[tokio::test]
    async fn privileged_verb_from_privileged_caller_is_allowed() {
        let gate = passing_gate();
        let result = authorize("svc.dispatch", true, 1234, &gate).await;
        assert_eq!(result, Ok(None));
    }

    #[tokio::test]
    async fn unknown_verb_from_unprivileged_caller_is_denied_fail_closed() {
        let gate = passing_gate();
        let result = authorize("svc.totally_unknown_verb", false, 1234, &gate).await;
        assert!(
            result.is_err(),
            "unknown verbs must fail closed as Privileged"
        );
    }

    #[tokio::test]
    async fn unknown_verb_from_privileged_caller_still_classifies_privileged() {
        let gate = passing_gate();
        let result = authorize("svc.totally_unknown_verb", true, 1234, &gate).await;
        assert_eq!(result, Ok(None));
    }

    #[tokio::test]
    async fn all_session_helper_verbs_allow_unsigned_path_pinned_peer() {
        let gate = passing_gate();
        for verb in [
            "svc.clipboard.report_event",
            "svc.policy.install_epoch",
            "svc.ink_receipt.reserve_ticket",
            "svc.ink_receipt.report_receipt",
        ] {
            let result = authorize(verb, false, 1234, &gate).await;
            assert_eq!(
                result,
                Ok(Some(TrustOrigin::SessionHelperPinned)),
                "unsigned path-pinned helper should be allowed for {verb}"
            );
        }
    }

    #[tokio::test]
    async fn session_helper_verb_privileged_caller_alone_no_longer_suffices() {
        // D-2's central point: admin/SYSTEM privilege is not a substitute
        // for peer_auth confirmation. A gate that denies must still deny
        // even when `caller_privileged` is true.
        let gate = gate_denying_with(PeerAuthError::PathNotAllowed);
        let result = authorize("svc.clipboard.report_event", true, 1234, &gate).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn session_helper_verb_denied_for_each_individual_peer_auth_failure_reason() {
        for reason in [
            PeerAuthError::IdentityUnavailable,
            PeerAuthError::NoInteractiveSession,
            PeerAuthError::WrongSession,
            PeerAuthError::PathNotAllowed,
        ] {
            let gate = gate_denying_with(reason);
            let result = authorize("svc.clipboard.report_event", false, 1234, &gate).await;
            assert!(result.is_err(), "expected deny for {reason:?}");
        }
    }

    #[tokio::test]
    async fn session_helper_verb_rate_limited_after_max_calls() {
        let gate = passing_gate();
        for _ in 0..crate::peer_auth::RATE_LIMIT_MAX_CALLS {
            authorize("svc.clipboard.report_event", false, 1234, &gate)
                .await
                .expect("within limit");
        }
        let result = authorize("svc.clipboard.report_event", false, 1234, &gate).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn authorize_error_messages_never_look_like_a_path() {
        // `PeerAuthError`'s own `Display` (exercised via every
        // `SessionHelper`-deny reason) never contains a path separator at
        // all — see that type's own `error_display_never_looks_like_a_path`
        // test. The fixed `Privileged`-deny string legitimately contains a
        // literal `/` as English punctuation ("SYSTEM/Admin"), so this
        // test only checks for a backslash there — a `/` alone isn't
        // evidence of a leaked Windows path.
        let privileged_deny = authorize("svc.dispatch", false, 1, &passing_gate())
            .await
            .unwrap_err();
        assert!(
            !privileged_deny.contains('\\'),
            "leaked a path: {privileged_deny}"
        );

        for reason in [
            PeerAuthError::IdentityUnavailable,
            PeerAuthError::WrongSession,
            PeerAuthError::PathNotAllowed,
        ] {
            let gate = gate_denying_with(reason);
            let err = authorize("svc.clipboard.report_event", false, 1, &gate)
                .await
                .unwrap_err();
            assert!(
                !err.contains('\\') && !err.contains('/'),
                "leaked a path for {reason:?}: {err}"
            );
        }
    }

    // ── install_epoch: version guard + independent signature verification ─

    #[test]
    fn install_epoch_accepts_first_version_and_rejects_stale_replay() {
        let store = test_policy_store();
        let v1 = pipe_wire_args(1, PIPE_SIG_V1_B64);
        handle_install_epoch(&store, v1.clone()).expect("first install of v1 succeeds");

        // Same version again (a replay) must be rejected, not silently
        // re-applied.
        let err = handle_install_epoch(&store, v1).unwrap_err();
        assert_eq!(err.kind, "policy_rejected");
    }

    #[test]
    fn install_epoch_accepts_strictly_greater_version() {
        let store = test_policy_store();
        handle_install_epoch(&store, pipe_wire_args(1, PIPE_SIG_V1_B64)).expect("v1 installs");
        let result = handle_install_epoch(&store, pipe_wire_args(2, PIPE_SIG_V2_B64));
        assert!(result.is_ok(), "{:?}", result.err().map(|e| e.message));
    }

    #[tokio::test]
    async fn install_epoch_forged_signature_is_rejected_even_though_peer_is_pinned() {
        // The peer is fully pinned for this verb (SessionHelperGate grants
        // trust)...
        let gate = passing_gate();
        let auth = authorize("svc.policy.install_epoch", false, 1234, &gate).await;
        assert_eq!(auth, Ok(Some(TrustOrigin::SessionHelperPinned)));

        // ...but a forged epoch (v2's signature claimed for v1) is still
        // rejected by svc's own independent verification (D-7's reasoning
        // applied to this hop) — peer trust never substitutes for it.
        let store = test_policy_store();
        let forged = pipe_wire_args(1, PIPE_SIG_V2_B64);
        let err = handle_install_epoch(&store, forged).unwrap_err();
        assert_eq!(err.kind, "policy_rejected");
    }

    #[test]
    fn install_epoch_no_op_when_neither_subtree_key_present() {
        let store = test_policy_store();
        let args = serde_json::json!({
            "policy_version": 1,
            "config": {},
            "locked_paths": [],
            "managed": true,
            "target_kind": "org",
            "signature": "irrelevant",
            "signer_key": "irrelevant",
        });
        let result = handle_install_epoch(&store, args).expect("no-op is not an error");
        assert_eq!(result["applied"], serde_json::json!([]));
    }

    #[test]
    fn install_epoch_errors_never_leak_a_path() {
        let store = test_policy_store();
        let err = handle_install_epoch(&store, serde_json::json!({})).unwrap_err();
        assert_eq!(err.kind, "bad_request");
        assert!(!err.message.contains('\\') && !err.message.contains('/'));

        let forged = pipe_wire_args(1, PIPE_SIG_V2_B64);
        let err = handle_install_epoch(&store, forged).unwrap_err();
        assert!(!err.message.contains('\\') && !err.message.contains('/'));
    }

    // ── get_policy: wire shape must match clipboard-guard-helper's client
    // contract exactly (`clipboard_guard_helper::policy::
    // ClipboardPolicyResponse { policy_version: i64, rules: Vec<Rule> }`,
    // both fields required, no `Option`/`#[serde(default)]`). commander-svc
    // does not depend on that crate, so this pins the exact JSON shape
    // directly rather than deserializing into the real client type — but
    // the two must never drift, since a field-name mismatch here is a
    // silent runtime failure no compiler catches (every real `get_policy`
    // call would come back `SvcError::Malformed` on the client side).

    #[test]
    fn get_policy_response_shape_matches_client_contract_when_nothing_installed() {
        let store = test_policy_store();
        let value = clipboard_policy_response(&store);
        assert_eq!(value["policy_version"], serde_json::json!(0));
        assert_eq!(value["rules"], serde_json::json!([]));
        // The client's `ClipboardPolicyResponse` has no `installed` field
        // and no `version` field — either key here would be silently
        // ignored at best, or (since both real fields are required with no
        // default) a MISSING `policy_version`/`rules` would hard-fail
        // deserialization. Assert their absence explicitly so a future
        // edit can't quietly reintroduce the old, mismatched shape.
        assert!(value.get("installed").is_none());
        assert!(value.get("version").is_none());
    }

    #[test]
    fn get_policy_response_shape_matches_client_contract_when_installed() {
        let store = test_policy_store();
        handle_install_epoch(&store, pipe_wire_args(1, PIPE_SIG_V1_B64)).expect("v1 installs");
        let value = clipboard_policy_response(&store);
        assert_eq!(value["policy_version"], serde_json::json!(1));
        assert!(value["rules"].as_array().is_some_and(|r| !r.is_empty()));
        assert!(value.get("installed").is_none());
        assert!(value.get("version").is_none());
    }

    // ── report_event / set_enabled ───────────────────────────────────────

    #[test]
    fn report_event_is_queued_with_trust_origin_and_flips_clear_failing() {
        let state = ClipboardGuardState::new();
        let result = handle_report_event(
            &state,
            sample_clipboard_event_report(),
            TrustOrigin::SessionHelperPinned,
        );
        assert!(result.is_ok());
        assert_eq!(state.events.len(), 1);
        let snapshot = state.events.snapshot();
        assert_eq!(snapshot[0].trust_origin, TrustOrigin::SessionHelperPinned);

        let store = test_policy_store();
        enforcement_tick(&store, &state, Instant::now());
        assert!(
            state.health().clear_failing,
            "an attempted-but-not-succeeded clear action must flip clear_failing"
        );
    }

    #[test]
    fn report_event_malformed_payload_is_rejected_without_queuing() {
        let state = ClipboardGuardState::new();
        let bad = serde_json::json!({ "not": "a report" });
        let err = handle_report_event(&state, bad, TrustOrigin::SessionHelperPinned).unwrap_err();
        assert_eq!(err.kind, "bad_request");
        assert_eq!(state.events.len(), 0);
    }

    #[test]
    fn report_event_rejected_when_administratively_disabled() {
        let state = ClipboardGuardState::new();
        state.set_enabled(false);
        let err = handle_report_event(
            &state,
            sample_clipboard_event_report(),
            TrustOrigin::SessionHelperPinned,
        )
        .unwrap_err();
        assert_eq!(err.kind, "clipboard_guard_disabled");
    }

    #[test]
    fn set_enabled_toggles_state() {
        let state = ClipboardGuardState::new();
        assert!(state.is_enabled());
        let result = handle_set_enabled(&state, serde_json::json!({ "enabled": false })).unwrap();
        assert_eq!(result, serde_json::json!({ "enabled": false }));
        assert!(!state.is_enabled());
    }

    // ── enforcement_tick: queue retention + health ───────────────────────

    #[test]
    fn enforcement_tick_prunes_aged_events_but_retains_fresh_ones() {
        let state = ClipboardGuardState::new();
        let far_past = Instant::now()
            .checked_sub(CLIPBOARD_EVENT_MAX_AGE + Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        state.events.push(QueuedClipboardEvent {
            report: serde_json::from_value(sample_clipboard_event_report()).unwrap(),
            trust_origin: TrustOrigin::SessionHelperPinned,
            queued_at: far_past,
        });
        state.events.push(QueuedClipboardEvent {
            report: serde_json::from_value(sample_clipboard_event_report()).unwrap(),
            trust_origin: TrustOrigin::SessionHelperPinned,
            queued_at: Instant::now(),
        });
        assert_eq!(state.events.len(), 2);

        let store = test_policy_store();
        enforcement_tick(&store, &state, Instant::now());

        // The aged-out entry is gone; the fresh one is retained "for
        // pickup" (never destructively drained by this tick).
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.health().queued_events, 1);
    }

    #[test]
    fn enforcement_tick_reflects_policy_store_health() {
        let store = test_policy_store();
        let state = ClipboardGuardState::new();
        enforcement_tick(&store, &state, Instant::now());
        assert!(!state.health().policy_current, "nothing installed yet");

        handle_install_epoch(&store, pipe_wire_args(1, PIPE_SIG_V1_B64)).expect("v1 installs");
        enforcement_tick(&store, &state, Instant::now());
        assert!(state.health().policy_current);
        assert!(state.health().rules_compiled);
    }

    #[test]
    fn mount_secret_parser_moves_only_the_password_and_clears_the_json_shell() {
        let mut args = serde_json::json!({"entry_id":"shared","password":"canary-secret","volume_role":"hidden"});
        let request = take_vault_mount_request(&mut args).unwrap();
        assert_eq!(request.entry_id, "shared");
        assert_eq!(request.password, "canary-secret");
        assert_eq!(
            request.volume_role,
            wincmd_shared::vault_access::VaultVolumeRole::Hidden
        );
        assert!(request.hidden_protection_password.is_none());
        assert!(args.as_object().unwrap().is_empty());
    }

    #[test]
    fn personal_mount_queries_reject_forged_identity_and_mutation_fields() {
        assert!(super::valid_personal_mount_query(
            &serde_json::json!({"personal":true})
        ));
        for invalid in [
            serde_json::json!({"personal":false}),
            serde_json::json!({"personal":true,"caller_sid":"forged"}),
            serde_json::json!({"personal":true,"internal_drive":26}),
            serde_json::json!({"personal":true,"internal_drive":-1}),
            serde_json::json!({"personal":true,"internal_drive":1,"session_id":7}),
            serde_json::json!({"personal":true,"internal_drive":1,"force":"true"}),
        ] {
            assert!(!super::valid_personal_mount_query(&invalid));
        }
        assert!(super::require_personal_mount_peer(None).is_err());
    }

    #[test]
    fn mount_secret_parser_keeps_legacy_standard_requests_compatible() {
        let mut args = serde_json::json!({"entry_id":"shared","password":"canary-secret"});
        let request = take_vault_mount_request(&mut args).unwrap();
        assert_eq!(
            request.volume_role,
            wincmd_shared::vault_access::VaultVolumeRole::Outer
        );
        assert!(request.hidden_protection_password.is_none());
        assert!(args.as_object().unwrap().is_empty());
    }

    #[test]
    fn mount_secret_parser_moves_hidden_protection_password_without_retaining_json() {
        let mut args = serde_json::json!({"entry_id":"shared","password":"outer-secret","volume_role":"outer","hidden_protection_password":"hidden-secret"});
        let request = take_vault_mount_request(&mut args).unwrap();
        assert_eq!(
            request.hidden_protection_password.as_deref(),
            Some("hidden-secret")
        );
        assert!(args.as_object().unwrap().is_empty());
    }

    // ── SVC_PROTOCOL_VERSION is exported and non-empty ───────────────────

    #[test]
    fn protocol_version_non_empty() {
        assert!(!SVC_PROTOCOL_VERSION.is_empty());
    }
}

/// Integration tests: spin up the pipe server, connect a client, perform
/// the Hello handshake, send a Request (bare or `Signed`), and verify
/// end-to-end behaviour — ACL enforcement with a forced privilege flag
/// (injected so we don't depend on the test process's real SID), and the
/// real Clipboard Guard verb dispatch over the actual wire framing.
#[cfg(test)]
mod integration {
    use tokio::net::windows::named_pipe::{ClientOptions, PipeMode, ServerOptions};
    use wincmd_shared::svc::hello_from_ui;
    use wincmd_shared::{read_envelope, write_envelope, Envelope, ErrorReply, Request};

    use super::test_support;
    use super::{handle_connection, ClipboardGuardState};
    use std::sync::Arc;

    /// Spin up one server instance on a uniquely-named test pipe, inject
    /// `forced_privilege`, connect a client, do the Hello handshake, send
    /// `verb` as a BARE (unsigned) Request, return the reply envelope.
    ///
    /// Each caller passes a distinct `pipe_suffix` so concurrent tests use
    /// different pipe names and don't race on `first_pipe_instance`.
    async fn run_one_request(
        pipe_suffix: &str,
        verb: &str,
        forced_privilege: bool,
        capture_live_peer: bool,
    ) -> Envelope {
        let pipe_name = format!(r"\\.\pipe\wincmd-svc-test-{}", pipe_suffix);

        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("create test pipe server");

        let pipe_name2 = pipe_name.clone();
        let policy_store = test_support::test_policy_store();
        let session_helper_gate = test_support::passing_gate();
        let clipboard_state = Arc::new(ClipboardGuardState::new());
        let vault_access = crate::vault_access::test_store();
        let vault_mount = Arc::new(crate::vault_mount::VaultMountBroker::new());

        let server_task = tokio::spawn(async move {
            let s = server;
            s.connect().await.expect("pipe connect");
            handle_connection(
                s,
                forced_privilege,
                false,
                None,
                capture_live_peer,
                policy_store,
                session_helper_gate,
                clipboard_state,
                vault_access,
                vault_mount,
            )
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
            diagnostic_operation_id: None,
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
        let reply = run_one_request(
            "forbidden",
            wincmd_shared::svc::APPLY_MACHINE_SETTING_VERB,
            false,
            false,
        )
        .await;
        match reply {
            Envelope::Error(ErrorReply { kind, .. }) => {
                assert_eq!(kind, "forbidden", "expected kind=forbidden, got {:?}", kind);
            }
            other => panic!("expected Envelope::Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn privileged_known_verb_with_forced_privileged_reaches_handler() {
        let reply = run_one_request(
            "priv-ok",
            wincmd_shared::svc::APPLY_MACHINE_SETTING_VERB,
            true,
            false,
        )
        .await;
        match reply {
            Envelope::Error(ErrorReply { kind, .. }) => {
                assert_eq!(kind, "machine_setting_validation_failed");
            }
            other => panic!("expected handler validation error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn unknown_verb_returns_the_same_bounded_error_for_every_caller() {
        for (suffix, privileged) in [("unknown-user", false), ("unknown-admin", true)] {
            let reply =
                run_one_request(suffix, "svc.totally_unknown_verb", privileged, false).await;
            match reply {
                Envelope::Error(ErrorReply {
                    request_id,
                    kind,
                    message,
                }) => {
                    assert_eq!(request_id, 1);
                    assert_eq!(kind, "unknown_verb");
                    assert_eq!(message, "service verb is not recognized");
                    assert!(!message.contains("totally_unknown"));
                }
                other => panic!("expected Envelope::Error, got {:?}", other),
            }
        }
    }

    #[tokio::test]
    async fn read_only_verb_with_forced_unprivileged_returns_response() {
        let reply = run_one_request("ro-ok", "svc.ping", false, false).await;
        match reply {
            Envelope::Response(r) => {
                assert_eq!(r.result["pong"], true);
            }
            other => panic!("expected Envelope::Response, got {:?}", other),
        }
    }

    // ── Task B: svc.vault.reconcile_access_groups end-to-end gate ─────────

    #[tokio::test]
    async fn vault_reconcile_access_groups_with_forced_unprivileged_returns_forbidden() {
        let reply = run_one_request(
            "vault-reconcile-forbidden",
            "svc.vault.reconcile_access_groups",
            false,
            false,
        )
        .await;
        match reply {
            Envelope::Error(ErrorReply { kind, .. }) => {
                assert_eq!(kind, "forbidden", "expected kind=forbidden, got {:?}", kind);
            }
            other => panic!("expected Envelope::Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn vault_legacy_group_wire_cannot_bypass_directory_membership_checks() {
        let reply = run_one_request(
            "vault-reconcile-priv",
            "svc.vault.reconcile_access_groups",
            true,
            false,
        )
        .await;
        match reply {
            Envelope::Error(ErrorReply { kind, .. }) => {
                assert_eq!(kind, "vault_legacy_group_wire_retired");
            }
            other => panic!("unexpected reply: {:?}", other),
        }
    }

    #[test]
    fn vault_access_directory_pipe_save_requires_authenticated_creator() {
        let store = crate::vault_access::test_store();
        let vault_mount = crate::vault_mount::VaultMountBroker::new();
        let request = serde_json::json!({
            "directory": {
                "schema_version": 1,
                "users": [{
                    "sid": "S-1-5-21-101",
                    "username": "Alex",
                    "display_name": "Alex Example"
                }],
                "groups": [{
                    "id": "sales",
                    "name": "Sales",
                    "local_group": "WC_Sales",
                    "member_sids": ["S-1-5-21-101"]
                }]
            }
        });
        let error = super::handle_vault_save_access_directory(&store, &vault_mount, request, None)
            .unwrap_err();
        assert_eq!(error.kind, "vault_not_authorized");
        assert!(store.access_directory().unwrap().groups.is_empty());
    }

    #[tokio::test]
    async fn captures_the_named_pipe_peer_after_hello_before_responding() {
        let reply = run_one_request("capture-after-hello", "svc.ping", false, true).await;
        assert!(matches!(reply, Envelope::Response(_)));
    }

    /// End-to-end: a `Signed` request (the documented post-handshake shape
    /// — matches exactly how `commander-free`'s real
    /// `relay_epoch_to_svc`/epoch relay connects) for a `SessionHelper`
    /// verb is unwrapped, authorized via the (fake, passing) peer gate,
    /// and dispatched for real.
    #[tokio::test]
    async fn signed_session_helper_request_report_event_is_accepted() {
        let pipe_name = r"\\.\pipe\wincmd-svc-test-signed-report-event";
        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(true)
            .create(pipe_name)
            .expect("create test pipe server");

        let policy_store = test_support::test_policy_store();
        let session_helper_gate = test_support::passing_gate();
        let clipboard_state = Arc::new(ClipboardGuardState::new());
        let vault_access = crate::vault_access::test_store();
        let vault_mount = Arc::new(crate::vault_mount::VaultMountBroker::new());

        let server_task = tokio::spawn(async move {
            let s = server;
            s.connect().await.expect("pipe connect");
            handle_connection(
                s,
                false,
                false,
                None,
                false,
                policy_store,
                session_helper_gate,
                clipboard_state,
                vault_access,
                vault_mount,
            )
            .await
            .expect("handle_connection");
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = ClientOptions::new()
            .pipe_mode(PipeMode::Byte)
            .open(pipe_name)
            .expect("open test pipe client");

        let session_token = "signed-test-token".to_string();
        let hello = Envelope::Hello(hello_from_ui(&session_token));
        write_envelope(&mut client, &hello)
            .await
            .expect("write Hello");
        let ack = read_envelope(&mut client).await.expect("read Hello ack");
        assert!(matches!(ack, Envelope::Hello(_)));

        let req = Envelope::Request(Request {
            request_id: 1,
            feature_id: "svc.clipboard.report_event".to_string(),
            diagnostic_operation_id: None,
            args: test_support::sample_clipboard_event_report(),
        })
        .sign(&session_token);
        write_envelope(&mut client, &req)
            .await
            .expect("write signed Request");

        let reply = read_envelope(&mut client).await.expect("read reply");
        match reply {
            Envelope::Response(r) => assert_eq!(r.result["accepted"], true),
            other => panic!("expected Envelope::Response, got {:?}", other),
        }

        let _ = write_envelope(&mut client, &Envelope::Bye).await;
        server_task.await.ok();
    }

    /// End-to-end: a `Signed` `svc.policy.install_epoch` request (the same
    /// framing `commander-free::relay_epoch_to_svc` uses) is verified and
    /// applied through the real pipe.
    #[tokio::test]
    async fn signed_install_epoch_request_is_verified_and_applied() {
        let pipe_name = r"\\.\pipe\wincmd-svc-test-signed-install-epoch";
        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(true)
            .create(pipe_name)
            .expect("create test pipe server");

        let policy_store = test_support::test_policy_store();
        let session_helper_gate = test_support::passing_gate();
        let clipboard_state = Arc::new(ClipboardGuardState::new());
        let vault_access = crate::vault_access::test_store();
        let vault_mount = Arc::new(crate::vault_mount::VaultMountBroker::new());

        let server_task = tokio::spawn(async move {
            let s = server;
            s.connect().await.expect("pipe connect");
            handle_connection(
                s,
                false,
                false,
                None,
                false,
                policy_store,
                session_helper_gate,
                clipboard_state,
                vault_access,
                vault_mount,
            )
            .await
            .expect("handle_connection");
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = ClientOptions::new()
            .pipe_mode(PipeMode::Byte)
            .open(pipe_name)
            .expect("open test pipe client");

        let session_token = "signed-test-token".to_string();
        let hello = Envelope::Hello(hello_from_ui(&session_token));
        write_envelope(&mut client, &hello)
            .await
            .expect("write Hello");
        let ack = read_envelope(&mut client).await.expect("read Hello ack");
        assert!(matches!(ack, Envelope::Hello(_)));

        let req = Envelope::Request(Request {
            request_id: 1,
            feature_id: "svc.policy.install_epoch".to_string(),
            diagnostic_operation_id: None,
            args: test_support::pipe_wire_args(1, test_support::PIPE_SIG_V1_B64),
        })
        .sign(&session_token);
        write_envelope(&mut client, &req)
            .await
            .expect("write signed Request");

        let reply = read_envelope(&mut client).await.expect("read reply");
        match reply {
            Envelope::Response(r) => {
                assert_eq!(r.result["applied"], serde_json::json!(["clipboardGuard"]));
            }
            other => panic!("expected Envelope::Response, got {:?}", other),
        }

        let _ = write_envelope(&mut client, &Envelope::Bye).await;
        server_task.await.ok();
    }
}
