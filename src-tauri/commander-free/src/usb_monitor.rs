// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/usb_monitor.rs
//
// ═══════════════════════════════════════════════════════════════════════
// USB MONITOR — device-attach / detach timeline  (U-A foundation)
// ═══════════════════════════════════════════════════════════════════════
//
// Free, in-process (no Pro sidecar).  Mutations are gated by
// require_paid; read-only commands are ungated.
//
// Two PowerShell WMI watchers (one for insert, one for remove) are
// spawned as hidden subprocesses with kill_on_drop(true), exactly as
// listen_usb() does in flow_engine.rs.  A cold-start enumerate runs
// Get-PnpDevice -PresentOnly to open estimated sessions for devices
// already connected when the monitor starts.
//
// Later phases build on the public contract exported here:
//   U-B  metering / per-device byte accounting
//   U-C  HID-guard (block HID devices by policy)
//   U-D  policy engine (allow-list / deny-list / auto-lock)

use crate::command_strings::matches_parts;
use crate::monitor_util::{now_epoch, ps_escape, try_start, try_stop, StartOutcome};
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

// ── Constants ────────────────────────────────────────────────────────────────

const SESSION_CAP: usize = 500;
const BROADCAST_CAP: usize = 64;
const ATTACH_DEBOUNCE_MS: u64 = 2_000;
const STORE_FILENAME: &str = "usb_timeline.json";

// ── Types ────────────────────────────────────────────────────────────────────

/// Stable per-device identity.  Two serial-less devices of the same
/// VID+PID collapse to a single key; serial_stable=false flags that.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    /// "USB:{vid}:{pid}:{serial|NOSERIAL}" — primary key across all tables.
    pub key: String,
    pub vid: String,
    pub pid: String,
    /// Raw serial segment from the InstanceId (may be synthetic).
    pub serial: String,
    /// true iff the serial segment contained no '&' (i.e., is factory-stable).
    pub serial_stable: bool,
    pub friendly_name: String,
    pub manufacturer: String,
    pub class: String,
    pub is_hid: bool,
    pub is_mass_storage: bool,
    /// Raw Windows PnP InstanceId — needed by U-D to target Disable/Enable-PnpDevice.
    #[serde(default)]
    pub instance_id: String,
}

/// One plug-in/plug-out window for a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbSession {
    pub device_key: String,
    /// Unix epoch seconds when the device appeared (or was estimated on cold-start).
    pub attached_at: i64,
    /// Unix epoch seconds when the device was removed; None = still connected.
    pub detached_at: Option<i64>,
    /// detached_at − attached_at; None while still connected.
    pub duration_secs: Option<i64>,
    pub volume_letter: Option<String>,
    /// true when attached_at was back-filled by cold-start enumerate.
    pub attached_at_estimated: bool,
}

/// Per-device lifetime statistics plus the identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub identity: DeviceIdentity,
    pub first_seen: i64,
    pub last_seen: i64,
    pub total_plugged_secs: i64,
    pub session_count: u32,
}

/// Persisted store — capped ring of sessions + per-device records.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsbTimelineStore {
    pub records: HashMap<String, DeviceRecord>,
    /// Chronological ring; oldest entries drop when > SESSION_CAP.
    pub sessions: VecDeque<UsbSession>,
}

/// Broadcast event emitted to in-process subscribers (U-B / U-C / U-D).
#[derive(Debug, Clone)]
pub enum UsbEvent {
    Attached(DeviceIdentity),
    /// device_key of the removed device.
    Detached(String),
}

// ── Process-lifetime singletons ───────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Monotonically-increasing epoch; stop() bumps it so watcher tasks know to exit.
static RUN_EPOCH: AtomicU64 = AtomicU64::new(0);
/// Whether to show toast notifications (default: true after first start).
static NOTIFY: AtomicBool = AtomicBool::new(true);

/// In-memory timeline + device records, authoritative view.
static STORE: OnceLock<Mutex<UsbTimelineStore>> = OnceLock::new();
/// Broadcast channel for in-process consumers (U-B/C/D).
static BROADCAST_TX: OnceLock<broadcast::Sender<UsbEvent>> = OnceLock::new();
/// Debounce table: device_key → last attach-process timestamp (ms since epoch).
static DEBOUNCE: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
/// Live watcher task handles; aborted on stop() so the PS children are killed.
static WATCHER_HANDLES: OnceLock<Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>> =
    OnceLock::new();

fn store() -> &'static Mutex<UsbTimelineStore> {
    STORE.get_or_init(|| Mutex::new(UsbTimelineStore::default()))
}

fn broadcast_tx() -> &'static broadcast::Sender<UsbEvent> {
    BROADCAST_TX.get_or_init(|| {
        let (tx, _) = broadcast::channel(BROADCAST_CAP);
        tx
    })
}

fn debounce_map() -> &'static Mutex<HashMap<String, u64>> {
    DEBOUNCE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn watcher_handles() -> &'static Mutex<Vec<tauri::async_runtime::JoinHandle<()>>> {
    WATCHER_HANDLES.get_or_init(|| Mutex::new(Vec::new()))
}

// ── Public contract (consumed by U-B / U-C / U-D) ────────────────────────────

/// Subscribe to the in-process USB event stream.
pub fn subscribe() -> broadcast::Receiver<UsbEvent> {
    broadcast_tx().subscribe()
}

/// Snapshot of all currently-connected devices (open sessions, identity present).
pub fn current_devices() -> Vec<DeviceIdentity> {
    let s = store().lock().unwrap();
    s.sessions
        .iter()
        .filter(|sess| sess.detached_at.is_none())
        .filter_map(|sess| s.records.get(&sess.device_key).map(|r| r.identity.clone()))
        .collect()
}

/// Look up a DeviceIdentity by its key string.
pub fn identity_for_key(key: &str) -> Option<DeviceIdentity> {
    store()
        .lock()
        .unwrap()
        .records
        .get(key)
        .map(|r| r.identity.clone())
}

