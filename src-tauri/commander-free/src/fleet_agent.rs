// SPDX-License-Identifier: AGPL-3.0-or-later
// commander-free/src/fleet_agent.rs
//
// ═══════════════════════════════════════════════════════════════════════════
// Fleet agent — Free-side thin wrappers for in-app fleet onboarding (PAID)
// ═══════════════════════════════════════════════════════════════════════════
//
// Each command is a thin #[tauri::command] shim that:
//   1. Gate-checks the explicit signed Fleet service entitlement.
//   2. Persists the config change to settings (app.fleet.*).
//   3. Delegates the runtime action to the Pro sidecar via dispatch_paid_command.
//
// PINNED feature_ids (do NOT rename — Pro handlers.rs matches on these):
//   "fleet_agent_configure"   → Pro: fleet_push::configure
//   "fleet_agent_status"      → Pro: fleet_push::status
//   "fleet_agent_disconnect"  → Pro: fleet_push::disconnect
//
// AV hygiene (lint:strings-free invariant): this file MUST NOT contain fleet
// server URLs or anything resembling C2 traffic. All HTTP lives in Pro.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::RwLock;
use url::Url;

/// Canonicalize the operator-entered Fleet base URL before it is persisted or
/// handed to Pro.  In particular, a Windows-style accidental extra slash in
/// `http:///host:port` must never become a permanently retrying agent config.
/// Fleet endpoints always live at the origin; accepting paths, credentials,
/// queries, or fragments here would make their later URL construction
/// ambiguous.
fn normalize_fleet_server_url(raw: &str) -> Result<String, String> {
    let input = raw.trim();
    let repaired = ["http", "https"]
        .iter()
        .find_map(|scheme| {
            let prefix = format!("{scheme}:///");
            input
                .strip_prefix(&prefix)
                .map(|rest| format!("{scheme}://{}", rest.trim_start_matches('/')))
        })
        .unwrap_or_else(|| input.to_string());
    let parsed = Url::parse(&repaired)
        .map_err(|_| "Fleet server URL must be a valid http(s) origin.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Fleet server URL must be a valid http(s) origin.".to_string());
    }
    Ok(parsed.origin().ascii_serialization())
}

/// Runtime fleet status snapshot returned from the Pro side.
/// Mirrors FleetStatus in commander-pro/src/fleet_push.rs.
// Mirrors Pro's FleetStatus; not constructed in the lib build but exercised by tests.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FleetStatus {
    pub connected: bool,
    pub device_id: String,
    pub server_url: String,
    pub last_enroll_at: Option<String>,
    pub last_error: Option<String>,
    /// True while Pro's check-in loop is alive and self-healing (connected or
    /// mid a transient-failure retry); false once it has permanently stopped
    /// (never started, disconnected, or a terminal rejection — e.g. this
    /// device was removed/unenrolled server-side). See fleet_push.rs.
    pub retrying: bool,
    /// True while this device's enrollment is still awaiting admin approval:
    /// the server accepts its check-ins (so `connected` is true) but withholds
    /// all config/commands until approved. The Fleet panel shows "Request
    /// submitted — waiting for admin approval" rather than "Enrolled". See
    /// fleet_push.rs FleetStatus.
    #[serde(default)]
    pub pending_approval: bool,
}

// Shared state: when the apply loop detects consecutive Pro-sidecar failures it
// writes the specific transport error here (e.g. "PRO_NOT_INSTALLED: ...",
// "Pro spawn failed: ..."). fleet_status falls back to this string when the
// sidecar is unreachable, so the frontend sees the real reason rather than a
// silent stale status or an opaque generic label.
static SIDECAR_LAST_ERROR: RwLock<Option<String>> = RwLock::new(None);

/// Called by the apply loop on each Pro-sidecar failure.
pub fn set_sidecar_error(msg: String) {
    if let Ok(mut g) = SIDECAR_LAST_ERROR.write() {
        *g = Some(msg);
    }
}

/// Called by the apply loop on success — clears any previously latched error.
pub fn clear_sidecar_error() {
    if let Ok(mut g) = SIDECAR_LAST_ERROR.write() {
        *g = None;
    }
}

/// Read the latched sidecar error (None if healthy).
pub fn sidecar_last_error() -> Option<String> {
    SIDECAR_LAST_ERROR.read().ok().and_then(|g| g.clone())
}

/// Classification of a fleet apply-loop error.
/// KT: only TransportFailure increments the unreachable streak and latches
/// pro_unreachable. PolicyError means Pro is reachable but the epoch cannot
/// be applied for a policy/config reason (no key, bad sig, not newer, etc.) —
/// do NOT escalate unreachable backoff for these.
#[derive(Debug)]
pub enum ApplyError {
    /// Pro sidecar IPC failed — pipe broken, spawn failed, timeout.
    TransportFailure(String),
    /// Pro is reachable but the epoch was rejected for a policy reason.
    PolicyError(String),
}

impl ApplyError {
    pub fn is_transport(&self) -> bool {
        matches!(self, ApplyError::TransportFailure(_))
    }
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::TransportFailure(s) => write!(f, "{s}"),
            ApplyError::PolicyError(s) => write!(f, "{s}"),
        }
    }
}

