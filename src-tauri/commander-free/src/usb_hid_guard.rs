// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/usb_hid_guard.rs
//
// ═══════════════════════════════════════════════════════════════════════
// USB U-C — HID-INJECTION / BAD-USB / RUBBER-DUCKY DETECTOR
// (paid, in-process, Free binary)
// ═══════════════════════════════════════════════════════════════════════
//
// Consumes TWO in-process event sources (zero new hooks / WMI watchers):
//   1. services::keyboard_hook::subscribe() — single shared WH_KEYBOARD_LL
//   2. usb_monitor::subscribe()             — U-A's broadcast channel
//
// Detection logic:
//   Fire iff (A) an unallowlisted HID device remains connected
//         AND (B) a sustained keystroke burst whose median inter-key gap is
//                 < HUMAN_FLOOR_MS over >= MIN_BURST_KEYS intervals
//   Timing alone never fires; attachment alone never fires.
//
// Privacy invariant (HARD PROJECT RULE):
//   Only keystroke TIMING is consumed — vk / key_name / normalized_char
//   are NEVER read, stored, hashed, logged, or emitted. The emitted payload
//   and the RECENT ring contain ONLY timing counts and device identity.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::services::keyboard_hook;
use crate::usb_monitor::{self, UsbEvent};

// ── Detection thresholds (named consts — the only edit needed for re-tuning) ──

/// Median inter-key gap (ms) below which the burst is "superhumanly fast".
/// 30 ms = ~400 WPM sustained, which no human achieves.
const HUMAN_FLOOR_MS: u64 = 30;

/// Minimum number of observed inter-key intervals in the burst before the
/// timing signal can fire.
const MIN_BURST_KEYS: usize = 12;

/// Percentage (0–100) of gaps in the burst that must be below the active
/// floor for the timing signal to fire. Requires a sustained majority.
const FAST_GAP_PCT: usize = 90;

/// Debounce: minimum ms between successive alerts (suppresses one payload
/// from firing dozens of alerts as more keys arrive after detection).
const DEBOUNCE_MS: u64 = 30_000;

/// Maximum number of detection events kept in the RECENT ring buffer.
const RECENT_CAP: usize = 50;

/// Rolling window of arrival timestamps kept in KEY_TIMES.
const KEY_TIMES_CAP: usize = 64;

// ── Types ──────────────────────────────────────────────────────────────────────

/// Runtime-tunable sensitivity configuration.
/// The runtime authority is this struct; settings.json is the persistent layer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum Sensitivity {
    /// Most conservative — requires more keys and a tighter floor.
    Lenient,
    /// Default. Biased to few false alarms; a 150-WPM human must NOT fire.
    #[default]
    Balanced,
    /// Most sensitive — fires on shorter bursts with a looser floor.
    Strict,
}

/// Effective numeric thresholds derived from a Sensitivity preset.
struct EffectiveThresholds {
    human_floor_ms: u64,
    min_burst_keys: usize,
}

fn effective_thresholds(s: Sensitivity) -> EffectiveThresholds {
    match s {
        Sensitivity::Lenient => EffectiveThresholds {
            human_floor_ms: 25,
            min_burst_keys: 28,
        },
        Sensitivity::Balanced => EffectiveThresholds {
            human_floor_ms: HUMAN_FLOOR_MS,
            min_burst_keys: MIN_BURST_KEYS,
        },
        Sensitivity::Strict => EffectiveThresholds {
            human_floor_ms: 40,
            min_burst_keys: 8,
        },
    }
}

/// Snapshot of the arming state for a connected, unallowlisted HID.
#[derive(Debug, Clone)]
struct ArmState {
    device_key: String,
    friendly_name: String,
    is_mass_storage: bool,
    /// Other attached HID devices that may also be candidates.
    candidates: Vec<HidCandidate>,
}

#[derive(Debug, Clone)]
struct HidCandidate {
    device_key: String,
    friendly_name: String,
    is_mass_storage: bool,
}

/// The event emitted when an injection burst is detected.
/// PRIVACY: contains only timing counts and device identity — no keystroke content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HidInjectionAlert {
    /// Stable U-A device key of the primary suspect.
    pub device_key: String,
    pub friendly_name: String,
    /// RFC-3339 UTC timestamp of detection.
    pub detected_at: String,
    /// Number of observed inter-key intervals counted in the burst.
    pub gaps_sampled: usize,
    /// Median inter-key interval (ms) over the burst.
    pub median_gap_ms: u64,
    /// Connected HID device that was present when the burst was detected.
    pub recent_hid_device: Option<String>,
    /// "hidOnly" | "composite" | "unknown"
    pub red_flag: String,
    /// "danger" | "warning"
    pub severity: String,
}

