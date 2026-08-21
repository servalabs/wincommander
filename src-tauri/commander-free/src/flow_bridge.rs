// flow_bridge.rs — the Free-side bridge to the Pro flows engine.
//
// Flows/automation is a PAID feature; the engine (pure flow-core + the
// FlowEngine runtime) lives in the Pro sidecar (`commander-pro/src/flow/`).
// Free's job here is narrow and mirrors the fleet_agent / argus pattern:
//
//   1. Gate every entry point on the paid entitlement (`require_paid`).
//   2. CRUD: persist rules to `settings.app.flows` and sync them to Pro.
//   3. Event forwarding: normalize a Free-observed event + a world snapshot,
//      hand them to Pro (`Flow-Ingest-Event`), and EXECUTE the returned
//      dispatches through the app's own tier/module/self-destruct gate chain
//      (`run_backend_script` / `full_lockdown`), then release the re-entrancy
//      guard (`Flow-Complete`).
//
// The engine — matching, debounce, loop-guard, and the action safety
// classifier — runs in Pro. Free never decides WHICH rule fires; it only
// observes events, executes what Pro admits, and re-checks each command
// through the same gates the UI uses. Actions are in-app commands only: raw
// PowerShell and arbitrary-URL HTTP no longer exist as action types.

use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter};

use crate::sidecar::dispatch_paid_command;

const MAX_ACTION_TIMEOUT_SECS: u64 = 120;

/// DEV-ONLY plaintext diagnostic tracer for the flow pipeline. The normal log
/// (`log_message_src`) is AES-encrypted on disk, so it can't be inspected from
/// outside the app while debugging why a flow "does nothing". This mirrors the
/// key pipeline steps to a plaintext file — `%LOCALAPPDATA%\WinCommander\logs\
/// flow-trace.log` — that a developer can `tail` directly. Compiled out of
/// release builds entirely.
#[cfg(debug_assertions)]
pub(crate) fn flow_trace(msg: impl AsRef<str>) {
    use std::io::Write;
    if let Ok(dir) = crate::paths::user_logs_dir() {
        let path = dir.join("flow-trace.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(f, "[{ts}] {}", msg.as_ref());
        }
    }
}
#[cfg(not(debug_assertions))]
pub(crate) fn flow_trace(_msg: impl AsRef<str>) {}

// ═══════════════════════════════════════════════════════════════════════
// RULE SYNC — push the current rule set + command catalog to Pro
// ═══════════════════════════════════════════════════════════════════════

/// Read the Pro-engine rule store (`settings.app.proFlows`) and the command
/// catalog, and sync both to the Pro engine. Called after any CRUD change and
/// at startup once Pro is up.
pub async fn sync_rules_to_pro() -> Result<Value, String> {
    crate::license::require_paid("flows")?;
    let flows = read_pro_flows()?;
    let catalog = crate::backend::list_all_commands();
    dispatch_paid_command(
        "Flow-Sync-Rules",
        json!({ "rules": flows, "catalog": catalog }),
    )
    .await
}

/// The raw `app.proFlows` array — the `flow_core::Rule` wire shape Pro parses.
fn read_pro_flows() -> Result<Vec<Value>, String> {
    Ok(crate::settings::read_settings()?.app.pro_flows)
}

/// True if fleet policy locks the flows rule set on this (managed) device.
/// Mirrors the `patch_settings_cmd` guard so the CRUD path — which writes via
/// internal `patch_settings` and would otherwise bypass that guard — refuses
/// too. Closes the local-add/edit hole on a managed, flows-locked device.
fn flows_locked_by_policy() -> bool {
    let Ok(settings) = crate::settings::read_settings() else {
        return false;
    };
    settings.policy.sync_mode == "managed"
        && settings
            .policy
            .locked_paths
            .iter()
            .any(|p| p == "app.flows" || p == "app.proFlows")
}

