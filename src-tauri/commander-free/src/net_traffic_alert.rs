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
    pub cooldown_secs: u32,
    /// Forward this alert to the Fleet console when it fires. Admin-lockable
    /// via `ConfigEpoch.locked_paths` on `notifications.{cpuUsage,ramUsage,
    /// networkUsage}.reportToFleet` — same mechanism `privacy.privacyShield`
    /// already uses.
    pub report_to_fleet: bool,
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
            cooldown_secs: s.cooldown_secs.min(86_400),
            report_to_fleet: s.report_to_fleet,
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
            cooldown_secs: self.cooldown_secs.min(86_400),
            report_to_fleet: self.report_to_fleet,
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
            "cooldownSecs": self.cooldown_secs,
            "reportToFleet": self.report_to_fleet,
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
    /// The sampler retains safe one-second source samples, but evaluates the
    /// policy only at this cadence. A signed Fleet policy may use 1..=300s.
    #[serde(default = "default_evaluation_interval_secs")]
    pub evaluation_interval_secs: u32,
}

fn default_evaluation_interval_secs() -> u32 {
    1
}

impl From<crate::settings::MetricAlertsSettings> for MetricAlertsConfig {
    fn from(s: crate::settings::MetricAlertsSettings) -> Self {
        Self {
            cpu: s.cpu.into(),
            ram: s.ram.into(),
            upload: s.upload.into(),
            download: s.download.into(),
            evaluation_interval_secs: default_evaluation_interval_secs(),
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

const MASTER_FLEET_ALERT_PATH: &str = "security.requireAllDeviceAlertsInFleet";

/// Resolve the user-facing configuration from persisted settings. A signed
/// Fleet master policy is an effective value, not a second notification
/// pipeline: it simply turns on the existing per-metric reports.
fn config_from_settings(settings: &crate::settings::AppSettings) -> MetricAlertsConfig {
    let mut config: MetricAlertsConfig = settings.app.metric_alerts.clone().into();
    // CPU and RAM have separate limits but one Fleet reporting policy.
    // A partially enabled legacy setting must not silently report either metric.
    let system_reports_to_fleet = config.cpu.report_to_fleet && config.ram.report_to_fleet;
    config.cpu.report_to_fleet = system_reports_to_fleet;
    config.ram.report_to_fleet = system_reports_to_fleet;
    // Upload and download have separate limits but one Fleet reporting policy.
    // A partially enabled legacy setting must not silently report either direction.
    let network_reports_to_fleet = config.upload.report_to_fleet && config.download.report_to_fleet;
    config.upload.report_to_fleet = network_reports_to_fleet;
    config.download.report_to_fleet = network_reports_to_fleet;
    if settings.ideal.security.require_all_device_alerts_in_fleet {
        config.cpu.report_to_fleet = true;
        config.ram.report_to_fleet = true;
        config.upload.report_to_fleet = true;
        config.download.report_to_fleet = true;
    }
    // A device-scoped signed policy is deliberately typed under
    // `ideal.security`, not an untyped `notifications.*` object that serde
    // drops while applying the epoch. Pair CPU/RAM and upload/download so a
    // Fleet checkbox cannot leave half a category reporting.
    if settings
        .ideal
        .security
        .metric_alert_reporting
        .system_report_to_fleet
        == Some(true)
    {
        config.cpu.report_to_fleet = true;
        config.ram.report_to_fleet = true;
    }
    if settings
        .ideal
        .security
        .metric_alert_reporting
        .network_report_to_fleet
        == Some(true)
    {
        config.upload.report_to_fleet = true;
        config.download.report_to_fleet = true;
    }
    if let Some(policy) = settings
        .ideal
        .security
        .metric_alert_reporting
        .policy
        .as_ref()
    {
        // Fleet policy is one atomic evaluator policy. Clamp again at this
        // boundary so a hand-edited cache can neither silence monitoring nor
        // create a tight loop, even though the server has already validated
        // the signed values.
        config.evaluation_interval_secs = policy.evaluation_interval_secs.clamp(1, 300);
        apply_fleet_metric_policy(&mut config.cpu, policy.cpu_threshold_pct, policy);
        apply_fleet_metric_policy(&mut config.ram, policy.ram_threshold_pct, policy);
        apply_fleet_metric_policy(&mut config.upload, policy.upload_threshold_mb_s, policy);
        apply_fleet_metric_policy(&mut config.download, policy.download_threshold_mb_s, policy);
    }
    config
}

fn apply_fleet_metric_policy(
    metric: &mut MetricAlert,
    threshold: f64,
    policy: &crate::settings::FleetMetricAlertPolicy,
) {
    metric.threshold = threshold.max(0.1);
    metric.hysteresis_enabled = true;
    metric.hysteresis_pct = policy.reset_pct.clamp(1, 90);
    metric.sustained_enabled = true;
    metric.sustained_secs = policy.sustained_secs.clamp(1, 600);
    metric.cooldown_secs = policy.cooldown_secs.min(86_400);
}

/// Refresh the sampler after a signed configuration epoch has changed the
/// Fleet master gate. This is called only after the complete settings object
/// has been assembled, so the sampler cannot observe a half-applied policy.
pub fn reload_from_settings(settings: &crate::settings::AppSettings) {
    if let Ok(mut config) = CONFIG.lock() {
        *config = config_from_settings(settings);
    }
}

fn policy_locks_path(locked_paths: &[String], path: &str) -> bool {
    locked_paths.iter().any(|locked| {
        let locked = locked.trim();
        !locked.is_empty()
            && (path.starts_with(locked) || format!("ideal.{path}").starts_with(locked))
    })
}

fn metric_report_path(metric: &str) -> &'static str {
    match metric {
        "cpu" => "notifications.cpuUsage.reportToFleet",
        "ram" => "notifications.ramUsage.reportToFleet",
        _ => "notifications.networkUsage.reportToFleet",
    }
}

fn ensure_report_to_fleet_changes_allowed(
    current: &MetricAlertsConfig,
    proposed: &MetricAlertsConfig,
    settings: &crate::settings::AppSettings,
) -> Result<(), String> {
    let master_enabled = settings.ideal.security.require_all_device_alerts_in_fleet;
    let master_locked = policy_locks_path(&settings.policy.locked_paths, MASTER_FLEET_ALERT_PATH);
    if master_enabled
        && master_locked
        && [
            proposed.cpu.report_to_fleet,
            proposed.ram.report_to_fleet,
            proposed.upload.report_to_fleet,
            proposed.download.report_to_fleet,
        ]
        .iter()
        .any(|enabled| !enabled)
    {
        return Err("Fleet policy requires all device alerts to be reported".to_string());
    }

    for (metric, before, after) in [
        (
            "cpu",
            current.cpu.report_to_fleet,
            proposed.cpu.report_to_fleet,
        ),
        (
            "ram",
            current.ram.report_to_fleet,
            proposed.ram.report_to_fleet,
        ),
        (
            "upload",
            current.upload.report_to_fleet,
            proposed.upload.report_to_fleet,
        ),
        (
            "download",
            current.download.report_to_fleet,
            proposed.download.report_to_fleet,
        ),
    ] {
        if before != after
            && policy_locks_path(&settings.policy.locked_paths, metric_report_path(metric))
        {
            return Err("This Fleet alert setting is locked by administrator policy".to_string());
        }
    }
    Ok(())
}

/// Per-metric evaluator state — not exposed; lives entirely in the sampler.
struct AlertState {
    breach_start: Option<Instant>,
    alerted: bool,
    last_fired: Option<Instant>,
}

impl AlertState {
    fn new() -> Self {
        Self {
            breach_start: None,
            alerted: false,
            last_fired: None,
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

/// One threshold breach that has transitioned into an alert. Grouping these
/// related fields prevents the notification/reporting boundary from gaining a
/// brittle positional parameter list as alert metadata evolves.
struct AlertFire<'a> {
    metric: &'a str,
    label: &'a str,
    value: f64,
    unit: &'a str,
    threshold: f64,
    report_to_fleet: bool,
    sustained_secs: u32,
}

/// Evaluate one metric alert. It emits exactly one fire transition for a
/// continuous breach, then a recovery transition below the reset level. A
/// later breach respects the configured cooldown before it can fire again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertTransition {
    None,
    Fired,
    Recovered,
}

fn evaluate(
    cfg: &MetricAlert,
    state: &mut AlertState,
    value: f64,
    now: Instant,
) -> AlertTransition {
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
            let cooldown_elapsed = state
                .last_fired
                .map(|last| now.duration_since(last).as_secs() >= cfg.cooldown_secs as u64)
                .unwrap_or(true);
            if !cooldown_elapsed {
                return AlertTransition::None;
            }
            state.alerted = true;
            state.last_fired = Some(now);
            return AlertTransition::Fired;
        }
        AlertTransition::None
    } else {
        let reset_level = if cfg.hysteresis_enabled {
            cfg.threshold * (1.0 - cfg.hysteresis_pct as f64 / 100.0)
        } else {
            cfg.threshold
        };
        if value < reset_level {
            state.alerted = false;
            state.breach_start = None;
            return AlertTransition::Recovered;
        }
        AlertTransition::None
    }
}

fn fire_alert(app: &AppHandle, alert: AlertFire<'_>) {
    let body = format!(
        "{} hit {:.1}{} (limit {:.0}{}).",
        alert.label, alert.value, alert.unit, alert.threshold, alert.unit
    );
    if let Err(e) =
        crate::native_notify::show_native_notification(app, "WinCommander - Alert", &body)
    {
        crate::log_message("warn", &format!("[MetricAlert] notification failed: {}", e));
    }
    let _ = app.emit(
        "metrics://metric-alert",
        MetricAlertPayload {
            metric: alert.metric.to_string(),
            label: alert.label.to_string(),
            value: alert.value,
            unit: alert.unit.to_string(),
            threshold: alert.threshold,
        },
    );
    crate::log_message("warn", &format!("[MetricAlert] {}", body));

    if alert.report_to_fleet {
        // "cpu"|"ram" map 1:1; "upload"/"download" both collapse to the
        // server's single "network_usage" alert type (see the contract's
        // LocalAlertReport doc) — the fleet console models network as one
        // reportable metric, not per-direction.
        let alert_type = match alert.metric {
            "cpu" => "cpu_usage",
            "ram" => "ram_usage",
            _ => "network_usage",
        };
        let detail = if alert_type == "network_usage" {
            // Throughput is sampled in MB/s, never percent. Keep its unit
            // honest all the way to the Fleet console.
            serde_json::json!({
                "direction": alert.metric,
                "value_mb_s": alert.value,
                "threshold_mb_s": alert.threshold,
                "duration_s": alert.sustained_secs,
            })
        } else {
            serde_json::json!({
                "metric": alert.metric,
                "value_pct": alert.value,
                "threshold_pct": alert.threshold,
                "duration_s": alert.sustained_secs,
            })
        };
        let alert_type = alert_type.to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(error) =
                crate::fleet_agent::fleet_report_local_alert(alert_type.clone(), detail).await
            {
                crate::flow_bridge::flow_trace(format!(
                    "[MetricAlert] Fleet report not queued ({}): {}",
                    alert_type, error
                ));
            }
        });
    }
}