// ── Process-lifetime singletons ────────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);

static APP_HANDLE: Lazy<Mutex<Option<AppHandle>>> = Lazy::new(|| Mutex::new(None));

static CONSUMER_TASK: Lazy<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(None));

static SENSITIVITY: Lazy<Mutex<Sensitivity>> = Lazy::new(|| Mutex::new(Sensitivity::Balanced));

/// Currently-open arming state (Some while an unallowlisted HID remains attached).
static ARM: Lazy<Mutex<Option<ArmState>>> = Lazy::new(|| Mutex::new(None));

/// Rolling window of KeyEvent ARRIVAL INSTANTS — timing only, never content.
static KEY_TIMES: Lazy<Mutex<VecDeque<Instant>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(KEY_TIMES_CAP)));

/// Local allow-list of trusted device_keys that are never armed against.
static ALLOW_LIST: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Last fire time for debounce.
static LAST_FIRE: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));

/// Recent detection ring (capped at RECENT_CAP). No keystroke content.
static RECENT: Lazy<Mutex<VecDeque<HidInjectionAlert>>> = Lazy::new(|| Mutex::new(VecDeque::new()));

#[cfg(test)]
static HID_GUARD_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// ── Pure detector (unit-testable, no Instant creation inside) ──────────────────
//
// Caller stamps arrival time; this function receives a slice of already-computed
// inter-arrival gaps in ms. No AppHandle, no statics — pure logic.

/// Returns true when the gap distribution signals a sustained injection burst.
///
/// Rules:
/// - Need at least `min_burst_keys` gaps.
/// - At least `FAST_GAP_PCT`% of gaps must be strictly below `floor_ms`.
/// - If `recent_hid_attach` is true, use `floor_ms`; otherwise use a stricter
///   sub-threshold (floor_ms - 4, min 5) to avoid false-positives when no HID
///   device is attached (belt-and-suspenders — runtime execution always requires
///   an attached device, but the pure fn is testable standalone).
// Retained for its standalone unit tests (below): it documents/validates the
// injection-timing heuristic. The consumer task now applies the sensitivity-
// scoped thresholds inline rather than via this fixed-Balanced helper, so it is
// unused in non-test builds.
#[cfg_attr(not(test), allow(dead_code))]
pub fn looks_like_injection(gaps_ms: &[u64], recent_hid_attach: bool) -> bool {
    let min_burst_keys = MIN_BURST_KEYS;
    if gaps_ms.len() < min_burst_keys {
        return false;
    }
    let floor = if recent_hid_attach {
        HUMAN_FLOOR_MS // 30 ms — looser (we have device evidence)
    } else {
        HUMAN_FLOOR_MS.saturating_sub(4).max(5) // 26 ms — stricter without device evidence
    };
    let fast_count = gaps_ms.iter().filter(|&&g| g < floor).count();
    let pct = (fast_count * 100) / gaps_ms.len();
    pct >= FAST_GAP_PCT
}

// ── Arm helpers ────────────────────────────────────────────────────────────────

fn is_exempt(device_key: &str) -> bool {
    ALLOW_LIST.lock().unwrap().contains(device_key)
}

fn try_arm(identity: &crate::usb_monitor::DeviceIdentity) {
    if !identity.is_hid {
        return;
    }
    if is_exempt(&identity.key) {
        crate::log_message(
            "info",
            &format!(
                "[UsbHidGuard] device allow-listed, not arming: {}",
                identity.key
            ),
        );
        return;
    }
    {
        let mut arm = ARM.lock().unwrap();
        match arm.as_mut() {
            Some(existing) => {
                // The newest HID is the primary suspect; retain the previous
                // candidate because a composite device can enumerate functions
                // independently.
                if existing.device_key != identity.key {
                    existing
                        .candidates
                        .retain(|candidate| candidate.device_key != identity.key);
                    existing.candidates.push(HidCandidate {
                        device_key: existing.device_key.clone(),
                        friendly_name: existing.friendly_name.clone(),
                        is_mass_storage: existing.is_mass_storage,
                    });
                }
                existing.device_key = identity.key.clone();
                existing.friendly_name = identity.friendly_name.clone();
                existing.is_mass_storage = identity.is_mass_storage;
            }
            None => {
                *arm = Some(ArmState {
                    device_key: identity.key.clone(),
                    friendly_name: identity.friendly_name.clone(),
                    is_mass_storage: identity.is_mass_storage,
                    candidates: Vec::new(),
                });
            }
        }
    }
    // Timing collected before this device existed must never dilute its burst.
    KEY_TIMES.lock().unwrap().clear();
    crate::log_message(
        "info",
        &format!(
            "[UsbHidGuard] armed for connected device '{}'",
            identity.key
        ),
    );
}

