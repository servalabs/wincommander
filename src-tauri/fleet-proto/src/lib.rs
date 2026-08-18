// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fleet wire types — shared by the fleet server (`commander-pro/fleet-server`)
//! and the desktop Fleet admin panel (`commander` frontend, generated via
//! ts-rs). These are pure data shapes: the protocol's security comes from
//! Ed25519 signatures + RBAC, not from keeping the definitions private.
//!
//! Invariant: NO AV-flagged command strings appear here. Remote commands
//! reference a catalog id (`catalog_id: String`), never a recombined literal.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Re-exported from `wincmd-clip-rules`, the clipboard-guard rule engine's
/// SSOT for policy types (plan §4.1). This crate deliberately does NOT
/// redefine `Severity`/`Action`/`Rule`/`MatchKind`/etc. — `NON-GOALS.md`
/// forbids duplicating a shared wire type, and `wincmd-clip-rules` is
/// already the one place both the fleet console (rule editor + test panel,
/// plan §4.5) and the endpoint (the actual clipboard matcher) compile it
/// from, so a rule that validates in the console behaves identically
/// on-device. Re-exporting here gives every consumer of THIS crate —
/// fleet-server, the admin panel's `ts-codegen` output, and (through
/// `wincmd_shared::fleet`'s existing blanket re-export) Free/Pro — ONE
/// import path (`fleet_proto::{Rule, Action, Severity, ...}`) instead of a
/// second, independent dependency edge on `wincmd-clip-rules` that could
/// drift out of lockstep with this crate's own re-export.
pub use wincmd_clip_rules::{
    compile, truncate_for_match, Action, BuiltinPattern, CompileError, CompiledRuleSet,
    CooldownLedger, Emit, MatchKind, Rule, RuleId, RuleIdError, RuleSetLimits, Severity,
    StructuredKind, Verdict,
};

/// Tenant identifier. `"local"` for single-tenant self-host installs; real
/// org slugs once multi-tenancy is enabled.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct OrgId(pub String);

/// Device identifier — the agent's `settings.device_id` (a UUID) carried as a
/// string on the wire so this crate stays dependency-light (no `uuid`). The
/// fleet server parses it to a real UUID at the storage boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceId(pub String);

/// Risk tier of a remote action — drives the safety-gate ladder in the fleet
/// server (Milestone 3) and tells the admin panel which confirmation UI to
/// render. Contains no command strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    /// Reversible, no data loss (status reads, benign config).
    Safe,
    /// Reversible but disruptive (service restart, network reconfig).
    Destructive,
    /// Cannot be undone (lockdown, erase, dismount) — top of the gate ladder.
    Irreversible,
}

impl ActionClass {
    /// The wire string used in command preimages and DB storage.
    // KT: SSOT for ActionClass → string mapping. Drive this from serde's
    // snake_case representation so it stays in sync with the JSON wire format.
    // All prior hand-written copies in store/mod.rs and fleet_push.rs are deleted.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            ActionClass::Safe => "safe",
            ActionClass::Destructive => "destructive",
            ActionClass::Irreversible => "irreversible",
        }
    }
}

/// Catalog ids for OPERATOR-signed control commands — verified against the
/// org's operator command key, NEVER the server signing key. Every catalog id
/// NOT in this list is a server-signed ordinary command. This is the SSOT for
/// the dual-key command-verification routing in `fleet-agent-core` and the
/// operator-command issuance gate in `fleet-server` (whose constant retains
/// this historical name and carries the `ActionClass` mapping). A server test
/// pins its ids equal to this list. Not feature-gated — the verifier needs it
/// without `command-metadata`.
pub const DURESS_CATALOG_IDS: &[&str] = &[
    "duress_seal",
    "raise_posture",
    "duress_wipe",
    "all_clear_revoke",
    "duress_unseal",
    "all_clear",
    "rotate_key",
    "unenroll",
    "suspend_deadman",
    "set_deadman_policy",
    "set_posture_policy",
];

/// Whether `catalog_id` is an operator-signed control command (see
/// [`DURESS_CATALOG_IDS`]). The dual-key verifier routes these commands to the
/// pinned OPERATOR key and every other command to the pinned SERVER signing
/// key, so the server can't forge an operator command (it lacks the operator
/// key) and an operator-signed blob can't masquerade as an ordinary command.
pub fn is_duress_catalog(catalog_id: &str) -> bool {
    DURESS_CATALOG_IDS.contains(&catalog_id)
}

/// Discoverable, AV-safe catalog metadata for one remote command. Mirrors an
/// entry in commander-pro's `fleet_dispatch::COMMAND_CATALOG` but deliberately
/// excludes `feature_id` (the AV-flagged Pro handler name) — only
/// `catalog_id` + `action_class` + a human-safe `summary` + an optional
/// hand-authored `payload_schema` cross the wire to integrators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CommandMeta {
    pub catalog_id: &'static str,
    pub action_class: ActionClass,
    pub summary: &'static str,
    /// Compact JSON-Schema-ish literal string, or `None` for parameterless
    /// commands. Kept as a JSON *string* (not a parsed `Value`) so this
    /// struct — and the `COMMAND_METADATA` table below — stays a `const`.
    pub payload_schema: Option<&'static str>,
}