/// True if the dispatch error string represents a transport-level failure
/// (pipe dead, spawn failed, timeout) rather than a Pro application error.
/// KT: transport errors from dispatch_paid_command carry well-known prefixes set
/// in sidecar.rs. Pro semantic errors start with "[pro:" and are NOT transport.
fn is_transport_err(e: &str) -> bool {
    // Semantic Pro errors (handled server-side, pipe still alive) are not transport.
    if e.starts_with("[pro:") {
        return false;
    }
    // Known transport-error prefixes from dispatch_paid_command / spawn_pro_session.
    e.starts_with("Pro transport error:")
        || e.starts_with("Pro spawn failed")
        || e.starts_with("Pro response timeout")
        || e.starts_with("Pro reader exited")
        || e.starts_with("pool semaphore closed")
        || e.starts_with("write request:")
        || e.starts_with("PRO_NOT_INSTALLED:")
        || e.contains("pipe")
        || e.contains("spawn")
        || e.contains("timeout")
}

/// Connect this device to a fleet server and persist the config to settings.
///
/// Params: serverUrl, dispatch (bool), signingKeyPub (base64 Ed25519 key).
/// Persists { enabled:true, serverUrl, dispatch, signingKeyPub } to app.fleet
/// then forwards the same values to the running Pro sidecar so the agent
/// starts immediately without requiring an app restart.
#[tauri::command]
pub async fn fleet_connect(
    app: tauri::AppHandle,
    server_url: String,
    dispatch: bool,
    signing_key_pub: String,
    confirmation: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    let server_url = normalize_fleet_server_url(&server_url)?;

    // Re-pin guard (audit C4): if this device is already enrolled with a
    // DIFFERENT signing key or server, re-pointing it is security-critical — a
    // compromised WebView could otherwise silently re-enroll the agent to an
    // attacker-controlled server and then receive attacker-signed commands.
    // Require a confirmation the WebView cannot forge (a capability token, or an
    // OS-native confirm — enrollment is always user-present). First-time
    // enrollment (no prior key/server) is unaffected.
    if let Ok(prev) = crate::settings::read_settings() {
        let prev_key = prev.app.fleet.signing_key_pub;
        let prev_url = prev.app.fleet.server_url;
        let changing = (!prev_key.is_empty() && prev_key != signing_key_pub)
            || (!prev_url.is_empty() && prev_url != server_url);
        if changing {
            let ok = match confirmation.as_deref() {
                Some(tok) => crate::authz::consume(
                    tok,
                    crate::authz::DestructiveAction::FleetReenroll,
                    &server_url,
                )
                .is_ok(),
                None => {
                    crate::authz::native_confirm_action(
                        &app,
                        crate::authz::DestructiveAction::FleetReenroll,
                    )
                    .await
                }
            };
            if !ok {
                return Err(
                    "Re-enrolling this device to a different fleet server requires confirmation."
                        .to_string(),
                );
            }
        }
    }

    // 1. Persist config so it survives Pro restarts and app reboots.
    //    Free owns settings; Pro reads the config via IPC args at start time.
    //    The fleet signing key is ALSO pinned into policy.fleet_signing_key so the
    //    policy-apply path (apply_admin_config_cmd) only accepts epochs signed
    //    by this fleet server — fail-closed if no key is supplied (P2 locks).
    //    NOTE (2026-07-09): the check-in transport carries NO device keypair — the
    //    server issues a per-device HMAC `checkin_secret` at enroll. The old
    //    `agentSigningKey{Priv,Pub}` generation is gone.
    let pinned_key = if signing_key_pub.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(signing_key_pub.clone())
    };
    let patch = serde_json::json!({
        "app": {
            "fleet": {
                "enabled": true,
                "serverUrl": server_url,
                "dispatch": dispatch,
                "signingKeyPub": signing_key_pub,
                "privacyShieldSessionOwned": false,
            },
            // Fleet enrollment must not take ownership of the local camera or
            // start Privacy Shield. An administrator can publish a signed
            // policy from the Fleet console later; until then any Shield
            // session is employee-started and remains locally controllable.
            "modules": { "privacyShield": true }
        },
        "ideal": {
            "privacy": {
                "privacyShield": {
                    "fleetManaged": false,
                    "fleetMonitoringEnabled": false,
                }
            }
        },
        "policy": {
            "fleetSigningKey": pinned_key,
        }
    });
    crate::settings::patch_settings(patch)?;

    // 2. Forward to the running Pro process so the agent starts immediately.
    //    Pass the machine's STABLE device_id so the fleet sees the same device on
    //    every re-enroll (no device keypair anymore — HMAC check-in secret only).
    let device_id = crate::settings::read_settings()
        .map(|s| s.device_id)
        .unwrap_or_default();
    let args = serde_json::json!({
        "serverUrl": server_url,
        "dispatch": dispatch,
        "signingKeyPub": signing_key_pub,
        "deviceId": device_id,
    });
    crate::sidecar::dispatch_paid_command("fleet_agent_configure", args).await
}

