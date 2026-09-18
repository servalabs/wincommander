// SPDX-License-Identifier: AGPL-3.0-or-later
//
// USB Guard keeps Free monitoring deliberately narrow: current USB attach/detach
// observation plus a persisted local timeline. Advanced analysis and enforcement
// remain in the licensed Pro sidecar. The Free monitor never records filenames,
// copied content, keystrokes, or claims complete Windows USB history.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

const TIMELINE_CAP: usize = 200;
static BASIC_RUNNING: AtomicBool = AtomicBool::new(false);
static BASIC_NOTIFY: AtomicBool = AtomicBool::new(true);
static BASIC_STATE: OnceLock<Mutex<BasicTimeline>> = OnceLock::new();
static BASIC_TASK: OnceLock<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> = OnceLock::new();
static BASIC_LOADED: OnceLock<Mutex<bool>> = OnceLock::new();
static BASIC_CURRENT_KEYS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
static BASIC_HEALTH: OnceLock<Mutex<BasicMonitorHealth>> = OnceLock::new();

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BasicTimeline {
    #[serde(default)]
    generation: u64,
    #[serde(default = "default_basic_notify")]
    notify: bool,
    #[serde(default)]
    records: BTreeMap<String, BasicRecord>,
    #[serde(default)]
    sessions: VecDeque<BasicSession>,
}

fn default_basic_notify() -> bool {
    true
}

