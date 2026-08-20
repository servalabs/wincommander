// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/usb_auto_sandbox.rs
//
// ═══════════════════════════════════════════════════════════════════════
// USB U-F — AUTO-SANDBOX / QUARANTINE ORCHESTRATION  (Free, in-process)
// ═══════════════════════════════════════════════════════════════════════
//
// Subscribes to U-A's broadcast channel and, on every Attached event,
// evaluates a pure decision function (decide_auto_action) against the
// current config to determine whether to Alert or Quarantine.
//
// SAFETY-CRITICAL: default mode is OBSERVE (alert-only).
// ENFORCE (auto-quarantine) is an explicit user opt-in and applies ONLY to
// removable mass-storage (+ optionally HID for BadUSB) — never arbitrary
// devices.
//
// Enforcement is delegated to the Pro sidecar via
//   dispatch_paid_command("Invoke-UsbQuarantine", { instanceId })
// keeping the Free binary AV-clean (no mountvol/Set-Disk/diskpart/registry
// strings in this file — see spec §"AV-clean rationale").
//
// Pattern mirrors usb_metering.rs / usb_hid_guard.rs:
//   OnceLock statics, epoch+abort-handle stop, subscribe() / broadcast
//   Closed + Lagged handling, require_paid on mutations.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::usb_monitor::{subscribe, UsbEvent};

// ── Constants ────────────────────────────────────────────────────────────────

/// Ring capacity for recent auto-action records.
const RECENT_CAP: usize = 50;

/// Per-device attach-debounce in milliseconds (mirrors U-A ATTACH_DEBOUNCE_MS).
const DEBOUNCE_MS: u64 = 2_000;

// ── Types ────────────────────────────────────────────────────────────────────

/// Operating mode for the auto-sandbox monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Monitor is disabled; no alerts or quarantines.
    Off,
    /// Default: alert + evidence only; no enforcement action.
    #[default]
    Observe,
    /// Explicit opt-in: auto-quarantine via Pro sidecar.
    /// Applies ONLY to mass-storage (+ HID if act_on_hid is true).
    Enforce,
}

/// Runtime configuration — stored in CONFIG static.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Operating mode (Off | Observe | Enforce). Default: Observe.
    pub mode: Mode,
    /// Device keys (U-A "USB:{vid}:{pid}:{serial}") that are always ignored.
    /// Comparison is case-insensitive.
    pub allow_keys: Vec<String>,
    /// Vendor IDs (4-hex, e.g. "05ac") whose devices are always ignored.
    /// Comparison is case-insensitive.
    pub allow_vids: Vec<String>,
    /// When true, HID devices are also in-scope for Observe/Enforce.
    /// Default: false (storage-only by default, for BadUSB caution).
    pub act_on_hid: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Observe,
            allow_keys: Vec::new(),
            allow_vids: Vec::new(),
            act_on_hid: false,
        }
    }
}

/// The decision returned by `decide_auto_action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoAction {
    Ignore,
    Alert,
    Quarantine,
}

/// Decision for forensic read-only handling of newly attached storage.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForensicReadonlyAction {
    Ignore,
    MountReadOnly,
}

/// One entry in the recent auto-action ring.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoActionRecord {
    /// ISO-8601 UTC time of the action.
    pub time: String,
    pub device_key: String,
    pub friendly_name: String,
    pub action: AutoAction,
    /// true if Quarantine was dispatched to Pro.
    pub enforced: bool,
    /// Detail string (Pro dispatch result or alert reason).
    pub detail: String,
}

/// Status shape returned by usb_autosandbox_status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoSandboxStatus {
    pub running: bool,
    pub mode: Mode,
    pub recent_count: usize,
}

// ── Process-lifetime singletons ───────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static RUN_EPOCH: AtomicU64 = AtomicU64::new(0);

static CONFIG: OnceLock<Mutex<Config>> = OnceLock::new();
static RECENT: OnceLock<Mutex<VecDeque<AutoActionRecord>>> = OnceLock::new();
/// Per-device-key debounce: last fire timestamp (ms since UNIX epoch).
static DEBOUNCE: OnceLock<Mutex<std::collections::HashMap<String, u64>>> = OnceLock::new();
/// Background listener task handle — aborted on stop.
static LISTENER: OnceLock<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> = OnceLock::new();

fn cfg() -> &'static Mutex<Config> {
    CONFIG.get_or_init(|| Mutex::new(Config::default()))
}

fn recent() -> &'static Mutex<VecDeque<AutoActionRecord>> {
    RECENT.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn debounce_map() -> &'static Mutex<std::collections::HashMap<String, u64>> {
    DEBOUNCE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn listener_slot() -> &'static Mutex<Option<tauri::async_runtime::JoinHandle<()>>> {
    LISTENER.get_or_init(|| Mutex::new(None))
}

