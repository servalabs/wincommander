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
//! service enforces a capability-class split on the peer identity.  There are
//! **three** classes, in increasing order of trust required from the peer:
//!
//! - **[`CapabilityClass::ReadOnly`]** — queries and status checks that carry
//!   no privilege risk.  The service allows these from any authenticated peer
//!   (interactive user session or restricted helper).
//! - **[`CapabilityClass::SessionHelper`]** — a named action performed by a
//!   specific, pinned per-user helper process that is *not* an admin:
//!   submitting an already-locally-generated event/receipt, or installing a
//!   policy epoch that was signature-verified earlier in the chain.  Strictly
//!   stronger than `ReadOnly` (an arbitrary session peer must not be able to
//!   call these — that would let any process forge receipts or push policy)
//!   and strictly weaker than `Privileged` (no admin/LocalSystem token is
//!   required, and this class must never be treated as implying admin
//!   rights).  This module only *classifies* a verb into this bucket; it is
//!   the service's responsibility — not this crate's — to additionally
//!   confirm the connected peer is (a) a process in the current interactive
//!   session and (b) an approved helper filename directly inside the
//!   administrator/SYSTEM-protected service installation directory, and to
//!   apply service-side rate limiting on top. Authenticode is deliberately
//!   not required, so unsigned and self-signed Free builds remain functional.
//!   Anything the service persists on behalf of a `SessionHelper` caller
//!   should carry a trust-origin marker so a forged-submission investigation
//!   is possible later.
//! - **[`CapabilityClass::Privileged`]** — mutations, dispatches, and anything
//!   unknown.  The service requires the peer SID to be in the admin or
//!   LocalSystem group before honouring these.  Fail-closed: an unrecognised
//!   verb is treated as `Privileged` so a future verb that forgets to
//!   register itself is never accidentally served as read-only *or* as a
//!   session helper.  Adding the `SessionHelper` variant does not weaken this
//!   guarantee: a verb is only ever `ReadOnly` or `SessionHelper` if it is
//!   named explicitly in one of [`classify_verb`]'s match arms — every other
//!   string, known or not, falls through to the wildcard arm and is
//!   `Privileged`.
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

/// Privileged RPC for the small, typed machine-setting allow-list.  This is
/// deliberately not a generic "apply tweak" endpoint: callers cannot supply
/// registry paths, shell commands, firewall rules, ACLs, or arbitrary JSON.
pub const APPLY_MACHINE_SETTING_VERB: &str = "svc.apply.machine_setting";

/// A machine-owned Windows setting the service may change.
///
/// This enum is the allow-list. Adding a setting requires an explicit shared
/// contract change plus a service-side implementation and read-back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineSettingId {
    RdpIncoming,
    RdpLock,
}

/// Typed desired value for one [`MachineSettingId`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineSettingValue {
    /// Enables/disables incoming RDP and sets its idle timeout in seconds.
    RdpIncoming {
        enabled: bool,
        idle_timeout_seconds: u32,
    },
    /// Adds/removes the service-owned inbound TCP/3389 block rule.
    RdpLock { locked: bool },
}

/// The only accepted payload for [`APPLY_MACHINE_SETTING_VERB`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyMachineSettingRequest {
    pub setting: MachineSettingId,
    pub value: MachineSettingValue,
}

impl ApplyMachineSettingRequest {
    /// Reject a mismatched setting/value pair and values outside the bounded
    /// RDP policy range before any Windows command is attempted.
    pub fn validate(&self) -> Result<(), &'static str> {
        match (self.setting, &self.value) {
            (
                MachineSettingId::RdpIncoming,
                MachineSettingValue::RdpIncoming {
                    enabled: true,
                    idle_timeout_seconds,
                },
            ) if !(10..=86_400).contains(idle_timeout_seconds) => {
                Err("RDP incoming idle timeout must be between 10 and 86400 seconds")
            }
            (MachineSettingId::RdpIncoming, MachineSettingValue::RdpIncoming { .. })
            | (MachineSettingId::RdpLock, MachineSettingValue::RdpLock { .. }) => Ok(()),
            _ => Err("machine setting identifier and value kind do not match"),
        }
    }
}