impl Default for BasicTimeline {
    fn default() -> Self {
        Self {
            generation: 0,
            notify: true,
            records: BTreeMap::new(),
            sessions: VecDeque::new(),
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BasicIdentity {
    key: String,
    vid: String,
    pid: String,
    friendly_name: String,
    is_hid: bool,
    is_mass_storage: bool,
    instance_id: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BasicRecord {
    identity: BasicIdentity,
    #[serde(default)]
    first_seen: i64,
    last_seen: i64,
    #[serde(default)]
    total_plugged_secs: i64,
    #[serde(default)]
    session_count: u32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BasicSession {
    device_key: String,
    attached_at: i64,
    detached_at: Option<i64>,
    duration_secs: Option<i64>,
    volume_letter: Option<String>,
    #[serde(default)]
    attached_at_estimated: bool,
}

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BasicMonitorHealth {
    monitor_started_at: Option<i64>,
    last_poll_at: Option<i64>,
    last_error: Option<BasicMonitorFailure>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BasicMonitorFailure {
    code: String,
    message: String,
    recovery_action: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BasicPnpRow {
    instance_id: Option<String>,
    friendly_name: Option<String>,
    class: Option<String>,
    service: Option<String>,
}

fn basic_state() -> &'static Mutex<BasicTimeline> {
    BASIC_STATE.get_or_init(|| Mutex::new(BasicTimeline::default()))
}

fn basic_task() -> &'static Mutex<Option<tauri::async_runtime::JoinHandle<()>>> {
    BASIC_TASK.get_or_init(|| Mutex::new(None))
}

fn basic_loaded() -> &'static Mutex<bool> {
    BASIC_LOADED.get_or_init(|| Mutex::new(false))
}

fn basic_current_keys() -> &'static Mutex<BTreeSet<String>> {
    BASIC_CURRENT_KEYS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn basic_health() -> &'static Mutex<BasicMonitorHealth> {
    BASIC_HEALTH.get_or_init(|| Mutex::new(BasicMonitorHealth::default()))
}

fn monitor_failure(code: &str, message: &str, recovery_action: &str) -> BasicMonitorFailure {
    BasicMonitorFailure {
        code: code.to_string(),
        message: message.to_string(),
        recovery_action: recovery_action.to_string(),
    }
}

fn classify_snapshot_failure(message: &str) -> BasicMonitorFailure {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timed out") {
        return monitor_failure(
            "query_timeout",
            "Windows did not return the USB device list before the safety timeout.",
            "Refresh once. If it repeats, restart WinCommander and verify the Windows Plug and Play service is running.",
        );
    }
    if lower.contains("access is denied")
        || lower.contains("access denied")
        || lower.contains("unauthorized")
        || lower.contains("permission")
    {
        return monitor_failure(
            "permission_denied",
            "Windows denied access to the USB device source.",
            "Run WinCommander with the required administrator permissions, then arm USB Protection again.",
        );
    }
    if lower.contains("get-pnpdevice") || lower.contains("plug and play") || lower.contains("pnpdevice") {
        return monitor_failure(
            "source_unavailable",
            "The Windows Plug and Play USB device source is unavailable.",
            "Verify Windows PowerShell and the Plug and Play service, then refresh or restart WinCommander.",
        );
    }
    monitor_failure(
        "query_failed",
        "WinCommander could not read the current Windows USB device list.",
        "Refresh the USB Protection card. If the error persists, restart WinCommander and check Windows Plug and Play health.",
    )
}

fn set_monitor_error(error: BasicMonitorFailure) {
    basic_health().lock().unwrap().last_error = Some(error);
}

fn clear_monitor_error(last_poll_at: i64) {
    let mut health = basic_health().lock().unwrap();
    health.last_poll_at = Some(last_poll_at);
    health.last_error = None;
}

/// The consumer timeline is shared across Windows accounts. It is a small,
/// neutral inventory of monitor-observed sessions, deliberately separate from
/// Pro USB intelligence. It is not a complete Windows USB history database.
fn ensure_basic_loaded() -> Result<(), String> {
    if *basic_loaded().lock().unwrap() {
        return Ok(());
    }
    let _lock = crate::paths::acquire_machine_state_lock("usb-basic-timeline")?;
    if *basic_loaded().lock().unwrap() {
        return Ok(());
    }
    let path = crate::paths::machine_state_file("usb_timeline.json")?;
    let restored = if path.exists() {
        let raw = std::fs::read(&path).map_err(|error| format!("read USB timeline: {error}"))?;
        serde_json::from_slice(&raw).map_err(|error| format!("parse USB timeline: {error}"))?
    } else {
        BasicTimeline::default()
    };
    *basic_state().lock().unwrap() = restored;
    BASIC_NOTIFY.store(basic_state().lock().unwrap().notify, Ordering::SeqCst);
    *basic_loaded().lock().unwrap() = true;
    Ok(())
}

fn merge_basic_timeline(disk: BasicTimeline, memory: &BasicTimeline) -> BasicTimeline {
    let mut sessions: BTreeMap<(String, i64), BasicSession> = BTreeMap::new();
    for session in disk.sessions.into_iter().chain(memory.sessions.iter().cloned()) {
        let key = (session.device_key.clone(), session.attached_at);
        match sessions.get_mut(&key) {
            Some(existing) if existing.detached_at.is_none() && session.detached_at.is_some() => {
                *existing = session;
            }
            None => {
                sessions.insert(key, session);
            }
            _ => {}
        }
    }

    let mut records = disk.records;
    for (key, record) in &memory.records {
        match records.get_mut(key) {
            Some(existing) => {
                if record.last_seen >= existing.last_seen {
                    existing.identity = record.identity.clone();
                }
                if existing.first_seen == 0 {
                    existing.first_seen = record.first_seen;
                } else if record.first_seen > 0 {
                    existing.first_seen = existing.first_seen.min(record.first_seen);
                }
                existing.last_seen = existing.last_seen.max(record.last_seen);
            }
            None => {
                records.insert(key.clone(), record.clone());
            }
        }
    }

    let mut merged_sessions: Vec<BasicSession> = sessions.into_values().collect();
    merged_sessions.sort_by_key(|session| session.attached_at);
    if merged_sessions.len() > TIMELINE_CAP {
        let keep_from = merged_sessions.len() - TIMELINE_CAP;
        merged_sessions.drain(0..keep_from);
    }
    for (key, record) in &mut records {
        let relevant: Vec<&BasicSession> = merged_sessions
            .iter()
            .filter(|session| session.device_key == *key)
            .collect();
        record.session_count = relevant.len() as u32;
        record.total_plugged_secs = relevant
            .iter()
            .filter_map(|session| session.duration_secs)
            .sum();
    }

    BasicTimeline {
        generation: disk.generation.max(memory.generation),
        notify: disk.notify,
        records,
        sessions: merged_sessions.into(),
    }
}

fn reconcile_basic_timeline(disk: BasicTimeline, memory: &BasicTimeline) -> BasicTimeline {
    if disk.generation > memory.generation {
        disk
    } else {
        merge_basic_timeline(disk, memory)
    }
}

fn reload_basic_timeline_machine_wide() -> Result<(), String> {
    let _lock = crate::paths::acquire_machine_state_lock("usb-basic-timeline")?;
    let path = crate::paths::machine_state_file("usb_timeline.json")?;
    let disk = if path.exists() {
        let raw = std::fs::read(&path).map_err(|error| format!("read USB timeline: {error}"))?;
        serde_json::from_slice(&raw).map_err(|error| format!("parse USB timeline: {error}"))?
    } else {
        BasicTimeline::default()
    };
    let memory = basic_state().lock().unwrap().clone();
    let merged = reconcile_basic_timeline(disk, &memory);
    BASIC_NOTIFY.store(merged.notify, Ordering::SeqCst);
    *basic_state().lock().unwrap() = merged;
    *basic_loaded().lock().unwrap() = true;
    Ok(())
}

fn persist_basic_timeline(snapshot: &BasicTimeline, merge: bool) -> Result<BasicTimeline, String> {
    let _lock = crate::paths::acquire_machine_state_lock("usb-basic-timeline")?;
    let path = crate::paths::machine_state_file("usb_timeline.json")?;
    let state = if merge && path.exists() {
        let raw = std::fs::read(&path)
            .map_err(|error| format!("read USB timeline before merge: {error}"))?;
        let disk: BasicTimeline = serde_json::from_slice(&raw)
            .map_err(|error| format!("parse USB timeline before merge: {error}"))?;
        reconcile_basic_timeline(disk, snapshot)
    } else {
        snapshot.clone()
    };
    let payload = serde_json::to_vec(&state).map_err(|error| format!("encode USB timeline: {error}"))?;
    crate::paths::atomic_write_machine_state(&path, &payload)?;
    Ok(state)
}

fn clear_basic_timeline_machine_wide() -> Result<(), String> {
    let _lock = crate::paths::acquire_machine_state_lock("usb-basic-timeline")?;
    let path = crate::paths::machine_state_file("usb_timeline.json")?;
    let disk: BasicTimeline = if path.exists() {
        let raw = std::fs::read(&path).map_err(|error| format!("read USB timeline before clear: {error}"))?;
        serde_json::from_slice(&raw).map_err(|error| format!("parse USB timeline before clear: {error}"))?
    } else {
        BasicTimeline::default()
    };
    let cleared = BasicTimeline {
        generation: disk.generation.saturating_add(1),
        notify: disk.notify,
        ..BasicTimeline::default()
    };
    let payload = serde_json::to_vec(&cleared).map_err(|error| format!("encode cleared USB timeline: {error}"))?;
    crate::paths::atomic_write_machine_state(&path, &payload)?;
    *basic_state().lock().unwrap() = cleared;
    BASIC_NOTIFY.store(basic_state().lock().unwrap().notify, Ordering::SeqCst);
    *basic_loaded().lock().unwrap() = true;
    Ok(())
}

fn set_basic_notify_machine_wide(enabled: bool) -> Result<(), String> {
    let _lock = crate::paths::acquire_machine_state_lock("usb-basic-timeline")?;
    let path = crate::paths::machine_state_file("usb_timeline.json")?;
    let mut state: BasicTimeline = if path.exists() {
        let raw = std::fs::read(&path).map_err(|error| format!("read USB timeline before notify update: {error}"))?;
        serde_json::from_slice(&raw).map_err(|error| format!("parse USB timeline before notify update: {error}"))?
    } else {
        BasicTimeline::default()
    };
    state.notify = enabled;
    let payload = serde_json::to_vec(&state).map_err(|error| format!("encode USB timeline notify update: {error}"))?;
    crate::paths::atomic_write_machine_state(&path, &payload)?;
    BASIC_NOTIFY.store(enabled, Ordering::SeqCst);
    *basic_state().lock().unwrap() = state;
    *basic_loaded().lock().unwrap() = true;
    Ok(())
}

fn epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn basic_identity(row: BasicPnpRow) -> Option<BasicIdentity> {
    let instance_id = row.instance_id?.replace('/', "\\");
    let mut parts = instance_id.splitn(3, '\\');
    let bus = parts.next()?.to_ascii_uppercase();
    if bus != "USB" && bus != "HID" {
        return None;
    }
    let identifiers = parts.next().unwrap_or_default().to_ascii_uppercase();
    let serial = parts.next().unwrap_or_default().to_ascii_uppercase();
    let field = |prefix: &str| {
        identifiers
            .split('&')
            .find_map(|part| part.strip_prefix(prefix))
            .filter(|value| value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .unwrap_or("0000")
            .to_string()
    };
    let vid = field("VID_");
    let pid = field("PID_");
    let stable_serial = if serial.is_empty() || serial.contains('&') {
        "NOSERIAL".to_string()
    } else {
        serial
    };
    let class = row.class.unwrap_or_default().to_ascii_uppercase();
    let service = row.service.unwrap_or_default().to_ascii_uppercase();
    let usb_storage_service = wincmd_shared::command_strings::join_parts(&["USB~", "STOR~"]);
    Some(BasicIdentity {
        key: format!("USB:{vid}:{pid}:{stable_serial}"),
        vid,
        pid,
        friendly_name: row.friendly_name.unwrap_or_default(),
        is_hid: bus == "HID" || class.contains("HID") || service.contains("HID"),
        is_mass_storage: class.contains("DISK")
            || class.contains("STORAGE")
            || service.contains(&usb_storage_service)
            || service.contains("UASPSTOR"),
        instance_id,
    })
}

async fn basic_snapshot() -> Result<Vec<BasicIdentity>, String> {
    let script = r#"
$ErrorActionPreference='Stop'
try {
  $rows=@(Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -like 'USB\\*' -or $_.InstanceId -like 'HID\\*' } | Select-Object InstanceId,FriendlyName,Class,Service)
  $rows | ConvertTo-Json -Compress
} catch {
  Write-Error $_.Exception.Message
  exit 1
}
"#;
    let mut command = tokio::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .map_err(|_| "USB device query timed out".to_string())?
        .map_err(|error| format!("USB device query failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(if detail.is_empty() {
            "USB device query failed: Windows Plug and Play source returned an error".to_string()
        } else {
            format!("USB device query failed: {detail}")
        });
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "[]" {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(trimmed).map_err(|error| format!("USB device JSON: {error}"))?;
    let rows: Vec<BasicPnpRow> = if value.is_array() {
        serde_json::from_value(value).map_err(|error| format!("USB device rows: {error}"))?
    } else {
        vec![serde_json::from_value(value).map_err(|error| format!("USB device row: {error}"))?]
    };
    Ok(rows.into_iter().filter_map(basic_identity).collect())
}

fn open_session_in_current_run(session: &BasicSession, started_at: i64) -> bool {
    session.detached_at.is_none() && session.attached_at >= started_at
}

fn apply_basic_snapshot(
    state: &mut BasicTimeline,
    current: &BTreeMap<String, BasicIdentity>,
    now: i64,
    monitor_started_at: i64,
    first_poll: bool,
) -> (Vec<BasicIdentity>, Vec<String>) {
    let mut attached = Vec::new();
    let mut detached = Vec::new();

    let open_current_run: Vec<String> = state
        .sessions
        .iter()
        .filter(|session| open_session_in_current_run(session, monitor_started_at))
        .map(|session| session.device_key.clone())
        .collect();

    for key in open_current_run {
        if !current.contains_key(&key) {
            if let Some(session) = state.sessions.iter_mut().rev().find(|session| {
                session.device_key == key && open_session_in_current_run(session, monitor_started_at)
            }) {
                session.detached_at = Some(now);
                session.duration_secs = Some(now.saturating_sub(session.attached_at));
                detached.push(key);
            }
        }
    }

    for identity in current.values() {
        let has_open_current_run = state.sessions.iter().any(|session| {
            session.device_key == identity.key && open_session_in_current_run(session, monitor_started_at)
        });
        let record = state.records.entry(identity.key.clone()).or_insert_with(|| BasicRecord {
            identity: identity.clone(),
            first_seen: now,
            last_seen: now,
            total_plugged_secs: 0,
            session_count: 0,
        });
        record.identity = identity.clone();
        record.last_seen = now;
        if !has_open_current_run {
            state.sessions.push_back(BasicSession {
                device_key: identity.key.clone(),
                attached_at: now,
                detached_at: None,
                duration_secs: None,
                volume_letter: None,
                attached_at_estimated: first_poll,
            });
            while state.sessions.len() > TIMELINE_CAP {
                state.sessions.pop_front();
            }
            attached.push(identity.clone());
        }
    }

    let totals: BTreeMap<String, i64> = state
        .sessions
        .iter()
        .filter_map(|session| session.duration_secs.map(|duration| (session.device_key.clone(), duration)))
        .fold(BTreeMap::new(), |mut totals, (key, duration)| {
            *totals.entry(key).or_default() += duration;
            totals
        });
    for record in state.records.values_mut() {
        record.total_plugged_secs = totals.get(&record.identity.key).copied().unwrap_or_default();
        record.session_count = state
            .sessions
            .iter()
            .filter(|session| session.device_key == record.identity.key)
            .count() as u32;
    }

    (attached, detached)
}

async fn basic_poll(app: &AppHandle, first_poll: bool) -> Result<(), String> {
    ensure_basic_loaded()?;
    let devices = match basic_snapshot().await {
        Ok(devices) => devices,
        Err(error) => {
            let failure = classify_snapshot_failure(&error);
            let safe = format!("{} {}", failure.message, failure.recovery_action);
            set_monitor_error(failure);
            return Err(safe);
        }
    };
    let now = epoch();
    let started_at = basic_health()
        .lock()
        .unwrap()
        .monitor_started_at
        .unwrap_or(now);
    let current: BTreeMap<String, BasicIdentity> = devices
        .into_iter()
        .map(|device| (device.key.clone(), device))
        .collect();

    let (attached, detached) = {
        let mut state = basic_state().lock().unwrap();
        apply_basic_snapshot(&mut state, &current, now, started_at, first_poll)
    };

    *basic_current_keys().lock().unwrap() = current.keys().cloned().collect();
    let snapshot = basic_state().lock().unwrap().clone();
    let merged = persist_basic_timeline(&snapshot, true).map_err(|error| {
        set_monitor_error(monitor_failure(
            "persistence_failed",
            "WinCommander observed USB state but could not save the monitor record.",
            "Check ProgramData permissions and free disk space, then refresh USB Protection.",
        ));
        error
    })?;
    *basic_state().lock().unwrap() = merged;
    clear_monitor_error(now);

    for identity in attached {
        let _ = app.emit("usb-device-attached", &identity);
        if BASIC_NOTIFY.load(Ordering::SeqCst) {
            let label = if identity.friendly_name.is_empty() {
                "USB device"
            } else {
                &identity.friendly_name
            };
            let _ = crate::native_notify::show_native_notification(app, "USB device connected", label);
        }
    }
    for key in detached {
        let _ = app.emit("usb-device-detached", json!({ "key": key }));
    }
    Ok(())
}

async fn dispatch_paid(feature: &str, capability: &str, args: Value) -> Result<Value, String> {
    crate::license::require_paid(capability)?;
    crate::sidecar::dispatch_paid_command(feature, args).await
}

async fn dispatch_cleanup(feature: &str) -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command(feature, Value::Null).await
}

#[tauri::command]
pub async fn start_usb_monitor(app: AppHandle) -> Result<Value, String> {
    ensure_basic_loaded()?;
    if BASIC_RUNNING.swap(true, Ordering::SeqCst) {
        return usb_monitor_status();
    }
    let started_at = epoch();
    {
        let mut health = basic_health().lock().unwrap();
        health.monitor_started_at = Some(started_at);
        health.last_poll_at = None;
        health.last_error = None;
    }
    basic_current_keys().lock().unwrap().clear();

    if let Err(error) = basic_poll(&app, true).await {
        BASIC_RUNNING.store(false, Ordering::SeqCst);
        basic_current_keys().lock().unwrap().clear();
        basic_health().lock().unwrap().monitor_started_at = None;
        return Err(error);
    }

    let task_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(3));
        ticker.tick().await;
        while BASIC_RUNNING.load(Ordering::SeqCst) {
            if let Err(error) = basic_poll(&task_app, false).await {
                crate::log_message("warn", &format!("[UsbTimeline] poll failed: {error}"));
            }
            ticker.tick().await;
        }
    });
    *basic_task().lock().unwrap() = Some(task);
    usb_monitor_status()
}