// ── Pure decision function (no I/O — unit-testable) ─────────────────────────

/// Decide what automatic action to take for a just-attached device.
///
/// Rules (in priority order):
/// 1. Off  → Ignore regardless.
/// 2. Allowlisted (key or vid) → Ignore regardless.
/// 3. Not in scope (not mass-storage AND (not HID OR act_on_hid is false)) → Ignore.
/// 4. Observe + in-scope + not allowlisted → Alert.
/// 5. Enforce + in-scope + not allowlisted → Quarantine.
pub fn decide_auto_action(
    mode: Mode,
    is_mass_storage: bool,
    is_hid: bool,
    is_allowlisted: bool,
    act_on_hid: bool,
) -> AutoAction {
    if matches!(mode, Mode::Off) {
        return AutoAction::Ignore;
    }
    if is_allowlisted {
        return AutoAction::Ignore;
    }
    let in_scope = is_mass_storage || (is_hid && act_on_hid);
    if !in_scope {
        return AutoAction::Ignore;
    }
    match mode {
        Mode::Off => AutoAction::Ignore, // unreachable — handled above
        Mode::Observe => AutoAction::Alert,
        Mode::Enforce => AutoAction::Quarantine,
    }
}

/// Mount unknown mass-storage devices read-only when forensic mode is enabled.
#[cfg(test)]
pub fn decide_forensic_readonly_action(
    enabled: bool,
    is_mass_storage: bool,
    is_trusted: bool,
) -> ForensicReadonlyAction {
    if enabled && is_mass_storage && !is_trusted {
        ForensicReadonlyAction::MountReadOnly
    } else {
        ForensicReadonlyAction::Ignore
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn is_allowlisted(key: &str, vid: &str, config: &Config) -> bool {
    let key_lower = key.to_lowercase();
    let vid_lower = vid.to_lowercase();
    config
        .allow_keys
        .iter()
        .any(|k| k.to_lowercase() == key_lower)
        || config
            .allow_vids
            .iter()
            .any(|v| v.to_lowercase() == vid_lower)
}

fn push_recent(record: AutoActionRecord) {
    let mut q = recent().lock().unwrap();
    q.push_front(record);
    q.truncate(RECENT_CAP);
}

// ── Background task ───────────────────────────────────────────────────────────

fn spawn_listener(app: AppHandle, epoch: u64) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut rx = subscribe();

        loop {
            // Exit if stopped (epoch changed).
            if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                break;
            }

            let event = match rx.recv().await {
                Ok(ev) => ev,
                Err(RecvError::Closed) => {
                    crate::log::log_message(
                        "usb_auto_sandbox",
                        "broadcast channel closed; stopping listener",
                    );
                    break;
                }
                Err(RecvError::Lagged(n)) => {
                    crate::log::log_message(
                        "usb_auto_sandbox",
                        &format!("broadcast lagged — dropped {n} events"),
                    );
                    continue; // catch up; do NOT busy-loop by re-subscribing
                }
            };

            // Check epoch again after await point.
            if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                break;
            }

            let identity = match event {
                UsbEvent::Attached(id) => id,
                UsbEvent::Detached(_) => continue,
            };

            // ── Debounce per device key ───────────────────────────────────
            let now_ms = now_epoch_ms();
            {
                let mut db = debounce_map().lock().unwrap();
                let last = db.entry(identity.key.clone()).or_insert(0);
                if now_ms.saturating_sub(*last) < DEBOUNCE_MS {
                    continue;
                }
                *last = now_ms;
            }

            // ── Evaluate config and decide ────────────────────────────────
            let (mode, allowlisted, act_on_hid) = {
                let c = cfg().lock().unwrap();
                let al = is_allowlisted(&identity.key, &identity.vid, &c);
                (c.mode, al, c.act_on_hid)
            };

            let action = decide_auto_action(
                mode,
                identity.is_mass_storage,
                identity.is_hid,
                allowlisted,
                act_on_hid,
            );

            match action {
                AutoAction::Ignore => continue,

                AutoAction::Alert => {
                    crate::fleet_agent::report_required_device_alert(
                        "usb_security",
                        "untrusted_device",
                        "warning",
                    );
                    let title = "WinCommander - USB auto-isolate";
                    let body = format!(
                        "Untrusted USB device attached: {} — observing (Enforce mode off)",
                        identity.friendly_name
                    );
                    let _ = crate::native_notify::show_native_notification(&app, title, &body);
                    let _ = crate::evidence::evidence_record(
                        "usb_auto_sandbox".to_string(),
                        "warn".to_string(),
                        format!("USB auto-sandbox ALERT: {}", identity.friendly_name),
                        Some(format!(
                            "key={} vid={} hid={} storage={}",
                            identity.key, identity.vid, identity.is_hid, identity.is_mass_storage
                        )),
                    );
                    let _ = app.emit(
                        "usb-autosandbox-action",
                        json!({
                            "action": "alert",
                            "deviceKey": identity.key,
                            "friendlyName": identity.friendly_name,
                            "enforced": false,
                        }),
                    );
                    push_recent(AutoActionRecord {
                        time: now_iso(),
                        device_key: identity.key.clone(),
                        friendly_name: identity.friendly_name.clone(),
                        action: AutoAction::Alert,
                        enforced: false,
                        detail: "Observe mode — alert only".into(),
                    });
                }

                AutoAction::Quarantine => {
                    // Never dispatch a quarantine without a real PnP InstanceId — an
                    // empty id (record predating instance_id) would target nothing or
                    // the wrong device. Degrade to a recorded non-enforced attempt.
                    let dispatch_result = if identity.instance_id.is_empty() {
                        Err("no InstanceId for this device".to_string())
                    } else {
                        // Dispatch quarantine to Pro sidecar (keeps Free AV-clean).
                        crate::sidecar::dispatch_paid_command(
                            "Invoke-UsbQuarantine",
                            json!({ "instanceId": identity.instance_id }),
                        )
                        .await
                    };

                    let (enforced, detail) = match dispatch_result {
                        Ok(v) => {
                            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                            let err_msg = v
                                .get("error")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            if ok {
                                (
                                    true,
                                    format!(
                                        "quarantined via Pro (instanceId={})",
                                        identity.instance_id
                                    ),
                                )
                            } else {
                                (false, format!("Pro dispatch failed: {err_msg}"))
                            }
                        }
                        Err(e) => (false, format!("dispatch error: {e}")),
                    };
                    crate::fleet_agent::report_required_device_alert(
                        "usb_security",
                        if enforced {
                            "quarantined"
                        } else {
                            "quarantine_failed"
                        },
                        if enforced { "warning" } else { "danger" },
                    );

                    let title = "WinCommander - USB auto-isolate";
                    let body = if enforced {
                        format!(
                            "USB device quarantined: {} — files inaccessible until approved",
                            identity.friendly_name
                        )
                    } else {
                        format!(
                            "USB auto-quarantine attempted but failed: {} — {}",
                            identity.friendly_name, detail
                        )
                    };
                    let _ = crate::native_notify::show_native_notification(&app, title, &body);
                    let _ = crate::evidence::evidence_record(
                        "usb_auto_sandbox".to_string(),
                        if enforced {
                            "warn".to_string()
                        } else {
                            "error".to_string()
                        },
                        format!(
                            "USB auto-sandbox QUARANTINE (enforced={}): {}",
                            enforced, identity.friendly_name
                        ),
                        Some(detail.clone()),
                    );
                    let _ = app.emit(
                        "usb-autosandbox-action",
                        json!({
                            "action": "quarantine",
                            "deviceKey": identity.key,
                            "friendlyName": identity.friendly_name,
                            "enforced": enforced,
                            "detail": detail,
                        }),
                    );
                    push_recent(AutoActionRecord {
                        time: now_iso(),
                        device_key: identity.key.clone(),
                        friendly_name: identity.friendly_name.clone(),
                        action: AutoAction::Quarantine,
                        enforced,
                        detail: detail.clone(),
                    });
                }
            }
        }

        RUNNING.store(false, Ordering::Relaxed);
    })
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Start the USB auto-sandbox monitor.
/// Requires paid license; idempotent (second call is a no-op if already running).
#[tauri::command]
pub async fn start_usb_autosandbox(app: AppHandle) -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB auto-sandbox")?;
    if RUNNING.swap(true, Ordering::Relaxed) {
        return Ok(json!({ "ok": true, "already": true }));
    }
    // Refuse to arm if U-A monitor is not running (no events to subscribe to).
    if !crate::usb_monitor::is_running() {
        RUNNING.store(false, Ordering::Relaxed);
        return Err("USB monitor (U-A) is not running — enable it first".into());
    }

    let epoch = RUN_EPOCH.fetch_add(1, Ordering::Relaxed) + 1;
    let handle = spawn_listener(app, epoch);
    *listener_slot().lock().unwrap() = Some(handle);
    Ok(json!({ "ok": true, "started": true }))
}