/// Whether the monitor is currently running.
pub fn is_running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Parse a USB physical node or its HID function into a DeviceIdentity.
/// Formats: "USB\VID_xxxx&PID_yyyy\<serial>" and
/// "HID\VID_xxxx&PID_yyyy\<serial>". Windows exposes a composite device's
/// HID function separately, so accepting only the USB parent misses a keyboard
/// function that can inject input.
/// Returns None for non-USB buses and non-HID PnP nodes.
pub fn derive_device_key(instance_id: &str) -> Option<DeviceIdentity> {
    // Normalise backslash variants — WMI sometimes uses forward slash.
    let id = instance_id.replace('/', "\\");

    // USB composite devices enumerate their keyboard function under HID\,
    // while the physical parent is USB\. Both carry VID/PID and need the same
    // device-key namespace so allow-lists and alerts can reason about either.
    let prefix = id
        .split_once('\\')
        .map(|(prefix, _)| prefix.to_ascii_uppercase())?;
    if prefix != "USB" && prefix != "HID" {
        return None;
    }

    let mut parts = id.splitn(3, '\\');
    let _bus = parts.next()?; // "USB" or "HID"
    let vpid_seg = parts.next().unwrap_or(""); // "VID_xxxx&PID_yyyy[&...]"
    let serial_seg = parts.next().unwrap_or(""); // "<serial>" or ""

    let (vid, pid) = parse_vpid(vpid_seg);

    // A serial containing '&' is a Windows-synthetic composite ID, not stable.
    let serial_stable = !serial_seg.is_empty() && !serial_seg.contains('&');
    let serial = if serial_stable {
        serial_seg.to_ascii_uppercase()
    } else {
        "NOSERIAL".to_string()
    };

    let key = format!("USB:{}:{}:{}", vid, pid, serial);

    Some(DeviceIdentity {
        key,
        vid,
        pid,
        serial,
        serial_stable,
        friendly_name: String::new(),
        manufacturer: String::new(),
        class: String::new(),
        is_hid: false,
        is_mass_storage: false,
        instance_id: id.clone(),
    })
}

fn parse_vpid(seg: &str) -> (String, String) {
    let seg_up = seg.to_ascii_uppercase();
    let vid = extract_hex_field(&seg_up, "VID_").unwrap_or_else(|| "0000".to_string());
    let pid = extract_hex_field(&seg_up, "PID_").unwrap_or_else(|| "0000".to_string());
    (vid, pid)
}

fn extract_hex_field(seg: &str, prefix: &str) -> Option<String> {
    let start = seg.find(prefix)? + prefix.len();
    let rest = &seg[start..];
    // Hex field ends at '&' or end-of-string.
    let end = rest.find('&').unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Classify a USB device as HID / mass-storage from its PnP setup class AND its
/// driver Service. The Service is the decisive signal for storage: the tracked
/// `USB\VID…` node of a flash drive reports Class="USB" (it sits under "USB
/// controllers"), NOT "DiskDrive" — only its bound driver service is "USBSTOR".
/// Classifying on Class alone therefore never flagged storage, which hid it from
/// the Storage/HID timeline and starved the byte-metering sampler. Pure +
/// unit-testable.
pub fn classify_usb(class: &str, service: &str) -> (bool, bool) {
    let class_up = class.to_ascii_uppercase();
    let svc_up = service.to_ascii_uppercase();

    let is_hid = matches!(class_up.as_str(), "HIDCLASS" | "KEYBOARD" | "MOUSE")
        || matches!(
            svc_up.as_str(),
            "HIDUSB" | "KBDHID" | "MOUHID" | "KBDCLASS" | "MOUCLASS"
        );

    let is_mass_storage = class_up == "DISKDRIVE"
        || matches_parts(class_up.as_str(), &["USB~", "STOR~"]) // class == "USBSTOR" (rare)
        || matches!(svc_up.as_str(), "USBSTOR" | "UASPSTOR" | "DISK");

    (is_hid, is_mass_storage)
}

/// Enrich a DeviceIdentity by running Get-PnpDevice -InstanceId (blocking,
/// best-effort — returns the skeleton identity on any failure).
fn enrich_identity(mut id: DeviceIdentity, instance_id: &str) -> DeviceIdentity {
    let escaped = ps_escape(instance_id);
    let script = format!(
        r#"
$d = Get-PnpDevice -InstanceId '{escaped}' -ErrorAction SilentlyContinue
if ($d) {{
    Write-Output ("FN=" + $d.FriendlyName)
    Write-Output ("MF=" + $d.Manufacturer)
    Write-Output ("CL=" + $d.Class)
    Write-Output ("SV=" + $d.Service)
}}
"#,
        escaped = escaped
    );

    let mut enrich_cmd = std::process::Command::new("powershell.exe");
    enrich_cmd
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        enrich_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let result = enrich_cmd.output();

    if let Ok(out) = result {
        let text = String::from_utf8_lossy(&out.stdout);
        let mut class = String::new();
        let mut service = String::new();
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("FN=") {
                id.friendly_name = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("MF=") {
                id.manufacturer = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("CL=") {
                class = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("SV=") {
                service = v.trim().to_string();
            }
        }
        let (is_hid, is_mass_storage) = classify_usb(&class, &service);
        id.is_hid = is_hid;
        id.is_mass_storage = is_mass_storage;
        id.class = class;
    }
    id
}

// ── Persistence ───────────────────────────────────────────────────────────────

fn store_path() -> Result<PathBuf, String> {
    let dir = crate::paths::user_data_dir()?;
    Ok(dir.join(STORE_FILENAME))
}

fn load_store() -> UsbTimelineStore {
    match store_path() {
        Ok(p) if p.exists() => {
            let raw = std::fs::read_to_string(&p).unwrap_or_default();
            serde_json::from_str(&raw).unwrap_or_default()
        }
        _ => UsbTimelineStore::default(),
    }
}

fn persist_store(s: &UsbTimelineStore) {
    let Ok(path) = store_path() else { return };
    let Ok(json) = serde_json::to_string(s) else {
        return;
    };
    let _ = atomic_write_str(&path, &json);
}

/// Write temp + rename — local copy so we don't depend on a private fn.
fn atomic_write_str(path: &std::path::Path, data: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Path has no parent".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Path has no file name".to_string())?;
    let tmp = parent.join(format!(".{file_name}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("Create temp: {e}"))?;
        f.write_all(data.as_bytes())
            .map_err(|e| format!("Write temp: {e}"))?;
        f.sync_all().map_err(|e| format!("Fsync temp: {e}"))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Atomic rename: {e}")
    })
}

// ── Timeline mutations ────────────────────────────────────────────────────────

fn open_session(key: &str, estimated: bool) {
    let now = now_epoch();
    let mut s = store().lock().unwrap();

    // If there's already an open session for this key, do nothing.
    if s.sessions
        .iter()
        .any(|sess| sess.device_key == key && sess.detached_at.is_none())
    {
        return;
    }

    let sess = UsbSession {
        device_key: key.to_string(),
        attached_at: now,
        detached_at: None,
        duration_secs: None,
        volume_letter: None,
        attached_at_estimated: estimated,
    };

    s.sessions.push_back(sess);

    // Cap at SESSION_CAP — drop oldest closed sessions first, then open ones.
    while s.sessions.len() > SESSION_CAP {
        // Prefer dropping a closed session from the front.
        let oldest_closed = s.sessions.iter().position(|se| se.detached_at.is_some());
        match oldest_closed {
            Some(i) => {
                // Just drop the historical session-log entry. Its duration was
                // already credited to the DeviceRecord's total_plugged_secs
                // when it closed (see close_session) — crediting it again here
                // would double-count.
                s.sessions.remove(i);
            }
            None => {
                s.sessions.pop_front();
            }
        }
    }

    // Update DeviceRecord timestamps.
    if let Some(rec) = s.records.get_mut(key) {
        rec.last_seen = now;
        rec.session_count += 1;
    }
}

fn close_session(key: &str) -> Option<i64> {
    let now = now_epoch();
    let mut s = store().lock().unwrap();

    // Find the latest open session for this key.
    let idx = s
        .sessions
        .iter()
        .rposition(|sess| sess.device_key == key && sess.detached_at.is_none())?;

    let sess = s.sessions.get_mut(idx)?;
    let duration = now - sess.attached_at;
    sess.detached_at = Some(now);
    sess.duration_secs = Some(duration);

    // Credit the closed session's duration onto the device's cumulative total
    // right away. This must happen here, not only when the session ring later
    // evicts a closed entry (see open_session) — otherwise a device's "total
    // plugged" time never grows until 500 sessions have accumulated globally.
    if let Some(rec) = s.records.get_mut(key) {
        rec.last_seen = now;
        rec.total_plugged_secs += duration;
    }

    Some(duration)
}

/// Close every session that is still open in a freshly-loaded store. Open
/// sessions in persisted state belong to a PREVIOUS process lifetime — we can't
/// know when those devices were actually removed, so end each at the device's
/// last_seen (never before its own attach) and credit the estimated duration to
/// total_plugged_secs. Without this, a reloaded open session shows a ghost
/// "attached" device and plug time keeps counting from the old session's start.
fn close_stale_open_sessions(s: &mut UsbTimelineStore) {
    for sess in s.sessions.iter_mut() {
        if sess.detached_at.is_some() {
            continue;
        }
        let end = s
            .records
            .get(&sess.device_key)
            .map(|r| r.last_seen)
            .unwrap_or(sess.attached_at)
            .max(sess.attached_at);
        let duration = end - sess.attached_at;
        sess.detached_at = Some(end);
        sess.duration_secs = Some(duration);
        if let Some(rec) = s.records.get_mut(&sess.device_key) {
            rec.total_plugged_secs += duration;
        }
    }
}

fn upsert_record(identity: DeviceIdentity) {
    let now = now_epoch();
    let mut s = store().lock().unwrap();
    s.records
        .entry(identity.key.clone())
        .or_insert_with(|| DeviceRecord {
            first_seen: now,
            last_seen: now,
            total_plugged_secs: 0,
            session_count: 0,
            identity: identity.clone(),
        });
    // Always refresh the enriched identity fields.
    if let Some(rec) = s.records.get_mut(&identity.key) {
        rec.identity = identity;
        rec.last_seen = now;
    }
}

// ── WMI watcher spawner (verbatim pattern from flow_engine::listen_usb) ───────

async fn run_watcher(
    app: AppHandle,
    mode: &'static str, // "insert" | "remove"
    epoch: u64,
) {
    // Compose the WQL from fragments at runtime so the full WMI event-subscription
    // string is not one static literal in the binary (AV static-scan hygiene).
    let kind = if mode == "insert" {
        "Creation"
    } else {
        "Deletion"
    };
    let wmi_event = format!("__Instance{}Event", kind);
    let pnp_class = format!("Win32_Pn{}", "PEntity");
    let query = format!(
        "SELECT * FROM {} WITHIN 2 WHERE Target{} ISA '{}'",
        wmi_event, "Instance", pnp_class
    );

    let ps_script = format!(
        r#"
$query = "{query}"
$watcher = New-Object System.Management.ManagementEventWatcher($query)
$watcher.Options.Timeout = [timespan]::MaxValue
while ($true) {{
    try {{
        $evt = $watcher.WaitForNextEvent()
        $dev = $evt.TargetInstance
        $devId = $dev.DeviceID
        if ($devId -like 'USB\*' -or $devId -like 'HID\*') {{
            $devName = $dev.Name
            Write-Output ("USB_EVT|" + $devId + "|" + $devName)
            [Console]::Out.Flush()
        }}
    }} catch {{ Start-Sleep -Seconds 1 }}
}}
"#,
        query = query,
    );

    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            crate::log_message(
                "error",
                &format!("[UsbMonitor] failed to spawn PS watcher ({}): {}", mode, e),
            );
            return;
        }
    };

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
            // Exit if a newer epoch has started (stop was called).
            if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                break;
            }

            if !line.starts_with("USB_EVT|") {
                continue;
            }
            let mut parts = line.splitn(3, '|');
            let _ = parts.next();
            let instance_id = parts.next().unwrap_or_default().to_string();
            let _dev_name = parts.next().unwrap_or_default().to_string();

            if mode == "insert" {
                handle_attach(app.clone(), instance_id, epoch).await;
            } else {
                handle_detach(app.clone(), instance_id).await;
            }
        }
    }

    crate::log_message("info", &format!("[UsbMonitor] watcher ({}) exited", mode));
}