#[tauri::command]
pub async fn stop_usb_monitor() -> Result<Value, String> {
    BASIC_RUNNING.store(false, Ordering::SeqCst);
    if let Some(task) = basic_task().lock().unwrap().take() {
        task.abort();
    }
    basic_current_keys().lock().unwrap().clear();
    basic_health().lock().unwrap().monitor_started_at = None;
    Ok(json!({ "ok": true, "stopped": true }))
}

#[tauri::command]
pub fn usb_monitor_status() -> Result<Value, String> {
    ensure_basic_loaded()?;
    reload_basic_timeline_machine_wide()?;
    let running = BASIC_RUNNING.load(Ordering::SeqCst);
    let connected = if running {
        basic_current_keys().lock().unwrap().len()
    } else {
        0
    };
    let health = basic_health().lock().unwrap().clone();
    Ok(json!({
        "running": running,
        "notify": BASIC_NOTIFY.load(Ordering::SeqCst),
        "connected": connected,
        "monitorStartedAt": health.monitor_started_at,
        "lastPollAt": health.last_poll_at,
        "lastError": health.last_error,
        "historyCoverage": "monitorOnly",
        "windowsHistoryAvailable": false
    }))
}

#[tauri::command]
pub fn get_usb_timeline() -> Result<Value, String> {
    ensure_basic_loaded()?;
    reload_basic_timeline_machine_wide()?;
    let state = basic_state().lock().unwrap().clone();
    let health = basic_health().lock().unwrap().clone();
    let current_keys: Vec<String> = if BASIC_RUNNING.load(Ordering::SeqCst) {
        basic_current_keys().lock().unwrap().iter().cloned().collect()
    } else {
        Vec::new()
    };
    Ok(json!({
        "records": state.records,
        "sessions": state.sessions,
        "currentKeys": current_keys,
        "monitorStartedAt": health.monitor_started_at,
        "lastPollAt": health.last_poll_at,
        "historyCoverage": "monitorOnly",
        "windowsHistoryAvailable": false
    }))
}