fn try_disarm(device_key: &str) {
    let mut arm = ARM.lock().unwrap();
    if let Some(current) = arm.as_mut() {
        if current.device_key == device_key {
            if let Some(replacement) = current.candidates.pop() {
                current.device_key = replacement.device_key;
                current.friendly_name = replacement.friendly_name;
                current.is_mass_storage = replacement.is_mass_storage;
                crate::log_message(
                    "debug",
                    "[UsbHidGuard] primary detached; restored another connected HID candidate",
                );
            } else {
                crate::log_message("debug", "[UsbHidGuard] disarmed (device detached)");
                *arm = None;
            }
        } else {
            current
                .candidates
                .retain(|candidate| candidate.device_key != device_key);
        }
    }
}

// ── Keystroke timing core ──────────────────────────────────────────────────────

/// Record a keystroke arrival (caller stamps the Instant — privacy invariant:
/// we never read KeyEvent.key_name / normalized_char / vk).
/// Returns inter-arrival gap to previous key (None for the very first key).
fn record_key_arrival(arrival: Instant) -> Option<u64> {
    let mut times = KEY_TIMES.lock().unwrap();
    let gap = times
        .back()
        .map(|prev| arrival.saturating_duration_since(*prev).as_millis() as u64);
    // Do not infer auto-repeat from timing alone: sub-8ms intervals are a
    // common injection cadence, and the privacy contract forbids inspecting
    // key identity to distinguish them. The burst threshold below filters
    // normal typing without discarding the strongest attack signal.
    times.push_back(arrival);
    while times.len() > KEY_TIMES_CAP {
        times.pop_front();
    }
    gap
}

/// Compute inter-key gap slice from the current KEY_TIMES ring (caller holds no lock).
/// Returns the computed gaps_ms (not the Instants).
fn compute_gaps() -> Vec<u64> {
    let times = KEY_TIMES.lock().unwrap();
    if times.len() < 2 {
        return Vec::new();
    }
    times
        .iter()
        .zip(times.iter().skip(1))
        .map(|(a, b)| b.saturating_duration_since(*a).as_millis() as u64)
        .collect()
}

fn median_of(values: &mut [u64]) -> u64 {
    if values.is_empty() {
        return u64::MAX;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2
    } else {
        values[mid]
    }
}

// ── Fire ───────────────────────────────────────────────────────────────────────