/// Catalog metadata SSOT, feature-gated so it can never compile into the
/// AV-scanned Free binary (commander-free does not, and must not, depend on
/// this feature). Exactly one entry per `commander-pro::fleet_dispatch::COMMAND_CATALOG`
/// entry, same `catalog_id` + `action_class` — cross-checked by the
/// `fleet_dispatch` drift-guard test. `summary` is written from the
/// `catalog_id`'s meaning (never copied/paraphrased from `feature_id`) and is
/// AV-token-free.
#[cfg(feature = "command-metadata")]
pub const COMMAND_METADATA: &[CommandMeta] = &[
    // ── Safe: status reads + protection-restoring / harmless config ──────────
    CommandMeta {
        catalog_id: "defender.enable",
        action_class: ActionClass::Safe,
        summary: "Enable real-time malware protection",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "usb.storage.lockdown.status",
        action_class: ActionClass::Safe,
        summary: "Read whether USB storage lockdown is active",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "rdp.keepalive.status",
        action_class: ActionClass::Safe,
        summary: "Read whether Remote Desktop access is currently allowed",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "rdp.unlock",
        action_class: ActionClass::Safe,
        summary: "Restore Remote Desktop access",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "encryption.status",
        action_class: ActionClass::Safe,
        summary: "Read the status of encrypted volumes",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "encryption.system.status",
        action_class: ActionClass::Safe,
        summary: "Read whether the system drive is encrypted",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "activation.status",
        action_class: ActionClass::Safe,
        summary: "Read the license activation status",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "mesh.status",
        action_class: ActionClass::Safe,
        summary: "Read the mesh VPN connection status",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "screen_capture.status",
        action_class: ActionClass::Safe,
        summary: "Read whether screen-capture monitoring is running",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "driver.watch.status",
        action_class: ActionClass::Safe,
        summary: "Read whether driver-change monitoring is running",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "print_audit.status",
        action_class: ActionClass::Safe,
        summary: "Read whether print-job auditing is running",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "endpoint.security_snapshot",
        action_class: ActionClass::Safe,
        summary: "Collect bounded processes, listening ports, and services for device triage",
        payload_schema: Some(r#"{"type":"object","properties":{},"additionalProperties":false}"#),
    },
    CommandMeta {
        catalog_id: "velociraptor.collect.client_info",
        action_class: ActionClass::Safe,
        summary: "Collect a bounded Velociraptor client information summary",
        payload_schema: Some(r#"{"type":"object","properties":{},"additionalProperties":false}"#),
    },
    CommandMeta {
        catalog_id: "dns.cache.clear",
        action_class: ActionClass::Safe,
        summary: "Clear the local DNS resolver cache",
        payload_schema: None,
    },
    // Cross-device file search (fan-out): each device runs a name-only filename
    // walk and returns matching names/paths. Safe = the taxonomy classifies by
    // reversibility/data-loss (this mutates nothing), NOT sensitivity — that is
    // handled out-of-band by the server fan-out cap + audit + RBAC.
    CommandMeta {
        catalog_id: "files.search",
        action_class: ActionClass::Safe,
        summary: "Search a device's files by name and return matching names and paths",
        payload_schema: Some(
            r#"{"type":"object","properties":{"terms":{"type":"string"},"scope":{"type":"array","items":{"type":"string"}},"mode":{"type":"string","enum":["keyword","semantic","forensic"]},"predicates":{"type":"object","properties":{"ext_in":{"type":"array","items":{"type":"string"}},"size_min":{"type":"integer"},"size_max":{"type":"integer"},"modified_after":{"type":"integer"},"modified_before":{"type":"integer"},"path_exclude":{"type":"array","items":{"type":"string"}}}},"rank":{"type":"string","enum":["ModifiedDesc","SizeDesc","TraversalOrder"]}},"required":["terms","mode"]}"#,
        ),
    },
    // Per-device recent-downloads listing — metadata only (name/path/size/
    // mtime), same Safe rationale as files.search above.
    CommandMeta {
        catalog_id: "files.recent_downloads",
        action_class: ActionClass::Safe,
        summary: "List a device's recently downloaded files (name/size/date, no content)",
        payload_schema: None,
    },
    // Remote file-CONTENT fetch. Safe by the reversibility taxonomy (reads
    // only), but the most sensitive capability in the catalog — gated
    // server-side at Admin role + an org-level opt-in
    // (OrgSettings.file_content_fetch_enabled, default off), on top of the
    // normal Safe/Operator floor every other entry here gets.
    CommandMeta {
        catalog_id: "files.fetch_content",
        action_class: ActionClass::Safe,
        summary: "Fetch a specific file's content from a device (path must come from a prior search/listing)",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"},"transfer_token":{"type":"string"}},"required":["path","transfer_token"]}"#,
        ),
    },
    // ── File operations (browse + manage a device's files) ───────────────────
    // Classes follow the repo's reversibility taxonomy, NOT sensitivity: Safe
    // reads only; Destructive is disruptive-but-reversible (recycle is
    // Destructive precisely because the Recycle Bin keeps it restorable);
    // Irreversible loses data (permanent delete / overwrite-shred / content
    // overwrite). Sensitivity is gated out-of-band server-side (role floor +
    // audit), the same "Safe = reversibility, not sensitivity" split
    // files.search / files.fetch_content already draw. Every path is
    // server-disclosed (files.search / files.recent_downloads / files.list_dir)
    // before any mutation can name it.
    CommandMeta {
        catalog_id: "files.list_dir",
        action_class: ActionClass::Safe,
        summary: "List a directory's entries (names, sizes, dates — no content)",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.stat",
        action_class: ActionClass::Safe,
        summary: "Read a file or folder's size, timestamps, and attributes",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.hash",
        action_class: ActionClass::Safe,
        summary: "Compute a file's SHA-256 hash",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"},"algo":{"type":"string","enum":["sha256"]}},"required":["path","algo"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.mkdir",
        action_class: ActionClass::Destructive,
        summary: "Create a new directory",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.rename",
        action_class: ActionClass::Destructive,
        summary: "Rename a file or folder in place",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"},"new_name":{"type":"string"}},"required":["path","new_name"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.move",
        action_class: ActionClass::Destructive,
        summary: "Move a file or folder into another directory",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"},"dest_dir":{"type":"string"}},"required":["path","dest_dir"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.copy",
        action_class: ActionClass::Destructive,
        summary: "Copy a file or folder into another directory",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"},"dest_dir":{"type":"string"}},"required":["path","dest_dir"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.recycle",
        action_class: ActionClass::Destructive,
        summary: "Send a file or folder to the Recycle Bin",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.delete_permanent",
        action_class: ActionClass::Irreversible,
        summary: "Permanently delete a file or folder (no Recycle Bin)",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.shred",
        action_class: ActionClass::Irreversible,
        summary: "Overwrite a file's data, then delete it so it cannot be recovered",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"},"passes":{"type":"integer","minimum":1}},"required":["path"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "files.write_content",
        action_class: ActionClass::Irreversible,
        summary: "Overwrite a file's contents with uploaded data",
        payload_schema: Some(
            r#"{"type":"object","properties":{"path":{"type":"string"},"transfer_token":{"type":"string"}},"required":["path","transfer_token"]}"#,
        ),
    },
    // ── Destructive: reversible but disruptive / security-reducing ───────────
    CommandMeta {
        catalog_id: "defender.disable",
        action_class: ActionClass::Destructive,
        summary: "Disable real-time malware protection",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "usb.writeprotect.enable",
        action_class: ActionClass::Destructive,
        summary: "Make removable USB storage read-only",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "usb.writeprotect.disable",
        action_class: ActionClass::Destructive,
        summary: "Allow writes to removable USB storage again",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "usb.storage.lockdown.enable",
        action_class: ActionClass::Destructive,
        summary: "Block removable USB storage devices",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "usb.storage.lockdown.disable",
        action_class: ActionClass::Destructive,
        summary: "Allow removable USB storage devices again",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "capability.set",
        action_class: ActionClass::Destructive,
        summary: "Allow or deny an app-capability (camera, microphone, location, etc.)",
        payload_schema: Some(
            r#"{"type":"object","properties":{"Capability":{"type":"string","enum":["webcam","microphone","location","contacts","appointments","phoneCall","phoneCallHistory","chat","userNotificationListener","documentsLibrary","picturesLibrary","videosLibrary","broadFileSystemAccess","gazeInput","appDiagnostics","userAccountInformation","bluetoothSync"]},"Access":{"type":"string","enum":["Allow","Deny"]}},"required":["Capability","Access"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "bitlocker.autoencrypt.enable",
        action_class: ActionClass::Destructive,
        summary: "Turn on automatic drive encryption",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "rdp.session.disconnect_all",
        action_class: ActionClass::Destructive,
        summary: "Disconnect all active Remote Desktop sessions",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "rdp.lock",
        action_class: ActionClass::Destructive,
        summary: "Lock down Remote Desktop access",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "session.lock",
        action_class: ActionClass::Destructive,
        summary: "Lock the local Windows session (return to the lock screen)",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "mesh.service.start",
        action_class: ActionClass::Destructive,
        summary: "Start the mesh VPN service",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "mesh.service.stop",
        action_class: ActionClass::Destructive,
        summary: "Stop the mesh VPN service",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "vault.dismount_all",
        action_class: ActionClass::Destructive,
        summary: "Dismount all encrypted volumes",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "vault.clear_keys",
        action_class: ActionClass::Destructive,
        summary: "Clear cached encryption keys and history",
        payload_schema: None,
    },
    // ── Monitoring start / stop (safe to start, destructive to stop) ────────
    CommandMeta {
        catalog_id: "monitoring.remote_access.start",
        action_class: ActionClass::Safe,
        summary: "Start remote-access monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.remote_access.stop",
        action_class: ActionClass::Destructive,
        summary: "Stop remote-access monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.screen_capture.start",
        action_class: ActionClass::Safe,
        summary: "Start screen-capture monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.screen_capture.stop",
        action_class: ActionClass::Destructive,
        summary: "Stop screen-capture monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.ransomware.start",
        action_class: ActionClass::Safe,
        summary: "Start ransomware activity monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.ransomware.stop",
        action_class: ActionClass::Destructive,
        summary: "Stop ransomware activity monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.auth_anomaly.start",
        action_class: ActionClass::Safe,
        summary: "Start sign-in anomaly monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.auth_anomaly.stop",
        action_class: ActionClass::Destructive,
        summary: "Stop sign-in anomaly monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.wifi_guard.start",
        action_class: ActionClass::Safe,
        summary: "Start Wi-Fi network safety monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.wifi_guard.stop",
        action_class: ActionClass::Destructive,
        summary: "Stop Wi-Fi network safety monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.driver_watch.start",
        action_class: ActionClass::Safe,
        summary: "Start driver-change monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "monitoring.driver_watch.stop",
        action_class: ActionClass::Destructive,
        summary: "Stop driver-change monitoring",
        payload_schema: None,
    },
    // ── Ink Receipt — controlled-PDF print-workflow bridge (plan §5.3, D-1) ──
    // Mirrors commander-pro::fleet_dispatch::COMMAND_CATALOG's own comment:
    // status/recent reads and starting enforcement are Safe; stopping
    // enforcement is Destructive because it is security-reducing (same
    // reasoning as every monitoring.*.stop entry above).
    CommandMeta {
        catalog_id: "ink_receipt.status",
        action_class: ActionClass::Safe,
        summary: "Read whether Ink Receipt enforcement is running",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "ink_receipt.enforce",
        action_class: ActionClass::Safe,
        summary: "Start Ink Receipt enforcement",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "ink_receipt.disable",
        action_class: ActionClass::Destructive,
        summary: "Stop Ink Receipt enforcement",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "ink_receipt.receipts.recent",
        action_class: ActionClass::Safe,
        summary: "Read recent Ink Receipt bridge activity (content-free)",
        payload_schema: None,
    },
    // ── Fleet P4: special actions + tripwire arming ─────────────────────────
    CommandMeta {
        catalog_id: "tripwire.honeypot.arm",
        action_class: ActionClass::Destructive,
        summary: "Arm the network honeypot tripwire",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "tripwire.honeypot.disarm",
        action_class: ActionClass::Destructive,
        summary: "Disarm the network honeypot tripwire",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "tripwire.canary.arm",
        action_class: ActionClass::Destructive,
        summary: "Arm the canary token listener",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "tripwire.canary.disarm",
        action_class: ActionClass::Destructive,
        summary: "Disarm the canary token listener",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "tripwire.tamper.arm",
        action_class: ActionClass::Destructive,
        summary: "Arm tamper-detection monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "tripwire.tamper.disarm",
        action_class: ActionClass::Destructive,
        summary: "Disarm tamper-detection monitoring",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "tripwire.autoerase.set",
        action_class: ActionClass::Irreversible,
        summary: "Schedule an automatic data-erase trigger",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "tripwire.autoerase.remove",
        action_class: ActionClass::Destructive,
        summary: "Remove a scheduled automatic data-erase trigger",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "network.protocol.block",
        action_class: ActionClass::Destructive,
        summary: "Block a network protocol",
        payload_schema: Some(
            r#"{"type":"object","properties":{"Protocol":{"type":"string"}},"required":["Protocol"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "network.protocol.unblock",
        action_class: ActionClass::Destructive,
        summary: "Unblock a network protocol",
        payload_schema: Some(
            r#"{"type":"object","properties":{"Protocol":{"type":"string"}},"required":["Protocol"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "cleanup.browser_footprints",
        action_class: ActionClass::Irreversible,
        summary: "Erase browser history and cached traces",
        payload_schema: None,
    },
    // ── Irreversible: data destruction / cannot be undone (top of the ladder) ─
    CommandMeta {
        catalog_id: "cleanup.shadow_copies",
        action_class: ActionClass::Irreversible,
        summary: "Remove volume shadow copies",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "cleanup.event_logs",
        action_class: ActionClass::Irreversible,
        summary: "Erase Windows event logs",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "cleanup.prefetch",
        action_class: ActionClass::Irreversible,
        summary: "Erase Windows prefetch data",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "cleanup.master_privacy_clean",
        action_class: ActionClass::Irreversible,
        summary: "Run the full privacy-cleaning sequence",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "cleanup.secure_shutdown",
        action_class: ActionClass::Irreversible,
        summary: "Shut down after purging RAM-remnant traces (hiberfil.sys, pagefile, swapfile.sys)",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "contingency.start",
        action_class: ActionClass::Irreversible,
        summary: "Begin the contingency (emergency data-protection) sequence",
        payload_schema: None,
    },
    CommandMeta {
        catalog_id: "cleanup.shred_folders",
        action_class: ActionClass::Irreversible,
        summary: "Securely erase the specified folders (single-pass shred)",
        payload_schema: Some(
            r#"{"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}}},"required":["paths"]}"#,
        ),
    },
    CommandMeta {
        catalog_id: "lockdown.full",
        action_class: ActionClass::Irreversible,
        summary: "Full lockdown: dismount encrypted volumes, erase specified folders and privacy traces, then shut down",
        payload_schema: Some(
            r#"{"type":"object","properties":{"shredPaths":{"type":"array","items":{"type":"string"}},"clearTraces":{"type":"boolean"}}}"#,
        ),
    },
    CommandMeta {
        catalog_id: "policy.reapply",
        action_class: ActionClass::Safe,
        summary: "Re-check and re-apply the latest policy immediately",
        payload_schema: None,
    },
    // ── Privacy toggles: 21 on/off pairs. enable = apply protection (Safe),
    // disable = remove it (Destructive → confirm gate). Must stay 1:1 with
    // commander-pro's COMMAND_CATALOG (a drift-guard test enforces it).
    CommandMeta { catalog_id: "privacy.telemetry.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Block Telemetry", payload_schema: None },
    CommandMeta { catalog_id: "privacy.telemetry.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Block Telemetry", payload_schema: None },
    CommandMeta { catalog_id: "privacy.clipboardHistory.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Clipboard History", payload_schema: None },
    CommandMeta { catalog_id: "privacy.clipboardHistory.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Clipboard History", payload_schema: None },
    CommandMeta { catalog_id: "privacy.cloudClipboard.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Cloud Clipboard Sync", payload_schema: None },
    CommandMeta { catalog_id: "privacy.cloudClipboard.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Cloud Clipboard Sync", payload_schema: None },
    CommandMeta { catalog_id: "privacy.recentFiles.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Recent Files Tracking (Start Menu / Explorer MRU)", payload_schema: None },
    CommandMeta { catalog_id: "privacy.recentFiles.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Recent Files Tracking (Start Menu / Explorer MRU)", payload_schema: None },
    CommandMeta { catalog_id: "privacy.jumpLists.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Jump Lists", payload_schema: None },
    CommandMeta { catalog_id: "privacy.jumpLists.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Jump Lists", payload_schema: None },
    CommandMeta { catalog_id: "privacy.thumbnailCache.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Thumbnail Cache", payload_schema: None },
    CommandMeta { catalog_id: "privacy.thumbnailCache.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Thumbnail Cache", payload_schema: None },
    CommandMeta { catalog_id: "privacy.activity.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Activity History", payload_schema: None },
    CommandMeta { catalog_id: "privacy.activity.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Activity History", payload_schema: None },
    CommandMeta { catalog_id: "privacy.location.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Location Tracking", payload_schema: None },
    CommandMeta { catalog_id: "privacy.location.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Location Tracking", payload_schema: None },
    CommandMeta { catalog_id: "privacy.suggestions.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Windows Suggestions", payload_schema: None },
    CommandMeta { catalog_id: "privacy.suggestions.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Windows Suggestions", payload_schema: None },
    CommandMeta { catalog_id: "privacy.lockScreenPrivacy.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Lock Screen Privacy", payload_schema: None },
    CommandMeta { catalog_id: "privacy.lockScreenPrivacy.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Lock Screen Privacy", payload_schema: None },
    CommandMeta { catalog_id: "privacy.recallSnapshots.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Recall Snapshots", payload_schema: None },
    CommandMeta { catalog_id: "privacy.recallSnapshots.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Recall Snapshots", payload_schema: None },
    CommandMeta { catalog_id: "privacy.typingInsights.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Typing Insights", payload_schema: None },
    CommandMeta { catalog_id: "privacy.typingInsights.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Typing Insights", payload_schema: None },
    CommandMeta { catalog_id: "privacy.internetComm.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Restrict Internet Comms", payload_schema: None },
    CommandMeta { catalog_id: "privacy.internetComm.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Restrict Internet Comms", payload_schema: None },
    CommandMeta { catalog_id: "privacy.advertisingId.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Nuke Advertising ID", payload_schema: None },
    CommandMeta { catalog_id: "privacy.advertisingId.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Nuke Advertising ID", payload_schema: None },
    CommandMeta { catalog_id: "privacy.tailoredExp.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Tailored Experiences", payload_schema: None },
    CommandMeta { catalog_id: "privacy.tailoredExp.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Tailored Experiences", payload_schema: None },
    CommandMeta { catalog_id: "privacy.officeLog.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Office Logging", payload_schema: None },
    CommandMeta { catalog_id: "privacy.officeLog.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Office Logging", payload_schema: None },
    CommandMeta { catalog_id: "privacy.diagTracing.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Diagnostic Tracing", payload_schema: None },
    CommandMeta { catalog_id: "privacy.diagTracing.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Diagnostic Tracing", payload_schema: None },
    CommandMeta { catalog_id: "privacy.hideQuickAccessRecent.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Hide Recent Files in Quick Access", payload_schema: None },
    CommandMeta { catalog_id: "privacy.hideQuickAccessRecent.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Hide Recent Files in Quick Access", payload_schema: None },
    CommandMeta { catalog_id: "privacy.hideQuickAccessFrequent.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Hide Frequent Folders in Quick Access", payload_schema: None },
    CommandMeta { catalog_id: "privacy.hideQuickAccessFrequent.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Hide Frequent Folders in Quick Access", payload_schema: None },
    CommandMeta { catalog_id: "privacy.hideRunMRU.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Don't Save Run Dialog History", payload_schema: None },
    CommandMeta { catalog_id: "privacy.hideRunMRU.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Don't Save Run Dialog History", payload_schema: None },
    CommandMeta { catalog_id: "privacy.disableSearchHistory.enable", action_class: ActionClass::Safe, summary: "Apply privacy protection: Disable Search Box History", payload_schema: None },
    CommandMeta { catalog_id: "privacy.disableSearchHistory.disable", action_class: ActionClass::Destructive, summary: "Remove privacy protection: Disable Search Box History", payload_schema: None },
    // Ink Receipt and Clipboard Guard are delivered through signed org settings.
    // Do not advertise endpoint commands until matching, audited handlers exist.
];

/// A signed, versioned configuration snapshot. Append-only and monotonic per
/// org; agents reject any epoch whose `version` is <= the one they hold
/// (anti-rollback). `config_json` is `AppSettings`-shaped policy.
///
/// Fleet Control Plane P2 adds **targeting** + **locks**: an epoch is scoped to
/// a target (org / group / device) and carries the settings paths the device
/// must not let the local user change (`locked_paths`). The signature binds ALL
/// of these via [`epoch_signing_envelope`] so a device cannot replay another
/// scope's policy (target spoofing) or strip the locks (downgrade).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ConfigEpoch {
    pub org_id: OrgId,
    pub version: i64,
    pub config_json: Value,
    /// Scope this epoch applies to: "org" | "group" | "device". Defaults to
    /// "org" for epochs published before targeting existed.
    #[serde(default = "default_target_kind")]
    pub target_kind: String,
    /// Target id within the kind (group_id / device_id). `None` for org-wide.
    #[serde(default)]
    pub target_id: Option<String>,
    /// Settings dot-paths the device must keep at the published value — locked
    /// against local edits while the device is fleet-managed.
    #[serde(default)]
    pub locked_paths: Vec<String>,
    /// P5 enrollment lock: when true the device should refuse a local unenroll
    /// without admin approval. Carried here (signed) so it can't be spoofed.
    #[serde(default)]
    pub managed: bool,
    /// Base64 Ed25519 signature over [`canonical_epoch_bytes`]`(version,
    /// epoch_signing_envelope(config_json, locked_paths, managed, target))`.
    pub signature: String,
    /// Base64 of the public key that produced `signature` (lets agents pin the
    /// fleet signing key and detect rotation).
    pub signer_key: String,
}

fn default_target_kind() -> String {
    "org".to_string()
}

/// Admin request to publish new policy. The server assigns the next monotonic
/// version and signs it; the admin never supplies version or signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ConfigPushRequest {
    pub config_json: Value,
    /// Scope: "org" (default) | "group" | "device".
    #[serde(default = "default_target_kind")]
    pub target_kind: String,
    /// group_id / device_id for a scoped push; ignored for "org".
    #[serde(default)]
    pub target_id: Option<String>,
    /// Settings dot-paths to lock on the targeted devices.
    #[serde(default)]
    pub locked_paths: Vec<String>,
    /// Mark the targeted devices enrollment-locked (P5).
    #[serde(default)]
    pub managed: bool,
}

/// Lifecycle of a remote command. `pending` awaits multi-party approval;
/// `approved` is signed and pollable; `dispatched` was handed to the agent;
/// `acked`/`failed` are terminal results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Pending,
    Approved,
    Dispatched,
    Acked,
    Failed,
    Rejected,
    Expired,
}

/// Explicit lifecycle state of an enrolled device. `pending` is reserved for a
/// future approve-before-active enrollment flow; devices enrolled today start
/// `active`. `suspended` is reversible (temporary hold — no re-enroll needed to
/// lift it); `revoked` and `decommissioned` are terminal (all credential
/// material is invalidated; the device must re-enroll to rejoin, subject to the
/// blocklist). A `suspended` or `revoked` device is denied config resolution and
/// command polling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum DeviceLifecycle {
    Pending,
    Active,
    Suspended,
    Revoked,
    Decommissioned,
}