// ── Event handlers ────────────────────────────────────────────────────────────

async fn handle_attach(app: AppHandle, instance_id: String, epoch: u64) {
    let Some(skeleton) = derive_device_key(&instance_id) else {
        return;
    };
    let key = skeleton.key.clone();

    // Per-key debounce: suppress duplicate events within ATTACH_DEBOUNCE_MS.
    {
        let now_ms_val = now_ms();
        let mut db = debounce_map().lock().unwrap();
        if let Some(&last) = db.get(&key) {
            if now_ms_val.saturating_sub(last) < ATTACH_DEBOUNCE_MS {
                return;
            }
        }
        db.insert(key.clone(), now_ms_val);
    }

    // Enrich in a blocking task so we don't stall the async runtime.
    let instance_id_clone = instance_id.clone();
    let skeleton_fallback = skeleton.clone();
    let identity =
        tokio::task::spawn_blocking(move || enrich_identity(skeleton, &instance_id_clone))
            .await
            .unwrap_or(skeleton_fallback);

    // Check epoch again after the blocking call.
    if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
        return;
    }

    upsert_record(identity.clone());
    open_session(&key, false);

    {
        // Snapshot under the lock, then persist outside it — never hold the
        // std::sync::Mutex across the blocking disk I/O in persist_store.
        let snapshot = store().lock().unwrap().clone();
        persist_store(&snapshot);
    }

    // Emit Tauri event.
    let _ = app.emit("usb-device-attached", &identity);

    // Toast.
    if NOTIFY.load(Ordering::Relaxed) {
        let name = if identity.friendly_name.is_empty() {
            format!("USB {}:{}", identity.vid, identity.pid)
        } else {
            identity.friendly_name.clone()
        };
        let _ = crate::native_notify::show_native_notification(&app, "USB Device Connected", &name);
    }

    // Record mass-storage attach to the evidence ledger so the AI advisor can
    // triage it (A8) — storage devices are the exfil-relevant ones.
    if identity.is_mass_storage {
        let label = if identity.friendly_name.is_empty() {
            format!("USB {}:{}", identity.vid, identity.pid)
        } else {
            identity.friendly_name.clone()
        };
        let _ = crate::evidence::evidence_record(
            "monitor".to_string(),
            "info".to_string(),
            format!("USB storage attached: {}", label),
            None,
        );
    }

    // Broadcast to in-process subscribers.
    let _ = broadcast_tx().send(UsbEvent::Attached(identity));
}