/// Stop the USB auto-sandbox monitor.
/// Does NOT auto-release any held devices; they remain held in Pro until approved.
#[tauri::command]
pub async fn stop_usb_autosandbox() -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB auto-sandbox")?;
    RUN_EPOCH.fetch_add(1, Ordering::Relaxed);
    RUNNING.store(false, Ordering::Relaxed);
    if let Some(h) = listener_slot().lock().unwrap().take() {
        h.abort();
    }
    Ok(json!({ "ok": true, "stopped": true }))
}

/// Returns the current running state, mode, and recent-event count.
/// Read-only — ungated.
#[tauri::command]
pub fn usb_autosandbox_status() -> AutoSandboxStatus {
    let (mode, _) = {
        let c = cfg().lock().unwrap();
        (c.mode, ())
    };
    AutoSandboxStatus {
        running: RUNNING.load(Ordering::Relaxed),
        mode,
        recent_count: recent().lock().unwrap().len(),
    }
}

/// Update the auto-sandbox configuration.
/// Requires paid; takes effect immediately (the running listener reads CONFIG on each event).
#[tauri::command]
pub async fn set_usb_autosandbox_config(config: Config) -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB auto-sandbox")?;
    *cfg().lock().unwrap() = config;
    Ok(json!({ "ok": true }))
}