/// Admin request to create a remote command. `catalog_id` references a
/// COMMAND_CATALOG entry — NEVER a raw command string. `confirmation_token` is
/// required for irreversible actions (obtained from the confirm-token endpoint).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CommandRequest {
    pub device_id: DeviceId,
    pub catalog_id: String,
    pub action_class: ActionClass,
    #[serde(default)]
    pub payload: Value,
    pub idempotency_key: String,
    pub confirmation_token: Option<String>,
}

/// Admin-facing view of a command and its gate state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CommandView {
    pub command_id: String,
    pub device_id: DeviceId,
    pub catalog_id: String,
    pub action_class: ActionClass,
    pub status: CommandStatus,
    pub approvals: i32,
    pub required_approvals: i32,
    pub requested_by: String,
    pub created_at: String,
    /// Command payload — included so the admin panel can reconstruct current
    /// per-capability state from command history without a separate endpoint.
    #[serde(default)]
    pub payload: Value,
}

/// What an agent receives when it polls — the signed, executable envelope. The
/// agent verifies `signature` against `signer_key` and checks `epoch_version`
/// (anti-rollback) before executing the `catalog_id` action.
///
/// # `idempotency_key` — the actual signed id (P0 fix)
///
/// The operator (offline signing tool `sign-duress-command.rs`) signs
/// `canonical_command_bytes(command_id = idempotency_key, …)` because the
/// server-assigned UUID does not exist yet at signing time. `command_id` on
/// this struct is the server-assigned UUID (delivery/dedup/ack id) — it is
/// NOT what was signed. Every verifier (fleet-agent-core, Android) MUST
/// rebuild the preimage using `idempotency_key`, never `command_id`, or
/// signature verification silently fails and the command (including
/// `duress_wipe`) never executes. See `canonical_command_bytes`'s doc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct SignedCommand {
    pub command_id: String,
    /// The stable id the signer actually signed over (operator-signed
    /// commands: the offline tool's `--idempotency-key`; server-signed
    /// commands: the same `idempotency_key` the caller supplied at creation —
    /// see `finalize_if_ready`). Verifiers must use THIS field, not
    /// `command_id`, when rebuilding `canonical_command_bytes`.
    pub idempotency_key: String,
    pub device_id: DeviceId,
    pub catalog_id: String,
    pub action_class: ActionClass,
    pub payload: Value,
    pub epoch_version: i64,
    pub signature: String,
    pub signer_key: String,
}

/// Request a single-use confirmation token for an irreversible action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ConfirmTokenRequest {
    pub device_id: DeviceId,
    pub catalog_id: String,
}

/// Issued confirmation token (single-use, short-lived, bound to device+catalog).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ConfirmTokenResponse {
    pub token: String,
    pub expires_at: String,
}

// ── File-search seam (reserved, not yet wired) ──────────────────────────────
// Reserved seam for future remote file-search; carried in a command payload +
// returned via the command result endpoint. Not yet wired to any catalog entry
// or handler.

/// Search backend a remote file-search request should use. Open/extensible —
/// new variants may be added as backends are built (Keyword ships first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Keyword,
    Semantic,
    Forensic,
}

/// Result predicates evaluated ON THE DEVICE, during the walk and *before* the
/// row cap.
///
/// This exists because the cap truncates, it does not rank: `fleet_search_walk`
/// returns the moment it reaches `FLEET_SEARCH_MAX_ROWS` in depth-first
/// traversal order. Filtering those rows in the console therefore filters an
/// arbitrary sample — a search for spreadsheets could return none while the
/// device holds hundreds, because the cap was spent on rows the operator was
/// about to discard. Pushing the predicate down makes the capped set 200
/// *relevant* rows.
///
/// Every field is `#[serde(default)]`: the whole struct is optional in a signed
/// payload, and an omitted predicate means "no constraint", never "match
/// nothing".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct SearchPredicates {
    /// Lowercase extensions WITHOUT the leading dot (`["xlsx", "csv"]`).
    /// Empty ⇒ any extension.
    #[serde(default)]
    pub ext_in: Vec<String>,
    /// Inclusive size bounds in bytes.
    #[serde(default)]
    pub size_min: Option<u64>,
    #[serde(default)]
    pub size_max: Option<u64>,
    /// Inclusive mtime bounds, Unix seconds.
    #[serde(default)]
    pub modified_after: Option<u64>,
    #[serde(default)]
    pub modified_before: Option<u64>,
    /// Case-insensitive path substrings to skip (e.g. `node_modules`). Applied
    /// to the full path, and used to prune directory recursion where possible.
    #[serde(default)]
    pub path_exclude: Vec<String>,
}

/// Which rows survive the cap when more match than it allows. `TraversalOrder`
/// is the pre-predicate behaviour, kept so an explicit request can still get it;
/// `ModifiedDesc` is the default because "what changed most recently" is the
/// question an operator is nearly always actually asking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub enum SearchRank {
    #[default]
    ModifiedDesc,
    SizeDesc,
    TraversalOrder,
}

/// Reserved seam for future remote file-search; carried in a command payload +
/// returned via the command result endpoint. Not yet wired to any catalog entry
/// or handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct RemoteSearchRequest {
    pub terms: String,
    /// Roots to walk. Omitted/empty ⇒ the device uses a bounded default scope
    /// (the common user-profile dirs). `#[serde(default)]` so the payload can
    /// omit it and the JSON-Schema `required` stays `[terms, mode]`.
    #[serde(default)]
    pub scope: Vec<String>,
    pub mode: SearchMode,
    /// Device-side filters applied before the row cap. `#[serde(default)]` keeps
    /// the JSON-Schema `required` at `[terms, mode]`, so an older console that
    /// omits this is unaffected.
    #[serde(default)]
    pub predicates: SearchPredicates,
    #[serde(default)]
    pub rank: SearchRank,
    /// Maximum result rows the device may return. Fleet derives this from the
    /// organization setting before signing the command; callers cannot select
    /// a larger limit through the search request itself.
    #[serde(default = "default_remote_search_result_limit")]
    pub result_limit: usize,
}

fn default_remote_search_result_limit() -> usize {
    200
}

/// Cross-device file search — one result row. Wired to `FILES_SEARCH_CATALOG_ID`
/// and commander-pro's `fleet_file_search` handler; dispatched via
/// `routes::search`'s fan-out endpoint. Name/path/size/mtime metadata only —
/// never content (see `NON-GOALS.md`'s "aggregate scalars, not raw filenames"
/// boundary — that boundary is about PASSIVE telemetry; this is an explicit,
/// audited, admin-triggered pull, the same distinction `files.recent_downloads`
/// below draws).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct RemoteSearchResultRow {
    pub doc_id: String,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub mtime: u64,
    pub size: u64,
    pub score: f32,
    pub match_kind: String,
    pub snippet: String,
}

/// Cross-device file search result, returned via the command result endpoint
/// (§3.4) / the search fan-out aggregation route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct RemoteSearchResult {
    pub command_id: String,
    pub rows: Vec<RemoteSearchResultRow>,
    pub index_status: Option<Value>,
    /// Whether this device actually evaluated `RemoteSearchRequest::predicates`.
    ///
    /// Serde ignores unknown fields, so an agent older than the predicate
    /// contract accepts a filtered search and answers with UNFILTERED rows. The
    /// console would then render an arbitrary capped sample as though it were
    /// the filtered answer — a wrong result presented as a right one. Agents
    /// that honour predicates set this true; `#[serde(default)]` makes every
    /// older ack read false, so the console can say "this device ignored your
    /// filters" instead of quietly lying.
    ///
    /// False is also correct for a request that carried no predicates at all;
    /// the console only surfaces the warning when it actually sent some.
    #[serde(default)]
    pub predicates_applied: bool,
}

/// Catalog id for the cross-device file-search command (`ActionClass::Safe`).
/// Wired into both `COMMAND_METADATA` (above) and commander-pro's
/// `COMMAND_CATALOG`; the fleet-server fan-out endpoint and the console use this
/// constant so all three sides stay on the same literal (the drift guard pins
/// `catalog_id` + `action_class` across proto and the Pro catalog).
pub const FILES_SEARCH_CATALOG_ID: &str = "files.search";

/// One entry in a device's Downloads folder, returned by the
/// `files.recent_downloads` command. Metadata only (name/path/size/mtime) —
/// same disclosure boundary as `RemoteSearchResultRow`: an explicit,
/// audited, admin-triggered listing, never passive/automatic collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct RecentDownloadEntry {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    /// RFC3339 last-modified time.
    pub modified_at: String,
}

/// `files.recent_downloads` command result — newest-first, capped at a
/// device-side limit (see the handler).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct RecentDownloadsResult {
    pub entries: Vec<RecentDownloadEntry>,
}

/// Catalog id for the per-device recent-downloads listing command
/// (`ActionClass::Safe`). Same wiring contract as `FILES_SEARCH_CATALOG_ID`.
pub const FILES_RECENT_DOWNLOADS_CATALOG_ID: &str = "files.recent_downloads";

/// Catalog id for the bounded, on-demand endpoint triage snapshot.
pub const ENDPOINT_SECURITY_SNAPSHOT_CATALOG_ID: &str = "endpoint.security_snapshot";

/// Maximum rows returned for each security-snapshot domain. The endpoint must
/// stop collecting at this cap; the server re-validates it before persistence.
pub const SECURITY_SNAPSHOT_MAX_ROWS_PER_DOMAIN: usize = 200;
/// Maximum UTF-8 bytes accepted for the complete security-snapshot result.
pub const SECURITY_SNAPSHOT_MAX_RESULT_BYTES: usize = 256 * 1024;
/// Maximum UTF-8 bytes accepted for an individual snapshot text field.
pub const SECURITY_SNAPSHOT_MAX_TEXT_BYTES: usize = 1024;

/// Transport protocol for a listening socket. Snapshot v1 intentionally
/// excludes connection history and remote peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum SecuritySnapshotProtocol {
    Tcp,
    Udp,
}

