// src-tauri/src/flow_engine.rs
//
// Flow Engine — n8n-style modular automation for WinCommander.
//
// Architecture:
//   1. Flows are stored in settings.json → app.flows[]
//   2. Each flow has: 1 Trigger → 0-N Conditions → 1-N Actions
//   3. Trigger listeners run in background tasks, started/stopped dynamically
//   4. When a trigger fires: evaluate conditions → execute actions sequentially
//   5. Execution log kept in memory (ring buffer, last 50 runs)
//
// Adding a new trigger/action = add one enum variant + one handler. Nothing else.
//
// Keep types in sync with: src/types/flows.ts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tokio::sync::{mpsc, Notify};

use crate::backend;
use crate::settings;

// ═══════════════════════════════════════════════════════════════════════
// BLOCK TYPE DEFINITIONS (mirrors src/types/flows.ts)
// ═══════════════════════════════════════════════════════════════════════

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TriggerBlock {
    HotkeyTrigger {
        hotkey: String,
    },
    KeySequenceTrigger {
        key: String,
        taps: u32,
        #[serde(rename = "windowMs")]
        window_ms: u64,
    },
    USBTrigger {
        mode: String, // "insert" | "remove"
        #[serde(rename = "deviceInstanceId")]
        device_instance_id: Option<String>,
        #[serde(rename = "deviceName")]
        device_name: Option<String>,
        #[serde(rename = "delaySeconds", default)]
        delay_seconds: u64,
    },
    LidCloseTrigger {},
    WebhookTrigger {
        path: String,
        secret: String,
    },
    CameraTrigger {
        event: String,
    },
    ScheduleTrigger {
        #[serde(rename = "cronExpression")]
        cron_expression: String,
    },
    NetworkTrigger {
        event: String,
    },
    FileTrigger {
        path: String,
        event: String,
    },
    ProcessTrigger {
        #[serde(rename = "processName")]
        process_name: String,
        event: String,
    },
    SignalReceivedTrigger {},
    /// `flows.add-monitor-triggers` — subscribes to the paste monitor's
    /// `paste-monitor-detected` Tauri event. Optional filters narrow to
    /// a single pattern category or severity; both None = fire on any
    /// detection.
    PasteMonitorTrigger {
        /// Match against `DetectionEvent.pattern` substring (case-insensitive).
        /// e.g. "AWS" matches "AWS Access Key", "AWS Secret Key".
        /// None = any pattern.
        #[serde(default)]
        pattern_contains: Option<String>,
        /// Match against `DetectionEvent.severity` ("warning" | "danger").
        /// None = any severity.
        #[serde(default)]
        severity: Option<String>,
    },
    /// `flows.add-monitor-triggers` — subscribes to the decoy monitor's
    /// `decoy-accessed` Tauri event. Optional `path_contains` filter
    /// narrows to a specific decoy file or directory; None = any decoy.
    DecoyMonitorTrigger {
        #[serde(default)]
        path_contains: Option<String>,
    },
    /// `flows.add-monitor-triggers` — subscribes to the ransomware
    /// monitor's `ransomware-detected` Tauri event. No filter — there's
    /// only one event shape.
    RansomwareMonitorTrigger {},
    /// `network.wifi-guard` — subscribes to the
    /// `wifi-guard-detected` event from `wifi_check.rs`.
    /// Optional `ssid_contains` substring filter narrows to a specific
    /// SSID; None = any rogue association.
    WifiGuardTrigger {
        #[serde(default)]
        ssid_contains: Option<String>,
    },
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConditionBlock {
    TimeCondition {
        #[serde(rename = "startHour")]
        start_hour: u32,
        #[serde(rename = "endHour")]
        end_hour: u32,
    },
    SettingCondition {
        path: String,
        operator: String,
        value: serde_json::Value,
    },
    NetworkCondition {
        mode: String,
        #[serde(rename = "ssidName")]
        ssid_name: Option<String>,
    },
    BatteryCondition {
        operator: String,
        percentage: u32,
    },
    USBPresenceCondition {
        #[serde(rename = "deviceInstanceId")]
        device_instance_id: Option<String>,
        #[serde(rename = "deviceName")]
        device_name: Option<String>,
        mode: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ActionBlock {
    CommandAction {
        command: String,
        #[serde(default)]
        params: Option<HashMap<String, String>>,
    },
    SignalAction {
        #[serde(rename = "targetRole")]
        target_role: String,
        #[serde(rename = "signalType")]
        signal_type: String,
    },
    HTTPAction {
        method: String,
        url: String,
        headers: Option<HashMap<String, String>>,
        body: Option<String>,
    },
    NotifyAction {
        message: String,
        severity: String,
        duration: Option<u64>,
    },
    DelayAction {
        seconds: u64,
    },
    ShellAction {
        script: String,
    },
    ParallelGroup {
        actions: Vec<ActionBlock>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    pub id: String,
    pub name: String,
    pub system: bool,
    pub enabled: bool,
    pub triggers: Vec<TriggerBlock>,
    #[serde(default)]
    pub conditions: Vec<ConditionBlock>,
    pub actions: Vec<ActionBlock>,

    // ── Metadata (flows.metadata-fields) ────────────────────────────
    //
    // All optional with #[serde(default)] so existing settings.json
    // entries without these fields keep loading cleanly. Frontend
    // surfaces them in a "Metadata" pane on the flow editor; they
    // drive filter / sort / risk-confirm UX but don't affect runtime
    // execution.
    /// Free-text "why this flow exists" — visible only in the editor.
    /// Useful when revisiting a flow you wrote 6 months ago.
    #[serde(default)]
    pub notes: String,
    /// Free-form tags for filtering and grouping (e.g. ["travel",
    /// "emergency", "daily-driver"]).
    #[serde(default)]
    pub tags: Vec<String>,
    /// In a fleet/team context, who authored this flow. Free-form
    /// string — typically an email or handle. Empty on personal-use.
    #[serde(default)]
    pub owner: String,
    /// Operator-visible risk classification. Drives the
    /// confirmation-on-save UX in the editor (a high-risk flow asks
    /// for an extra Are-You-Sure before save). Default "low".
    #[serde(default)]
    pub risk_level: FlowRiskLevel,
    /// ISO-8601 timestamp of the last time the user reviewed / signed
    /// off on this flow. Surfaced as "stale flow" badge if older than
    /// e.g. 90 days. Empty = never reviewed.
    #[serde(default)]
    pub last_reviewed_at: String,
}

/// Operator-visible risk classification on every Flow.
/// Defaults to Low — opt-in for confirmations on Medium/High.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlowRiskLevel {
    #[default]
    Low,
    Medium,
    High,
}

// ═══════════════════════════════════════════════════════════════════════
// CONTINGENCY SETTINGS (identity table, signal transport, USB gate)
// ═══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContingencyIdentity {
    #[serde(default)]
    pub brand: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub phrase: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContingencySettings {
    #[serde(default)]
    pub identities: HashMap<String, ContingencyIdentity>,
    #[serde(default)]
    pub my_index: u32,
    #[serde(default = "default_signal_transport")]
    pub signal_transport: String,
    #[serde(default)]
    pub hmac_key: String,
    #[serde(default)]
    pub server_tailscale_ip: String,
    #[serde(default)]
    pub usb_key_device_id: Option<String>,
    #[serde(default)]
    pub usb_gate_enabled: bool,
    #[serde(default = "default_webhook_port")]
    pub webhook_port: u16,
}

fn default_signal_transport() -> String {
    "taildrop".to_string()
}

fn default_webhook_port() -> u16 {
    47821
}

impl Default for ContingencySettings {
    fn default() -> Self {
        Self {
            identities: HashMap::new(),
            my_index: 0,
            signal_transport: default_signal_transport(),
            hmac_key: String::new(),
            server_tailscale_ip: String::new(),
            usb_key_device_id: None,
            usb_gate_enabled: false,
            webhook_port: default_webhook_port(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// EXECUTION LOG
// ═══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowStepResult {
    pub action_index: usize,
    pub action_type: String,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
    /// `true` when the action ran in dry-run mode (no side effects).
    /// `#[serde(default)]` so historical execution logs deserialize cleanly.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowExecutionEvent {
    pub timestamp: String,
    pub level: String,
    pub stage: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowExecution {
    pub flow_id: String,
    pub trigger_type: String,
    pub started_at: String,
    pub events: Vec<FlowExecutionEvent>,
    pub steps: Vec<FlowStepResult>,
    pub completed: bool,
    pub total_duration_ms: u64,
    /// `true` when this execution was a dry run (no irreversible side
    /// effects). `#[serde(default)]` for back-compat with historical
    /// execution logs.
    #[serde(default)]
    pub dry_run: bool,
}

// ═══════════════════════════════════════════════════════════════════════
// ENGINE STATE — Managed by Tauri
// ═══════════════════════════════════════════════════════════════════════

/// Internal message sent from trigger listeners to the engine loop.
pub struct TriggerEvent {
    pub flow_id: String,
    pub trigger_type: String,
    pub detail: Option<String>,
}

/// Tracks active trigger listener tasks so they can be stopped on flow edit.
struct ActiveListener {
    /// Join handles for all spawned trigger tasks — call .abort() on each to cancel
    handles: Vec<tauri::async_runtime::JoinHandle<()>>,
    /// If any listener registered global shortcuts, store them for cleanup
    registered_hotkeys: Vec<String>,
}

/// Core engine state — managed as Tauri state via app.manage().
pub struct FlowEngineState {
    /// Channel sender for trigger events
    tx: mpsc::UnboundedSender<TriggerEvent>,
    /// Active trigger listeners keyed by flow ID
    listeners: Mutex<HashMap<String, ActiveListener>>,
    /// Per-flow listener-startup errors (e.g. "hotkey conflict", "Pvt Mesh
    /// VPN down", "AV blocked WH_KEYBOARD_LL"). Surfaces via
    /// `flow_health::get_flow_health` so the operator sees WHY the trigger
    /// isn't firing instead of guessing. Cleared on stop_listener.
    listener_errors: Mutex<HashMap<String, String>>,
    /// Execution log ring buffer (last 50 runs)
    executions: Mutex<Vec<FlowExecution>>,
}

const MAX_EXECUTIONS: usize = 50;

#[derive(Debug, Clone)]
struct ConditionEvaluation {
    passed: bool,
    detail: String,
}

fn default_system_flows() -> Vec<Flow> {
    vec![
        Flow {
            id: "contingency".to_string(),
            name: "Contingency".to_string(),
            system: true,
            enabled: true,
            triggers: vec![TriggerBlock::KeySequenceTrigger {
                key: "F12".to_string(),
                taps: 3,
                window_ms: 1000,
            }],
            conditions: vec![],
            actions: vec![
                ActionBlock::SignalAction {
                    target_role: "admins".to_string(),
                    signal_type: "contingency".to_string(),
                },
                ActionBlock::CommandAction {
                    command: "Disconnect-AllRDPSessions".to_string(),
                    params: None,
                },
                ActionBlock::CommandAction {
                    command: "Start-ContingencySequence".to_string(),
                    params: None,
                },
            ],
            notes: "Triple-tap F12 within 1s to fire the full contingency cascade. System flow — editable but not deletable.".to_string(),
            tags: vec!["system".to_string(), "panic".to_string()],
            owner: String::new(),
            risk_level: FlowRiskLevel::High,
            last_reviewed_at: String::new(),
        },
        Flow {
            id: "panic-hotkey".to_string(),
            name: "Panic Hotkey".to_string(),
            system: true,
            enabled: true,
            triggers: vec![TriggerBlock::HotkeyTrigger {
                hotkey: "Ctrl+Shift+Q".to_string(),
            }],
            conditions: vec![],
            actions: vec![
                ActionBlock::SignalAction {
                    target_role: "admins".to_string(),
                    signal_type: "contingency".to_string(),
                },
                ActionBlock::CommandAction {
                    command: "Disconnect-AllRDPSessions".to_string(),
                    params: None,
                },
                ActionBlock::CommandAction {
                    command: "Start-ContingencySequence".to_string(),
                    params: None,
                },
            ],
            notes: "Ctrl+Shift+Q from anywhere fires the full contingency cascade. System flow — editable but not deletable.".to_string(),
            tags: vec!["system".to_string(), "panic".to_string()],
            owner: String::new(),
            risk_level: FlowRiskLevel::High,
            last_reviewed_at: String::new(),
        },
        Flow {
            id: "lid-guard".to_string(),
            name: "Lid Guard".to_string(),
            system: true,
            enabled: false,
            triggers: vec![TriggerBlock::LidCloseTrigger {}],
            conditions: vec![],
            actions: vec![
                ActionBlock::CommandAction {
                    command: "Disconnect-AllRDPSessions".to_string(),
                    params: None,
                },
                ActionBlock::SignalAction {
                    target_role: "admins".to_string(),
                    signal_type: "alert".to_string(),
                },
            ],
            notes: "Closing the laptop lid kills RDP sessions and alerts admins. Off by default — enable for travel.".to_string(),
            tags: vec!["system".to_string(), "travel".to_string()],
            owner: String::new(),
            risk_level: FlowRiskLevel::Medium,
            last_reviewed_at: String::new(),
        },
        Flow {
            id: "usb-guard".to_string(),
            name: "USB Key Guard".to_string(),
            system: true,
            enabled: false,
            triggers: vec![TriggerBlock::USBTrigger {
                mode: "remove".to_string(),
                device_instance_id: None,
                device_name: None,
                delay_seconds: 0,
            }],
            conditions: vec![],
            actions: vec![
                ActionBlock::CommandAction {
                    command: "Disconnect-AllRDPSessions".to_string(),
                    params: None,
                },
                ActionBlock::SignalAction {
                    target_role: "admins".to_string(),
                    signal_type: "alert".to_string(),
                },
            ],
            notes: "Removing the designated USB key kills RDP sessions and alerts admins. Configure the device ID in the trigger settings. Off by default.".to_string(),
            tags: vec!["system".to_string(), "deadman".to_string()],
            owner: String::new(),
            risk_level: FlowRiskLevel::Medium,
            last_reviewed_at: String::new(),
        },
    ]
}

fn sanitize_legacy_flow(mut flow: Flow) -> Flow {
    for action in &mut flow.actions {
        if let ActionBlock::CommandAction { command, .. } = action {
            if command == "Disconnect-OwnRDP" {
                *command = "Disconnect-AllRDPSessions".to_string();
            }
        }
    }

    flow
}

fn repair_system_flow(mut flow: Flow, default_flow: &Flow) -> Flow {
    flow.system = true;

    if flow.triggers.is_empty() {
        flow.triggers = default_flow.triggers.clone();
    }
    if flow.actions.is_empty() {
        flow.actions = default_flow.actions.clone();
    }

    for (index, action) in flow.actions.iter_mut().enumerate() {
        if let ActionBlock::CommandAction { command, params } = action {
            if command.trim().is_empty() {
                if let Some(ActionBlock::CommandAction {
                    command: default_command,
                    params: default_params,
                }) = default_flow.actions.get(index)
                {
                    *command = default_command.clone();
                    if params.is_none() {
                        *params = default_params.clone();
                    }
                }
            }
        }
    }

    flow
}

fn merge_default_system_flows(flows: Vec<Flow>) -> Vec<Flow> {
    let mut merged = Vec::new();
    let defaults = default_system_flows();

    for default_flow in defaults {
        if let Some(existing) = flows.iter().find(|flow| flow.id == default_flow.id) {
            let sanitized =
                repair_system_flow(sanitize_legacy_flow(existing.clone()), &default_flow);
            merged.push(sanitized);
        } else {
            merged.push(default_flow);
        }
    }

    for flow in flows {
        if merged.iter().any(|existing| existing.id == flow.id) {
            continue;
        }
        merged.push(sanitize_legacy_flow(flow));
    }

    merged
}

fn flow_engine_log(level: &str, flow_id: Option<&str>, context: &str, message: impl AsRef<str>) {
    let prefix = match flow_id {
        Some(flow_id) => format!("[FlowEngine][{}][{}] ", flow_id, context),
        None => format!("[FlowEngine][{}] ", context),
    };
    crate::log_message_src(level, crate::LOG_SRC_FLOWS, &(prefix + message.as_ref()));
}

fn push_execution_event(
    events: &mut Vec<FlowExecutionEvent>,
    flow_id: &str,
    trigger_type: &str,
    level: &str,
    stage: &str,
    message: impl Into<String>,
) {
    let message = message.into();
    flow_engine_log(
        level,
        Some(flow_id),
        &format!("{}:{}", trigger_type, stage),
        &message,
    );
    events.push(FlowExecutionEvent {
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: level.to_string(),
        stage: stage.to_string(),
        message,
    });
}

pub(crate) fn persist_flows_to_settings(flows: &[Flow]) -> Result<(), String> {
    let patch = serde_json::json!({
        "app": { "flows": flows }
    });
    settings::patch_settings(patch).map(|_| ())
}

fn ensure_default_flows_persisted() {
    let flows = get_flows_from_settings();
    if let Err(err) = persist_flows_to_settings(&flows) {
        flow_engine_log(
            "error",
            None,
            "bootstrap",
            format!("Failed to persist merged flow catalog: {}", err),
        );
    }
}

fn validate_flow(flow: &Flow) -> Result<(), String> {
    if flow.triggers.is_empty() {
        return Err("Flow must have at least one trigger".to_string());
    }
    if flow.actions.is_empty() {
        return Err("Flow must have at least one action".to_string());
    }

    for trigger in &flow.triggers {
        match trigger {
            TriggerBlock::HotkeyTrigger { hotkey } => {
                let has_modifier = ["Ctrl", "Shift", "Alt", "Super"]
                    .iter()
                    .any(|modifier| hotkey.contains(modifier));
                if !has_modifier {
                    return Err(
                        "Hotkey must include at least one modifier (Ctrl, Shift, Alt, or Super)"
                            .to_string(),
                    );
                }
            }
            TriggerBlock::KeySequenceTrigger {
                key,
                taps,
                window_ms,
            } => {
                if key.trim().is_empty() {
                    return Err("Key sequence trigger requires a key".to_string());
                }
                if *taps < 2 {
                    return Err("Key sequence trigger must require at least 2 taps".to_string());
                }
                if *window_ms == 0 {
                    return Err("Key sequence trigger window must be greater than 0ms".to_string());
                }
            }
            TriggerBlock::WebhookTrigger { path, secret } => {
                if !path.starts_with('/') {
                    return Err(
                        "WebhookTrigger path must start with '/' (e.g. '/my-flow')".to_string()
                    );
                }
                if secret.len() < 16 {
                    return Err("WebhookTrigger secret must be at least 16 characters".to_string());
                }
            }
            TriggerBlock::CameraTrigger { .. } => {
                // Listener exists but no producer emits `privacy-shield-event`
                // yet — the listener would block forever. Hide-list in the
                // frontend keeps this off the palette; this reject is
                // defence-in-depth against hand-edited settings.json.
                return Err(
                    "CameraTrigger is not yet wired — Privacy Shield doesn't emit camera events on the bus this listener reads from. Coming in a later milestone."
                        .to_string(),
                );
            }
            TriggerBlock::ScheduleTrigger { cron_expression } => {
                // Full cron parsing via the `cron` crate. Accepts standard
                // 5-field (m h dom mon dow), 6-field with seconds (s m h
                // dom mon dow), or 7-field with year. Replaces the legacy
                // "interval-only `*/N * * * *`" validator.
                let is_supported = parse_cron_schedule(cron_expression).is_some();
                if !is_supported {
                    return Err(
                        "ScheduleTrigger: invalid cron expression. Examples: `0 */30 * * * *` (every 30 min), `0 0 9 * * *` (9am daily), `0 0 9 * * MON-FRI` (weekdays 9am)."
                            .to_string(),
                    );
                }
            }
            // `ssidChanged` is now supported — see listen_network_event.
            _ => {}
        }
    }

    for action in &flow.actions {
        validate_action(action)?;
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// PREFLIGHT VALIDATOR — non-blocking advisory checks (flows.preflight-validator)
// ═══════════════════════════════════════════════════════════════════════
//
// `validate_flow` above is the hard gate — anything it returns Err on
// blocks save. The preflight validator runs the SAME hard checks but
// continues past the first error, accumulates them, AND adds soft
// warnings the user should see before committing:
//
//   - Errors    — save-blocking. Same set as `validate_flow`.
//   - Warnings  — allow save but flag known caveats:
//                   · CommandAction referencing a command that isn't
//                     in `list_backend_commands()` → likely a typo or
//                     a paid-only command on a free install
//                   · Trigger with `support == Partial` → known
//                     external-state dependency
//                   · ShellAction → reminder about lack of sandbox
//                   · High-risk flow without a `notes` field → no
//                     documentation of WHY this dangerous flow exists
//   - Info      — purely informational; reviewable in the editor:
//                   · High-risk flow last reviewed > 90 days ago
//                   · Flow only emits NotifyActions → useful for
//                     dry-run-style scaffolding but won't DO anything
//
// Frontend calls `preflight_validate_flow` on every edit (debounced)
// and renders a strip above the canvas with grouped issues.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightIssue {
    /// "error" | "warning" | "info" — drives badge colour client-side.
    pub severity: String,
    /// Block identifier the issue is attached to, if any. Format
    /// matches the frontend node-id convention: "trigger[0]",
    /// "condition[1]", "action[3]". `None` means "applies to the
    /// whole flow."
    pub target: Option<String>,
    /// Human-readable message rendered in the UI.
    pub message: String,
    /// Optional stable code so the UI can swap copy without parsing
    /// `message`. Examples: "missing-trigger", "command-unknown",
    /// "support-partial".
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub errors: Vec<PreflightIssue>,
    pub warnings: Vec<PreflightIssue>,
    pub info: Vec<PreflightIssue>,
}

impl PreflightReport {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
        }
    }
    fn err(&mut self, code: &str, target: Option<String>, message: impl Into<String>) {
        self.errors.push(PreflightIssue {
            severity: "error".to_string(),
            target,
            message: message.into(),
            code: code.to_string(),
        });
    }
    fn warn(&mut self, code: &str, target: Option<String>, message: impl Into<String>) {
        self.warnings.push(PreflightIssue {
            severity: "warning".to_string(),
            target,
            message: message.into(),
            code: code.to_string(),
        });
    }
    fn info(&mut self, code: &str, target: Option<String>, message: impl Into<String>) {
        self.info.push(PreflightIssue {
            severity: "info".to_string(),
            target,
            message: message.into(),
            code: code.to_string(),
        });
    }
}

/// Preflight-validate a flow without saving. Returns a report grouped
/// by severity. Frontend renders these as inline alerts in the editor.
#[tauri::command]
pub fn preflight_validate_flow(flow: Flow) -> PreflightReport {
    let mut report = PreflightReport::new();

    // ── Errors — save-blocking ──────────────────────────────────────
    if flow.triggers.is_empty() {
        report.err("no-trigger", None, "Flow must have at least one trigger.");
    }
    if flow.actions.is_empty() {
        report.err("no-action", None, "Flow must have at least one action.");
    }

    for (idx, trigger) in flow.triggers.iter().enumerate() {
        let target = Some(format!("trigger[{}]", idx));
        preflight_check_trigger(trigger, &target, &mut report);
    }
    for (idx, action) in flow.actions.iter().enumerate() {
        let target = Some(format!("action[{}]", idx));
        preflight_check_action(action, &target, &mut report);
    }

    // ── Whole-flow info ─────────────────────────────────────────────
    if flow.risk_level == FlowRiskLevel::High && flow.notes.trim().is_empty() {
        report.warn(
            "high-risk-no-notes",
            None,
            "This flow is classified high-risk but has no notes. Document why it exists.",
        );
    }

    if flow.risk_level == FlowRiskLevel::High && !flow.last_reviewed_at.is_empty() {
        // Parse the timestamp; if older than 90 days, suggest a review.
        if let Ok(reviewed) = chrono::DateTime::parse_from_rfc3339(&flow.last_reviewed_at) {
            let now = chrono::Utc::now();
            let days_since = (now - reviewed.with_timezone(&chrono::Utc)).num_days();
            if days_since > 90 {
                report.info(
                    "review-stale",
                    None,
                    format!(
                        "High-risk flow last reviewed {} days ago — consider re-reading it via Metadata.",
                        days_since
                    ),
                );
            }
        }
    }

    let only_notify = !flow.actions.is_empty()
        && flow
            .actions
            .iter()
            .all(|a| matches!(a, ActionBlock::NotifyAction { .. }));
    if only_notify {
        report.info(
            "notify-only",
            None,
            "All actions are NotifyAction — this flow will only show toasts. Fine for testing; add real actions to do anything else.",
        );
    }

    report
}

/// Check one trigger and append issues to `report`. Mirrors the
/// per-trigger arms of `validate_flow` (errors) AND adds support-class
/// + sanity warnings.
fn preflight_check_trigger(
    trigger: &TriggerBlock,
    target: &Option<String>,
    report: &mut PreflightReport,
) {
    match trigger {
        TriggerBlock::HotkeyTrigger { hotkey } => {
            let has_modifier = ["Ctrl", "Shift", "Alt", "Super"]
                .iter()
                .any(|m| hotkey.contains(m));
            if !has_modifier {
                report.err(
                    "hotkey-no-modifier",
                    target.clone(),
                    "Hotkey must include at least one modifier (Ctrl / Shift / Alt / Super).",
                );
            }
        }
        TriggerBlock::KeySequenceTrigger {
            key,
            taps,
            window_ms,
        } => {
            if key.trim().is_empty() {
                report.err(
                    "keyseq-no-key",
                    target.clone(),
                    "Key sequence trigger requires a key (e.g. \"F12\").",
                );
            }
            if *taps < 2 {
                report.err(
                    "keyseq-bad-taps",
                    target.clone(),
                    "Key sequence must require at least 2 taps.",
                );
            }
            if *window_ms == 0 {
                report.err(
                    "keyseq-bad-window",
                    target.clone(),
                    "Key sequence window must be greater than 0 ms.",
                );
            }
        }
        TriggerBlock::WebhookTrigger { path, secret } => {
            if !path.starts_with('/') {
                report.err(
                    "webhook-bad-path",
                    target.clone(),
                    "Webhook path must start with '/' (e.g. '/my-flow').",
                );
            }
            if secret.len() < 16 {
                report.err(
                    "webhook-secret-short",
                    target.clone(),
                    "Webhook secret must be at least 16 characters.",
                );
            }
            report.warn(
                "support-partial",
                target.clone(),
                "WebhookTrigger requires Pvt Mesh VPN to be running — the listener fails to start otherwise.",
            );
        }
        TriggerBlock::CameraTrigger { .. } => {
            report.err(
                "camera-disabled",
                target.clone(),
                "CameraTrigger is not yet wired — Privacy Shield doesn't emit camera events yet. Coming in a later milestone.",
            );
        }
        TriggerBlock::ScheduleTrigger { cron_expression } => {
            if parse_cron_schedule(cron_expression).is_none() {
                report.err(
                    "cron-invalid",
                    target.clone(),
                    "Invalid cron expression. Examples: `0 */30 * * * *` (every 30 min), `0 0 9 * * MON-FRI` (weekdays 9am).",
                );
            }
        }
        TriggerBlock::SignalReceivedTrigger {} => {
            report.warn(
                "support-partial",
                target.clone(),
                "Signal-received polls the Pvt Mesh VPN drop directory. Brittle if the VPN relocates its inbox.",
            );
        }
        _ => {}
    }
}

fn preflight_check_action(
    action: &ActionBlock,
    target: &Option<String>,
    report: &mut PreflightReport,
) {
    match action {
        ActionBlock::CommandAction { command, .. } => {
            if command.trim().is_empty() {
                report.err(
                    "command-empty",
                    target.clone(),
                    "No command picked — choose one from the action's picker on the canvas, or delete this action.",
                );
            } else {
                // Check if the command exists in the backend registry.
                // Note: paid-only commands may legitimately be absent on
                // a free install — that's reflected as a warning, not
                // an error.
                let known: Vec<String> = backend::list_all_commands()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                if !known.iter().any(|c| c == command) {
                    report.warn(
                        "command-unknown",
                        target.clone(),
                        format!(
                            "Backend doesn't recognise command '{}'. Typo, or paid-only command on a free install.",
                            command
                        ),
                    );
                }
            }
        }
        ActionBlock::HTTPAction { url, .. } => {
            if url.trim().is_empty() {
                report.err("http-no-url", target.clone(), "HTTPAction needs a URL.");
            }
        }
        ActionBlock::NotifyAction { message, .. } => {
            if message.trim().is_empty() {
                report.err(
                    "notify-empty",
                    target.clone(),
                    "NotifyAction needs a message.",
                );
            }
        }
        ActionBlock::ShellAction { script } => {
            if script.trim().is_empty() {
                report.err("shell-empty", target.clone(), "ShellAction needs a script.");
            } else {
                report.warn(
                    "shell-experimental",
                    target.clone(),
                    "ShellAction runs raw PowerShell with your full user privileges. No sandbox. Only run scripts you wrote and audited yourself.",
                );
            }
        }
        ActionBlock::ParallelGroup { actions } => {
            if actions.is_empty() {
                report.err(
                    "parallel-empty",
                    target.clone(),
                    "ParallelGroup needs at least one nested action.",
                );
            } else {
                for nested in actions {
                    preflight_check_action(nested, target, report);
                }
            }
        }
        _ => {}
    }
}

fn validate_action(action: &ActionBlock) -> Result<(), String> {
    match action {
        ActionBlock::CommandAction { command, .. } => {
            if command.trim().is_empty() {
                return Err(
                    "A Run-Command action has no command picked. Open it on the canvas and choose one from the picker, or delete the node entirely."
                        .to_string(),
                );
            }
        }
        ActionBlock::HTTPAction { url, .. } => {
            if url.trim().is_empty() {
                return Err("HTTPAction requires a URL".to_string());
            }
        }
        ActionBlock::NotifyAction { message, .. } => {
            if message.trim().is_empty() {
                return Err("NotifyAction requires a message".to_string());
            }
        }
        ActionBlock::ShellAction { script } => {
            if script.trim().is_empty() {
                return Err("ShellAction requires a script".to_string());
            }
        }
        ActionBlock::ParallelGroup { actions } => {
            if actions.is_empty() {
                return Err("ParallelGroup requires at least one nested action".to_string());
            }
            for nested in actions {
                validate_action(nested)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn json_preview(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string())
}

fn optional_string_preview(value: Option<&String>) -> String {
    value
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .unwrap_or_else(|| "any".to_string())
}

fn describe_usb_filter(
    device_instance_id: Option<&String>,
    device_name: Option<&String>,
) -> String {
    if let Some(device_id) = device_instance_id
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return format!("deviceInstanceId~'{}'", device_id);
    }

    if let Some(name) = device_name
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return format!("deviceName~'{}'", name);
    }

    "any USB device".to_string()
}

/// Parses one `FLOW_TRIGGER|...` line from the USB watcher subprocess.
/// With `delay_seconds > 0` the watcher tags lines with which instance class
/// fired (`FLOW_TRIGGER|<kind>|<id>|<name>`, kind = "insert"/"remove") so a
/// pending delayed fire can be canceled by the opposite event. Without a
/// delay there's nothing to cancel, so the watcher only emits the configured
/// direction's own events untagged (`FLOW_TRIGGER|<id>|<name>`) — kind is
/// then just `mode`. Returns `None` for anything that isn't a trigger line.
fn parse_usb_trigger_line(
    line: &str,
    delay_seconds: u64,
    mode: &str,
) -> Option<(String, String, String)> {
    if !line.starts_with("FLOW_TRIGGER|") {
        return None;
    }
    if delay_seconds > 0 {
        let mut parts = line.splitn(4, '|');
        let _ = parts.next();
        Some((
            parts.next().unwrap_or_default().to_string(),
            parts.next().unwrap_or_default().to_string(),
            parts.next().unwrap_or_default().to_string(),
        ))
    } else {
        let mut parts = line.splitn(3, '|');
        let _ = parts.next();
        Some((
            mode.to_string(),
            parts.next().unwrap_or_default().to_string(),
            parts.next().unwrap_or_default().to_string(),
        ))
    }
}

/// Key used to correlate a device's removal/reinsertion for delay
/// cancellation. Prefers the WMI DeviceID; falls back to the device name if
/// no ID was reported (should not normally happen, but WMI output is
/// best-effort text we don't fully control).
fn usb_device_key(device_id: &str, device_name: &str) -> String {
    if device_id.is_empty() {
        device_name.to_string()
    } else {
        device_id.to_string()
    }
}

fn describe_trigger(trigger: &TriggerBlock) -> String {
    match trigger {
        TriggerBlock::HotkeyTrigger { hotkey } => format!("Hotkey '{}'", hotkey),
        TriggerBlock::KeySequenceTrigger {
            key,
            taps,
            window_ms,
        } => format!("Key sequence '{}' x{} within {}ms", key, taps, window_ms),
        TriggerBlock::USBTrigger {
            mode,
            device_instance_id,
            device_name,
            delay_seconds,
        } => format!(
            "USB {} on {} with {}s delay",
            mode,
            describe_usb_filter(device_instance_id.as_ref(), device_name.as_ref()),
            delay_seconds
        ),
        TriggerBlock::LidCloseTrigger {} => "Lid close event".to_string(),
        TriggerBlock::WebhookTrigger { path, .. } => format!("Webhook path '{}'", path),
        TriggerBlock::CameraTrigger { event } => format!("Privacy Shield event '{}'", event),
        TriggerBlock::ScheduleTrigger { cron_expression } => {
            format!("Schedule '{}'", cron_expression)
        }
        TriggerBlock::NetworkTrigger { event } => format!("Network event '{}'", event),
        TriggerBlock::FileTrigger { path, event } => format!("File '{}' {}", path, event),
        TriggerBlock::ProcessTrigger {
            process_name,
            event,
        } => format!("Process '{}' {}", process_name, event),
        TriggerBlock::SignalReceivedTrigger {} => "Taildrop contingency signal".to_string(),
        TriggerBlock::PasteMonitorTrigger {
            pattern_contains,
            severity,
        } => {
            let filters: Vec<String> = [
                pattern_contains
                    .as_deref()
                    .map(|s| format!("pattern~'{}'", s)),
                severity.as_deref().map(|s| format!("severity={}", s)),
            ]
            .into_iter()
            .flatten()
            .collect();
            if filters.is_empty() {
                "Paste monitor (any)".to_string()
            } else {
                format!("Paste monitor ({})", filters.join(", "))
            }
        }
        TriggerBlock::DecoyMonitorTrigger { path_contains } => match path_contains.as_deref() {
            Some(p) => format!("Decoy access (path~'{}')", p),
            None => "Decoy access (any)".to_string(),
        },
        TriggerBlock::RansomwareMonitorTrigger {} => "Ransomware detected".to_string(),
        TriggerBlock::WifiGuardTrigger { ssid_contains } => match ssid_contains.as_deref() {
            Some(s) => format!("Wi-Fi Guard AP (ssid~'{}')", s),
            None => "Wi-Fi Guard AP (any)".to_string(),
        },
    }
}

fn describe_condition(condition: &ConditionBlock) -> String {
    match condition {
        ConditionBlock::TimeCondition {
            start_hour,
            end_hour,
        } => format!("Time window {:02}:00-{:02}:00", start_hour, end_hour),
        ConditionBlock::SettingCondition {
            path,
            operator,
            value,
        } => format!("Setting '{}' {} {}", path, operator, json_preview(value)),
        ConditionBlock::NetworkCondition { mode, ssid_name } => {
            if mode == "trustedSSID" {
                format!(
                    "Network mode '{}' expected SSID '{}'",
                    mode,
                    optional_string_preview(ssid_name.as_ref())
                )
            } else {
                format!("Network mode '{}'", mode)
            }
        }
        ConditionBlock::BatteryCondition {
            operator,
            percentage,
        } => format!("Battery {} {}", operator, percentage),
        ConditionBlock::USBPresenceCondition {
            device_instance_id,
            device_name,
            mode,
        } => format!(
            "USB presence '{}' on {}",
            mode,
            describe_usb_filter(device_instance_id.as_ref(), device_name.as_ref())
        ),
    }
}

fn describe_action(action: &ActionBlock) -> String {
    match action {
        ActionBlock::CommandAction { command, params } => {
            let param_keys = params
                .as_ref()
                .map(|map| {
                    let mut keys: Vec<String> = map.keys().cloned().collect();
                    keys.sort();
                    keys
                })
                .unwrap_or_default();
            if param_keys.is_empty() {
                format!("Backend command '{}'", command)
            } else {
                format!(
                    "Backend command '{}' with params [{}]",
                    command,
                    param_keys.join(", ")
                )
            }
        }
        ActionBlock::SignalAction {
            target_role,
            signal_type,
        } => format!("Signal '{}' to '{}'", signal_type, target_role),
        ActionBlock::HTTPAction {
            method,
            url,
            headers,
            body,
        } => format!(
            "HTTP {} {} (headers={}, bodyLen={})",
            method,
            url,
            headers.as_ref().map(|map| map.len()).unwrap_or(0),
            body.as_ref().map(|value| value.len()).unwrap_or(0)
        ),
        ActionBlock::NotifyAction {
            message,
            severity,
            duration,
        } => format!(
            "Notify '{}' severity={} duration={}ms",
            message,
            severity,
            duration.unwrap_or(4000)
        ),
        ActionBlock::DelayAction { seconds } => format!("Delay {}s", seconds),
        ActionBlock::ShellAction { script } => {
            format!("Inline PowerShell script ({} chars)", script.len())
        }
        ActionBlock::ParallelGroup { actions } => {
            format!("Parallel group with {} action(s)", actions.len())
        }
    }
}

fn extract_ssid_from_netsh(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.to_ascii_lowercase().starts_with("ssid") || !trimmed.contains(':') {
            return None;
        }
        let mut parts = trimmed.splitn(2, ':');
        let label = parts.next()?.trim().to_ascii_lowercase();
        if label.starts_with("bssid") {
            return None;
        }
        let value = parts.next()?.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

fn queue_trigger_event(
    tx: &mpsc::UnboundedSender<TriggerEvent>,
    flow_id: &str,
    trigger_type: &str,
    detail: impl Into<String>,
) {
    let detail = detail.into();
    flow_engine_log(
        "info",
        Some(flow_id),
        "trigger",
        format!("{} fired: {}", trigger_type, detail),
    );

    if let Err(err) = tx.send(TriggerEvent {
        flow_id: flow_id.to_string(),
        trigger_type: trigger_type.to_string(),
        detail: Some(detail),
    }) {
        flow_engine_log(
            "error",
            Some(flow_id),
            "trigger",
            format!("Failed to queue {} event: {}", trigger_type, err),
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ENGINE INITIALIZATION
// ═══════════════════════════════════════════════════════════════════════

/// Initialize the flow engine. Called from lib.rs setup().
/// Spawns the main event loop and registers listeners for all enabled flows.
pub fn init(app: &AppHandle) {
    init_headless(app);

    ensure_default_flows_persisted();

    // Start listeners for all enabled flows from settings
    start_all_listeners(app);
}

/// Register the in-memory engine and execution loop without persisting default
/// flows or starting ambient trigger listeners. Tauri CLI commands need the
/// managed state, but a read-only command must not arm automations as an
/// incidental process-start side effect.
pub(crate) fn init_headless(app: &AppHandle) {
    if app.try_state::<FlowEngineState>().is_some() {
        return;
    }
    let (tx, rx) = mpsc::unbounded_channel::<TriggerEvent>();

    let state = FlowEngineState {
        tx,
        listeners: Mutex::new(HashMap::new()),
        listener_errors: Mutex::new(HashMap::new()),
        executions: Mutex::new(Vec::new()),
    };

    app.manage(state);

    // Spawn the main event loop that processes trigger events
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        event_loop(app_handle, rx).await;
    });
}

/// Read flows from settings and start listeners for all enabled ones.
fn start_all_listeners(app: &AppHandle) {
    let flows = get_flows_from_settings();
    flow_engine_log(
        "info",
        None,
        "bootstrap",
        format!("Evaluating {} flow(s) for listener startup", flows.len()),
    );
    for flow in &flows {
        if flow.enabled {
            if let Err(err) = validate_flow(flow) {
                flow_engine_log(
                    "error",
                    Some(&flow.id),
                    "bootstrap",
                    format!("Skipping invalid enabled flow '{}': {}", flow.name, err),
                );
                continue;
            }
            start_listener_for_flow(app, flow);
        }
    }
}

/// Read flows from settings.json → app.flows[]
pub(crate) fn get_flows_from_settings() -> Vec<Flow> {
    match settings::read_settings() {
        Ok(s) => merge_default_system_flows(s.app.flows),
        Err(err) => {
            flow_engine_log(
                "warn",
                None,
                "settings",
                format!(
                    "Falling back to built-in default flows because settings could not be read: {}",
                    err
                ),
            );
            default_system_flows()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// TRIGGER LISTENERS — One per active flow
// ═══════════════════════════════════════════════════════════════════════

/// Start a background listener for a specific flow's trigger type.
/// Spawns one task per trigger — any trigger in the array can fire the flow.
fn start_listener_for_flow(app: &AppHandle, flow: &Flow) {
    if let Err(err) = validate_flow(flow) {
        flow_engine_log(
            "error",
            Some(&flow.id),
            "listener",
            format!(
                "Refusing to start listener for invalid flow '{}': {}",
                flow.name, err
            ),
        );
        return;
    }

    let tx = {
        let state = match app.try_state::<FlowEngineState>() {
            Some(s) => s,
            None => return,
        };
        state.tx.clone()
    };

    flow_engine_log(
        "info",
        Some(&flow.id),
        "listener",
        format!(
            "Arming flow '{}' with {} trigger(s), {} condition(s), {} action(s)",
            flow.name,
            flow.triggers.len(),
            flow.conditions.len(),
            flow.actions.len()
        ),
    );

    let mut handles = Vec::new();
    let mut registered_hotkeys = Vec::new();

    for (trigger_index, trigger) in flow.triggers.iter().enumerate() {
        flow_engine_log(
            "info",
            Some(&flow.id),
            "listener",
            format!(
                "Trigger #{} armed: {}",
                trigger_index + 1,
                describe_trigger(trigger)
            ),
        );

        let flow_id = flow.id.clone();
        let trigger = trigger.clone();
        let app_handle = app.clone();
        let tx = tx.clone();

        // Extract hotkey string if this is a hotkey trigger (for cleanup on stop)
        if let TriggerBlock::HotkeyTrigger { ref hotkey } = trigger {
            registered_hotkeys.push(hotkey.clone());
        }

        let task = tauri::async_runtime::spawn(async move {
            match trigger {
                TriggerBlock::HotkeyTrigger { hotkey } => {
                    listen_hotkey(app_handle, flow_id, hotkey, tx).await;
                }
                TriggerBlock::KeySequenceTrigger {
                    key,
                    taps,
                    window_ms,
                } => {
                    listen_key_sequence(app_handle, flow_id, key, taps, window_ms, tx).await;
                }
                TriggerBlock::USBTrigger {
                    mode,
                    device_instance_id,
                    device_name,
                    delay_seconds,
                } => {
                    listen_usb(
                        app_handle,
                        flow_id,
                        mode,
                        device_instance_id,
                        device_name,
                        delay_seconds,
                        tx,
                    )
                    .await;
                }
                TriggerBlock::LidCloseTrigger {} => {
                    listen_lid_close(app_handle, flow_id, tx).await;
                }
                TriggerBlock::SignalReceivedTrigger {} => {
                    listen_signal_received(app_handle, flow_id, tx).await;
                }
                TriggerBlock::CameraTrigger { event } => {
                    listen_camera_event(app_handle, flow_id, event, tx).await;
                }
                TriggerBlock::ProcessTrigger {
                    process_name,
                    event,
                } => {
                    listen_process_event(app_handle, flow_id, process_name, event, tx).await;
                }
                TriggerBlock::FileTrigger { path, event } => {
                    listen_file_event(app_handle, flow_id, path, event, tx).await;
                }
                TriggerBlock::ScheduleTrigger { cron_expression } => {
                    listen_schedule(app_handle, flow_id, cron_expression, tx).await;
                }
                TriggerBlock::NetworkTrigger { event } => {
                    listen_network_event(app_handle, flow_id, event, tx).await;
                }
                TriggerBlock::WebhookTrigger { path, secret } => {
                    listen_webhook(flow_id, path, secret, tx).await;
                }
                TriggerBlock::PasteMonitorTrigger {
                    pattern_contains,
                    severity,
                } => {
                    listen_paste_monitor(app_handle, flow_id, pattern_contains, severity, tx).await;
                }
                TriggerBlock::DecoyMonitorTrigger { path_contains } => {
                    listen_decoy_monitor(app_handle, flow_id, path_contains, tx).await;
                }
                TriggerBlock::RansomwareMonitorTrigger {} => {
                    listen_ransomware_monitor(app_handle, flow_id, tx).await;
                }
                TriggerBlock::WifiGuardTrigger { ssid_contains } => {
                    listen_wifi_guard(app_handle, flow_id, ssid_contains, tx).await;
                }
            }
        });

        handles.push(task);
    }

    // Store all handles so we can cancel on flow edit/delete
    if let Some(state) = app.try_state::<FlowEngineState>() {
        let mut listeners = match state.listeners.lock() {
            Ok(l) => l,
            Err(_) => return,
        };
        listeners.insert(
            flow.id.clone(),
            ActiveListener {
                handles,
                registered_hotkeys,
            },
        );
    }
}

/// Stop a listener for a specific flow.
fn stop_listener(app: &AppHandle, flow_id: &str) {
    let state = match app.try_state::<FlowEngineState>() {
        Some(s) => s,
        None => return,
    };
    let removed = {
        if let Ok(mut listeners) = state.listeners.lock() {
            listeners.remove(flow_id)
        } else {
            None
        }
    };
    if let Some(listener) = removed {
        flow_engine_log(
            "info",
            Some(flow_id),
            "listener",
            "Disarming flow listeners",
        );
        // Unregister all OS-level global shortcuts BEFORE aborting the tasks
        if !listener.registered_hotkeys.is_empty() {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            for hotkey in &listener.registered_hotkeys {
                let _ = app.global_shortcut().unregister(hotkey.as_str());
            }
        }
        for handle in listener.handles {
            handle.abort();
        }
    }
    // Clear any stored listener error for this flow — disabling /
    // re-saving resets the diagnostic. The lock-guard lifetime overlaps
    // `state`'s, so we must explicitly bind + drop it before the function
    // returns and `state` goes out of scope.
    let cleared = match state.listener_errors.lock() {
        Ok(mut errs) => {
            errs.remove(flow_id);
            true
        }
        Err(_) => false,
    };
    let _ = cleared;
}

// ═══════════════════════════════════════════════════════════════════════
// TRIGGER IMPLEMENTATIONS
// ═══════════════════════════════════════════════════════════════════════

/// Hotkey trigger — registers a Tauri global shortcut that fires the flow.
async fn listen_hotkey(
    app: AppHandle,
    flow_id: String,
    hotkey: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Registering global hotkey '{}'", hotkey),
    );

    let flow_id_clone = flow_id.clone();
    let hotkey_clone = hotkey.clone();
    let result = app
        .global_shortcut()
        .on_shortcut(hotkey.as_str(), move |_app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                queue_trigger_event(
                    &tx,
                    &flow_id_clone,
                    "HotkeyTrigger",
                    format!("Global shortcut '{}' pressed", hotkey_clone),
                );
            }
        });

    if let Err(e) = result {
        let msg = format!(
            "Could not register hotkey '{}': {}. Another app is probably already using it — try a different combination.",
            hotkey, e
        );
        flow_engine_log("error", Some(&flow_id), "listener", msg.clone());
        if let Some(state) = app.try_state::<FlowEngineState>() {
            if let Ok(mut errs) = state.listener_errors.lock() {
                errs.insert(flow_id.clone(), msg.clone());
            }
        }
        use tauri::Emitter;
        let _ = app.emit(
            "flow-listener-error",
            serde_json::json!({ "flowId": flow_id, "message": msg }),
        );
        return;
    }

    // Clear any prior error for this flow on successful registration.
    if let Some(state) = app.try_state::<FlowEngineState>() {
        if let Ok(mut errs) = state.listener_errors.lock() {
            errs.remove(&flow_id);
        }
    }

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Hotkey '{}' registered successfully", hotkey),
    );

    // Keep the task alive — the shortcut callback handles firing
    // This task will be aborted when the flow is disabled/deleted
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

/// Key sequence trigger — system-wide keyboard capture via the shared
/// `services::keyboard_hook` service. Was previously a frontend bridge
/// (window keydown → `flow-key-press` Tauri event → this listener) which
/// only fired when WC had foreground focus AND wasn't eaten by the
/// WebView's DevTools shortcut (F12 specifically). The shared hook fixes
/// both bugs — the pre-shipped Contingency flow (F12 ×3) now actually
/// fires regardless of focus or DevTools.
///
/// The `key` config string matches `KeyEvent.key_name` from the shared
/// service (which itself mirrors browser `KeyboardEvent.key`). So:
///   - "F12" = the F12 function key (NOT typing F, 1, 2)
///   - "A".."Z" = letters
///   - "0".."9" = digits
///   - "Escape" / "Tab" / "Enter" / "Backspace" / "ArrowUp" etc. for
///     special keys
///
/// Case-insensitive comparison preserved for backward compat with flows
/// authored before this refactor (saved with "f12" / "F12" both work).
async fn listen_key_sequence(
    _app: AppHandle,
    flow_id: String,
    key: String,
    taps: u32,
    window_ms: u64,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let mut sub = crate::services::keyboard_hook::subscribe();

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!(
            "Listening for key sequence '{}' ×{} within {}ms (system-wide via shared kbd hook)",
            key, taps, window_ms
        ),
    );

    let mut timestamps: Vec<std::time::Instant> = Vec::new();
    let window = std::time::Duration::from_millis(window_ms);

    while let Some(event) = sub.rx.recv().await {
        // Case-insensitive identity match against the configured key
        // name. The hook fires for every keydown system-wide; filter to
        // the one this flow cares about.
        if !event.key_name.eq_ignore_ascii_case(&key) {
            continue;
        }

        let now = std::time::Instant::now();
        timestamps.retain(|t| now.duration_since(*t) <= window);
        timestamps.push(now);

        flow_engine_log(
            "info",
            Some(&flow_id),
            "trigger",
            format!(
                "Key '{}' tap {}/{} within {}ms window",
                key,
                timestamps.len(),
                taps,
                window_ms
            ),
        );

        if timestamps.len() >= taps as usize {
            timestamps.clear();
            queue_trigger_event(
                &tx,
                &flow_id,
                "KeySequenceTrigger",
                format!("Matched '{}' {} time(s) within {}ms", key, taps, window_ms),
            );
        }
    }

    // Channel closed = shared hook was uninstalled (no more subscribers).
    // Listener exits; flow engine will re-spawn on next reload.
    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Key sequence listener for '{}' exited", key),
    );
}

/// USB trigger — spawns a PowerShell WMI event watcher for USB device changes.
/// Watches Win32_PnPEntity (not just DiskDrive) so it can match ANY USB device
/// including mice, dongles, keyboards, etc.
async fn listen_usb(
    _app: AppHandle,
    flow_id: String,
    mode: String,
    device_instance_id: Option<String>,
    device_name: Option<String>,
    delay_seconds: u64,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!(
            "Starting USB watcher for mode='{}' filter={} delay={}s",
            mode,
            describe_usb_filter(device_instance_id.as_ref(), device_name.as_ref()),
            delay_seconds
        ),
    );

    // Build the WMI query based on insert vs remove
    let wmi_event = if mode == "insert" {
        "__InstanceCreationEvent"
    } else {
        "__InstanceDeletionEvent"
    };

    // Build a filter for the specific device if provided
    let device_filter = if let Some(ref id) = device_instance_id {
        format!(
            " AND TargetInstance.DeviceID LIKE '%{}%'",
            id.replace('\'', "''")
        )
    } else {
        String::new()
    };

    let name_check = if let Some(ref name) = device_name {
        format!(
            "if ($devName -notlike '*{}*') {{ $match = $false }}",
            name.replace('\'', "''")
        )
    } else {
        String::new()
    };

    // With a delay configured we need to know about the *opposite* event too
    // (e.g. reinsert while a "remove" delay is pending) so it can cancel the
    // pending fire. Watch both instance classes and tag each line with which
    // one it was; without a delay there's nothing to cancel, so keep the
    // original single-class watcher (cheaper, unchanged behavior).
    let ps_script = if delay_seconds > 0 {
        format!(
            r#"
$query = "SELECT * FROM __InstanceOperationEvent WITHIN 2 WHERE TargetInstance ISA 'Win32_PnPEntity'{device_filter}"
$watcher = New-Object System.Management.ManagementEventWatcher($query)
$watcher.Options.Timeout = [timespan]::MaxValue
while ($true) {{
    try {{
        $evt = $watcher.WaitForNextEvent()
        $cls = $evt.__CLASS
        if ($cls -ne '__InstanceCreationEvent' -and $cls -ne '__InstanceDeletionEvent') {{ continue }}
        $dev = $evt.TargetInstance
        $devId = $dev.DeviceID
        $devName = $dev.Name
        $match = $true
        {name_check}
        if ($match) {{
            $kind = if ($cls -eq '__InstanceCreationEvent') {{ 'insert' }} else {{ 'remove' }}
            Write-Output ("FLOW_TRIGGER|" + $kind + "|" + $devId + "|" + $devName)
            [Console]::Out.Flush()
        }}
    }} catch {{ Start-Sleep -Seconds 1 }}
}}
"#,
            device_filter = device_filter,
            name_check = name_check,
        )
    } else {
        format!(
            r#"
$query = "SELECT * FROM {wmi_event} WITHIN 2 WHERE TargetInstance ISA 'Win32_PnPEntity'{device_filter}"
$watcher = New-Object System.Management.ManagementEventWatcher($query)
$watcher.Options.Timeout = [timespan]::MaxValue
while ($true) {{
    try {{
        $evt = $watcher.WaitForNextEvent()
        $dev = $evt.TargetInstance
        $devId = $dev.DeviceID
        $devName = $dev.Name
        $match = $true
        {name_check}
        if ($match) {{
            Write-Output ("FLOW_TRIGGER|" + $devId + "|" + $devName)
            [Console]::Out.Flush()
        }}
    }} catch {{ Start-Sleep -Seconds 1 }}
}}
"#,
            wmi_event = wmi_event,
            device_filter = device_filter,
            name_check = name_check,
        )
    };

    // Spawn PowerShell subprocess
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.kill_on_drop(true);
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000); // CREATE_NO_WINDOW

    match cmd.spawn() {
        Ok(mut child) => {
            // Write script to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(ps_script.as_bytes()).await;
                drop(stdin);
            }

            // Read stdout line by line for trigger events.
            //
            // Delay-with-cancellation: a pending fire for a device is tracked in
            // `pending_notify` keyed by device id (or name if no id), and the
            // actual delay runs as a future inside `pending_fires` — polled from
            // *this* same task via select! (not a detached spawn), so aborting
            // this listener (flow disabled/edited) cancels any in-flight delay
            // too, same as it did before delay+cancellation existed. If the
            // opposite event shows up for the same device before the delay
            // elapses, its Notify is woken and the fire is skipped — e.g. a USB
            // key that's unplugged and immediately replugged won't trip a
            // "remove" flow's deadman-switch actions.
            if let Some(stdout) = child.stdout.take() {
                use futures::stream::{FuturesUnordered, StreamExt};
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                let mut pending_notify: HashMap<String, Arc<Notify>> = HashMap::new();
                let mut pending_fires = FuturesUnordered::new();
                // Once stdout closes we stop reading it, but keep polling
                // pending_fires until every armed delay resolves — a watcher
                // crash mid-delay must still let the deadman-switch fire
                // (fail-secure) rather than silently dropping it.
                let mut stdout_open = true;

                loop {
                    if !stdout_open && pending_fires.is_empty() {
                        break;
                    }
                    tokio::select! {
                        line = lines.next_line(), if stdout_open => {
                            let line = match line {
                                Ok(Some(line)) => line,
                                _ => {
                                    stdout_open = false;
                                    continue;
                                }
                            };
                            let Some((kind, triggered_device_id, triggered_device_name)) =
                                parse_usb_trigger_line(&line, delay_seconds, &mode)
                            else {
                                continue;
                            };

                            if delay_seconds == 0 {
                                queue_trigger_event(
                                    &tx,
                                    &flow_id,
                                    "USBTrigger",
                                    format!(
                                        "mode='{}' deviceId='{}' deviceName='{}' delay=0s",
                                        mode, triggered_device_id, triggered_device_name
                                    ),
                                );
                                continue;
                            }

                            let device_key = usb_device_key(&triggered_device_id, &triggered_device_name);

                            if kind == mode {
                                // Primary event for this trigger's configured direction —
                                // arm a cancellable delayed fire.
                                let notify = Arc::new(Notify::new());
                                pending_notify.insert(device_key, notify.clone());

                                let flow_id = flow_id.clone();
                                let mode = mode.clone();

                                pending_fires.push(async move {
                                    tokio::select! {
                                        _ = tokio::time::sleep(std::time::Duration::from_secs(delay_seconds)) => {
                                            Some((mode, triggered_device_id, triggered_device_name))
                                        }
                                        _ = notify.notified() => {
                                            flow_engine_log(
                                                "info",
                                                Some(&flow_id),
                                                "trigger",
                                                format!(
                                                    "USB {} delay canceled — device '{}' reappeared within {}s",
                                                    mode, triggered_device_name, delay_seconds
                                                ),
                                            );
                                            None
                                        }
                                    }
                                });
                            } else if let Some(notify) = pending_notify.remove(&device_key) {
                                // Opposite event — cancel any pending fire for this device.
                                notify.notify_one();
                            }
                        }
                        Some(outcome) = pending_fires.next(), if !pending_fires.is_empty() => {
                            if let Some((fired_mode, device_id, device_name)) = outcome {
                                queue_trigger_event(
                                    &tx,
                                    &flow_id,
                                    "USBTrigger",
                                    format!(
                                        "mode='{}' deviceId='{}' deviceName='{}' delay={}s",
                                        fired_mode, device_id, device_name, delay_seconds
                                    ),
                                );
                            }
                        }
                    }
                }
            }

            match child.wait().await {
                Ok(status) => {
                    flow_engine_log(
                        "warn",
                        Some(&flow_id),
                        "listener",
                        format!("USB watcher exited with status {:?}", status.code()),
                    );
                }
                Err(err) => {
                    flow_engine_log(
                        "error",
                        Some(&flow_id),
                        "listener",
                        format!("USB watcher wait failed: {}", err),
                    );
                }
            }
        }
        Err(e) => {
            flow_engine_log(
                "error",
                Some(&flow_id),
                "listener",
                format!("Failed to spawn USB watcher: {}", e),
            );
        }
    }
}

/// Lid close trigger — watches Win32_PowerManagementEvent type 4 (entering suspend).
async fn listen_lid_close(
    _app: AppHandle,
    flow_id: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        "Applying lid-close power policy override and starting watcher",
    );

    // KT: Set lid action to "Do Nothing" so signal can be sent before OS sleeps.
    // powercfg changes are non-destructive and reversible.
    let _ = tokio::process::Command::new("powercfg.exe")
        .args([
            "/SETACVALUEINDEX",
            "SCHEME_CURRENT",
            "SUB_BUTTONS",
            "LIDACTION",
            "0",
        ])
        .creation_flags(0x08000000)
        .output()
        .await;
    let _ = tokio::process::Command::new("powercfg.exe")
        .args([
            "/SETDCVALUEINDEX",
            "SCHEME_CURRENT",
            "SUB_BUTTONS",
            "LIDACTION",
            "0",
        ])
        .creation_flags(0x08000000)
        .output()
        .await;
    let _ = tokio::process::Command::new("powercfg.exe")
        .args(["/SETACTIVE", "SCHEME_CURRENT"])
        .creation_flags(0x08000000)
        .output()
        .await;

    let ps_script = r#"
Register-CimIndicationEvent -Query "SELECT * FROM Win32_PowerManagementEvent WHERE EventType = 4" -SourceIdentifier "LidClose"
while ($true) {
    $event = Wait-Event -SourceIdentifier "LidClose" -Timeout 86400
    if ($event) {
        Remove-Event -EventIdentifier $event.EventIdentifier
        Write-Output "FLOW_TRIGGER|LidClose"
        [Console]::Out.Flush()
    }
}
"#;

    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.kill_on_drop(true);
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000);

    match cmd.spawn() {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(ps_script.as_bytes()).await;
                drop(stdin);
            }

            if let Some(stdout) = child.stdout.take() {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    if line.starts_with("FLOW_TRIGGER|") {
                        queue_trigger_event(
                            &tx,
                            &flow_id,
                            "LidCloseTrigger",
                            "Detected lid-close power management event",
                        );
                    }
                }
            }

            match child.wait().await {
                Ok(status) => {
                    flow_engine_log(
                        "warn",
                        Some(&flow_id),
                        "listener",
                        format!("Lid-close watcher exited with status {:?}", status.code()),
                    );
                }
                Err(err) => {
                    flow_engine_log(
                        "error",
                        Some(&flow_id),
                        "listener",
                        format!("Lid-close watcher wait failed: {}", err),
                    );
                }
            }
        }
        Err(e) => {
            flow_engine_log(
                "error",
                Some(&flow_id),
                "listener",
                format!("Failed to spawn lid-close watcher: {}", e),
            );
        }
    }
}

