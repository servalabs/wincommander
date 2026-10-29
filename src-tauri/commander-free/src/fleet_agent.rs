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
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, RwLock};
use url::Url;
use wincmd_shared::diagnostics::{
    DiagnosticEvent, DiagnosticLifecycle, DiagnosticOutcome, DiagnosticPrivacyClass,
    DiagnosticRetryability, DiagnosticSeverity,
};
use wincmd_shared::fleet::{
    ClipboardEventReport, ShieldNativeOptions, ShieldPolicyProgress, ShieldPolicyProgressReason,
    ShieldPolicyProgressStage, ShieldStoredSettings,
};

fn record_fleet_lifecycle(
    operation_id: &str,
    lifecycle: DiagnosticLifecycle,
    outcome: DiagnosticOutcome,
    stage: &str,
    error_code: Option<&str>,
) {
    let stamp = chrono::Utc::now();
    let _ = crate::diagnostics::record(DiagnosticEvent {
        event_id: format!("evt-fleet-{}", stamp.timestamp_millis()),
        operation_id: operation_id.to_string(),
        parent_operation_id: None,
        occurred_at: stamp.to_rfc3339(),
        component: "fleet_agent".into(),
        feature: "fleet".into(),
        action: "policy_sync".into(),
        stage: stage.into(),
        lifecycle,
        outcome,
        error_code: error_code.map(str::to_string),
        severity: if error_code.is_some() {
            DiagnosticSeverity::Warn
        } else {
            DiagnosticSeverity::Info
        },
        retryability: if error_code.is_some() {
            DiagnosticRetryability::Automatic
        } else {
            DiagnosticRetryability::Never
        },
        suggested_next_action: if error_code.is_some() {
            "retry".into()
        } else {
            "none".into()
        },
        duration_ms: None,
        privacy_class: DiagnosticPrivacyClass::LocalSensitive,
        redacted_context: BTreeMap::from([("state".into(), stage.into())]),
    });
}

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

static FLEET_CONNECT_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    fleet_connect_inner(app, server_url, dispatch, signing_key_pub, confirmation, None).await
}

/// Repair a missing service trust record for this already-linked device.
/// The caller cannot supply another server, key or device identity. A captured
/// administrator token and a native confirmation are required before the
/// same-device enrollment challenge is requested. This path never asks Fleet
/// to revoke, unenroll, or disconnect the device.
#[tauri::command]
pub async fn fleet_repair_machine_trust(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    let current = crate::settings::read_settings()?;
    if !current.app.fleet.enabled
        || current.app.fleet.server_url.is_empty()
        || current.app.fleet.signing_key_pub.is_empty()
    {
        return Err("This Windows profile has no linked Fleet identity to repair".into());
    }
    let plan = crate::settings::fleet_enrollment::ConnectionPlan::new(
        &current,
        normalize_fleet_server_url(&current.app.fleet.server_url)?,
        current.app.fleet.dispatch,
        current.app.fleet.signing_key_pub.clone(),
    )?;
    if plan.needs_confirmation() {
        return Err("Fleet identity has changed; use the full confirmed Connect flow".into());
    }
    let prior = crate::svc_client::call("svc.fleet.get_trust", serde_json::json!({})).await?;
    if prior.get("trust").is_none() {
        return Err("Fleet service trust response is incomplete".into());
    }
    if !prior["trust"].is_null() {
        return Err("Machine Fleet trust is already provisioned".into());
    }
    if !crate::authz::native_confirm_fleet_trust_repair(&app).await {
        return Err("Machine trust repair was cancelled".into());
    }
    fleet_connect_inner(
        app,
        current.app.fleet.server_url,
        current.app.fleet.dispatch,
        current.app.fleet.signing_key_pub,
        None,
        Some(plan.confirmation_binding()),
    )
    .await
}

/// Read-only machine trust visibility for the already-linked device card.
/// The service supplies only a boolean, never the signing key or proof.
#[tauri::command]
pub async fn fleet_machine_trust_status() -> Result<serde_json::Value, String> {
    let response = crate::svc_client::call("svc.fleet.get_policy", serde_json::json!({})).await?;
    let trust_bound = response.get("trust_bound").and_then(Value::as_bool)
        .ok_or("Fleet service trust status is unavailable")?;
    Ok(serde_json::json!({"trustBound": trust_bound}))
}

fn repair_connection_matches(
    plan: &crate::settings::fleet_enrollment::ConnectionPlan,
    expected_binding: &str,
) -> bool {
    !plan.needs_confirmation() && plan.confirmation_binding() == expected_binding
}