/// Read-back result from Windows after a machine-setting request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineSettingObserved {
    RdpIncoming {
        enabled: bool,
        deny_connections: Option<bool>,
        idle_timeout_seconds: Option<u32>,
        max_idle_time_ms: Option<u32>,
        max_disconnection_time_ms: Option<u32>,
        max_connection_time_ms: Option<u32>,
        reset_broken: Option<bool>,
    },
    RdpLock {
        locked: bool,
    },
}

/// Capability class of a `svc.*` verb — drives the service-side peer gate.
///
/// The service enforces this at the pipe connection layer after establishing
/// the peer's identity (Windows SID via `GetNamedPipeClientProcessId` + token
/// query for `Privileged`; session membership + protected-path pinning for
/// `SessionHelper`).  The UI can call [`classify_verb`] before sending to
/// decide whether to show a UAC/admin prompt, but the service always
/// re-checks regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CapabilityClass {
    /// Mutations, dispatches, and any unrecognised verb.  Fail-closed: unknown
    /// verbs default here so they are never accidentally served read-only or
    /// as a session helper.  Requires the peer to be admin or LocalSystem.
    Privileged,
    /// A named, non-admin action performed by a specific pinned per-user
    /// helper process — e.g. submitting a locally-generated event/receipt, or
    /// installing a policy epoch that was already signature-verified
    /// upstream.
    ///
    /// Strictly stronger than [`CapabilityClass::ReadOnly`] (an arbitrary
    /// session peer must not be able to call these) and strictly weaker than
    /// [`CapabilityClass::Privileged`] (no admin/LocalSystem token is
    /// required — and this class must never be treated as implying admin
    /// rights).  This variant only records *which bucket a verb falls into*;
    /// confirming that the connected peer is actually (a) in the current
    /// interactive session and (b) the pinned helper binary, plus rate
    /// limiting submissions, is the service's job, not this module's — see
    /// the module-level docs for the full obligation list.
    SessionHelper,
    /// Read-only queries (status, settings reads, ping, health).  No state is
    /// mutated and no privileged Windows API is invoked.
    ReadOnly,
}

impl CapabilityClass {
    /// Ordinal strength of this class, strictly increasing with the trust the
    /// service demands of the peer: `ReadOnly` (0) < `SessionHelper` (1) <
    /// `Privileged` (2).
    ///
    /// A pure ordering helper for the service side — e.g. to assert that a
    /// peer's confirmed identity class dominates a verb's required class
    /// (`peer_class.rank() >= classify_verb(verb).rank()`) without writing a
    /// fresh match arm per comparison.  It does not perform, and is not a
    /// substitute for, the peer-identity checks (session membership, binary
    /// pinning, SID/token checks) that remain the service's responsibility —
    /// this module only classifies verbs, it never authenticates peers.
    pub fn rank(self) -> u8 {
        match self {
            CapabilityClass::ReadOnly => 0,
            CapabilityClass::SessionHelper => 1,
            CapabilityClass::Privileged => 2,
        }
    }
}