/// One bounded process row from `endpoint.security_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct SecuritySnapshotProcess {
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One bounded listening-port row from `endpoint.security_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct SecuritySnapshotListeningPort {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub protocol: SecuritySnapshotProtocol,
    pub local_address: String,
    pub local_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_path: Option<String>,
}

/// One bounded Windows service row from `endpoint.security_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct SecuritySnapshotService {
    pub name: String,
    pub start_type: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Typed result for `endpoint.security_snapshot`. Version 1 contains exactly
/// processes, listening ports, and services; it deliberately excludes files,
/// scheduled tasks, startup items, hashes, and connection history. `truncated`
/// is true whenever an endpoint reached a collection bound, so an incomplete
/// result must never be rendered as a full inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(deny_unknown_fields)]
pub struct SecuritySnapshotResult {
    pub processes: Vec<SecuritySnapshotProcess>,
    pub listening_ports: Vec<SecuritySnapshotListeningPort>,
    pub services: Vec<SecuritySnapshotService>,
    pub truncated: bool,
}

/// Validate the fixed, bounded v1 snapshot result before it crosses a process
/// or persistence boundary. This is shared by the endpoint and server.
pub fn validate_security_snapshot_result(
    result: &SecuritySnapshotResult,
) -> Result<(), &'static str> {
    if result.processes.len() > SECURITY_SNAPSHOT_MAX_ROWS_PER_DOMAIN
        || result.listening_ports.len() > SECURITY_SNAPSHOT_MAX_ROWS_PER_DOMAIN
        || result.services.len() > SECURITY_SNAPSHOT_MAX_ROWS_PER_DOMAIN
    {
        return Err("security snapshot row limit exceeded");
    }

    let text_is_valid =
        |text: &str| !text.is_empty() && text.len() <= SECURITY_SNAPSHOT_MAX_TEXT_BYTES;
    let optional_text_is_valid = |text: &Option<String>| text.as_deref().is_none_or(text_is_valid);

    if result
        .processes
        .iter()
        .any(|row| !text_is_valid(&row.name) || !optional_text_is_valid(&row.path))
    {
        return Err("security snapshot process field is invalid");
    }
    if result.listening_ports.iter().any(|row| {
        !text_is_valid(&row.local_address)
            || !optional_text_is_valid(&row.process_name)
            || !optional_text_is_valid(&row.process_path)
    }) {
        return Err("security snapshot listening-port field is invalid");
    }
    if result.services.iter().any(|row| {
        !text_is_valid(&row.name)
            || !text_is_valid(&row.start_type)
            || !text_is_valid(&row.status)
            || !optional_text_is_valid(&row.path)
    }) {
        return Err("security snapshot service field is invalid");
    }

    let bytes = serde_json::to_vec(result).map_err(|_| "security snapshot cannot serialize")?;
    if bytes.len() > SECURITY_SNAPSHOT_MAX_RESULT_BYTES {
        return Err("security snapshot byte limit exceeded");
    }
    Ok(())
}

/// `files.fetch_content` command payload. `path` must be echoed from a prior
/// `files.search`/`files.recent_downloads` result for the SAME device — never
/// admin-typed free text (closes off an arbitrary-path read/enumeration
/// primitive). `transfer_token` is minted server-side, injected into the
/// payload BEFORE the command is signed (so it travels under the same
/// signature as `path`, no second unsigned side-channel) — the admin never
/// supplies it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct FetchContentPayload {
    pub path: String,
    pub transfer_token: String,
}

/// Catalog id for the remote file-content-fetch command (`ActionClass::Safe`
/// — it mutates nothing on the device; sensitivity is handled out-of-band by
/// the server's Admin-role gate + `OrgSettings.file_content_fetch_enabled`
/// opt-in + full audit, the same "Safe = reversibility, not sensitivity"
/// split `files.search` already draws).
pub const FILES_FETCH_CONTENT_CATALOG_ID: &str = "files.fetch_content";

// ── File-operations catalog result types ─────────────────────────────────────
// Result shapes for the "file operations" fleet feature (files.list_dir / stat /
// hash / mkdir / rename / move / copy / recycle / delete_permanent / shred /
// write_content). commander-pro's `fleet_files` handlers build these; the
// console renders them. Every result carries at least `{ok, path}` (+ `detail`
// when `ok` is false); the reads (list_dir / stat / hash) add their own fields.
// `path` echoes the (server-disclosed) request path for correlation.
//
// write_content REUSES `FetchContentPayload` above ({path, transfer_token}) —
// the payload shape is identical; only the byte direction (console → device) is
// reversed.

/// One row of a `files.list_dir` result — metadata only, never content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    /// RFC3339 last-modified time, or `None` if unavailable.
    pub modified_at: Option<String>,
}

/// `files.list_dir` result — a bounded directory listing (see the handler's
/// entry cap). `entries` is empty when `ok` is false.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ListDirResult {
    pub ok: bool,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub entries: Vec<FileEntry>,
}

/// `files.stat` result — size / timestamps / attributes for one path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct FileStatResult {
    pub ok: bool,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub is_dir: bool,
    pub size_bytes: u64,
    /// RFC3339 last-modified time, or `None` if unavailable.
    pub modified_at: Option<String>,
    /// RFC3339 creation time, or `None` if the platform doesn't record it.
    pub created_at: Option<String>,
    /// Windows file-attribute bitmask (0 on non-Windows agents).
    pub attributes: u32,
}

/// `files.hash` result — a content digest (currently SHA-256 only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct HashResult {
    pub ok: bool,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub algo: String,
    pub hex: String,
    pub size_bytes: u64,
}

/// Result of a `files.*` MUTATION (mkdir / rename / move / copy / recycle /
/// delete_permanent / shred / write_content). `ok` is ALWAYS derived from a
/// post-op re-stat of the effect on the device — never hardcoded — so a
/// denied / locked / still-present target reports `ok:false` with `detail`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct FileOpResult {
    pub ok: bool,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Aggregate productivity sample sent by an agent. Carries NO raw titles, URLs,
/// or keystrokes — only scalars + category scores (raw signals stay AES-GCM
/// encrypted on the device). Ingest is rejected unless `consent_version` matches
/// `disclosure_version` (the monitored user's consent must match the active
/// disclosure). Disclosed/consent-gated — never coupled to covert features.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ProductivitySample {
    pub window_start: String,
    pub window_end: String,
    pub active_seconds: i32,
    pub idle_seconds: i32,
    /// Aggregate category → score map, e.g. `{"productive": 0.8}`. Aggregate only.
    pub category_scores: Value,
    pub consent_version: i32,
    pub disclosure_version: i32,
}

/// Per-device productivity rollup for the admin dashboard.
// `Eq` is intentionally NOT derived: `category_totals` is a `serde_json::Value`
// (holds floats), which is `PartialEq` but not `Eq` — matching `ProductivitySample`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceProductivity {
    pub device_id: DeviceId,
    pub hostname: Option<String>,
    pub active_seconds: i64,
    pub idle_seconds: i64,
    pub sample_count: i64,
    pub last_window_end: Option<String>,
    /// Per-category weighted totals folded from each sample's `category_scores`
    /// (each score weighted by that sample's `active_seconds`), e.g.
    /// `{"productive": 135.0, "distracting": 20.0}`. Aggregate scalars only — no
    /// PII. Empty object when no samples carried category scores.
    pub category_totals: Value,
}

/// Typed failure surface shared with the admin panel so the UI can branch on
/// the kind rather than parsing error strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum FleetError {
    NotFound,
    /// A config epoch (or command) was rejected as older than the current one.
    StaleEpoch,
    BadSignature,
    /// Malformed input (bad timestamps, negative counts, consent mismatch).
    BadRequest,
    /// A uniqueness constraint was violated (duplicate version / idempotency key).
    Conflict,
    /// The request lacked valid credentials (401).
    Unauthorized,
    /// Valid credentials but insufficient privileges for this action (403).
    Forbidden,
    /// Too many failed attempts — temporarily locked out (login throttle).
    RateLimited,
    Internal,
}

/// One audit-log entry as returned to the admin panel. Shared so the server and
/// the panel agree on the shape (SSOT) — RFC 3339 `at`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct AuditEntry {
    pub actor: String,
    pub action: String,
    pub target: Option<String>,
    pub outcome: String,
    pub detail: Value,
    pub at: String,
}

/// Latest framework-authoritative Android status exposed to fleet operators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct AndroidFleetStatus {
    pub framework_key_fingerprint: String,
    pub deadman_ladder_step: i32,
    pub authoritative_last_contact_age_seconds: i64,
    pub audit_ledger_count: i64,
    pub audit_ledger_head_hash: String,
    pub reported_at: String,
}

/// A device as shown in the admin panel's fleet view. `online` is computed by
/// the server from `last_seen` against a freshness window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceSummary {
    pub device_id: DeviceId,
    pub device_hash: String,
    /// Enrolled agent class (`wincommander`, `tuxcommander`, or `android`).
    /// Additive/defaulted so older cached API responses still deserialize.
    #[serde(default)]
    pub device_kind: String,
    pub hostname: Option<String>,
    pub os_version: Option<String>,
    pub agent_version: String,
    pub enrolled_at: String,
    pub last_seen_at: Option<String>,
    pub online: bool,
    /// Group this device is assigned to, if any (F4 device groups).
    #[serde(default)]
    pub group_id: Option<String>,
    /// Latest live resource sample (F-metrics), if the device has ever
    /// reported one. `None` for a device on an agent build predating this
    /// field, or one that simply hasn't checked in yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<DeviceResourceSample>,
    /// Framework-authoritative Android fleet state from the latest check-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android_fleet_status: Option<AndroidFleetStatus>,
}

/// A device group — a named bucket of devices within an org (F4). Membership is
/// 1:1 (a device is in at most one group) and stored separately from `devices`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceGroup {
    pub group_id: String,
    pub org_id: OrgId,
    pub name: String,
}

/// Create a device group (Admin+).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CreateGroupRequest {
    pub name: String,
}

/// Assign (or, with `None`, clear) a device's group (Admin+).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct AssignGroupRequest {
    pub group_id: Option<String>,
}

/// Argus security-signal sample sent by an agent. Carries NO filenames, paths,
/// URLs, printer names, document names, or usernames — only kind + class +
/// magnitude + severity + a disclosure-notice version. `consent_version` is
/// vestigial (hardcoded `1`, never read) — there is no consent gate; ingest is
/// never refused for a disclosure_version mismatch, only recorded as one.
/// kind ∈ {"dlp_exfil","tamper","print","removable_media"}.
/// class = sub-type label (e.g. "usb_large_transfer", "log_cleared").
/// magnitude = aggregate scalar (bytes / pages / count).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ArgusSignal {
    pub window_start: String,
    pub window_end: String,
    /// High-level category: "dlp_exfil" | "tamper" | "print" | "removable_media" | "tripwire" | "access"
    pub kind: String,
    /// Sub-type label within the kind (no PII — e.g. "usb_large_transfer").
    pub class: String,
    /// Aggregate scalar for this window (bytes transferred, pages printed, count).
    pub magnitude: i64,
    /// "info" | "warn" | "critical"
    pub severity: String,
    pub consent_version: i32,
    pub disclosure_version: i32,
    /// Stable, device-minted idempotency key for a retried signal. Older
    /// agents omit it, in which case the server preserves legacy insertion.
    #[serde(default)]
    pub event_id: Option<String>,
}

/// Per-(kind) breakdown within a device's Argus rollup — a severity-aware,
/// unit-consistent bucket. Every signal in a bucket shares one `kind`, so their
/// `magnitude` is commensurable (bytes with bytes, pages with pages) — unlike
/// the flat `total_magnitude`, which mixes incommensurable units. Aggregate
/// scalars only — no filenames, paths, URLs, printer/document names, or
/// usernames (the Argus privacy invariant).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ArgusKindBucket {
    /// High-level category: "dlp_exfil" | "tamper" | "print" | "removable_media" | "tripwire" | "access".
    pub kind: String,
    /// Single sub-type label shared by every signal in the bucket, or `None`
    /// when the kind spans multiple classes (mixed → collapse to kind level).
    pub class: Option<String>,
    /// Number of signal records in this bucket.
    pub count: i64,
    /// Sum of magnitude within this single-kind bucket (commensurable units).
    pub magnitude: i64,
    /// Highest severity seen, ordered info < warn < critical.
    pub max_severity: String,
}

/// Per-device Argus signal rollup for the admin dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceArgusSummary {
    pub device_id: DeviceId,
    pub hostname: Option<String>,
    /// Total number of signal records for this device in the window.
    pub signal_count: i64,
    /// Sum of magnitude across all signals. NOTE: mixes incommensurable units
    /// (bytes + pages + counts) — kept for back-compat; prefer `by_kind` and
    /// `risk_score` for a meaningful read.
    pub total_magnitude: i64,
    /// RFC 3339 timestamp of the most recent signal window end.
    pub last_window_end: Option<String>,
    /// Per-kind severity-aware breakdown (additive). Sorted by `kind`. Each
    /// bucket aggregates one commensurable signal family so magnitudes never mix.
    pub by_kind: Vec<ArgusKindBucket>,
    /// Normalised insider-risk score (additive). Severity-weighted signal
    /// volume, unit-independent — see `store::argus_risk_score` for the model.
    pub risk_score: i64,
}

/// Per-device Argus monitoring-coverage view for the admin (Feature B —
/// transparency). Tells the admin whether Argus is actually observing a device,
/// under which disclosure version, whether disclosure is in good standing (no
/// recent disclosure-mismatch ingest denial in the audit log), and which signal
/// kinds have been seen. Aggregate/opaque scalars only — no PII.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceArgusCoverage {
    pub device_id: DeviceId,
    pub hostname: Option<String>,
    /// True when ≥1 Argus signal was ingested for this device within the window.
    pub monitoring_active: bool,
    /// RFC 3339 window_end of the most recent ingested signal, else `None`.
    pub last_signal_at: Option<String>,
    /// Disclosure version of the most recent ingested signal, else `None`.
    /// (Ingested signals always satisfy consent_version == disclosure_version.)
    pub disclosure_version: Option<i32>,
    /// Distinct signal kinds observed for this device in the window, sorted asc.
    pub kinds_observed: Vec<String>,
    /// Number of signal records ingested for this device in the window.
    pub signal_count: i64,
    /// False when a disclosure-mismatch ingest denial was recorded for this
    /// device in the audit log within the window.
    pub disclosure_in_good_standing: bool,
    /// RFC 3339 timestamp of the most recent disclosure-mismatch denial, else `None`.
    pub last_disclosure_mismatch_at: Option<String>,
}