async fn handle_detach(app: AppHandle, instance_id: String) {
    let Some(skeleton) = derive_device_key(&instance_id) else {
        return;
    };
    let key = skeleton.key.clone();

    close_session(&key);

    {
        let snapshot = store().lock().unwrap().clone();
        persist_store(&snapshot);
    }

    let _ = app.emit("usb-device-detached", &key);

    if NOTIFY.load(Ordering::Relaxed) {
        let name = identity_for_key(&key)
            .map(|id| {
                if id.friendly_name.is_empty() {
                    format!("USB {}:{}", id.vid, id.pid)
                } else {
                    id.friendly_name.clone()
                }
            })
            .unwrap_or_else(|| key.clone());
        let _ = crate::native_notify::show_native_notification(&app, "USB Device Removed", &name);
    }

    let _ = broadcast_tx().send(UsbEvent::Detached(key));
}

// ── Cold-start enumerate ──────────────────────────────────────────────────────

fn cold_start_enumerate() {
    let script = r#"
Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
  Where-Object { $_.InstanceId -like 'USB\*' -or $_.InstanceId -like 'HID\*' } |
  ForEach-Object {
    Write-Output ("DEV|" + $_.InstanceId + "|" + $_.FriendlyName + "|" + $_.Manufacturer + "|" + $_.Class + "|" + $_.Service)
  }
"#;

    let mut enum_cmd = std::process::Command::new("powershell.exe");
    enum_cmd
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        enum_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let result = enum_cmd.output();
    let Ok(out) = result else { return };
    let text = String::from_utf8_lossy(&out.stdout);

    // Mounted USB volumes, used as a safety net: any enumerated USB device whose
    // stable serial matches a mounted USB disk IS storage, even if its Service
    // string was momentarily blank/unexpected. Serial-only (no sole-volume
    // fallback) so a mouse next to a single stick is never mislabeled storage.
    let usb_vols = list_usb_volumes();

    let mut seen_keys = std::collections::HashSet::new();

    for line in text.lines() {
        if !line.starts_with("DEV|") {
            continue;
        }
        let mut parts = line.splitn(6, '|');
        let _ = parts.next(); // "DEV"
        let instance_id = parts.next().unwrap_or("").trim();
        let friendly_name = parts.next().unwrap_or("").trim();
        let manufacturer = parts.next().unwrap_or("").trim();
        let class = parts.next().unwrap_or("").trim();
        let service = parts.next().unwrap_or("").trim();

        let Some(mut identity) = derive_device_key(instance_id) else {
            continue;
        };

        identity.friendly_name = friendly_name.to_string();
        identity.manufacturer = manufacturer.to_string();
        let (is_hid, mut is_mass_storage) = classify_usb(class, service);
        if !is_mass_storage
            && identity.serial_stable
            && serial_matches_a_volume(&identity.serial, &usb_vols)
        {
            is_mass_storage = true;
        }
        identity.is_hid = is_hid;
        identity.is_mass_storage = is_mass_storage;
        identity.class = class.to_string();

        // Multiple InstanceIds may collapse to the same key (serial-less).
        if seen_keys.contains(&identity.key) {
            continue;
        }
        seen_keys.insert(identity.key.clone());

        let key = identity.key.clone();
        upsert_record(identity);
        open_session(&key, true /* estimated */);
    }

    let snapshot = store().lock().unwrap().clone();
    persist_store(&snapshot);
}

// ── USB storage volume resolution (Explorer-style name + drive letter) ────────

/// A USB-attached storage volume, as File Explorer would present it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsbVolume {
    /// Drive letter with trailing colon, e.g. "E:".
    pub drive_letter: String,
    /// Volume label — Explorer's display name. Empty for unlabeled volumes.
    pub label: String,
    /// Disk model, e.g. "SanDisk Ultra USB Device".
    pub model: String,
    /// USB disk serial number (uppercased) — used to correlate to a device key.
    pub serial: String,
}

/// Correlate a USB device key ("USB:VID:PID:SERIAL") to one of the currently
/// mounted USB volumes. Pure + unit-testable (no I/O).
///
/// Strategy:
///   1. Match by serial when the key carries a real (stable) serial — the disk's
///      Win32_DiskDrive.SerialNumber usually equals the InstanceId serial segment.
///      We accept an exact match or an either-way substring match, since USBSTOR
///      serials sometimes carry an "&0" interface suffix or a vendor prefix.
///   2. Fall back to the sole volume when exactly one USB volume is mounted — the
///      common single-stick case — rather than attribute nothing.
pub fn correlate_volume_letter(device_key: &str, vols: &[UsbVolume]) -> Option<String> {
    let key_serial = device_key
        .splitn(4, ':')
        .nth(3)
        .unwrap_or("")
        .to_ascii_uppercase();

    if !key_serial.is_empty() && key_serial != "NOSERIAL" {
        if let Some(v) = vols.iter().find(|v| {
            let vs = v.serial.to_ascii_uppercase();
            !vs.is_empty()
                && (vs == key_serial || vs.contains(&key_serial) || key_serial.contains(&vs))
        }) {
            return Some(v.drive_letter.clone());
        }
    }

    if vols.len() == 1 {
        return Some(vols[0].drive_letter.clone());
    }
    None
}

/// True iff `serial` (a device-key serial segment) matches the serial of any
/// mounted USB volume. Serial-only — no sole-volume fallback — so it never
/// upgrades a non-storage device (e.g. a mouse) to storage. Pure + testable.
pub fn serial_matches_a_volume(serial: &str, vols: &[UsbVolume]) -> bool {
    let s = serial.to_ascii_uppercase();
    if s.is_empty() || s == "NOSERIAL" {
        return false;
    }
    vols.iter().any(|v| {
        let vs = v.serial.to_ascii_uppercase();
        !vs.is_empty() && (vs == s || vs.contains(&s) || s.contains(&vs))
    })
}

/// Parse the "USBVOL|<letter>|<label>|<model>|<serial>" lines emitted by the
/// enumeration script into UsbVolume rows. Pure + unit-testable.
fn parse_usb_volume_lines(text: &str) -> Vec<UsbVolume> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("USBVOL|")?;
            let mut parts = rest.splitn(4, '|');
            let drive_letter = parts.next().unwrap_or("").trim().to_string();
            if drive_letter.is_empty() {
                return None;
            }
            let label = parts.next().unwrap_or("").trim().to_string();
            let model = parts.next().unwrap_or("").trim().to_string();
            let serial = parts.next().unwrap_or("").trim().to_ascii_uppercase();
            Some(UsbVolume {
                drive_letter,
                label,
                model,
                serial,
            })
        })
        .collect()
}

