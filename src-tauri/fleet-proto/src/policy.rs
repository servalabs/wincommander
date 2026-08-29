// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signed policy wire contract. B2 owns this module after G-1.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{write_canonical, DeviceId, OrgId};

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
    pub value: Value,
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
    /// The durable per-device command that caused this desired state, when
    /// this is a device-scoped Start/Stop request. The endpoint echoes this
    /// identifier only in lifecycle receipts; it never carries camera media
    /// or application details. Org/group defaults have no command id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EpochSigningInput<'a> {
    pub version: i64,
    pub config: &'a Value,
    pub locked_paths: &'a [String],
    pub managed: bool,
    pub target_kind: &'a str,
    pub target_id: Option<&'a str>,
}

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

pub fn canonical_epoch_bytes(version: i64, config: &Value) -> Vec<u8> {
    let mut buf = version.to_be_bytes().to_vec();
    let mut json = String::new();
    write_canonical(config, &mut json);
    buf.extend_from_slice(json.as_bytes());
    buf
}

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

pub fn policy_preimage(
    version: i64,
    org_id: &str,
    device_id: &str,
    intents: &[PolicyIntent],
) -> Vec<u8> {
    let mut sorted: Vec<&PolicyIntent> = intents.iter().collect();
    sorted.sort_by(|a, b| a.key.cmp(&b.key));
    let intents_value: Value = sorted
        .iter()
        .map(|intent| serde_json::to_value(intent).expect("PolicyIntent serialises infallibly"))
        .collect::<Vec<_>>()
        .into();
    let envelope = serde_json::json!({
        "device_id": device_id,
        "intents": intents_value,
        "org_id": org_id,
        "version": version,
    });
    canonical_epoch_bytes(version, &envelope)
}