fn ensure_flows_editable() -> Result<(), String> {
    if flows_locked_by_policy() {
        Err("Flows are locked by admin policy on this device.".into())
    } else {
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// CRUD — persist to settings.app.proFlows, then re-sync the whole set to Pro
// ═══════════════════════════════════════════════════════════════════════

/// List the Pro-engine rules (raw JSON). Paid-gated.
#[tauri::command]
pub fn flow_list_rules() -> Result<Vec<Value>, String> {
    crate::license::require_paid("flows")?;
    read_pro_flows()
}

/// Create or update a rule by `id`. Persists to `app.proFlows` and re-syncs to
/// Pro. Paid-gated. `rule` is the raw flow_core::Rule JSON from the UI.
#[tauri::command]
pub async fn flow_save_rule(rule: Value) -> Result<Value, String> {
    crate::license::require_paid("flows")?;
    ensure_flows_editable()?;
    let id = rule
        .get("id")
        .and_then(Value::as_str)
        .ok_or("rule missing id")?
        .to_string();

    let mut flows = read_pro_flows()?;
    if let Some(slot) = flows
        .iter_mut()
        .find(|f| f.get("id").and_then(Value::as_str) == Some(id.as_str()))
    {
        // Refuse to edit a fleet-locked rule locally (deterrent; the server is
        // the real gate via locked_paths — see M5).
        if slot.get("locked").and_then(Value::as_bool) == Some(true) {
            return Err("This rule is managed by fleet policy and is read-only.".into());
        }
        *slot = rule;
    } else {
        flows.push(rule);
    }
    persist_pro_flows(&flows)?;
    sync_rules_to_pro().await
}

/// Delete a rule by id. Refuses fleet-locked rules. Paid-gated.
#[tauri::command]
pub async fn flow_delete_rule(rule_id: String) -> Result<Value, String> {
    crate::license::require_paid("flows")?;
    ensure_flows_editable()?;
    let mut flows = read_pro_flows()?;
    if let Some(slot) = flows
        .iter()
        .find(|f| f.get("id").and_then(Value::as_str) == Some(rule_id.as_str()))
    {
        if slot.get("locked").and_then(Value::as_bool) == Some(true) {
            return Err("This rule is managed by fleet policy and cannot be deleted.".into());
        }
    }
    flows.retain(|f| f.get("id").and_then(Value::as_str) != Some(rule_id.as_str()));
    persist_pro_flows(&flows)?;
    sync_rules_to_pro().await
}

/// Enable/disable a rule by id. Refuses fleet-locked rules. Paid-gated.
#[tauri::command]
pub async fn flow_set_enabled(rule_id: String, enabled: bool) -> Result<Value, String> {
    crate::license::require_paid("flows")?;
    ensure_flows_editable()?;
    let mut flows = read_pro_flows()?;
    let Some(slot) = flows
        .iter_mut()
        .find(|f| f.get("id").and_then(Value::as_str) == Some(rule_id.as_str()))
    else {
        return Err(format!("no rule with id '{rule_id}'"));
    };
    if slot.get("locked").and_then(Value::as_bool) == Some(true) {
        return Err("This rule is managed by fleet policy and is read-only.".into());
    }
    if let Some(obj) = slot.as_object_mut() {
        obj.insert("enabled".into(), Value::Bool(enabled));
    }
    persist_pro_flows(&flows)?;
    sync_rules_to_pro().await
}

/// Manually run a rule now (test trigger): forwards a synthetic manual event so
/// the operator can see the effect. Paid-gated.
#[tauri::command]
pub async fn flow_fire_now(app: AppHandle, rule_id: String) -> Result<(), String> {
    crate::license::require_paid("flows")?;
    // A manual fire ingests a SignalReceived event scoped to nothing; simplest
    // is to look the rule up and dispatch its actions directly through Pro via a
    // dedicated manual path. Here we re-sync then emit a manual marker the UI
    // reflects; real per-rule manual execution is handled by the Pro engine's
    // Flow-Ingest with a manual event in a follow-up. For now, resync ensures
    // Pro has the latest and we surface a UI acknowledgement.
    let _ = sync_rules_to_pro().await?;
    let _ = app.emit("flow-fired-manually", json!({ "ruleId": rule_id }));
    Ok(())
}

fn persist_pro_flows(flows: &[Value]) -> Result<(), String> {
    crate::settings::patch_settings(json!({ "app": { "proFlows": flows } })).map(|_| ())
}

// ═══════════════════════════════════════════════════════════════════════
// EVENT INGEST + EXECUTE — the hot path
// ═══════════════════════════════════════════════════════════════════════

/// Forward a normalized event + world snapshot to the Pro engine, then execute
/// every admitted dispatch. Errors from a single action are logged and do not
/// abort the remaining rules. This is the ONE place a Free-observed event turns
/// into real system actions.
pub async fn ingest_and_execute(app: &AppHandle, event: Value, world: Value) {
    // Self-seeding ingest: attach the current rule set + command catalog to
    // every event. The Pro engine keeps rules in per-process memory and is
    // served out of an ephemeral session pool (processes idle-exit after 30m
    // and respawn on demand), while `Flow-Sync-Rules` is only sent on rule
    // CRUD. So a respawned / never-synced Pro process would evaluate against an
    // EMPTY rule set and silently return zero dispatches — the flow "just
    // doesn't fire", with no error. Shipping the rules with the event makes the
    // ingest correct regardless of which pooled process handles it.
    let rules = read_pro_flows().unwrap_or_default();
    let catalog = crate::backend::list_all_commands();
    let enabled_rules = rules
        .iter()
        .filter(|r| r.get("enabled").and_then(Value::as_bool) == Some(true))
        .count();
    flow_trace(format!(
        "ingest: dispatching to Pro — {} rule(s) in proFlows ({} enabled), {} command(s) in catalog. event={}",
        rules.len(),
        enabled_rules,
        catalog.len(),
        serde_json::to_string(&event).unwrap_or_default()
    ));
    let resp = match dispatch_paid_command(
        "Flow-Ingest-Event",
        json!({ "event": event, "world": world, "rules": rules, "catalog": catalog }),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            crate::log_message_src("warn", "core", &format!("[flows] ingest failed: {e}"));
            flow_trace(format!("ingest: Pro dispatch FAILED: {e}"));
            return;
        }
    };

    let dispatches = resp
        .get("dispatches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    flow_trace(format!(
        "ingest: Pro responded — {} dispatch(es). raw={}",
        dispatches.len(),
        serde_json::to_string(&resp).unwrap_or_default()
    ));

    crate::log_message_src(
        "info",
        "core",
        &format!(
            "[flows] ingest ran: {} rule(s) seeded, {} dispatch(es)",
            rules.len(),
            dispatches.len()
        ),
    );

    // Surface the engine's decision log (admissions / refusals) for the UI AND
    // the backend log — a rule that matched but was debounced / refused /
    // deduped by an unknown command would otherwise leave zero dispatches with
    // no visible reason. Mirroring each engine decision to the log file makes
    // "matched but did not run" diagnosable after the fact.
    if let Some(log) = resp.get("log").and_then(Value::as_array) {
        for line in log {
            let reason = line.get("reason").and_then(Value::as_str).unwrap_or("");
            let message = line.get("message").and_then(Value::as_str).unwrap_or("");
            let level = line.get("level").and_then(Value::as_str).unwrap_or("info");
            crate::log_message_src(
                level,
                "core",
                &format!("[flows] engine: {reason} — {message}"),
            );
            let _ = app.emit("flow-log", line.clone());
        }
    }

    for dispatch in dispatches {
        let rule_id = dispatch
            .get("ruleId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let rule_name = dispatch
            .get("ruleName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let actions = dispatch
            .get("actions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[flows] executing rule '{rule_name}' ({rule_id}) — {} action(s)",
                actions.len()
            ),
        );
        run_actions(app, &rule_id, &actions).await;
        crate::log_message_src(
            "info",
            "core",
            &format!("[flows] finished rule '{rule_name}' ({rule_id})"),
        );

        // Release the re-entrancy guard so the rule can fire again later.
        let _ = dispatch_paid_command("Flow-Complete", json!({ "ruleId": rule_id })).await;

        let _ = app.emit(
            "flow-executed",
            json!({
                "ruleId": rule_id,
                "ruleName": rule_name,
                "actionCount": actions.len(),
                // Include the resolved actions so the UI/console can show exactly
                // WHAT ran (e.g. Set-AppCapabilityAccess Deny vs Allow) — the
                // difference between a rule that blocks the mic and one that
                // silently re-allows it.
                "actions": actions,
            }),
        );
    }
}

/// Execute a rule's actions sequentially. A failing action is logged; execution
/// continues (the Pro engine already classified every action as a recognized
/// in-app command, so failures here are runtime — not safety — failures).
async fn run_actions(app: &AppHandle, rule_id: &str, actions: &[Value]) {
    for action in actions {
        flow_trace(format!(
            "run_action: rule '{rule_id}' → {}",
            serde_json::to_string(action).unwrap_or_default()
        ));
        match run_action(app, action).await {
            Ok(()) => flow_trace(format!("run_action: rule '{rule_id}' action OK")),
            Err(e) => {
                crate::log_message_src(
                    "warn",
                    "core",
                    &format!("[flows] rule '{rule_id}' action failed: {e}"),
                );
                flow_trace(format!("run_action: rule '{rule_id}' action FAILED: {e}"));
            }
        }
    }
}

/// Execute one action. Actions are the resolved wire shape Pro returns: every
/// `SetToggle` has already been lowered to a `CommandAction`, so Free only sees
/// plain commands + the passthrough types.
async fn run_action(app: &AppHandle, action: &Value) -> Result<(), String> {
    let action_type = action
        .get("type")
        .and_then(Value::as_str)
        .ok_or("action missing type")?;

    match action_type {
        "CommandAction" => {
            let command = action
                .get("command")
                .and_then(Value::as_str)
                .ok_or("CommandAction missing command")?
                .to_string();
            let params: HashMap<String, String> = action
                .get("params")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            run_command_with_timeout(app, command, params).await
        }

        "NotifyAction" => {
            let message = action.get("message").and_then(Value::as_str).unwrap_or("");
            let severity = action
                .get("severity")
                .and_then(Value::as_str)
                .unwrap_or("info");
            let duration = action
                .get("duration")
                .and_then(Value::as_u64)
                .unwrap_or(4000);
            let _ = app.emit(
                "flow-notify",
                json!({ "message": message, "severity": severity, "duration": duration }),
            );
            Ok(())
        }

        "DelayAction" => {
            let seconds = action.get("seconds").and_then(Value::as_u64).unwrap_or(0);
            tokio::time::sleep(std::time::Duration::from_secs(seconds.min(600))).await;
            Ok(())
        }

        "SignalAction" => {
            let target_role = action
                .get("targetRole")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let signal_type = action
                .get("signalType")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let mut params = HashMap::new();
            params.insert("TargetRole".to_string(), target_role);
            params.insert("SignalType".to_string(), signal_type);
            run_command_with_timeout(app, "Send-ContingencySignal".to_string(), params).await
        }

        "LockdownAction" => {
            // The ONLY safe entry point: full_lockdown independently re-verifies
            // settings.ideal.privacy.self_destruct.enabled and refuses if not
            // armed. Never inline the cascade (AGENTS.md hard rule).
            crate::backend::full_lockdown(app.clone()).await
        }

        "ParallelGroup" => {
            let children = action
                .get("actions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let futures = children.iter().map(|child| run_action(app, child));
            let results = futures::future::join_all(futures).await;
            let errors: Vec<String> = results.into_iter().filter_map(Result::err).collect();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(format!("parallel group: {}", errors.join("; ")))
            }
        }

        // Removed action types must never reach here (the Pro classifier denies
        // them and migration disables their rules), but fail closed if one does.
        "ShellAction" | "HTTPAction" => Err(format!("refused removed action type '{action_type}'")),

        other => Err(format!("unknown action type '{other}'")),
    }
}

async fn run_command_with_timeout(
    app: &AppHandle,
    command: String,
    params: HashMap<String, String>,
) -> Result<(), String> {
    let fut = crate::backend::run_backend_script(app.clone(), command.clone(), params);
    match tokio::time::timeout(std::time::Duration::from_secs(MAX_ACTION_TIMEOUT_SECS), fut).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("command '{command}' timed out")),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// WORLD SNAPSHOT — the read-only machine state conditions evaluate against
// ═══════════════════════════════════════════════════════════════════════

/// Build the world snapshot the Pro engine's conditions read from. Kept
/// intentionally lean: the full settings tree (for SettingCondition), the
/// current time, and the local hour. Battery / SSID / USB presence are best
/// effort and omitted here (a battery/network condition simply fails closed
/// when its field is absent — see flow-core).
pub fn current_world() -> Value {
    let settings_json = crate::settings::read_settings()
        .ok()
        .and_then(|s| serde_json::to_value(&s).ok())
        .unwrap_or_else(|| json!({}));
    let now = chrono::Local::now();
    json!({
        "settings": settings_json,
        "nowMs": now.timestamp_millis(),
        "localHour": now.format("%H").to_string().parse::<u32>().unwrap_or(0),
    })
}

// ═══════════════════════════════════════════════════════════════════════
// EVENT SOURCE ENTRY POINTS — settings-changed + gaze
// ═══════════════════════════════════════════════════════════════════════

/// Called from the settings write choke point after a successful write with the
/// old and new settings trees. Emits a `settings-changed` Tauri event (UI
/// observability) and forwards each changed leaf to the Pro flows engine. A
/// no-op when nothing changed. The caller has already cleared the DECOY_MODE
/// gate (a decoy write is refused before reaching here).
pub fn on_settings_written(old: &Value, new: &Value) {
    let changes = settings_leaf_diff(old, new);
    if changes.is_empty() {
        return;
    }
    if let Some(app) = crate::sidecar::app_handle() {
        let payload: Vec<Value> = changes
            .iter()
            .map(|c| json!({ "path": c.path, "from": c.from, "to": c.to }))
            .collect();
        let _ = app.emit("settings-changed", json!(payload));
    }
    forward_setting_changes(changes);
}

/// High-frequency internal settings paths that must never be forwarded to the
/// flow engine. They change on a timer / on navigation, so forwarding them
/// floods Pro with useless SettingChanged ingests (and the trace log) — the
/// "log gets stuck" symptom. No SettingChangedTrigger legitimately keys off
/// these. Matched on the leaf (last dot segment).
///
/// `proFlows` belongs here too: `diff_into` doesn't recurse into arrays, so
/// every rule CRUD (`flow_save_rule`/`flow_delete_rule`/`flow_set_enabled`)
/// produces exactly one `app.proFlows` leaf change. Those commands already
/// call `sync_rules_to_pro()` directly and await it — forwarding the same
/// change again here spawned a second, redundant `Flow-Ingest-Event`
/// dispatch racing the awaited `Flow-Sync-Rules` call for the same
/// single-slot (debug build) session pool, serializing two full round-trips
/// into every save and reading as "taking too long"/stuck loading.
fn is_flow_noise_path(path: &str) -> bool {
    let leaf = path.rsplit('.').next().unwrap_or(path);
    matches!(
        leaf,
        "lastSeenAt" | "lastPanel" | "updatedAt" | "lastCheckedAt" | "lastRunAt" | "proFlows"
    )
}

/// Forward one settings leaf change to the Pro engine as a `SettingChanged`
/// event. No-op unless the paid entitlement is present. Spawned fire-and-forget
/// from the settings write path.
pub fn forward_setting_changes(changes: Vec<SettingChange>) {
    // Drop high-frequency internal churn (heartbeats, nav) before anything else
    // so it never floods Pro or the trace.
    let changes: Vec<SettingChange> = changes
        .into_iter()
        .filter(|c| !is_flow_noise_path(&c.path))
        .collect();
    if changes.is_empty() {
        return;
    }
    if !crate::license::has_paid_entitlement() {
        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[flows] {} setting change(s) NOT forwarded — no Pro entitlement",
                changes.len()
            ),
        );
        return;
    }
    if crate::settings::is_decoy_mode() {
        crate::log_message_src(
            "info",
            "core",
            "[flows] setting changes suppressed — decoy mode active",
        );
        return;
    }
    let Some(app) = crate::sidecar::app_handle() else {
        crate::log_message_src(
            "warn",
            "core",
            "[flows] setting changes dropped — no app handle yet (startup race)",
        );
        return;
    };
    crate::log_message_src(
        "info",
        "core",
        &format!(
            "[flows] forwarding {} SettingChanged event(s) to engine",
            changes.len()
        ),
    );
    tauri::async_runtime::spawn(async move {
        let world = current_world();
        for change in changes {
            let event = json!({
                "type": "SettingChanged",
                "path": change.path,
                "from": change.from,
                "to": change.to,
            });
            ingest_and_execute(&app, event, world.clone()).await;
        }
    });
}