/// Webhook trigger — register a route on the shared webhook server.
/// The server binds to the Tailscale IPv4 only (refuses to start if
/// Tailscale isn't running) and verifies HMAC-SHA256 signatures on
/// every POST. When a valid signed request arrives at our path, the
/// `fire` closure pushed below queues a TriggerEvent into the engine's
/// channel, exactly like every other trigger listener.
async fn listen_webhook(
    flow_id: String,
    path: String,
    secret: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let flow_id_for_fire = flow_id.clone();
    let path_for_fire = path.clone();
    let tx_for_fire = tx.clone();
    let fire: Box<dyn Fn() + Send + Sync + 'static> = Box::new(move || {
        queue_trigger_event(
            &tx_for_fire,
            &flow_id_for_fire,
            "WebhookTrigger",
            format!("POST {} verified", path_for_fire),
        );
    });

    // `_route_handle` is intentionally unused as a name — its sole
    // purpose is to live in scope. When the listener task aborts (flow
    // disabled), the handle drops, unregisters the route, and the
    // shared webhook server shuts down if no other routes remain.
    let _route_handle = match crate::services::webhook_server::register(path.clone(), secret, fire)
    {
        Ok(h) => h,
        Err(e) => {
            flow_engine_log(
                "error",
                Some(&flow_id),
                "listener",
                format!("Webhook register {} failed: {}", path, e),
            );
            return;
        }
    };

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Webhook route '{}' registered (Tailscale-only)", path),
    );

    // Park forever; the only way out is task abort, which runs Drop.
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