/// One raw Argus signal record in a device's drill-down window (Feature C). The
/// per-signal row behind the aggregated `DeviceArgusSummary` — still PII-free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ArgusSignalDetail {
    pub window_start: String,
    pub window_end: String,
    pub kind: String,
    pub class: String,
    pub magnitude: i64,
    pub severity: String,
    pub consent_version: i32,
    pub disclosure_version: i32,
}

/// Agent → server applied-posture report (F6/F7). The agent reports the config
/// epoch it currently holds + a hash of its applied settings, so the server can
/// compute drift (behind the latest epoch) and compliance. No PII.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct PostureReport {
    pub applied_epoch: i64,
    #[serde(default)]
    pub settings_hash: String,
    /// Optional PII-free map of `settings-dot-path → applied value` (booleans /
    /// scalars only — never names/paths/URLs). When present the server diffs it
    /// against the resolved epoch's desired config to surface PER-TOGGLE drift
    /// (P3). Absent (`null`) → drift is computed at epoch granularity only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle_states: Option<Value>,
    /// Whether the Privacy Shield is ACTUALLY running on the device right
    /// now, self-reported at posture time. Compared against the resolved
    /// `ShieldDesiredState` (a SEPARATE, non-epoch-versioned channel — see
    /// that type's doc) to raise real shield drift, independent of
    /// `applied_epoch`/`toggle_states`. `None` on any pre-shield-drift agent
    /// build, or when the agent has no shield status to report this cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shield_running: Option<bool>,
}

/// Privacy Shield's admin-desired on/off + mode, resolved device > group > org
/// (same precedence as `ConfigEpoch`) but stored and versioned SEPARATELY from
/// the policy `config_epochs` chain. Toggling the shield from the console
/// must NEVER advance `ConfigEpoch.version` — that chain is for actual policy
/// edits, and every version bump makes every OTHER device look momentarily
/// "behind" in `routes::posture::drift` (which compares against the org's
/// single highest version regardless of target). Delivered to the agent on
/// the same fast `/v1/agents/checkin` round-trip as `config_epoch`, via
/// `CheckinResponse.shield_state`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ShieldDesiredState {
    pub enabled: bool,
    /// `"blur_notify"` (blur/black-out the screen AND notify) | `"notify_only"`
    /// (notification only, no visual blur). Mutually exclusive — see the
    /// Privacy Shield card's segmented toggle.
    pub mode: String,
    /// RFC3339, set by the server when this desired state was last written.
    /// Informational only (no anti-rollback gate — latest write always wins,
    /// unlike `ConfigEpoch`).
    pub updated_at: String,
}

/// Agent → server report of ONE local Windows notification the agent already
/// showed the user (screen-capture-tool detected, CPU/RAM/network threshold
/// exceeded, ...), forwarded to the fleet console only when the corresponding
/// per-type `notifications.<type>.reportToFleet` setting is on (agent-side
/// gate — the server stores whatever it is sent). Carries the SAME concrete
/// detail the local toast already showed, so the console can render a
/// specific message instead of a generic "alert" line. PII-free: `detail`
/// must contain only scalars/process-executable-names/metric values, never
/// window titles, file contents, or free-text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct LocalAlertReport {
    /// `"screen_capture" | "cpu_usage" | "ram_usage" | "network_usage"`.
    pub alert_type: String,
    /// e.g. `{"detected":"OBS Studio","process":"obs64.exe"}` or
    /// `{"metric":"cpu","value_pct":94,"threshold_pct":85,"duration_s":300}`.
    pub detail: Value,
    /// RFC3339, set by the agent at the moment the local notification fired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

// ── Clipboard Guard + Ink Receipt device reports (plan §4.4, §5.6) ───────
// Both types below ride `CheckinRequest` as a new batched `Vec<_>` field —
// `local_alerts: Vec<LocalAlertReport>` above is the batching precedent
// (`duress.rs:676`). `CheckinRequest`/`CheckinResponse` themselves are
// local to fleet-server's `routes/duress.rs`, not this crate: additive
// fields on that envelope are cheap, so the item types are the only piece
// that needs to live in the shared SSOT.
//
// Unlike `CheckinRequest`, EVERY type in this section carries
// `#[serde(deny_unknown_fields)]` — the strictness is deliberately scoped
// to these new report types, not inherited by the envelope. Neither type
// contains a `serde_json::Value` or any other free-text field; the in-tree
// anti-pattern both deliberately avoid is `LocalAlertReport::detail: Value`
// immediately above, which would let content leak through a field nobody
// reviews. `device_id` is deliberately ABSENT from both: it comes from the
// authenticated HMAC check-in identity (`duress.rs::authenticate_device_hmac`),
// never the request body — a body-supplied `device_id` would let a
// compromised agent attribute events to a different device.

/// One clipboard-guard rule match, reported by the agent as part of a
/// `CheckinRequest` batch (`clipboard_events: Vec<ClipboardEventReport>`,
/// plan §4.4).
///
/// `event_id`/`occurred_at` are `String` (RFC3339 for the latter), matching
/// this crate's universal id/timestamp idiom — see `ProductivitySample::
/// window_start` and `LocalAlertReport::occurred_at` above. `fleet-proto`
/// has ZERO `uuid`/`chrono` dependency, deliberately, to keep the
/// AV-scanned Free binary's dependency closure small (see the crate doc
/// comment); parsing/validating `event_id` as a UUID and `occurred_at` as
/// RFC3339 is the fleet-server ROUTE layer's job, not this type's.
///
/// `rule_id` is likewise a `String` — the wire form of a
/// `wincmd_clip_rules::RuleId` (see that type's own doc comment: "hand to a
/// `fleet-proto` `String`-typed wire field"). The route layer re-validates
/// it via `RuleId::new` before use; a malformed value is a `BadRequest`,
/// not a panic.
///
/// `severity`/`actions_attempted`/`actions_succeeded` are the closed enums
/// re-exported from `wincmd_clip_rules` above — never a `String` a caller
/// could set to an unrecognised value. `suppressed_count` is the
/// content-free cooldown metric from `wincmd_clip_rules::CooldownLedger::
/// should_emit`'s `Emit::Suppressed { count }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ClipboardEventReport {
    /// Device-minted UUIDv7 — the idempotency key.
    pub event_id: String,
    /// RFC3339, agent clock.
    pub occurred_at: String,
    pub policy_version: i64,
    /// String form of a `wincmd_clip_rules::RuleId` — see the struct doc.
    pub rule_id: String,
    pub rule_revision: u32,
    pub severity: Severity,
    pub actions_attempted: Vec<Action>,
    pub actions_succeeded: Vec<Action>,
    pub suppressed_count: u32,
}

/// Closed set of printer classes an Ink Receipt ticket or receipt can name
/// (plan §5.4/§5.6). Plan §5.4 uses exactly `Pdf`/`SecurePhysical` as its
/// cross-class replay example — a `pdf` ticket must never be presentable
/// for `secure_physical` — which is why `printer_class` binds into
/// [`ticket_preimage`] below as well as appearing here on the receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum PrinterClass {
    Pdf,
    SecurePhysical,
}

impl PrinterClass {
    /// The wire string this class binds into [`ticket_preimage`] under —
    /// `TicketSigningInput::printer_class` is a plain `&str` (see that
    /// type's doc for why), so a caller minting or verifying a ticket needs
    /// a single, non-hand-copied source for the string form of this enum.
    /// Mirrors `ActionClass::as_wire_str`'s KT rationale above: drive the
    /// mapping from serde's own `snake_case` representation so a future
    /// hand-written copy at a call site can't drift from what actually gets
    /// serialised.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            PrinterClass::Pdf => "pdf",
            PrinterClass::SecurePhysical => "secure_physical",
        }
    }
}

/// Closed outcome of one Ink Receipt render/print attempt (plan §5.5/§5.6).
/// Mirrors the exact status set the renderer maps onto: `scrub_warning` /
/// `failed` / `cancelled` come from the metadata-scrubber outcome mapping
/// (§5.5), `failed_after_render` is the "writer failed after the watermark
/// was already applied" case that must never be reported as `completed`,
/// `blocked` is the online-path zero-rows-consumed replay/expiry outcome
/// (§5.4), and `duplicate_or_replay` is the offline-path ex-post duplicate
/// detection outcome (§5.4, D-9) — never silently merged or dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum InkReceiptStatus {
    Completed,
    ScrubWarning,
    Failed,
    FailedAfterRender,
    Cancelled,
    Blocked,
    DuplicateOrReplay,
}

/// One Ink Receipt lifecycle report, batched onto `CheckinRequest` as
/// `ink_receipts: Vec<InkReceiptReport>` (plan §5.6). Same rules as
/// [`ClipboardEventReport`] above: `deny_unknown_fields` scoped to this
/// type only, no `Value`, no free-text field, `device_id` from the
/// authenticated identity rather than the body.
///
/// Fields are deliberately ONLY: `receipt_id`, `ticket_id`,
/// `printer_class`, `pages`, `status`, `policy_version`, `occurred_at`.
/// Specifically **NOT** present, and never to be added: document name,
/// file path, printer queue name, or OS user name — the whole point of the
/// controlled-PDF lane (plan §5) is that Fleet learns a page COUNT and an
/// OUTCOME, never what was printed or by whom on the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct InkReceiptReport {
    /// Device-minted UUID — the idempotency key.
    pub receipt_id: String,
    /// The `IR-<uuid-simple>` ticket id (D-5) this receipt completes.
    /// Treated as pseudonymous and encrypted at rest (D-8) — the route
    /// layer, not this type, owns that encryption and any format check.
    pub ticket_id: String,
    pub printer_class: PrinterClass,
    pub pages: u32,
    pub status: InkReceiptStatus,
    pub policy_version: i64,
    /// RFC3339, agent clock.
    pub occurred_at: String,
}

/// One local drive/volume's capacity, sampled by the agent's existing
/// `sysinfo`-backed local dashboard collector. Reused verbatim for the fleet
/// resource sample below — same shape, no PII (mount point / drive letter
/// only, never file contents or names).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DiskMetric {
    pub name: String,
    pub total_gb: f64,
    pub free_gb: f64,
}

/// Agent → server live resource telemetry, folded into the SAME check-in
/// round-trip as `posture`/`productivity`/`argus`. Always-on (no consent
/// gate) — this is aggregate device-health data, not user-activity data,
/// treated the same as `PostureReport`. Latest-snapshot-only: the server
/// overwrites the prior sample rather than accumulating history. No process
/// list, no filenames — aggregate scalars only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceResourceSample {
    /// 0.0–100.0, whole-system CPU utilization.
    pub cpu_usage_pct: f32,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub disks: Vec<DiskMetric>,
    pub net_up_bytes_per_sec: f64,
    pub net_down_bytes_per_sec: f64,
    /// RFC3339, set by the agent at sample time (not the server's receipt time).
    pub sampled_at: String,
}

/// One admin-facing notification (P3). Currently emitted for config DRIFT — a
/// device running behind its resolved policy epoch (and, when the device reports
/// `toggle_states`, the specific toggles that diverged). `detail` is PII-free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct FleetNotification {
    pub id: i64,
    /// Notification class — "drift" (config drift) or "insider_risk" (a critical
    /// Argus DLP/tamper/tripwire signal); extensible (e.g. "device_dark").
    pub kind: String,
    /// "info" | "warn" | "critical".
    pub severity: String,
    /// One-line human summary (no PII).
    pub summary: String,
    /// Structured PII-free detail (e.g. drifted toggle paths + desired values).
    pub detail: Value,
    #[serde(default)]
    pub device_id: Option<DeviceId>,
    #[serde(default)]
    pub hostname: Option<String>,
    pub read: bool,
    pub created_at: String,
}

/// Per-device drift view (F7): how the device's applied epoch compares to the
/// org's latest published policy epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct DeviceDrift {
    pub device_id: DeviceId,
    pub hostname: Option<String>,
    pub applied_epoch: i64,
    pub latest_epoch: i64,
    pub drifted: bool,
    pub last_reported: Option<String>,
}

/// Admin authority levels, ascending. The fleet server enforces a minimum role
/// per privileged action; the admin panel uses it to show/hide controls (UI is
/// not a security control — the server re-checks every request).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum AdminRole {
    Viewer,
    Operator,
    Admin,
    SuperAdmin,
}

/// Admin login credentials. Transport security (Tailscale/TLS) protects these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Issued session. `token` is an opaque bearer the panel sends as
/// `Authorization: Bearer <token>`; the server resolves it to the admin + role
/// (re-read from the DB) on every request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct LoginResponse {
    pub token: String,
    pub role: AdminRole,
    pub expires_at: String,
}

/// Non-secret view of the authenticated admin (the panel's "who am I").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct AdminView {
    pub email: String,
    pub role: AdminRole,
}

/// One admin account as listed by the admin-management UI. Carries `admin_id`
/// (needed to update/delete), unlike `AdminView` (the "who am I" shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct AdminAccount {
    pub admin_id: String,
    pub email: String,
    pub role: AdminRole,
}