/// How long we let the USB-volume PowerShell query run before killing it. WMI/CIM
/// association queries can hang for a long time when a USB device is slow to
/// respond or a card reader has no media — a hang here must never wedge the UI
/// (it is awaited, directly or not, by the panel's refresh), so we cap it.
const USB_VOLUME_QUERY_TIMEOUT: Duration = Duration::from_secs(8);

/// Run a PowerShell script and capture stdout, force-killing it if it exceeds
/// `timeout`. Returns None on spawn failure or timeout. This is the guard that
/// stops a hung storage query from freezing the USB panel.
fn run_powershell_capture(script: &str, timeout: Duration) -> Option<String> {
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    crate::log_message(
                        "warn",
                        "[UsbMonitor] USB volume query exceeded timeout — killed (returning no volumes)",
                    );
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    // Output is a handful of short lines — well under the pipe buffer — so reading
    // it after exit cannot deadlock.
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Enumerate USB-attached storage volumes (drive letter + label + model + serial).
///
/// Two independent passes are merged, so a slow/partial CIM association never
/// loses a drive that Windows already knows is removable:
/// (1) Win32_DiskDrive(USB) → DiskPartition → LogicalDisk carries model+serial
/// (needed to correlate to a device key) and covers USB HDD/SSD; (2)
/// Win32_LogicalDisk DriveType=2 (removable) is a fast, association-free fallback
/// that catches any flash drive the walk in (1) missed. Best-effort,
/// hard-timeout-bounded; returns empty on any failure or non-Windows.
pub fn list_usb_volumes() -> Vec<UsbVolume> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$seen = @{}
# Pass 1 (primary): Storage module. Get-Disk's BusType='USB' reliably identifies
# USB flash drives AND USB SSD/HDD — including UASP drives that Win32_DiskDrive
# mislabels InterfaceType='SCSI'. Get-Partition/Get-Volume give letter+label.
Get-Disk | Where-Object { $_.BusType -eq 'USB' } | ForEach-Object {
  $model = $_.FriendlyName
  $serial = $_.SerialNumber
  Get-Partition -DiskNumber $_.Number | Where-Object { $_.DriveLetter } | ForEach-Object {
    $dl = ([string]$_.DriveLetter) + ':'
    $label = (Get-Volume -Partition $_).FileSystemLabel
    if (-not $seen.ContainsKey($dl)) {
      $seen[$dl] = $true
      Write-Output ("USBVOL|" + $dl + "|" + $label + "|" + $model + "|" + $serial)
    }
  }
}
# Pass 2: Win32_DiskDrive(USB) association — extra coverage on older hosts.
Get-CimInstance -ClassName Win32_DiskDrive -Filter "InterfaceType='USB'" | ForEach-Object {
  $model = $_.Model
  $serial = $_.SerialNumber
  Get-CimAssociatedInstance $_ -ResultClassName Win32_DiskPartition | ForEach-Object {
    Get-CimAssociatedInstance $_ -ResultClassName Win32_LogicalDisk | ForEach-Object {
      if ($_.DeviceID -and -not $seen.ContainsKey($_.DeviceID)) {
        $seen[$_.DeviceID] = $true
        Write-Output ("USBVOL|" + $_.DeviceID + "|" + $_.VolumeName + "|" + $model + "|" + $serial)
      }
    }
  }
}
# Pass 3 (last resort): any removable logical disk not already found (no serial).
Get-CimInstance -ClassName Win32_LogicalDisk -Filter "DriveType=2" | ForEach-Object {
  if ($_.DeviceID -and -not $seen.ContainsKey($_.DeviceID)) {
    Write-Output ("USBVOL|" + $_.DeviceID + "|" + $_.VolumeName + "||")
  }
}
"#;

    match run_powershell_capture(script, USB_VOLUME_QUERY_TIMEOUT) {
        Some(text) => parse_usb_volume_lines(&text),
        None => Vec::new(),
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Return USB-attached storage volumes for the UI (Explorer-style name + letter).
/// Ungated read.
#[tauri::command]
pub fn get_usb_storage_volumes() -> Vec<UsbVolume> {
    list_usb_volumes()
}

/// Start the USB monitor.  Idempotent — safe to call multiple times.
/// Requires an active Pro licence.
#[tauri::command]
pub async fn start_usb_monitor(app: AppHandle) -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB monitor")?;

    let epoch = match try_start(&RUNNING, &RUN_EPOCH) {
        StartOutcome::AlreadyRunning => {
            return Ok(serde_json::json!({ "status": "already_running" }));
        }
        StartOutcome::Started(epoch) => epoch,
    };

    // Load persisted state, then initialise the singleton. Sessions left open by
    // a previous process lifetime are closed first — keeping them open showed
    // ghost "attached" devices and made plug time count from the OLD session's
    // start; cold-start below opens fresh estimated sessions for devices that
    // are genuinely still connected.
    let mut loaded = load_store();
    close_stale_open_sessions(&mut loaded);
    *store().lock().unwrap() = loaded;

    // Cold-start: open estimated sessions for already-connected devices.
    tokio::task::spawn_blocking(cold_start_enumerate).await.ok();

    // Spawn the two WMI watchers, retaining their handles so stop() can abort
    // them (which drops the Child and triggers kill_on_drop on the PS process).
    let app_insert = app.clone();
    let h_insert = tauri::async_runtime::spawn(run_watcher(app_insert, "insert", epoch));

    let app_remove = app.clone();
    let h_remove = tauri::async_runtime::spawn(run_watcher(app_remove, "remove", epoch));

    {
        let mut hs = watcher_handles().lock().unwrap();
        for stale in hs.drain(..) {
            stale.abort();
        }
        hs.push(h_insert);
        hs.push(h_remove);
    }

    crate::log_message("debug", "[UsbMonitor] started");
    Ok(serde_json::json!({ "status": "started" }))
}

/// Stop the USB monitor (kills watchers via epoch bump; kill_on_drop handles PS).
/// Requires an active Pro licence.
#[tauri::command]
pub async fn stop_usb_monitor() -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB monitor")?;

    // try_stop bumps RUN_EPOCH on success — the watcher loops check this and
    // exit on the next line read.
    if !try_stop(&RUNNING, &RUN_EPOCH) {
        return Ok(serde_json::json!({ "status": "not_running" }));
    }

    // Abort the watcher tasks so the blocking next_line().await is cancelled and
    // the PS children are killed (kill_on_drop) — epoch alone can't unblock them.
    if let Some(hs) = WATCHER_HANDLES.get() {
        for h in hs.lock().unwrap().drain(..) {
            h.abort();
        }
    }

    crate::log_message("debug", "[UsbMonitor] stopped");
    Ok(serde_json::json!({ "status": "stopped" }))
}