fn fire_recovery(app: &AppHandle, alert: AlertFire<'_>) {
    let body = format!("{} recovered below its reset level.", alert.label);
    if let Err(e) =
        crate::native_notify::show_native_notification(app, "WinCommander - Alert recovery", &body)
    {
        crate::log_message(
            "warn",
            &format!("[MetricAlert] recovery notification failed: {}", e),
        );
    }
    let _ = app.emit(
        "metrics://metric-alert-recovery",
        MetricAlertPayload {
            metric: alert.metric.to_string(),
            label: alert.label.to_string(),
            value: alert.value,
            unit: alert.unit.to_string(),
            threshold: alert.threshold,
        },
    );
    if alert.report_to_fleet {
        let alert_type = match alert.metric {
            "cpu" => "cpu_usage_recovered",
            "ram" => "ram_usage_recovered",
            _ => "network_usage_recovered",
        };
        let detail = serde_json::json!({
            "metric": alert.metric,
            "value": alert.value,
            "unit": alert.unit,
            "threshold": alert.threshold,
            "recovered": true,
        });
        let alert_type = alert_type.to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(error) =
                crate::fleet_agent::fleet_report_local_alert(alert_type.clone(), detail).await
            {
                crate::flow_bridge::flow_trace(format!(
                    "[MetricAlert] Fleet recovery not queued ({}): {}",
                    alert_type, error
                ));
            }
        });
    }
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
        match evaluate(cfg, state, value, now) {
            AlertTransition::Fired => fire_alert(
                app,
                AlertFire {
                    metric,
                    label,
                    value,
                    unit,
                    threshold: cfg.threshold,
                    report_to_fleet: cfg.report_to_fleet,
                    sustained_secs: cfg.sustained_secs,
                },
            ),
            AlertTransition::Recovered => fire_recovery(
                app,
                AlertFire {
                    metric,
                    label,
                    value,
                    unit,
                    threshold: cfg.threshold,
                    report_to_fleet: cfg.report_to_fleet,
                    sustained_secs: cfg.sustained_secs,
                },
            ),
            AlertTransition::None => {}
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
        reload_from_settings(&s);
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
        let mut last_evaluated = Instant::now() - SAMPLE_INTERVAL;

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
            if now.duration_since(last_evaluated).as_secs()
                < cfg.evaluation_interval_secs.max(1) as u64
            {
                tokio::time::sleep(SAMPLE_INTERVAL).await;
                continue;
            }
            last_evaluated = now;
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

    let mut normalized = MetricAlertsConfig {
        cpu: config.cpu.normalized(),
        ram: config.ram.normalized(),
        upload: config.upload.normalized(),
        download: config.download.normalized(),
        evaluation_interval_secs: config.evaluation_interval_secs.clamp(1, 300),
    };
    let system_reports_to_fleet = normalized.cpu.report_to_fleet && normalized.ram.report_to_fleet;
    normalized.cpu.report_to_fleet = system_reports_to_fleet;
    normalized.ram.report_to_fleet = system_reports_to_fleet;
    let network_reports_to_fleet =
        normalized.upload.report_to_fleet && normalized.download.report_to_fleet;
    normalized.upload.report_to_fleet = network_reports_to_fleet;
    normalized.download.report_to_fleet = network_reports_to_fleet;

    let settings = crate::settings::read_settings()?;
    let current = config_from_settings(&settings);
    ensure_report_to_fleet_changes_allowed(&current, &normalized, &settings)?;

    // A managed master policy is effective immediately. Persist the local
    // preference for later, but never let it weaken the live report gate.
    let mut effective = normalized.clone();
    if settings.ideal.security.require_all_device_alerts_in_fleet {
        effective.cpu.report_to_fleet = true;
        effective.ram.report_to_fleet = true;
        effective.upload.report_to_fleet = true;
        effective.download.report_to_fleet = true;
    }
    *CONFIG.lock().unwrap() = effective.clone();

    // Persist via the settings patch path so the value survives restart.
    let patch = serde_json::json!({
        "app": {
            "metricAlerts": {
                "cpu": normalized.cpu.to_json(),
                "ram": normalized.ram.to_json(),
                "upload": normalized.upload.to_json(),
                "download": normalized.download.to_json(),
                "evaluationIntervalSecs": normalized.evaluation_interval_secs,
            }
        }
    });
    crate::settings::patch_settings(patch).map_err(|e| format!("persist failed: {}", e))?;

    Ok(effective)
}