/// Query the running fleet agent's status (connected, deviceId, lastEnrollAt,
/// lastError). Purely a status read — no side effects, no settings writes.
/// When Pro is unreachable, falls back to a synthetic status that surfaces the
/// specific transport error so the frontend never shows stale/empty state silently.
#[tauri::command]
pub async fn fleet_status() -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    match crate::sidecar::dispatch_paid_command("fleet_agent_status", serde_json::Value::Null).await
    {
        Ok(mut v) => {
            // Pro responded — merge any latched error (or clear it if Pro is ok).
            if let Some(err) = sidecar_last_error() {
                if let Some(obj) = v.as_object_mut() {
                    obj.entry("lastError")
                        .or_insert_with(|| serde_json::Value::String(err));
                }
            }
            Ok(v)
        }
        Err(e) => {
            // Pro sidecar is unreachable — synthesize a status struct so the UI
            // sees "disconnected + <specific reason>" instead of an error toast.
            // KT: prefer the latched apply-loop error (already includes a fail
            // streak) but fall back to THIS call's own dispatch error text (e.g.
            // "PRO_NOT_INSTALLED: ...", "Pro spawn failed: ...") rather than the
            // opaque "pro_unreachable" — the real reason is already specific.
            let last_err = sidecar_last_error().unwrap_or(e);
            Ok(serde_json::json!({
                "connected": false,
                "deviceId": "",
                "serverUrl": "",
                "lastEnrollAt": null,
                "lastError": last_err,
            }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteCommandStateUpdate {
    command_id: String,
    catalog_id: String,
    #[serde(default)]
    payload: Value,
}

fn privacy_remote_path(key: &str) -> Option<&'static str> {
    match key {
        "telemetry" => Some("privacy.telemetry.windowsDisabled"),
        "clipboardHistory" => Some("privacy.clipboard.historyDisabled"),
        "cloudClipboard" => Some("privacy.clipboard.cloudSyncDisabled"),
        "recentFiles" => Some("privacy.tracking.recentFilesDisabled"),
        "jumpLists" => Some("privacy.tracking.jumpListsDisabled"),
        "thumbnailCache" => Some("privacy.tracking.thumbnailCacheDisabled"),
        "activity" => Some("privacy.telemetry.activityHistoryDisabled"),
        "location" => Some("privacy.telemetry.locationTrackingDisabled"),
        "suggestions" => Some("privacy.telemetry.windowsSuggestionsDisabled"),
        "lockScreenPrivacy" => Some("privacy.lockscreen.privacyDisabled"),
        "recallSnapshots" => Some("privacy.tracking.recallSnapshotsDisabled"),
        "typingInsights" => Some("privacy.tracking.typingInsightsDisabled"),
        "internetComm" => Some("privacy.internetCommunication.restrictedEnabled"),
        "advertisingId" => Some("privacy.tracking.advertisingIdDisabled"),
        "tailoredExp" => Some("privacy.tracking.tailoredExperiencesDisabled"),
        "officeLog" => Some("privacy.tracking.officeLoggingDisabled"),
        "diagTracing" => Some("privacy.tracking.diagnosticEventTracingDisabled"),
        "hideQuickAccessRecent" => Some("privacy.tracking.quickAccessRecentDisabled"),
        "hideQuickAccessFrequent" => Some("privacy.tracking.quickAccessFrequentDisabled"),
        "hideRunMRU" => Some("privacy.tracking.runMruDisabled"),
        "disableSearchHistory" => Some("privacy.tracking.searchHistoryDisabled"),
        _ => None,
    }
}

fn capability_remote_field(capability: &str) -> Option<&'static str> {
    match capability {
        "webcam" => Some("webcam"),
        "microphone" => Some("microphone"),
        "contacts" => Some("contacts"),
        "appointments" => Some("calendar"),
        "phoneCall" => Some("phoneCall"),
        "phoneCallHistory" => Some("callHistory"),
        "chat" => Some("messaging"),
        "userNotificationListener" => Some("notifications"),
        "documentsLibrary" => Some("documents"),
        "picturesLibrary" => Some("pictures"),
        "videosLibrary" => Some("videos"),
        "broadFileSystemAccess" => Some("fileSystem"),
        "gazeInput" => Some("gazeInput"),
        "appDiagnostics" => Some("appDiagnostics"),
        "userAccountInformation" => Some("userAccountInformation"),
        "bluetoothSync" => Some("bluetoothSync"),
        _ => None,
    }
}

fn insert_dot_path(root: &mut Value, path: &str, value: Value) {
    let mut cursor = root;
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            if let Value::Object(map) = cursor {
                map.insert(segment.to_string(), value.clone());
            }
            return;
        }
        let map = cursor
            .as_object_mut()
            .expect("remote state patch root is an object");
        cursor = map
            .entry(segment.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
}

fn merge_remote_patch(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                merge_remote_patch(base_map.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (slot, value) => *slot = value.clone(),
    }
}

fn mirrored_remote_patch(path: &str, value: Value) -> Value {
    let mut patch = Value::Object(Map::new());
    insert_dot_path(&mut patch, &format!("ideal.{path}"), value.clone());
    insert_dot_path(&mut patch, &format!("current.{path}"), value);
    patch
}

fn remote_command_settings_patch(update: &RemoteCommandStateUpdate) -> Option<Value> {
    if let Some(rest) = update.catalog_id.strip_prefix("privacy.") {
        let (key, enabled) = rest
            .strip_suffix(".enable")
            .map(|key| (key, true))
            .or_else(|| rest.strip_suffix(".disable").map(|key| (key, false)))?;
        return Some(mirrored_remote_patch(
            privacy_remote_path(key)?,
            Value::Bool(enabled),
        ));
    }

    let (path, value) = match update.catalog_id.as_str() {
        // The Pro sidecar executes these monitor commands before handing their
        // result to Free. Mirror that effective state into settings so the
        // Privacy card does not display a stale local toggle.
        "monitoring.screen_capture.start" => {
            ("privacy.screenCapture.detectionEnabled", Value::Bool(true))
        }
        "monitoring.screen_capture.stop" => {
            ("privacy.screenCapture.detectionEnabled", Value::Bool(false))
        }
        "defender.enable" => ("tweaks.security.defenderDisabled", Value::Bool(false)),
        "defender.disable" => ("tweaks.security.defenderDisabled", Value::Bool(true)),
        "usb.writeprotect.enable" => ("tweaks.security.usbWriteProtect", Value::Bool(true)),
        "usb.writeprotect.disable" => ("tweaks.security.usbWriteProtect", Value::Bool(false)),
        "usb.storage.lockdown.enable" => ("tweaks.security.usbStorageLockdown", Value::Bool(true)),
        "usb.storage.lockdown.disable" => {
            ("tweaks.security.usbStorageLockdown", Value::Bool(false))
        }
        "capability.set" => {
            let capability = update.payload.get("Capability")?.as_str()?;
            let access = update.payload.get("Access")?.as_str()?;
            if access != "Allow" && access != "Deny" {
                return None;
            }
            let field = capability_remote_field(capability)?;
            return Some(mirrored_remote_patch(
                &format!("privacy.appCapabilities.{field}"),
                Value::String(access.to_string()),
            ));
        }
        _ => return None,
    };
    Some(mirrored_remote_patch(path, value))
}

async fn apply_remote_command_updates(resp: &Value) -> Result<usize, ApplyError> {
    let updates: Vec<RemoteCommandStateUpdate> = serde_json::from_value(
        resp.get("commandUpdates")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .map_err(|e| ApplyError::PolicyError(format!("invalid fleet command state: {e}")))?;

    let mut patch = Value::Object(Map::new());
    let mut command_ids = Vec::new();
    for update in &updates {
        if let Some(update_patch) = remote_command_settings_patch(update) {
            merge_remote_patch(&mut patch, &update_patch);
            command_ids.push(update.command_id.clone());
        }
    }
    if command_ids.is_empty() {
        return Ok(0);
    }

    // Persist first, ack second. A failed write leaves the Pro queue intact so
    // the next poll retries instead of silently losing verified admin intent.
    crate::settings::patch_settings(patch).map_err(ApplyError::PolicyError)?;
    let applied_count = command_ids.len();
    crate::sidecar::dispatch_paid_command(
        "fleet_agent_ack_remote_state",
        serde_json::json!({ "commandIds": &command_ids }),
    )
    .await
    .map_err(|e| {
        if is_transport_err(&e) {
            ApplyError::TransportFailure(e)
        } else {
            ApplyError::PolicyError(e)
        }
    })?;
    Ok(applied_count)
}

/// Internal (typed) implementation of fleet_apply_pending_epoch.
/// Returns `ApplyError::TransportFailure` when the Pro IPC channel is broken
/// (pipe dead, spawn failed, timeout) and `ApplyError::PolicyError` for all
/// other rejections (no pinned key, bad signature, epoch not newer, etc.).
/// KT: the apply loop uses this to distinguish genuine "Pro unreachable" from
/// reachable-but-policy-rejected so backoff/status are not mis-escalated.
pub async fn fleet_apply_pending_epoch_typed() -> Result<serde_json::Value, ApplyError> {
    crate::license::require_service_feature("fleet").map_err(ApplyError::PolicyError)?;

    // The dispatch call is the ONLY place a transport failure can originate.
    let resp =
        crate::sidecar::dispatch_paid_command("fleet_agent_pending_epoch", serde_json::Value::Null)
            .await
            .map_err(|e| {
                // KT: classify by error string; see is_transport_err for the heuristics.
                if is_transport_err(&e) {
                    ApplyError::TransportFailure(e)
                } else {
                    // Semantic Pro error ([pro:...]) — pipe still alive, not a transport failure.
                    ApplyError::PolicyError(e)
                }
            })?;

    let epoch = resp
        .get("epoch")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if epoch.is_null() {
        let remote_updates_applied = apply_remote_command_updates(&resp).await?;
        return Ok(serde_json::json!({
            "applied": remote_updates_applied > 0,
            "remoteUpdatesApplied": remote_updates_applied,
            "reason": "no_pending_epoch",
        }));
    }

    let version = epoch.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
    let settings = crate::settings::read_settings().map_err(ApplyError::PolicyError)?;
    let applied = settings.policy.master_config_version.unwrap_or(0) as i64;
    if version <= applied {
        let remote_updates_applied = apply_remote_command_updates(&resp).await?;
        return Ok(serde_json::json!({
            "applied": remote_updates_applied > 0,
            "remoteUpdatesApplied": remote_updates_applied,
            "reason": "not_newer",
            "version": version,
        }));
    }

    // Never apply unsigned/unpinned fleet policy.
    // KT: no pinned key is a reachable config state (device enrolled without key);
    // this is a PolicyError, NOT a transport failure.
    if settings.policy.fleet_signing_key.is_none() {
        return Err(ApplyError::PolicyError(
            "cannot apply fleet policy: no pinned fleet signing key".to_string(),
        ));
    }

    let config = epoch
        .get("config_json")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let locked_paths: Vec<String> = epoch
        .get("locked_paths")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let managed = epoch
        .get("managed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let target_kind = epoch
        .get("target_kind")
        .and_then(|v| v.as_str())
        .map(String::from);
    let target_id = epoch
        .get("target_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let signature = epoch
        .get("signature")
        .and_then(|v| v.as_str())
        .map(String::from);
    let signer_key = epoch
        .get("signer_key")
        .and_then(|v| v.as_str())
        .map(String::from);

    // apply_admin_config_cmd can fail on signature verification — also a PolicyError.
    let updated = crate::settings::apply_admin_config_cmd(
        config,
        locked_paths,
        "merge".to_string(),
        version as u32,
        signature,
        signer_key,
        target_kind,
        target_id,
        Some(managed),
    )
    .map_err(ApplyError::PolicyError)?;

    // A cached epoch may predate a command that executed moments ago. Apply
    // the verified command handoff last so it is the deterministic last writer
    // and cannot be immediately overwritten by older policy intent.
    let remote_updates_applied = apply_remote_command_updates(&resp).await?;

    Ok(serde_json::json!({
        "applied": true,
        "version": version,
        "remoteUpdatesApplied": remote_updates_applied,
        "settings": updated,
    }))
}

/// Fetch the latest signed policy epoch the Pro agent has pulled, verify it
/// against the pinned fleet key, and apply it (values + locked paths) via the
/// signature-checking `apply_admin_config_cmd`. This is the Free side of the
/// one-way-IPC policy-apply loop (Pro caches the epoch; Free pulls + applies).
///
/// Fail-closed: refuses to apply when no fleet key is pinned, and skips an epoch
/// that is not strictly newer than the one already applied (monotonic).
/// Returns `{ applied: bool, reason?, version? }`.
/// KT: the apply loop calls fleet_apply_pending_epoch_typed() for error classification.
/// This Tauri command exists for direct frontend/test invocation only.
#[tauri::command]
pub async fn fleet_apply_pending_epoch() -> Result<serde_json::Value, String> {
    fleet_apply_pending_epoch_typed()
        .await
        .map_err(|e| e.to_string())
}

/// Internal (typed) wrapper for fleet_update_posture_snapshot.
/// Returns `ApplyError::TransportFailure` only when the dispatch IPC fails at the
/// transport level; other errors are `ApplyError::PolicyError`.
pub async fn fleet_update_posture_snapshot_typed() -> Result<serde_json::Value, ApplyError> {
    crate::license::require_service_feature("fleet").map_err(ApplyError::PolicyError)?;
    let settings = crate::settings::read_settings().map_err(ApplyError::PolicyError)?;
    let settings_hash = crate::settings::get_settings_hash().map_err(ApplyError::PolicyError)?;
    let settings_value = serde_json::to_value(&settings)
        .map_err(|e| ApplyError::PolicyError(format!("settings serialization failed: {e}")))?;
    let mut flat = Map::new();
    if let Some(ideal) = settings_value.get("ideal") {
        flatten_reportable_preferences("", ideal, &mut flat);
    }
    let args = serde_json::json!({
        "settingsHash": settings_hash,
        "toggleStates": Value::Object(flat),
    });
    crate::sidecar::dispatch_paid_command("fleet_agent_set_posture", args)
        .await
        .map_err(|e| {
            if is_transport_err(&e) {
                ApplyError::TransportFailure(e)
            } else {
                ApplyError::PolicyError(e)
            }
        })
}

fn should_report_preference_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    ![
        "pin",
        "hash",
        "phrase",
        "word",
        "key",
        "token",
        "secret",
        "path",
        "paths",
        "serverurl",
        "signingkeypub",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn flatten_reportable_preferences(prefix: &str, value: &Value, out: &mut Map<String, Value>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_reportable_preferences(&path, child, out);
            }
        }
        Value::Array(_) => {
            if !prefix.is_empty() && should_report_preference_path(prefix) {
                out.insert(prefix.to_string(), value.clone());
            }
        }
        Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Null => {
            if !prefix.is_empty() && should_report_preference_path(prefix) {
                out.insert(prefix.to_string(), value.clone());
            }
        }
    }
}

/// Push a redacted settings snapshot to Pro so the next fleet posture heartbeat
/// can include actual preference values for admin drift detection.
/// KT: the apply loop calls fleet_update_posture_snapshot_typed() for error
/// classification. This Tauri command exists for direct frontend/test invocation.
#[tauri::command]
pub async fn fleet_update_posture_snapshot() -> Result<serde_json::Value, String> {
    fleet_update_posture_snapshot_typed()
        .await
        .map_err(|e| e.to_string())
}

/// Send a privacy-shield *state* transition to the dedicated Fleet agent.
/// This payload intentionally carries no frame, image, application content, or
/// camera identifier. The Pro agent persists/delivers only this small status
/// record to the Fleet console.
#[tauri::command]
pub async fn fleet_report_privacy_shield_status(
    status: String,
    detail: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    let settings = crate::settings::read_settings()?;
    if !settings.app.fleet.enabled {
        return Err("Fleet is not enabled on this device.".to_string());
    }
    crate::sidecar::dispatch_paid_command(
        "fleet_agent_privacy_shield_status",
        serde_json::json!({
            "status": status,
            "detail": detail,
            "framesUploaded": false,
        }),
    )
    .await
}

/// Pull the Privacy Shield's admin-desired on/off + mode from the Fleet
/// server via the Pro sidecar's cached check-in state (`CheckinResponse
/// .shield_state`), and mirror it into local settings as
/// `app.fleet.shieldDesiredState` so the UI (PrivacyShieldCard's Stop-button
/// lock + alert-mode segmented toggle) can read it without its own poll
/// cycle. This is a SEPARATE, non-`policy_epoch`-versioned channel — see
/// `ShieldDesiredState`'s doc in `fleet-proto` — so calling this never
/// touches `policy.masterConfigVersion` or triggers a config-drift check.
/// Returns the resolved state (or `null` when the org has none / the device
/// isn't fleet-managed) so callers can react immediately without a settings
/// re-read.
#[tauri::command]
pub async fn fleet_sync_shield_state() -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    let settings = crate::settings::read_settings()?;
    if !settings.app.fleet.enabled {
        return Ok(serde_json::Value::Null);
    }
    let resp =
        crate::sidecar::dispatch_paid_command("fleet_agent_shield_state", serde_json::Value::Null)
            .await?;
    let state = resp.get("shieldState").cloned().unwrap_or(serde_json::Value::Null);
    crate::settings::patch_settings(serde_json::json!({
        "app": { "fleet": { "shieldDesiredState": state } }
    }))?;
    Ok(state)
}

/// Forward one local Windows-notification alert (screen-capture detected,
/// CPU/RAM/network threshold exceeded) to the Fleet console, carrying the
/// SAME concrete detail the local toast already showed. Callers MUST check
/// the corresponding `notifications.<type>.reportToFleet` setting before
/// calling this — this command does not re-check it, matching
/// `fleet_report_privacy_shield_status`'s division of responsibility (the
/// caller decides IF to report; this only decides HOW).
#[tauri::command]
pub async fn fleet_report_local_alert(
    alert_type: String,
    detail: serde_json::Value,
) -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    let settings = crate::settings::read_settings()?;
    if !settings.app.fleet.enabled {
        return Err("Fleet is not enabled on this device.".to_string());
    }
    crate::sidecar::dispatch_paid_command(
        "fleet_agent_local_alert",
        serde_json::json!({
            "alertType": alert_type,
            "detail": detail,
            "occurredAt": chrono::Utc::now().to_rfc3339(),
        }),
    )
    .await
}

/// Request to leave the fleet. Posts to the fleet server's unenroll-request
/// endpoint so an admin (two Operator+ admins under MPA) can approve the
/// departure. Idempotent — calling again while a pending request exists
/// returns the existing record, with fresh approval counts.  The device is
/// NOT disconnected yet; the admin must approve first (see `fleet_disconnect`).
///
/// Applies to every enrolled device, not just `policy.managed` ones — a
/// device is never allowed to silently drop off the fleet unnoticed; the
/// admin console must see and approve every departure.
#[tauri::command]
pub async fn fleet_request_unenroll() -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;

    // KT: delegates the HTTP call entirely to Pro, which authenticates the
    // unenroll request via the per-device HMAC checkin_secret it holds in
    // the AgentSession. Pro returns the server's JSON response.
    crate::sidecar::dispatch_paid_command("fleet_agent_request_unenroll", serde_json::Value::Null)
        .await
}