/// `PasteMonitorTrigger` — subscribes to the `paste-monitor-detected`
/// Tauri event already emitted by `paste_monitor.rs`. Optional
/// pattern + severity filters narrow which detections fire this flow.
///
/// Privacy invariant inherited: the upstream event never carries
/// clipboard content. We only see the matched pattern name +
/// severity, so this trigger doesn't leak more than the paste monitor
/// already does.
async fn listen_paste_monitor(
    app: AppHandle,
    flow_id: String,
    pattern_contains: Option<String>,
    severity: Option<String>,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let (relay_tx, mut relay_rx) = mpsc::unbounded_channel::<(String, String)>();

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!(
            "Listening for paste-monitor-detected (pattern~={:?}, severity={:?})",
            pattern_contains, severity
        ),
    );

    let pattern_filter = pattern_contains.as_ref().map(|s| s.to_lowercase());
    let severity_filter = severity.clone();
    let _listener = app.listen("paste-monitor-detected", move |event: tauri::Event| {
        let payload = event.payload();
        let value: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => return,
        };
        let evt_pattern = value
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let evt_severity = value
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(pf) = &pattern_filter {
            if !evt_pattern.to_lowercase().contains(pf) {
                return;
            }
        }
        if let Some(sf) = &severity_filter {
            if !evt_severity.eq_ignore_ascii_case(sf) {
                return;
            }
        }
        let _ = relay_tx.send((evt_pattern, evt_severity));
    });

    while let Some((pattern, severity_evt)) = relay_rx.recv().await {
        queue_trigger_event(
            &tx,
            &flow_id,
            "PasteMonitorTrigger",
            format!("pattern='{}' severity='{}'", pattern, severity_evt),
        );
    }
}