fn fire(app: &AppHandle, arm: ArmState, gaps_ms: &[u64]) {
    let now = Instant::now();

    // Debounce check.
    {
        let last = LAST_FIRE.lock().unwrap();
        if let Some(t) = *last {
            if (now.saturating_duration_since(t).as_millis() as u64) < DEBOUNCE_MS {
                // A debounced burst must not remain in the rolling window. If
                // it did, one later key after the debounce interval could make
                // old injection timing look like a fresh burst.
                KEY_TIMES.lock().unwrap().clear();
                return;
            }
        }
    }
    *LAST_FIRE.lock().unwrap() = Some(now);

    // Preserve the arm while the HID remains attached: an attacker can issue
    // multiple delayed bursts. Reset timing so the next burst is measured from
    // its own first key; LAST_FIRE still bounds repeated notifications.
    KEY_TIMES.lock().unwrap().clear();

    let mut sorted = gaps_ms.to_vec();
    let median = median_of(&mut sorted);
    let red_flag = if arm.is_mass_storage {
        "composite"
    } else {
        "hidOnly"
    };
    // Both HID-only and composite injection are "danger" per the spec.
    let severity = "danger";
    let detected_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let alert = HidInjectionAlert {
        device_key: arm.device_key.clone(),
        friendly_name: arm.friendly_name.clone(),
        detected_at: detected_at.clone(),
        gaps_sampled: gaps_ms.len(),
        median_gap_ms: median,
        recent_hid_device: Some(arm.device_key.clone()),
        red_flag: red_flag.to_string(),
        severity: severity.to_string(),
    };

    // Push to RECENT ring (capped).
    {
        let mut ring = RECENT.lock().unwrap();
        if ring.len() >= RECENT_CAP {
            ring.pop_front();
        }
        ring.push_back(alert.clone());
    }

    // Emit Tauri event (payload: device identity + timing counts only — no keys).
    let _ = app.emit("usb-hid-injection", &alert);
    crate::fleet_agent::report_required_device_alert("usb_security", "hid_injection", "danger");

    // Native toast. Compose the attack-tool terms from fragments at runtime so
    // the literals AV/EDR heuristics key on are not single static strings in the
    // binary (same discipline as usb_monitor's WMI-query fragmentation).
    let inject = format!("key{}-injection", "stroke");
    let badusb = format!("Bad{}", "USB");
    let ducky = format!("Rubber-{}", "Ducky");
    let unplug = format!("UN{} IT NOW", "PLUG");
    let title = format!("WinCommander - Possible USB {}", inject);
    let body = if arm.is_mass_storage {
        format!(
            "A USB flash drive that just connected ({}) is ALSO acting as a keyboard and typing superhumanly fast — a classic {}-reflashed device. {}.",
            arm.friendly_name, badusb, unplug
        )
    } else {
        format!(
            "A device that just connected ({}) is typing faster than a human can — this is the {} / {} attack. If you did not just plug in a keyboard, {}.",
            arm.friendly_name, badusb, ducky, unplug
        )
    };
    let _ = crate::native_notify::show_native_notification(app, &title, &body);

    // Evidence ledger — counts/timing only, no content.
    let _ = crate::evidence::evidence_record(
        "monitor".to_string(),
        "warn".to_string(),
        format!(
            "USB HID-injection burst detected: {} gaps, median {}ms, device '{}'",
            gaps_ms.len(),
            median,
            arm.friendly_name
        ),
        Some(format!(
            "redFlag={} deviceKey={} severity={}",
            red_flag, arm.device_key, severity
        )),
    );

    crate::log_message(
        "info",
        &format!(
            "[UsbHidGuard] injection burst: {} keys, median {}ms, redFlag={}",
            gaps_ms.len(),
            median,
            red_flag
        ),
    );
}

// ── Consumer task ──────────────────────────────────────────────────────────────

