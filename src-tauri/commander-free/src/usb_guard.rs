// SPDX-License-Identifier: AGPL-3.0-or-later
//
// USB Guard is a deliberately narrow Free/Pro split.  Free retains a simple
// consumer attach/detach display and storage-volume query. HID cadence analysis,
// transfer metering, trust scoring and automatic response execute only in the
// licensed Pro sidecar. Keep no attack heuristic or policy decision here.

use std::collections::{BTreeMap, VecDeque};
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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BasicTimeline {
    #[serde(default)]
    generation: u64,
    #[serde(default = "default_basic_notify")]
    notify: bool,
    records: BTreeMap<String, BasicRecord>,
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
    first_seen: i64,
    last_seen: i64,
    total_plugged_secs: i64,
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
    attached_at_estimated: bool,
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

/// The consumer timeline is shared across Windows accounts.  It is a small,
/// neutral inventory of attach/detach events, deliberately separate from Pro's
/// richer USB Guard intelligence state.
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
    for session in disk
        .sessions
        .into_iter()
        .chain(memory.sessions.iter().cloned())
    {
        let key = (session.device_key.clone(), session.attached_at);
        match sessions.get_mut(&key) {
            Some(existing) if existing.detached_at.is_none() && session.detached_at.is_some() => {
                *existing = session
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
                existing.first_seen = existing.first_seen.min(record.first_seen);
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
    // A clear advances the generation.  An older RDS process may still have a
    // snapshot in memory, but it must never resurrect records the newer clear
    // intentionally removed.
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
    let payload =
        serde_json::to_vec(&state).map_err(|error| format!("encode USB timeline: {error}"))?;
    crate::paths::atomic_write_machine_state(&path, &payload)?;
    Ok(state)
}

fn clear_basic_timeline_machine_wide() -> Result<(), String> {
    let _lock = crate::paths::acquire_machine_state_lock("usb-basic-timeline")?;
    let path = crate::paths::machine_state_file("usb_timeline.json")?;
    let disk: BasicTimeline = if path.exists() {
        let raw = std::fs::read(&path)
            .map_err(|error| format!("read USB timeline before clear: {error}"))?;
        serde_json::from_slice(&raw)
            .map_err(|error| format!("parse USB timeline before clear: {error}"))?
    } else {
        BasicTimeline::default()
    };
    let cleared = BasicTimeline {
        generation: disk.generation.saturating_add(1),
        notify: disk.notify,
        ..BasicTimeline::default()
    };
    let payload = serde_json::to_vec(&cleared)
        .map_err(|error| format!("encode cleared USB timeline: {error}"))?;
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
        let raw = std::fs::read(&path)
            .map_err(|error| format!("read USB timeline before notify update: {error}"))?;
        serde_json::from_slice(&raw)
            .map_err(|error| format!("parse USB timeline before notify update: {error}"))?
    } else {
        BasicTimeline::default()
    };
    state.notify = enabled;
    let payload = serde_json::to_vec(&state)
        .map_err(|error| format!("encode USB timeline notify update: {error}"))?;
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
    // Keep the paid USB-storage registry token out of the Free binary while
    // still recognising the Windows PnP service name at runtime.
    let usb_storage_service =
        wincmd_shared::command_strings::join_parts(&["USB~", "STOR~"]);
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
$ErrorActionPreference='SilentlyContinue'
$rows=@(Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -like 'USB\\*' -or $_.InstanceId -like 'HID\\*' } | Select-Object InstanceId,FriendlyName,Class,Service)
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
        .map_err(|_| "USB device query timed out".to_string())?
        .map_err(|error| format!("USB device query failed: {error}"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "[]" {
        return Ok(Vec::new());
    }
    let value: Value =
        serde_json::from_str(trimmed).map_err(|error| format!("USB device JSON: {error}"))?;
    let rows: Vec<BasicPnpRow> = if value.is_array() {
        serde_json::from_value(value).map_err(|error| format!("USB device rows: {error}"))?
    } else {
        vec![serde_json::from_value(value).map_err(|error| format!("USB device row: {error}"))?]
    };
    Ok(rows.into_iter().filter_map(basic_identity).collect())
}

async fn basic_poll(app: &AppHandle) {
    if ensure_basic_loaded().is_err() {
        return;
    }
    let Ok(devices) = basic_snapshot().await else {
        return;
    };
    let now = epoch();
    let current: BTreeMap<String, BasicIdentity> = devices
        .into_iter()
        .map(|device| (device.key.clone(), device))
        .collect();
    let mut attached = Vec::new();
    let mut detached = Vec::new();
    {
        let mut state = basic_state().lock().unwrap();
        let open: Vec<String> = state
            .sessions
            .iter()
            .filter(|session| session.detached_at.is_none())
            .map(|session| session.device_key.clone())
            .collect();
        for key in open {
            if !current.contains_key(&key) {
                if let Some(session) = state
                    .sessions
                    .iter_mut()
                    .rev()
                    .find(|session| session.device_key == key && session.detached_at.is_none())
                {
                    session.detached_at = Some(now);
                    session.duration_secs = Some(now.saturating_sub(session.attached_at));
                    detached.push(key);
                }
            }
        }
        for identity in current.values() {
            let is_new = !state
                .sessions
                .iter()
                .any(|session| session.device_key == identity.key && session.detached_at.is_none());
            let record = state
                .records
                .entry(identity.key.clone())
                .or_insert_with(|| BasicRecord {
                    identity: identity.clone(),
                    first_seen: now,
                    last_seen: now,
                    total_plugged_secs: 0,
                    session_count: 0,
                });
            record.identity = identity.clone();
            record.last_seen = now;
            if is_new {
                record.session_count = record.session_count.saturating_add(1);
                state.sessions.push_back(BasicSession {
                    device_key: identity.key.clone(),
                    attached_at: now,
                    detached_at: None,
                    duration_secs: None,
                    volume_letter: None,
                    attached_at_estimated: false,
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
            .filter_map(|session| {
                session
                    .duration_secs
                    .map(|duration| (session.device_key.clone(), duration))
            })
            .fold(BTreeMap::new(), |mut totals, (key, duration)| {
                *totals.entry(key).or_default() += duration;
                totals
            });
        for record in state.records.values_mut() {
            record.total_plugged_secs = totals
                .get(&record.identity.key)
                .copied()
                .unwrap_or_default();
        }
    }
    let snapshot = basic_state().lock().unwrap().clone();
    match persist_basic_timeline(&snapshot, true) {
        Ok(merged) => *basic_state().lock().unwrap() = merged,
        Err(error) => crate::log_message(
            "warn",
            &format!("[UsbTimeline] machine-state write failed: {error}"),
        ),
    }
    for identity in attached {
        let _ = app.emit("usb-device-attached", &identity);
        if BASIC_NOTIFY.load(Ordering::SeqCst) {
            let label = if identity.friendly_name.is_empty() {
                "USB device"
            } else {
                &identity.friendly_name
            };
            let _ =
                crate::native_notify::show_native_notification(app, "USB device connected", label);
        }
    }
    for key in detached {
        let _ = app.emit("usb-device-detached", json!({ "key": key }));
    }
}

async fn dispatch_paid(feature: &str, capability: &str, args: Value) -> Result<Value, String> {
    crate::license::require_paid(capability)?;
    crate::sidecar::dispatch_paid_command(feature, args).await
}

/// Stop paths intentionally do not require a current entitlement.  A customer
/// whose licence has just expired must still be able to turn monitoring off.
async fn dispatch_cleanup(feature: &str) -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command(feature, Value::Null).await
}

#[tauri::command]
pub async fn start_usb_monitor(app: AppHandle) -> Result<Value, String> {
    ensure_basic_loaded()?;
    if BASIC_RUNNING.swap(true, Ordering::SeqCst) {
        return usb_monitor_status();
    }
    basic_poll(&app).await;
    let task_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(3));
        ticker.tick().await;
        while BASIC_RUNNING.load(Ordering::SeqCst) {
            basic_poll(&task_app).await;
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
    Ok(json!({ "ok": true, "stopped": true }))
}

#[tauri::command]
pub fn usb_monitor_status() -> Result<Value, String> {
    ensure_basic_loaded()?;
    reload_basic_timeline_machine_wide()?;
    let connected = basic_state()
        .lock()
        .unwrap()
        .sessions
        .iter()
        .filter(|session| session.detached_at.is_none())
        .count();
    Ok(
        json!({ "running": BASIC_RUNNING.load(Ordering::SeqCst), "notify": BASIC_NOTIFY.load(Ordering::SeqCst), "connected": connected }),
    )
}

#[tauri::command]
pub fn get_usb_timeline() -> Result<Value, String> {
    ensure_basic_loaded()?;
    reload_basic_timeline_machine_wide()?;
    let state = basic_state().lock().unwrap();
    Ok(json!({ "records": state.records, "sessions": state.sessions }))
}

#[tauri::command]
pub async fn get_usb_storage_volumes() -> Result<Value, String> {
    // Read-only consumer convenience: no device history, policy, scoring, or
    // automatic response is calculated in this path.
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
    let value: Value =
        serde_json::from_str(trimmed).map_err(|error| format!("USB volume JSON: {error}"))?;
    Ok(Value::Array(if let Value::Array(rows) = value {
        rows
    } else {
        vec![value]
    }))
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

/// Paid startup reconciliation reads the Pro ProgramData policy.  The optional
/// legacy settings are imported only when Pro has no state yet, so an RDS
/// session holding an old settings cache cannot re-enable a monitor that was
/// deliberately disabled elsewhere.
#[tauri::command]
pub async fn reconcile_usb_guard(legacy: Value) -> Result<Value, String> {
    dispatch_paid(
        "reconcile_usb_guard",
        "USB Guard",
        json!({ "legacy": legacy }),
    )
    .await
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
    dispatch_paid(
        "usb_hid_guard_allow_device",
        "USB Guard",
        json!({ "deviceKey": device_key }),
    )
    .await
}

#[tauri::command]
pub async fn usb_hid_guard_disallow_device(device_key: String) -> Result<Value, String> {
    dispatch_paid(
        "usb_hid_guard_disallow_device",
        "USB Guard",
        json!({ "deviceKey": device_key }),
    )
    .await
}

#[tauri::command]
pub async fn usb_hid_guard_allow_list() -> Result<Value, String> {
    dispatch_paid("usb_hid_guard_allow_list", "USB Guard", Value::Null).await
}

// The approval gate is Pro-owned. It reacts after the keyboard arrives and
// deliberately makes no pre-boot or first-keystroke prevention claim. Positive
// trust actions require a challenge generated and verified by Pro; Free only
// carries neutral transport fields and never owns approval logic.
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
pub async fn begin_usb_hid_visual_challenge(
    device_key: String,
    action: String,
) -> Result<Value, String> {
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
    dispatch_paid(
        "block_usb_hid_pending",
        "USB Guard",
        json!({ "deviceKey": device_key }),
    )
    .await
}

#[tauri::command]
pub async fn usb_device_trust_score(device_key: String) -> Result<Value, String> {
    dispatch_paid(
        "usb_device_trust_score",
        "USB Guard",
        json!({ "deviceKey": device_key }),
    )
    .await
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
    dispatch_paid(
        "set_usb_autosandbox_config",
        "USB Guard",
        json!({ "config": config }),
    )
    .await
}

#[tauri::command]
pub async fn get_usb_autosandbox_recent() -> Result<Value, String> {
    dispatch_paid("get_usb_autosandbox_recent", "USB Guard", Value::Null).await
}

#[tauri::command]
pub async fn clear_usb_autosandbox_recent() -> Result<Value, String> {
    dispatch_paid("clear_usb_autosandbox_recent", "USB Guard", Value::Null).await
}

// USB enforcement remains in Pro. These aliases preserve the existing public
// Tauri command names and payloads while making the trust decision itself Pro
// owned as well.
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

    fn identity(key: &str) -> BasicIdentity {
        BasicIdentity {
            key: key.to_string(),
            vid: "1234".to_string(),
            pid: "5678".to_string(),
            friendly_name: "USB drive".to_string(),
            is_hid: false,
            is_mass_storage: true,
            instance_id: r"USB\VID_1234&PID_5678\SERIAL".to_string(),
        }
    }

    #[test]
    fn basic_timeline_defaults_to_machine_notification_enabled() {
        assert!(BasicTimeline::default().notify);
    }

    #[test]
    fn newer_machine_clear_cannot_be_undone_by_an_old_rds_snapshot() {
        let old = BasicTimeline {
            generation: 3,
            notify: true,
            records: BTreeMap::from([(
                "USB:1234:5678:SERIAL".to_string(),
                BasicRecord {
                    identity: identity("USB:1234:5678:SERIAL"),
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