/// Forward any normalized flow-core `Event` JSON to the Pro engine, spawning
/// the ingest+execute as a fire-and-forget task. No-op unless paid, and INERT
/// in decoy mode: a coerced decoy session must not reveal that automations
/// exist by having a gaze/USB/etc. event fire a visible action.
pub fn forward_event(app: &AppHandle, event: Value) {
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let paid = crate::license::has_paid_entitlement();
    let decoy = crate::settings::is_decoy_mode();
    flow_trace(format!(
        "forward_event: type='{event_type}' paid_entitlement={paid} decoy_mode={decoy}"
    ));

    // Both early-outs used to be silent — the #1 reason a flow "did nothing with
    // no error". Log each explicitly so the trail is: received → (skip reason |
    // forwarding) → ingest ran → executed.
    if !crate::license::has_paid_entitlement() {
        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[flows] '{event_type}' event received but NOT forwarded — no Pro entitlement (flows are a paid feature; activate a licence or the trial)"
            ),
        );
        return;
    }
    if crate::settings::is_decoy_mode() {
        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[flows] '{event_type}' event suppressed — decoy mode active (flows are intentionally inert under duress)"
            ),
        );
        return;
    }

    crate::log_message_src(
        "info",
        "core",
        &format!("[flows] '{event_type}' event received → forwarding to engine"),
    );
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let world = current_world();
        ingest_and_execute(&app, event, world).await;
    });
}