/// Read the current metric-alert config. Free — reading the config doesn't
/// expose any paid capability and the cards need it to render their controls.
#[tauri::command]
pub fn metric_alerts_get_config() -> MetricAlertsConfig {
    CONFIG.lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_master_forces_cpu_and_network_reporting_effective() {
        let mut settings = crate::settings::create_default_settings();
        settings.ideal.security.require_all_device_alerts_in_fleet = true;

        let config = config_from_settings(&settings);
        assert!(config.cpu.report_to_fleet);
        assert!(config.upload.report_to_fleet);
        assert!(config.download.report_to_fleet);
    }

    #[test]
    fn typed_device_policy_forces_paired_resource_reporting() {
        let mut settings = crate::settings::create_default_settings();
        settings
            .ideal
            .security
            .metric_alert_reporting
            .system_report_to_fleet = Some(true);
        settings
            .ideal
            .security
            .metric_alert_reporting
            .network_report_to_fleet = Some(true);

        let config = config_from_settings(&settings);

        assert!(config.cpu.report_to_fleet);
        assert!(config.ram.report_to_fleet);
        assert!(config.upload.report_to_fleet);
        assert!(config.download.report_to_fleet);
    }

    #[test]
    fn typed_fleet_policy_overrides_threshold_timing_and_reset_for_every_metric() {
        let mut settings = crate::settings::create_default_settings();
        settings.ideal.security.metric_alert_reporting.policy =
            Some(crate::settings::FleetMetricAlertPolicy {
                evaluation_interval_secs: 5,
                sustained_secs: 45,
                cooldown_secs: 600,
                reset_pct: 25,
                cpu_threshold_pct: 50.0,
                ram_threshold_pct: 70.0,
                upload_threshold_mb_s: 12.5,
                download_threshold_mb_s: 20.0,
            });

        let config = config_from_settings(&settings);

        assert_eq!(config.evaluation_interval_secs, 5);
        assert_eq!(config.cpu.threshold, 50.0);
        assert_eq!(config.ram.threshold, 70.0);
        assert_eq!(config.upload.threshold, 12.5);
        assert_eq!(config.download.threshold, 20.0);
        for metric in [&config.cpu, &config.ram, &config.upload, &config.download] {
            assert!(metric.sustained_enabled);
            assert_eq!(metric.sustained_secs, 45);
            assert_eq!(metric.cooldown_secs, 600);
            assert!(metric.hysteresis_enabled);
            assert_eq!(metric.hysteresis_pct, 25);
        }
    }

    #[test]
    fn locked_fleet_master_rejects_local_report_disable() {
        let mut settings = crate::settings::create_default_settings();
        settings.ideal.security.require_all_device_alerts_in_fleet = true;
        settings.policy.locked_paths = vec![MASTER_FLEET_ALERT_PATH.to_string()];
        let current = config_from_settings(&settings);
        let mut proposed = current.clone();
        proposed.cpu.report_to_fleet = false;

        assert!(ensure_report_to_fleet_changes_allowed(&current, &proposed, &settings).is_err());
    }

    #[test]
    fn locked_network_report_path_rejects_either_network_direction() {
        let mut settings = crate::settings::create_default_settings();
        settings.policy.locked_paths = vec!["notifications.networkUsage.reportToFleet".to_string()];
        let current = config_from_settings(&settings);
        let mut proposed = current.clone();
        proposed.download.report_to_fleet = !current.download.report_to_fleet;

        assert!(ensure_report_to_fleet_changes_allowed(&current, &proposed, &settings).is_err());
    }

    #[test]
    fn network_fleet_reporting_is_disabled_when_legacy_directions_disagree() {
        let mut settings = crate::settings::create_default_settings();
        settings.app.metric_alerts.upload.report_to_fleet = true;
        settings.app.metric_alerts.download.report_to_fleet = false;

        let config = config_from_settings(&settings);

        assert!(!config.upload.report_to_fleet);
        assert!(!config.download.report_to_fleet);
    }

    #[test]
    fn system_fleet_reporting_is_disabled_when_legacy_metrics_disagree() {
        let mut settings = crate::settings::create_default_settings();
        settings.app.metric_alerts.cpu.report_to_fleet = true;
        settings.app.metric_alerts.ram.report_to_fleet = false;

        let config = config_from_settings(&settings);

        assert!(!config.cpu.report_to_fleet);
        assert!(!config.ram.report_to_fleet);
    }

    #[test]
    fn evaluator_emits_one_breach_then_recovery_then_respects_cooldown() {
        let cfg = MetricAlert {
            enabled: true,
            threshold: 50.0,
            hysteresis_enabled: true,
            hysteresis_pct: 20,
            sustained_enabled: false,
            sustained_secs: 1,
            cooldown_secs: 60,
            report_to_fleet: true,
        };
        let start = Instant::now();
        let mut state = AlertState::new();

        assert_eq!(
            evaluate(&cfg, &mut state, 70.0, start),
            AlertTransition::Fired
        );
        assert_eq!(
            evaluate(&cfg, &mut state, 80.0, start + Duration::from_secs(5)),
            AlertTransition::None,
            "a continuing breach is one incident"
        );
        assert_eq!(
            evaluate(&cfg, &mut state, 39.0, start + Duration::from_secs(10)),
            AlertTransition::Recovered
        );
        assert_eq!(
            evaluate(&cfg, &mut state, 70.0, start + Duration::from_secs(20)),
            AlertTransition::None,
            "a new breach inside cooldown is suppressed"
        );
        assert_eq!(
            evaluate(&cfg, &mut state, 70.0, start + Duration::from_secs(61)),
            AlertTransition::Fired
        );
    }
}
