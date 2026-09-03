// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/dead_mans_switch.rs
//
// ═══════════════════════════════════════════════════════════════════════
// DEAD MAN'S SWITCH — "if I disappear, lock down the device"
// ═══════════════════════════════════════════════════════════════════════
//
// A-4 module 4 evaluation: this file stays in Free.
//
// One-line spec: if WinCommander doesn't see the operator for N days,
// fire the Lockdown / self-destruct cascade (same path as Destroy PIN).
// Rationale: if the operator disappears (arrest, seizure, …), the
// device should destroy/lock down, not run an export flow.
//
// Architecture:
//
//   State (persisted in settings.app.deadMansSwitch):
//     - enabled              : bool
//     - thresholdDays        : u32  (default 14, clamped [1, 365])
//     - flowIdToFire         : Option<String>  (deprecated — no longer
//                              used by the trip path; kept for settings
//                              back-compat so existing JSON doesn't fail
//                              to deserialize)
//     - lastActivityAt       : ISO-8601 (reset on tap-out)
//     - lastFiredAt          : Option<ISO-8601> (set when the switch
//                              fires so we don't re-fire every hour
//                              afterwards)
//
//   Watchdog: background task started by `init(app)` ticks once an hour.
//   Checks: if enabled AND lastFiredAt is None AND
//   now - lastActivityAt > thresholdDays, invoke lockdown + record
//   lastFiredAt.
//
//   Tap-out:
//     - WinCommander startup → auto-reset (just being alive is enough)
//     - Manual "I'm alive" button in Privacy panel
//     - Could later wire to: panel switches, settings saves, etc.
//
//   Reset after fire: not automatic. Once the switch fires, the operator
//   must explicitly clear `lastFiredAt` via the UI before the timer
//   resumes — failing-loud beats silently re-arming an irreversible
//   action that already ran.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;
use tauri::{AppHandle, Emitter};

use crate::settings::{read_settings, write_settings};

const DEFAULT_THRESHOLD_DAYS: u32 = 14;
const MIN_THRESHOLD_DAYS: u32 = 1;
const MAX_THRESHOLD_DAYS: u32 = 365;
/// How often the watchdog wakes up. Hourly is plenty — granularity
/// finer than this gives no operator-visible benefit for a multi-day
/// timer, and we don't want to spam the engine on a 14-day threshold.
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadMansSwitchConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_threshold_days")]
    pub threshold_days: u32,
    #[serde(default)]
    pub flow_id_to_fire: Option<String>,
    /// ISO-8601 UTC. Empty on first install — `init()` stamps it.
    #[serde(default)]
    pub last_activity_at: String,
    /// ISO-8601 UTC of when the switch last fired, or None if it
    /// hasn't. UI surfaces a "switch tripped" banner until cleared.
    #[serde(default)]
    pub last_fired_at: Option<String>,
}

impl Default for DeadMansSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_days: DEFAULT_THRESHOLD_DAYS,
            flow_id_to_fire: None,
            last_activity_at: String::new(),
            last_fired_at: None,
        }
    }
}

fn default_threshold_days() -> u32 {
    DEFAULT_THRESHOLD_DAYS
}

// ── Watchdog lifecycle ─────────────────────────────────────────────────

static WATCHDOG_STARTED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

/// Bootstrap. Called from `lib.rs` on app startup. Idempotent.
pub fn init(app: &AppHandle) {
    // Mark startup as a tap-out: the operator booted WinCommander, so
    // we know they're alive. Failures (no settings file yet, etc.) are
    // logged but don't block startup.
    if let Err(e) = reset_timer_internal() {
        crate::log_message(
            "warn",
            &format!("[DeadMansSwitch] startup reset failed: {}", e),
        );
    }

    // Spawn the watchdog exactly once.
    {
        let mut started = WATCHDOG_STARTED.lock().unwrap();
        if *started {
            return;
        }
        *started = true;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // First check is delayed by half the interval so we don't race
        // with `init`'s reset.
        tokio::time::sleep(WATCHDOG_INTERVAL / 2).await;
        loop {
            if let Err(e) = tick(&app).await {
                crate::log_message(
                    "warn",
                    &format!("[DeadMansSwitch] watchdog tick failed: {}", e),
                );
            }
            tokio::time::sleep(WATCHDOG_INTERVAL).await;
        }
    });
}