/// `DecoyMonitorTrigger` — subscribes to the `decoy-accessed` event
/// emitted by `decoy_monitor.rs`. Optional `path_contains` filter
/// narrows to a specific decoy file or directory.
async fn listen_decoy_monitor(
    app: AppHandle,
    flow_id: String,
    path_contains: Option<String>,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let (relay_tx, mut relay_rx) = mpsc::unbounded_channel::<(String, String)>();

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Listening for decoy-accessed (path~={:?})", path_contains),
    );

    let path_filter = path_contains.as_ref().map(|s| s.to_lowercase());
    let _listener = app.listen("decoy-accessed", move |event: tauri::Event| {
        let payload = event.payload();
        let value: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => return,
        };
        let evt_path = value
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let evt_kind = value
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(pf) = &path_filter {
            if !evt_path.to_lowercase().contains(pf) {
                return;
            }
        }
        let _ = relay_tx.send((evt_path, evt_kind));
    });

    while let Some((path, kind)) = relay_rx.recv().await {
        queue_trigger_event(
            &tx,
            &flow_id,
            "DecoyMonitorTrigger",
            format!("path='{}' kind='{}'", path, kind),
        );
    }
}

/// `RansomwareMonitorTrigger` — subscribes to the `ransomware-detected`
/// event emitted by `ransomware_monitor.rs`. No filter; the monitor
/// already debounces internally so we just relay.
async fn listen_ransomware_monitor(
    app: AppHandle,
    flow_id: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let (relay_tx, mut relay_rx) = mpsc::unbounded_channel::<(u32, u32)>();

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        "Listening for ransomware-detected",
    );

    let _listener = app.listen("ransomware-detected", move |event: tauri::Event| {
        let payload = event.payload();
        let value: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => return,
        };
        let count = value.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let window_seconds = value
            .get("windowSeconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let _ = relay_tx.send((count, window_seconds));
    });

    while let Some((count, window_seconds)) = relay_rx.recv().await {
        queue_trigger_event(
            &tx,
            &flow_id,
            "RansomwareMonitorTrigger",
            format!("{} files modified in {}s", count, window_seconds),
        );
    }
}