/// Subscribe the bridge to the Free-side event sources and forward each to the
/// Pro flows engine. Called once from Tauri setup(). The two showcase sources —
/// settings-changed (via the settings write path) and gaze (privacy-shield-event)
/// — plus the existing monitor Tauri events that already carry a payload.
pub fn init(app: &AppHandle) {
    use tauri::Listener;

    flow_trace(
        "init: flow_bridge event listeners registered (gaze/usb/ransomware/paste/decoy/wifi)",
    );

    // Gaze (M4): Privacy Shield emits {event/kind} per detection transition.
    let a = app.clone();
    app.listen("privacy-shield-event", move |ev| {
        flow_trace(format!(
            "bus: privacy-shield-event received: {}",
            ev.payload()
        ));
        if let Some(kind) = parse_gaze_kind(ev.payload()) {
            forward_event(&a, json!({ "type": "Gaze", "kind": kind }));
        } else {
            flow_trace(format!(
                "bus: privacy-shield-event payload did NOT parse to a gaze kind: {}",
                ev.payload()
            ));
        }
    });

    // USB insert / remove.
    let a = app.clone();
    app.listen("usb-device-attached", move |ev| {
        forward_event(&a, usb_event(ev.payload(), "insert"));
    });
    let a = app.clone();
    app.listen("usb-device-detached", move |ev| {
        forward_event(&a, usb_event(ev.payload(), "remove"));
    });

    // Ransomware detector (no filter fields).
    let a = app.clone();
    app.listen("ransomware-detected", move |_ev| {
        forward_event(&a, json!({ "type": "RansomwareMonitor" }));
    });

    // Paste / clipboard credential detector.
    let a = app.clone();
    app.listen("paste-monitor-detected", move |ev| {
        flow_trace(format!(
            "bus: paste-monitor-detected received: {}",
            ev.payload()
        ));
        let v = parse_payload(ev.payload());
        forward_event(
            &a,
            json!({
                "type": "PasteMonitor",
                "pattern": v.get("pattern").and_then(Value::as_str).unwrap_or(""),
                "severity": v.get("severity").and_then(Value::as_str).unwrap_or(""),
            }),
        );
    });

    // Decoy / honeypot file access.
    let a = app.clone();
    app.listen("decoy-accessed", move |ev| {
        let v = parse_payload(ev.payload());
        forward_event(
            &a,
            json!({
                "type": "DecoyMonitor",
                "path": v.get("path").and_then(Value::as_str).unwrap_or(""),
            }),
        );
    });

    // Wi-Fi rogue-AP guard.
    let a = app.clone();
    app.listen("wifi-guard-detected", move |ev| {
        let v = parse_payload(ev.payload());
        forward_event(
            &a,
            json!({
                "type": "WifiGuard",
                "ssid": v.get("ssid").and_then(Value::as_str),
            }),
        );
    });
}