/// Read the current managed-unenroll status without re-submitting it. Once
/// approval revokes the device HMAC secret, Pro maps the status endpoint's 401
/// to `{ approved: true }` so the local cleanup can finish cleanly.
#[tauri::command]
pub async fn fleet_unenroll_status() -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    crate::sidecar::dispatch_paid_command("fleet_agent_unenroll_status", serde_json::Value::Null)
        .await
}

/// Disconnect this device from the fleet and persist enabled=false to settings.
/// The Pro agent task is aborted; no further heartbeats or command polls occur.
///
/// Succeeds only when the server has already approved a prior unenroll
/// request (the device's credentials are revoked server-side as part of
/// approval — checkin_secret nulled, so it can never authenticate or receive
/// commands again; a 401 on the unenroll-status poll is treated as implicit
/// approval).  Until approved the command returns an error so the UI can
/// direct the user to request approval via `fleet_request_unenroll` first.
#[tauri::command]
pub async fn fleet_disconnect() -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;

    // KT: Ask Pro to poll the server's unenroll-status endpoint.
    // Pro returns `{ "approved": bool }` or errors if the server is unreachable.
    // A 401 from the server means the token was already revoked (approval
    // happened and revoke ran) — treat that as approved too.
    let status_result = crate::sidecar::dispatch_paid_command(
        "fleet_agent_unenroll_status",
        serde_json::Value::Null,
    )
    .await;

    let approved = match &status_result {
        Ok(v) => {
            v.get("approved").and_then(|a| a.as_bool()).unwrap_or(false)
            // A 401 flag from Pro means the token is already revoked = approved.
            || v.get("token_revoked").and_then(|a| a.as_bool()).unwrap_or(false)
        }
        // Pro failed to reach the server — refuse to disconnect (fail-closed).
        Err(_) => false,
    };

    if !approved {
        // KT: don't always blame "admin approval required" — that specific
        // message is only true when Pro reached the server and it said
        // not-yet-approved. Pro can also fail here because it's mid
        // auto-reconnect (a fresh sidecar process hasn't re-established its
        // AgentSession/checkin_secret yet — see fleet_push::request_unenroll's
        // "not connected — enroll first") or because the sidecar is simply
        // unreachable. Both are transient and have nothing to do with
        // approval status, so a distinct message points the user at
        // "retry" instead of "wait on your admin".
        if let Err(e) = &status_result {
            if e.contains("not connected — enroll first") {
                return Err(
                    "Fleet agent is still reconnecting to the server after a restart — wait a few seconds and try Disconnect again."
                        .to_string(),
                );
            }
            if is_transport_err(e) {
                return Err(
                    "Could not reach the Pro sidecar to check unenroll approval — try again."
                        .to_string(),
                );
            }
        }
        return Err(
            "Admin approval is required to disconnect this device. Use 'Request to leave' to submit a request."
                .to_string(),
        );
    }
    // Approved: proceed with disconnect below.

    // Persist the disabled flag so the agent stays off on next launch.
    let patch = serde_json::json!({
        "app": { "fleet": { "enabled": false, "privacyShieldSessionOwned": false } },
        "ideal": { "privacy": { "privacyShield": {
            "fleetManaged": false,
            "fleetMonitoringEnabled": false,
        } } }
    });
    crate::settings::patch_settings(patch)?;

    crate::sidecar::dispatch_paid_command("fleet_agent_disconnect", serde_json::Value::Null).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn every_fleet_entry_path_uses_the_explicit_service_gate() {
        let source = include_str!("fleet_agent.rs");
        let service_gate = ["require_service_feature(\"", "fleet\")"].concat();
        let obsolete_gate = ["require", "_paid(\"fleet agent\")"].concat();
        assert_eq!(
            source.matches(&service_gate).count(),
            8,
            "connect, status, policy apply, posture, privacy-shield status, unenroll request/status, and disconnect must all require Fleet"
        );
        assert!(!source.contains(&obsolete_gate));
    }

    #[test]
    fn startup_and_periodic_fleet_paths_require_the_service_feature() {
        let source = include_str!("lib.rs");
        let fleet_check = ["require_service_feature(\"", "fleet\")"].concat();
        assert_eq!(
            source.matches(&fleet_check).count(),
            2,
            "startup auto-connect and periodic apply must both stop without active Fleet service"
        );
    }

    /// Verify the JSON shape sent to Pro for configure matches what FleetConfig
    /// in fleet_push.rs expects (camelCase serde rename).
    #[test]
    fn configure_args_json_has_camel_case_keys() {
        let server_url = "http://fleet.example.internal:8787";
        let dispatch = true;
        let signing_key_pub = "AAABBBCCC";

        let args = json!({
            "serverUrl": server_url,
            "dispatch": dispatch,
            "signingKeyPub": signing_key_pub,
            "deviceId": "stable-device-uuid",
        });

        assert_eq!(args["serverUrl"], server_url);
        assert_eq!(args["dispatch"], dispatch);
        assert_eq!(args["signingKeyPub"], signing_key_pub);
        assert_eq!(args["deviceId"], "stable-device-uuid");
        // Ensure no snake_case keys accidentally leaked.
        assert!(args.get("server_url").is_none());
        assert!(args.get("signing_key_pub").is_none());
    }

    #[test]
    fn fleet_server_url_repairs_a_single_accidental_extra_slash() {
        assert_eq!(
            super::normalize_fleet_server_url(" http:///10.10.111.145:8787/ ").unwrap(),
            "http://10.10.111.145:8787"
        );
        assert_eq!(
            super::normalize_fleet_server_url("https://fleet.example.internal").unwrap(),
            "https://fleet.example.internal"
        );
    }

    #[test]
    fn fleet_server_url_rejects_non_origin_addresses() {
        for raw in [
            "",
            "ftp://fleet.example.internal",
            "http://fleet.example.internal/api",
            "http://user@fleet.example.internal",
            "http://fleet.example.internal?next=elsewhere",
        ] {
            assert!(super::normalize_fleet_server_url(raw).is_err(), "{raw}");
        }
    }

    /// Verify the settings patch shape for connect: nested app.fleet with
    /// enabled=true and all fields present.
    #[test]
    fn connect_patch_shape_is_correct() {
        let patch = json!({
            "app": {
                "fleet": {
                    "enabled": true,
                    "serverUrl": "http://fleet.example.internal:8787",
                    "dispatch": false,
                    "signingKeyPub": "",
                }
            }
        });

        assert!(patch["app"]["fleet"]["enabled"].as_bool().unwrap());
        assert!(patch["app"]["fleet"]["serverUrl"].as_str().is_some());
        assert!(patch["app"]["fleet"]["signingKeyPub"].as_str().is_some());
    }

    fn remote_update(
        catalog_id: &str,
        payload: serde_json::Value,
    ) -> super::RemoteCommandStateUpdate {
        super::RemoteCommandStateUpdate {
            command_id: "cmd-test".to_string(),
            catalog_id: catalog_id.to_string(),
            payload,
        }
    }

    #[test]
    fn fleet_privacy_toggle_updates_ideal_and_current_together() {
        let patch = super::remote_command_settings_patch(&remote_update(
            "privacy.telemetry.disable",
            json!({}),
        ))
        .expect("telemetry is a supported remote toggle");

        assert_eq!(
            patch["ideal"]["privacy"]["telemetry"]["windowsDisabled"],
            false
        );
        assert_eq!(
            patch["current"]["privacy"]["telemetry"]["windowsDisabled"],
            false
        );

        let enabled = super::remote_command_settings_patch(&remote_update(
            "privacy.telemetry.enable",
            json!({}),
        ))
        .unwrap();
        assert_eq!(
            enabled["ideal"]["privacy"]["telemetry"]["windowsDisabled"],
            true
        );
        assert_eq!(
            enabled["current"]["privacy"]["telemetry"]["windowsDisabled"],
            true
        );
    }

    #[test]
    fn fleet_capability_and_inverted_defender_semantics_are_mirrored() {
        let capability = super::remote_command_settings_patch(&remote_update(
            "capability.set",
            json!({ "Capability": "microphone", "Access": "Deny" }),
        ))
        .unwrap();
        assert_eq!(
            capability["ideal"]["privacy"]["appCapabilities"]["microphone"],
            "Deny"
        );
        assert_eq!(
            capability["current"]["privacy"]["appCapabilities"]["microphone"],
            "Deny"
        );

        let defender =
            super::remote_command_settings_patch(&remote_update("defender.enable", json!({})))
                .unwrap();
        assert_eq!(
            defender["ideal"]["tweaks"]["security"]["defenderDisabled"],
            false
        );
        assert_eq!(
            defender["current"]["tweaks"]["security"]["defenderDisabled"],
            false
        );
    }

    #[test]
    fn remote_patch_merge_keeps_the_latest_command_state() {
        let mut merged = json!({});
        let enabled = super::remote_command_settings_patch(&remote_update(
            "usb.writeprotect.enable",
            json!({}),
        ))
        .unwrap();
        let disabled = super::remote_command_settings_patch(&remote_update(
            "usb.writeprotect.disable",
            json!({}),
        ))
        .unwrap();
        super::merge_remote_patch(&mut merged, &enabled);
        super::merge_remote_patch(&mut merged, &disabled);

        assert_eq!(
            merged["ideal"]["tweaks"]["security"]["usbWriteProtect"],
            false
        );
        assert_eq!(
            merged["current"]["tweaks"]["security"]["usbWriteProtect"],
            false
        );
        assert!(
            super::remote_command_settings_patch(&remote_update("status.read", json!({})))
                .is_none()
        );
    }

    #[test]
    fn fleet_screen_capture_commands_update_the_privacy_toggle() {
        let started = super::remote_command_settings_patch(&remote_update(
            "monitoring.screen_capture.start",
            json!({}),
        ))
        .expect("screen-capture start must be mirrored locally");
        assert_eq!(
            started["ideal"]["privacy"]["screenCapture"]["detectionEnabled"],
            true
        );
        assert_eq!(
            started["current"]["privacy"]["screenCapture"]["detectionEnabled"],
            true
        );

        let stopped = super::remote_command_settings_patch(&remote_update(
            "monitoring.screen_capture.stop",
            json!({}),
        ))
        .expect("screen-capture stop must be mirrored locally");
        assert_eq!(
            stopped["ideal"]["privacy"]["screenCapture"]["detectionEnabled"],
            false
        );
        assert_eq!(
            stopped["current"]["privacy"]["screenCapture"]["detectionEnabled"],
            false
        );
    }

    /// Verify disconnect patch sets only enabled=false (minimal merge-safe patch).
    #[test]
    fn disconnect_patch_sets_enabled_false() {
        let patch = json!({ "app": { "fleet": { "enabled": false } } });
        assert!(!patch["app"]["fleet"]["enabled"].as_bool().unwrap());
        // Must not contain other keys that would accidentally clear the URL.
        assert!(patch["app"]["fleet"].get("serverUrl").is_none());
    }

    /// FleetStatus default is disconnected with empty strings.
    #[test]
    fn fleet_status_default_is_disconnected() {
        let s = super::FleetStatus::default();
        assert!(!s.connected);
        assert!(s.device_id.is_empty());
        assert!(s.server_url.is_empty());
        assert!(s.last_enroll_at.is_none());
        assert!(s.last_error.is_none());
    }

    #[test]
    fn posture_snapshot_filters_sensitive_paths() {
        assert!(super::should_report_preference_path(
            "privacy.telemetry.windowsDisabled"
        ));
        assert!(!super::should_report_preference_path(
            "privacy.startupPin.realHash"
        ));
        assert!(!super::should_report_preference_path(
            "privacy.coercionPhrase.phrases"
        ));
        assert!(!super::should_report_preference_path(
            "app.fleet.signingKeyPub"
        ));
    }

    /// Transport-error strings from dispatch_paid_command must be classified
    /// as TransportFailure. Semantic Pro errors ([pro:...]) and policy errors
    /// (no key, bad sig) must NOT be classified as transport.
    #[test]
    fn is_transport_err_classifies_correctly() {
        // Transport failures — these increment fail_streak and latch pro_unreachable.
        assert!(
            super::is_transport_err("Pro transport error: pipe is being closed (os error 232)"),
            "stale-pipe transport error must be classified as transport"
        );
        assert!(
            super::is_transport_err("Pro spawn failed after transport error: access denied"),
            "spawn failure must be classified as transport"
        );
        assert!(
            super::is_transport_err("Pro response timeout"),
            "timeout must be classified as transport"
        );
        assert!(
            super::is_transport_err("Pro reader exited before responding"),
            "reader-exit must be classified as transport"
        );
        assert!(
            super::is_transport_err("PRO_NOT_INSTALLED:WinCommander Pro is not installed."),
            "pro-not-installed must be classified as transport (Pro unreachable)"
        );

        // Policy/application errors — Pro IS reachable; must NOT be transport.
        assert!(
            !super::is_transport_err("cannot apply fleet policy: no pinned fleet signing key"),
            "no-key policy error must NOT be classified as transport"
        );
        assert!(
            !super::is_transport_err("config push signature verification failed"),
            "signature verification failure must NOT be classified as transport"
        );
        assert!(
            !super::is_transport_err("[pro:fleet] device not enrolled"),
            "semantic Pro error must NOT be classified as transport"
        );
    }

    /// ApplyError::is_transport() correctly distinguishes the two variants.
    #[test]
    fn apply_error_is_transport_variant_check() {
        let transport = super::ApplyError::TransportFailure("pipe dead".to_string());
        let policy = super::ApplyError::PolicyError("no pinned key".to_string());
        assert!(
            transport.is_transport(),
            "TransportFailure must report is_transport=true"
        );
        assert!(
            !policy.is_transport(),
            "PolicyError must report is_transport=false"
        );
    }
}
