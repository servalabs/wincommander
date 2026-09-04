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

use std::collections::VecDeque;
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
use wincmd_shared::svc::{
    classify_verb, is_known_verb, ApplyMachineSettingRequest, CapabilityClass,
    APPLY_MACHINE_SETTING_VERB, SVC_PIPE_NAME, SVC_PROTOCOL_VERSION,
};
use wincmd_shared::{
    read_envelope, write_envelope, Envelope, ErrorReply, Hello, Request, Response,
};

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
        "svc.vault.get_policy" | "svc.vault.get_status" | "svc.vault.apply_policy"
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

        let policy_store = Arc::clone(&policy_store);
        let session_helper_gate = Arc::clone(&session_helper_gate);
        let clipboard_state = Arc::clone(&clipboard_state);
        let vault_access = Arc::clone(&vault_access);
        let vault_mount = Arc::clone(&vault_mount);

        tokio::spawn(async move {
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
    let session_token = match read_envelope(&mut conn).await.context("read Hello")? {
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
            let _ = write_envelope(&mut conn, &err).await;
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
    let ack = Envelope::Hello(wincmd_shared::svc::hello_from_ui("svc-ack"));
    write_envelope(&mut conn, &ack)
        .await
        .context("write Hello ack")?;

    // (b)/(c) Request loop.
    loop {
        let env = match read_envelope(&mut conn).await {
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
                        write_envelope(&mut conn, &reply).await?;
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
            write_envelope(&mut conn, &reply).await?;
            continue;
        }
        match authorize(
            &req.feature_id,
            caller_privileged
                || (vault_policy_manager && is_vault_management_verb(&req.feature_id)),
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
                write_envelope(&mut conn, &reply).await?;
            }
            Ok(trust_origin) => {
                if is_vault_management_verb(&req.feature_id)
                    && !caller_privileged
                    && !vault_policy_manager
                {
                    let reply = Envelope::Error(ErrorReply {
                        request_id,
                        kind: "forbidden".to_string(),
                        message: "vault policy operation requires Vault Policy Administrator"
                            .to_string(),
                    });
                    write_envelope(&mut conn, &reply).await?;
                    continue;
                }
                let reply = dispatch_verb(
                    req,
                    trust_origin,
                    &policy_store,
                    &clipboard_state,
                    &vault_access,
                    &vault_mount,
                    peer.as_deref(),
                    caller_privileged || vault_policy_manager,
                )
                .await;
                write_envelope(&mut conn, &reply).await?;
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
pub async fn authorize(
    verb: &str,
    caller_privileged: bool,
    pid: u32,
    session_helper_gate: &Arc<SessionHelperGate>,
) -> Result<Option<TrustOrigin>, String> {
    match classify_verb(verb) {
        CapabilityClass::ReadOnly => Ok(None),

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
        args,
    } = req;

    let outcome: Result<serde_json::Value, VerbError> = match feature_id.as_str() {
        "svc.status" => Ok(serde_json::to_value(settings_host::status())
            .unwrap_or_else(|_| serde_json::json!({"ok": true}))),
        "svc.get_settings" => Ok(settings_host::get_settings()),
        "svc.health" => Ok(settings_host::health()),
        "svc.ping" => Ok(serde_json::json!({ "pong": true })),

        APPLY_MACHINE_SETTING_VERB => handle_apply_machine_setting(args),

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

        "svc.vault.get_policy" => {
            Ok(serde_json::to_value(vault_access.policy()).unwrap_or(serde_json::Value::Null))
        }
        "svc.vault.get_status" => {
            Ok(serde_json::to_value(vault_access.status()).unwrap_or(serde_json::Value::Null))
        }
        "svc.vault.apply_policy" => handle_vault_apply_and_cleanup(vault_access, vault_mount, args),
        "svc.vault.authorize_mount" => handle_vault_authorize(vault_access, args, peer),
        "svc.vault.mount" if args.get("personal") == Some(&serde_json::Value::Bool(true)) => {
            handle_personal_vault_mount(request_id, vault_access, vault_mount, args, peer).await
        }
        "svc.vault.mount" => {
            handle_vault_mount(request_id, vault_access, vault_mount, args, peer).await
        }
        "svc.vault.create_personal" => {
            handle_personal_vault_create(request_id, vault_access, args, peer)
        }
        "svc.vault.unmount" => {
            handle_vault_unmount(request_id, vault_access, vault_mount, args, peer)
        }
        "svc.vault.list_authorized" => {
            handle_vault_list_authorized(vault_access, vault_mount, peer)
        }
        "svc.vault.capabilities" => {
            Ok(serde_json::json!({ "can_manage_policy": caller_privileged }))
        }
        "svc.vault.reconcile_access_groups" => {
            handle_vault_reconcile_access_groups(vault_access, args)
        }

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

fn handle_vault_apply(
    vault_access: &VaultAccessStore,
    args: serde_json::Value,
) -> Result<serde_json::Value, VerbError> {
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

/// A policy swap can remove a caller or narrow a grant. Dismount every active
/// entry *before* accepting the new policy; failure leaves the prior policy
/// intact and reports no false success to an administrator.
fn handle_vault_apply_and_cleanup(
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
) -> Result<serde_json::Value, VerbError> {
    vault_mount.with_exclusive_operation(|| {
        vault_mount.dismount_all_locked(vault_access).map_err(|_| {
            VerbError::new(
                "vault_dismount_failed",
                "active vaults could not be dismounted",
            )
        })?;
        handle_vault_apply(vault_access, args)
    })
}

/// Backs `svc.vault.reconcile_access_groups` — Privileged (SYSTEM/Admin
/// only, gated identically to `svc.vault.apply_policy`; see `classify_verb`
/// and this file's `authorize`/`is_vault_management_verb`). The Access
/// control UI defines admin-authored Windows local groups (friendly name +
/// `local_group`) and ticks users into them; the UI itself is unprivileged
/// and cannot create/mutate a real Windows group, so this SYSTEM-service
/// verb does it on the UI's behalf. `args`/response shape is the frozen
/// contract `commander-free::vault_access::reconcile_vault_access_groups`
/// already sends: `{"groups":[{"local_group":"...","member_sids":[...]}]}`
/// in, `{"results":[{"local_group","state","error"}]}` out. A malformed
/// request or an oversized batch fails the whole call; a single group's
/// reconciliation failure is instead reported in that group's own `result`
/// entry (see `VaultAccessStore::reconcile_access_groups`'s doc comment) so
/// it never aborts the rest of the batch.
fn handle_vault_reconcile_access_groups(
    vault_access: &VaultAccessStore,
    args: serde_json::Value,
) -> Result<serde_json::Value, VerbError> {
    let request: wincmd_shared::vault_access::VaultReconcileAccessGroupsRequest =
        serde_json::from_value(args).map_err(|_| {
            VerbError::new(
                "vault_validation_failed",
                "access group reconciliation request is invalid",
            )
        })?;
    let results = vault_access
        .reconcile_access_groups(&request.groups)
        .map_err(|error| VerbError::new("vault_validation_failed", vault_error_message(error)))?;
    serde_json::to_value(
        wincmd_shared::vault_access::VaultReconcileAccessGroupsResponse { results },
    )
    .map_err(|_| {
        VerbError::new(
            "vault_internal_error",
            "access group reconciliation response could not be created",
        )
    })
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
fn handle_personal_vault_create(
    operation_id: u64,
    vault_access: &VaultAccessStore,
    mut args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let Some(peer) = peer else {
        zeroize_json(&mut args);
        return Err(VerbError::new(
            PERSONAL_VAULT_SESSION_ABSENT,
            "no interactive Windows session",
        ));
    };
    let path = args
        .get("Path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if path.is_empty() || !Path::new(path).is_absolute() {
        zeroize_json(&mut args);
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal vault path is invalid",
        ));
    }
    if !matches!(
        args.get("TargetKind").and_then(serde_json::Value::as_str),
        None | Some("") | Some("file")
    ) {
        zeroize_json(&mut args);
        return Err(VerbError::new(
            "vault_validation_failed",
            "personal vault creation requires a file container",
        ));
    }
    if peer.caller_sid().is_empty() || !peer_has_active_interactive_session(peer) {
        zeroize_json(&mut args);
        return Err(VerbError::new(
            PERSONAL_VAULT_SESSION_ABSENT,
            "no interactive Windows session",
        ));
    }
    let requested_path = path.to_string();
    let now = crate::vault_access::unix_time_seconds();
    let registration = vault_access
        .begin_personal_registration(
            &requested_path,
            crate::vault_access::PersonalCreationCaller {
                owner_sid: peer.caller_sid(),
                session_id: peer.session_id(),
                client_pid: peer.client_pid(),
                authentication_id: peer.authentication_id(),
            },
            operation_id,
            now,
        )
        .map_err(|_| {
            VerbError::new(
                "vault_owner_record_failed",
                "personal vault ownership could not be recorded",
            )
        })?;
    let requested_path = registration.normalized_path().to_string();
    args["Path"] = serde_json::Value::String(requested_path.clone());
    args["TargetSessionId"] = serde_json::Value::from(peer.session_id());
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(crate::pro_broker::vault_call(
            crate::pro_broker::VaultCall {
                request_id: operation_id,
                target_session_id: peer.session_id(),
                caller_sid: peer.caller_sid(),
                caller_token: Some(peer.token()),
                caller_authentication_id: Some(peer.authentication_id()),
                presentation: wincmd_shared::vault_access::VaultPresentation::PerUser,
                feature_id: "vault.broker.create_personal",
                args,
            },
        ))
    })
    .map_err(|_| {
        vault_access.cancel_personal_registration(&registration);
        VerbError::new("vault_create_failed", "personal vault creation failed")
    })?;
    let broker_path = result
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if vault_access
        .record_personal_broker_completion(
            &registration,
            broker_path,
            crate::vault_access::unix_time_seconds(),
        )
        .is_err()
    {
        vault_access.cancel_personal_registration(&registration);
        return Err(VerbError::new(
            "vault_create_failed",
            "personal vault creation could not be verified",
        ));
    }
    vault_access
        .complete_personal_registration(&registration, crate::vault_access::unix_time_seconds())
        .map_err(|_| {
            VerbError::new(
                "vault_owner_record_failed",
                "personal vault ownership could not be recorded",
            )
        })?;
    Ok(result)
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
    let registered =
        match vault_access.personal_for_owner(&request.container_path, peer.caller_sid()) {
            Ok(record) => record,
            Err(_) => {
                zeroize_personal_mount(&mut request);
                return Err(VerbError::new(
                    PERSONAL_VAULT_UNAUTHORIZED,
                    "caller is not authorized for this personal vault",
                ));
            }
        };
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
    let mount_result = vault_mount.with_exclusive_operation(|| {
        let legacy = if registered.is_none() {
            match vault_access.prepare_legacy_personal_mount(
                &request.container_path,
                peer.caller_sid(),
                peer.session_id(),
                peer.token(),
            ) {
                Ok(preparation) => Some(preparation),
                Err(_) => {
                    zeroize_personal_mount(&mut request);
                    return Err(VerbError::new(
                        PERSONAL_VAULT_UNAUTHORIZED,
                        "caller is not authorized for this personal vault",
                    ));
                }
            }
        } else {
            None
        };
        let record = match (registered.as_ref(), legacy.as_ref()) {
            (Some(record), _) => record,
            (None, Some(preparation)) => preparation.record(),
            (None, None) => {
                zeroize_personal_mount(&mut request);
                return Err(VerbError::new(
                    PERSONAL_VAULT_UNAUTHORIZED,
                    "caller is not authorized for this personal vault",
                ));
            }
        };
        // The service runs in Session 0, where a DOS-device query cannot
        // authoritatively describe the caller's per-user device namespace. The
        // authenticated broker and native engine run in the requested desktop
        // session and reject an occupied presentation letter immediately before
        // linking it. Do not turn a Session-0 lookup failure into a false
        // "drive letter unavailable" result for a genuinely free letter.
        let mounted = vault_mount.mount_personal_authorized_locked(
            operation_id,
            vault_access,
            record,
            &mut request,
            peer.token(),
            peer.session_id(),
            peer.caller_sid(),
            peer.authentication_id(),
        );
        let mounted = match mounted {
            Ok(mounted) => mounted,
            Err(reason) => {
                if let Some(preparation) = legacy {
                    if reason == wincmd_shared::vault_access::VaultMountReason::DismountFailed {
                        vault_access.mark_personal_recovery_uncertain();
                        drop(preparation);
                        return Err(VerbError::new(
                            VaultMountBroker::personal_mount_failure_code(reason),
                            "personal vault cleanup could not be confirmed",
                        ));
                    }
                    if vault_access
                        .restore_legacy_personal_mount(preparation)
                        .is_err()
                    {
                        return Err(VerbError::new(
                            "vault_owner_record_failed",
                            "personal vault recovery could not be rolled back",
                        ));
                    }
                }
                return Err(VerbError::new(
                    VaultMountBroker::personal_mount_failure_code(reason),
                    "personal vault mount failed",
                ));
            }
        };
        if let Some(preparation) = legacy {
            if vault_access
                .commit_legacy_personal_mount(&preparation, peer.caller_sid(), peer.session_id())
                .is_err()
            {
                let dismounted = vault_mount.dismount_personal_registration_failure_locked(
                    operation_id,
                    vault_access,
                    preparation.record(),
                    peer.token(),
                );
                let restored = dismounted
                    && vault_access
                        .restore_legacy_personal_mount(preparation)
                        .is_ok();
                if !restored {
                    vault_access.mark_personal_recovery_uncertain();
                }
                return Err(VerbError::new(
                    "vault_owner_record_failed",
                    if dismounted && restored {
                        "personal vault ownership could not be recorded"
                    } else {
                        "personal vault recovery could not be rolled back"
                    },
                ));
            }
        }
        Ok(mounted)
    });
    let (drive_letter, internal_drive, acl_attested) = mount_result?;
    Ok(serde_json::json!({
        "status": "mounted",
        "drive": drive_letter,
        "internalDrive": internal_drive,
        "scope": "per-user",
        "aclAttested": acl_attested,
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
    operation_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    mut args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let mut request = take_vault_mount_request(&mut args)
        .map_err(|_| VerbError::new("vault_validation_failed", "mount request is invalid"))?;
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
                        request_id: operation_id,
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
            return Err(VerbError::new(
                "vault_driver_unavailable",
                error.public_message(),
            ));
        }
        let Some(peer) = peer else {
            return Err(VerbError::new(
                "vault_not_authorized",
                "vault peer token unavailable",
            ));
        };
        vault_mount.mount_authorized(
            operation_id,
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
        }
    };
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
    operation_id: u64,
    vault_access: &VaultAccessStore,
    vault_mount: &VaultMountBroker,
    args: serde_json::Value,
    peer: Option<&AuthenticatedPipePeer>,
) -> Result<serde_json::Value, VerbError> {
    let request: wincmd_shared::vault_access::VaultUnmountRequest = serde_json::from_value(args)
        .map_err(|_| VerbError::new("vault_validation_failed", "unmount request is invalid"))?;
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
        let Some(peer) = peer else {
            return Err(VerbError::new(
                "vault_not_authorized",
                "vault peer token unavailable",
            ));
        };
        vault_mount.dismount_authorized(
            vault_access,
            crate::vault_mount::AuthorizedDismount {
                operation_id,
                entry_id: &request.entry_id,
                caller_token: peer.token(),
                caller_session: peer.session_id(),
                caller_sid: peer.caller_sid(),
                presentation: authorization
                    .presentation
                    .unwrap_or(wincmd_shared::vault_access::VaultPresentation::PerUser),
            },
        )
    } else {
        wincmd_shared::vault_access::VaultMountResult {
            entry_id: request.entry_id,
            state: wincmd_shared::vault_access::VaultMountState::Denied,
            presentation: None,
            drive_letter: None,
            reason: Some(wincmd_shared::vault_access::VaultMountReason::NotAuthorized),
        }
    };
    serde_json::to_value(result).map_err(|_| {
        VerbError::new(
            "vault_internal_error",
            "unmount result could not be created",
        )
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
        .filter_map(|(entry_id, label, container_kind)| {
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

    #[test]
    fn capability_probe_is_not_a_policy_management_operation() {
        assert!(!is_vault_management_verb("svc.vault.capabilities"));
        assert!(is_vault_management_verb("svc.vault.get_policy"));
        assert!(is_vault_management_verb("svc.vault.get_status"));
        assert!(is_vault_management_verb("svc.vault.apply_policy"));
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
    async fn vault_reconcile_access_groups_with_forced_privileged_reaches_handler() {
        // `run_one_request` always sends `{}` as args, which is not a valid
        // `VaultReconcileAccessGroupsRequest` — so this still errors, but
        // with a DIFFERENT kind than "forbidden". That difference is exactly
        // what proves a privileged caller cleared the gate and reached
        // `handle_vault_reconcile_access_groups` rather than being denied by
        // `authorize`/`is_vault_management_verb`.
        let reply = run_one_request(
            "vault-reconcile-priv",
            "svc.vault.reconcile_access_groups",
            true,
            false,
        )
        .await;
        match reply {
            Envelope::Error(ErrorReply { kind, .. }) => {
                assert_ne!(
                    kind, "forbidden",
                    "a privileged caller must pass the gate and reach the handler"
                );
            }
            Envelope::Response(_) => {}
            other => panic!("unexpected reply: {:?}", other),
        }
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