async fn fleet_connect_inner(
    app: tauri::AppHandle,
    server_url: String,
    dispatch: bool,
    signing_key_pub: String,
    confirmation: Option<String>,
    repair_expected_binding: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    let _connection = FLEET_CONNECT_GATE
        .try_lock()
        .map_err(|_| "A Fleet connection change is already in progress".to_string())?;
    let server_url = normalize_fleet_server_url(&server_url)?;
    // A failed read cannot prove this device is unenrolled.
    let current = crate::settings::read_settings()?;
    let plan = crate::settings::fleet_enrollment::ConnectionPlan::new(
        &current,
        server_url,
        dispatch,
        signing_key_pub.clone(),
    )?;
    if repair_expected_binding.as_deref().is_some_and(|binding| !repair_connection_matches(&plan, binding)) {
        return Err("Fleet identity changed during machine trust repair; retry".into());
    }
    let confirmed = if plan.needs_confirmation() {
        let confirmed = match confirmation.as_deref() {
            Some(token) => crate::authz::consume(
                token,
                crate::authz::DestructiveAction::FleetReenroll,
                &plan.confirmation_binding(),
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
        if !confirmed {
            return Err("Re-enrolling this device or changing managed dispatch requires native confirmation.".into());
        }
        true
    } else {
        false
    };
    // A previously interrupted first enrollment can leave app settings
    // committed while service trust is still absent. An elevated reconnect
    // may finish that provision, but ordinary reconnect never replaces an
    // established machine binding.
    let prior = if plan.needs_service_challenge() || repair_expected_binding.is_some() {
        Some(crate::svc_client::call("svc.fleet.get_trust", serde_json::json!({})).await?)
    } else {
        crate::svc_client::call("svc.fleet.get_trust", serde_json::json!({})).await.ok()
    };
    if prior.as_ref().is_some_and(|reply| reply.get("trust").is_none()) {
        return Err("Fleet service trust response is incomplete".into());
    }
    let previous_trust = prior.as_ref()
        .and_then(|reply| reply.get("trust"))
        .filter(|value| !value.is_null())
        .cloned();
    if repair_expected_binding.is_some() && previous_trust.is_some() {
        return Err("Machine Fleet trust changed during repair; retry".into());
    }
    let needs_challenge = plan.needs_service_challenge()
        || (prior.is_some() && previous_trust.is_none());
    let service_challenge = if needs_challenge {
        if previous_trust.is_some() && !plan.allows_trust_replacement() {
            return Err("This machine already has Fleet trust; a confirmed re-enrollment is required".into());
        }
        let challenge_reply = crate::svc_client::call(
            "svc.fleet.begin_enrollment",
            serde_json::json!({
                "device_id": current.device_id,
                "signer_key": signing_key_pub,
            }),
        )
        .await?;
        let challenge = challenge_reply
            .get("challenge")
            .and_then(Value::as_str)
            .filter(|value| uuid::Uuid::parse_str(value).ok().is_some_and(|id| id.to_string() == *value))
            .ok_or("Fleet service enrollment challenge is invalid")?
            .to_string();
        Some((challenge, previous_trust))
    } else {
        None
    };
    // Recheck the observed authority under the settings lock before persisting.
    let committed = plan.commit(confirmed)?;
    let mut args = serde_json::json!({
        "serverUrl": committed.app.fleet.server_url,
        "dispatch": committed.app.fleet.dispatch,
        "signingKeyPub": committed.app.fleet.signing_key_pub,
        "deviceId": committed.device_id,
    });
    if let Some((challenge, _)) = &service_challenge {
        args["shieldProgressChallenge"] = Value::String(challenge.clone());
    }
    let status = crate::sidecar::dispatch_paid_command("fleet_agent_configure", args).await?;
    if let Some((_, previous_trust)) = service_challenge {
        establish_service_enrollment_trust(
            &committed.device_id,
            &signing_key_pub,
            previous_trust,
            plan.allows_trust_replacement() && confirmed,
        )
        .await?;
    }
    Ok(status)
}

fn service_enrollment_action(
    previous: Option<&Value>,
    replacement: &Value,
    confirmed_identity_change: bool,
) -> Result<Option<&'static str>, String> {
    match previous {
        None => Ok(Some("svc.fleet.provision_trust")),
        Some(previous) if previous == replacement => Ok(None),
        Some(_) if confirmed_identity_change => Ok(Some("svc.fleet.replace_trust")),
        Some(_) => Err("A confirmed Fleet identity change is required to replace machine trust".into()),
    }
}

async fn establish_service_enrollment_trust(
    device_id: &str,
    signer_key: &str,
    previous_trust: Option<Value>,
    confirmed_identity_change: bool,
) -> Result<(), String> {
    for _ in 0..40 {
        if let Ok(proof) = crate::sidecar::dispatch_paid_command(
            "fleet_agent_enrollment_trust", Value::Null,
        ).await {
            if let Some(replacement) = proof.get("trust") {
                if replacement.get("device_id").and_then(Value::as_str) == Some(device_id)
                    && replacement.get("signer_key").and_then(Value::as_str) == Some(signer_key)
                    && replacement.get("org_id").and_then(Value::as_str).is_some_and(|v| !v.is_empty())
                    && replacement.get("enrollment_generation").and_then(Value::as_str)
                        .is_some_and(|v| uuid::Uuid::parse_str(v).is_ok())
                    && proof.get("signature").and_then(Value::as_str).is_some()
                {
                    match service_enrollment_action(
                        previous_trust.as_ref(), replacement, confirmed_identity_change,
                    )? {
                        None => return Ok(()),
                        Some("svc.fleet.provision_trust") => {
                            if crate::svc_client::call("svc.fleet.provision_trust", proof.clone()).await.is_ok() {
                                return Ok(());
                            }
                        }
                        Some("svc.fleet.replace_trust") => {
                            if crate::svc_client::call("svc.fleet.replace_trust", serde_json::json!({
                                "previous": previous_trust,
                                "replacement": replacement,
                                "signature": proof["signature"],
                            })).await.is_ok() {
                                return Ok(());
                            }
                        }
                        Some(_) => unreachable!(),
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err("Fleet enrollment could not establish machine trust; retry after the agent connects".into())
}

/// Query the running fleet agent's status (connected, deviceId, lastEnrollAt,
/// lastError). Purely a status read — no side effects, no settings writes.
///
/// This intentionally is not a service-operation gate. The enrolled device's
/// agent session is shared machine state, whereas the UI's app.fleet setting
/// belongs to the signed-in Windows user. An administrator needs truthful
/// read-only link state even when that user's settings contain no Fleet block.
/// It exposes no check-in secret, signing key, or configuration mutation.
/// When Pro is unreachable, falls back to a synthetic status that surfaces the
/// specific transport error so the frontend never shows stale/empty state silently.
#[tauri::command]
pub async fn fleet_status() -> Result<serde_json::Value, String> {
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
    // A Windows named mutex is thread-owned. Acquire, run the async policy
    // transaction, and release on the same blocking thread. This serializes
    // epoch commit with the Shield supervisor across Free processes.
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _guard = crate::paths::acquire_machine_state_lock("fleet-shield-policy")
            .map_err(ApplyError::PolicyError)?;
        runtime.block_on(fleet_apply_pending_epoch_inner())
    })
    .await
    .map_err(|error| ApplyError::PolicyError(format!("Shield policy worker failed: {error}")))?
}

async fn fleet_apply_pending_epoch_inner() -> Result<serde_json::Value, ApplyError> {
    let operation_id = format!("FLEET-{}", chrono::Utc::now().timestamp_millis());
    record_fleet_lifecycle(
        &operation_id,
        DiagnosticLifecycle::Requested,
        DiagnosticOutcome::Started,
        "requested",
        None,
    );
    crate::license::require_service_feature("fleet").map_err(ApplyError::PolicyError)?;

    // The dispatch call is the ONLY place a transport failure can originate.
    let resp =
        crate::sidecar::dispatch_paid_command("fleet_agent_pending_epoch", serde_json::Value::Null)
            .await
            .map_err(|e| {
                record_fleet_lifecycle(
                    &operation_id,
                    DiagnosticLifecycle::Delivered,
                    DiagnosticOutcome::Failed,
                    "delivery",
                    Some("FLT.DELIVERY.FAILED"),
                );
                // KT: classify by error string; see is_transport_err for the heuristics.
                if is_transport_err(&e) {
                    ApplyError::TransportFailure(e)
                } else {
                    // Semantic Pro error ([pro:...]) — pipe still alive, not a transport failure.
                    ApplyError::PolicyError(e)
                }
            })?;
    record_fleet_lifecycle(
        &operation_id,
        DiagnosticLifecycle::Delivered,
        DiagnosticOutcome::Progress,
        "delivered",
        None,
    );
    // The sidecar has returned an authenticated Fleet response. This is a
    // receipt only; it is deliberately separate from Windows application and
    // later verification below.
    record_fleet_lifecycle(
        &operation_id,
        DiagnosticLifecycle::Acknowledged,
        DiagnosticOutcome::Progress,
        "acknowledged",
        None,
    );

    let epoch = resp
        .get("epoch")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if epoch.is_null() {
        let remote_updates_applied = apply_remote_command_updates(&resp).await?;
        record_fleet_lifecycle(
            &operation_id,
            DiagnosticLifecycle::Verified,
            DiagnosticOutcome::Succeeded,
            "verified",
            None,
        );
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
        record_fleet_lifecycle(
            &operation_id,
            DiagnosticLifecycle::Verified,
            DiagnosticOutcome::Succeeded,
            "verified",
            None,
        );
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
        record_fleet_lifecycle(
            &operation_id,
            DiagnosticLifecycle::Acknowledged,
            DiagnosticOutcome::Failed,
            "authorization",
            Some("FLT.SIGNING_KEY.MISSING"),
        );
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

    let config_version = u32::try_from(version).map_err(|_| {
        ApplyError::PolicyError("Fleet policy version is outside the supported range".into())
    })?;
    // Verify the exact signed candidate before any native pause. The commit
    // below independently repeats this validation under the settings lock.
    let preview = crate::settings::preview_verified_admin_config(
        config.clone(),
        locked_paths.clone(),
        "merge".to_string(),
        config_version,
        signature.clone(),
        signer_key.clone(),
        target_kind.clone(),
        target_id.clone(),
        Some(managed),
    )
    .map_err(ApplyError::PolicyError)?;
    let epoch_signature_hash = {
        use sha2::Digest;
        let signed = signature.as_deref().ok_or_else(|| {
            ApplyError::PolicyError("Shield policy epoch has no signature".into())
        })?;
        hex::encode(sha2::Sha256::digest(signed.as_bytes()))
    };
    let shield_changed = serde_json::to_value(&preview.ideal.privacy.privacy_shield).ok()
        != serde_json::to_value(&settings.ideal.privacy.privacy_shield).ok();
    let shield_policy_envelope = if shield_changed
        || settings.app.fleet.privacy_shield_policy_transaction.is_some()
    {
        let envelope: wincmd_shared::fleet::PolicyEnvelope = serde_json::from_value(
            resp.get("policyEnvelope").cloned().unwrap_or(Value::Null),
        )
        .map_err(|_| ApplyError::PolicyError("signed Shield policy envelope is unavailable".into()))?;
        let pinned = settings.policy.fleet_signing_key.as_deref().ok_or_else(||
            ApplyError::PolicyError("Fleet signing key is unavailable".into()))?;
        if envelope.signer_key != pinned
            || envelope.device_id.0 != settings.device_id
            || !wincmd_shared::fleet::verify_signature_b64(pinned, &envelope.preimage(), &envelope.signature)
            || serde_json::to_value(envelope.config.as_ref()).ok() != Some(epoch.clone())
            || envelope.enrollment_generation.as_ref().is_none_or(|generation| uuid::Uuid::parse_str(generation).ok().is_none_or(|id| id.to_string() != *generation))
            || envelope.config_revision.is_none_or(|revision| revision < i64::from(config_version))
            || envelope.resolution_revision.is_none_or(|revision| revision < 0)
        {
            return Err(ApplyError::PolicyError("signed Shield policy envelope failed device binding".into()));
        }
        install_service_shield_policy(&envelope, pinned)
            .await.map_err(ApplyError::PolicyError)?;
        Some(envelope)
    } else {
        None
    };
    let mut shield_transaction = if shield_changed
        || settings
            .app
            .fleet
            .privacy_shield_policy_transaction
            .as_ref()
            .is_some_and(|tx| tx.epoch_version == config_version)
    {
        Some(
            prepare_shield_policy_transaction(
                &settings,
                &preview,
                config_version,
                &epoch_signature_hash,
                shield_policy_envelope.as_ref().and_then(|envelope| envelope.resolution_revision)
                    .ok_or_else(|| ApplyError::PolicyError("signed Shield resolution revision is unavailable".into()))?,
            )
            .await
            .map_err(ApplyError::PolicyError)?,
        )
    } else {
        None
    };

    // apply_verified_admin_config can fail on signature verification — also a PolicyError.
    record_fleet_lifecycle(
        &operation_id,
        DiagnosticLifecycle::Applying,
        DiagnosticOutcome::Progress,
        "applying",
        None,
    );
    let updated_result = crate::settings::apply_verified_admin_config(
        config,
        locked_paths,
        "merge".to_string(),
        config_version,
        signature,
        signer_key,
        target_kind,
        target_id,
        Some(managed),
    );
    let updated = match updated_result {
        Ok(updated) => updated,
        Err(error) => {
            record_fleet_lifecycle(
                &operation_id,
                DiagnosticLifecycle::Applying,
                DiagnosticOutcome::Failed,
                "applying",
                Some("FLT.POLICY.APPLY_FAILED"),
            );
            if let Some(transaction) = &mut shield_transaction {
                advance_shield_policy_progress(
                    transaction,
                    ShieldPolicyProgressStage::Failed,
                    Some(ShieldPolicyProgressReason::PolicyApplyFailed),
                )
                .await
                .map_err(ApplyError::PolicyError)?;
            }
            return Err(ApplyError::PolicyError(error));
        }
    };
    if let Some(transaction) = &mut shield_transaction {
        advance_shield_policy_progress(
            transaction,
            ShieldPolicyProgressStage::SettingsApplied,
            None,
        )
        .await
        .map_err(ApplyError::PolicyError)?;
    }
    record_fleet_lifecycle(
        &operation_id,
        DiagnosticLifecycle::Applied,
        DiagnosticOutcome::Succeeded,
        "applied",
        None,
    );

    // A cached epoch may predate a command that executed moments ago. Apply
    // the verified command handoff last so it is the deterministic last writer
    // and cannot be immediately overwritten by older policy intent.
    let remote_updates_applied = apply_remote_command_updates(&resp).await?;
    record_fleet_lifecycle(
        &operation_id,
        DiagnosticLifecycle::Verified,
        DiagnosticOutcome::Succeeded,
        "verified",
        None,
    );

    Ok(serde_json::json!({
        "applied": true,
        "version": version,
        "remoteUpdatesApplied": remote_updates_applied,
        "settings": updated,
    }))
}

async fn install_service_shield_policy(
    envelope: &wincmd_shared::fleet::PolicyEnvelope,
    pinned: &str,
) -> Result<(), String> {
    let args = serde_json::json!({"policy": envelope});
    if crate::svc_client::call("svc.fleet.install_policy", args.clone()).await.is_ok() {
        return Ok(());
    }
    // A first enrollment may have a verified Pro proof but no provisioned
    // service trust yet. The SYSTEM service independently validates the
    // service-issued one-use challenge, signature and captured admin token.
    let proof = crate::sidecar::dispatch_paid_command(
        "fleet_agent_enrollment_trust", Value::Null,
    ).await?;
    let trust = proof.get("trust").ok_or("Fleet enrollment trust is unavailable")?;
    let generation = envelope.enrollment_generation.as_deref().ok_or(
        "signed Shield enrollment generation is unavailable",
    )?;
    if trust.get("device_id").and_then(Value::as_str) != Some(envelope.device_id.0.as_str())
        || trust.get("org_id").and_then(Value::as_str) != Some(envelope.org_id.0.as_str())
        || trust.get("signer_key").and_then(Value::as_str) != Some(pinned)
        || trust.get("enrollment_generation").and_then(Value::as_str) != Some(generation)
        || proof.get("signature").and_then(Value::as_str).is_none()
    {
        return Err("Fleet enrollment proof does not match the signed device policy".into());
    }
    crate::svc_client::call("svc.fleet.provision_trust", proof).await?;
    crate::svc_client::call("svc.fleet.install_policy", args).await?;
    Ok(())
}

/// Each Windows profile has its own Shield preferences, while the Fleet epoch
/// is machine-scoped. Refuse native Start when this profile has not adopted
/// the service's current signed projection; another user's epoch write must
/// not make stale local options look current.
pub(crate) async fn verify_service_shield_profile() -> Result<(), String> {
    let settings = crate::settings::read_settings()?;
    let response = crate::svc_client::call("svc.fleet.get_policy", serde_json::json!({}))
        .await
        .map_err(|_| "WinCommander service is unavailable; Shield cannot verify this machine's Fleet policy".to_string())?;
    verify_shield_profile_against_service(&settings, &response)
}

fn verify_shield_profile_against_service(
    settings: &crate::settings::AppSettings,
    response: &Value,
) -> Result<(), String> {
    let managed = response.get("trust_bound").and_then(Value::as_bool)
        .ok_or("service Fleet trust state is unavailable")?;
    if !managed {
        if response.get("policy").is_some_and(|value| !value.is_null())
            || settings.policy.fleet_signing_key.as_deref().is_some_and(|key| !key.is_empty())
            || settings.policy.master_config_version.unwrap_or(0) > 0
        {
            return Err("service and local Fleet trust state disagree".into());
        }
        return Ok(());
    }
    let pinned = settings.policy.fleet_signing_key.as_deref()
        .filter(|key| !key.is_empty())
        .ok_or("this Windows profile lacks the machine Fleet signing key")?;
    if settings.policy.master_config_version.unwrap_or(0) == 0 {
        return Err("this Windows profile lacks the current Fleet epoch".into());
    }
    let envelope: wincmd_shared::fleet::PolicyEnvelope = serde_json::from_value(
        response.get("policy").cloned().unwrap_or(Value::Null),
    ).map_err(|_| "service Shield policy is unavailable".to_string())?;
    if envelope.signer_key != pinned
        || envelope.device_id.0 != settings.device_id
        || envelope.enrollment_generation.is_none()
        || !wincmd_shared::fleet::verify_signature_b64(pinned, &envelope.preimage(), &envelope.signature)
    {
        return Err("service Shield policy identity is not verified".into());
    }
    let Some(epoch) = envelope.config.as_ref() else {
        return Ok(());
    };
    if epoch.version != i64::from(settings.policy.master_config_version.unwrap_or(0)) {
        return Err("this Windows profile has not adopted the current Fleet epoch".into());
    }
    let Some(signed_shield) = epoch.config_json.pointer("/privacy/privacyShield") else {
        return Ok(());
    };
    let signed_fields = signed_shield.as_object().ok_or("signed Shield policy is malformed")?;
    let stored = serde_json::to_value(&settings.ideal.privacy.privacy_shield)
        .map_err(|_| "stored Shield settings are unavailable")?;
    if signed_fields.iter().any(|(key, value)| stored.get(key) != Some(value)) {
        return Err("this Windows profile retains stale Shield settings".into());
    }
    Ok(())
}

/// Fetch the latest signed policy epoch the Pro agent has pulled, verify it
/// against the pinned fleet key, and apply it (values + locked paths) via the
/// signature-checking `apply_verified_admin_config`. This is the Free side of the
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
    app: tauri::AppHandle,
    status: String,
    detail: Option<String>,
    command_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // This is a device-status receipt, not a per-user Fleet operation. The
    // authenticated Pro agent owns delivery and device identity; a stale
    // Windows-user settings cache must not hide a running local detector or
    // leave a Fleet command applying forever.
    // A running receipt is not enough to prove a mode-sensitive Fleet Start.
    // Re-read the detector's own applied command-line state at the bridge so
    // both the headless native supervisor and the WebView sender carry the
    // same bounded truth. `None` is intentionally preserved when WMI cannot
    // expose the process command line: Pro/Fleet will then refuse Verified
    // rather than guessing from desired state or settings cache.
    let mode = if matches!(
        status.as_str(),
        "running_fleet_session" | "running_local_session" | "running" | "started"
    ) {
        privacy_shield_status(&app)
            .await
            .ok()
            .and_then(|read_back| fleet_shield_reported_mode(&read_back))
    } else {
        None
    };
    crate::sidecar::dispatch_paid_command(
        "fleet_agent_privacy_shield_status",
        serde_json::json!({
            "status": status,
            "detail": detail,
            "commandId": command_id,
            "mode": mode,
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
    // The signed state comes from the machine-scoped Pro agent. Do not use a
    // profile-local `app.fleet.enabled` cache as an authority check: a second
    // Windows user can be Not Linked while the device agent is connected.
    let resp =
        crate::sidecar::dispatch_paid_command("fleet_agent_shield_state", serde_json::Value::Null)
            .await?;
    let state = resp
        .get("shieldState")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    crate::settings::patch_settings(serde_json::json!({
        "app": { "fleet": { "shieldDesiredState": state } }
    }))?;
    Ok(state)
}

/// Run the device-side Fleet Privacy Shield policy loop independently of the
/// WebView.  A scheduled/tray WinCommander instance deliberately has no main
/// window, so a React-only supervisor would otherwise receive a signed policy
/// but never apply it.  The policy itself remains authenticated by Pro; this
/// loop only consumes Pro's cached, verified desired state and uses the same
/// backend command boundary as the Privacy Shield card.
pub fn start_privacy_shield_policy_supervisor(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let reported_for_command = Arc::new(Mutex::new(std::collections::BTreeSet::new()));
        loop {
            if let Err(error) =
                apply_fleet_privacy_shield_policy_locked(&app, &reported_for_command).await
            {
                // Fleet may not be configured yet while Pro is starting.  This
                // is retried; never claim an unobserved local state to Fleet.
                crate::log_message(
                    "warn",
                    &format!("[FleetPrivacyShield] policy tick deferred: {error}"),
                );
            }
            if let Err(error) = report_pending_shield_refresh_observations(&app).await {
                crate::log_message(
                    "warn",
                    &format!("[FleetPrivacyShield] native refresh deferred: {error}"),
                );
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

async fn apply_fleet_privacy_shield_policy_locked(
    app: &tauri::AppHandle,
    reported: &Arc<Mutex<std::collections::BTreeSet<String>>>,
) -> Result<(), String> {
    let runtime = tokio::runtime::Handle::current();
    let app = app.clone();
    let reported = Arc::clone(reported);
    tokio::task::spawn_blocking(move || {
        let _guard = crate::paths::acquire_machine_state_lock("fleet-shield-policy")?;
        runtime.block_on(apply_fleet_privacy_shield_policy(&app, &reported))
    })
    .await
    .map_err(|error| format!("Shield supervisor worker failed: {error}"))?
}

fn fleet_shield_report_key(command_id: Option<&str>, status: &str) -> String {
    format!("{}:{status}", command_id.unwrap_or(""))
}

fn single_shield_session_matches(current: u32, sessions: &[u32]) -> bool {
    current != 0 && sessions == [current]
}

/// Until the SYSTEM service owns the effective Shield epoch, a per-user
/// settings overlay cannot safely coordinate simultaneous Windows sessions.
/// Count disconnected RDS sessions too: their detector may still be running.
pub(crate) fn current_shield_session_id() -> Result<u32, String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        let mut current = 0u32;
        if unsafe { ProcessIdToSessionId(std::process::id(), &mut current) } == 0 || current == 0 {
            return Err("Shield Windows session identity is unavailable".into());
        }
        Ok(current)
    }
    #[cfg(not(windows))]
    {
        Ok(1)
    }
}

pub(crate) fn require_single_interactive_shield_session() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::RemoteDesktop::{
            WTSEnumerateSessionsW, WTSFreeMemory, WTSActive,
            WTSConnected, WTSDisconnected, WTSIdle, WTS_CURRENT_SERVER_HANDLE,
        };
        let current = current_shield_session_id()?;
        let mut sessions = std::ptr::null_mut();
        let mut count = 0u32;
        if unsafe { WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut sessions, &mut count) } == 0 {
            return Err("Shield Windows session enumeration is unavailable".into());
        }
        let result = if count > 256 || (count > 0 && sessions.is_null()) {
            Err("Shield Windows session enumeration is invalid".into())
        } else {
            let active: Vec<u32> = if count == 0 { Vec::new() } else {
                unsafe { std::slice::from_raw_parts(sessions, count as usize) }
                    .iter()
                    .filter(|row| row.SessionId != 0 && [WTSActive, WTSConnected, WTSDisconnected, WTSIdle].contains(&row.State))
                    .map(|row| row.SessionId)
                    .collect()
            };
            if single_shield_session_matches(current, &active) {
                Ok(())
            } else {
                Err("Shield policy requires one confirmed Windows user session".into())
            }
        };
        if !sessions.is_null() {
            unsafe { WTSFreeMemory(sessions.cast()) };
        }
        result
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

async fn report_shield_policy_progress(
    transaction: &crate::settings::ShieldPolicyTransaction,
    stage: ShieldPolicyProgressStage,
    reason_code: Option<ShieldPolicyProgressReason>,
) -> Result<(), String> {
    let event = ShieldPolicyProgress {
        event_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: transaction.attempt_id.clone(),
        epoch_version: i64::from(transaction.epoch_version),
        epoch_signature_hash: transaction.epoch_signature_hash.clone(),
        resolution_revision: transaction.resolution_revision,
        enrollment_generation: None,
        shield_revision: transaction.shield_revision,
        stage,
        sampled_at: chrono::Utc::now().to_rfc3339(),
        reason_code,
    };
    let args = serde_json::to_value(event)
        .map_err(|_| "Shield policy progress could not be encoded".to_string())?;
    crate::sidecar::dispatch_paid_command("fleet_agent_shield_policy_progress", args).await?;
    Ok(())
}

fn store_shield_policy_transaction(
    transaction: Option<&crate::settings::ShieldPolicyTransaction>,
) -> Result<(), String> {
    crate::settings::patch_settings(serde_json::json!({
        "app": { "fleet": { "privacyShieldPolicyTransaction": transaction } }
    }))?;
    Ok(())
}

async fn advance_shield_policy_progress(
    transaction: &mut crate::settings::ShieldPolicyTransaction,
    stage: ShieldPolicyProgressStage,
    reason_code: Option<ShieldPolicyProgressReason>,
) -> Result<(), String> {
    if transaction.last_stage == Some(ShieldPolicyProgressStage::Failed) {
        return Err("a failed Shield policy attempt cannot advance".into());
    }
    if transaction.last_stage == Some(ShieldPolicyProgressStage::Verified)
        && stage == ShieldPolicyProgressStage::Failed
    {
        return Err("a verified Shield policy attempt cannot fail retroactively".into());
    }
    if transaction.last_stage.is_some_and(|last| {
        last == stage
            || (stage != ShieldPolicyProgressStage::Failed
                && shield_stage_rank(last) >= shield_stage_rank(stage))
    }) {
        return Ok(());
    }
    report_shield_policy_progress(transaction, stage, reason_code).await?;
    transaction.last_stage = Some(stage);
    if stage == ShieldPolicyProgressStage::Failed {
        transaction.retry_not_before_ms = chrono::Utc::now().timestamp_millis() + 60_000;
    }
    store_shield_policy_transaction(Some(transaction))
}

fn shield_retry_is_due(transaction: &crate::settings::ShieldPolicyTransaction) -> bool {
    transaction.attempt_count < 3
        && chrono::Utc::now().timestamp_millis() >= transaction.retry_not_before_ms
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShieldRecoveryCheckpoint {
    BeforeCommit,
    Committed,
    Superseded,
}

fn shield_recovery_checkpoint(
    transaction_epoch: u32,
    stored_epoch: u32,
) -> ShieldRecoveryCheckpoint {
    match stored_epoch.cmp(&transaction_epoch) {
        std::cmp::Ordering::Less => ShieldRecoveryCheckpoint::BeforeCommit,
        std::cmp::Ordering::Equal => ShieldRecoveryCheckpoint::Committed,
        std::cmp::Ordering::Greater => ShieldRecoveryCheckpoint::Superseded,
    }
}

fn shield_policy_intent_is_superseded(
    transaction: &crate::settings::ShieldPolicyTransaction,
    state: Option<&crate::settings::FleetShieldDesiredState>,
) -> bool {
    transaction.local_stop_requested
        || state.map_or(transaction.shield_revision != 0, |state| {
            state.revision != transaction.shield_revision
                || (transaction.was_running
                    && !state.enabled
                    && (transaction.was_fleet_owned
                        || transaction.pre_pause_mode.is_none()
                        || state.command_id.is_some()))
        })
}

fn shield_stage_rank(stage: ShieldPolicyProgressStage) -> u8 {
    match stage {
        ShieldPolicyProgressStage::Received => 0,
        ShieldPolicyProgressStage::Pausing => 1,
        ShieldPolicyProgressStage::Paused => 2,
        ShieldPolicyProgressStage::SettingsApplied => 3,
        ShieldPolicyProgressStage::Restarting => 4,
        ShieldPolicyProgressStage::Verifying => 5,
        ShieldPolicyProgressStage::Verified => 6,
        ShieldPolicyProgressStage::Failed => 7,
    }
}

fn parse_fleet_shield_state(
    value: serde_json::Value,
) -> Result<Option<crate::settings::FleetShieldDesiredState>, String> {
    if value.is_null() {
        Ok(None)
    } else {
        serde_json::from_value(value)
            .map(Some)
            .map_err(|_| "Fleet Shield desired state is malformed".to_string())
    }
}

async fn prepare_shield_policy_transaction(
    current: &crate::settings::AppSettings,
    preview: &crate::settings::AppSettings,
    epoch_version: u32,
    epoch_signature_hash: &str,
    resolution_revision: i64,
) -> Result<crate::settings::ShieldPolicyTransaction, String> {
    require_single_interactive_shield_session()?;
    let app = crate::sidecar::app_handle()
        .ok_or_else(|| "WinCommander native Shield handle is unavailable".to_string())?;
    let state = fleet_sync_shield_state().await?;
    let state = parse_fleet_shield_state(state)?;
    let native = privacy_shield_status(&app).await?;
    let running = fleet_shield_running_state(&native)?;
    let preserve_local = running
        && !current.app.fleet.privacy_shield_session_owned
        && state
            .as_ref()
            .is_none_or(|state| !state.enabled && state.command_id.is_none());
    if running && state.as_ref().is_some_and(|state| !state.enabled) && !preserve_local {
        return Err(
            "a newer Stop intent prevents pausing and restarting Shield for this policy".into(),
        );
    }
    let pre_pause_mode = if preserve_local {
        Some(
            fleet_shield_reported_mode(&native)
                .ok_or("active local Shield mode is unavailable; policy pause was not attempted")?,
        )
    } else {
        None
    };
    let pre_pause_native_options =
        if preserve_local {
            Some(fleet_shield_native_options(&native, true).ok_or(
                "active local Shield options are unavailable; policy pause was not attempted",
            )?)
        } else {
            None
        };
    let pre_pause_camera_index = if preserve_local {
        Some(
            native
                .get("cameraIndex")
                .and_then(serde_json::Value::as_u64)
                .filter(|index| *index <= 32)
                .ok_or(
                    "active local Shield camera is unavailable; policy pause was not attempted",
                )? as u32,
        )
    } else {
        None
    };
    let revision = state.as_ref().map_or(0, |state| state.revision);
    let previous = current.app.fleet.privacy_shield_policy_transaction.as_ref();
    if previous.is_some_and(|tx| tx.epoch_version > epoch_version) {
        return Err("a newer Shield policy transaction is already pending".to_string());
    }
    let expected_hash = fleet_shield_tuning_hash(&preview.ideal.privacy.privacy_shield);
    if previous.is_some_and(|tx| {
        tx.epoch_version == epoch_version
            && tx.last_stage == Some(ShieldPolicyProgressStage::Failed)
            && !shield_retry_is_due(tx)
    }) {
        return Err("Shield policy retry is deferred after failure".into());
    }
    let mut transaction = match previous {
        Some(tx)
            if tx.epoch_version == epoch_version
                && tx.epoch_signature_hash == epoch_signature_hash
                && tx.resolution_revision == resolution_revision
                && tx.expected_hash == expected_hash
                && tx.last_stage != Some(ShieldPolicyProgressStage::Failed) =>
        {
            tx.clone()
        }
        _ => crate::settings::ShieldPolicyTransaction {
            epoch_version,
            epoch_signature_hash: epoch_signature_hash.to_string(),
            resolution_revision,
            shield_revision: revision,
            attempt_id: uuid::Uuid::new_v4().to_string(),
            was_running: running || previous.is_some_and(|tx| tx.was_running),
            was_fleet_owned: previous
                .map_or(current.app.fleet.privacy_shield_session_owned, |tx| {
                    tx.was_fleet_owned
                }),
            owner_session_id: previous
                .and_then(|tx| tx.owner_session_id)
                .or(if running { Some(current_shield_session_id()?) } else { None }),
            pre_pause_mode: previous
                .and_then(|tx| tx.pre_pause_mode.clone())
                .or(pre_pause_mode),
            pre_pause_camera_index: previous
                .and_then(|tx| tx.pre_pause_camera_index)
                .or(pre_pause_camera_index),
            pre_pause_native_options: previous
                .and_then(|tx| tx.pre_pause_native_options.clone())
                .or(pre_pause_native_options),
            local_stop_requested: previous.is_some_and(|tx| tx.local_stop_requested),
            attempt_count: previous
                .filter(|tx| tx.epoch_version == epoch_version)
                .map_or(1, |tx| tx.attempt_count.saturating_add(1).max(1)),
            retry_not_before_ms: 0,
            expected_hash,
            last_stage: None,
        },
    };
    // Durable intent precedes the first native change. A crash now leaves a
    // marker that blocks the old supervisor/autostart until this epoch resumes.
    store_shield_policy_transaction(Some(&transaction))?;
    if transaction.last_stage.is_none() {
        advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Received, None)
            .await?;
    }
    if transaction.was_running {
        if running {
            if transaction.last_stage != Some(ShieldPolicyProgressStage::Paused) {
                advance_shield_policy_progress(
                    &mut transaction,
                    ShieldPolicyProgressStage::Pausing,
                    None,
                )
                .await?;
            }
            let stopped = crate::backend::run_backend_script_unlocked(
                app.clone(),
                "Stop-PrivacyShield".to_string(),
                HashMap::new(),
            )
            .await?;
            if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
                advance_shield_policy_progress(
                    &mut transaction,
                    ShieldPolicyProgressStage::Failed,
                    Some(ShieldPolicyProgressReason::StopRejected),
                )
                .await?;
                return Err("Privacy Shield pause was rejected before policy commit".to_string());
            }
            let observed = privacy_shield_status(&app).await?;
            if fleet_shield_running_state(&observed)? {
                advance_shield_policy_progress(
                    &mut transaction,
                    ShieldPolicyProgressStage::Failed,
                    Some(ShieldPolicyProgressReason::StopNotObserved),
                )
                .await?;
                return Err(
                    "Privacy Shield pause was not observed before policy commit".to_string()
                );
            }
        }
        advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Paused, None)
            .await?;
    }
    Ok(transaction)
}

/// Queue one lifecycle observation without allowing an unavailable/slow Fleet
/// reply to hold the local policy worker hostage.  A report is never treated
/// as the proof of a Shield transition: callers only queue a terminal state
/// after an independent local read-back.  The command id remains attached to
/// every retry, so Fleet can only complete the exact command it issued.
fn report_fleet_shield_once(
    app: tauri::AppHandle,
    reported_for_command: &Arc<Mutex<std::collections::BTreeSet<String>>>,
    command_id: Option<&str>,
    status: &str,
) {
    let key = fleet_shield_report_key(command_id, status);
    let Ok(mut reported) = reported_for_command.lock() else {
        return;
    };
    if !reported.insert(key.clone()) {
        return;
    }
    drop(reported);
    let status = status.to_string();
    let command_id = command_id.map(str::to_string);
    let reported_for_command = Arc::clone(reported_for_command);
    tauri::async_runtime::spawn(async move {
        // The dedicated Pro agent owns this queue.  Its handler can persist a
        // lifecycle transition before a transient IPC reply is lost; bounded
        // delivery/retry therefore keeps the worker responsive without
        // claiming a terminal result from the start request alone.
        for attempt in 1..=2 {
            match tokio::time::timeout(
                std::time::Duration::from_secs(15),
                fleet_report_privacy_shield_status(
                    app.clone(),
                    status.clone(),
                    None,
                    command_id.clone(),
                ),
            )
            .await
            {
                Ok(Ok(_)) => return,
                Ok(Err(error)) => crate::log_message(
                    "warn",
                    &format!(
                        "[FleetPrivacyShield] status '{}' delivery attempt {} deferred: {}",
                        status, attempt, error
                    ),
                ),
                Err(_) => crate::log_message(
                    "warn",
                    &format!(
                        "[FleetPrivacyShield] status '{}' delivery attempt {} timed out",
                        status, attempt
                    ),
                ),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        // An IPC outage must not permanently suppress this command's stage.
        // The next supervisor tick may retry; Pro de-duplicates accepted rows.
        if let Ok(mut reported) = reported_for_command.lock() {
            reported.remove(&key);
        }
    });
}

/// Local sessions have no command id, so retain only the current read-back.
/// This permits `stopped → running_local_session → stopped` to reach Fleet
/// while still suppressing the five-second supervisor poll between changes.
fn report_fleet_shield_local_observation(
    app: tauri::AppHandle,
    reported_for_command: &Arc<Mutex<std::collections::BTreeSet<String>>>,
    status: &'static str,
) {
    if let Ok(mut reported) = reported_for_command.lock() {
        reconcile_fleet_shield_local_observation_keys(&mut reported, status);
    }
    report_fleet_shield_once(app, reported_for_command, None, status);
}

fn reconcile_fleet_shield_local_observation_keys(
    reported_for_command: &mut std::collections::BTreeSet<String>,
    status: &str,
) {
    for prior in ["running_local_session", "stopped"] {
        if prior != status {
            reported_for_command.remove(&fleet_shield_report_key(None, prior));
        }
    }
}

fn fleet_shield_camera_issue(status: &serde_json::Value) -> Option<&'static str> {
    if status
        .get("cameraAvailable")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        return None;
    }
    match status
        .get("cameraStatus")
        .and_then(serde_json::Value::as_str)
    {
        Some("windows_camera_policy_denied") => Some("windows_camera_policy_denied"),
        Some("app_camera_permission_denied") => Some("app_camera_permission_denied"),
        Some("hardware_unavailable") => Some("hardware_unavailable"),
        Some("camera_busy") => Some("camera_busy"),
        Some("camera_feed_unavailable") => Some("camera_feed_unavailable"),
        _ => Some("camera_status_unknown"),
    }
}

fn fleet_shield_start_params(
    state: &crate::settings::FleetShieldDesiredState,
    shield: &crate::settings::PrivacyShieldSettings,
) -> Result<HashMap<String, String>, String> {
    let blur_enabled = match state.mode.as_str() {
        "notify_only" => false,
        "blur_notify" => true,
        _ => return Err("Fleet supplied an unsupported Privacy Shield mode".to_string()),
    };
    let mut params = HashMap::from([
        ("Camera".to_string(), "0".to_string()),
        ("Mode".to_string(), state.mode.clone()),
        // Fleet observes all three classes.  The signed mode below controls
        // display blur only; Notify-only must never silently become blur.
        ("CheckGaze".to_string(), "true".to_string()),
        ("CheckFaces".to_string(), "true".to_string()),
        ("CheckPhone".to_string(), "true".to_string()),
        ("CaptureOnDevice".to_string(), "false".to_string()),
        ("CaptureOnMultiFace".to_string(), "false".to_string()),
        (
            "ModelLevel".to_string(),
            shield
                .model_size
                .clone()
                .unwrap_or_else(|| "medium".to_string()),
        ),
        (
            "Confidence".to_string(),
            shield.confidence_threshold.unwrap_or(0.5).to_string(),
        ),
        (
            "OverlayOpacity".to_string(),
            (shield.blur_opacity.unwrap_or(200.0) as u32).to_string(),
        ),
        (
            "WakeDelayMs".to_string(),
            shield.wake_delay_seconds.unwrap_or(150).to_string(),
        ),
        (
            "DeviceWakeMultiplier".to_string(),
            shield.device_wake_multiplier.unwrap_or(5.0).to_string(),
        ),
        (
            "MultiFaceWakeMultiplier".to_string(),
            shield.multi_face_wake_multiplier.unwrap_or(5.0).to_string(),
        ),
        (
            "BufferFrames".to_string(),
            shield.detection_buffer_frames.unwrap_or(2).to_string(),
        ),
        (
            "CaptureSpeed".to_string(),
            shield.capture_speed.unwrap_or(1).to_string(),
        ),
    ]);
    params.insert(
        "BlurGaze".to_string(),
        (blur_enabled && shield.gaze_detection_enabled.unwrap_or(true)).to_string(),
    );
    params.insert(
        "BlurFaces".to_string(),
        (blur_enabled && shield.anti_peeping_enabled.unwrap_or(true)).to_string(),
    );
    params.insert(
        "BlurPhone".to_string(),
        (blur_enabled && shield.camera_hunter_enabled.unwrap_or(true)).to_string(),
    );
    Ok(params)
}

fn local_shield_restart_params(
    mode: &str,
    camera_index: u32,
    observed: &ShieldNativeOptions,
    shield: &crate::settings::PrivacyShieldSettings,
) -> Result<HashMap<String, String>, String> {
    if !matches!(mode, "notify_only" | "blur_notify") || camera_index > 32 || !observed.is_bounded()
    {
        return Err("pre-pause local Shield options are unavailable".into());
    }
    let blur = mode == "blur_notify";
    Ok(HashMap::from([
        ("Camera".into(), camera_index.to_string()),
        ("Mode".into(), mode.into()),
        ("CheckGaze".into(), observed.check_gaze.to_string()),
        ("CheckFaces".into(), observed.check_faces.to_string()),
        ("CheckPhone".into(), observed.check_phone.to_string()),
        (
            "CaptureOnDevice".into(),
            observed.capture_on_device.to_string(),
        ),
        (
            "CaptureOnMultiFace".into(),
            observed.capture_on_multi_face.to_string(),
        ),
        (
            "BlurGaze".into(),
            (blur && shield.gaze_detection_enabled.unwrap_or(observed.blur_gaze)).to_string(),
        ),
        (
            "BlurFaces".into(),
            (blur && shield.anti_peeping_enabled.unwrap_or(observed.blur_faces)).to_string(),
        ),
        (
            "BlurPhone".into(),
            (blur && shield.camera_hunter_enabled.unwrap_or(observed.blur_phone)).to_string(),
        ),
        (
            "ModelLevel".into(),
            shield
                .model_size
                .clone()
                .unwrap_or_else(|| observed.model_level.clone()),
        ),
        (
            "Confidence".into(),
            shield
                .confidence_threshold
                .map(f64::from)
                .unwrap_or(observed.confidence)
                .to_string(),
        ),
        (
            "OverlayOpacity".into(),
            (shield
                .blur_opacity
                .map(|v| v as u32)
                .unwrap_or(observed.overlay_opacity))
            .to_string(),
        ),
        (
            "WakeDelayMs".into(),
            shield
                .wake_delay_seconds
                .unwrap_or(observed.wake_delay_ms)
                .to_string(),
        ),
        (
            "DeviceWakeMultiplier".into(),
            shield
                .device_wake_multiplier
                .map(f64::from)
                .unwrap_or(observed.device_wake_multiplier)
                .to_string(),
        ),
        (
            "MultiFaceWakeMultiplier".into(),
            shield
                .multi_face_wake_multiplier
                .map(f64::from)
                .unwrap_or(observed.multi_face_wake_multiplier)
                .to_string(),
        ),
        (
            "BufferFrames".into(),
            shield
                .detection_buffer_frames
                .unwrap_or(observed.buffer_frames)
                .to_string(),
        ),
        (
            "CaptureSpeed".into(),
            shield
                .capture_speed
                .unwrap_or(observed.capture_speed)
                .to_string(),
        ),
    ]))
}

async fn local_shield_intent_still_current(
    transaction: &crate::settings::ShieldPolicyTransaction,
) -> Result<bool, String> {
    let state = parse_fleet_shield_state(fleet_sync_shield_state().await?)?;
    let settings = crate::settings::read_settings()?;
    Ok(state
        .as_ref()
        .map_or(transaction.shield_revision == 0, |state| {
            state.revision == transaction.shield_revision
                && !state.enabled
                && state.command_id.is_none()
        })
        && !settings
            .app
            .fleet
            .privacy_shield_policy_transaction
            .as_ref()
            .is_some_and(|tx| tx.local_stop_requested)
        && settings.policy.master_config_version == Some(transaction.epoch_version)
        && fleet_shield_tuning_hash(&settings.ideal.privacy.privacy_shield)
            == transaction.expected_hash)
}

fn fleet_shield_requires_reconfiguration(
    applied_revision: Option<i64>,
    applied_command_id: Option<&str>,
    state: &crate::settings::FleetShieldDesiredState,
) -> bool {
    if applied_revision != Some(state.revision) {
        return true;
    }
    // A reset/re-enrolment checkpoint may replay a revision number. Only an
    // organization default without a command id can rely on revision alone.
    state
        .command_id
        .as_deref()
        .is_some_and(|command_id| applied_command_id != Some(command_id))
}

fn fleet_shield_tuning_hash(shield: &crate::settings::PrivacyShieldSettings) -> String {
    use sha2::{Digest, Sha256};
    let tuning = serde_json::json!({
        "gaze": shield.gaze_detection_enabled,
        "faces": shield.anti_peeping_enabled,
        "phone": shield.camera_hunter_enabled,
        "confidence": shield.confidence_threshold,
        "wake": shield.wake_delay_seconds,
        "opacity": shield.blur_opacity,
        "model": shield.model_size,
        "buffer": shield.detection_buffer_frames,
        "device_multiplier": shield.device_wake_multiplier,
        "face_multiplier": shield.multi_face_wake_multiplier,
        "capture_speed": shield.capture_speed,
        "notify_look_away": shield.look_away_notifications_enabled,
        "notify_multiple_faces": shield.multiple_faces_notifications_enabled,
        "notify_phone": shield.phone_notifications_enabled,
        "fleet_notification_limit": shield.fleet_notification_limit,
        "fleet_notification_window": shield.fleet_notification_window_seconds,
    });
    hex::encode(Sha256::digest(tuning.to_string().as_bytes()))
}

async fn fleet_shield_intent_still_current(
    expected: &crate::settings::FleetShieldDesiredState,
    expected_epoch: Option<u32>,
    expected_hash: &str,
) -> Result<bool, String> {
    let latest = fleet_sync_shield_state().await?;
    let latest = serde_json::from_value::<crate::settings::FleetShieldDesiredState>(latest).ok();
    let settings = crate::settings::read_settings()?;
    Ok(fleet_shield_intent_matches(
        expected,
        latest.as_ref(),
        expected_epoch,
        settings.policy.master_config_version,
        expected_hash,
        &fleet_shield_tuning_hash(&settings.ideal.privacy.privacy_shield),
    ))
}

fn fleet_shield_intent_matches(
    expected: &crate::settings::FleetShieldDesiredState,
    latest: Option<&crate::settings::FleetShieldDesiredState>,
    expected_epoch: Option<u32>,
    latest_epoch: Option<u32>,
    expected_hash: &str,
    latest_hash: &str,
) -> bool {
    latest == Some(expected) && latest_epoch == expected_epoch && latest_hash == expected_hash
}

fn fleet_shield_should_apply_stop(
    running: bool,
    state: &crate::settings::FleetShieldDesiredState,
    fleet_owns_session: bool,
    needs_reconfiguration: bool,
) -> bool {
    running
        && !state.enabled
        && needs_reconfiguration
        && (state.command_id.is_some() || fleet_owns_session)
}

async fn privacy_shield_status(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    crate::backend::run_backend_script_unlocked(
        app.clone(),
        "Get-PrivacyShieldStatus".to_string(),
        HashMap::new(),
    )
    .await
}

fn fleet_shield_running_state(status: &serde_json::Value) -> Result<bool, String> {
    if status.get("error").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err("Privacy Shield native status is unavailable".to_string());
    }
    status
        .get("running")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "Privacy Shield native running state is unknown".to_string())
}

/// Each refresh token came from a verified, device-scoped Pro command. Sample
/// the native detector after receiving it, then return only closed state and
/// the locally stored signed policy epoch. An old Pro cache cannot manufacture this sample.
async fn report_pending_shield_refresh_observations(app: &tauri::AppHandle) -> Result<(), String> {
    let response = crate::sidecar::dispatch_paid_command(
        "fleet_agent_shield_refresh_requests",
        serde_json::Value::Null,
    )
    .await?;
    let Some(requests) = response
        .get("requests")
        .and_then(serde_json::Value::as_array)
    else {
        return Err("Shield refresh request response is malformed".into());
    };
    if requests.is_empty() {
        return Ok(());
    }
    let eligible: Vec<_> = requests
        .iter()
        .filter_map(|request| {
            let command_id = request.get("commandId")?.as_str()?;
            let epoch_version = request.get("epochVersion")?.as_i64()?;
            (uuid::Uuid::parse_str(command_id).is_ok() && epoch_version >= 0)
                .then(|| (command_id.to_string(), epoch_version))
        })
        .collect();
    if eligible.is_empty() {
        return Ok(());
    }
    let sample = sample_shield_refresh_observation_locked(app).await?;
    for (command_id, epoch_version) in eligible {
        crate::sidecar::dispatch_paid_command(
            "fleet_agent_shield_refresh_observation",
            serde_json::json!({
                "commandId": command_id,
                "epochVersion": epoch_version,
                "storedEpoch": sample.stored_epoch,
                "sampledAt": sample.sampled_at.clone(),
                "running": sample.running,
                "mode": sample.mode.clone(),
                "nativeOptions": sample.native_options.clone(),
                "storedSettings": sample.stored_settings.clone(),
            }),
        )
        .await?;
    }
    Ok(())
}

#[derive(Clone)]
struct ShieldRefreshSample {
    stored_epoch: i64,
    sampled_at: String,
    running: bool,
    mode: Option<String>,
    native_options: Option<ShieldNativeOptions>,
    stored_settings: ShieldStoredSettings,
}

async fn sample_shield_refresh_observation_locked(
    app: &tauri::AppHandle,
) -> Result<ShieldRefreshSample, String> {
    let runtime = tokio::runtime::Handle::current();
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = crate::paths::acquire_machine_state_lock("fleet-shield-policy")?;
        runtime.block_on(sample_shield_refresh_observation(&app))
    })
    .await
    .map_err(|error| format!("Shield inspection worker failed: {error}"))?
}

async fn sample_shield_refresh_observation(
    app: &tauri::AppHandle,
) -> Result<ShieldRefreshSample, String> {
    require_single_interactive_shield_session()?;
    let settings = crate::settings::read_settings()?;
    if settings
        .app
        .fleet
        .privacy_shield_policy_transaction
        .as_ref()
        .is_some_and(|tx| tx.last_stage != Some(ShieldPolicyProgressStage::Failed))
    {
        return Err("Shield policy is transitioning; inspection will retry".into());
    }
    let native = privacy_shield_status(app).await?;
    let running = fleet_shield_running_state(&native)?;
    let mode = fleet_shield_reported_mode(&native);
    if running && mode.is_none() {
        return Err("native Shield running mode is unknown".into());
    }
    Ok(ShieldRefreshSample {
        stored_epoch: i64::from(settings.policy.master_config_version.unwrap_or(0)),
        sampled_at: chrono::Utc::now().to_rfc3339(),
        running,
        mode,
        native_options: fleet_shield_native_options(&native, running),
        stored_settings: fleet_shield_stored_settings(&settings.ideal.privacy.privacy_shield),
    })
}

/// Extract only the detector's closed, process-derived mode vocabulary. The
/// bridge never forwards a raw command line, camera name, PID, frame, or log.
fn fleet_shield_reported_mode(status: &serde_json::Value) -> Option<String> {
    match status.get("activeMode").and_then(serde_json::Value::as_str) {
        Some("notify_only") => Some("notify_only".to_string()),
        Some("blur_notify") => Some("blur_notify".to_string()),
        _ => None,
    }
}

fn fleet_shield_native_options(
    status: &serde_json::Value,
    running: bool,
) -> Option<ShieldNativeOptions> {
    if !running {
        return None;
    }
    status
        .get("nativeOptions")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .filter(ShieldNativeOptions::is_bounded)
}

fn fleet_shield_launch_matches(
    status: &serde_json::Value,
    state: &crate::settings::FleetShieldDesiredState,
    params: &HashMap<String, String>,
) -> bool {
    if status
        .get("cameraIndex")
        .and_then(serde_json::Value::as_u64)
        != params
            .get("Camera")
            .and_then(|value| value.parse::<u64>().ok())
    {
        return false;
    }
    if fleet_shield_reported_mode(status).as_deref() != Some(state.mode.as_str()) {
        return false;
    }
    let Some(observed) = fleet_shield_native_options(status, true) else {
        return false;
    };
    let expected = serde_json::json!({
        "checkGaze": params.get("CheckGaze") == Some(&"true".to_string()),
        "checkFaces": params.get("CheckFaces") == Some(&"true".to_string()),
        "checkPhone": params.get("CheckPhone") == Some(&"true".to_string()),
        "blurGaze": params.get("BlurGaze") == Some(&"true".to_string()),
        "blurFaces": params.get("BlurFaces") == Some(&"true".to_string()),
        "blurPhone": params.get("BlurPhone") == Some(&"true".to_string()),
        "captureOnDevice": params.get("CaptureOnDevice") == Some(&"true".to_string()),
        "captureOnMultiFace": params.get("CaptureOnMultiFace") == Some(&"true".to_string()),
        "modelLevel": params.get("ModelLevel"),
        "confidence": params.get("Confidence").and_then(|v| v.parse::<f64>().ok()),
        "overlayOpacity": params.get("OverlayOpacity").and_then(|v| v.parse::<u32>().ok()),
        "wakeDelayMs": params.get("WakeDelayMs").and_then(|v| v.parse::<u32>().ok()),
        "deviceWakeMultiplier": params.get("DeviceWakeMultiplier").and_then(|v| v.parse::<f64>().ok()),
        "multiFaceWakeMultiplier": params.get("MultiFaceWakeMultiplier").and_then(|v| v.parse::<f64>().ok()),
        "bufferFrames": params.get("BufferFrames").and_then(|v| v.parse::<u32>().ok()),
        "captureSpeed": params.get("CaptureSpeed").and_then(|v| v.parse::<u32>().ok()),
    });
    serde_json::from_value::<ShieldNativeOptions>(expected)
        .ok()
        .as_ref()
        == Some(&observed)
}

fn fleet_shield_stored_settings(
    shield: &crate::settings::PrivacyShieldSettings,
) -> ShieldStoredSettings {
    ShieldStoredSettings {
        wake_delay_seconds: shield
            .wake_delay_seconds
            .filter(|value| (50..=1_500).contains(value)),
        device_wake_multiplier: shield
            .device_wake_multiplier
            .map(f64::from)
            .filter(|value| value.is_finite() && (1.0..=20.0).contains(value)),
        multi_face_wake_multiplier: shield
            .multi_face_wake_multiplier
            .map(f64::from)
            .filter(|value| value.is_finite() && (1.0..=20.0).contains(value)),
        detection_buffer_frames: shield
            .detection_buffer_frames
            .filter(|value| (1..=8).contains(value)),
        gaze_detection_enabled: shield.gaze_detection_enabled,
        anti_peeping_enabled: shield.anti_peeping_enabled,
        camera_hunter_enabled: shield.camera_hunter_enabled,
        look_away_notifications_enabled: shield.look_away_notifications_enabled,
        multiple_faces_notifications_enabled: shield.multiple_faces_notifications_enabled,
        phone_notifications_enabled: shield.phone_notifications_enabled,
        fleet_notification_limit: shield
            .fleet_notification_limit
            .filter(|value| *value <= 1_000),
        fleet_notification_window_seconds: shield
            .fleet_notification_window_seconds
            .filter(|value| (1..=86_400).contains(value)),
    }
}

async fn apply_fleet_privacy_shield_policy(
    app: &tauri::AppHandle,
    reported_for_command: &Arc<Mutex<std::collections::BTreeSet<String>>>,
) -> Result<(), String> {
    require_single_interactive_shield_session()?;
    verify_service_shield_profile().await?;
    if let Some(transaction) = crate::settings::read_settings()?
        .app
        .fleet
        .privacy_shield_policy_transaction
    {
        return resume_shield_policy_transaction(app, reported_for_command, transaction).await;
    }
    let state = fleet_sync_shield_state().await?;
    let Some(state) =
        serde_json::from_value::<crate::settings::FleetShieldDesiredState>(state).ok()
    else {
        // No Fleet Shield desired state does not mean the detector is stopped:
        // an employee can have started it locally. Publish one fresh, bounded
        // read-back so Fleet distinguishes a local session from a disconnected
        // agent or a Fleet-managed session.
        let local = privacy_shield_status(app).await?;
        let status = if fleet_shield_running_state(&local)? {
            "running_local_session"
        } else {
            "stopped"
        };
        report_fleet_shield_local_observation(app.clone(), reported_for_command, status);
        return Ok(());
    };
    let command_id = state.command_id.as_deref();
    report_fleet_shield_once(app.clone(), reported_for_command, command_id, "received");

    let before = privacy_shield_status(app).await?;
    let running = fleet_shield_running_state(&before)?;
    let settings = crate::settings::read_settings()?;
    if !running
        && state.enabled
        && settings
            .app
            .fleet
            .privacy_shield_stopped_policy_hold
            .as_ref()
            .is_some_and(|hold| {
                hold.shield_revision == state.revision
                    && hold.command_id.as_deref() == state.command_id.as_deref()
            })
    {
        // Saving policy while stopped must not manufacture a Start. A newer
        // desired-state request changes revision/command and releases hold.
        report_fleet_shield_local_observation(app.clone(), reported_for_command, "stopped");
        return Ok(());
    }
    let tuning_hash = fleet_shield_tuning_hash(&settings.ideal.privacy.privacy_shield);
    let policy_epoch = settings.policy.master_config_version;
    let needs_reconfiguration = fleet_shield_requires_reconfiguration(
        settings.app.fleet.privacy_shield_applied_revision,
        settings
            .app
            .fleet
            .privacy_shield_applied_command_id
            .as_deref(),
        &state,
    ) || (state.enabled
        && running
        && settings
            .app
            .fleet
            .privacy_shield_applied_tuning_hash
            .as_deref()
            != Some(tuning_hash.as_str()));

    if !state.enabled {
        let should_stop = fleet_shield_should_apply_stop(
            running,
            &state,
            settings.app.fleet.privacy_shield_session_owned,
            needs_reconfiguration,
        );
        if !needs_reconfiguration {
            // Fleet's Off state is an instruction to stop the current session,
            // not a permanent local lock. Once this exact revision/command
            // has been applied, observe later local starts instead of stopping
            // them again on every supervisor tick.
            if settings.app.fleet.privacy_shield_session_owned {
                crate::settings::patch_settings(serde_json::json!({
                    "app": { "fleet": { "privacyShieldSessionOwned": false } }
                }))?;
            }
            crate::set_tray_shield_running(app, running);
            report_fleet_shield_local_observation(
                app.clone(),
                reported_for_command,
                if running {
                    "running_local_session"
                } else {
                    "stopped"
                },
            );
            return Ok(());
        }
        if should_stop {
            report_fleet_shield_once(app.clone(), reported_for_command, command_id, "applying");
            let stopped = crate::backend::run_backend_script_unlocked(
                app.clone(),
                "Stop-PrivacyShield".to_string(),
                HashMap::new(),
            )
            .await?;
            if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
                return Err("Privacy Shield stop was not accepted by the device".to_string());
            }
            let read_back = privacy_shield_status(app).await?;
            if fleet_shield_running_state(&read_back)? {
                return Err("Privacy Shield is still running after the stop request".to_string());
            }
        }
        if !fleet_shield_intent_still_current(&state, policy_epoch, &tuning_hash).await? {
            return Ok(());
        }
        crate::settings::patch_settings(serde_json::json!({
            "app": { "fleet": {
                "privacyShieldSessionOwned": false,
                "privacyShieldAppliedRevision": state.revision,
                "privacyShieldAppliedCommandId": state.command_id.clone(),
                "privacyShieldAppliedTuningHash": tuning_hash,
            } }
        }))?;
        crate::set_tray_shield_running(app, running && !should_stop);
        if command_id.is_some() {
            report_fleet_shield_once(
                app.clone(),
                reported_for_command,
                command_id,
                "disabled_by_policy",
            );
        } else {
            report_fleet_shield_local_observation(
                app.clone(),
                reported_for_command,
                if running && !should_stop {
                    "running_local_session"
                } else {
                    "stopped"
                },
            );
        }
        return Ok(());
    }

    if let Some(issue) = fleet_shield_camera_issue(&before) {
        report_fleet_shield_once(app.clone(), reported_for_command, command_id, issue);
        return Ok(());
    }
    if running && !needs_reconfiguration {
        if !fleet_shield_intent_still_current(&state, policy_epoch, &tuning_hash).await? {
            return Ok(());
        }
        crate::settings::patch_settings(serde_json::json!({
            "app": { "fleet": { "privacyShieldSessionOwned": true } }
        }))?;
        crate::set_tray_shield_running(app, true);
        let params = fleet_shield_start_params(&state, &settings.ideal.privacy.privacy_shield)?;
        report_fleet_shield_once(
            app.clone(),
            reported_for_command,
            command_id,
            if fleet_shield_launch_matches(&before, &state, &params) {
                "running_fleet_session"
            } else {
                "applying"
            },
        );
        return Ok(());
    }

    report_fleet_shield_once(app.clone(), reported_for_command, command_id, "applying");
    if running {
        // A mode is encoded in the long-lived detector launch flags.  A
        // fresh Fleet revision therefore reconfigures a running session by
        // stopping it, verifying that stop, then launching it with the new
        // signed mode.  Never claim the new mode from a running bit alone.
        let stopped = crate::backend::run_backend_script_unlocked(
            app.clone(),
            "Stop-PrivacyShield".to_string(),
            HashMap::new(),
        )
        .await?;
        if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(
                "Privacy Shield mode reconfiguration was not accepted by the device".to_string(),
            );
        }
        let stopped_read_back = privacy_shield_status(app).await?;
        if fleet_shield_running_state(&stopped_read_back)? {
            return Err(
                "Privacy Shield is still running before Fleet mode reconfiguration".to_string(),
            );
        }
    }
    // Policy and Start/Stop commands arrive independently. A newer Stop or
    // signed epoch received while we awaited the stop must win before launch.
    if !fleet_shield_intent_still_current(&state, policy_epoch, &tuning_hash).await? {
        return Ok(());
    }
    let params = fleet_shield_start_params(&state, &settings.ideal.privacy.privacy_shield)?;
    let started = crate::backend::run_backend_script_unlocked(
        app.clone(),
        "Start-PrivacyShield".to_string(),
        params.clone(),
    )
    .await?;
    if started.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
        let read_back = privacy_shield_status(app).await.unwrap_or(before);
        let issue = fleet_shield_camera_issue(&read_back).unwrap_or("start_failed");
        report_fleet_shield_once(app.clone(), reported_for_command, command_id, issue);
        return Ok(());
    }
    let read_back = privacy_shield_status(app).await?;
    if !fleet_shield_running_state(&read_back)? {
        return Err("Privacy Shield start has not reached a running read-back yet".to_string());
    }
    let still_current = fleet_shield_intent_still_current(&state, policy_epoch, &tuning_hash).await;
    if !matches!(still_current, Ok(true)) {
        // A Stop can arrive while the launcher is in flight. Do not publish
        // the old Start as applied, and leave no stale Fleet-started session.
        // An unavailable Pro read is unknown, so it also fails closed here.
        let stopped = crate::backend::run_backend_script_unlocked(
            app.clone(),
            "Stop-PrivacyShield".to_string(),
            HashMap::new(),
        )
        .await?;
        if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true)
            || fleet_shield_running_state(&privacy_shield_status(app).await?)?
        {
            return Err("Privacy Shield could not stop after a newer Fleet request".to_string());
        }
        return still_current.map(|_| ());
    }
    crate::settings::patch_settings(serde_json::json!({
        "app": { "fleet": {
            "privacyShieldSessionOwned": true,
            "privacyShieldAppliedRevision": state.revision,
            "privacyShieldAppliedCommandId": state.command_id.clone(),
            "privacyShieldAppliedTuningHash": tuning_hash,
        } }
    }))?;
    crate::set_tray_shield_running(app, true);
    report_fleet_shield_once(
        app.clone(),
        reported_for_command,
        command_id,
        if fleet_shield_launch_matches(&read_back, &state, &params) {
            "running_fleet_session"
        } else {
            "applying"
        },
    );
    Ok(())
}