/// `WifiGuardTrigger` listener — subscribes to `wifi-guard-detected`
/// emitted by `wifi_check.rs`. Optional `ssid_contains`
/// substring filter narrows to a specific SSID.
async fn listen_wifi_guard(
    app: AppHandle,
    flow_id: String,
    ssid_contains: Option<String>,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let (relay_tx, mut relay_rx) = mpsc::unbounded_channel::<(String, String, String)>();
    let filter = ssid_contains.map(|s| s.to_lowercase());

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        "Listening for wifi-guard-detected",
    );

    let _listener = app.listen("wifi-guard-detected", move |event: tauri::Event| {
        let payload = event.payload();
        let value: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => return,
        };
        let ssid = value
            .get("ssid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let bssid = value
            .get("bssid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(needle) = &filter {
            if !ssid.to_lowercase().contains(needle) {
                return;
            }
        }
        let _ = relay_tx.send((ssid, bssid, reason));
    });

    while let Some((ssid, bssid, reason)) = relay_rx.recv().await {
        queue_trigger_event(
            &tx,
            &flow_id,
            "WifiGuardTrigger",
            format!("SSID '{}' / BSSID {} ({})", ssid, bssid, reason),
        );
    }
}

/// Listen for contingency signals received from mesh peers.
/// Watches for taildrop file arrivals in the Tailscale inbox directory.
async fn listen_signal_received(
    _app: AppHandle,
    flow_id: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    // KT: Tailscale stores received files in %LOCALAPPDATA%\Tailscale\Taildrop\
    let taildrop_dir = format!(
        "{}\\Tailscale\\Taildrop",
        std::env::var("LOCALAPPDATA").unwrap_or_default()
    );

    let path = std::path::Path::new(&taildrop_dir);
    if !path.exists() {
        flow_engine_log(
            "warn",
            Some(&flow_id),
            "listener",
            format!("Taildrop directory not found yet: {}", taildrop_dir),
        );
        // Keep alive and retry periodically
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            if path.exists() {
                flow_engine_log(
                    "info",
                    Some(&flow_id),
                    "listener",
                    format!("Taildrop directory is now available: {}", taildrop_dir),
                );
                break;
            }
        }
    }

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Watching Taildrop directory '{}'", taildrop_dir),
    );

    // Poll-based file watcher for signal files
    // KT: Using polling instead of notify crate for simplicity in v1.
    // Signal files are named "wc-signal-*.json"
    let mut seen_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        if let Ok(entries) = std::fs::read_dir(&taildrop_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("wc-signal-")
                    && name.ends_with(".json")
                    && !seen_files.contains(&name)
                {
                    seen_files.insert(name.clone());
                    // Read and validate the signal
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Ok(signal) = serde_json::from_str::<serde_json::Value>(&content) {
                            // Basic validation: must have version, type, timestamp
                            if signal.get("v").is_some() && signal.get("t").is_some() {
                                queue_trigger_event(
                                    &tx,
                                    &flow_id,
                                    "SignalReceivedTrigger",
                                    format!(
                                        "file='{}' signalType='{}' timestamp='{}'",
                                        name,
                                        signal
                                            .get("t")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown"),
                                        signal
                                            .get("ts")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown")
                                    ),
                                );
                            } else {
                                flow_engine_log(
                                    "warn",
                                    Some(&flow_id),
                                    "trigger",
                                    format!("Ignoring malformed signal file '{}'", name),
                                );
                            }
                        }
                    }
                    // Clean up the signal file after processing
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Camera trigger — listens for Privacy Shield events on the Tauri event bus.
async fn listen_camera_event(
    app: AppHandle,
    flow_id: String,
    target_event: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let (relay_tx, mut relay_rx) = mpsc::unbounded_channel::<()>();
    let target_event_for_listener = target_event.clone();

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Listening for privacy-shield-event '{}'", target_event),
    );

    let _listener = app.listen("privacy-shield-event", move |event: tauri::Event| {
        let payload = event.payload();
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(evt_type) = data.get("event").and_then(|v| v.as_str()) {
                if evt_type == target_event_for_listener {
                    let _ = relay_tx.send(());
                }
            }
        }
    });

    while let Some(()) = relay_rx.recv().await {
        queue_trigger_event(
            &tx,
            &flow_id,
            "CameraTrigger",
            format!("Received privacy-shield-event '{}'", target_event),
        );
    }
}