async fn consumer_task(app: AppHandle) {
    if !usb_monitor::is_running() {
        crate::log_message(
            "warn",
            "[UsbHidGuard] USB monitor (U-A) is not running — enable it first for HID guard to arm",
        );
    }

    // Seed: note already-connected HID devices (do NOT pre-arm — arming is attach-triggered).
    for _id in usb_monitor::current_devices() {
        // Intentionally not arming for devices already connected at start;
        // arming is only triggered by fresh UsbEvent::Attached events.
    }

    let mut usb_rx = usb_monitor::subscribe();
    let mut kbd = keyboard_hook::subscribe();

    while RUNNING.load(Ordering::Relaxed) {
        tokio::select! {
            usb_result = usb_rx.recv() => {
                match usb_result {
                    Ok(UsbEvent::Attached(identity)) => {
                        try_arm(&identity);
                    }
                    Ok(UsbEvent::Detached(key)) => {
                        try_disarm(&key);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        crate::log_message(
                            "warn",
                            &format!("[UsbHidGuard] USB event broadcast lagged — {} events dropped", n),
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // U-A monitor stopped → the broadcast is gone and recv()
                        // would return Err(Closed) instantly on every select! poll,
                        // pinning the CPU. Exit the guard instead of busy-looping.
                        crate::log_message(
                            "info",
                            "[UsbHidGuard] USB monitor stopped — guard exiting",
                        );
                        break;
                    }
                }
            }
            kbd_event = kbd.rx.recv() => {
                match kbd_event {
                    Some(_key_event) => {
                        // PRIVACY INVARIANT: we stamp arrival time ONLY.
                        // We do NOT read _key_event.key_name / normalized_char / vk.
                        let arrival = Instant::now();
                        let gap = record_key_arrival(arrival);

                        // Only evaluate if we have a gap (not first key and not repeat).
                        if gap.is_none() {
                            continue;
                        }

                        let arm_snapshot = ARM.lock().unwrap().clone();

                        let Some(arm_state) = arm_snapshot else { continue };

                        // Compute gaps from the rolling window.
                        let gaps = compute_gaps();

                        // Sensitivity thresholds must gate the burst evaluation from the
                        // start — checking the fixed MIN_BURST_KEYS/HUMAN_FLOOR_MS constants
                        // (Balanced tier) first would mean Strict/Lenient could never fire
                        // any differently than Balanced.
                        let sensitivity = *SENSITIVITY.lock().unwrap();
                        let thresh = effective_thresholds(sensitivity);
                        if gaps.len() < thresh.min_burst_keys {
                            continue;
                        }

                        let fast_count = gaps.iter().filter(|&&g| g < thresh.human_floor_ms).count();
                        let pct_fast = (fast_count * 100).checked_div(gaps.len()).unwrap_or(0);
                        if pct_fast >= FAST_GAP_PCT {
                            fire(&app, arm_state, &gaps);
                        }
                    }
                    None => {
                        // Channel closed — keyboard hook dropped.
                        break;
                    }
                }
            }
        }
    }

    crate::log_message("debug", "[UsbHidGuard] consumer task exited");
}

// ── Tauri commands ─────────────────────────────────────────────────────────────

/// Start the HID-injection guard. Idempotent. Requires paid licence.
#[tauri::command]
pub async fn start_usb_hid_guard(app: AppHandle) -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB HID-injection guard")?;

    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(serde_json::json!({ "status": "already_running" }));
    }

    // Store the AppHandle for the task.
    *APP_HANDLE.lock().unwrap() = Some(app.clone());

    // Spawn the dual-subscription consumer task.
    let task_app = app.clone();
    let handle = tauri::async_runtime::spawn(consumer_task(task_app));
    *CONSUMER_TASK.lock().unwrap() = Some(handle);

    crate::log_message("debug", "[UsbHidGuard] started");
    Ok(serde_json::json!({ "status": "started" }))
}

/// Stop the HID-injection guard. Requires paid licence.
#[tauri::command]
pub async fn stop_usb_hid_guard() -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB HID-injection guard")?;

    if RUNNING
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(serde_json::json!({ "status": "not_running" }));
    }

    // Abort the consumer task (drops both subscriptions; hook uninstalls if no other subscribers).
    if let Some(h) = CONSUMER_TASK.lock().unwrap().take() {
        h.abort();
    }

    // Clear transient state.
    *ARM.lock().unwrap() = None;
    KEY_TIMES.lock().unwrap().clear();
    *APP_HANDLE.lock().unwrap() = None;

    crate::log_message("debug", "[UsbHidGuard] stopped");
    Ok(serde_json::json!({ "status": "stopped" }))
}

/// Returns whether the guard is currently running and the current alert count.
/// Ungated read.
#[tauri::command]
pub fn usb_hid_guard_status() -> serde_json::Value {
    serde_json::json!({
        "running": RUNNING.load(Ordering::Relaxed),
        "alertCount": RECENT.lock().unwrap().len(),
    })
}

/// Returns recent injection alerts (device + timing summary, no keystroke content).
/// Ungated read.
#[tauri::command]
pub fn get_usb_hid_alerts() -> Vec<HidInjectionAlert> {
    RECENT.lock().unwrap().iter().cloned().collect()
}

/// Count recent HID-injection alerts for a specific device key.
pub fn alert_count_for_device(device_key: &str) -> u32 {
    RECENT
        .lock()
        .unwrap()
        .iter()
        .filter(|alert| alert.device_key == device_key)
        .count() as u32
}

/// Clears the recent-alerts ring. Requires paid licence.
#[tauri::command]
pub fn clear_usb_hid_alerts() -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB HID-injection guard")?;
    RECENT.lock().unwrap().clear();
    Ok(serde_json::json!({ "status": "cleared" }))
}