#[tauri::command]
pub async fn get_usb_storage_volumes() -> Result<Value, String> {
    let script = r#"
$ErrorActionPreference='SilentlyContinue'
$rows=@(Get-CimInstance Win32_DiskDrive | Where-Object { $_.InterfaceType -eq 'USB' } | ForEach-Object { $m=$_.Model;$s=$_.SerialNumber;Get-CimAssociatedInstance -InputObject $_ -Association Win32_DiskDriveToDiskPartition | ForEach-Object { Get-CimAssociatedInstance -InputObject $_ -Association Win32_LogicalDiskToPartition | ForEach-Object { [pscustomobject]@{DriveLetter=$_.DeviceID;Label=$_.VolumeName;Model=$m;Serial=$s} } } })
$rows | ConvertTo-Json -Compress
"#;
    let mut command = tokio::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .map_err(|_| "USB volume query timed out".to_string())?
        .map_err(|error| format!("USB volume query failed: {error}"))?;
    if !output.status.success() {
        return Ok(Value::Array(Vec::new()));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Value::Array(Vec::new()));
    }
    let value: Value = serde_json::from_str(trimmed).map_err(|error| format!("USB volume JSON: {error}"))?;
    Ok(Value::Array(if let Value::Array(rows) = value { rows } else { vec![value] }))
}