/// Classify a `svc.*` feature_id into its [`CapabilityClass`].
///
/// The classification is **fail-closed**: any verb not in the explicit
/// `ReadOnly` / `SessionHelper` allow-lists is returned as
/// [`CapabilityClass::Privileged`].  This means a newly-added verb that
/// mutates state is safe by default even if the author forgets to add it
/// here — it will be gated until the service explicitly lists it as
/// read-only or session-helper.
///
/// # Examples
///
/// ```
/// use wincmd_shared::svc::{classify_verb, CapabilityClass};
/// assert_eq!(classify_verb("svc.ping"),         CapabilityClass::ReadOnly);
/// assert_eq!(classify_verb("svc.clipboard.report_event"), CapabilityClass::SessionHelper);
/// assert_eq!(classify_verb("svc.dispatch"),     CapabilityClass::Privileged);
/// assert_eq!(classify_verb("svc.unknown_verb"), CapabilityClass::Privileged);
/// ```
pub fn classify_verb(feature_id: &str) -> CapabilityClass {
    match feature_id {
        // ── Existing read-only verbs (unchanged) ──────────────────────────
        "svc.status" => CapabilityClass::ReadOnly,
        "svc.get_settings" => CapabilityClass::ReadOnly,
        "svc.ping" => CapabilityClass::ReadOnly,
        "svc.health" => CapabilityClass::ReadOnly,

        // ── Clipboard Guard (D-2 / plan §4.3) ─────────────────────────────
        // The resolved ruleset is already observable by triggering it, so
        // reading it back carries no additional privilege risk.
        "svc.clipboard.get_policy" => CapabilityClass::ReadOnly,
        // Submitted by the per-user clipboard helper, which is never admin.
        // Must not be ReadOnly (any process in the session could forge
        // receipts) and must not be Privileged (the helper can't satisfy
        // that). SessionHelper is the third stance D-2 introduces for
        // exactly this shape of caller.
        "svc.clipboard.report_event" => CapabilityClass::SessionHelper,
        // Toggling enforcement is an admin decision. Listed explicitly
        // (rather than left to the wildcard arm) so it reads alongside the
        // rest of the clipboard-guard verb table.
        "svc.clipboard.set_enabled" => CapabilityClass::Privileged,

        // ── Policy epoch install, svc-side (D-2 / plan §4.3, caller 3) ────
        // Pro/Free hands svc an already signature-verified epoch. Neither
        // Pro nor Free runs as admin/LocalSystem, so this hop hits the same
        // fail-closed wall as `report_event` above and gets the same
        // SessionHelper stance. svc must still independently re-verify the
        // signature and enforce the monotonic-version guard before swapping
        // the active ruleset — SessionHelper only says "a pinned, in-session
        // peer may call this verb", not "trust its payload".
        "svc.policy.install_epoch" => CapabilityClass::SessionHelper,

        // ── Ink Receipt (D-2 / plan §4.3, §5) ─────────────────────────────
        "svc.ink_receipt.get_policy" => CapabilityClass::ReadOnly,
        // Ticket reservation and receipt submission both originate from the
        // Ink Receipt bridge running as the interactive user, not an admin.
        "svc.ink_receipt.reserve_ticket" => CapabilityClass::SessionHelper,
        "svc.ink_receipt.report_receipt" => CapabilityClass::SessionHelper,
        "svc.ink_receipt.status" => CapabilityClass::ReadOnly,

        // Vault mount/unmount is user-initiated rather than administrator-
        // initiated.  It is safe here only because the service derives the
        // connected process token and re-checks policy membership itself; the
        // client cannot provide a path, SID, ACL, or presentation decision.
        "svc.vault.authorize_mount"
        | "svc.vault.mount"
        | "svc.vault.create_personal"
        | "svc.vault.unmount"
        | "svc.vault.list_authorized"
        | "svc.vault.capabilities" => CapabilityClass::ReadOnly,

        // Creates/updates a Windows local group and sets its EXACT
        // membership from admin-supplied SIDs (the Access control UI's
        // group editor). Unlike the vault mount verbs above — which only
        // ever act on facts the service itself derives from the caller's
        // token — this mutates real Windows local-group state from
        // caller-supplied SIDs, so it stays on the same fail-closed
        // Privileged (SYSTEM/Admin only) footing as `svc.vault.apply_policy`
        // rather than being added to the ReadOnly list above. Listed
        // explicitly (the wildcard arm below would already classify it
        // Privileged) so it reads alongside the rest of the vault verb
        // table.
        "svc.vault.reconcile_access_groups" => CapabilityClass::Privileged,

        // All other verbs — including mutations, dispatches, fleet toggles,
        // and any future verb not yet added above — are Privileged.
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
        for verb in &[
            "svc.status",
            "svc.get_settings",
            "svc.ping",
            "svc.health",
            "svc.vault.capabilities",
            "svc.vault.list_authorized",
        ] {
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
            "svc.vault.reconcile_access_groups",
            APPLY_MACHINE_SETTING_VERB,
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
        for verb in &[
            "svc.nuke",
            "garbage",
            "",
            "svc.",
            "totally_unknown",
            // Near-misses of the SessionHelper verb names added below (D-2):
            // adding the SessionHelper variant must not make matching looser
            // — anything not spelled exactly right still falls through to
            // the wildcard arm and stays Privileged.
            "svc.clipboard.",
            "svc.clipboard.report_events",
            "svc.policy.install",
            "svc.policy.install_epoch.x",
        ] {
            assert_eq!(
                classify_verb(verb),
                CapabilityClass::Privileged,
                "expected Privileged (fail-closed) for {:?}",
                verb
            );
        }
    }

    #[test]
    fn machine_setting_contract_rejects_mismatched_or_unbounded_values() {
        assert!(
            ApplyMachineSettingRequest {
                setting: MachineSettingId::RdpIncoming,
                value: MachineSettingValue::RdpIncoming {
                    enabled: true,
                    idle_timeout_seconds: 900,
                },
            }
            .validate()
            .is_ok()
        );
        assert!(
            ApplyMachineSettingRequest {
                setting: MachineSettingId::RdpIncoming,
                value: MachineSettingValue::RdpIncoming {
                    enabled: true,
                    idle_timeout_seconds: 9,
                },
            }
            .validate()
            .is_err()
        );
        assert!(
            ApplyMachineSettingRequest {
                setting: MachineSettingId::RdpLock,
                value: MachineSettingValue::RdpIncoming {
                    enabled: true,
                    idle_timeout_seconds: 900,
                },
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn machine_setting_wire_rejects_raw_execution_fields() {
        let raw = serde_json::json!({
            "setting": "rdp_lock",
            "value": { "kind": "rdp_lock", "locked": true, "command": "netsh ..." },
            "registry_path": "HKLM\\Software"
        });
        assert!(serde_json::from_value::<ApplyMachineSettingRequest>(raw).is_err());
    }

    // ── classify_verb: Clipboard Guard verbs (D-2 / plan §4.3) ───────────────

    #[test]
    fn clipboard_guard_verbs_classify_per_table() {
        let cases = [
            ("svc.clipboard.get_policy", CapabilityClass::ReadOnly),
            ("svc.clipboard.report_event", CapabilityClass::SessionHelper),
            ("svc.clipboard.set_enabled", CapabilityClass::Privileged),
        ];
        for (verb, expected) in cases {
            assert_eq!(classify_verb(verb), expected, "verb {:?}", verb);
        }
    }

    // ── classify_verb: policy epoch install (D-2 / plan §4.3, caller 3) ─────

    #[test]
    fn policy_install_epoch_is_session_helper() {
        assert_eq!(
            classify_verb("svc.policy.install_epoch"),
            CapabilityClass::SessionHelper
        );
    }

    // ── classify_verb: Ink Receipt verbs (D-2 / plan §5) ─────────────────────

    #[test]
    fn ink_receipt_verbs_classify_per_table() {
        let cases = [
            ("svc.ink_receipt.get_policy", CapabilityClass::ReadOnly),
            (
                "svc.ink_receipt.reserve_ticket",
                CapabilityClass::SessionHelper,
            ),
            (
                "svc.ink_receipt.report_receipt",
                CapabilityClass::SessionHelper,
            ),
            ("svc.ink_receipt.status", CapabilityClass::ReadOnly),
        ];
        for (verb, expected) in cases {
            assert_eq!(classify_verb(verb), expected, "verb {:?}", verb);
        }
    }

    // ── CapabilityClass: SessionHelper is distinct from both other classes ──

    #[test]
    fn session_helper_is_distinct_from_read_only_and_privileged() {
        assert_ne!(CapabilityClass::SessionHelper, CapabilityClass::ReadOnly);
        assert_ne!(CapabilityClass::SessionHelper, CapabilityClass::Privileged);
    }

    // ── CapabilityClass::rank: strictly increasing with required trust ─────

    #[test]
    fn rank_is_strictly_ordered_read_only_lt_session_helper_lt_privileged() {
        assert!(CapabilityClass::ReadOnly.rank() < CapabilityClass::SessionHelper.rank());
        assert!(CapabilityClass::SessionHelper.rank() < CapabilityClass::Privileged.rank());
    }
}