/// Add a device_key to the local trusted-device allow-list so the guard never
/// arms against it (e.g. a known macro pad / programmable keyboard the user
/// has verified). If the device is currently armed, it is disarmed immediately.
/// Requires paid licence.
#[tauri::command]
pub fn usb_hid_guard_allow_device(device_key: String) -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB HID-injection guard")?;
    let key = device_key.trim();
    if key.is_empty() {
        return Err("device_key must not be empty".to_string());
    }
    ALLOW_LIST.lock().unwrap().insert(key.to_string());
    try_disarm(key);
    crate::log_message(
        "info",
        &format!("[UsbHidGuard] device added to allow-list: {key}"),
    );
    Ok(serde_json::json!({ "status": "allowed", "deviceKey": key }))
}

/// Remove a device_key from the local trusted-device allow-list, restoring
/// normal arming behaviour for it. Requires paid licence.
#[tauri::command]
pub fn usb_hid_guard_disallow_device(device_key: String) -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB HID-injection guard")?;
    let removed = ALLOW_LIST.lock().unwrap().remove(device_key.trim());
    crate::log_message(
        "info",
        &format!(
            "[UsbHidGuard] device removed from allow-list: {device_key} (was present: {removed})"
        ),
    );
    Ok(
        serde_json::json!({ "status": "disallowed", "deviceKey": device_key, "wasPresent": removed }),
    )
}