#[tauri::command]
pub fn clear_usb_timeline() -> Result<Value, String> {
    ensure_basic_loaded()?;
    clear_basic_timeline_machine_wide()?;
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub fn set_usb_monitor_notify(enabled: bool) -> Result<Value, String> {
    ensure_basic_loaded()?;
    set_basic_notify_machine_wide(enabled)?;
    Ok(json!({ "notify": enabled }))
}

#[tauri::command]
pub async fn start_usb_metering() -> Result<Value, String> {
    dispatch_paid("start_usb_metering", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn reconcile_usb_guard(legacy: Value) -> Result<Value, String> {
    dispatch_paid("reconcile_usb_guard", "USB Guard", json!({ "legacy": legacy })).await
}

#[tauri::command]
pub async fn stop_usb_metering() -> Result<Value, String> {
    dispatch_cleanup("stop_usb_metering").await
}

#[tauri::command]
pub async fn usb_metering_status() -> Result<Value, String> {
    dispatch_paid("usb_metering_status", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn get_usb_transfer_stats() -> Result<Value, String> {
    dispatch_paid("get_usb_transfer_stats", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn clear_usb_transfer_stats() -> Result<Value, String> {
    dispatch_paid("clear_usb_transfer_stats", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn set_usb_metering_config(
    sample_interval_secs: Option<u64>,
    large_transfer_enabled: Option<bool>,
    large_transfer_threshold_bytes: Option<u64>,
) -> Result<Value, String> {
    dispatch_paid(
        "set_usb_metering_config",
        "USB Guard",
        json!({
            "sampleIntervalSecs": sample_interval_secs,
            "largeTransferEnabled": large_transfer_enabled,
            "largeTransferThresholdBytes": large_transfer_threshold_bytes,
        }),
    )
    .await
}

#[tauri::command]
pub async fn start_usb_hid_guard() -> Result<Value, String> {
    dispatch_paid("start_usb_hid_guard", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn stop_usb_hid_guard() -> Result<Value, String> {
    dispatch_cleanup("stop_usb_hid_guard").await
}

#[tauri::command]
pub async fn usb_hid_guard_status() -> Result<Value, String> {
    dispatch_paid("usb_hid_guard_status", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn set_usb_hid_guard_sensitivity(sensitivity: Value) -> Result<Value, String> {
    dispatch_paid(
        "set_usb_hid_guard_sensitivity",
        "USB Guard",
        json!({ "sensitivity": sensitivity }),
    )
    .await
}

#[tauri::command]
pub async fn get_usb_hid_alerts() -> Result<Value, String> {
    dispatch_paid("get_usb_hid_alerts", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn clear_usb_hid_alerts() -> Result<Value, String> {
    dispatch_paid("clear_usb_hid_alerts", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn usb_hid_guard_allow_device(device_key: String) -> Result<Value, String> {
    dispatch_paid("usb_hid_guard_allow_device", "USB Guard", json!({ "deviceKey": device_key })).await
}

#[tauri::command]
pub async fn usb_hid_guard_disallow_device(device_key: String) -> Result<Value, String> {
    dispatch_paid("usb_hid_guard_disallow_device", "USB Guard", json!({ "deviceKey": device_key })).await
}

#[tauri::command]
pub async fn usb_hid_guard_allow_list() -> Result<Value, String> {
    dispatch_paid("usb_hid_guard_allow_list", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn start_usb_hid_approval_gate(approval_ttl_secs: Option<u64>) -> Result<Value, String> {
    dispatch_paid(
        "start_usb_hid_approval_gate",
        "USB Guard",
        json!({ "approvalTtlSecs": approval_ttl_secs }),
    )
    .await
}

#[tauri::command]
pub async fn stop_usb_hid_approval_gate() -> Result<Value, String> {
    dispatch_cleanup("stop_usb_hid_approval_gate").await
}

#[tauri::command]
pub async fn usb_hid_approval_gate_status() -> Result<Value, String> {
    dispatch_paid("usb_hid_approval_gate_status", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn get_usb_hid_pending_approvals() -> Result<Value, String> {
    dispatch_paid("get_usb_hid_pending_approvals", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn begin_usb_hid_visual_challenge(device_key: String, action: String) -> Result<Value, String> {
    dispatch_paid(
        "begin_usb_hid_visual_challenge",
        "USB Guard",
        json!({ "deviceKey": device_key, "action": action }),
    )
    .await
}

#[tauri::command]
pub async fn submit_usb_hid_visual_challenge_digit(
    device_key: String,
    challenge_id: String,
    step: u8,
    digit: String,
) -> Result<Value, String> {
    dispatch_paid(
        "submit_usb_hid_visual_challenge_digit",
        "USB Guard",
        json!({ "deviceKey": device_key, "challengeId": challenge_id, "step": step, "digit": digit }),
    )
    .await
}

#[tauri::command]
pub async fn approve_usb_hid_once(
    device_key: String,
    challenge_id: String,
    step: u8,
    digit: String,
) -> Result<Value, String> {
    dispatch_paid(
        "approve_usb_hid_once",
        "USB Guard",
        json!({ "deviceKey": device_key, "challengeId": challenge_id, "step": step, "digit": digit }),
    )
    .await
}

#[tauri::command]
pub async fn trust_usb_hid_always(
    device_key: String,
    challenge_id: String,
    step: u8,
    digit: String,
) -> Result<Value, String> {
    dispatch_paid(
        "trust_usb_hid_always",
        "USB Guard",
        json!({ "deviceKey": device_key, "challengeId": challenge_id, "step": step, "digit": digit }),
    )
    .await
}

#[tauri::command]
pub async fn block_usb_hid_pending(device_key: String) -> Result<Value, String> {
    dispatch_paid("block_usb_hid_pending", "USB Guard", json!({ "deviceKey": device_key })).await
}

#[tauri::command]
pub async fn usb_device_trust_score(device_key: String) -> Result<Value, String> {
    dispatch_paid("usb_device_trust_score", "USB Guard", json!({ "deviceKey": device_key })).await
}

#[tauri::command]
pub async fn start_usb_autosandbox() -> Result<Value, String> {
    dispatch_paid("start_usb_autosandbox", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn stop_usb_autosandbox() -> Result<Value, String> {
    dispatch_cleanup("stop_usb_autosandbox").await
}

#[tauri::command]
pub async fn usb_autosandbox_status() -> Result<Value, String> {
    dispatch_paid("usb_autosandbox_status", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn set_usb_autosandbox_config(config: Value) -> Result<Value, String> {
    dispatch_paid("set_usb_autosandbox_config", "USB Guard", json!({ "config": config })).await
}

#[tauri::command]
pub async fn get_usb_autosandbox_recent() -> Result<Value, String> {
    dispatch_paid("get_usb_autosandbox_recent", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn clear_usb_autosandbox_recent() -> Result<Value, String> {
    dispatch_paid("clear_usb_autosandbox_recent", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn block_usb_device(args: Value) -> Result<Value, String> {
    dispatch_paid("Set-UsbDeviceBlock", "USB device policy", args).await
}

#[tauri::command]
pub async fn allow_usb_device(args: Value) -> Result<Value, String> {
    dispatch_paid("Set-UsbDeviceAllow", "USB device policy", args).await
}

#[tauri::command]
pub async fn set_usb_volume_readonly(args: Value) -> Result<Value, String> {
    dispatch_paid("Set-UsbVolumeReadOnly", "USB device policy", args).await
}

#[tauri::command]
pub async fn quarantine_usb_device(args: Value) -> Result<Value, String> {
    dispatch_paid("Invoke-UsbQuarantine", "USB device policy", args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(key: &str, friendly_name: &str) -> BasicIdentity {
        BasicIdentity {
            key: key.to_string(),
            vid: "1234".to_string(),
            pid: "5678".to_string(),
            friendly_name: friendly_name.to_string(),
            is_hid: false,
            is_mass_storage: true,
            instance_id: r"USB\VID_1234&PID_5678\SERIAL".to_string(),
        }
    }

    fn current(key: &str) -> BTreeMap<String, BasicIdentity> {
        BTreeMap::from([(key.to_string(), identity(key, "USB drive"))])
    }

    #[test]
    fn basic_timeline_defaults_to_machine_notification_enabled() {
        assert!(BasicTimeline::default().notify);
    }

    #[test]
    fn lifecycle_arm_attach_refresh_detach_preserves_duration() {
        let key = "USB:1234:5678:SERIAL";
        let mut state = BasicTimeline::default();
        let started = 100;

        let (attached, detached) = apply_basic_snapshot(&mut state, &current(key), 100, started, true);
        assert_eq!(attached.len(), 1);
        assert!(detached.is_empty());
        assert!(state.sessions.back().unwrap().attached_at_estimated);

        let (attached, detached) = apply_basic_snapshot(&mut state, &current(key), 105, started, false);
        assert!(attached.is_empty());
        assert!(detached.is_empty());
        assert_eq!(state.sessions.len(), 1);

        let (attached, detached) = apply_basic_snapshot(&mut state, &BTreeMap::new(), 112, started, false);
        assert!(attached.is_empty());
        assert_eq!(detached, vec![key.to_string()]);
        let session = state.sessions.back().unwrap();
        assert_eq!(session.detached_at, Some(112));
        assert_eq!(session.duration_secs, Some(12));
    }

    #[test]
    fn persisted_open_session_is_not_assumed_current_after_restart() {
        let key = "USB:1234:5678:SERIAL";
        let mut state = BasicTimeline::default();
        state.records.insert(
            key.to_string(),
            BasicRecord {
                identity: identity(key, "USB drive"),
                first_seen: 10,
                last_seen: 10,
                total_plugged_secs: 0,
                session_count: 1,
            },
        );
        state.sessions.push_back(BasicSession {
            device_key: key.to_string(),
            attached_at: 10,
            detached_at: None,
            duration_secs: None,
            volume_letter: None,
            attached_at_estimated: false,
        });

        let (attached, detached) = apply_basic_snapshot(&mut state, &current(key), 200, 200, true);
        assert_eq!(attached.len(), 1);
        assert!(detached.is_empty());
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.sessions[0].detached_at, None);
        assert_eq!(state.sessions[1].attached_at, 200);
        assert!(state.sessions[1].attached_at_estimated);
    }

    #[test]
    fn persisted_timeline_round_trip_keeps_monitor_evidence_without_sensitive_content() {
        let key = "USB:1234:5678:SERIAL";
        let timeline = BasicTimeline {
            generation: 7,
            notify: false,
            records: BTreeMap::from([(
                key.to_string(),
                BasicRecord {
                    identity: identity(key, "USB drive"),
                    first_seen: 10,
                    last_seen: 20,
                    total_plugged_secs: 10,
                    session_count: 1,
                },
            )]),
            sessions: VecDeque::from([BasicSession {
                device_key: key.to_string(),
                attached_at: 10,
                detached_at: Some(20),
                duration_secs: Some(10),
                volume_letter: None,
                attached_at_estimated: false,
            }]),
        };
        let encoded = serde_json::to_vec(&timeline).unwrap();
        let decoded: BasicTimeline = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.generation, 7);
        assert_eq!(decoded.sessions[0].duration_secs, Some(10));
        let text = String::from_utf8(encoded).unwrap().to_ascii_lowercase();
        assert!(!text.contains("filename"));
        assert!(!text.contains("keystroke"));
        assert!(!text.contains("copiedcontent"));
    }

    #[test]
    fn source_failures_map_to_safe_recovery_reasons() {
        let denied = classify_snapshot_failure("USB device query failed: Access is denied");
        assert_eq!(denied.code, "permission_denied");
        assert!(denied.recovery_action.contains("administrator"));

        let timeout = classify_snapshot_failure("USB device query timed out");
        assert_eq!(timeout.code, "query_timeout");
        assert!(timeout.recovery_action.contains("Plug and Play"));

        let missing = classify_snapshot_failure("Get-PnpDevice is not recognized");
        assert_eq!(missing.code, "source_unavailable");
    }

    #[test]
    fn newer_machine_clear_cannot_be_undone_by_an_old_rds_snapshot() {
        let old = BasicTimeline {
            generation: 3,
            notify: true,
            records: BTreeMap::from([(
                "USB:1234:5678:SERIAL".to_string(),
                BasicRecord {
                    identity: identity("USB:1234:5678:SERIAL", "USB drive"),
                    first_seen: 1,
                    last_seen: 2,
                    total_plugged_secs: 1,
                    session_count: 1,
                },
            )]),
            sessions: VecDeque::new(),
        };
        let cleared = BasicTimeline {
            generation: 4,
            notify: false,
            ..BasicTimeline::default()
        };
        let reconciled = reconcile_basic_timeline(cleared, &old);
        assert!(reconciled.records.is_empty());
        assert!(!reconciled.notify);
    }
}