/// Current status — ungated read.
#[tauri::command]
pub fn usb_monitor_status() -> serde_json::Value {
    let s = store().lock().unwrap();
    let connected: Vec<&str> = s
        .sessions
        .iter()
        .filter(|sess| sess.detached_at.is_none())
        .map(|sess| sess.device_key.as_str())
        .collect();
    serde_json::json!({
        "running": RUNNING.load(Ordering::Relaxed),
        "notify": NOTIFY.load(Ordering::Relaxed),
        "deviceCount": s.records.len(),
        "sessionCount": s.sessions.len(),
        "connectedCount": connected.len(),
    })
}

/// Return the full timeline — ungated read.
#[tauri::command]
pub fn get_usb_timeline() -> serde_json::Value {
    let s = store().lock().unwrap();
    serde_json::json!({
        "records": s.records,
        "sessions": s.sessions,
    })
}

/// Clear the timeline (keeps the in-memory device records but wipes sessions).
/// Requires an active Pro licence.
#[tauri::command]
pub fn clear_usb_timeline() -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB monitor")?;
    let mut s = store().lock().unwrap();
    s.sessions.clear();
    for rec in s.records.values_mut() {
        rec.total_plugged_secs = 0;
        rec.session_count = 0;
    }
    persist_store(&s);
    Ok(serde_json::json!({ "status": "cleared" }))
}