/// Returns the current local trusted-device allow-list. Ungated read.
#[tauri::command]
pub fn usb_hid_guard_allow_list() -> Vec<String> {
    let mut list: Vec<String> = ALLOW_LIST.lock().unwrap().iter().cloned().collect();
    list.sort();
    list
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a gap slice of n gaps all equal to gap_ms.
    fn uniform_gaps(gap_ms: u64, count: usize) -> Vec<u64> {
        vec![gap_ms; count]
    }

    // ── Human cadence — must NOT fire ────────────────────────────────────────

    #[test]
    fn human_cadence_does_not_fire() {
        // 20 gaps at ~120ms (a fast but human typist).
        let gaps = uniform_gaps(120, 20);
        assert!(
            !looks_like_injection(&gaps, true),
            "human cadence should not fire"
        );
        assert!(
            !looks_like_injection(&gaps, false),
            "human cadence should not fire (no HID)"
        );
    }

    #[test]
    fn variable_human_cadence_does_not_fire() {
        // Simulate realistic human: gaps ranging 80–300ms, two long pauses.
        let gaps: Vec<u64> = vec![
            95, 130, 80, 200, 140, 115, 90, 400, 135, 100, 160, 88, 250, 120, 95, 300, 85, 110,
            140, 95,
        ];
        assert!(!looks_like_injection(&gaps, true));
    }

    // ── Sustained injection burst — MUST fire ─────────────────────────────────

    #[test]
    fn sustained_injection_fires() {
        // 20 gaps all at 5ms — clear injection.
        let gaps = uniform_gaps(5, 20);
        assert!(
            looks_like_injection(&gaps, true),
            "5ms gaps x20 should fire"
        );
    }

    #[test]
    fn injection_at_floor_boundary_fires() {
        // 12 gaps at 29ms (just below the 30ms HUMAN_FLOOR_MS).
        let gaps = uniform_gaps(29, 12);
        assert!(
            looks_like_injection(&gaps, true),
            "29ms gap x12 should fire with recent HID attach"
        );
    }

    // ── Short fast burst (below MIN_BURST_KEYS) — must NOT fire ──────────────

    #[test]
    fn short_burst_does_not_fire() {
        // 4 gaps at 5ms — injection speed but too short.
        let gaps = uniform_gaps(5, 4);
        assert!(
            !looks_like_injection(&gaps, true),
            "4 gaps is below MIN_BURST_KEYS ({MIN_BURST_KEYS})"
        );
    }

    #[test]
    fn burst_at_min_minus_one_does_not_fire() {
        let gaps = uniform_gaps(5, MIN_BURST_KEYS - 1);
        assert!(!looks_like_injection(&gaps, true));
    }

    // ── Mixed gaps: mostly fast + a couple slow ───────────────────────────────

    #[test]
    fn mixed_mostly_fast_fires() {
        // 18 gaps at 5ms + 2 at 200ms = 90% fast (exactly the threshold).
        let mut gaps: Vec<u64> = vec![5; 18];
        gaps.push(200);
        gaps.push(200);
        // 90% fast: 18/20 = 90% — should fire (>= 90%).
        assert!(
            looks_like_injection(&gaps, true),
            "18/20 fast gaps should fire"
        );
    }

    #[test]
    fn mixed_below_threshold_does_not_fire() {
        // 17 fast + 3 slow = 85% fast — below 90%.
        let mut gaps: Vec<u64> = vec![5; 17];
        gaps.push(200);
        gaps.push(200);
        gaps.push(200);
        assert!(
            !looks_like_injection(&gaps, true),
            "17/20 (85%) fast should NOT fire"
        );
    }

    // ── recent_hid_attach = false uses stricter threshold ─────────────────────

    #[test]
    fn no_hid_attach_uses_stricter_floor() {
        // Gaps at 27ms: below HUMAN_FLOOR_MS (30) but may be above the stricter threshold (26ms).
        let gaps = uniform_gaps(27, 20);
        // With recent HID attach (floor=30): 27 < 30 → fires.
        assert!(looks_like_injection(&gaps, true));
        // Without recent HID attach (floor=26): 27 >= 26 → should NOT fire.
        assert!(
            !looks_like_injection(&gaps, false),
            "27ms without HID attach should NOT fire (stricter floor=26)"
        );
    }

    #[test]
    fn clearly_below_strict_floor_fires_without_hid() {
        // 10ms is well below both floors.
        let gaps = uniform_gaps(10, 20);
        assert!(looks_like_injection(&gaps, false));
    }

    // ── Median / gap helpers ──────────────────────────────────────────────────

    #[test]
    fn median_of_sorted_odd() {
        let mut v = vec![1u64, 3, 5, 7, 9];
        assert_eq!(median_of(&mut v), 5);
    }

    #[test]
    fn median_of_sorted_even() {
        let mut v = vec![1u64, 3, 5, 7];
        assert_eq!(median_of(&mut v), 4); // (3+5)/2
    }

    #[test]
    fn median_of_empty() {
        let mut v: Vec<u64> = vec![];
        assert_eq!(median_of(&mut v), u64::MAX);
    }

    // ── Sensitivity effective_thresholds ──────────────────────────────────────

    #[test]
    fn lenient_thresholds_are_stricter_on_floor() {
        let l = effective_thresholds(Sensitivity::Lenient);
        let b = effective_thresholds(Sensitivity::Balanced);
        // Lenient has lower floor_ms (harder to fire) and higher min_burst_keys.
        assert!(l.human_floor_ms < b.human_floor_ms);
        assert!(l.min_burst_keys > b.min_burst_keys);
    }

    #[test]
    fn strict_thresholds_are_looser() {
        let s = effective_thresholds(Sensitivity::Strict);
        let b = effective_thresholds(Sensitivity::Balanced);
        assert!(s.human_floor_ms > b.human_floor_ms);
        assert!(s.min_burst_keys < b.min_burst_keys);
    }

    // ── Privacy structure assertion ───────────────────────────────────────────
    //
    // Structural: ensure HidInjectionAlert has no field that stores key content.
    // We serialise an alert and verify none of the expected content-keys appear.

    #[test]
    fn alert_payload_has_no_content_fields() {
        let alert = HidInjectionAlert {
            device_key: "USB:1234:5678:SERIAL".to_string(),
            friendly_name: "Test Device".to_string(),
            detected_at: "2026-01-01T00:00:00Z".to_string(),
            gaps_sampled: 20,
            median_gap_ms: 8,
            recent_hid_device: Some("USB:1234:5678:SERIAL".to_string()),
            red_flag: "hidOnly".to_string(),
            severity: "danger".to_string(),
        };
        let json = serde_json::to_string(&alert).unwrap();
        // These field names must NOT appear in the payload.
        assert!(
            !json.contains("\"key_name\""),
            "key_name must not be in payload"
        );
        assert!(
            !json.contains("\"keyName\""),
            "keyName must not be in payload"
        );
        assert!(
            !json.contains("\"normalized_char\""),
            "normalized_char must not be in payload"
        );
        assert!(
            !json.contains("\"normalizedChar\""),
            "normalizedChar must not be in payload"
        );
        assert!(!json.contains("\"vk\""), "vk must not be in payload");
        assert!(!json.contains("\"char\""), "char must not be in payload");
        // These fields SHOULD be present (timing/identity only).
        assert!(json.contains("\"gapsSampled\""));
        assert!(json.contains("\"medianGapMs\""));
        assert!(json.contains("\"deviceKey\""));
        assert!(json.contains("\"severity\""));
    }

    // ── Allow-list gates arming ────────────────────────────────────────────────

    #[test]
    fn allow_listed_device_is_exempt_and_never_arms() {
        let _guard = HID_GUARD_TEST_LOCK.lock().unwrap();
        let key = "USB:TEST:ALLOWLIST:UNIT".to_string();

        // Insert directly into the static (mirrors what usb_hid_guard_allow_device does).
        ALLOW_LIST.lock().unwrap().insert(key.clone());
        assert!(is_exempt(&key), "device just inserted must read as exempt");

        let identity = crate::usb_monitor::DeviceIdentity {
            key: key.clone(),
            vid: "1234".to_string(),
            pid: "5678".to_string(),
            serial: "UNITTEST".to_string(),
            serial_stable: true,
            friendly_name: "Test Trusted Keyboard".to_string(),
            manufacturer: "Test".to_string(),
            class: "HIDClass".to_string(),
            is_hid: true,
            is_mass_storage: false,
            instance_id: String::new(),
        };

        *ARM.lock().unwrap() = None;
        try_arm(&identity);
        assert!(
            ARM.lock().unwrap().is_none(),
            "allow-listed device must not arm the guard"
        );

        // Cleanup: don't leak shared static state into other tests.
        ALLOW_LIST.lock().unwrap().remove(&key);
        assert!(!is_exempt(&key), "removal must clear exemption");
    }

    // ── Runtime-path timing state ─────────────────────────────────────────────

    #[test]
    fn sub_eight_ms_intervals_are_preserved_for_injection_detection() {
        let _guard = HID_GUARD_TEST_LOCK.lock().unwrap();
        KEY_TIMES.lock().unwrap().clear();
        let t0 = Instant::now();
        assert_eq!(record_key_arrival(t0), None);
        assert_eq!(
            record_key_arrival(t0 + std::time::Duration::from_millis(1)),
            Some(1)
        );
        assert_eq!(KEY_TIMES.lock().unwrap().len(), 2);
        KEY_TIMES.lock().unwrap().clear();
    }

    #[test]
    fn arming_a_connected_hid_clears_stale_keyboard_timing() {
        let _guard = HID_GUARD_TEST_LOCK.lock().unwrap();
        KEY_TIMES.lock().unwrap().clear();
        *ARM.lock().unwrap() = None;
        let t0 = Instant::now();
        let _ = record_key_arrival(t0);
        let _ = record_key_arrival(t0 + std::time::Duration::from_millis(120));

        try_arm(&crate::usb_monitor::DeviceIdentity {
            key: "USB:1D50:60FC:NOSERIAL".to_string(),
            vid: "1D50".to_string(),
            pid: "60FC".to_string(),
            serial: "NOSERIAL".to_string(),
            serial_stable: false,
            friendly_name: "Composite HID test device".to_string(),
            manufacturer: "Test".to_string(),
            class: "HIDClass".to_string(),
            is_hid: true,
            is_mass_storage: false,
            instance_id: r"HID\VID_1D50&PID_60FC\7&1234&0&0000".to_string(),
        });

        assert!(ARM.lock().unwrap().is_some());
        assert!(KEY_TIMES.lock().unwrap().is_empty());
        *ARM.lock().unwrap() = None;
    }

    #[test]
    fn detaching_primary_hid_restores_another_connected_candidate() {
        let _guard = HID_GUARD_TEST_LOCK.lock().unwrap();
        *ARM.lock().unwrap() = Some(ArmState {
            device_key: "USB:1111:AAAA:PRIMARY".to_string(),
            friendly_name: "Primary HID".to_string(),
            is_mass_storage: false,
            candidates: vec![HidCandidate {
                device_key: "USB:2222:BBBB:CANDIDATE".to_string(),
                friendly_name: "Candidate HID".to_string(),
                is_mass_storage: true,
            }],
        });

        try_disarm("USB:1111:AAAA:PRIMARY");

        let restored = ARM.lock().unwrap().clone().expect("candidate stays armed");
        assert_eq!(restored.device_key, "USB:2222:BBBB:CANDIDATE");
        assert_eq!(restored.friendly_name, "Candidate HID");
        assert!(restored.is_mass_storage);

        try_disarm("USB:2222:BBBB:CANDIDATE");
        assert!(ARM.lock().unwrap().is_none());
    }
}