/// Process trigger — watches for process start/stop via WMI.
async fn listen_process_event(
    _app: AppHandle,
    flow_id: String,
    process_name: String,
    event_type: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!(
            "Starting process watcher for '{}' event='{}'",
            process_name, event_type
        ),
    );

    let wmi_class = if event_type == "started" {
        "Win32_ProcessStartTrace"
    } else {
        "Win32_ProcessStopTrace"
    };

    let ps_script = format!(
        r#"
$query = "SELECT * FROM {wmi_class} WHERE ProcessName = '{name}'"
Register-CimIndicationEvent -Query $query -SourceIdentifier "ProcWatch"
while ($true) {{
    $event = Wait-Event -SourceIdentifier "ProcWatch" -Timeout 86400
    if ($event) {{
        Remove-Event -EventIdentifier $event.EventIdentifier
        Write-Output "FLOW_TRIGGER|Process"
        [Console]::Out.Flush()
    }}
}}
"#,
        wmi_class = wmi_class,
        name = process_name.replace('\'', "''"),
    );

    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.kill_on_drop(true);
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000);

    match cmd.spawn() {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(ps_script.as_bytes()).await;
                drop(stdin);
            }

            if let Some(stdout) = child.stdout.take() {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    if line.starts_with("FLOW_TRIGGER|") {
                        queue_trigger_event(
                            &tx,
                            &flow_id,
                            "ProcessTrigger",
                            format!("Process '{}' {}", process_name, event_type),
                        );
                    }
                }
            }

            match child.wait().await {
                Ok(status) => {
                    flow_engine_log(
                        "warn",
                        Some(&flow_id),
                        "listener",
                        format!("Process watcher exited with status {:?}", status.code()),
                    );
                }
                Err(err) => {
                    flow_engine_log(
                        "error",
                        Some(&flow_id),
                        "listener",
                        format!("Process watcher wait failed: {}", err),
                    );
                }
            }
        }
        Err(e) => {
            flow_engine_log(
                "error",
                Some(&flow_id),
                "listener",
                format!("Failed to spawn process watcher: {}", e),
            );
        }
    }
}

/// File trigger — watches a file path for changes via the shared
/// `services::fs_watcher` service (which uses `notify` /
/// ReadDirectoryChangesW under the hood). Gives us proper rename From/To
/// pairs instead of the polling-era "file disappeared, treat as rename"
/// approximation.
///
/// We subscribe to the PARENT directory non-recursively (cheap) and
/// filter events to those touching our specific target path. Multiple
/// FileTrigger flows watching files in the same parent dir share one
/// underlying notify watcher thanks to the shared service.
///
/// Rename events carry old + new paths when the rename happens within
/// the watched directory; cross-directory renames manifest as Remove +
/// Create which we coalesce inside a short window for the `renamed`
/// event-type.
async fn listen_file_event(
    _app: AppHandle,
    flow_id: String,
    path: String,
    event_type: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    use notify::{
        event::{ModifyKind, RenameMode},
        EventKind,
    };

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!(
            "Watching file '{}' for '{}' events (fs_watcher)",
            path, event_type
        ),
    );

    let target = std::path::PathBuf::from(&path);
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        // Bare filename — fall back to CWD. Unusual but tolerated.
        _ => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };

    let mut handle = match crate::services::fs_watcher::subscribe(parent.clone(), false) {
        Ok(h) => h,
        Err(e) => {
            flow_engine_log(
                "error",
                Some(&flow_id),
                "listener",
                format!("Failed to subscribe to '{}': {}", parent.display(), e),
            );
            return;
        }
    };

    // For the `renamed` event-type: track a recent Remove of the target
    // path so a following Create within the same parent (cross-dir rename
    // shows as Remove + Create) can be reported as a rename event.
    let mut pending_remove: Option<std::time::Instant> = None;
    let rename_coalesce_window = std::time::Duration::from_millis(500);

    while let Some(event) = handle.rx.recv().await {
        // Only events that touch our specific target path matter. notify
        // reports every change in the watched dir; the filter is here.
        let touches_target = event.paths.iter().any(|p| p == &target);
        // Rename From/To events carry both old + new in `event.paths`.
        // For `renamed` we want events that either originated FROM our
        // target OR resulted in our target. Both forms are interesting.
        let rename_from_or_to_target = matches!(event.kind, EventKind::Modify(ModifyKind::Name(_)))
            && event.paths.iter().any(|p| p == &target);

        if !touches_target && !rename_from_or_to_target {
            continue;
        }

        match event_type.as_str() {
            "deleted" => {
                if matches!(event.kind, EventKind::Remove(_)) && touches_target {
                    queue_trigger_event(
                        &tx,
                        &flow_id,
                        "FileTrigger",
                        format!("File '{}' was deleted", path),
                    );
                }
            }
            "modified" => {
                if matches!(event.kind, EventKind::Modify(ModifyKind::Data(_))) && touches_target {
                    queue_trigger_event(
                        &tx,
                        &flow_id,
                        "FileTrigger",
                        format!("File '{}' was modified", path),
                    );
                }
            }
            "renamed" => {
                match event.kind {
                    // True rename within the watched dir: notify emits
                    // ModifyKind::Name with Both paths attached.
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                        // event.paths is [from, to]
                        let from = event.paths.first().map(|p| p.display().to_string());
                        let to = event.paths.get(1).map(|p| p.display().to_string());
                        queue_trigger_event(
                            &tx,
                            &flow_id,
                            "FileTrigger",
                            format!(
                                "File rename: {} → {}",
                                from.as_deref().unwrap_or("?"),
                                to.as_deref().unwrap_or("?")
                            ),
                        );
                    }
                    // Half-event: the From side of an out-of-dir rename.
                    // Record it; if a matching Create lands within the
                    // coalesce window we'll fire as a rename. If not, the
                    // file is genuinely gone and we DON'T fire (that's a
                    // delete, not a rename).
                    EventKind::Modify(ModifyKind::Name(RenameMode::From))
                    | EventKind::Remove(_)
                        if event.paths.iter().any(|p| p == &target) =>
                    {
                        pending_remove = Some(std::time::Instant::now());
                    }
                    EventKind::Modify(ModifyKind::Name(RenameMode::To)) | EventKind::Create(_) => {
                        if let Some(when) = pending_remove {
                            if std::time::Instant::now().duration_since(when)
                                <= rename_coalesce_window
                            {
                                queue_trigger_event(
                                    &tx,
                                    &flow_id,
                                    "FileTrigger",
                                    format!("File '{}' renamed (cross-dir)", path),
                                );
                            }
                            pending_remove = None;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("File watcher for '{}' exited", path),
    );
}

/// Schedule trigger — fires on cron expressions. Uses the `cron` crate
/// which accepts 5-field (m h dom mon dow), 6-field with seconds, or
/// 7-field with year. Listener computes the next firing time after each
/// fire and sleeps exactly until then — no fixed-interval polling.
async fn listen_schedule(
    _app: AppHandle,
    flow_id: String,
    cron_expression: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let schedule = match parse_cron_schedule(&cron_expression) {
        Some(s) => s,
        None => {
            flow_engine_log(
                "error",
                Some(&flow_id),
                "listener",
                format!(
                    "Schedule '{}' rejected at listener start (unparseable cron)",
                    cron_expression
                ),
            );
            return;
        }
    };

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Starting schedule '{}' (cron)", cron_expression),
    );

    loop {
        // Compute the next firing time. If there are no more events
        // (e.g. an expression with a fixed year that's in the past),
        // exit the listener.
        let next = match schedule.upcoming(chrono::Local).next() {
            Some(t) => t,
            None => {
                flow_engine_log(
                    "info",
                    Some(&flow_id),
                    "listener",
                    format!(
                        "Schedule '{}' has no upcoming events — listener exiting",
                        cron_expression
                    ),
                );
                return;
            }
        };

        let now = chrono::Local::now();
        let wait = (next - now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(1));
        // Cap waits at 1 hour so a manual clock change or DST transition
        // can't strand the listener for days. We re-query upcoming() on
        // each loop iteration.
        let capped = wait.min(std::time::Duration::from_secs(3600));
        tokio::time::sleep(capped).await;

        // Only fire if we've actually reached the scheduled time
        // (handles the case where we slept the 1-hour cap and there's
        // still time to go).
        if chrono::Local::now() >= next {
            queue_trigger_event(
                &tx,
                &flow_id,
                "ScheduleTrigger",
                format!(
                    "Cron '{}' fired at {}",
                    cron_expression,
                    next.format("%Y-%m-%d %H:%M:%S")
                ),
            );
        }
    }
}

/// Parse a cron expression with the `cron` crate, returning a schedule
/// usable by the listener. Returns `None` if the expression is invalid.
/// The crate accepts 5-field, 6-field (with seconds prefix), and 7-field
/// (with year suffix) expressions; we don't restrict here.
fn parse_cron_schedule(expr: &str) -> Option<cron::Schedule> {
    use std::str::FromStr;
    cron::Schedule::from_str(expr.trim()).ok()
}

/// Network trigger — polls network state changes.
///
/// Boolean-state events (`vpnConnected`, `vpnDisconnected`, `internetLost`)
/// flow through `last_state` for edge detection. `ssidChanged` keeps a
/// separate string-typed `last_ssid` because its "did the value change"
/// semantics are about identity, not on/off.
async fn listen_network_event(
    _app: AppHandle,
    flow_id: String,
    event_type: String,
    tx: mpsc::UnboundedSender<TriggerEvent>,
) {
    let mut last_state: Option<bool> = None;
    let mut last_ssid: Option<String> = None;

    flow_engine_log(
        "info",
        Some(&flow_id),
        "listener",
        format!("Polling network state for '{}'", event_type),
    );

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // ── ssidChanged: own loop, fires on every Wi-Fi association change.
        if event_type == "ssidChanged" {
            let current_ssid = read_current_ssid().await;
            // Only fire when we have a previous reading AND the SSID has
            // changed identity. Going from None → Some(x) on startup
            // doesn't fire (treat it as the baseline, not a "change").
            // Going from Some(x) → None (disconnected) DOES fire so flows
            // can react to losing Wi-Fi.
            match (&last_ssid, &current_ssid) {
                (Some(prev), Some(curr)) if prev != curr => {
                    queue_trigger_event(
                        &tx,
                        &flow_id,
                        "NetworkTrigger",
                        format!("event='ssidChanged' from='{}' to='{}'", prev, curr),
                    );
                }
                (Some(prev), None) => {
                    queue_trigger_event(
                        &tx,
                        &flow_id,
                        "NetworkTrigger",
                        format!("event='ssidChanged' from='{}' to=<disconnected>", prev),
                    );
                }
                _ => {}
            }
            last_ssid = current_ssid;
            continue;
        }

        let current_state = match event_type.as_str() {
            "vpnConnected" | "vpnDisconnected" => {
                // Check Tailscale status
                let output = tokio::process::Command::new("tailscale.exe")
                    .args(["status", "--json"])
                    .creation_flags(0x08000000)
                    .output()
                    .await;

                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                            Some(
                                json.get("BackendState").and_then(|v| v.as_str())
                                    == Some("Running"),
                            )
                        } else {
                            Some(false)
                        }
                    }
                    Err(_) => Some(false),
                }
            }
            "internetLost" => {
                // Simple ping check
                let output = tokio::process::Command::new("ping.exe")
                    .args(["-n", "1", "-w", "2000", "1.1.1.1"])
                    .creation_flags(0x08000000)
                    .output()
                    .await;
                Some(output.map(|o| o.status.success()).unwrap_or(false))
            }
            _ => None,
        };

        if let (Some(prev), Some(curr)) = (last_state, current_state) {
            let should_fire = match event_type.as_str() {
                "vpnConnected" => !prev && curr,
                "vpnDisconnected" => prev && !curr,
                "internetLost" => prev && !curr,
                _ => false,
            };

            if should_fire {
                queue_trigger_event(
                    &tx,
                    &flow_id,
                    "NetworkTrigger",
                    format!(
                        "event='{}' previousState={} currentState={}",
                        event_type, prev, curr
                    ),
                );
            }
        }

        last_state = current_state;
    }
}