/// One watchdog iteration. Returns Err only for IO/settings issues;
/// "switch fired" is a normal Ok path with a side-effect.
async fn tick(app: &AppHandle) -> Result<(), String> {
    let cfg = read_config()?;
    if !cfg.enabled {
        return Ok(());
    }
    // Already fired — don't re-fire. Operator must explicitly clear.
    if cfg.last_fired_at.is_some() {
        return Ok(());
    }
    let last = match parse_iso(&cfg.last_activity_at) {
        Some(t) => t,
        None => return Ok(()), // Never stamped; init() should have. No-op.
    };
    let now = chrono::Utc::now();
    let elapsed = now.signed_duration_since(last);
    let threshold = chrono::Duration::days(cfg.threshold_days as i64);
    if elapsed < threshold {
        return Ok(());
    }

    // ── Switch tripped ─────────────────────────────────────────────────
    crate::log_message(
        "warn",
        &format!(
            "[DeadMansSwitch] tripped: {} since last activity (threshold {} days)",
            elapsed.num_hours().to_string() + "h",
            cfg.threshold_days
        ),
    );

    // Record the fire BEFORE invoking the lockdown. If the cascade panics
    // or the app dies mid-fire, we'd rather skip re-firing on next boot
    // than re-trigger irreversible actions.
    let fire_time = now.to_rfc3339();
    write_last_fired(&fire_time)?;

    // Fire the lockdown / self-destruct cascade (same path as Destroy PIN).
    // deactivate_license_first=false, shutdown_system=false — matches the
    // CalculatorGate destroy convention; the cascade itself handles further
    // steps per the user's selfDestruct settings.
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = crate::backend::lockdown_impl(app_clone, false, false, true).await;
    });

    let _ = app.emit(
        "dead-mans-switch-fired",
        serde_json::json!({ "firedAt": fire_time }),
    );
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────

fn parse_iso(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

fn read_config() -> Result<DeadMansSwitchConfig, String> {
    let settings = read_settings()?;
    Ok(settings.app.dead_mans_switch.unwrap_or_default())
}

fn write_config(cfg: &DeadMansSwitchConfig) -> Result<(), String> {
    let mut settings = read_settings()?;
    settings.app.dead_mans_switch = Some(cfg.clone());
    write_settings(&settings)
}

fn reset_timer_internal() -> Result<(), String> {
    let mut cfg = read_config()?;
    cfg.last_activity_at = chrono::Utc::now().to_rfc3339();
    write_config(&cfg)
}

fn write_last_fired(when: &str) -> Result<(), String> {
    let mut cfg = read_config()?;
    cfg.last_fired_at = Some(when.to_string());
    write_config(&cfg)
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI COMMANDS
// ═══════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn get_dead_mans_switch_config() -> Result<DeadMansSwitchConfig, String> {
    read_config()
}

#[tauri::command]
pub fn set_dead_mans_switch_config(
    mut config: DeadMansSwitchConfig,
) -> Result<DeadMansSwitchConfig, String> {
    crate::license::require_paid("dead man's switch")?;
    // Validate + clamp threshold.
    config.threshold_days = config
        .threshold_days
        .clamp(MIN_THRESHOLD_DAYS, MAX_THRESHOLD_DAYS);
    // Preserve last_activity_at if the caller passed empty (UI doesn't
    // edit this field directly).
    let existing = read_config().unwrap_or_default();
    if config.last_activity_at.is_empty() {
        config.last_activity_at = existing.last_activity_at;
    }
    // Same for last_fired_at — UI clears it via the dedicated command.
    if config.last_fired_at.is_none() && existing.last_fired_at.is_some() {
        config.last_fired_at = existing.last_fired_at;
    }
    // If we're enabling for the first time, stamp now as the activity
    // baseline so the threshold doesn't trigger immediately.
    if config.enabled && config.last_activity_at.is_empty() {
        config.last_activity_at = chrono::Utc::now().to_rfc3339();
    }
    write_config(&config)?;
    Ok(config)
}

/// "I'm alive." Resets the inactivity timer. Frontend wires this to a
/// big button + (optionally) to other interaction events.
#[tauri::command]
pub fn reset_dead_mans_switch_timer() -> Result<DeadMansSwitchConfig, String> {
    reset_timer_internal()?;
    read_config()
}

/// Clears the `lastFiredAt` flag so the watchdog resumes monitoring.
/// Operator-only — never automatic.
#[tauri::command]
pub fn clear_dead_mans_switch_fired() -> Result<DeadMansSwitchConfig, String> {
    let mut cfg = read_config()?;
    cfg.last_fired_at = None;
    // Also reset the activity timer so we don't immediately re-fire
    // because lastActivityAt is still old.
    cfg.last_activity_at = chrono::Utc::now().to_rfc3339();
    write_config(&cfg)?;
    Ok(cfg)
}

// ═══════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso_handles_empty() {
        assert!(parse_iso("").is_none());
    }

    #[test]
    fn parse_iso_roundtrips() {
        let now = chrono::Utc::now();
        let s = now.to_rfc3339();
        let back = parse_iso(&s).expect("parse");
        // Some sub-microsecond truncation is fine.
        let delta = (back - now).num_milliseconds().abs();
        assert!(delta < 1000);
    }

    #[test]
    fn config_defaults_are_safe() {
        let c = DeadMansSwitchConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.threshold_days, DEFAULT_THRESHOLD_DAYS);
        assert!(c.flow_id_to_fire.is_none());
        assert!(c.last_fired_at.is_none());
    }

    /// Sanity: a 14-day default threshold means a brand-new install
    /// can't fire until 14 days from now, ever.
    #[test]
    fn threshold_default_is_safe() {
        let c = DeadMansSwitchConfig::default();
        assert!(c.threshold_days >= 1);
        assert!(c.threshold_days <= MAX_THRESHOLD_DAYS);
    }
}