async fn resume_shield_policy_transaction(
    app: &tauri::AppHandle,
    reported_for_command: &Arc<Mutex<std::collections::BTreeSet<String>>>,
    mut transaction: crate::settings::ShieldPolicyTransaction,
) -> Result<(), String> {
    let prior_failed = transaction.last_stage == Some(ShieldPolicyProgressStage::Failed);
    if prior_failed && !shield_retry_is_due(&transaction) && !transaction.local_stop_requested {
        return Err("Shield policy retry is deferred after failure".into());
    }
    let settings = crate::settings::read_settings()?;
    let native = privacy_shield_status(app).await?;
    let mut running = fleet_shield_running_state(&native)?;
    if transaction.local_stop_requested {
        if running {
            let stopped = crate::backend::run_backend_script_unlocked(
                app.clone(),
                "Stop-PrivacyShield".to_string(),
                HashMap::new(),
            )
            .await?;
            if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true)
                || fleet_shield_running_state(&privacy_shield_status(app).await?)?
            {
                return Err("local Stop could not be confirmed during Shield policy update".into());
            }
        }
        let state = parse_fleet_shield_state(fleet_sync_shield_state().await?)?;
        if !prior_failed {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::IntentSuperseded),
            )
            .await?;
        }
        crate::settings::patch_settings(serde_json::json!({ "app": { "fleet": {
            "privacyShieldStoppedPolicyHold": {
                "epochVersion": settings.policy.master_config_version.unwrap_or(0),
                "shieldRevision": state.as_ref().map_or(0, |state| state.revision),
                "commandId": state.as_ref().and_then(|state| state.command_id.clone()),
            }
        }}}))?;
        store_shield_policy_transaction(None)?;
        return Ok(());
    }
    if shield_recovery_checkpoint(
        transaction.epoch_version,
        settings.policy.master_config_version.unwrap_or(0),
    ) == ShieldRecoveryCheckpoint::BeforeCommit
    {
        if prior_failed {
            return Err("Shield awaits a newly verified signed pre-commit retry".into());
        }
        // Crash recovery after the durable marker and before the signed
        // commit. Keep the detector paused; the next policy sync re-verifies
        // and commits the same signed epoch.
        if running {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Pausing,
                None,
            )
            .await?;
            let stopped = crate::backend::run_backend_script_unlocked(
                app.clone(),
                "Stop-PrivacyShield".to_string(),
                HashMap::new(),
            )
            .await?;
            if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true)
                || fleet_shield_running_state(&privacy_shield_status(app).await?)?
            {
                return Err("Shield could not recover its pre-commit pause".into());
            }
        }
        if transaction.was_running {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Paused,
                None,
            )
            .await?;
        }
        return Err("Shield is paused while its signed policy epoch is pending".into());
    }
    if shield_recovery_checkpoint(
        transaction.epoch_version,
        settings.policy.master_config_version.unwrap_or(0),
    ) == ShieldRecoveryCheckpoint::Superseded
    {
        if !prior_failed {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::IntentSuperseded),
            )
            .await?;
        }
        store_shield_policy_transaction(None)?;
        return Ok(());
    }
    if fleet_shield_tuning_hash(&settings.ideal.privacy.privacy_shield) != transaction.expected_hash
    {
        if !prior_failed {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::StoredSettingsMismatch),
            )
            .await?;
        }
        return Err("stored Shield settings differ from the signed policy candidate".into());
    }
    let state = fleet_sync_shield_state().await?;
    let state = parse_fleet_shield_state(state)?;
    if shield_policy_intent_is_superseded(&transaction, state.as_ref()) {
        if running {
            let stopped = crate::backend::run_backend_script_unlocked(
                app.clone(),
                "Stop-PrivacyShield".to_string(),
                HashMap::new(),
            )
            .await?;
            if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true)
                || fleet_shield_running_state(&privacy_shield_status(app).await?)?
            {
                return Err("newer Shield request could not stop the old session".into());
            }
        }
        if !prior_failed {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::IntentSuperseded),
            )
            .await?;
        }
        store_shield_policy_transaction(None)?;
        return Ok(());
    }
    if prior_failed {
        // Retry only after the committed epoch, stored values and latest
        // desired state have been revalidated. A fresh attempt begins with
        // Received and can then observe/repair the native state.
        transaction.attempt_id = uuid::Uuid::new_v4().to_string();
        transaction.attempt_count = transaction.attempt_count.saturating_add(1).max(1);
        transaction.last_stage = None;
        transaction.retry_not_before_ms = 0;
        store_shield_policy_transaction(Some(&transaction))?;
        advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Received, None)
            .await?;
    }
    if !transaction.was_running {
        if running {
            let stopped = crate::backend::run_backend_script_unlocked(
                app.clone(),
                "Stop-PrivacyShield".to_string(),
                HashMap::new(),
            )
            .await?;
            if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true)
                || fleet_shield_running_state(&privacy_shield_status(app).await?)?
            {
                return Err("Shield started unexpectedly during a stopped policy update".into());
            }
        }
        advance_shield_policy_progress(
            &mut transaction,
            ShieldPolicyProgressStage::SettingsApplied,
            None,
        )
        .await?;
        advance_shield_policy_progress(
            &mut transaction,
            ShieldPolicyProgressStage::Verifying,
            None,
        )
        .await?;
        advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Verified, None)
            .await?;
        crate::settings::patch_settings(serde_json::json!({ "app": { "fleet": {
            "privacyShieldStoppedPolicyHold": {
                "epochVersion": transaction.epoch_version,
                "shieldRevision": transaction.shield_revision,
                "commandId": state.as_ref().and_then(|state| state.command_id.clone()),
            }
        }}}))?;
        store_shield_policy_transaction(None)?;
        return Ok(());
    }
    let local_session = !transaction.was_fleet_owned
        && transaction.pre_pause_mode.is_some()
        && state
            .as_ref()
            .is_none_or(|state| !state.enabled && state.command_id.is_none());
    let state = if local_session {
        crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: transaction
                .pre_pause_mode
                .clone()
                .ok_or("pre-pause local Shield mode is unavailable")?,
            revision: transaction.shield_revision,
            updated_at: String::new(),
            command_id: None,
        }
    } else {
        state.ok_or_else(|| "Shield desired state is unavailable".to_string())?
    };
    let params = if local_session {
        local_shield_restart_params(
            &state.mode,
            transaction
                .pre_pause_camera_index
                .ok_or("pre-pause local Shield camera is unavailable")?,
            transaction
                .pre_pause_native_options
                .as_ref()
                .ok_or("pre-pause local Shield options are unavailable")?,
            &settings.ideal.privacy.privacy_shield,
        )?
    } else {
        fleet_shield_start_params(&state, &settings.ideal.privacy.privacy_shield)?
    };
    if running && !fleet_shield_launch_matches(&native, &state, &params) {
        advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Pausing, None)
            .await?;
        let stopped = crate::backend::run_backend_script_unlocked(
            app.clone(),
            "Stop-PrivacyShield".to_string(),
            HashMap::new(),
        )
        .await?;
        if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::StopRejected),
            )
            .await?;
            return Err("Shield retry pause was rejected".into());
        }
        running = fleet_shield_running_state(&privacy_shield_status(app).await?)?;
        if running {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::StopNotObserved),
            )
            .await?;
            return Err("Shield retry pause was not observed".into());
        }
        advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Paused, None)
            .await?;
    }
    advance_shield_policy_progress(
        &mut transaction,
        ShieldPolicyProgressStage::SettingsApplied,
        None,
    )
    .await?;
    let native = if running {
        native
    } else {
        advance_shield_policy_progress(
            &mut transaction,
            ShieldPolicyProgressStage::Restarting,
            None,
        )
        .await?;
        let current = if local_session {
            local_shield_intent_still_current(&transaction).await?
        } else {
            fleet_shield_intent_still_current(
                &state,
                Some(transaction.epoch_version),
                &transaction.expected_hash,
            )
            .await?
        };
        if !current {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::IntentSuperseded),
            )
            .await?;
            store_shield_policy_transaction(None)?;
            return Ok(());
        }
        let started = crate::backend::run_backend_script_unlocked(
            app.clone(),
            "Start-PrivacyShield".to_string(),
            params.clone(),
        )
        .await?;
        if started.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::StartRejected),
            )
            .await?;
            return Err("Shield restart was rejected after policy commit".into());
        }
        privacy_shield_status(app).await?
    };
    advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Verifying, None)
        .await?;
    if !fleet_shield_running_state(&native)? {
        advance_shield_policy_progress(
            &mut transaction,
            ShieldPolicyProgressStage::Failed,
            Some(ShieldPolicyProgressReason::StartNotObserved),
        )
        .await?;
        return Err("Shield restart was not observed".into());
    }
    if !fleet_shield_launch_matches(&native, &state, &params) {
        if fleet_shield_native_options(&native, true).is_some() {
            advance_shield_policy_progress(
                &mut transaction,
                ShieldPolicyProgressStage::Failed,
                Some(ShieldPolicyProgressReason::NativeOptionsMismatch),
            )
            .await?;
            return Err("Shield native options do not match the signed policy".into());
        }
        // Windows can hide the process command line. Keep Verifying and
        // retry observation without another stop/start cycle.
        return Err("Shield native launch options are unavailable".into());
    }
    let current = if local_session {
        local_shield_intent_still_current(&transaction).await?
    } else {
        fleet_shield_intent_still_current(
            &state,
            Some(transaction.epoch_version),
            &transaction.expected_hash,
        )
        .await?
    };
    if !current {
        let stopped = crate::backend::run_backend_script_unlocked(
            app.clone(),
            "Stop-PrivacyShield".to_string(),
            HashMap::new(),
        )
        .await?;
        if stopped.get("success").and_then(serde_json::Value::as_bool) != Some(true)
            || fleet_shield_running_state(&privacy_shield_status(app).await?)?
        {
            return Err("newer Shield request could not stop the old session".into());
        }
        advance_shield_policy_progress(
            &mut transaction,
            ShieldPolicyProgressStage::Failed,
            Some(ShieldPolicyProgressReason::IntentSuperseded),
        )
        .await?;
        store_shield_policy_transaction(None)?;
        return Ok(());
    }
    advance_shield_policy_progress(&mut transaction, ShieldPolicyProgressStage::Verified, None)
        .await?;
    crate::settings::patch_settings(serde_json::json!({ "app": { "fleet": {
        "privacyShieldSessionOwned": transaction.was_fleet_owned && !local_session,
        "privacyShieldAppliedRevision": state.revision,
        "privacyShieldAppliedCommandId": state.command_id.clone(),
        "privacyShieldAppliedTuningHash": transaction.expected_hash,
        "privacyShieldStoppedPolicyHold": null,
    }}}))?;
    store_shield_policy_transaction(None)?;
    crate::set_tray_shield_running(app, true);
    report_fleet_shield_once(
        app.clone(),
        reported_for_command,
        state.command_id.as_deref(),
        if transaction.was_fleet_owned && !local_session {
            "running_fleet_session"
        } else {
            "running_local_session"
        },
    );
    Ok(())
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
    // The Pro agent owns device enrollment. A second Windows user's local
    // app.fleet.enabled cache can be false while that machine agent is live;
    // Pro must check its shared machine status before accepting this alert.
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

