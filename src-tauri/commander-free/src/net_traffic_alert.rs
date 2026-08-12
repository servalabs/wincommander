// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/net_traffic_alert.rs
//
// ═══════════════════════════════════════════════════════════════════════
// LIVE METRIC SAMPLER + REUSABLE PER-METRIC ALERTING (paid)
// ═══════════════════════════════════════════════════════════════════════
//
// A single background sampler reads, once per second:
//   • aggregate NIC throughput  → emits `metrics://network`
//     ({ upBytesPerSec, downBytesPerSec }) for the Dashboard's live readout.
//   • global CPU usage          → used for the CPU alert.
//
// It then evaluates a REUSABLE alert per monitored metric. Each metric (CPU %,
// Upload MB/s, Download MB/s) carries an independent `MetricAlert` config —
// enable + threshold + the two suppressors — and they all share ONE evaluator
// and ONE notification path. Adding a new metric later is: sample it here, add
// a field to MetricAlertsConfig/Settings, and call `evaluate`/`fire_alert`.
//
// The two suppressors (each toggleable per metric):
//   • Hysteresis — once fired, stays fired until the value drops below a reset
//     band (threshold × (1 − hysteresisPct/100)). Prevents flapping.
//   • Sustained  — only fire after the value stays above the limit for
//     `sustainedSecs` continuously. Ignores brief spikes.
//
// CONFIG is the runtime authority (seeded from settings at startup, updated by
// the paid setter). The sampler runs regardless of entitlement so the live
// readout works for any paid user who opens the card, but alerts only fire when
// a metric's `enabled` is set — and that can only be turned on through the
// require_paid-gated setter.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sysinfo::{Networks, System};
use tauri::{AppHandle, Emitter};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const BYTES_PER_MB: f64 = 1_000_000.0; // decimal MB/s (matches link-speed convention)

/// One reusable metric alert. Same shape for every monitored metric; the unit
/// of `threshold`/value depends on the metric (CPU = %, up/down = MB/s).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricAlert {
    pub enabled: bool,
    pub threshold: f64,
    pub hysteresis_enabled: bool,
    pub hysteresis_pct: u8,
    pub sustained_enabled: bool,
    pub sustained_secs: u32,
}

impl From<crate::settings::MetricAlertSettings> for MetricAlert {
    fn from(s: crate::settings::MetricAlertSettings) -> Self {
        Self {
            enabled: s.enabled,
            threshold: s.threshold,
            hysteresis_enabled: s.hysteresis_enabled,
            hysteresis_pct: s.hysteresis_pct.clamp(1, 90),
            sustained_enabled: s.sustained_enabled,
            sustained_secs: s.sustained_secs.clamp(1, 600),
        }
    }
}

impl MetricAlert {
    /// Clamp/normalise a config received from the frontend before storing it.
    fn normalized(&self) -> Self {
        Self {
            enabled: self.enabled,
            threshold: self.threshold.max(0.1),
            hysteresis_enabled: self.hysteresis_enabled,
            hysteresis_pct: self.hysteresis_pct.clamp(1, 90),
            sustained_enabled: self.sustained_enabled,
            sustained_secs: self.sustained_secs.clamp(1, 600),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled": self.enabled,
            "threshold": self.threshold,
            "hysteresisEnabled": self.hysteresis_enabled,
            "hysteresisPct": self.hysteresis_pct,
            "sustainedEnabled": self.sustained_enabled,
            "sustainedSecs": self.sustained_secs,
        })
    }
}

/// The full reusable alert set. Add a metric → add a field here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricAlertsConfig {
    pub cpu: MetricAlert,
    pub ram: MetricAlert,
    pub upload: MetricAlert,
    pub download: MetricAlert,
}

impl From<crate::settings::MetricAlertsSettings> for MetricAlertsConfig {
    fn from(s: crate::settings::MetricAlertsSettings) -> Self {
        Self {
            cpu: s.cpu.into(),
            ram: s.ram.into(),
            upload: s.upload.into(),
            download: s.download.into(),
        }
    }
}