/// Create a new admin (super_admin only). The password is hashed server-side
/// (argon2id) and never stored or echoed in plaintext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct CreateAdminRequest {
    pub email: String,
    pub password: String,
    pub role: AdminRole,
}

/// Change an admin's role (super_admin only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct UpdateAdminRoleRequest {
    pub role: AdminRole,
}

// ── PKM-1 / Policy types ─────────────────────────────────────────────────
// Three-layer policy model (org → group → device). The fleet server resolves
// all applicable layers into a per-device `ResolvedPolicy` and signs it with
// `policy_preimage`; the agent verifies the signature and applies the intents.
//
// Layer priority (higher wins):
//   org < group < device
//
// Intent modes:
//   "off"       — feature is disabled; no enforcement.
//   "report"    — observe and audit but do not block.
//   "heal"      — detect divergence and automatically remediate.
//   "hard-lock" — enforce and block any local override.

/// The per-key intent published within one policy layer.  Each intent binds
/// a policy key to a desired value, an enforcement mode, and an optional TTL.
/// The server normalises and signs an ordered slice of these in
/// [`ResolvedPolicy`]; the agent enforces them in `mode` order.
///
/// Wire type — shared with the fleet server and the admin panel.  All fields
/// are serialised; none are optional except `ttl_secs` (absent = no expiry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct PolicyIntent {
    /// Dot-path policy key, e.g. `"privacy.telemetry"` or `"fleet.enabled"`.
    pub key: String,
    /// Desired value — matches the AppSettings field type on the wire.
    pub value: serde_json::Value,
    /// Enforcement mode: `"off"` | `"report"` | `"heal"` | `"hard-lock"`.
    /// Unknown modes are treated as `"off"` by the agent (fail-safe).
    pub mode: String,
    /// How long (seconds) this intent is valid for.  `None` = no expiry.
    /// The agent re-fetches policy before acting on an expired intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
}

/// One layer of the policy stack as stored and served by the fleet server.
/// Layers are merged by the server (priority ascending) into a
/// [`ResolvedPolicy`] that is signed and handed to the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct PolicyLayer {
    /// Scope of this layer: `"org"` | `"group"` | `"device"`.
    pub layer_kind: String,
    /// Scope identifier within the kind (group_id / device_id). `None` for
    /// org-wide layers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
    /// Priority used when merging layers.  Higher priority wins on conflict.
    /// Conventional values: org=0, group=100, device=200.
    pub priority: i64,
    /// The intents this layer declares.  The server deduplicates by `key`
    /// (highest-priority wins) before producing [`ResolvedPolicy`].
    pub intents: Vec<PolicyIntent>,
}

/// The signed, per-device policy projection delivered to the agent.  Produced
/// by the fleet server after merging all applicable [`PolicyLayer`]s.
///
/// The agent verifies the signature over [`policy_preimage`]`(version, org_id,
/// device_id, intents)` using the fleet signing key pinned at enroll time, then
/// applies the intents in priority order.
///
/// `version` is monotonically increasing per org (same anti-rollback rule as
/// [`ConfigEpoch`]).  Agents reject any `ResolvedPolicy` whose `version` is ≤
/// the last applied version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
pub struct ResolvedPolicy {
    /// Monotonically increasing policy version for this org.
    pub version: i64,
    pub org_id: OrgId,
    pub device_id: DeviceId,
    /// Merged, deduplicated intents (sorted by `key` in the preimage — see
    /// [`policy_preimage`]).
    pub intents: Vec<PolicyIntent>,
}

// ── Canonical signing preimages (SSOT) ──────────────────────────────────
// The fleet server signs with these; the agent (and Free's policy-apply path)
// rebuild the same bytes to verify. Object keys are sorted recursively so
// semantically-equal JSON always produces identical bytes regardless of map
// ordering (serde_json's order is NOT guaranteed across the workspace).

/// All inputs that bind into a config-epoch signature. Every call site must
/// construct this struct explicitly so adding a field later causes a compile
/// error at every site rather than silently falling out of the preimage.
// KT: Typed struct is the SSOT entry point. `epoch_preimage` owns the byte
// layout. Nothing else should call `canonical_epoch_bytes` + `epoch_signing_envelope`
// by hand for an epoch — use `epoch_preimage(EpochSigningInput { … })` instead.
#[derive(Debug, Clone)]
pub struct EpochSigningInput<'a> {
    pub version: i64,
    pub config: &'a Value,
    pub locked_paths: &'a [String],
    pub managed: bool,
    pub target_kind: &'a str,
    pub target_id: Option<&'a str>,
}

/// The single function that owns the epoch signing preimage. All signers and
/// verifiers must call this — never assemble the bytes by hand.
///
/// Byte layout (stable): 8-byte big-endian `version`, then canonical JSON of the
/// signing envelope `{config, locked_paths, managed, target_id, target_kind}`.
/// `locked_paths` is sorted+deduped so semantically-equal lock sets are
/// byte-identical. Object keys are recursively sorted for the same reason.
pub fn epoch_preimage(input: &EpochSigningInput<'_>) -> Vec<u8> {
    let envelope = epoch_signing_envelope(
        input.config,
        input.locked_paths,
        input.managed,
        input.target_kind,
        input.target_id,
    );
    canonical_epoch_bytes(input.version, &envelope)
}

/// Preimage for a config epoch: 8-byte big-endian version, then canonical JSON.
/// The `config` argument is the *signed object* — for a targeted/locked epoch
/// that is the envelope produced by [`epoch_signing_envelope`], NOT the bare
/// `config_json`. Keeping this function payload-agnostic means the targeting +
/// lock fields (P2) bind into the signature with no change to this primitive.
// KT: This is a low-level primitive kept `pub` for tests. Production paths must
// go through `epoch_preimage(EpochSigningInput { … })` to avoid missing fields.
pub fn canonical_epoch_bytes(version: i64, config: &Value) -> Vec<u8> {
    let mut buf = version.to_be_bytes().to_vec();
    let mut json = String::new();
    write_canonical(config, &mut json);
    buf.extend_from_slice(json.as_bytes());
    buf
}

/// Build the canonical *signed envelope* for a Fleet Control Plane epoch. The
/// server signs `canonical_epoch_bytes(version, &envelope)`; the agent and
/// Free's policy-apply path rebuild the identical envelope to verify. Binding
/// the target + locks into the preimage is security-critical:
///   - `target_kind`/`target_id` stop a device replaying another scope's epoch.
///   - `locked_paths`/`managed` stop a tampered pull from stripping locks.
///
/// `locked_paths` is sorted so semantically-equal lock sets sign identically.
// KT: This is the building block for `epoch_preimage`. Call `epoch_preimage`
// in production; this is exposed for low-level testing only.
pub fn epoch_signing_envelope(
    config: &Value,
    locked_paths: &[String],
    managed: bool,
    target_kind: &str,
    target_id: Option<&str>,
) -> Value {
    let mut locks: Vec<String> = locked_paths.to_vec();
    locks.sort();
    locks.dedup();
    serde_json::json!({
        "config": config,
        "locked_paths": locks,
        "managed": managed,
        "target_kind": target_kind,
        "target_id": target_id,
    })
}

/// Preimage for a remote command. The agent rebuilds this from the
/// `SignedCommand` fields and verifies before executing.
///
/// **`command_id` param — pass `idempotency_key`, not the delivery UUID.**
/// Despite the parameter name (kept for wire/backward compatibility with the
/// existing preimage layout), every caller must pass the STABLE id the
/// signer actually signed over — `SignedCommand.idempotency_key` — never
/// `SignedCommand.command_id` (the server-assigned UUID, unknown at offline
/// signing time). This function's byte layout is otherwise unchanged.
pub fn canonical_command_bytes(
    command_id: &str,
    device_id: &str,
    catalog_id: &str,
    action_class: &str,
    payload: &Value,
    epoch_version: i64,
) -> Vec<u8> {
    let envelope = serde_json::json!({
        "command_id": command_id,
        "device_id": device_id,
        "catalog_id": catalog_id,
        "action_class": action_class,
        "payload": payload,
        "epoch_version": epoch_version,
    });
    let mut out = String::new();
    write_canonical(&envelope, &mut out);
    out.into_bytes()
}

/// Canonical signing preimage for a [`ResolvedPolicy`].  The fleet server
/// signs this; the agent rebuilds the identical bytes to verify before applying
/// the policy.
///
/// Byte layout (stable): 8-byte big-endian `version`, then canonical JSON of
/// `{"device_id": …, "intents": […], "org_id": …, "version": …}`.
///
/// **Security-critical bindings** — mirrors the rationale in `epoch_signing_envelope`:
///   - `version` (prefix + JSON field) prevents rollback to a superseded policy.
///   - `org_id` prevents cross-org replay (a policy signed for org A cannot
///     be presented to org B's fleet server as valid).
///   - `device_id` prevents cross-device replay (a policy resolved for device D1
///     cannot be replayed onto device D2 — the preimage would not match).
///
/// `intents` is sorted by `key` ascending so the projection is
/// order-independent: publishing the same logical intents in any order always
/// produces the same bytes.
///
/// Reuses [`canonical_epoch_bytes`] + [`write_canonical`] — no hand-rolled
/// byte logic here.
// KT: SSOT for policy preimage. Signers and verifiers must call this;
// never assemble the bytes by hand at call sites.
pub fn policy_preimage(
    version: i64,
    org_id: &str,
    device_id: &str,
    intents: &[PolicyIntent],
) -> Vec<u8> {
    // Sort intents by key so the preimage is order-independent.
    let mut sorted: Vec<&PolicyIntent> = intents.iter().collect();
    sorted.sort_by(|a, b| a.key.cmp(&b.key));

    // Serialise each intent to a serde_json::Value for canonical encoding.
    // The intents array preserves the key-sorted order established above.
    let intents_value: serde_json::Value = sorted
        .iter()
        .map(|i| serde_json::to_value(i).expect("PolicyIntent serialises infallibly"))
        .collect::<Vec<_>>()
        .into();

    // Envelope keys (device_id, intents, org_id, version) are already
    // alphabetically ordered here; write_canonical sorts them recursively
    // regardless, so this is just for readability at the call site.
    let envelope = serde_json::json!({
        "device_id": device_id,
        "intents":   intents_value,
        "org_id":    org_id,
        "version":   version,
    });

    canonical_epoch_bytes(version, &envelope)
}

/// All inputs that bind into an Ink Receipt ticket's signature (plan §5.4).
/// A typed struct, exactly like [`EpochSigningInput`] above — every call
/// site must construct this explicitly, so adding a new signed field later
/// is a compile error at every site rather than a silent omission from the
/// preimage. This is the same discipline `crypto.rs:118-121`
/// (fleet-server) records a real bug for under the old positional-argument
/// style; this typed-struct convention is the deliberate fix, applied here
/// too.
#[derive(Debug, Clone, Copy)]
pub struct TicketSigningInput<'a> {
    pub ticket_id: &'a str,
    pub org_id: &'a str,
    pub device_id: &'a str,
    pub printer_class: &'a str,
    pub policy_version: i64,
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
    pub nonce: &'a str,
}

/// The single function that owns the Ink Receipt ticket signing preimage.
/// The fleet server signs this at mint time (`POST
/// /v1/agents/ink-receipt/ticket`, plan §5.4); the endpoint's
/// `ink_receipt/tickets.rs` verifies before rendering — so this byte layout
/// is the SSOT both sides share, exactly like [`epoch_preimage`] and
/// [`policy_preimage`] above. All signers and verifiers must call this —
/// never assemble the bytes by hand.
///
/// Byte layout (stable): 8-byte big-endian `policy_version`, then
/// canonical JSON (via [`canonical_epoch_bytes`] / `write_canonical` —
/// never hand-rolled) of every field on [`TicketSigningInput`], object
/// keys recursively sorted.
///
/// **Security-critical bindings** (mirrors the rationale on
/// [`policy_preimage`] and `epoch_signing_envelope`):
///   - `policy_version` → a ticket cannot outlive the policy that
///     authorised it: a policy rollback/edit invalidates every ticket
///     minted under the superseded version (also the 8-byte prefix, same
///     anti-rollback shape as every other preimage in this crate).
///   - `org_id` → no cross-org replay: a ticket minted for org A cannot be
///     presented to org B's fleet server as valid.
///   - `device_id` → no cross-device replay: a ticket minted for device D1
///     cannot be redeemed by device D2 — the preimage would not match.
///   - `printer_class` → a `pdf` ticket cannot be presented for
///     `secure_physical`, or vice versa: the two lanes carry different
///     failure-stance policy (D-1), and this binding is what makes
///     cross-class substitution fail signature verification rather than
///     silently succeeding.
///   - `expires_at_unix` → bounded validity: the endpoint checks this
///     field's value against its own clock before accepting a ticket at
///     all (residual risk noted in plan §5.4: for the offline path this is
///     the DEVICE clock, not the server's ±300s check-in window).
///   - `nonce` → distinguishes otherwise-identical tickets (same org,
///     device, class, version, and validity window) so two such tickets
///     never collide on the same preimage/signature.
pub fn ticket_preimage(input: &TicketSigningInput<'_>) -> Vec<u8> {
    let envelope = serde_json::json!({
        "ticket_id": input.ticket_id,
        "org_id": input.org_id,
        "device_id": input.device_id,
        "printer_class": input.printer_class,
        "policy_version": input.policy_version,
        "issued_at_unix": input.issued_at_unix,
        "expires_at_unix": input.expires_at_unix,
        "nonce": input.nonce,
    });
    canonical_epoch_bytes(input.policy_version, &envelope)
}