/// Queue a content-minimized security alert when the Fleet administrator has
/// required reporting for every device alert. Background monitors use this
/// instead of reading a UI-specific notification preference: the signed,
/// lockable master policy is the sole decision point.
///
/// This deliberately accepts only closed metadata chosen by the producer. Do
/// not add device names, paths, clipboard data, or other local evidence here.
pub fn report_required_device_alert(
    alert_type: &'static str,
    class: &'static str,
    severity: &'static str,
) {
    let Ok(settings) = crate::settings::read_settings() else {
        return;
    };
    if !should_report_required_device_alert(&settings) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        if let Err(error) = fleet_report_local_alert(
            alert_type.to_string(),
            serde_json::json!({ "class": class, "severity": severity }),
        )
        .await
        {
            crate::log_message(
                "warn",
                &format!("[Fleet] required device alert could not be queued: {error}"),
            );
        }
    });
}

fn should_report_required_device_alert(settings: &crate::settings::AppSettings) -> bool {
    settings.app.fleet.enabled && settings.ideal.security.require_all_device_alerts_in_fleet
}

/// Hand one content-free Clipboard Guard match to Pro's existing bounded
/// clipboard-event outbox. The Free producer keeps its own pending record
/// until Pro has accepted the stable event id, so a sidecar outage cannot
/// silently discard the match.
pub async fn fleet_report_clipboard_event(
    report: ClipboardEventReport,
) -> Result<serde_json::Value, String> {
    crate::license::require_service_feature("fleet")?;
    let settings = crate::settings::read_settings()?;
    if !settings.app.fleet.enabled {
        return Err("Fleet is not enabled on this device.".to_string());
    }
    let args = serde_json::to_value(report)
        .map_err(|_| "Clipboard event could not be encoded".to_string())?;
    crate::sidecar::dispatch_paid_command("fleet_agent_clipboard_event", args).await
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

    let disconnected =
        crate::sidecar::dispatch_paid_command("fleet_agent_disconnect", serde_json::Value::Null)
            .await?;

    // Clear only the managed source after Pro confirms disconnect. Personal
    // rules stay active; transient status/disconnect failures retain both.
    crate::paste_monitor::clear_paste_monitor_fleet_policy_on_unenroll();
    Ok(disconnected)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn machine_trust_repair_is_bound_to_the_same_link_and_never_requests_leave() {
        use base64::Engine as _;
        let mut current = crate::settings::create_default_settings();
        let key = base64::engine::general_purpose::STANDARD.encode(
            ed25519_dalek::SigningKey::from_bytes(&[37; 32]).verifying_key().to_bytes(),
        );
        current.app.fleet.enabled = true;
        current.app.fleet.dispatch = true;
        current.app.fleet.server_url = "https://fleet.example.invalid".into();
        current.app.fleet.signing_key_pub = key.clone();
        current.policy.fleet_signing_key = Some(key.clone());
        let plan = crate::settings::fleet_enrollment::ConnectionPlan::new(
            &current, current.app.fleet.server_url.clone(), true, key.clone(),
        ).unwrap();
        let binding = plan.confirmation_binding();
        assert!(super::repair_connection_matches(&plan, &binding));
        current.device_id = uuid::Uuid::new_v4().to_string();
        let changed = crate::settings::fleet_enrollment::ConnectionPlan::new(
            &current, current.app.fleet.server_url.clone(), true, key,
        ).unwrap();
        assert!(!super::repair_connection_matches(&changed, &binding));

        let source = include_str!("fleet_agent.rs");
        let repair = source.split("pub async fn fleet_repair_machine_trust").nth(1).unwrap()
            .split("fn repair_connection_matches").next().unwrap();
        let connect = source.split("async fn fleet_connect_inner").nth(1).unwrap()
            .split("pub async fn fleet_status").next().unwrap();
        for forbidden in ["fleet_request_unenroll", "fleet_disconnect", "fleet_agent_disconnect", "fleet_agent_request_unenroll"] {
            assert!(!repair.contains(forbidden), "repair must not call {forbidden}");
            assert!(!connect.contains(forbidden), "connect must not call {forbidden}");
        }
    }

    #[test]
    fn fresh_profile_cannot_bypass_machine_fleet_policy() {
        let fresh = crate::settings::create_default_settings();
        assert!(super::verify_shield_profile_against_service(
            &fresh, &json!({"trust_bound": true, "policy": null})
        ).is_err());
        assert!(super::verify_shield_profile_against_service(
            &fresh, &json!({"policy": null})
        ).is_err());
        assert!(super::verify_shield_profile_against_service(
            &fresh, &json!({"trust_bound": false, "policy": null})
        ).is_ok());
        let mut stale = fresh;
        stale.policy.master_config_version = Some(7);
        assert!(super::verify_shield_profile_against_service(
            &stale, &json!({"trust_bound": false, "policy": null})
        ).is_err());
    }

    #[test]
    fn service_trust_replacement_requires_confirmed_identity_change() {
        let old = json!({"org_id":"org-a","device_id":"device-a","enrollment_generation":"generation-a","signer_key":"key-a"});
        let new = json!({"org_id":"org-b","device_id":"device-a","enrollment_generation":"generation-b","signer_key":"key-b"});
        assert_eq!(super::service_enrollment_action(None, &new, false).unwrap(), Some("svc.fleet.provision_trust"));
        assert_eq!(super::service_enrollment_action(Some(&old), &old, false).unwrap(), None);
        assert!(super::service_enrollment_action(Some(&old), &new, false).is_err());
        assert_eq!(super::service_enrollment_action(Some(&old), &new, true).unwrap(), Some("svc.fleet.replace_trust"));
    }

    #[test]
    fn status_bridge_forwards_only_a_detector_confirmed_mode() {
        assert_eq!(
            super::fleet_shield_reported_mode(&json!({ "activeMode": "notify_only" })),
            Some("notify_only".to_string())
        );
        assert_eq!(
            super::fleet_shield_reported_mode(&json!({ "activeMode": "blur_notify" })),
            Some("blur_notify".to_string())
        );
        // Process-status gaps and unbounded values cannot become a Fleet
        // receipt. The server will keep the command unverified instead.
        assert_eq!(super::fleet_shield_reported_mode(&json!({})), None);
        assert_eq!(
            super::fleet_shield_reported_mode(&json!({ "activeMode": "blur_everything" })),
            None
        );
    }

    #[test]
    fn native_fleet_supervisor_preserves_notify_only_without_blur() {
        let state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "notify_only".to_string(),
            revision: 9,
            updated_at: "2026-09-15T10:00:00Z".to_string(),
            command_id: Some("command-9".to_string()),
        };
        let shield = crate::settings::PrivacyShieldSettings {
            gaze_detection_enabled: Some(true),
            anti_peeping_enabled: Some(true),
            camera_hunter_enabled: Some(true),
            ..Default::default()
        };
        let params = super::fleet_shield_start_params(&state, &shield).unwrap();
        assert_eq!(params["CheckGaze"], "true");
        assert_eq!(params["CheckFaces"], "true");
        assert_eq!(params["CheckPhone"], "true");
        assert_eq!(params["BlurGaze"], "false");
        assert_eq!(params["BlurFaces"], "false");
        assert_eq!(params["BlurPhone"], "false");
        assert_eq!(params["Mode"], "notify_only");
    }

    #[test]
    fn explicit_blur_mode_survives_all_blur_triggers_being_disabled() {
        let state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "blur_notify".to_string(),
            revision: 9,
            updated_at: "2026-09-15T10:00:00Z".to_string(),
            command_id: Some("command-9".to_string()),
        };
        let shield = crate::settings::PrivacyShieldSettings {
            gaze_detection_enabled: Some(false),
            anti_peeping_enabled: Some(false),
            camera_hunter_enabled: Some(false),
            ..Default::default()
        };
        let params = super::fleet_shield_start_params(&state, &shield).unwrap();
        assert_eq!(params["Mode"], "blur_notify");
        assert_eq!(params["BlurGaze"], "false");
        assert_eq!(params["BlurFaces"], "false");
        assert_eq!(params["BlurPhone"], "false");
    }

    #[test]
    fn policy_hash_changes_for_each_fleet_shield_field() {
        let mut shield = crate::settings::PrivacyShieldSettings::default();
        let original = super::fleet_shield_tuning_hash(&shield);
        shield.wake_delay_seconds = Some(160);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.wake_delay_seconds = None;
        shield.device_wake_multiplier = Some(6.0);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.device_wake_multiplier = None;
        shield.multi_face_wake_multiplier = Some(6.0);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.multi_face_wake_multiplier = None;
        shield.detection_buffer_frames = Some(3);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.detection_buffer_frames = None;
        shield.look_away_notifications_enabled = Some(false);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.look_away_notifications_enabled = None;
        shield.multiple_faces_notifications_enabled = Some(false);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.multiple_faces_notifications_enabled = None;
        shield.phone_notifications_enabled = Some(false);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.phone_notifications_enabled = None;
        shield.fleet_notification_limit = Some(1);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.fleet_notification_limit = None;
        shield.fleet_notification_window_seconds = Some(120);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.fleet_notification_window_seconds = None;
        shield.gaze_detection_enabled = Some(false);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.gaze_detection_enabled = None;
        shield.anti_peeping_enabled = Some(false);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
        shield.anti_peeping_enabled = None;
        shield.camera_hunter_enabled = Some(false);
        assert_ne!(super::fleet_shield_tuning_hash(&shield), original);
    }

    #[test]
    fn stored_shield_settings_keep_independent_notification_and_blur_values() {
        let shield = crate::settings::PrivacyShieldSettings {
            gaze_detection_enabled: Some(false),
            anti_peeping_enabled: Some(true),
            camera_hunter_enabled: Some(true),
            look_away_notifications_enabled: Some(false),
            multiple_faces_notifications_enabled: Some(true),
            phone_notifications_enabled: Some(true),
            wake_delay_seconds: Some(150),
            device_wake_multiplier: Some(5.0),
            multi_face_wake_multiplier: Some(5.0),
            detection_buffer_frames: Some(2),
            fleet_notification_limit: Some(0),
            fleet_notification_window_seconds: Some(60),
            ..Default::default()
        };
        let stored = super::fleet_shield_stored_settings(&shield);
        assert_eq!(stored.gaze_detection_enabled, Some(false));
        assert_eq!(stored.look_away_notifications_enabled, Some(false));
        assert_eq!(stored.anti_peeping_enabled, Some(true));
        assert_eq!(stored.multiple_faces_notifications_enabled, Some(true));
        assert_eq!(stored.camera_hunter_enabled, Some(true));
        assert_eq!(stored.phone_notifications_enabled, Some(true));
        assert_eq!(stored.fleet_notification_limit, Some(0));
        assert_eq!(stored.fleet_notification_window_seconds, Some(60));
        let mut invalid = shield.clone();
        invalid.wake_delay_seconds = Some(1);
        let partial = super::fleet_shield_stored_settings(&invalid);
        assert_eq!(partial.wake_delay_seconds, None);
        assert_eq!(partial.phone_notifications_enabled, Some(true));
        let state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "blur_notify".into(),
            revision: 1,
            updated_at: String::new(),
            command_id: None,
        };
        let params = super::fleet_shield_start_params(&state, &shield).unwrap();
        assert_eq!(params["CheckGaze"], "true");
        assert_eq!(params["BlurGaze"], "false");
        assert_eq!(params["CheckFaces"], "true");
        assert_eq!(params["BlurFaces"], "true");
        assert_eq!(params["CheckPhone"], "true");
        assert_eq!(params["BlurPhone"], "true");
    }

    #[test]
    fn unreadable_native_shield_status_is_not_stopped() {
        assert!(super::fleet_shield_running_state(&json!({"error": true})).is_err());
        assert!(super::fleet_shield_running_state(&json!({})).is_err());
        assert_eq!(
            super::fleet_shield_running_state(&json!({"running": false})).unwrap(),
            false
        );
    }

    #[test]
    fn newer_stop_or_policy_epoch_supersedes_in_flight_start() {
        let start = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "blur_notify".into(),
            revision: 10,
            updated_at: "2026-10-29T00:00:00Z".into(),
            command_id: Some("start-10".into()),
        };
        assert!(super::fleet_shield_intent_matches(
            &start,
            Some(&start),
            Some(4),
            Some(4),
            "hash",
            "hash"
        ));
        let stop = crate::settings::FleetShieldDesiredState {
            enabled: false,
            revision: 11,
            command_id: Some("stop-11".into()),
            ..start.clone()
        };
        assert!(!super::fleet_shield_intent_matches(
            &start,
            Some(&stop),
            Some(4),
            Some(4),
            "hash",
            "hash"
        ));
        assert!(!super::fleet_shield_intent_matches(
            &start,
            Some(&start),
            Some(4),
            Some(5),
            "hash",
            "hash"
        ));
        assert!(!super::fleet_shield_intent_matches(
            &start,
            Some(&start),
            Some(4),
            Some(4),
            "hash",
            "new-hash"
        ));
        assert!(!super::fleet_shield_intent_matches(
            &start,
            None,
            Some(4),
            Some(4),
            "hash",
            "hash"
        ));
    }

    #[test]
    fn native_option_bridge_requires_complete_closed_running_process_sample() {
        let native = serde_json::json!({ "activeMode": "blur_notify", "cameraIndex": 0, "nativeOptions": {
            "checkGaze": true, "checkFaces": true, "checkPhone": true,
            "blurGaze": true, "blurFaces": false, "blurPhone": false,
            "captureOnDevice": false, "captureOnMultiFace": false,
            "modelLevel": "medium", "confidence": 0.5, "overlayOpacity": 200,
            "wakeDelayMs": 150, "deviceWakeMultiplier": 5.0,
            "multiFaceWakeMultiplier": 5.0, "bufferFrames": 2, "captureSpeed": 1,
        }});
        assert!(super::fleet_shield_native_options(&native, true).is_some());
        assert!(super::fleet_shield_native_options(&native, false).is_none());
        let state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "blur_notify".into(),
            revision: 1,
            updated_at: String::new(),
            command_id: None,
        };
        let shield = crate::settings::PrivacyShieldSettings {
            anti_peeping_enabled: Some(false),
            camera_hunter_enabled: Some(false),
            ..Default::default()
        };
        let params = super::fleet_shield_start_params(&state, &shield).unwrap();
        assert!(super::fleet_shield_launch_matches(&native, &state, &params));
        let mut changed_camera = native.clone();
        changed_camera["cameraIndex"] = serde_json::json!(2);
        assert!(!super::fleet_shield_launch_matches(
            &changed_camera,
            &state,
            &params
        ));
        let mut mismatched = native.clone();
        mismatched["nativeOptions"]["blurGaze"] = serde_json::json!(false);
        assert!(!super::fleet_shield_launch_matches(
            &mismatched,
            &state,
            &params
        ));
        mismatched = native.clone();
        mismatched["activeMode"] = serde_json::json!("notify_only");
        assert!(!super::fleet_shield_launch_matches(
            &mismatched,
            &state,
            &params
        ));
        let mut leaked = native.clone();
        leaked["nativeOptions"]["commandLine"] = serde_json::json!("private path");
        assert!(super::fleet_shield_native_options(&leaked, true).is_none());
        let mut missing = native;
        missing["nativeOptions"]
            .as_object_mut()
            .unwrap()
            .remove("checkGaze");
        assert!(super::fleet_shield_native_options(&missing, true).is_none());
    }

    #[test]
    fn native_fleet_supervisor_rejects_unknown_shield_modes() {
        let state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "not-a-mode".to_string(),
            revision: 9,
            updated_at: "2026-09-15T10:00:00Z".to_string(),
            command_id: None,
        };
        assert!(super::fleet_shield_start_params(
            &state,
            &crate::settings::PrivacyShieldSettings::default(),
        )
        .is_err());
    }

    #[test]
    fn native_fleet_supervisor_honours_blur_notify_when_tuning_has_not_arrived() {
        let state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "blur_notify".to_string(),
            revision: 9,
            updated_at: "2026-09-16T10:00:00Z".to_string(),
            command_id: Some("command-9".to_string()),
        };
        let params = super::fleet_shield_start_params(
            &state,
            &crate::settings::PrivacyShieldSettings::default(),
        )
        .unwrap();
        assert_eq!(params["BlurGaze"], "true");
        assert_eq!(params["BlurFaces"], "true");
        assert_eq!(params["BlurPhone"], "true");
    }

    #[test]
    fn local_shield_readback_reports_both_edges_without_poll_spam() {
        let mut reported = std::collections::BTreeSet::new();
        reported.insert(":stopped".to_string());
        super::reconcile_fleet_shield_local_observation_keys(&mut reported, "stopped");
        assert_eq!(
            reported,
            std::collections::BTreeSet::from([":stopped".to_string()])
        );
        super::reconcile_fleet_shield_local_observation_keys(
            &mut reported,
            "running_local_session",
        );
        reported.insert(":running_local_session".to_string());
        assert_eq!(
            reported,
            std::collections::BTreeSet::from([":running_local_session".to_string()])
        );
        super::reconcile_fleet_shield_local_observation_keys(&mut reported, "stopped");
        reported.insert(":stopped".to_string());
        assert_eq!(
            reported,
            std::collections::BTreeSet::from([":stopped".to_string()])
        );
    }

    #[test]
    fn applied_fleet_stop_does_not_stop_a_later_local_session() {
        let state = crate::settings::FleetShieldDesiredState {
            enabled: false,
            mode: "notify_only".to_string(),
            revision: 12,
            updated_at: "2026-09-25T10:00:00Z".to_string(),
            command_id: Some("stop-12".to_string()),
        };

        let first_application = super::fleet_shield_requires_reconfiguration(None, None, &state);
        assert!(super::fleet_shield_should_apply_stop(
            true,
            &state,
            false,
            first_application
        ));

        let repeated_tick = super::fleet_shield_requires_reconfiguration(
            Some(state.revision),
            state.command_id.as_deref(),
            &state,
        );
        assert!(!super::fleet_shield_should_apply_stop(
            true,
            &state,
            false,
            repeated_tick
        ));
        assert!(!super::fleet_shield_should_apply_stop(
            false,
            &state,
            false,
            first_application
        ));
    }

    #[test]
    fn organization_off_default_keeps_a_local_shield_session_running() {
        let state = crate::settings::FleetShieldDesiredState {
            enabled: false,
            mode: "notify_only".to_string(),
            revision: 13,
            updated_at: "2026-09-25T10:00:00Z".to_string(),
            command_id: None,
        };

        assert!(!super::fleet_shield_should_apply_stop(
            true, &state, false, true
        ));
        assert!(super::fleet_shield_should_apply_stop(
            true, &state, true, true
        ));
    }

    #[test]
    fn native_fleet_supervisor_requires_command_identity_after_revision_replay() {
        let state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "notify_only".to_string(),
            revision: 10,
            updated_at: "2026-09-15T10:00:00Z".to_string(),
            command_id: Some("command-10".to_string()),
        };
        assert!(super::fleet_shield_requires_reconfiguration(
            Some(9),
            Some("command-9"),
            &state
        ));
        assert!(!super::fleet_shield_requires_reconfiguration(
            Some(10),
            Some("command-10"),
            &state
        ));
        assert!(super::fleet_shield_requires_reconfiguration(
            Some(10),
            Some("pre-reset-command"),
            &state
        ));
        assert!(super::fleet_shield_requires_reconfiguration(
            Some(10),
            None,
            &state
        ));
    }

    #[test]
    fn fleet_operations_use_the_explicit_service_gate_but_status_is_read_only() {
        let source = include_str!("fleet_agent.rs").replace("\r\n", "\n");
        let service_gate = ["require_service_feature(\"", "fleet\")"].concat();
        let obsolete_gate = ["require", "_paid(\"fleet agent\")"].concat();
        assert_eq!(
            source.matches(&service_gate).count(),
            8,
            "Fleet mutations require the service gate; machine-scoped status, Shield desired-state reads, and Shield read-back receipts deliberately do not"
        );
        let status_start = source
            .find("pub async fn fleet_status")
            .expect("fleet_status command");
        let status_end = source[status_start..]
            .find("\n}\n")
            .map(|offset| status_start + offset)
            .expect("fleet_status end");
        assert!(
            !source[status_start..status_end].contains(&service_gate),
            "the link-status bridge must remain readable without a per-user Fleet setting"
        );
        let shield_status_start = source
            .find("pub async fn fleet_report_privacy_shield_status")
            .expect("privacy-shield status bridge");
        let shield_status_end = source[shield_status_start..]
            .find("\n}\n")
            .map(|offset| shield_status_start + offset)
            .expect("privacy-shield status bridge end");
        assert!(
            !source[shield_status_start..shield_status_end].contains(&service_gate),
            "Shield device read-back must not be blocked by a stale per-user Fleet cache"
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

    #[test]
    fn required_device_alerts_need_fleet_and_the_signed_master_policy() {
        let mut settings = crate::settings::create_default_settings();
        assert!(!super::should_report_required_device_alert(&settings));

        settings.app.fleet.enabled = true;
        assert!(!super::should_report_required_device_alert(&settings));

        settings.ideal.security.require_all_device_alerts_in_fleet = true;
        assert!(super::should_report_required_device_alert(&settings));
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

    #[test]
    fn approved_disconnect_clears_fleet_policy_only_after_sidecar_disconnect_succeeds() {
        let source = include_str!("fleet_agent.rs").replace("\r\n", "\n");
        let disconnect = source
            .find("dispatch_paid_command(\"fleet_agent_disconnect\"")
            .expect("disconnect dispatch must remain present");
        let clear = source
            .find("clear_paste_monitor_fleet_policy_on_unenroll();")
            .expect("approved unenroll must clear the Fleet policy source");
        assert!(
            disconnect < clear,
            "Fleet policy must remain installed when the disconnect dispatch fails"
        );
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

    fn shield_transaction_fixture() -> crate::settings::ShieldPolicyTransaction {
        crate::settings::ShieldPolicyTransaction {
            epoch_version: 44,
            epoch_signature_hash: "a".repeat(64),
            resolution_revision: 2,
            shield_revision: 7,
            attempt_id: uuid::Uuid::new_v4().to_string(),
            was_running: true,
            was_fleet_owned: false,
            owner_session_id: Some(1),
            pre_pause_mode: Some("notify_only".into()),
            pre_pause_camera_index: Some(2),
            pre_pause_native_options: None,
            local_stop_requested: false,
            attempt_count: 1,
            retry_not_before_ms: 0,
            expected_hash: "candidate".into(),
            last_stage: Some(wincmd_shared::fleet::ShieldPolicyProgressStage::Paused),
        }
    }

    #[test]
    fn shield_crash_checkpoints_never_resume_an_old_epoch_as_current() {
        use super::ShieldRecoveryCheckpoint as Checkpoint;
        assert_eq!(
            super::shield_recovery_checkpoint(44, 43),
            Checkpoint::BeforeCommit
        );
        assert_eq!(
            super::shield_recovery_checkpoint(44, 44),
            Checkpoint::Committed
        );
        assert_eq!(
            super::shield_recovery_checkpoint(44, 45),
            Checkpoint::Superseded
        );
        let paused = shield_transaction_fixture();
        let persisted: crate::settings::ShieldPolicyTransaction =
            serde_json::from_value(serde_json::to_value(&paused).unwrap()).unwrap();
        assert_eq!(persisted.last_stage, paused.last_stage);
        assert_eq!(persisted.attempt_id, paused.attempt_id);
        assert!(!persisted.was_fleet_owned);
        assert_eq!(persisted.epoch_signature_hash, "a".repeat(64));
    }

    #[test]
    fn shield_session_guard_rejects_second_or_disconnected_user_session() {
        assert!(super::single_shield_session_matches(5, &[5]));
        assert!(!super::single_shield_session_matches(5, &[5, 8]));
        assert!(!super::single_shield_session_matches(5, &[8]));
        assert!(!super::single_shield_session_matches(0, &[0]));
        assert!(!super::single_shield_session_matches(5, &[]));
    }

    #[test]
    fn newer_stop_or_local_stop_supersedes_paused_or_restarting_policy() {
        let mut transaction = shield_transaction_fixture();
        let mut state = crate::settings::FleetShieldDesiredState {
            enabled: true,
            mode: "notify_only".into(),
            revision: 7,
            updated_at: "2026-10-29T00:00:00Z".into(),
            command_id: None,
        };
        assert!(!super::shield_policy_intent_is_superseded(
            &transaction,
            Some(&state)
        ));
        state.revision = 8;
        state.enabled = false;
        assert!(super::shield_policy_intent_is_superseded(
            &transaction,
            Some(&state)
        ));
        transaction.last_stage = Some(wincmd_shared::fleet::ShieldPolicyProgressStage::Restarting);
        assert!(super::shield_policy_intent_is_superseded(
            &transaction,
            Some(&state)
        ));
        state.revision = 7;
        state.enabled = true;
        transaction.local_stop_requested = true;
        assert!(super::shield_policy_intent_is_superseded(
            &transaction,
            Some(&state)
        ));
        transaction.local_stop_requested = false;
        state.enabled = false;
        assert!(!super::shield_policy_intent_is_superseded(
            &transaction,
            Some(&state)
        ));
        state.command_id = Some("explicit-stop".into());
        assert!(super::shield_policy_intent_is_superseded(
            &transaction,
            Some(&state)
        ));
        state.command_id = None;
        transaction.shield_revision = 0;
        assert!(!super::shield_policy_intent_is_superseded(
            &transaction,
            None
        ));
    }

    #[test]
    fn failed_policy_retry_is_bounded_and_delayed() {
        let mut transaction = shield_transaction_fixture();
        transaction.last_stage = Some(wincmd_shared::fleet::ShieldPolicyProgressStage::Failed);
        transaction.retry_not_before_ms = chrono::Utc::now().timestamp_millis() + 60_000;
        assert!(!super::shield_retry_is_due(&transaction));
        transaction.retry_not_before_ms = 0;
        assert!(super::shield_retry_is_due(&transaction));
        transaction.attempt_count = 3;
        assert!(!super::shield_retry_is_due(&transaction));
    }

    #[test]
    fn local_restart_preserves_mode_and_switches_while_applying_signed_tuning() {
        let native: wincmd_shared::fleet::ShieldNativeOptions = serde_json::from_value(json!({
            "checkGaze": false, "checkFaces": true, "checkPhone": true,
            "blurGaze": false, "blurFaces": false, "blurPhone": false,
            "captureOnDevice": true, "captureOnMultiFace": false,
            "modelLevel": "medium", "confidence": 0.5, "overlayOpacity": 200,
            "wakeDelayMs": 150, "deviceWakeMultiplier": 5.0,
            "multiFaceWakeMultiplier": 5.0, "bufferFrames": 2, "captureSpeed": 1
        }))
        .unwrap();
        let mut shield = crate::settings::PrivacyShieldSettings::default();
        shield.wake_delay_seconds = Some(240);
        let params =
            super::local_shield_restart_params("notify_only", 2, &native, &shield).unwrap();
        assert_eq!(params["Camera"], "2");
        assert_eq!(params["Mode"], "notify_only");
        assert_eq!(params["CheckGaze"], "false");
        assert_eq!(params["CaptureOnDevice"], "true");
        assert_eq!(params["WakeDelayMs"], "240");
        assert_eq!(params["BlurGaze"], "false");
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
