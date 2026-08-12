// src-tauri/commander-free/src/flow_health.rs
//
// ═══════════════════════════════════════════════════════════════════════
// FLOW HEALTH — per-flow readiness snapshot
// ═══════════════════════════════════════════════════════════════════════
//
// Intern audit ("most 'broken flow' reports are environment issues, not
// logic issues") motivated this — operators need to see WHY a flow
// won't fire WITHOUT triggering it. We aggregate signals from:
//
//   - flow_engine.listeners  → is the listener armed on this process?
//   - flow_engine.executions → last execution timestamp + status?
//   - flow_engine.preflight  → does the config validate?
//   - flow_capabilities      → are the subsystems this flow needs healthy?
//
// Returned as `Vec<FlowHealth>` from a single `get_flow_health` command
// so the frontend doesn't have to glue four queries together itself.
//
// Health rollup:
//   - ok       — listener armed (or flow disabled), no preflight errors,
//                no capability fails, last execution (if any) succeeded.
//   - warn     — preflight warnings or capability warns, OR last
//                execution had a step failure (but flow listener still
//                armed).
//   - fail     — listener should be armed but isn't, OR preflight has
//                errors, OR a required subsystem is failing.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::flow_capabilities;
use crate::flow_engine::{
    self, get_flows_from_settings, preflight_validate_flow, Flow, FlowEngineState,
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowHealth {
    pub flow_id: String,
    pub name: String,
    pub enabled: bool,
    pub status: HealthStatus,
    /// One-line summary suitable for a tooltip. Empty when `status: ok`.
    pub headline: String,

    // ── Inputs to the rollup ─────────────────────────────────────────
    /// `true` iff the engine's listener map contains an entry for this
    /// flow id. For disabled flows this is `false` by design (we don't
    /// arm listeners for disabled flows).
    pub listener_armed: bool,
    /// Per-flow listener-startup error, captured when registration failed
    /// (e.g. "hotkey conflict — another app owns Ctrl+Shift+H"). Empty
    /// when the listener registered cleanly OR no listener has been
    /// attempted yet.
    pub listener_error: String,
    /// ISO-8601 timestamp of the most recent execution (any status), or
    /// empty if the flow has never fired.
    pub last_execution_at: String,
    /// `"success"` / `"partial"` / `"failed"` / empty if never fired.
    pub last_execution_status: String,
    /// First failure message from the most recent execution, or empty.
    pub last_execution_error: String,
    /// Count of preflight `error` issues. >0 means save would be blocked
    /// on enable.
    pub preflight_error_count: usize,
    /// Count of preflight `warning` issues — informational, not blocking.
    pub preflight_warning_count: usize,
    /// Block types in this flow whose `flow_capabilities.block_readiness`
    /// is `fail` or `warn`. Empty list means every block's dependencies
    /// are green.
    pub problematic_block_types: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════

/// Pulled out so we can unit-test the rollup without an AppHandle.
fn classify(
    enabled: bool,
    listener_armed: bool,
    preflight_error_count: usize,
    preflight_warning_count: usize,
    problematic_blocks: &[String],
    last_execution_status: &str,
) -> (HealthStatus, String) {
    if preflight_error_count > 0 {
        return (
            HealthStatus::Fail,
            format!(
                "{} preflight error{}",
                preflight_error_count,
                if preflight_error_count == 1 { "" } else { "s" }
            ),
        );
    }
    if enabled && !listener_armed {
        return (
            HealthStatus::Fail,
            "Flow is enabled but the listener didn't start.".to_string(),
        );
    }
    if !problematic_blocks.is_empty() {
        // Distinguish fail vs warn — we treat "any problematic block" as
        // warn here since the capability probe itself already breaks
        // the rollup into fail vs warn; if a block lands in this list,
        // its readiness is at-least-warn but might be fail. We can't
        // tell without a fresh probe; warn is the conservative output.
        return (
            HealthStatus::Warn,
            format!(
                "Subsystem issue: {} block type{} affected",
                problematic_blocks.len(),
                if problematic_blocks.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
        );
    }
    if preflight_warning_count > 0 {
        return (
            HealthStatus::Warn,
            format!(
                "{} preflight warning{}",
                preflight_warning_count,
                if preflight_warning_count == 1 {
                    ""
                } else {
                    "s"
                }
            ),
        );
    }
    if last_execution_status == "failed" {
        return (
            HealthStatus::Warn,
            "Last execution failed (listener still armed).".to_string(),
        );
    }
    if last_execution_status == "partial" {
        return (
            HealthStatus::Warn,
            "Last execution had at least one step failure.".to_string(),
        );
    }
    (HealthStatus::Ok, String::new())
}

/// Roll up FlowExecution log for one flow into the three snapshot fields.
fn summarise_executions(
    flow_id: &str,
    executions: &[serde_json::Value],
) -> (String, String, String) {
    // Walk executions newest-first to find the most recent for this id.
    for e in executions.iter().rev() {
        let fid = e.get("flowId").and_then(|v| v.as_str()).unwrap_or("");
        if fid != flow_id {
            continue;
        }
        let started_at = e
            .get("startedAt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let steps = e.get("steps").and_then(|v| v.as_array());
        let (status, error) = if let Some(steps) = steps {
            // KT: a serialized FlowStepResult reports per-step outcome as
            // `success: bool` (camelCase) — there is no `status` field. Reading
            // a non-existent `status` made `failed` always 0, so every execution
            // reported "success" and the health dashboard never surfaced failures.
            let is_failed =
                |s: &&serde_json::Value| s.get("success").and_then(|v| v.as_bool()) == Some(false);
            let failed = steps.iter().filter(is_failed).count();
            let total = steps.len();
            if failed == 0 {
                ("success".to_string(), String::new())
            } else {
                // First failure's message — usually the most informative.
                let err = steps
                    .iter()
                    .find_map(|s| {
                        if is_failed(&s) {
                            s.get("error").and_then(|v| v.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                if failed == total {
                    ("failed".to_string(), err)
                } else {
                    ("partial".to_string(), err)
                }
            }
        } else {
            ("success".to_string(), String::new())
        };
        return (started_at, status, error);
    }
    (String::new(), String::new(), String::new())
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI COMMAND
// ═══════════════════════════════════════════════════════════════════════

/// Snapshot health for every flow. The capability probe is run ONCE
/// and shared across all flow rollups so we don't re-spawn
/// `tailscale.exe ip --4` N times for N flows.
#[tauri::command]
pub async fn get_flow_health(app: AppHandle) -> Result<Vec<FlowHealth>, String> {
    let flows = get_flows_from_settings();

    // Single capability probe — shared across all flows.
    let capabilities = flow_capabilities::probe_flow_capabilities().await?;

    // Snapshot the executions log (separately so we don't hold the
    // engine mutex across the probe).
    let executions_json: Vec<serde_json::Value> = {
        let state = app
            .try_state::<FlowEngineState>()
            .ok_or_else(|| "FlowEngineState not initialised".to_string())?;
        let snap = flow_engine::snapshot_executions(&state);
        serde_json::to_value(&snap)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
    };

    // Listener-armed snapshot.
    let armed_ids: std::collections::HashSet<String> = {
        let state = app
            .try_state::<FlowEngineState>()
            .ok_or_else(|| "FlowEngineState not initialised".to_string())?;
        flow_engine::snapshot_listener_ids(&state)
            .into_iter()
            .collect()
    };

    // Per-flow listener-startup error snapshot.
    let listener_errors: std::collections::HashMap<String, String> = {
        let state = app
            .try_state::<FlowEngineState>()
            .ok_or_else(|| "FlowEngineState not initialised".to_string())?;
        flow_engine::snapshot_listener_errors(&state)
    };

    let mut out = Vec::with_capacity(flows.len());
    for flow in flows {
        let preflight = preflight_validate_flow(flow.clone());
        let preflight_error_count = preflight.errors.len();
        let preflight_warning_count = preflight.warnings.len();

        let problematic_block_types = collect_problematic_block_types(&flow, &capabilities);

        let (last_execution_at, last_execution_status, last_execution_error) =
            summarise_executions(&flow.id, &executions_json);

        let listener_armed = armed_ids.contains(&flow.id);
        let listener_error = listener_errors.get(&flow.id).cloned().unwrap_or_default();
        let (mut status, mut headline) = classify(
            flow.enabled,
            listener_armed,
            preflight_error_count,
            preflight_warning_count,
            &problematic_block_types,
            &last_execution_status,
        );
        // Promote a listener-startup error to the headline so the
        // operator sees WHY the trigger isn't firing.
        if !listener_error.is_empty() {
            status = HealthStatus::Fail;
            headline = listener_error.clone();
        }

        out.push(FlowHealth {
            flow_id: flow.id.clone(),
            name: flow.name.clone(),
            enabled: flow.enabled,
            status,
            headline,
            listener_armed,
            listener_error,
            last_execution_at,
            last_execution_status,
            last_execution_error,
            preflight_error_count,
            preflight_warning_count,
            problematic_block_types,
        });
    }

    Ok(out)
}

/// For each block in the flow, look it up in `capabilities.block_readiness`
/// and keep the type if the readiness is `warn` or `fail`. Dedupes.
fn collect_problematic_block_types(
    flow: &Flow,
    capabilities: &flow_capabilities::CapabilityReport,
) -> Vec<String> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    // We need block_type strings; the Rust Flow type uses enum variants.
    // Easiest path: serialise the flow to JSON and pluck `type` fields,
    // matching the wire format that `flow_capabilities.block_readiness`
    // is keyed on.
    if let Ok(value) = serde_json::to_value(flow) {
        for field in ["triggers", "conditions", "actions"] {
            if let Some(items) = value.get(field).and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(t) = item.get("type").and_then(|v| v.as_str()) {
                        let readiness = capabilities
                            .block_readiness
                            .get(t)
                            .cloned()
                            .unwrap_or_else(|| "ok".to_string());
                        if readiness == "fail" || readiness == "warn" {
                            seen.insert(t.to_string());
                        }
                    }
                }
            }
        }
    }

    seen.into_iter().collect()
}

// ═══════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_when_everything_clean() {
        let (status, headline) = classify(true, true, 0, 0, &[], "success");
        assert_eq!(status, HealthStatus::Ok);
        assert!(headline.is_empty());
    }

    #[test]
    fn fail_when_enabled_but_not_armed() {
        let (status, headline) = classify(true, false, 0, 0, &[], "");
        assert_eq!(status, HealthStatus::Fail);
        assert!(headline.contains("listener"));
    }

    #[test]
    fn fail_when_preflight_errors() {
        let (status, headline) = classify(true, true, 2, 0, &[], "success");
        assert_eq!(status, HealthStatus::Fail);
        assert!(headline.contains("2 preflight error"));
    }

    #[test]
    fn warn_when_capability_problem() {
        let blocks = vec!["WebhookTrigger".to_string()];
        let (status, headline) = classify(true, true, 0, 0, &blocks, "success");
        assert_eq!(status, HealthStatus::Warn);
        assert!(headline.contains("Subsystem"));
    }

    #[test]
    fn warn_when_last_execution_failed() {
        let (status, headline) = classify(true, true, 0, 0, &[], "failed");
        assert_eq!(status, HealthStatus::Warn);
        assert!(headline.contains("Last execution failed"));
    }

    #[test]
    fn ok_when_disabled_even_without_armed() {
        // Disabled flows shouldn't be flagged just for missing a listener.
        let (status, _) = classify(false, false, 0, 0, &[], "");
        assert_eq!(status, HealthStatus::Ok);
    }

    #[test]
    fn summarise_executions_finds_latest_per_flow() {
        let log = vec![
            json!({ "flowId": "a", "startedAt": "t1", "steps": [{ "success": true }] }),
            json!({ "flowId": "b", "startedAt": "t2", "steps": [{ "success": true }] }),
            json!({ "flowId": "a", "startedAt": "t3", "steps": [{ "success": false, "error": "boom" }] }),
        ];
        let (at, status, err) = summarise_executions("a", &log);
        assert_eq!(at, "t3");
        assert_eq!(status, "failed");
        assert_eq!(err, "boom");
    }

    #[test]
    fn summarise_executions_partial_split() {
        let log = vec![json!({
            "flowId": "a",
            "startedAt": "t",
            "steps": [
                { "success": true },
                { "success": false, "error": "nope" }
            ]
        })];
        let (_, status, err) = summarise_executions("a", &log);
        assert_eq!(status, "partial");
        assert_eq!(err, "nope");
    }

    #[test]
    fn summarise_executions_returns_empty_for_unknown_flow() {
        let log = vec![json!({ "flowId": "a", "startedAt": "t1", "steps": [] })];
        let (at, status, err) = summarise_executions("missing", &log);
        assert!(at.is_empty());
        assert!(status.is_empty());
        assert!(err.is_empty());
    }
}