/// Verify a base64 Ed25519 `signature` over `msg` against a base64 32-byte key.
/// Fail-closed: any decode/parse error returns false.
pub fn verify_signature_b64(public_key_b64: &str, msg: &[u8], signature_b64: &str) -> bool {
    let (Ok(key_bytes), Ok(sig_bytes)) = (B64.decode(public_key_b64), B64.decode(signature_b64))
    else {
        return false;
    };
    let Ok(key_arr): Result<[u8; 32], _> = key_bytes.as_slice().try_into() else {
        return false;
    };
    let Ok(sig_arr): Result<[u8; 64], _> = sig_bytes.as_slice().try_into() else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&key_arr) else {
        return false;
    };
    vk.verify(msg, &Signature::from_bytes(&sig_arr)).is_ok()
}

// KT: write_canonical is the ONLY place that produces signing bytes. Two
// invariants must hold:
//
//   (1) serde_json::to_string on a Value::String key or any scalar CANNOT fail
//       unless the `arbitrary_precision` feature is enabled (which would allow a
//       Value::Number to hold a non-finite float string). That feature is OFF in
//       this workspace and must stay OFF — see the boundary validations in
//       fleet-server push_config and in Free apply_admin_config_cmd, which reject
//       non-serializable config_json before reaching this function.
//
//   (2) The produced bytes must be deterministic for a given logical value — any
//       change here requires regenerating golden vectors AND a coordinated deploy
//       of all consumers (fleet-server + commander-free + any cached signatures).
//
// The debug_assert below documents (1): it fires in debug/test builds if
// arbitrary_precision is ever switched on and a non-finite Number sneaks through.
// In release builds the assert compiles away; the boundary validations upstream
// are the runtime guard.
fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // A String key always serializes; the debug_assert catches the
                // impossible case where arbitrary_precision breaks that guarantee.
                let key_json = serde_json::to_string(k)
                    .expect("string key serializes — arbitrary_precision must be OFF");
                debug_assert!(
                    !key_json.is_empty(),
                    "write_canonical: key serialization produced empty output"
                );
                out.push_str(&key_json);
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
        Value::Array(arr) => {
            out.push('[');
            for (i, e) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(e, out);
            }
            out.push(']');
        }
        // Scalars (null / bool / number / string) always serialize unless
        // arbitrary_precision is ON and a non-finite Number was constructed.
        // Boundary validation upstream (push_config, apply_admin_config_cmd)
        // must reject such input before it reaches here. The .expect message
        // names the invariant so a future failure is self-explaining.
        scalar => out.push_str(
            &serde_json::to_string(scalar)
                .expect("scalar serializes — arbitrary_precision must be OFF; boundary validation must reject non-serializable input upstream"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn security_snapshot() -> SecuritySnapshotResult {
        SecuritySnapshotResult {
            processes: vec![SecuritySnapshotProcess {
                pid: 42,
                parent_pid: Some(1),
                name: "agent.exe".into(),
                path: Some(r"C:\\Program Files\\Agent\\agent.exe".into()),
            }],
            listening_ports: vec![SecuritySnapshotListeningPort {
                pid: Some(42),
                protocol: SecuritySnapshotProtocol::Tcp,
                local_address: "0.0.0.0".into(),
                local_port: 443,
                process_name: Some("agent.exe".into()),
                process_path: None,
            }],
            services: vec![SecuritySnapshotService {
                name: "AgentSvc".into(),
                start_type: "auto".into(),
                status: "running".into(),
                path: Some(r"C:\\Program Files\\Agent\\agent.exe".into()),
            }],
            truncated: false,
        }
    }

    #[test]
    fn security_snapshot_contract_round_trips_and_is_bounded() {
        let snapshot = security_snapshot();
        validate_security_snapshot_result(&snapshot).unwrap();
        let round_trip: SecuritySnapshotResult =
            serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();
        assert_eq!(round_trip, snapshot);
    }

    #[test]
    fn security_snapshot_rejects_rows_past_the_domain_cap() {
        let mut snapshot = security_snapshot();
        snapshot.processes = (0..=SECURITY_SNAPSHOT_MAX_ROWS_PER_DOMAIN)
            .map(|pid| SecuritySnapshotProcess {
                pid: pid as u32,
                parent_pid: None,
                name: "process.exe".into(),
                path: None,
            })
            .collect();
        assert_eq!(
            validate_security_snapshot_result(&snapshot),
            Err("security snapshot row limit exceeded")
        );
    }

    #[test]
    fn security_snapshot_rejects_unknown_wire_fields() {
        let raw = serde_json::json!({
            "processes": [],
            "listening_ports": [],
            "services": [],
            "truncated": false,
            "scheduled_tasks": []
        });
        assert!(serde_json::from_value::<SecuritySnapshotResult>(raw).is_err());
    }

    #[test]
    fn config_epoch_round_trips_and_preserves_version() {
        let epoch = ConfigEpoch {
            org_id: OrgId("local".into()),
            version: 7,
            config_json: serde_json::json!({ "telemetry": false }),
            target_kind: "device".into(),
            target_id: Some("dev-1".into()),
            locked_paths: vec!["privacy.telemetry".into()],
            managed: true,
            signature: "c2ln".into(),
            signer_key: "a2V5".into(),
        };
        let json = serde_json::to_string(&epoch).unwrap();
        let back: ConfigEpoch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, 7);
        assert_eq!(back, epoch);
    }

    #[test]
    fn config_epoch_targeting_fields_default_for_legacy_json() {
        // A pre-targeting epoch (no target/lock fields) must still deserialize,
        // defaulting to an org-wide, unlocked, unmanaged epoch.
        let legacy = serde_json::json!({
            "org_id": "local", "version": 3,
            "config_json": { "a": 1 },
            "signature": "s", "signer_key": "k"
        });
        let back: ConfigEpoch = serde_json::from_value(legacy).unwrap();
        assert_eq!(back.target_kind, "org");
        assert_eq!(back.target_id, None);
        assert!(back.locked_paths.is_empty());
        assert!(!back.managed);
    }

    #[test]
    fn epoch_envelope_binds_target_and_locks_and_is_order_stable() {
        let cfg = serde_json::json!({ "telemetry": false });
        // Lock-path order must not change the signed bytes.
        let a = canonical_epoch_bytes(
            5,
            &epoch_signing_envelope(&cfg, &["b".into(), "a".into()], false, "group", Some("g1")),
        );
        let b = canonical_epoch_bytes(
            5,
            &epoch_signing_envelope(&cfg, &["a".into(), "b".into()], false, "group", Some("g1")),
        );
        assert_eq!(a, b);
        // A different target → different preimage (no cross-scope replay).
        let other = canonical_epoch_bytes(
            5,
            &epoch_signing_envelope(&cfg, &["a".into(), "b".into()], false, "device", Some("g1")),
        );
        assert_ne!(a, other);
        // Stripping a lock → different preimage (no silent downgrade).
        let unlocked = canonical_epoch_bytes(
            5,
            &epoch_signing_envelope(&cfg, &[], false, "group", Some("g1")),
        );
        assert_ne!(a, unlocked);
    }

    #[test]
    fn action_class_serializes_snake_case() {
        let json = serde_json::to_string(&ActionClass::Irreversible).unwrap();
        assert_eq!(json, "\"irreversible\"");
    }

    #[test]
    fn fleet_error_serializes_snake_case() {
        let json = serde_json::to_string(&FleetError::StaleEpoch).unwrap();
        assert_eq!(json, "\"stale_epoch\"");
    }

    #[test]
    fn epoch_sign_verify_round_trips_and_rejects_tampering() {
        let signing = SigningKey::from_bytes(&[9u8; 32]);
        let pubkey_b64 = B64.encode(signing.verifying_key().to_bytes());
        let config = serde_json::json!({ "telemetry": false, "a": [1, 2] });

        let msg = canonical_epoch_bytes(4, &config);
        let sig_b64 = B64.encode(signing.sign(&msg).to_bytes());
        assert!(verify_signature_b64(&pubkey_b64, &msg, &sig_b64));

        // Tampered version → different preimage → rejected.
        let wrong = canonical_epoch_bytes(5, &config);
        assert!(!verify_signature_b64(&pubkey_b64, &wrong, &sig_b64));
    }

    #[test]
    fn canonical_epoch_bytes_are_key_order_independent() {
        let a = canonical_epoch_bytes(1, &serde_json::json!({ "a": 1, "b": 2 }));
        let b = canonical_epoch_bytes(1, &serde_json::json!({ "b": 2, "a": 1 }));
        assert_eq!(a, b);
    }

    #[test]
    fn action_class_as_wire_str_matches_serde_snake_case() {
        // as_wire_str MUST stay in sync with the serde representation used in
        // canonical_command_bytes. Verify the mapping is identical.
        assert_eq!(ActionClass::Safe.as_wire_str(), "safe");
        assert_eq!(ActionClass::Destructive.as_wire_str(), "destructive");
        assert_eq!(ActionClass::Irreversible.as_wire_str(), "irreversible");
        // Also confirm serde gives the same strings (double-check the invariant).
        for (v, s) in [
            (ActionClass::Safe, "safe"),
            (ActionClass::Destructive, "destructive"),
            (ActionClass::Irreversible, "irreversible"),
        ] {
            let j = serde_json::to_string(&v).unwrap();
            assert_eq!(j, format!("\"{s}\""));
        }
    }

    // ── FROZEN GOLDEN VECTORS ─────────────────────────────────────────────────
    // These tests pin the EXACT bytes produced by the signing preimage functions.
    // They MUST fail if anyone changes:
    //   - field order in the envelope JSON
    //   - the version prefix (big-endian i64)
    //   - number/boolean formatting
    //   - the action-class wire string mapping
    //   - the sorting/dedup logic for locked_paths
    //
    // To regenerate: comment out the assert_eq!, run the test, read the "left:"
    // value from the failure output, paste it back. Document the change in a commit
    // message explaining WHY the bytes changed (it requires a coordinated deploy of
    // both repos plus re-signing any outstanding epochs).
    //
    // Inputs used:
    //   Epoch: version=7, config={"telemetry":false}, locked_paths=["b","a"],
    //          managed=true, target_kind="device", target_id=Some("dev-1")
    //   Command: command_id="cmd-abc", device_id="dev-xyz", catalog_id="status.read",
    //            action_class=Safe, payload={}, epoch_version=3

    #[test]
    fn golden_epoch_preimage() {
        let cfg = serde_json::json!({ "telemetry": false });
        let input = EpochSigningInput {
            version: 7,
            config: &cfg,
            // Paths supplied out-of-order; epoch_preimage must sort them.
            locked_paths: &["b".to_string(), "a".to_string()],
            managed: true,
            target_kind: "device",
            target_id: Some("dev-1"),
        };
        let bytes = epoch_preimage(&input);
        // FROZEN golden vector — computed on 2026-06-24.
        // Byte layout: [0,0,0,0,0,0,0,7] ++ canonical JSON of the envelope.
        // Envelope (keys sorted alphabetically):
        //   {"config":{"telemetry":false},"locked_paths":["a","b"],
        //    "managed":true,"target_id":"dev-1","target_kind":"device"}
        // To regenerate: remove the assert, run the test with `-- --nocapture`,
        // read the printed hex, and paste it back. Any change requires a
        // coordinated deploy of both repos and re-signing all outstanding epochs.
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "00000000000000077b22636f6e666967223a7b2274656c656d65747279223a66616c73657d2c226c6f636b65645f7061746873223a5b2261222c2262225d2c226d616e61676564223a747275652c227461726765745f6964223a226465762d31222c227461726765745f6b696e64223a22646576696365227d"
        );
    }

    // ── golden_ticket_preimage: the Ink Receipt ticket-signing preimage ────
    // Same discipline as golden_epoch_preimage above — pins the EXACT bytes
    // `ticket_preimage` produces. Any change here requires a coordinated
    // deploy of fleet-server's mint route AND every ticket-verifying
    // endpoint (`ink_receipt/tickets.rs`), plus re-signing/re-minting any
    // outstanding tickets, so it must never drift silently.

    /// Fixed inputs shared by the golden vector and the three
    /// cross-field-binding tests below.
    fn ticket_input() -> TicketSigningInput<'static> {
        TicketSigningInput {
            ticket_id: "IR-0123456789abcdef0123456789abcdef",
            org_id: "org-test",
            device_id: "dev-abc",
            printer_class: "pdf",
            policy_version: 5,
            issued_at_unix: 1_755_000_000,
            expires_at_unix: 1_755_003_600,
            nonce: "nonce-abc123",
        }
    }

    #[test]
    fn golden_ticket_preimage() {
        // Inputs (documented for regeneration): see `ticket_input()` above —
        //   ticket_id="IR-0123456789abcdef0123456789abcdef", org_id="org-test",
        //   device_id="dev-abc", printer_class="pdf", policy_version=5,
        //   issued_at_unix=1755000000, expires_at_unix=1755003600,
        //   nonce="nonce-abc123"
        //
        // Byte layout: [0,0,0,0,0,0,0,5] ++ canonical JSON of the envelope
        // (keys sorted alphabetically):
        //   {"device_id":"dev-abc","expires_at_unix":1755003600,
        //    "issued_at_unix":1755000000,"nonce":"nonce-abc123",
        //    "org_id":"org-test","policy_version":5,"printer_class":"pdf",
        //    "ticket_id":"IR-0123456789abcdef0123456789abcdef"}
        //
        // FROZEN golden vector — computed on 2026-08-17.
        // To regenerate: comment out the assert_eq!, run
        //   cargo test -p fleet-proto golden_ticket_preimage -- --nocapture
        // read the printed hex from the failure, paste it back, and commit
        // with a message explaining WHY the bytes changed (requires a
        // coordinated deploy of both repos plus re-minting outstanding
        // tickets).
        let bytes = ticket_preimage(&ticket_input());
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "00000000000000057b226465766963655f6964223a226465762d616263222c22657870697265735f61745f756e6978223a313735353030333630302c226973737565645f61745f756e6978223a313735353030303030302c226e6f6e6365223a226e6f6e63652d616263313233222c226f72675f6964223a226f72672d74657374222c22706f6c6963795f76657273696f6e223a352c227072696e7465725f636c617373223a22706466222c227469636b65745f6964223a2249522d3031323334353637383961626364656630313233343536373839616263646566227d"
        );
    }

    #[test]
    fn ticket_preimage_changes_with_printer_class() {
        // Proves the cross-class binding is real (plan §5.4's example): a
        // `pdf` ticket's preimage must differ from an otherwise-identical
        // `secure_physical` ticket's, so one can never be substituted for
        // the other at verification time.
        let base = ticket_input();
        let mut other = base;
        other.printer_class = "secure_physical";
        assert_ne!(
            ticket_preimage(&base),
            ticket_preimage(&other),
            "printer_class must bind into the preimage"
        );
    }

    #[test]
    fn ticket_preimage_changes_with_device_id() {
        // No cross-device replay: a ticket minted for one device_id must not
        // produce the same preimage/signature for another.
        let base = ticket_input();
        let mut other = base;
        other.device_id = "dev-xyz";
        assert_ne!(
            ticket_preimage(&base),
            ticket_preimage(&other),
            "device_id must bind into the preimage"
        );
    }

    #[test]
    fn ticket_preimage_changes_with_org_id() {
        // No cross-org replay: a ticket minted for one org_id must not
        // produce the same preimage/signature for another org.
        let base = ticket_input();
        let mut other = base;
        other.org_id = "org-other";
        assert_ne!(
            ticket_preimage(&base),
            ticket_preimage(&other),
            "org_id must bind into the preimage"
        );
    }

    #[test]
    fn golden_canonical_command_bytes() {
        let payload = serde_json::json!({});
        let bytes = canonical_command_bytes(
            "cmd-abc",
            "dev-xyz",
            "status.read",
            ActionClass::Safe.as_wire_str(),
            &payload,
            3,
        );
        // FROZEN golden vector — computed on 2026-06-24.
        // Envelope (keys sorted alphabetically):
        //   {"action_class":"safe","catalog_id":"status.read","command_id":"cmd-abc",
        //    "device_id":"dev-xyz","epoch_version":3,"payload":{}}
        // To regenerate: see golden_epoch_preimage comment above.
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "7b22616374696f6e5f636c617373223a2273616665222c22636174616c6f675f6964223a227374617475732e72656164222c22636f6d6d616e645f6964223a22636d642d616263222c226465766963655f6964223a226465762d78797a222c2265706f63685f76657273696f6e223a332c227061796c6f6164223a7b7d7d"
        );
    }

    #[test]
    fn golden_set_posture_policy_command_bytes() {
        let payload = serde_json::json!({
            "posture_epoch": 3,
            "cut_camera": false,
            "cut_mic": false,
            "cut_gps": true,
            "cut_sim": false,
            "reboot_timeout_ms": 900000,
        });
        let bytes = canonical_command_bytes(
            "posture-policy-3",
            "android-dev-1",
            "set_posture_policy",
            ActionClass::Safe.as_wire_str(),
            &payload,
            12,
        );
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "7b22616374696f6e5f636c617373223a2273616665222c22636174616c6f675f6964223a227365745f706f73747572655f706f6c696379222c22636f6d6d616e645f6964223a22706f73747572652d706f6c6963792d33222c226465766963655f6964223a22616e64726f69642d6465762d31222c2265706f63685f76657273696f6e223a31322c227061796c6f6164223a7b226375745f63616d657261223a66616c73652c226375745f677073223a747275652c226375745f6d6963223a66616c73652c226375745f73696d223a66616c73652c22706f73747572655f65706f6368223a332c227265626f6f745f74696d656f75745f6d73223a3930303030307d7d"
        );
    }

    // ── PKM-1 policy_preimage tests ───────────────────────────────────────────

    /// Helper: build the two intents used by the policy_preimage tests.
    fn test_intents() -> Vec<PolicyIntent> {
        vec![
            PolicyIntent {
                key: "privacy.telemetry".to_string(),
                value: serde_json::json!(false),
                mode: "hard-lock".to_string(),
                ttl_secs: None,
            },
            PolicyIntent {
                key: "fleet.enabled".to_string(),
                value: serde_json::json!(true),
                mode: "heal".to_string(),
                ttl_secs: Some(3600),
            },
        ]
    }

    #[test]
    fn policy_preimage_is_order_independent() {
        // Supplying the same intents in reverse order must produce the same bytes
        // (policy_preimage sorts by key ascending before hashing).
        let mut reversed = test_intents();
        reversed.reverse();
        assert_ne!(
            test_intents()[0].key,
            reversed[0].key,
            "precondition: order must differ"
        );

        let a = policy_preimage(1, "org-a", "dev-1", &test_intents());
        let b = policy_preimage(1, "org-a", "dev-1", &reversed);
        assert_eq!(
            a, b,
            "policy_preimage must be order-independent over intent ordering"
        );
    }

    #[test]
    fn policy_preimage_changes_with_binding_fields() {
        let intents = test_intents();
        let base = policy_preimage(1, "org-a", "dev-1", &intents);

        // Different version → different preimage (anti-rollback).
        let diff_ver = policy_preimage(2, "org-a", "dev-1", &intents);
        assert_ne!(base, diff_ver, "version change must change preimage");

        // Different org_id → different preimage (cross-org replay prevention).
        let diff_org = policy_preimage(1, "org-b", "dev-1", &intents);
        assert_ne!(base, diff_org, "org_id change must change preimage");

        // Different device_id → different preimage (cross-device replay prevention).
        let diff_dev = policy_preimage(1, "org-a", "dev-2", &intents);
        assert_ne!(base, diff_dev, "device_id change must change preimage");
    }

    #[test]
    fn golden_policy_preimage() {
        // Inputs (documented for regeneration):
        //   version=5, org_id="org-test", device_id="dev-abc"
        //   intents (supplied out-of-order; policy_preimage sorts by key):
        //     { key="privacy.telemetry", value=false,  mode="hard-lock", ttl_secs=None  }
        //     { key="fleet.enabled",     value=true,   mode="heal",      ttl_secs=3600  }
        //
        // Sorted key order: "fleet.enabled" < "privacy.telemetry"
        //
        // Byte layout: [0,0,0,0,0,0,0,5] ++ canonical JSON of:
        //   {"device_id":"dev-abc","intents":[
        //     {"key":"fleet.enabled","mode":"heal","ttl_secs":3600,"value":true},
        //     {"key":"privacy.telemetry","mode":"hard-lock","value":false}
        //   ],"org_id":"org-test","version":5}
        //
        // FROZEN golden vector — computed on 2026-06-24.
        // To regenerate: comment out the assert_eq!, run
        //   cargo test -p wincmd-shared golden_policy_preimage -- --nocapture
        // read the printed hex from the failure, paste it back, and commit with
        // a message explaining WHY the bytes changed (requires coordinated deploy).
        let intents = vec![
            // supplied out-of-order intentionally — sort must fix it.
            PolicyIntent {
                key: "privacy.telemetry".to_string(),
                value: serde_json::json!(false),
                mode: "hard-lock".to_string(),
                ttl_secs: None,
            },
            PolicyIntent {
                key: "fleet.enabled".to_string(),
                value: serde_json::json!(true),
                mode: "heal".to_string(),
                ttl_secs: Some(3600),
            },
        ];
        let bytes = policy_preimage(5, "org-test", "dev-abc", &intents);
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "00000000000000057b226465766963655f6964223a226465762d616263222c22696e74656e7473223a5b7b226b6579223a22666c6565742e656e61626c6564222c226d6f6465223a226865616c222c2274746c5f73656373223a333630302c2276616c7565223a747275657d2c7b226b6579223a22707269766163792e74656c656d65747279222c226d6f6465223a22686172642d6c6f636b222c2276616c7565223a66616c73657d5d2c226f72675f6964223a226f72672d74657374222c2276657273696f6e223a357d"
        );
    }

    #[test]
    fn device_summary_device_kind_is_additive_and_defaulted() {
        let legacy = serde_json::json!({
            "device_id": "dev-1",
            "device_hash": "hash-1",
            "hostname": null,
            "os_version": null,
            "agent_version": "1.0.0",
            "enrolled_at": "2026-08-13T00:00:00Z",
            "last_seen_at": null,
            "online": false,
            "group_id": null
        });
        let summary: DeviceSummary = serde_json::from_value(legacy).unwrap();
        assert_eq!(summary.device_kind, "");

        let mut current = serde_json::to_value(summary).unwrap();
        current["device_kind"] = serde_json::json!("android");
        let summary: DeviceSummary = serde_json::from_value(current).unwrap();
        assert_eq!(summary.device_kind, "android");
    }

    // ── Content-free type-layer tests (plan §8 layer 1) ──────────────────
    // `ClipboardEventReport`/`InkReceiptReport` must (a) actually reject an
    // unknown field at deserialize time — `#[serde(deny_unknown_fields)]`
    // written on the struct is a promise, not a guarantee, until a test
    // exercises it — and (b) have a CLOSED, exhaustively-enumerated field
    // set, so a future PR that quietly adds a free-text field (a `note:
    // String`, say) has to edit the enumeration below, making that addition
    // a deliberate, reviewable diff rather than a silent one. This is the
    // practical, runtime-checkable proxy for "no field capable of carrying
    // free text": the type system already rules out `serde_json::Value`
    // (neither type imports it), and this test rules out an undocumented
    // field appearing at all.

    fn clipboard_event_report_sample() -> ClipboardEventReport {
        ClipboardEventReport {
            event_id: "018f2f3a-0000-7000-8000-000000000000".to_string(),
            occurred_at: "2026-08-17T00:00:00Z".to_string(),
            policy_version: 1,
            rule_id: "0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b".to_string(),
            rule_revision: 1,
            severity: Severity::Warn,
            actions_attempted: vec![Action::ClearClipboard],
            actions_succeeded: vec![Action::ClearClipboard],
            suppressed_count: 0,
        }
    }

    fn ink_receipt_report_sample() -> InkReceiptReport {
        InkReceiptReport {
            receipt_id: "018f2f3a-0000-7000-8000-000000000001".to_string(),
            ticket_id: "IR-0123456789abcdef0123456789abcdef".to_string(),
            printer_class: PrinterClass::Pdf,
            pages: 3,
            status: InkReceiptStatus::Completed,
            policy_version: 1,
            occurred_at: "2026-08-17T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn clipboard_event_report_rejects_unknown_fields() {
        let mut raw = serde_json::to_value(clipboard_event_report_sample()).unwrap();
        raw.as_object_mut().unwrap().insert(
            "matched_text".to_string(),
            serde_json::json!("whatever was on the clipboard"),
        );
        assert!(
            serde_json::from_value::<ClipboardEventReport>(raw).is_err(),
            "deny_unknown_fields must reject an unrecognised key"
        );
    }

    #[test]
    fn ink_receipt_report_rejects_unknown_fields() {
        let mut raw = serde_json::to_value(ink_receipt_report_sample()).unwrap();
        raw.as_object_mut().unwrap().insert(
            "document_name".to_string(),
            serde_json::json!("Q3-layoffs-draft.docx"),
        );
        assert!(
            serde_json::from_value::<InkReceiptReport>(raw).is_err(),
            "deny_unknown_fields must reject an unrecognised key"
        );
    }

    #[test]
    fn clipboard_event_report_field_set_is_closed() {
        let value = serde_json::to_value(clipboard_event_report_sample()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "actions_attempted",
                "actions_succeeded",
                "event_id",
                "occurred_at",
                "policy_version",
                "rule_id",
                "rule_revision",
                "severity",
                "suppressed_count",
            ],
            "adding a field here (e.g. free-form text) must be a deliberate edit to this test"
        );
    }

    #[test]
    fn ink_receipt_report_field_set_is_closed() {
        let value = serde_json::to_value(ink_receipt_report_sample()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "occurred_at",
                "pages",
                "policy_version",
                "printer_class",
                "receipt_id",
                "status",
                "ticket_id",
            ],
            "adding a field here (e.g. document name / file path) must be a deliberate edit to this test"
        );
    }

    #[cfg(feature = "command-metadata")]
    #[test]
    fn command_metadata_has_no_duplicate_catalog_ids() {
        let mut ids: Vec<&str> = COMMAND_METADATA.iter().map(|m| m.catalog_id).collect();
        ids.sort_unstable();
        let mut deduped = ids.clone();
        deduped.dedup();
        assert_eq!(
            ids.len(),
            deduped.len(),
            "COMMAND_METADATA has a duplicate catalog_id"
        );
    }
}