/// Read the currently-associated Wi-Fi SSID via `netsh wlan show interfaces`.
/// Returns `None` if no Wi-Fi adapter is connected (or the SSID can't be
/// parsed from the netsh output — which happens on machines without Wi-Fi
/// hardware too, treated as "no SSID").
///
/// We parse the human-readable text rather than a structured API because
/// Windows doesn't expose WlanGetProfileList in a way reachable from pure
/// Rust without a wlanapi binding crate. The output format is stable
/// across Win10/11 and locale-independent for the `SSID` label specifically
/// (the field name itself stays "SSID" in localised builds).
async fn read_current_ssid() -> Option<String> {
    let output = tokio::process::Command::new("netsh.exe")
        .args(["wlan", "show", "interfaces"])
        .creation_flags(0x08000000)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // netsh produces UTF-16-ish output on Win11 even for ASCII content;
    // String::from_utf8_lossy handles this gracefully (preserves ASCII,
    // garbage non-ASCII but we only need ASCII labels here).
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Find the FIRST `SSID` line that isn't a `BSSID` line. There are
    // usually multiple interfaces shown; we take the first associated
    // one. Lines look like: `    SSID                   : MyNetwork`
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        // Skip BSSID — has the same prefix
        if trimmed.starts_with("BSSID") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("SSID") {
            // Format is "SSID<spaces>: value"
            if let Some(colon_idx) = rest.find(':') {
                let value = rest[colon_idx + 1..].trim();
                if value.is_empty() {
                    return None;
                }
                return Some(value.to_string());
            }
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════
// EVENT LOOP — Processes trigger events, evaluates conditions, runs actions
// ═══════════════════════════════════════════════════════════════════════

async fn event_loop(app: AppHandle, mut rx: mpsc::UnboundedReceiver<TriggerEvent>) {
    while let Some(event) = rx.recv().await {
        flow_engine_log(
            "info",
            Some(&event.flow_id),
            "trigger",
            format!(
                "Dispatching queued {} trigger{}",
                event.trigger_type,
                event
                    .detail
                    .as_ref()
                    .map(|detail| format!(" ({})", detail))
                    .unwrap_or_default()
            ),
        );
        let app_clone = app.clone();
        // Spawn each flow execution as a separate task so they don't block each other.
        // Triggered (non-manual) executions are NEVER dry-run — dry-run is operator-opt-in.
        tauri::async_runtime::spawn(async move {
            execute_flow(
                &app_clone,
                &event.flow_id,
                &event.trigger_type,
                event.detail.as_deref(),
                false,
            )
            .await;
        });
    }
}

/// Execute a flow: evaluate conditions → run actions sequentially.
///
/// `dry_run = true` short-circuits every action that has external
/// side-effects (commands, shell, http, signals). NotifyAction still
/// fires (it's the operator's "did the path actually reach me" signal)
/// and DelayAction still waits so the operator sees realistic timing.
async fn execute_flow(
    app: &AppHandle,
    flow_id: &str,
    trigger_type: &str,
    trigger_detail: Option<&str>,
    dry_run: bool,
) {
    let flows = get_flows_from_settings();
    // Dry-runs are exempt from the enabled-gate — the whole point is
    // "let me see what would happen BEFORE I arm this". Live triggers
    // still require the flow to be enabled.
    let flow = match flows
        .iter()
        .find(|f| f.id == flow_id && (dry_run || f.enabled))
    {
        Some(f) => f.clone(),
        None => {
            flow_engine_log(
                "warn",
                Some(flow_id),
                "execution",
                format!(
                    "Ignoring execution request for disabled or missing flow via trigger '{}'{}",
                    trigger_type,
                    trigger_detail
                        .map(|detail| format!(" ({})", detail))
                        .unwrap_or_default()
                ),
            );
            return;
        }
    };

    let started_at = chrono::Utc::now().to_rfc3339();
    let start_instant = std::time::Instant::now();
    let mut events: Vec<FlowExecutionEvent> = Vec::new();
    let mut steps: Vec<FlowStepResult> = Vec::new();

    push_execution_event(
        &mut events,
        flow_id,
        trigger_type,
        "info",
        "trigger",
        match trigger_detail {
            Some(detail) => format!("Received {}: {}", trigger_type, detail),
            None => format!("Received {}", trigger_type),
        },
    );

    push_execution_event(
        &mut events,
        flow_id,
        trigger_type,
        "info",
        "start",
        format!(
            "Flow '{}' started with {} trigger(s), {} condition(s), {} action(s)",
            flow.name,
            flow.triggers.len(),
            flow.conditions.len(),
            flow.actions.len()
        ),
    );

    // ── Evaluate conditions (ALL must pass) ──
    if flow.conditions.is_empty() {
        push_execution_event(
            &mut events,
            flow_id,
            trigger_type,
            "info",
            "condition",
            "No conditions configured; proceeding directly to actions",
        );
    }
    for (index, condition) in flow.conditions.iter().enumerate() {
        let condition_type = get_condition_type_name(condition);
        push_execution_event(
            &mut events,
            flow_id,
            trigger_type,
            "info",
            "condition",
            format!(
                "Evaluating condition #{} ({}): {}",
                index + 1,
                condition_type,
                describe_condition(condition)
            ),
        );

        let evaluation = evaluate_condition(app, condition).await;

        if !evaluation.passed {
            push_execution_event(
                &mut events,
                flow_id,
                trigger_type,
                "warn",
                "condition",
                format!(
                    "Condition #{} ({}) failed; {}",
                    index + 1,
                    condition_type,
                    evaluation.detail
                ),
            );
            // Condition failed — log and abort
            let execution = FlowExecution {
                flow_id: flow_id.to_string(),
                trigger_type: trigger_type.to_string(),
                started_at,
                events,
                steps: vec![],
                completed: false,
                total_duration_ms: start_instant.elapsed().as_millis() as u64,
                dry_run,
            };
            let _ = app.emit(
                "flow-executed",
                serde_json::to_value(&execution).unwrap_or_default(),
            );
            log_execution(app, execution);
            return;
        }

        push_execution_event(
            &mut events,
            flow_id,
            trigger_type,
            "info",
            "condition",
            format!(
                "Condition #{} ({}) passed; {}",
                index + 1,
                condition_type,
                evaluation.detail
            ),
        );
    }

    // ── Execute actions sequentially ──
    let mut all_success = true;
    for (i, action) in flow.actions.iter().enumerate() {
        let action_start = std::time::Instant::now();
        let action_type = get_action_type_name(action);

        push_execution_event(
            &mut events,
            flow_id,
            trigger_type,
            "info",
            "action",
            format!(
                "{}Starting action #{} ({}): {}",
                if dry_run { "[DRY RUN] " } else { "" },
                i + 1,
                action_type,
                describe_action(action)
            ),
        );

        let result = execute_action(app, action, dry_run).await;
        let duration_ms = action_start.elapsed().as_millis() as u64;
        let error_message = result.as_ref().err().cloned();

        let step = FlowStepResult {
            action_index: i,
            action_type: action_type.clone(),
            success: result.is_ok(),
            error: error_message.clone(),
            duration_ms,
            dry_run,
        };

        if !step.success {
            all_success = false;
        }

        if let Some(ref error) = error_message {
            push_execution_event(
                &mut events,
                flow_id,
                trigger_type,
                "error",
                "action",
                format!(
                    "Action #{} ({}) failed after {}ms: {} [{}]",
                    i + 1,
                    action_type,
                    duration_ms,
                    error,
                    describe_action(action)
                ),
            );
        } else {
            push_execution_event(
                &mut events,
                flow_id,
                trigger_type,
                "info",
                "action",
                format!(
                    "Action #{} ({}) completed in {}ms: {}",
                    i + 1,
                    action_type,
                    duration_ms,
                    describe_action(action)
                ),
            );
        }

        steps.push(step);

        // Don't continue if a critical action fails
        if !all_success {
            break;
        }
    }

    push_execution_event(
        &mut events,
        flow_id,
        trigger_type,
        if all_success { "info" } else { "error" },
        "finish",
        if all_success {
            format!(
                "Flow completed successfully in {}ms",
                start_instant.elapsed().as_millis()
            )
        } else {
            format!(
                "Flow stopped after action failure in {}ms",
                start_instant.elapsed().as_millis()
            )
        },
    );

    let execution = FlowExecution {
        flow_id: flow_id.to_string(),
        trigger_type: trigger_type.to_string(),
        started_at,
        events,
        steps,
        completed: all_success,
        total_duration_ms: start_instant.elapsed().as_millis() as u64,
        dry_run,
    };

    // Emit event for frontend execution log
    let _ = app.emit(
        "flow-executed",
        serde_json::to_value(&execution).unwrap_or_default(),
    );

    log_execution(app, execution);
}

// ═══════════════════════════════════════════════════════════════════════
// CONDITION EVALUATOR
// ═══════════════════════════════════════════════════════════════════════

async fn evaluate_condition(_app: &AppHandle, condition: &ConditionBlock) -> ConditionEvaluation {
    match condition {
        ConditionBlock::TimeCondition {
            start_hour,
            end_hour,
        } => {
            let now = chrono::Local::now().hour();
            let passed = if start_hour <= end_hour {
                now >= *start_hour && now < *end_hour
            } else {
                // Wraps midnight, e.g. 22:00-06:00
                now >= *start_hour || now < *end_hour
            };

            ConditionEvaluation {
                passed,
                detail: format!(
                    "localHour={:02} window={:02}:00-{:02}:00",
                    now, start_hour, end_hour
                ),
            }
        }

        ConditionBlock::SettingCondition {
            path,
            operator,
            value,
        } => {
            match settings::read_settings() {
                Ok(settings_obj) => {
                    // Serialize to JSON for flexible nested path lookup
                    let settings_json = serde_json::to_value(&settings_obj).unwrap_or_default();
                    let actual = get_nested_value(&settings_json, path);
                    let passed = match operator.as_str() {
                        "==" => actual == *value,
                        "!=" => actual != *value,
                        _ => false,
                    };

                    ConditionEvaluation {
                        passed,
                        detail: format!(
                            "path='{}' actual={} operator='{}' expected={}",
                            path,
                            json_preview(&actual),
                            operator,
                            json_preview(value)
                        ),
                    }
                }
                Err(err) => ConditionEvaluation {
                    passed: false,
                    detail: format!("failed to read settings: {}", err),
                },
            }
        }

        ConditionBlock::NetworkCondition { mode, ssid_name } => match mode.as_str() {
            "tailscaleConnected" => {
                let output = tokio::process::Command::new("tailscale.exe")
                    .args(["status", "--json"])
                    .creation_flags(0x08000000)
                    .output()
                    .await;
                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        match serde_json::from_str::<serde_json::Value>(&stdout) {
                            Ok(json) => {
                                let backend_state = json
                                    .get("BackendState")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                ConditionEvaluation {
                                    passed: backend_state == "Running",
                                    detail: format!(
                                        "mode='{}' backendState='{}'",
                                        mode, backend_state
                                    ),
                                }
                            }
                            Err(err) => ConditionEvaluation {
                                passed: false,
                                detail: format!("failed to parse tailscale status JSON: {}", err),
                            },
                        }
                    }
                    Err(err) => ConditionEvaluation {
                        passed: false,
                        detail: format!("tailscale status command failed: {}", err),
                    },
                }
            }
            "trustedSSID" => {
                let output = tokio::process::Command::new("netsh.exe")
                    .args(["wlan", "show", "interfaces"])
                    .creation_flags(0x08000000)
                    .output()
                    .await;
                match output {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let actual_ssid = extract_ssid_from_netsh(&stdout);
                        let expected_ssid = optional_string_preview(ssid_name.as_ref());
                        ConditionEvaluation {
                            passed: ssid_name
                                .as_ref()
                                .map(|ssid| stdout.contains(ssid))
                                .unwrap_or(false),
                            detail: format!(
                                "mode='{}' actualSsid='{}' expectedSsid='{}'",
                                mode,
                                actual_ssid.unwrap_or_else(|| "unknown".to_string()),
                                expected_ssid
                            ),
                        }
                    }
                    Err(err) => ConditionEvaluation {
                        passed: false,
                        detail: format!("netsh wlan show interfaces failed: {}", err),
                    },
                }
            }
            _ => ConditionEvaluation {
                passed: false,
                detail: format!("unsupported network condition mode '{}'", mode),
            },
        },

        ConditionBlock::BatteryCondition {
            operator,
            percentage,
        } => {
            let ps = r#"(Get-CimInstance -ClassName Win32_Battery).EstimatedChargeRemaining"#;
            let output = tokio::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", ps])
                .creation_flags(0x08000000)
                .output()
                .await;
            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let level: u32 = stdout.trim().parse().unwrap_or(100);
                    let passed = match operator.as_str() {
                        "<" => level < *percentage,
                        ">" => level > *percentage,
                        "<=" => level <= *percentage,
                        ">=" => level >= *percentage,
                        _ => false,
                    };
                    ConditionEvaluation {
                        passed,
                        detail: format!(
                            "battery={} operator='{}' threshold={}",
                            level, operator, percentage
                        ),
                    }
                }
                Err(err) => ConditionEvaluation {
                    passed: false,
                    detail: format!("battery check failed: {}", err),
                },
            }
        }

        ConditionBlock::USBPresenceCondition {
            device_instance_id,
            device_name,
            mode,
        } => {
            let ps = if let Some(ref id) = device_instance_id {
                format!(
                    "Get-PnpDevice | Where-Object {{ $_.InstanceId -like '*{}*' -and $_.Status -eq 'OK' }} | Measure-Object | Select-Object -ExpandProperty Count",
                    id.replace('\'', "''")
                )
            } else if let Some(ref name) = device_name {
                format!(
                    "Get-PnpDevice | Where-Object {{ $_.FriendlyName -like '*{}*' -and $_.Status -eq 'OK' }} | Measure-Object | Select-Object -ExpandProperty Count",
                    name.replace('\'', "''")
                )
            } else {
                return ConditionEvaluation {
                    passed: false,
                    detail: "USB presence condition requires deviceInstanceId or deviceName"
                        .to_string(),
                };
            };

            let output = tokio::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &ps])
                .creation_flags(0x08000000)
                .output()
                .await;

            match output {
                Ok(out) => {
                    let count: u32 = String::from_utf8_lossy(&out.stdout)
                        .trim()
                        .parse()
                        .unwrap_or(0);
                    let passed = match mode.as_str() {
                        "present" => count > 0,
                        "absent" => count == 0,
                        _ => false,
                    };
                    ConditionEvaluation {
                        passed,
                        detail: format!(
                            "mode='{}' matchedDevices={} filter={}",
                            mode,
                            count,
                            describe_usb_filter(device_instance_id.as_ref(), device_name.as_ref())
                        ),
                    }
                }
                Err(err) => ConditionEvaluation {
                    passed: false,
                    detail: format!("USB presence check failed: {}", err),
                },
            }
        }
    }
}

/// Navigate a JSON value by dot-path (e.g. "current.privacy.telemetry.windowsDisabled")
fn get_nested_value(json: &serde_json::Value, path: &str) -> serde_json::Value {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = json;
    for part in parts {
        match current.get(part) {
            Some(val) => current = val,
            None => return serde_json::Value::Null,
        }
    }
    current.clone()
}

use chrono::Timelike;

// ═══════════════════════════════════════════════════════════════════════
// ACTION EXECUTOR
// ═══════════════════════════════════════════════════════════════════════

