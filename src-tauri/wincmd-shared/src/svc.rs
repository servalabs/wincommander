// SPDX-License-Identifier: AGPL-3.0-or-later
//! UI ↔ SYSTEM-service RPC namespace (`svc.*`).
//!
//! This module defines the shared contract between the desktop UI (Tauri /
//! commander-free) and the Windows system service (`commander-svc`).  Both
//! peers communicate over a **separate** named pipe ([`SVC_PIPE_NAME`]) using
//! the same length-prefixed JSON framing and [`crate::Envelope`] types already
//! defined in `lib.rs`; only the pipe name and the feature-id namespace differ.
//!
//! ## Protocol overview
//!
//! 1. The UI dials `SVC_PIPE_NAME` and sends an `Envelope::Hello` containing
//!    [`SVC_PROTOCOL_VERSION`] and a fresh random session token.
//! 2. The service echoes a `Hello` ack and begins accepting `Envelope::Request`
//!    frames; responses come back as `Envelope::Response` / `Envelope::Error`.
//! 3. All frames after the handshake are wrapped in `Envelope::Signed` using
//!    the session token as the HMAC key (same `sign` / `verify_and_unwrap`
//!    helpers from `lib.rs`).
//!
//! ## Feature-id (`svc.*`) namespace
//!
//! Every `Request::feature_id` in this namespace is prefixed `svc.`.  The
//! service enforces a capability-class split on the peer Windows SID:
//!
//! - **[`CapabilityClass::ReadOnly`]** — queries and status checks that carry
//!   no privilege risk.  The service allows these from any authenticated peer
//!   (interactive user session or restricted helper).
//! - **[`CapabilityClass::Privileged`]** — mutations, dispatches, and anything
//!   unknown.  The service requires the peer SID to be in the admin or
//!   LocalSystem group before honouring these.  Fail-closed: an unrecognised
//!   verb is treated as `Privileged` so a future verb that forgets to register
//!   itself is never accidentally served as read-only.
//!
//! [`classify_verb`] is the **SSOT** for that split — both the service-side
//! gate and any UI-side pre-checks must call it rather than reimplementing the
//! list.

/// Named-pipe path for the UI ↔ system-service channel.
/// Distinct from any Pro sidecar pipe; the service listens on this name and
/// every UI client dials it.
pub const SVC_PIPE_NAME: &str = r"\\.\pipe\wincmd-svc";

/// Magic string in the `Hello` frame so either side detects a version mismatch
/// before attempting to parse feature-specific payloads.  Bumped on
/// incompatible wire-format changes only (not on every new `svc.*` verb).
pub const SVC_PROTOCOL_VERSION: &str = "wincmd-svc-v1";

/// Capability class of a `svc.*` verb — drives the service-side peer-SID gate.
///
/// The service enforces this at the pipe connection layer after verifying the
/// peer's Windows SID (via `GetNamedPipeClientProcessId` + token query).
/// The UI can call [`classify_verb`] before sending to decide whether to show
/// a UAC/admin prompt, but the service always re-checks regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CapabilityClass {
    /// Mutations, dispatches, and any unrecognised verb.  Fail-closed: unknown
    /// verbs default here so they are never accidentally served read-only.
    Privileged,
    /// Read-only queries (status, settings reads, ping, health).  No state is
    /// mutated and no privileged Windows API is invoked.
    ReadOnly,
}

/// Classify a `svc.*` feature_id into its [`CapabilityClass`].
///
/// The classification is **fail-closed**: any verb not in the explicit
/// read-only allow-list is returned as [`CapabilityClass::Privileged`].  This
/// means a newly-added verb that mutates state is safe by default even if the
/// author forgets to add it here — it will be gated until the service
/// explicitly lists it as read-only.
///
/// # Examples
///
/// ```
/// use wincmd_shared::svc::{classify_verb, CapabilityClass};
/// assert_eq!(classify_verb("svc.ping"),         CapabilityClass::ReadOnly);
/// assert_eq!(classify_verb("svc.dispatch"),     CapabilityClass::Privileged);
/// assert_eq!(classify_verb("svc.unknown_verb"), CapabilityClass::Privileged);
/// ```
pub fn classify_verb(feature_id: &str) -> CapabilityClass {
    // Explicit read-only allow-list.  Everything not on this list is Privileged.
    match feature_id {
        "svc.status" => CapabilityClass::ReadOnly,
        "svc.get_settings" => CapabilityClass::ReadOnly,
        "svc.ping" => CapabilityClass::ReadOnly,
        "svc.health" => CapabilityClass::ReadOnly,
        // All other verbs — including mutations, dispatches, fleet toggles, and
        // any future verb not yet added to the read-only list — are Privileged.
        _ => CapabilityClass::Privileged,
    }
}

/// Build the `Hello` the UI sends when dialling the service pipe.
/// Mirrors `crate::hello_from_free` but uses [`SVC_PROTOCOL_VERSION`].
pub fn hello_from_ui(session_token: impl Into<String>) -> crate::Hello {
    crate::Hello {
        protocol_version: SVC_PROTOCOL_VERSION.to_string(),
        session_token: session_token.into(),
        binary_hash: None,
        free_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        pro_version: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_verb: read-only verbs ───────────────────────────────────────

    #[test]
    fn read_only_verbs_are_classified_read_only() {
        for verb in &["svc.status", "svc.get_settings", "svc.ping", "svc.health"] {
            assert_eq!(
                classify_verb(verb),
                CapabilityClass::ReadOnly,
                "expected ReadOnly for {:?}",
                verb
            );
        }
    }

    // ── classify_verb: privileged verbs ─────────────────────────────────────

    #[test]
    fn privileged_verbs_are_classified_privileged() {
        for verb in &[
            "svc.patch_settings",
            "svc.dispatch",
            "svc.set_fleet_enabled",
        ] {
            assert_eq!(
                classify_verb(verb),
                CapabilityClass::Privileged,
                "expected Privileged for {:?}",
                verb
            );
        }
    }

    // ── classify_verb: unknown verbs — fail-closed ───────────────────────────

    #[test]
    fn unknown_verbs_default_to_privileged_fail_closed() {
        for verb in &["svc.nuke", "garbage", "", "svc.", "totally_unknown"] {
            assert_eq!(
                classify_verb(verb),
                CapabilityClass::Privileged,
                "expected Privileged (fail-closed) for {:?}",
                verb
            );
        }
    }
}
