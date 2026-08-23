// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, process-local startup timing for diagnosing launch regressions.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const MAX_EVENTS: usize = 96;
const TRACE_EVENT_NAME: &str = "startup://milestone";
const TRACE_PHASE_EVENT_NAME: &str = "startup://phase";
const MAX_REPORTED_DURATION_MS: u64 = 60 * 60 * 1_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StartupJobId {
    SettingsCache,
    SystemProbe,
    StartupStatus,
    ProInstallStatus,
    ProManifest,
    ProHash,
    DefenderStatus,
    Dependencies,
    MeshStatus,
    AppInventory,
    PanelPreload,
    DiskCleanupPreload,
    SearchPreload,
}

impl StartupJobId {
    fn as_str(&self) -> &'static str {
        match self {
            Self::SettingsCache => "settings-cache",
            Self::SystemProbe => "system-probe",
            Self::StartupStatus => "startup-status",
            Self::ProInstallStatus => "pro-install-status",
            Self::ProManifest => "pro-manifest",
            Self::ProHash => "pro-hash",
            Self::DefenderStatus => "defender-status",
            Self::Dependencies => "dependencies",
            Self::MeshStatus => "mesh-status",
            Self::AppInventory => "app-inventory",
            Self::PanelPreload => "panel-preload",
            Self::DiskCleanupPreload => "disk-cleanup-preload",
            Self::SearchPreload => "search-preload",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StartupMilestone {
    Queued,
    Started,
    Completed,
    TimedOut,
    Cancelled,
    Failed,
}

impl StartupMilestone {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::TimedOut => "timed-out",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartupPhaseId {
    ProcessStart,
    NativeSetupEntered,
    MainWindowShowRequested,
    WebviewDomReady,
    SettingsCacheHydrated,
    DashboardFirstVisible,
    DashboardInteractive,
    ProtectionRequiredReady,
    ProtectionNotRequired,
    ProtectionFailed,
    FreshSystemProbeComplete,
    BackgroundIdle,
}

impl StartupPhaseId {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ProcessStart => "process_start",
            Self::NativeSetupEntered => "native_setup_entered",
            Self::MainWindowShowRequested => "main_window_show_requested",
            Self::WebviewDomReady => "webview_dom_ready",
            Self::SettingsCacheHydrated => "settings_cache_hydrated",
            Self::DashboardFirstVisible => "dashboard_first_visible",
            Self::DashboardInteractive => "dashboard_interactive",
            Self::ProtectionRequiredReady => "protection_required_ready",
            Self::ProtectionNotRequired => "protection_not_required",
            Self::ProtectionFailed => "protection_failed",
            Self::FreshSystemProbeComplete => "fresh_system_probe_complete",
            Self::BackgroundIdle => "background_idle",
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartupPhaseEvent {
    pub phase: StartupPhaseId,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartupMilestoneEvent {
    pub job: StartupJobId,
    pub milestone: StartupMilestone,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartupTraceEvent {
    pub kind: String,
    pub name: String,
    pub elapsed_ms: u64,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupTraceSnapshot {
    pub launch_id: String,
    pub elapsed_ms: u64,
    pub events: Vec<StartupTraceEvent>,
}

struct StartupTrace {
    launch_id: String,
    started_at: Instant,
    events: Mutex<VecDeque<StartupTraceEvent>>,
}

impl StartupTrace {
    fn new() -> Self {
        Self {
            launch_id: Uuid::new_v4().to_string(),
            started_at: Instant::now(),
            events: Mutex::new(VecDeque::with_capacity(MAX_EVENTS)),
        }
    }

    fn record(&self, kind: &str, name: &str, status: &str) -> StartupTraceEvent {
        let elapsed_ms = u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.record_at(kind, name, status, elapsed_ms)
    }

    fn record_at(
        &self,
        kind: &str,
        name: &str,
        status: &str,
        elapsed_ms: u64,
    ) -> StartupTraceEvent {
        let event = StartupTraceEvent {
            kind: kind.to_string(),
            name: name.to_string(),
            elapsed_ms,
            status: status.to_string(),
        };
        if let Ok(mut events) = self.events.lock() {
            if events.len() == MAX_EVENTS {
                events.pop_front();
            }
            events.push_back(event.clone());
        }
        event
    }

    fn snapshot(&self) -> StartupTraceSnapshot {
        StartupTraceSnapshot {
            launch_id: self.launch_id.clone(),
            elapsed_ms: u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            events: self
                .events
                .lock()
                .map(|events| events.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }
}

static STARTUP_TRACE: Lazy<StartupTrace> = Lazy::new(StartupTrace::new);

pub fn init() {
    Lazy::force(&STARTUP_TRACE);
}

pub fn pre_window_milestone(name: &str) {
    let _ = STARTUP_TRACE.record("milestone", name, "complete");
}

pub fn milestone(_app: &AppHandle, name: &str) {
    let _ = STARTUP_TRACE.record("milestone", name, "complete");
}

pub fn job_started(_app: &AppHandle, name: &str) {
    let _ = STARTUP_TRACE.record("job", name, "started");
}

pub fn job_finished(_app: &AppHandle, name: &str, succeeded: bool) {
    let status = if succeeded { "complete" } else { "failed" };
    let _ = STARTUP_TRACE.record("job", name, status);
}

/// Reports an allowlisted frontend startup milestone. This command deliberately
/// accepts no labels, error text, paths, settings, or command arguments.
#[tauri::command]
pub fn report_startup_milestone(
    app: AppHandle,
    job: StartupJobId,
    milestone: StartupMilestone,
    duration_ms: u64,
) -> Result<(), String> {
    let event = StartupMilestoneEvent {
        job,
        milestone,
        duration_ms: duration_ms.min(MAX_REPORTED_DURATION_MS),
    };
    let _ = STARTUP_TRACE.record("frontend-job", event.job.as_str(), event.milestone.as_str());
    app.emit(TRACE_EVENT_NAME, &event)
        .map_err(|error| format!("emit startup milestone: {error}"))
}

/// Reports one allowlisted, value-free launch phase into the native clock.
#[tauri::command]
pub fn report_startup_phase(app: AppHandle, phase: StartupPhaseId) -> Result<(), String> {
    let trace_event = STARTUP_TRACE.record("frontend-phase", phase.as_str(), "complete");
    let event = StartupPhaseEvent {
        phase,
        elapsed_ms: trace_event.elapsed_ms,
    };
    app.emit(TRACE_PHASE_EVENT_NAME, &event)
        .map_err(|error| format!("emit startup phase: {error}"))
}

/// Safe, process-local startup trace. Event names and statuses only; no paths,
/// settings, command arguments, account details, or error strings are retained.
#[tauri::command]
pub fn get_startup_trace() -> StartupTraceSnapshot {
    STARTUP_TRACE.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_evicts_oldest_events_at_the_bound() {
        let trace = StartupTrace::new();
        for index in 0..=MAX_EVENTS {
            trace.record_at("job", &format!("job-{index}"), "complete", index as u64);
        }

        let snapshot = trace.snapshot();
        assert_eq!(snapshot.events.len(), MAX_EVENTS);
        assert_eq!(snapshot.events.first().unwrap().name, "job-1");
        assert_eq!(
            snapshot.events.last().unwrap().name,
            format!("job-{MAX_EVENTS}")
        );
    }

    #[test]
    fn trace_event_carries_only_contract_fields() {
        let trace = StartupTrace::new();
        let event = trace.record_at("milestone", "setup.entered", "complete", 42);
        assert_eq!(event.kind, "milestone");
        assert_eq!(event.name, "setup.entered");
        assert_eq!(event.status, "complete");
        assert_eq!(event.elapsed_ms, 42);
    }

    #[test]
    fn frontend_contract_rejects_arbitrary_job_and_milestone_names() {
        assert_eq!(
            serde_json::from_str::<StartupJobId>(r#""settings-cache""#).unwrap(),
            StartupJobId::SettingsCache
        );
        assert_eq!(
            serde_json::from_str::<StartupMilestone>(r#""timed-out""#).unwrap(),
            StartupMilestone::TimedOut
        );
        assert_eq!(
            serde_json::from_str::<StartupPhaseId>(r#""dashboard_interactive""#).unwrap(),
            StartupPhaseId::DashboardInteractive
        );
        assert!(serde_json::from_str::<StartupJobId>(r#""arbitrary-command""#).is_err());
        assert!(serde_json::from_str::<StartupMilestone>(r#""diagnostic""#).is_err());
        assert!(serde_json::from_str::<StartupPhaseId>(r#""arbitrary_phase""#).is_err());
    }
}