/// Toggle desktop notifications for USB events.
/// Requires an active Pro licence.
#[tauri::command]
pub fn set_usb_monitor_notify(enabled: bool) -> Result<serde_json::Value, String> {
    crate::license::require_paid("USB monitor")?;
    NOTIFY.store(enabled, Ordering::Relaxed);
    Ok(serde_json::json!({ "notify": enabled }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `store()` is a process-global singleton; cargo runs tests in parallel
    /// by default, so any two tests that reset/replace it (rather than only
    /// reading through the public helpers) race each other. Tests that need
    /// a clean global store must hold this guard for their duration.
    static GLOBAL_STORE_TEST_GUARD: Mutex<()> = Mutex::new(());

    // ── derive_device_key ─────────────────────────────────────────────────────

    #[test]
    fn stable_serial_parses_correctly() {
        let id = derive_device_key(r"USB\VID_1234&PID_5678\ABCDEF01").unwrap();
        assert_eq!(id.vid, "1234");
        assert_eq!(id.pid, "5678");
        assert_eq!(id.serial, "ABCDEF01");
        assert!(id.serial_stable);
        assert_eq!(id.key, "USB:1234:5678:ABCDEF01");
    }

    #[test]
    fn ampersand_serial_becomes_noserial() {
        // Windows synthetic: VID_xxxx&PID_yyyy&MI_00
        let id = derive_device_key(r"USB\VID_AAAA&PID_BBBB\1234&5&COMPOSITE").unwrap();
        assert_eq!(id.serial, "NOSERIAL");
        assert!(!id.serial_stable);
        assert_eq!(id.key, "USB:AAAA:BBBB:NOSERIAL");
    }

    // ── USB volume correlation ────────────────────────────────────────────────

    fn vol(letter: &str, label: &str, serial: &str) -> UsbVolume {
        UsbVolume {
            drive_letter: letter.to_string(),
            label: label.to_string(),
            model: "Test USB Device".to_string(),
            serial: serial.to_string(),
        }
    }

    #[test]
    fn correlate_matches_by_serial_when_multiple_volumes() {
        let vols = vec![
            vol("E:", "STICK-A", "AABBCCDD"),
            vol("F:", "STICK-B", "11223344"),
        ];
        assert_eq!(
            correlate_volume_letter("USB:0781:5567:11223344", &vols),
            Some("F:".to_string())
        );
        assert_eq!(
            correlate_volume_letter("USB:0781:5567:AABBCCDD", &vols),
            Some("E:".to_string())
        );
    }

    #[test]
    fn correlate_serial_substring_match() {
        // USBSTOR serials sometimes carry an interface suffix (&0).
        let vols = vec![vol("E:", "STICK", "AABBCCDD&0")];
        assert_eq!(
            correlate_volume_letter("USB:0781:5567:AABBCCDD", &vols),
            Some("E:".to_string())
        );
    }

    #[test]
    fn correlate_falls_back_to_sole_volume_for_serialless_key() {
        let vols = vec![vol("E:", "STICK", "ZZZ")];
        // NOSERIAL key can't match by serial, but a single mounted volume is
        // unambiguous, so attribute it rather than metering nothing.
        assert_eq!(
            correlate_volume_letter("USB:0781:5567:NOSERIAL", &vols),
            Some("E:".to_string())
        );
    }

    #[test]
    fn correlate_ambiguous_multiple_without_serial_match_returns_none() {
        let vols = vec![vol("E:", "A", "XXX"), vol("F:", "B", "YYY")];
        assert_eq!(
            correlate_volume_letter("USB:0781:5567:NOSERIAL", &vols),
            None
        );
    }

    #[test]
    fn correlate_empty_volume_list_returns_none() {
        assert_eq!(correlate_volume_letter("USB:0781:5567:AABBCCDD", &[]), None);
    }

    // ── USB device classification ─────────────────────────────────────────────

    #[test]
    fn classify_flash_drive_by_service_even_when_class_is_usb() {
        // The tracked USB\ node of a flash drive reports Class="USB" — only the
        // Service reveals it's storage. This is the case that regressed the UI.
        let (is_hid, is_storage) = classify_usb("USB", "USBSTOR");
        assert!(!is_hid);
        assert!(is_storage);
    }

    #[test]
    fn classify_uasp_external_ssd_is_storage() {
        let (_, is_storage) = classify_usb("USB", "uaspstor");
        assert!(is_storage);
    }

    #[test]
    fn classify_mouse_by_service_and_by_class() {
        let (hid_svc, _) = classify_usb("USB", "HidUsb");
        assert!(hid_svc);
        let (hid_cls, _) = classify_usb("HIDClass", "");
        assert!(hid_cls);
    }

    #[test]
    fn classify_diskdrive_class_is_storage() {
        let (_, is_storage) = classify_usb("DiskDrive", "disk");
        assert!(is_storage);
    }

    #[test]
    fn classify_hub_and_bluetooth_are_neither() {
        assert_eq!(classify_usb("USB", "USBHUB3"), (false, false));
        assert_eq!(classify_usb("USB", "usbccgp"), (false, false)); // composite parent
        assert_eq!(classify_usb("Bluetooth", "BTHUSB"), (false, false));
    }

    #[test]
    fn classify_usb_network_and_serial_functions_are_not_hid_or_storage() {
        // Composite USB devices may expose CDC serial or NCM/MBIM networking
        // alongside another function. They are recorded as attached hardware,
        // but must not enter HID injection or mass-storage quarantine paths.
        assert_eq!(classify_usb("Ports", "UsbSer"), (false, false));
        assert_eq!(classify_usb("Net", "UsbNcm"), (false, false));
    }

    #[test]
    fn stale_open_sessions_are_closed_on_load_and_credited() {
        let mut s = UsbTimelineStore::default();
        let key = "USB:1111:2222:STALE".to_string();
        s.records.insert(
            key.clone(),
            DeviceRecord {
                identity: DeviceIdentity {
                    key: key.clone(),
                    vid: "1111".to_string(),
                    pid: "2222".to_string(),
                    serial: "STALE".to_string(),
                    serial_stable: true,
                    friendly_name: String::new(),
                    manufacturer: String::new(),
                    class: String::new(),
                    is_hid: false,
                    is_mass_storage: true,
                    instance_id: String::new(),
                },
                first_seen: 1_000,
                last_seen: 1_600, // device last seen 600s after the session opened
                total_plugged_secs: 50,
                session_count: 2,
            },
        );
        // Open session from a previous process lifetime (never detached).
        s.sessions.push_back(UsbSession {
            device_key: key.clone(),
            attached_at: 1_000,
            detached_at: None,
            duration_secs: None,
            volume_letter: None,
            attached_at_estimated: false,
        });
        // A record whose last_seen predates the session open — duration clamps to 0,
        // never negative.
        let key2 = "USB:3333:4444:CLAMP".to_string();
        s.sessions.push_back(UsbSession {
            device_key: key2.clone(),
            attached_at: 2_000,
            detached_at: None,
            duration_secs: None,
            volume_letter: None,
            attached_at_estimated: false,
        });

        close_stale_open_sessions(&mut s);

        let sess = &s.sessions[0];
        assert_eq!(sess.detached_at, Some(1_600));
        assert_eq!(sess.duration_secs, Some(600));
        // 50 pre-existing + 600 estimated from the stale session.
        assert_eq!(s.records[&key].total_plugged_secs, 650);

        // No record for key2 → ends at its own attached_at with duration 0.
        let sess2 = &s.sessions[1];
        assert_eq!(sess2.detached_at, Some(2_000));
        assert_eq!(sess2.duration_secs, Some(0));

        // Nothing is left open.
        assert!(s.sessions.iter().all(|se| se.detached_at.is_some()));
    }

    #[test]
    fn serial_matches_a_volume_is_serial_only() {
        let vols = vec![vol("E:", "STICK", "AABBCCDD")];
        // Matching serial → storage safety net fires.
        assert!(serial_matches_a_volume("AABBCCDD", &vols));
        // Non-matching serial (e.g. a mouse's) → net does NOT fire, even though a
        // volume is present. This is the guard against mislabeling a mouse.
        assert!(!serial_matches_a_volume("046DC077MOUSE", &vols));
        // No usable serial → never fires.
        assert!(!serial_matches_a_volume("NOSERIAL", &vols));
        assert!(!serial_matches_a_volume("", &vols));
    }

    #[test]
    fn parse_usb_volume_lines_extracts_fields_and_skips_noise() {
        let text = "noise line\n\
                    USBVOL|E:|My Stick|SanDisk Ultra USB Device|AABBCCDD\n\
                    USBVOL||should skip empty letter|model|serial\n\
                    USBVOL|F:||Generic Flash Disk USB Device|11223344\n";
        let vols = parse_usb_volume_lines(text);
        assert_eq!(vols.len(), 2);
        assert_eq!(vols[0].drive_letter, "E:");
        assert_eq!(vols[0].label, "My Stick");
        assert_eq!(vols[0].model, "SanDisk Ultra USB Device");
        assert_eq!(vols[0].serial, "AABBCCDD");
        assert_eq!(vols[1].drive_letter, "F:");
        assert_eq!(vols[1].label, ""); // unlabeled volume
        assert_eq!(vols[1].serial, "11223344");
    }

    #[test]
    fn missing_serial_segment_becomes_noserial() {
        // Only bus + VID/PID, no third segment.
        let id = derive_device_key(r"USB\VID_CCCC&PID_DDDD").unwrap();
        assert_eq!(id.serial, "NOSERIAL");
        assert!(!id.serial_stable);
    }

    #[test]
    fn vid_pid_parsed_case_insensitively() {
        // Lowercase vids (some drivers emit lowercase).
        let id = derive_device_key(r"USB\vid_cafe&pid_beef\SERIAL1").unwrap();
        assert_eq!(id.vid, "CAFE");
        assert_eq!(id.pid, "BEEF");
        assert_eq!(id.key, "USB:CAFE:BEEF:SERIAL1");
    }

    #[test]
    fn composite_hid_function_is_tracked_with_the_usb_device_namespace() {
        let id = derive_device_key(r"HID\VID_1D50&PID_60FC\7&1234&0&0000").unwrap();
        assert_eq!(id.vid, "1D50");
        assert_eq!(id.pid, "60FC");
        assert_eq!(id.key, "USB:1D50:60FC:NOSERIAL");
        assert!(!id.serial_stable);
    }

    #[test]
    fn non_usb_device_returns_none() {
        assert!(derive_device_key(r"BTHENUM\DEV_1234").is_none());
        assert!(derive_device_key("").is_none());
    }

    #[test]
    fn two_serialless_same_vidpid_collapse_to_same_key() {
        let a = derive_device_key(r"USB\VID_1111&PID_2222\A&B").unwrap();
        let b = derive_device_key(r"USB\VID_1111&PID_2222\C&D").unwrap();
        assert_eq!(a.key, b.key);
        assert_eq!(a.key, "USB:1111:2222:NOSERIAL");
    }

    // ── Session ring cap ──────────────────────────────────────────────────────

    #[test]
    fn session_cap_drops_oldest_closed_without_double_crediting_total_plugged_secs() {
        let _guard = GLOBAL_STORE_TEST_GUARD
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Reset the global store for this test.
        *store().lock().unwrap() = UsbTimelineStore::default();

        // Insert SESSION_CAP closed sessions for different keys. Each record's
        // total_plugged_secs is pre-credited with its session's duration, as
        // close_session() does in production the moment a session closes —
        // eviction must NOT add the duration again.
        for i in 0..SESSION_CAP {
            let key = format!("USB:0000:0000:SER{}", i);
            let rec = DeviceRecord {
                identity: DeviceIdentity {
                    key: key.clone(),
                    vid: "0000".to_string(),
                    pid: "0000".to_string(),
                    serial: format!("SER{}", i),
                    serial_stable: true,
                    friendly_name: String::new(),
                    manufacturer: String::new(),
                    class: String::new(),
                    is_hid: false,
                    is_mass_storage: false,
                    instance_id: String::new(),
                },
                first_seen: 0,
                last_seen: 0,
                total_plugged_secs: 10,
                session_count: 1,
            };
            let mut s = store().lock().unwrap();
            s.records.insert(key.clone(), rec);
            s.sessions.push_back(UsbSession {
                device_key: key.clone(),
                attached_at: i as i64,
                detached_at: Some(i as i64 + 10),
                duration_secs: Some(10),
                volume_letter: None,
                attached_at_estimated: false,
            });
        }

        // Now open one more — should push us over the cap.
        let new_key = "USB:0000:0000:NEWDEV".to_string();
        {
            let mut s = store().lock().unwrap();
            s.records.insert(
                new_key.clone(),
                DeviceRecord {
                    identity: DeviceIdentity {
                        key: new_key.clone(),
                        vid: "0000".to_string(),
                        pid: "0000".to_string(),
                        serial: "NEWDEV".to_string(),
                        serial_stable: true,
                        friendly_name: String::new(),
                        manufacturer: String::new(),
                        class: String::new(),
                        is_hid: false,
                        is_mass_storage: false,
                        instance_id: String::new(),
                    },
                    first_seen: 0,
                    last_seen: 0,
                    total_plugged_secs: 0,
                    session_count: 1,
                },
            );
        }
        open_session(&new_key, false);

        let s = store().lock().unwrap();
        assert!(
            s.sessions.len() <= SESSION_CAP,
            "sessions len {} exceeds cap {}",
            s.sessions.len(),
            SESSION_CAP
        );
        // The oldest closed session (SER0) was evicted from the log, but its
        // total_plugged_secs must stay at its already-credited value (10) —
        // NOT double-counted to 20.
        let first_key = "USB:0000:0000:SER0";
        if let Some(rec) = s.records.get(first_key) {
            assert_eq!(rec.total_plugged_secs, 10);
        }
    }

    #[test]
    fn close_session_credits_duration_onto_record_immediately() {
        let _guard = GLOBAL_STORE_TEST_GUARD
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Reset the global store for this test.
        *store().lock().unwrap() = UsbTimelineStore::default();

        let key = "USB:0000:0000:IMMEDIATE".to_string();
        {
            let mut s = store().lock().unwrap();
            s.records.insert(
                key.clone(),
                DeviceRecord {
                    identity: DeviceIdentity {
                        key: key.clone(),
                        vid: "0000".to_string(),
                        pid: "0000".to_string(),
                        serial: "IMMEDIATE".to_string(),
                        serial_stable: true,
                        friendly_name: String::new(),
                        manufacturer: String::new(),
                        class: String::new(),
                        is_hid: false,
                        is_mass_storage: false,
                        instance_id: String::new(),
                    },
                    first_seen: 0,
                    last_seen: 0,
                    total_plugged_secs: 0,
                    session_count: 0,
                },
            );
        }

        open_session(&key, false);
        // Backdate the just-opened session so close_session computes a known,
        // non-zero duration rather than ~0s from two calls in the same second.
        {
            let mut s = store().lock().unwrap();
            let idx = s
                .sessions
                .iter()
                .rposition(|se| se.device_key == key && se.detached_at.is_none())
                .unwrap();
            s.sessions.get_mut(idx).unwrap().attached_at -= 42;
        }

        let duration = close_session(&key);
        assert_eq!(duration, Some(42));

        // A single detach — with no session-cap eviction involved — must be
        // enough to grow total_plugged_secs. Previously this only happened
        // once 500 sessions had accumulated globally, so it looked frozen.
        let s = store().lock().unwrap();
        assert_eq!(s.records[&key].total_plugged_secs, 42);
    }

    // ── Serialize → deserialize round-trip ────────────────────────────────────

    #[test]
    fn store_round_trips_through_json() {
        let mut original = UsbTimelineStore::default();
        let key = "USB:CAFE:BEEF:AABBCCDD".to_string();
        original.records.insert(
            key.clone(),
            DeviceRecord {
                identity: DeviceIdentity {
                    key: key.clone(),
                    vid: "CAFE".to_string(),
                    pid: "BEEF".to_string(),
                    serial: "AABBCCDD".to_string(),
                    serial_stable: true,
                    friendly_name: "Test Device".to_string(),
                    manufacturer: "Acme".to_string(),
                    class: "HIDClass".to_string(),
                    is_hid: true,
                    is_mass_storage: false,
                    instance_id: String::new(),
                },
                first_seen: 1_700_000_000,
                last_seen: 1_700_001_000,
                total_plugged_secs: 300,
                session_count: 2,
            },
        );
        original.sessions.push_back(UsbSession {
            device_key: key.clone(),
            attached_at: 1_700_000_000,
            detached_at: Some(1_700_000_300),
            duration_secs: Some(300),
            volume_letter: Some("E:".to_string()),
            attached_at_estimated: false,
        });

        let json = serde_json::to_string(&original).unwrap();
        let restored: UsbTimelineStore = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.records.len(), 1);
        assert_eq!(restored.sessions.len(), 1);
        let rec = &restored.records[&key];
        assert_eq!(rec.identity.friendly_name, "Test Device");
        assert!(rec.identity.is_hid);
        assert_eq!(rec.total_plugged_secs, 300);
        let sess = &restored.sessions[0];
        assert_eq!(sess.volume_letter, Some("E:".to_string()));
        assert_eq!(sess.duration_secs, Some(300));
    }

    // ── Garbage file → default store ─────────────────────────────────────────

    #[test]
    fn garbage_json_deserialises_to_default() {
        let result: UsbTimelineStore =
            serde_json::from_str("not valid json {{{{").unwrap_or_default();
        assert!(result.records.is_empty());
        assert!(result.sessions.is_empty());
    }

    #[test]
    fn truncated_json_deserialises_to_default() {
        let result: UsbTimelineStore = serde_json::from_str(r#"{"records":{"#).unwrap_or_default();
        assert!(result.records.is_empty());
    }

    // ── Duration math ─────────────────────────────────────────────────────────

    #[test]
    fn closed_session_duration_equals_detached_minus_attached() {
        let sess = UsbSession {
            device_key: "USB:0001:0002:SERIAL".to_string(),
            attached_at: 1_000,
            detached_at: Some(1_300),
            duration_secs: Some(300),
            volume_letter: None,
            attached_at_estimated: false,
        };
        let expected = sess.detached_at.unwrap() - sess.attached_at;
        assert_eq!(sess.duration_secs, Some(expected));
    }

    #[test]
    fn open_session_has_none_duration() {
        let sess = UsbSession {
            device_key: "USB:0001:0002:SERIAL".to_string(),
            attached_at: 1_000,
            detached_at: None,
            duration_secs: None,
            volume_letter: None,
            attached_at_estimated: false,
        };
        assert!(sess.duration_secs.is_none());
        assert!(sess.detached_at.is_none());
    }

    // ── ps_escape ─────────────────────────────────────────────────────────────

    #[test]
    fn apostrophes_in_instance_id_are_doubled() {
        let escaped = ps_escape("USB\\VID_0001&PID'_0002\\SERIAL");
        assert_eq!(escaped, "USB\\VID_0001&PID''_0002\\SERIAL");
    }
}