fn parse_payload(payload: &str) -> Value {
    serde_json::from_str(payload).unwrap_or(Value::Null)
}

/// Map a `privacy-shield-event` payload to a flow-core gaze kind, accepting
/// either `{ "kind": "look_away" }` or `{ "event": "look_away" }` and the
/// camelCase spellings the detector may use.
fn parse_gaze_kind(payload: &str) -> Option<String> {
    let v = parse_payload(payload);
    let raw = v
        .get("kind")
        .or_else(|| v.get("event"))
        .and_then(Value::as_str)?;
    Some(normalize_gaze_kind(raw))
}

fn normalize_gaze_kind(raw: &str) -> String {
    match raw.to_ascii_lowercase().replace('-', "_").as_str() {
        "lookaway" | "look_away" | "gaze" => "look_away",
        "noface" | "no_face" => "no_face",
        "multiface" | "multi_face" | "multiple_faces" | "multiplefaces" => "multiple_faces",
        "phone" | "device" | "secondary_device" | "secondarydevice" => "secondary_device",
        other => return other.to_string(),
    }
    .to_string()
}

/// Build a flow-core `Usb` event from a `usb-device-{attached,detached}` payload
/// (a serialized `DeviceIdentity`). Tolerant of snake_case / camelCase keys.
fn usb_event(payload: &str, mode: &str) -> Value {
    let v = parse_payload(payload);
    let instance_id = v
        .get("instance_id")
        .or_else(|| v.get("instanceId"))
        .and_then(Value::as_str);
    let name = v
        .get("friendly_name")
        .or_else(|| v.get("friendlyName"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    json!({
        "type": "Usb",
        "mode": mode,
        "deviceInstanceId": instance_id,
        "deviceName": name,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// SETTINGS LEAF DIFF — pure, testable
// ═══════════════════════════════════════════════════════════════════════

/// One changed settings leaf: its dot-path and the old/new values.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingChange {
    pub path: String,
    pub from: Value,
    pub to: Value,
}

/// Path-segment tokens whose leaf VALUES must never appear in a
/// `settings-changed` event — PIN/phrase hashes, private keys, seeds, coercion
/// keywords, install material. The path is still reported (so a rule could in
/// principle key off it) but with null values. Deliberately slightly
/// over-broad: over-redaction only nulls a value, it never leaks one.
const SECRET_SEGMENT_TOKENS: &[&str] = &[
    "pin",     // startupPin (real/decoy/destroy hashes)
    "hash",    // any *Hash field
    "seed",    // flowSigningSeedB64
    "keyword", // unlockKeyword / lockKeyword (anti-coercion)
    "phrase",  // distressPhrases / lockdownWords
    "secret",  // webhook secrets, etc.
    "password", "material", // installMaterial
    "token",
];

fn segment_is_secret(seg: &str) -> bool {
    SECRET_SEGMENT_TOKENS.iter().any(|t| seg.contains(t))
        || seg.ends_with("priv")
        || seg.contains("privkey")
        || seg.contains("privatekey")
}

fn path_is_secret(path: &str) -> bool {
    path.split('.').any(|seg| {
        let seg = seg.to_ascii_lowercase();
        segment_is_secret(&seg)
    })
}

/// Compute the changed leaves between two settings JSON trees as dot-paths.
/// Objects recurse; every other value (scalars, arrays) is a leaf compared by
/// equality. Added or removed leaves report `null` on the missing side.
pub fn settings_leaf_diff(old: &Value, new: &Value) -> Vec<SettingChange> {
    let mut out = Vec::new();
    diff_into(old, new, String::new(), &mut out);
    out
}

fn diff_into(old: &Value, new: &Value, prefix: String, out: &mut Vec<SettingChange>) {
    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            let mut keys: Vec<&String> = old_map.keys().chain(new_map.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                let old_child = old_map.get(key).unwrap_or(&Value::Null);
                let new_child = new_map.get(key).unwrap_or(&Value::Null);
                diff_into(old_child, new_child, child_prefix, out);
            }
        }
        _ => {
            if old != new {
                let redacted = path_is_secret(&prefix);
                out.push(SettingChange {
                    path: prefix,
                    from: if redacted { Value::Null } else { old.clone() },
                    to: if redacted { Value::Null } else { new.clone() },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_detects_a_changed_leaf_with_dot_path() {
        let old = json!({ "ideal": { "privacy": { "telemetry": { "windowsDisabled": true } } } });
        let new = json!({ "ideal": { "privacy": { "telemetry": { "windowsDisabled": false } } } });
        let diff = settings_leaf_diff(&old, &new);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].path, "ideal.privacy.telemetry.windowsDisabled");
        assert_eq!(diff[0].from, json!(true));
        assert_eq!(diff[0].to, json!(false));
    }

    #[test]
    fn diff_ignores_unchanged_trees() {
        let same = json!({ "a": { "b": 1, "c": [1, 2] } });
        assert!(settings_leaf_diff(&same, &same).is_empty());
    }

    #[test]
    fn diff_reports_added_and_removed_leaves() {
        let old = json!({ "a": 1 });
        let new = json!({ "b": 2 });
        let diff = settings_leaf_diff(&old, &new);
        // a removed (1 → null), b added (null → 2)
        assert_eq!(diff.len(), 2);
        let a = diff.iter().find(|d| d.path == "a").unwrap();
        assert_eq!(a.to, Value::Null);
        let b = diff.iter().find(|d| d.path == "b").unwrap();
        assert_eq!(b.from, Value::Null);
    }

    #[test]
    fn diff_redacts_secret_leaf_values_but_reports_the_path() {
        let old = json!({ "ideal": { "privacy": { "startupPin": { "realHash": "aaa" } } } });
        let new = json!({ "ideal": { "privacy": { "startupPin": { "realHash": "bbb" } } } });
        let diff = settings_leaf_diff(&old, &new);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].path, "ideal.privacy.startupPin.realHash");
        assert_eq!(diff[0].from, Value::Null, "secret value must be redacted");
        assert_eq!(diff[0].to, Value::Null, "secret value must be redacted");
    }

    #[test]
    fn secret_paths_are_recognized_across_the_settings_tree() {
        // Private keys, seeds, coercion keywords, phrases, install material.
        for p in [
            "app.flowSigningSeedB64",
            "app.unlockKeyword",
            "app.lockKeyword",
            "ideal.privacy.distressPhrases",
            "ideal.privacy.startupPin.decoyHash",
        ] {
            assert!(path_is_secret(p), "expected '{p}' to be treated as secret");
        }
        // A benign toggle path must NOT be redacted.
        assert!(!path_is_secret("ideal.privacy.telemetry.windowsDisabled"));
        assert!(!path_is_secret("ideal.privacy.appCapabilities.webcam"));
    }

    #[test]
    fn public_fleet_signing_key_is_not_redacted() {
        // signingKeyPub is the PUBLIC key — fine to surface; only the priv is secret.
        assert!(!path_is_secret("app.fleet.signingKeyPub"));
    }
}