async fn execute_action(
    app: &AppHandle,
    action: &ActionBlock,
    dry_run: bool,
) -> Result<(), String> {
    match action {
        ActionBlock::CommandAction { command, params } => {
            if dry_run {
                // Skip the real backend invocation; the action_event log
                // line already records the command + params for review.
                return Ok(());
            }
            let params_map = params.clone().unwrap_or_default();
            backend::run_backend_script(app.clone(), command.clone(), params_map)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }

        ActionBlock::SignalAction {
            target_role,
            signal_type,
        } => {
            if dry_run {
                // Signals are destructive (alert other admins!) — never
                // emit them in dry-run mode.
                return Ok(());
            }
            // Send contingency signal via the existing PS command
            let mut params = HashMap::new();
            params.insert("TargetRole".to_string(), target_role.clone());
            params.insert("SignalType".to_string(), signal_type.clone());
            backend::run_backend_script(app.clone(), "Send-ContingencySignal".to_string(), params)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }

        ActionBlock::HTTPAction {
            method,
            url,
            headers,
            body,
        } => {
            if dry_run {
                // Outbound HTTP can be destructive (POST to /panic/erase…)
                // — never fire in dry-run.
                let _ = (headers, body);
                return Ok(());
            }
            // Raw arbitrary-URL HTTP is a paid, non-in-app action type; gate real
            // execution on the entitlement (same reasoning as ShellAction above).
            crate::license::require_paid("flows")?;
            let client = reqwest::Client::new();
            let mut request = match method.as_str() {
                "GET" => client.get(url),
                "POST" => client.post(url),
                "PUT" => client.put(url),
                "DELETE" => client.delete(url),
                _ => return Err(format!("Unknown HTTP method: {}", method)),
            };

            if let Some(hdrs) = headers {
                for (k, v) in hdrs {
                    request = request.header(k.as_str(), v.as_str());
                }
            }

            if let Some(body_str) = body {
                request = request.body(body_str.clone());
            }

            request
                .send()
                .await
                .map(|_| ())
                .map_err(|e| format!("HTTP request failed: {}", e))
        }

        ActionBlock::NotifyAction {
            message,
            severity,
            duration,
        } => {
            // Notifications run even in dry-run — they're the operator's
            // "did the path reach me?" signal and have no destructive
            // side effects. We prefix the message with [DRY RUN] so the
            // toast is recognisable.
            let display = if dry_run {
                format!("[DRY RUN] {}", message)
            } else {
                message.clone()
            };
            let _ = app.emit(
                "flow-notify",
                serde_json::json!({
                    "message": display,
                    "severity": severity,
                    "duration": duration.unwrap_or(4000),
                }),
            );
            Ok(())
        }

        ActionBlock::DelayAction { seconds } => {
            // Delays run unconditionally — dry-run includes timing so
            // the operator gets a realistic picture of how long the
            // flow takes end-to-end.
            tokio::time::sleep(std::time::Duration::from_secs(*seconds)).await;
            Ok(())
        }

        ActionBlock::ShellAction { script } => {
            if dry_run {
                let _ = script;
                return Ok(());
            }
            // Flows is a paid feature and raw PowerShell is the single highest-risk
            // action type. Gate REAL execution on the entitlement so no Free build,
            // no flow loaded from settings at startup, and no imported bundle can ever
            // run arbitrary shell without a license. Mirrors flow_bridge's
            // require_paid("flows") on the Flows-v2 engine of record.
            crate::license::require_paid("flows")?;
            let mut command = tokio::process::Command::new("powershell.exe");
            command
                .kill_on_drop(true)
                .args(["-NoProfile", "-NonInteractive", "-Command", "-"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .creation_flags(0x08000000);
            let output = command.spawn();

            match output {
                Ok(mut child) => {
                    if let Some(mut stdin) = child.stdin.take() {
                        use tokio::io::AsyncWriteExt;
                        let _ = stdin.write_all(script.as_bytes()).await;
                        drop(stdin);
                    }
                    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
                    if out.status.success() {
                        Ok(())
                    } else {
                        Err(String::from_utf8_lossy(&out.stderr).to_string())
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }

        ActionBlock::ParallelGroup { actions } => {
            let futs: Vec<_> = actions
                .iter()
                .map(|a| execute_action(app, a, dry_run))
                .collect();
            let results = futures::future::join_all(futs).await;
            let errors: Vec<String> = results.into_iter().filter_map(|r| r.err()).collect();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "Parallel group had {} failure(s): {}",
                    errors.len(),
                    errors.join("; ")
                ))
            }
        }
    }
}

fn get_action_type_name(action: &ActionBlock) -> String {
    match action {
        ActionBlock::CommandAction { .. } => "CommandAction".to_string(),
        ActionBlock::SignalAction { .. } => "SignalAction".to_string(),
        ActionBlock::HTTPAction { .. } => "HTTPAction".to_string(),
        ActionBlock::NotifyAction { .. } => "NotifyAction".to_string(),
        ActionBlock::DelayAction { .. } => "DelayAction".to_string(),
        ActionBlock::ShellAction { .. } => "ShellAction".to_string(),
        ActionBlock::ParallelGroup { .. } => "ParallelGroup".to_string(),
    }
}

fn get_condition_type_name(condition: &ConditionBlock) -> String {
    match condition {
        ConditionBlock::TimeCondition { .. } => "TimeCondition".to_string(),
        ConditionBlock::SettingCondition { .. } => "SettingCondition".to_string(),
        ConditionBlock::NetworkCondition { .. } => "NetworkCondition".to_string(),
        ConditionBlock::BatteryCondition { .. } => "BatteryCondition".to_string(),
        ConditionBlock::USBPresenceCondition { .. } => "USBPresenceCondition".to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// EXECUTION LOG
// ═══════════════════════════════════════════════════════════════════════

fn log_execution(app: &AppHandle, execution: FlowExecution) {
    let state = match app.try_state::<FlowEngineState>() {
        Some(s) => s,
        None => return,
    };
    let mut log = match state.executions.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    log.push(execution);
    // Ring buffer: keep only the last N entries
    if log.len() > MAX_EXECUTIONS {
        let drain_count = log.len() - MAX_EXECUTIONS;
        log.drain(0..drain_count);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI COMMANDS — Exposed to the frontend
// ═══════════════════════════════════════════════════════════════════════

/// Get all flows from settings.
#[tauri::command]
pub fn get_flows() -> Result<Vec<Flow>, String> {
    Ok(get_flows_from_settings())
}

/// Create or update a flow. Restarts the trigger listener if changed.
#[tauri::command]
pub async fn save_flow(app: AppHandle, flow: Flow) -> Result<(), String> {
    crate::license::require_paid("flows")?;
    validate_flow(&flow)?;

    flow_engine_log(
        "info",
        Some(&flow.id),
        "settings",
        format!(
            "Saving flow '{}' (enabled={}, system={}, triggers={}, conditions={}, actions={})",
            flow.name,
            flow.enabled,
            flow.system,
            flow.triggers.len(),
            flow.conditions.len(),
            flow.actions.len()
        ),
    );

    // Validate: system flows can't be deleted but can be edited
    let mut flows = get_flows_from_settings();

    // Check if this is an update or a new flow
    if let Some(existing) = flows.iter_mut().find(|f| f.id == flow.id) {
        // Update: system flows can change everything except id and system flag
        if existing.system && !flow.system {
            return Err("Cannot convert a system flow to a user flow".to_string());
        }
        *existing = flow.clone();
    } else {
        flows.push(flow.clone());
    }

    persist_flows_to_settings(&flows)?;

    // Restart the listener for this flow
    stop_listener(&app, &flow.id);
    if flow.enabled {
        start_listener_for_flow(&app, &flow);
    }

    flow_engine_log(
        "info",
        Some(&flow.id),
        "settings",
        "Flow saved and listeners refreshed",
    );

    Ok(())
}

/// Delete a flow. Only user flows can be deleted.
#[tauri::command]
pub async fn delete_flow(app: AppHandle, flow_id: String) -> Result<(), String> {
    crate::license::require_paid("flows")?;
    flow_engine_log("info", Some(&flow_id), "settings", "Deleting flow");

    let mut flows = get_flows_from_settings();

    // Check if it's a system flow
    if let Some(flow) = flows.iter().find(|f| f.id == flow_id) {
        if flow.system {
            return Err("Cannot delete a system flow".to_string());
        }
    }

    flows.retain(|f| f.id != flow_id);

    persist_flows_to_settings(&flows)?;

    // Stop the listener
    stop_listener(&app, &flow_id);

    flow_engine_log("info", Some(&flow_id), "settings", "Flow deleted");

    Ok(())
}

/// Toggle a flow's enabled state.
#[tauri::command]
pub async fn toggle_flow(app: AppHandle, flow_id: String, enabled: bool) -> Result<(), String> {
    crate::license::require_paid("flows")?;
    flow_engine_log(
        "info",
        Some(&flow_id),
        "settings",
        format!("Toggling flow to enabled={}", enabled),
    );

    let mut flows = get_flows_from_settings();

    if let Some(flow) = flows.iter_mut().find(|f| f.id == flow_id) {
        if enabled {
            validate_flow(flow)?;
        }
        flow.enabled = enabled;
    } else {
        return Err(format!("Flow not found: {}", flow_id));
    }

    persist_flows_to_settings(&flows)?;

    // Start or stop the listener
    stop_listener(&app, &flow_id);
    if enabled {
        let flow = flows
            .iter()
            .find(|f| f.id == flow_id)
            .cloned()
            .ok_or_else(|| format!("Flow not found after update: {}", flow_id))?;
        start_listener_for_flow(&app, &flow);
    }

    flow_engine_log(
        "info",
        Some(&flow_id),
        "settings",
        format!("Flow toggle completed; enabled={}", enabled),
    );

    Ok(())
}

/// Get the execution log (last N runs).
#[tauri::command]
pub fn get_flow_executions(app: AppHandle) -> Result<Vec<FlowExecution>, String> {
    let state = app
        .try_state::<FlowEngineState>()
        .ok_or("Flow engine not initialized")?;

    let log = state.executions.lock().map_err(|e| e.to_string())?;

    Ok(log.iter().rev().cloned().collect())
}

/// Internal helper for flow_health — snapshot executions without the
/// per-call `AppHandle` overhead and without the reverse (newest-first
/// is computed at the caller).
pub(crate) fn snapshot_executions(state: &FlowEngineState) -> Vec<FlowExecution> {
    state
        .executions
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default()
}

/// Internal helper for flow_health — return the flow ids currently
/// listening. Cheap; just clones the map's keys.
pub(crate) fn snapshot_listener_ids(state: &FlowEngineState) -> Vec<String> {
    state
        .listeners
        .lock()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Internal helper for flow_health — return the per-flow listener-startup
/// error map (e.g. "hotkey conflict", "AV blocked the keyboard hook").
pub(crate) fn snapshot_listener_errors(state: &FlowEngineState) -> HashMap<String, String> {
    state
        .listener_errors
        .lock()
        .map(|m| m.clone())
        .unwrap_or_default()
}

/// Manually fire a flow (for testing or the "Contingency Now" button).
#[tauri::command]
pub async fn fire_flow(
    app: AppHandle,
    flow_id: String,
    dry_run: Option<bool>,
) -> Result<(), String> {
    crate::license::require_paid("flows")?;
    let dry_run = dry_run.unwrap_or(false);
    flow_engine_log(
        "info",
        Some(&flow_id),
        "manual",
        if dry_run {
            "Manual flow DRY RUN requested"
        } else {
            "Manual flow execution requested"
        },
    );
    let detail = if dry_run {
        "Manual DRY RUN from UI — irreversible actions are skipped"
    } else {
        "Manual flow execution requested from UI"
    };
    execute_flow(
        &app,
        &flow_id,
        if dry_run {
            "ManualDryRun"
        } else {
            "ManualTrigger"
        },
        Some(detail),
        dry_run,
    )
    .await;
    Ok(())
}

/// List available USB devices for enrollment in trigger config.
#[tauri::command]
pub async fn list_usb_devices() -> Result<serde_json::Value, String> {
    let ps = r#"
Get-PnpDevice -Class 'DiskDrive','HIDClass','USB','Bluetooth','Net' -Status 'OK' -ErrorAction SilentlyContinue |
    Where-Object { $_.InstanceId -match 'USB' } |
    Select-Object InstanceId, FriendlyName, Class, Status |
    ConvertTo-Json -Compress
"#;

    let output = tokio::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", ps])
        .creation_flags(0x08000000)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse USB devices: {}", e))
}

/// List all registered backend PS command names for the flow editor command picker.
#[tauri::command]
pub fn list_backend_commands() -> Vec<String> {
    backend::list_all_commands()
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

/// Reload all flow listeners (used after importing settings).
#[tauri::command]
pub async fn reload_flows(app: AppHandle) -> Result<(), String> {
    crate::license::require_paid("flows")?;
    ensure_default_flows_persisted();
    // Stop all existing listeners and unregister their shortcuts
    {
        let state = app
            .try_state::<FlowEngineState>()
            .ok_or("Flow engine not initialized")?;
        let mut listeners = state.listeners.lock().map_err(|e| e.to_string())?;
        for (_, listener) in listeners.drain() {
            if !listener.registered_hotkeys.is_empty() {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                for hotkey in &listener.registered_hotkeys {
                    let _ = app.global_shortcut().unregister(hotkey.as_str());
                }
            }
            for handle in listener.handles {
                handle.abort();
            }
        }
    }

    // Start fresh
    flow_engine_log("info", None, "listener", "Reloading all flow listeners");
    start_all_listeners(&app);
    flow_engine_log("info", None, "listener", "Flow listener reload complete");
    Ok(())
}

#[cfg(test)]
mod usb_trigger_cancellation_tests {
    use super::*;

    // With delay_seconds > 0 the watcher tags each line with which instance
    // class fired, so cancellation can tell a "remove" line from an "insert"
    // line for the same device.
    #[test]
    fn parses_tagged_line_when_delay_configured() {
        let parsed = parse_usb_trigger_line(
            "FLOW_TRIGGER|remove|USB\\VID_1234|MySecureKey",
            30,
            "remove",
        );
        assert_eq!(
            parsed,
            Some((
                "remove".to_string(),
                "USB\\VID_1234".to_string(),
                "MySecureKey".to_string()
            ))
        );
    }

    #[test]
    fn tagged_line_can_report_the_opposite_kind() {
        // A "remove" flow is armed but the watcher also emits "insert" lines
        // (needed to detect reinsert-cancels-delay) — the kind must come
        // through as "insert", not get coerced to the trigger's own mode.
        let parsed = parse_usb_trigger_line(
            "FLOW_TRIGGER|insert|USB\\VID_1234|MySecureKey",
            30,
            "remove",
        );
        assert_eq!(parsed.map(|(kind, ..)| kind), Some("insert".to_string()));
    }

    // Without a delay there's nothing to cancel, so the watcher only emits
    // untagged lines for its own configured direction — kind falls back to
    // `mode` rather than trying to parse a kind field that isn't there.
    #[test]
    fn untagged_line_defaults_kind_to_mode_when_no_delay() {
        let parsed = parse_usb_trigger_line("FLOW_TRIGGER|USB\\VID_5678|MyMouse", 0, "insert");
        assert_eq!(
            parsed,
            Some((
                "insert".to_string(),
                "USB\\VID_5678".to_string(),
                "MyMouse".to_string()
            ))
        );
    }

    #[test]
    fn non_trigger_lines_are_ignored() {
        assert_eq!(
            parse_usb_trigger_line("some other output", 30, "remove"),
            None
        );
    }

    #[test]
    fn device_key_prefers_device_id_over_name() {
        assert_eq!(
            usb_device_key("USB\\VID_1234", "MySecureKey"),
            "USB\\VID_1234"
        );
    }

    #[test]
    fn device_key_falls_back_to_name_when_id_missing() {
        assert_eq!(usb_device_key("", "MySecureKey"), "MySecureKey");
    }
}