/// Return the recent auto-action ring (newest first), up to 50 entries.
/// Read-only — ungated.
#[tauri::command]
pub fn get_usb_autosandbox_recent() -> Vec<AutoActionRecord> {
    recent().lock().unwrap().iter().cloned().collect()
}

/// Count recent quarantine attempts for a specific device key.
pub fn quarantine_action_count_for_device(device_key: &str) -> u32 {
    recent()
        .lock()
        .unwrap()
        .iter()
        .filter(|record| {
            record.device_key == device_key && matches!(record.action, AutoAction::Quarantine)
        })
        .count() as u32
}

/// Clear the recent auto-action ring.
/// Requires paid.
#[tauri::command]
pub async fn clear_usb_autosandbox_recent() -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB auto-sandbox")?;
    recent().lock().unwrap().clear();
    Ok(json!({ "ok": true }))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_mode_always_ignores() {
        assert_eq!(
            decide_auto_action(Mode::Off, true, false, false, false),
            AutoAction::Ignore
        );
        assert_eq!(
            decide_auto_action(Mode::Off, true, true, false, true),
            AutoAction::Ignore
        );
    }

    #[test]
    fn allowlisted_always_ignores() {
        assert_eq!(
            decide_auto_action(Mode::Observe, true, false, true, false),
            AutoAction::Ignore
        );
        assert_eq!(
            decide_auto_action(Mode::Enforce, true, false, true, false),
            AutoAction::Ignore
        );
        assert_eq!(
            decide_auto_action(Mode::Enforce, true, true, true, true),
            AutoAction::Ignore
        );
    }

    #[test]
    fn mass_storage_observe_alerts() {
        assert_eq!(
            decide_auto_action(Mode::Observe, true, false, false, false),
            AutoAction::Alert
        );
    }

    #[test]
    fn mass_storage_enforce_quarantines() {
        assert_eq!(
            decide_auto_action(Mode::Enforce, true, false, false, false),
            AutoAction::Quarantine
        );
    }

    #[test]
    fn hid_without_act_on_hid_ignores() {
        assert_eq!(
            decide_auto_action(Mode::Enforce, false, true, false, false),
            AutoAction::Ignore
        );
        assert_eq!(
            decide_auto_action(Mode::Observe, false, true, false, false),
            AutoAction::Ignore
        );
    }

    #[test]
    fn hid_with_act_on_hid_enforce_quarantines() {
        assert_eq!(
            decide_auto_action(Mode::Enforce, false, true, false, true),
            AutoAction::Quarantine
        );
    }

    #[test]
    fn non_storage_non_hid_ignores() {
        assert_eq!(
            decide_auto_action(Mode::Observe, false, false, false, false),
            AutoAction::Ignore
        );
        assert_eq!(
            decide_auto_action(Mode::Enforce, false, false, false, true),
            AutoAction::Ignore
        );
    }

    #[test]
    fn hid_observe_with_act_on_hid_alerts() {
        assert_eq!(
            decide_auto_action(Mode::Observe, false, true, false, true),
            AutoAction::Alert
        );
    }

    #[test]
    fn composite_hid_and_storage_enforce_quarantines() {
        // A device that is both HID and mass-storage (e.g., some multifunction adapters).
        assert_eq!(
            decide_auto_action(Mode::Enforce, true, true, false, false),
            AutoAction::Quarantine
        );
    }

    #[test]
    fn forensic_readonly_mount_only_targets_unknown_mass_storage_when_enabled() {
        assert_eq!(
            decide_forensic_readonly_action(true, true, false),
            ForensicReadonlyAction::MountReadOnly
        );
        assert_eq!(
            decide_forensic_readonly_action(true, true, true),
            ForensicReadonlyAction::Ignore
        );
        assert_eq!(
            decide_forensic_readonly_action(true, false, false),
            ForensicReadonlyAction::Ignore
        );
        assert_eq!(
            decide_forensic_readonly_action(false, true, false),
            ForensicReadonlyAction::Ignore
        );
    }
}