impl Default for MetricAlertsConfig {
    fn default() -> Self {
        crate::settings::MetricAlertsSettings::default().into()
    }
}

static CONFIG: Lazy<Mutex<MetricAlertsConfig>> =
    Lazy::new(|| Mutex::new(MetricAlertsConfig::default()));

/// Per-metric evaluator state — not exposed; lives entirely in the sampler.
struct AlertState {
    breach_start: Option<Instant>,
    alerted: bool,
}

impl AlertState {
    fn new() -> Self {
        Self {
            breach_start: None,
            alerted: false,
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct NetSample {
    up_bytes_per_sec: u64,
    down_bytes_per_sec: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MetricAlertPayload {
    /// Metric key: "cpu" | "upload" | "download".
    metric: String,
    /// Human label, e.g. "CPU usage".
    label: String,
    /// Value at fire time, in `unit`.
    value: f64,
    /// Display unit, e.g. "%" or "MB/s".
    unit: String,
    threshold: f64,
}

/// Evaluate one metric alert against `value`. Returns true exactly on the
/// transition into the alerted state (the moment to fire). Shared by every
/// metric — this is the reusable core.
fn evaluate(cfg: &MetricAlert, state: &mut AlertState, value: f64, now: Instant) -> bool {
    let over = value > cfg.threshold;

    if over {
        if state.breach_start.is_none() {
            state.breach_start = Some(now);
        }
    } else {
        state.breach_start = None;
    }

    if !state.alerted {
        let should_fire = if cfg.sustained_enabled {
            state
                .breach_start
                .map(|t| now.duration_since(t).as_secs() >= cfg.sustained_secs as u64)
                .unwrap_or(false)
        } else {
            over
        };
        if should_fire {
            state.alerted = true;
            return true;
        }
        false
    } else {
        let reset_level = if cfg.hysteresis_enabled {
            cfg.threshold * (1.0 - cfg.hysteresis_pct as f64 / 100.0)
        } else {
            cfg.threshold
        };
        if value < reset_level {
            state.alerted = false;
            state.breach_start = None;
        }
        false
    }
}

fn fire_alert(app: &AppHandle, metric: &str, label: &str, value: f64, unit: &str, threshold: f64) {
    let body = format!(
        "{} hit {:.1}{} (limit {:.0}{}).",
        label, value, unit, threshold, unit
    );
    if let Err(e) =
        crate::native_notify::show_native_notification(app, "WinCommander - Alert", &body)
    {
        crate::log_message("warn", &format!("[MetricAlert] notification failed: {}", e));
    }
    let _ = app.emit(
        "metrics://metric-alert",
        MetricAlertPayload {
            metric: metric.to_string(),
            label: label.to_string(),
            value,
            unit: unit.to_string(),
            threshold,
        },
    );
    crate::log_message("warn", &format!("[MetricAlert] {}", body));
}

/// One enabled metric: evaluate + fire, or reset state while disabled.
///
/// `entitled` is re-checked by the caller every tick (not just at
/// config-write time) so a lapsed licence stops alerts from firing even
/// though the last-persisted config still has `enabled: true` — the
/// config-write gate (`metric_alerts_set_config`) only proves entitlement at
/// the moment the toggle was flipped, not for as long as it stays on.
#[allow(clippy::too_many_arguments)]
fn tick_metric(
    app: &AppHandle,
    cfg: &MetricAlert,
    state: &mut AlertState,
    metric: &str,
    label: &str,
    unit: &str,
    value: f64,
    now: Instant,
    entitled: bool,
) {
    if cfg.enabled && entitled {
        if evaluate(cfg, state, value, now) {
            fire_alert(app, metric, label, value, unit, cfg.threshold);
        }
    } else {
        // Keep state clean while disabled (or unentitled) so re-enabling
        // starts fresh instead of firing immediately on stale breach state.
        *state = AlertState::new();
    }
}

/// Start the 1s sampler. Call once from `run()`'s setup hook.
pub fn init(app: &AppHandle) {
    // Seed runtime config from persisted settings.
    if let Ok(s) = crate::settings::read_settings() {
        *CONFIG.lock().unwrap() = s.app.metric_alerts.into();
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut networks = Networks::new_with_refreshed_list();
        let mut sys = System::new();
        sys.refresh_cpu_all();

        let mut st_cpu = AlertState::new();
        let mut st_ram = AlertState::new();
        let mut st_up = AlertState::new();
        let mut st_down = AlertState::new();

        // Discard the first interval — NIC deltas are cumulative-since-boot on
        // the first reading, and CPU usage needs two refreshes to be meaningful.
        tokio::time::sleep(SAMPLE_INTERVAL).await;
        loop {
            networks.refresh(true);
            sys.refresh_cpu_all();
            sys.refresh_memory();

            let mut down: u64 = 0;
            let mut up: u64 = 0;
            for data in networks.values() {
                down += data.received();
                up += data.transmitted();
            }
            let down_per_sec = (down as f64 / SAMPLE_INTERVAL.as_secs_f64()) as u64;
            let up_per_sec = (up as f64 / SAMPLE_INTERVAL.as_secs_f64()) as u64;

            let _ = app.emit(
                "metrics://network",
                NetSample {
                    up_bytes_per_sec: up_per_sec,
                    down_bytes_per_sec: down_per_sec,
                },
            );

            let cfg = CONFIG.lock().unwrap().clone();
            let now = Instant::now();
            // Re-verified every tick — not just at config-write time — so a
            // lapsed licence silences alerts even if the persisted config
            // still has `enabled: true` from when the user was entitled.
            let entitled = crate::license::has_paid_entitlement();

            let cpu_pct = sys.global_cpu_usage() as f64;
            let ram_pct = if sys.total_memory() > 0 {
                sys.used_memory() as f64 / sys.total_memory() as f64 * 100.0
            } else {
                0.0
            };
            let up_mbps = up_per_sec as f64 / BYTES_PER_MB;
            let down_mbps = down_per_sec as f64 / BYTES_PER_MB;

            tick_metric(
                &app,
                &cfg.cpu,
                &mut st_cpu,
                "cpu",
                "CPU usage",
                "%",
                cpu_pct,
                now,
                entitled,
            );
            tick_metric(
                &app,
                &cfg.ram,
                &mut st_ram,
                "ram",
                "RAM usage",
                "%",
                ram_pct,
                now,
                entitled,
            );
            tick_metric(
                &app,
                &cfg.upload,
                &mut st_up,
                "upload",
                "Upload",
                "MB/s",
                up_mbps,
                now,
                entitled,
            );
            tick_metric(
                &app,
                &cfg.download,
                &mut st_down,
                "download",
                "Download",
                "MB/s",
                down_mbps,
                now,
                entitled,
            );

            tokio::time::sleep(SAMPLE_INTERVAL).await;
        }
    });
}

/// Update the metric-alert config (paid). Persists to settings and hot-reloads
/// the running sampler's CONFIG so changes take effect on the next tick.
#[tauri::command]
pub async fn metric_alerts_set_config(
    config: MetricAlertsConfig,
) -> Result<MetricAlertsConfig, String> {
    crate::license::require_paid("metric alerting")?;

    let normalized = MetricAlertsConfig {
        cpu: config.cpu.normalized(),
        ram: config.ram.normalized(),
        upload: config.upload.normalized(),
        download: config.download.normalized(),
    };

    *CONFIG.lock().unwrap() = normalized.clone();

    // Persist via the settings patch path so the value survives restart.
    let patch = serde_json::json!({
        "app": {
            "metricAlerts": {
                "cpu": normalized.cpu.to_json(),
                "ram": normalized.ram.to_json(),
                "upload": normalized.upload.to_json(),
                "download": normalized.download.to_json(),
            }
        }
    });
    crate::settings::patch_settings(patch).map_err(|e| format!("persist failed: {}", e))?;

    Ok(normalized)
}

/// Read the current metric-alert config. Free — reading the config doesn't
/// expose any paid capability and the cards need it to render their controls.
#[tauri::command]
pub fn metric_alerts_get_config() -> MetricAlertsConfig {
    CONFIG.lock().unwrap().clone()
}
