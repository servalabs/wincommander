// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/usb_metering.rs
//
// ═══════════════════════════════════════════════════════════════════════
// USB U-B — DATA-TRANSFER METERING  (paid, in-process, no new crate deps)
// ═══════════════════════════════════════════════════════════════════════
//
// Subscribes to U-A's broadcast channel (usb_monitor::subscribe()) and,
// for every mass-storage device that becomes attached, starts a lightweight
// per-device sampler that integrates Windows LogicalDisk perf counters
// (Get-Counter -Continuous) into accumulated read/write byte totals.
//
// Architecture:
//   U-A broadcast → on UsbEvent::Attached(identity) if is_mass_storage →
//     spawn sampler task per device_key
//   also, on start_usb_metering, a cold-start meters every mass-storage device
//     already attached (the common "stick was already plugged in" case)
//   sampler: powershell polls Win32_PerfRawData_PerfDisk_LogicalDisk(Name='E:')
//     parses "USBM|<cumulativeRead>|<cumulativeWrite>" lines
//     accumulates each interval's DELTA into STATS[device_key]
//     if bytes cross threshold (and latch not set) → emit + toast + evidence
//   on UsbEvent::Detached(key) → abort sampler handle
//
// Accuracy note: These figures include ALL volume I/O (OS background reads, AV
// scans, indexing, etc.), not only user file copies — they are for AWARENESS,
// not audit-grade accounting. The byte deltas themselves are exact (see below),
// but what counts as "the copy" is approximate. The UI labels figures as
// approximate. See spec §"Accuracy / threat note" for full caveats.
//
// Measurement mechanism: we read the RAW LogicalDisk perf counters via
// Win32_PerfRawData_PerfDisk_LogicalDisk. For a bulk-count counter the raw value
// is the CUMULATIVE total bytes for that volume, so per-interval deltas are exact
// (no rate-integration error). Crucially, the WMI property names
// (DiskReadBytesPersec / DiskWriteBytesPersec) are English and LOCALE-INDEPENDENT
// — unlike Get-Counter's localized "\LogicalDisk(E:)\Disk Read Bytes/sec" path,
// which silently fails on non-English Windows (a prior cause of a 0-byte meter).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::monitor_util::{now_epoch, ps_escape, try_start, try_stop, StartOutcome};
use crate::usb_monitor::{
    correlate_volume_letter, list_usb_volumes, subscribe, DeviceIdentity, UsbEvent, UsbVolume,
};

// ── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_SAMPLE_INTERVAL_SECS: u64 = 5;
const MIN_SAMPLE_INTERVAL_SECS: u64 = 2;
const MAX_SAMPLE_INTERVAL_SECS: u64 = 60;

/// Reconcile cadence while some attached storage device still lacks a sampler —
/// typically because its volume hasn't finished mounting yet (Windows mounts the
/// volume a few seconds AFTER the USB device attaches). Retry quickly.
const RECONCILE_FAST_SECS: u64 = 4;
/// Reconcile cadence once every attached storage device is being sampled — only
/// watching for late arrivals, so back off to spare PowerShell spawns.
const RECONCILE_SLOW_SECS: u64 = 12;

/// Default large-transfer threshold: 500 MiB expressed in bytes.
/// The spec lists 500 MB default (not 4 GiB — spec settings.ts says 4 GiB
/// but the task brief says 500 MB; we use 500 MiB to honour the task brief).
const DEFAULT_LARGE_XFER_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;

/// Maximum device_key entries kept in the in-memory stats map (rolling cap).
const STATS_CAP: usize = 64;

// ── Types ────────────────────────────────────────────────────────────────────

/// Per-device accumulated transfer totals — the primary data shape returned
/// to the frontend by get_usb_transfer_stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStat {
    pub device_key: String,
    pub friendly_name: String,
    /// Total bytes read from the volume (approximate — see module-level docs).
    pub read_bytes: u64,
    /// Total bytes written to the volume (approximate).
    pub write_bytes: u64,
    /// Unix epoch seconds of the last sample that updated this entry.
    pub last_sample_epoch: i64,
}

/// Metering configuration — persisted in memory; boot hook pushes settings
/// into this via set_usb_metering_config (mirror of paste_monitor pattern).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeteringConfig {
    /// Sample interval in seconds, clamped to [2, 60].
    pub sample_interval_secs: u64,
    /// Enable the large-transfer warning toast.
    pub large_transfer_enabled: bool,
    /// Byte threshold for the large-transfer warning. 0 → reset to default.
    pub large_transfer_threshold_bytes: u64,
}

impl Default for MeteringConfig {
    fn default() -> Self {
        Self {
            sample_interval_secs: DEFAULT_SAMPLE_INTERVAL_SECS,
            large_transfer_enabled: false,
            large_transfer_threshold_bytes: DEFAULT_LARGE_XFER_THRESHOLD_BYTES,
        }
    }
}

/// Internal per-device sampler state.
struct ActiveSampler {
    friendly_name: String,
    /// Drive letter(s) being sampled for this device (e.g. ["E:"]).
    /// v1: we sample the first drive letter; multi-letter summing is a
    /// documented v2 improvement.
    #[allow(dead_code)]
    drive_letter: String,
    /// Accumulated read bytes.
    read_bytes: u64,
    /// Accumulated write bytes.
    write_bytes: u64,
    /// Whether the large-transfer read latch has fired this session.
    latch_read_fired: bool,
    /// Whether the large-transfer write latch has fired this session.
    latch_write_fired: bool,
    /// Abort handle for the background sampler task.
    handle: tauri::async_runtime::JoinHandle<()>,
}

// ── Process-lifetime singletons ───────────────────────────────────────────────

/// Master enable flag — mirrors the U-A pattern.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Monotonically-increasing stop epoch; sampler tasks check this to exit.
static RUN_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Active per-device-key samplers (only for currently-attached devices).
static ACTIVE: OnceLock<Mutex<HashMap<String, ActiveSampler>>> = OnceLock::new();

/// Finalized (detached) per-device-key transfer stats, ring-capped.
static STATS: OnceLock<Mutex<Vec<TransferStat>>> = OnceLock::new();

/// Runtime metering configuration.
static CONFIG: OnceLock<Mutex<MeteringConfig>> = OnceLock::new();

/// Handle of the event-listener task (subscribes to usb_monitor broadcast).
static LISTENER_HANDLE: OnceLock<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> =
    OnceLock::new();

/// Handle of the reconcile task — periodically attaches samplers to storage
/// devices whose volume mounted after the attach event / after metering started.
static RECONCILE_HANDLE: OnceLock<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> =
    OnceLock::new();

fn active_map() -> &'static Mutex<HashMap<String, ActiveSampler>> {
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn stats_vec() -> &'static Mutex<Vec<TransferStat>> {
    STATS.get_or_init(|| Mutex::new(Vec::new()))
}

fn config() -> &'static Mutex<MeteringConfig> {
    CONFIG.get_or_init(|| Mutex::new(MeteringConfig::default()))
}

fn listener_handle() -> &'static Mutex<Option<tauri::async_runtime::JoinHandle<()>>> {
    LISTENER_HANDLE.get_or_init(|| Mutex::new(None))
}

fn reconcile_handle() -> &'static Mutex<Option<tauri::async_runtime::JoinHandle<()>>> {
    RECONCILE_HANDLE.get_or_init(|| Mutex::new(None))
}

// ── Pure, unit-testable helpers ───────────────────────────────────────────────

/// Clamp a proposed sample interval to [MIN, MAX].
pub fn clamp_interval(n: u64) -> u64 {
    n.clamp(MIN_SAMPLE_INTERVAL_SECS, MAX_SAMPLE_INTERVAL_SECS)
}

/// Bytes transferred between two readings of a CUMULATIVE byte counter.
///
/// `prev` is the previous raw counter value (None on the very first sample);
/// `cur` is the current one. Returns 0 on the first sample and whenever the
/// counter went backwards — a reboot resets it, or it wrapped — so a spurious
/// multi-exabyte delta can never be produced from a reset.
pub fn cumulative_delta(prev: Option<u64>, cur: u64) -> u64 {
    match prev {
        Some(p) if cur >= p => cur - p,
        _ => 0,
    }
}

/// Decide whether to fire the large-transfer latch.
///
/// Returns true iff `current_bytes` has crossed `threshold` AND the latch
/// has not already fired (`latch_already_fired = false`). This ensures the
/// warning fires at most once per session per direction.
pub fn should_fire_latch(current_bytes: u64, threshold: u64, latch_already_fired: bool) -> bool {
    !latch_already_fired && current_bytes >= threshold
}

// ── Event listener task ───────────────────────────────────────────────────────

/// Spawns the background task that listens to U-A's broadcast channel to
/// finalize per-device totals on detach. Sampler STARTS are owned by the
/// reconcile loop (spawn_reconcile). Called by start_usb_metering.
fn spawn_listener(epoch: u64) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut rx = subscribe();
        loop {
            // Exit if a newer epoch was started (stop was called).
            if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                break;
            }
            match rx.recv().await {
                Ok(UsbEvent::Attached(_identity)) => {
                    // Sampler start is owned by the reconcile loop, which retries
                    // until the volume actually mounts (Windows mounts it a few
                    // seconds after attach — starting here would race that and
                    // permanently miss the device when no letter exists yet).
                }
                Ok(UsbEvent::Detached(key)) => {
                    if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                        break;
                    }
                    on_detach(&key);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    crate::log_message(
                        "warn",
                        &format!(
                            "[UsbMetering] broadcast lagged {} messages — some attach/detach events may have been missed",
                            n
                        ),
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    // U-A's monitor stopped; we can't receive events.
                    break;
                }
            }
        }
        crate::log_message("debug", "[UsbMetering] listener task exited");
    })
}

/// Ensure a sampler is running for one device, given a pre-fetched snapshot of
/// mounted USB volumes. Returns whether the device is COVERED: true when a
/// sampler is now running (or nothing needs metering — non-storage), false when
/// it's a storage device whose drive letter can't be resolved yet (volume still
/// mounting) — the reconcile loop uses that to retry quickly.
fn start_sampler_for_identity(
    app: &AppHandle,
    identity: &DeviceIdentity,
    epoch: u64,
    vols: &[UsbVolume],
) -> bool {
    if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
        return true; // Stopped — nothing left to cover.
    }
    // Only meter mass-storage devices (those that mount a volume).
    if !identity.is_mass_storage {
        return true;
    }
    // Correlate against the snapshot (no I/O here). No letter yet — e.g. the
    // volume hasn't finished mounting, a card reader has no media, or several
    // volumes are ambiguous — reported as uncovered so the caller retries.
    let Some(letter) = correlate_volume_letter(&identity.key, vols) else {
        return false;
    };

    let cfg = config().lock().unwrap().clone();
    let interval = clamp_interval(cfg.sample_interval_secs);

    // Hold the ACTIVE lock across the contains-check and the insert so two
    // racing callers can't both spawn a sampler for the same device and leak
    // one of the PowerShell children.
    let mut map = active_map().lock().unwrap();
    if map.contains_key(&identity.key) {
        return true;
    }
    let handle = tauri::async_runtime::spawn(run_sampler(
        app.clone(),
        identity.key.clone(),
        identity.friendly_name.clone(),
        letter.clone(),
        interval,
        epoch,
    ));
    map.insert(
        identity.key.clone(),
        ActiveSampler {
            friendly_name: identity.friendly_name.clone(),
            drive_letter: letter.clone(),
            read_bytes: 0,
            write_bytes: 0,
            latch_read_fired: false,
            latch_write_fired: false,
            handle,
        },
    );
    crate::log_message(
        "info",
        &format!(
            "[UsbMetering] sampler started for {} on {}",
            identity.key, letter
        ),
    );
    true
}

/// Reconcile loop: while metering is running, periodically make sure every
/// attached mass-storage device has a byte sampler. This replaces a one-shot
/// cold start, which silently metered NOTHING whenever the volume mounted after
/// the check ran (Windows mounts a drive a few seconds after the USB device
/// attaches — the exact window a one-shot start hits). Retries fast until all
/// devices are covered, then backs off to catch late arrivals.
fn spawn_reconcile(app: AppHandle, epoch: u64) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                break;
            }
            // Volume enumeration shells out to PowerShell — run it on the
            // blocking pool, once per cycle (not per device).
            let vols = tauri::async_runtime::spawn_blocking(list_usb_volumes)
                .await
                .unwrap_or_default();
            if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                break;
            }
            let mut all_covered = true;
            for identity in crate::usb_monitor::current_devices() {
                if !start_sampler_for_identity(&app, &identity, epoch, &vols) {
                    all_covered = false;
                }
            }
            let sleep_secs = if all_covered {
                RECONCILE_SLOW_SECS
            } else {
                RECONCILE_FAST_SECS
            };
            tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
        }
        crate::log_message("debug", "[UsbMetering] reconcile task exited");
    })
}

// ── Sampler task ──────────────────────────────────────────────────────────────

/// Background task: spawns a long-lived `Get-Counter -Continuous` child and
/// integrates its output into ACTIVE[device_key].
async fn run_sampler(
    app: AppHandle,
    device_key: String,
    friendly_name: String,
    drive_letter: String,
    interval_secs: u64,
    epoch: u64,
) {
    let escaped_letter = ps_escape(&drive_letter);

    // Poll the RAW LogicalDisk perf counters for THIS volume only. We read
    // Win32_PerfRawData_PerfDisk_LogicalDisk instead of Get-Counter because:
    //   • Its property names (DiskReadBytesPersec / DiskWriteBytesPersec) are
    //     English and locale-independent. Get-Counter's "\LogicalDisk(E:)\Disk
    //     Read Bytes/sec" path is LOCALIZED and silently fails on non-English
    //     Windows — a prime reason the meter could read 0.
    //   • For a bulk-count counter the RAW value is the CUMULATIVE byte total, so
    //     we take exact per-interval deltas rather than integrating a rate (no
    //     ±integration error, and a missed sample never loses bytes).
    // Instance Name is the drive letter with a colon, e.g. "E:". Each sample line:
    //   USBM|<cumulativeReadBytes>|<cumulativeWriteBytes>
    let ps_script = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
while ($true) {{
    $d = Get-CimInstance -ClassName Win32_PerfRawData_PerfDisk_LogicalDisk -Filter "Name='{letter}'"
    if ($null -ne $d) {{
        [Console]::Out.WriteLine("USBM|" + [string]$d.DiskReadBytesPersec + "|" + [string]$d.DiskWriteBytesPersec)
        [Console]::Out.Flush()
    }}
    Start-Sleep -Seconds {interval}
}}
"#,
        letter = escaped_letter,
        interval = interval_secs,
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
                &format!(
                    "[UsbMetering] failed to spawn Get-Counter for {} ({}): {}",
                    device_key, drive_letter, e
                ),
            );
            return;
        }
    };

    // Write the script to stdin and close it.
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(ps_script.as_bytes()).await;
        drop(stdin);
    }

    // Read stdout lines and accumulate cumulative-counter deltas.
    if let Some(stdout) = child.stdout.take() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Previous CUMULATIVE raw counter readings (None until the first sample).
        let mut prev_read: Option<u64> = None;
        let mut prev_write: Option<u64> = None;

        while let Ok(Some(line)) = lines.next_line().await {
            // Check epoch: stop if metering was stopped.
            if RUN_EPOCH.load(Ordering::Relaxed) != epoch {
                break;
            }

            if let Some(err) = line.strip_prefix("USBM_ERR|") {
                crate::log_message(
                    "warn",
                    &format!(
                        "[UsbMetering] perf-counter error for {} ({}): {}",
                        device_key, drive_letter, err
                    ),
                );
                break;
            }

            if !line.starts_with("USBM|") {
                continue;
            }

            // Parse: USBM|<cumulativeRead>|<cumulativeWrite>. Raw counters are
            // u64; tolerate an accidental float form via a fallback parse.
            let mut parts = line.splitn(3, '|');
            let _ = parts.next(); // "USBM"
            let read_str = parts.next().unwrap_or("0").trim();
            let write_str = parts.next().unwrap_or("0").trim();
            let cur_read = read_str
                .parse::<u64>()
                .ok()
                .or_else(|| read_str.parse::<f64>().ok().map(|v| v as u64));
            let cur_write = write_str
                .parse::<u64>()
                .ok()
                .or_else(|| write_str.parse::<f64>().ok().map(|v| v as u64));
            let (Some(cur_read), Some(cur_write)) = (cur_read, cur_write) else {
                continue; // Unparseable sample — skip.
            };

            let d_read = cumulative_delta(prev_read, cur_read);
            let d_write = cumulative_delta(prev_write, cur_write);
            prev_read = Some(cur_read);
            prev_write = Some(cur_write);

            // Read config first (separate lock) so we never nest CONFIG inside
            // the ACTIVE lock — keeps lock-acquisition order single and safe.
            let cfg = config().lock().unwrap().clone();
            // Update ACTIVE under the mutex.
            let (new_read, new_write, cfg_large_enabled, cfg_threshold, latch_read, latch_write) = {
                let mut map = active_map().lock().unwrap();
                let Some(sampler) = map.get_mut(&device_key) else {
                    break; // Device was detached and removed from the map.
                };
                sampler.read_bytes = sampler.read_bytes.saturating_add(d_read);
                sampler.write_bytes = sampler.write_bytes.saturating_add(d_write);
                (
                    sampler.read_bytes,
                    sampler.write_bytes,
                    cfg.large_transfer_enabled,
                    cfg.large_transfer_threshold_bytes,
                    sampler.latch_read_fired,
                    sampler.latch_write_fired,
                )
            };

            // Large-transfer latch check (outside the mutex to avoid deadlock
            // with the evidence + Tauri emit calls below).
            if cfg_large_enabled {
                let effective_threshold = if cfg_threshold == 0 {
                    DEFAULT_LARGE_XFER_THRESHOLD_BYTES
                } else {
                    cfg_threshold
                };

                if should_fire_latch(new_read, effective_threshold, latch_read) {
                    {
                        let mut map = active_map().lock().unwrap();
                        if let Some(s) = map.get_mut(&device_key) {
                            s.latch_read_fired = true;
                        }
                    }
                    fire_large_transfer_alert(
                        &app,
                        &device_key,
                        &friendly_name,
                        &drive_letter,
                        "read",
                        new_read,
                        effective_threshold,
                    );
                }

                if should_fire_latch(new_write, effective_threshold, latch_write) {
                    {
                        let mut map = active_map().lock().unwrap();
                        if let Some(s) = map.get_mut(&device_key) {
                            s.latch_write_fired = true;
                        }
                    }
                    fire_large_transfer_alert(
                        &app,
                        &device_key,
                        &friendly_name,
                        &drive_letter,
                        "written",
                        new_write,
                        effective_threshold,
                    );
                }
            }
        }
    }

    crate::log_message(
        "info",
        &format!(
            "[UsbMetering] sampler exited for {} ({})",
            device_key, drive_letter
        ),
    );
}

/// Emit the "usb-large-transfer" Tauri event, show a native toast, and
/// append an evidence record. Fires at most once per session per direction
/// (the latch in ActiveSampler ensures this — we never call this function
/// after the latch is set).
fn fire_large_transfer_alert(
    app: &AppHandle,
    device_key: &str,
    friendly_name: &str,
    drive_letter: &str,
    direction: &str,
    bytes: u64,
    threshold: u64,
) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now_rfc3339 = {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Minimal RFC3339 — no chrono dep: 1970-01-01T00:00:00Z style.
        format_epoch_rfc3339(secs)
    };

    let mb = bytes as f64 / (1024.0 * 1024.0);
    let label = if friendly_name.is_empty() {
        device_key.to_string()
    } else {
        friendly_name.to_string()
    };

    // Tauri frontend event.
    let payload = serde_json::json!({
        "deviceKey": device_key,
        "friendlyName": label,
        "driveLetter": drive_letter,
        "direction": direction,
        "bytes": bytes,
        "threshold": threshold,
        "detectedAt": now_rfc3339,
    });
    let _ = app.emit("usb-large-transfer", &payload);

    // Native desktop toast.
    let summary = format!(
        "USB large transfer ({} — {} {:.0} MB)",
        label,
        if direction == "read" { "read" } else { "wrote" },
        mb
    );
    let _ = crate::native_notify::show_native_notification(
        app,
        "USB Large Transfer Detected",
        &summary,
    );

    // Evidence ledger.
    let _ = crate::evidence::evidence_record(
        "monitor".to_string(),
        "warn".to_string(),
        format!(
            "USB large transfer on {}: {} {:.0} MB (threshold {:.0} MB)",
            drive_letter,
            if direction == "read" {
                "read"
            } else {
                "written"
            },
            mb,
            threshold as f64 / (1024.0 * 1024.0)
        ),
        None,
    );

    crate::log_message(
        "warn",
        &format!(
            "[UsbMetering] large-transfer alert: {} {} {} bytes (threshold {})",
            device_key, direction, bytes, threshold
        ),
    );
}

/// Minimal epoch→RFC3339 without chrono. Produces "YYYY-MM-DDTHH:MM:SSZ".
fn format_epoch_rfc3339(secs: u64) -> String {
    // Days since 1970-01-01
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Gregorian calendar computation (Meeus / Fliegel & Van Flandern algorithm).
    let z = days + 719468;
    let era = z / 146097; // z is u64 (always >= 0) → no negative-era branch needed
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mo <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", yr, mo, d, h, m, s)
}

// ── Detach handler ────────────────────────────────────────────────────────────

/// Called when U-A emits Detached(key). Aborts the sampler task, finalizes
/// totals into STATS, and drops the entry from ACTIVE.
fn on_detach(device_key: &str) {
    let finalized = {
        let mut map = active_map().lock().unwrap();
        map.remove(device_key)
    };

    let Some(sampler) = finalized else {
        return; // No active sampler for this key (non-storage device or not metering).
    };

    sampler.handle.abort();

    // Finalize into STATS (ring-capped at STATS_CAP).
    let stat = TransferStat {
        device_key: device_key.to_string(),
        friendly_name: sampler.friendly_name,
        read_bytes: sampler.read_bytes,
        write_bytes: sampler.write_bytes,
        last_sample_epoch: now_epoch(),
    };

    let mut sv = stats_vec().lock().unwrap();
    // If there's already an entry for this device_key, update it (accumulate).
    if let Some(existing) = sv.iter_mut().find(|s| s.device_key == device_key) {
        existing.read_bytes = existing.read_bytes.saturating_add(stat.read_bytes);
        existing.write_bytes = existing.write_bytes.saturating_add(stat.write_bytes);
        existing.last_sample_epoch = stat.last_sample_epoch;
        if !stat.friendly_name.is_empty() {
            existing.friendly_name = stat.friendly_name;
        }
    } else {
        sv.push(stat);
        // Cap at STATS_CAP by dropping the oldest entry.
        while sv.len() > STATS_CAP {
            sv.remove(0);
        }
    }

    crate::log_message(
        "info",
        &format!(
            "[UsbMetering] finalized totals for {}: read={} write={}",
            device_key, sampler.read_bytes, sampler.write_bytes
        ),
    );
}

/// Abort all active sampler tasks and clear the ACTIVE map.
/// Called on stop_usb_metering.
fn abort_all_samplers() {
    let mut map = active_map().lock().unwrap();
    for (key, sampler) in map.drain() {
        sampler.handle.abort();
        crate::log_message(
            "info",
            &format!("[UsbMetering] aborted sampler for {} on stop", key),
        );
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Start USB data-transfer metering. Subscribes to U-A's broadcast channel
/// and begins sampling all currently- and subsequently-attached mass-storage
/// devices. Idempotent — safe to call if already running.
///
/// Requires an active Pro licence.
#[tauri::command]
pub async fn start_usb_metering(app: AppHandle) -> Result<(), String> {
    crate::license::require_paid("USB metering")?;

    let epoch = match try_start(&RUNNING, &RUN_EPOCH) {
        StartOutcome::AlreadyRunning => return Ok(()), // Already running — idempotent.
        StartOutcome::Started(epoch) => epoch,
    };
    let handle = spawn_listener(epoch);

    {
        let mut lh = listener_handle().lock().unwrap();
        if let Some(old) = lh.take() {
            old.abort();
        }
        *lh = Some(handle);
    }

    // Reconcile loop: immediately (and then periodically) attach a sampler to
    // every mass-storage device that is or becomes attached — retrying until
    // each device's volume has actually mounted and its drive letter resolves.
    // A one-shot start here used to miss any volume that mounted a few seconds
    // late, silently metering nothing.
    {
        let rh_task = spawn_reconcile(app, epoch);
        let mut rh = reconcile_handle().lock().unwrap();
        if let Some(old) = rh.take() {
            old.abort();
        }
        *rh = Some(rh_task);
    }

    crate::log_message("debug", "[UsbMetering] started");
    Ok(())
}

/// Stop USB data-transfer metering. Kills the listener task and all active
/// per-device sampler tasks. Accumulated totals are preserved in STATS and
/// remain readable via get_usb_transfer_stats.
///
/// Requires an active Pro licence.
#[tauri::command]
pub async fn stop_usb_metering() -> Result<(), String> {
    crate::license::require_paid("USB metering")?;

    // try_stop bumps RUN_EPOCH on success so all running tasks see the stop signal.
    if !try_stop(&RUNNING, &RUN_EPOCH) {
        return Ok(()); // Not running — idempotent.
    }

    // Abort listener + reconcile tasks.
    {
        let mut lh = listener_handle().lock().unwrap();
        if let Some(h) = lh.take() {
            h.abort();
        }
    }
    {
        let mut rh = reconcile_handle().lock().unwrap();
        if let Some(h) = rh.take() {
            h.abort();
        }
    }

    // Abort all per-device sampler tasks.
    abort_all_samplers();

    crate::log_message("debug", "[UsbMetering] stopped");
    Ok(())
}

/// Returns whether metering is currently active. Ungated read.
#[tauri::command]
pub fn usb_metering_status() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Returns accumulated transfer stats for all devices seen since last clear.
/// Includes both currently-active (live, updated each interval) and finalized
/// (detached) device totals. Ungated read.
///
/// Result shape: Vec<TransferStat> where each entry has:
///   deviceKey: String, friendlyName: String,
///   readBytes: u64, writeBytes: u64, lastSampleEpoch: i64
#[tauri::command]
pub fn get_usb_transfer_stats() -> Vec<TransferStat> {
    let mut result: Vec<TransferStat> = {
        let sv = stats_vec().lock().unwrap();
        sv.clone()
    };

    // Merge in any currently-active (not yet detached) device totals.
    let active = active_map().lock().unwrap();
    for (key, sampler) in active.iter() {
        if let Some(existing) = result.iter_mut().find(|s| s.device_key == *key) {
            // For active devices, report the live running total.
            existing.read_bytes = sampler.read_bytes;
            existing.write_bytes = sampler.write_bytes;
            existing.last_sample_epoch = now_epoch();
        } else {
            result.push(TransferStat {
                device_key: key.clone(),
                friendly_name: sampler.friendly_name.clone(),
                read_bytes: sampler.read_bytes,
                write_bytes: sampler.write_bytes,
                last_sample_epoch: now_epoch(),
            });
        }
    }

    result
}

/// Sum approximate read+write bytes for one device key across active and finalized stats.
pub fn total_transfer_bytes_for_device(device_key: &str) -> u64 {
    get_usb_transfer_stats()
        .into_iter()
        .filter(|stat| stat.device_key == device_key)
        .map(|stat| stat.read_bytes.saturating_add(stat.write_bytes))
        .sum()
}

/// Clear all accumulated transfer statistics. Requires an active Pro licence.
#[tauri::command]
pub fn clear_usb_transfer_stats() -> Result<(), String> {
    crate::license::require_paid("USB metering")?;
    stats_vec().lock().unwrap().clear();
    // Also reset in-flight sampler totals so live devices start fresh.
    let mut map = active_map().lock().unwrap();
    for sampler in map.values_mut() {
        sampler.read_bytes = 0;
        sampler.write_bytes = 0;
        sampler.latch_read_fired = false;
        sampler.latch_write_fired = false;
    }
    Ok(())
}

/// Update metering configuration at runtime. Changes take effect on the next
/// sampler cycle; running samplers are NOT restarted (the new interval applies
/// only to newly-attached devices). Requires an active Pro licence.
///
/// Parameters:
///   sample_interval_secs: u64  — backend clamps to [2, 60]
///   large_transfer_enabled: bool
///   large_transfer_threshold_bytes: u64  — 0 resets to default (500 MiB)
#[tauri::command]
pub fn set_usb_metering_config(
    sample_interval_secs: Option<u64>,
    large_transfer_enabled: Option<bool>,
    large_transfer_threshold_bytes: Option<u64>,
) -> Result<(), String> {
    crate::license::require_paid("USB metering")?;
    let mut cfg = config().lock().unwrap();
    if let Some(n) = sample_interval_secs {
        cfg.sample_interval_secs = clamp_interval(n);
    }
    if let Some(v) = large_transfer_enabled {
        cfg.large_transfer_enabled = v;
    }
    if let Some(t) = large_transfer_threshold_bytes {
        cfg.large_transfer_threshold_bytes = if t == 0 {
            DEFAULT_LARGE_XFER_THRESHOLD_BYTES
        } else {
            t
        };
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── cumulative_delta() ───────────────────────────────────────────────────

    #[test]
    fn cumulative_delta_first_sample_is_zero() {
        // No baseline yet → we don't know how much of the cumulative total
        // predates metering, so the first sample contributes nothing.
        assert_eq!(cumulative_delta(None, 1_000_000), 0);
    }

    #[test]
    fn cumulative_delta_counts_forward_progress() {
        assert_eq!(cumulative_delta(Some(1_000), 1_500), 500);
    }

    #[test]
    fn cumulative_delta_no_change_is_zero() {
        assert_eq!(cumulative_delta(Some(42), 42), 0);
    }

    #[test]
    fn cumulative_delta_counter_reset_is_zero() {
        // Reboot / counter wrap: cur < prev must yield 0, never a huge bogus
        // delta from underflow.
        assert_eq!(cumulative_delta(Some(5_000_000_000), 100), 0);
    }

    #[test]
    fn cumulative_delta_accumulates_a_multi_gb_transfer() {
        // Simulate a 1.9 GB copy captured across three intervals.
        let mut total: u64 = 0;
        let mut prev: Option<u64> = None;
        for raw in [
            10_000_000_000u64,
            10_800_000_000,
            11_500_000_000,
            11_900_000_000,
        ] {
            total = total.saturating_add(cumulative_delta(prev, raw));
            prev = Some(raw);
        }
        // First reading is the baseline (0); the rest sum to 11.9G − 10.0G.
        assert_eq!(total, 1_900_000_000);
    }

    // ── clamp_interval() ─────────────────────────────────────────────────────

    #[test]
    fn clamp_interval_within_bounds_unchanged() {
        assert_eq!(clamp_interval(2), 2);
        assert_eq!(clamp_interval(5), 5);
        assert_eq!(clamp_interval(30), 30);
        assert_eq!(clamp_interval(60), 60);
    }

    #[test]
    fn clamp_interval_below_min_returns_min() {
        assert_eq!(clamp_interval(0), MIN_SAMPLE_INTERVAL_SECS);
        assert_eq!(clamp_interval(1), MIN_SAMPLE_INTERVAL_SECS);
    }

    #[test]
    fn clamp_interval_above_max_returns_max() {
        assert_eq!(clamp_interval(61), MAX_SAMPLE_INTERVAL_SECS);
        assert_eq!(clamp_interval(3600), MAX_SAMPLE_INTERVAL_SECS);
        assert_eq!(clamp_interval(u64::MAX), MAX_SAMPLE_INTERVAL_SECS);
    }

    // ── should_fire_latch() ───────────────────────────────────────────────────

    #[test]
    fn latch_fires_when_threshold_crossed_and_not_yet_set() {
        let threshold = 500 * 1024 * 1024u64; // 500 MiB
        assert!(should_fire_latch(threshold, threshold, false));
        assert!(should_fire_latch(threshold + 1, threshold, false));
    }

    #[test]
    fn latch_does_not_fire_below_threshold() {
        let threshold = 500 * 1024 * 1024u64;
        assert!(!should_fire_latch(threshold - 1, threshold, false));
        assert!(!should_fire_latch(0, threshold, false));
    }

    #[test]
    fn latch_does_not_fire_when_already_set() {
        let threshold = 500 * 1024 * 1024u64;
        // Even if bytes >> threshold, latch must not fire again.
        assert!(!should_fire_latch(threshold * 10, threshold, true));
        assert!(!should_fire_latch(threshold, threshold, true));
    }

    #[test]
    fn latch_fires_once_then_suppresses_sequence() {
        let threshold = 100u64;
        let latch_fired = false;

        // Below threshold — no fire.
        let fire1 = should_fire_latch(50, threshold, latch_fired);
        assert!(!fire1);

        // Crosses threshold — fire.
        let fire2 = should_fire_latch(150, threshold, latch_fired);
        assert!(fire2);
        let latch_fired = true; // Simulate setting the latch after fire.

        // Still above threshold — suppressed.
        let fire3 = should_fire_latch(200, threshold, latch_fired);
        assert!(!fire3);

        let fire4 = should_fire_latch(1_000_000, threshold, latch_fired);
        assert!(!fire4);
    }

    // ── u64 JSON round-trip (multi-GB values below 2^53 stay exact) ──────────

    #[test]
    fn u64_multi_gb_round_trips_through_json() {
        // 8 TB — well below 2^53 (≈ 9 PB), so exact as f64/JSON number.
        let stat = TransferStat {
            device_key: "USB:1234:5678:SERIAL".to_string(),
            friendly_name: "Test Drive".to_string(),
            read_bytes: 8 * 1024 * 1024 * 1024 * 1024u64, // 8 TiB
            write_bytes: 1024 * 1024 * 1024 * 1024u64,    // 1 TiB
            last_sample_epoch: 1_700_000_000,
        };
        let json = serde_json::to_string(&stat).unwrap();
        let restored: TransferStat = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.read_bytes, stat.read_bytes);
        assert_eq!(restored.write_bytes, stat.write_bytes);
    }

    #[test]
    fn u64_near_2_pow_53_round_trips() {
        // 2^53 = 9_007_199_254_740_992 bytes ≈ 8 PB.
        // Any realistic drive in the next decade stays well below this.
        let limit = (1u64 << 53) - 1;
        let stat = TransferStat {
            device_key: "test".to_string(),
            friendly_name: String::new(),
            read_bytes: limit,
            write_bytes: 0,
            last_sample_epoch: 0,
        };
        let json = serde_json::to_string(&stat).unwrap();
        let restored: TransferStat = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.read_bytes, limit);
    }

    // ── format_epoch_rfc3339 ──────────────────────────────────────────────────

    #[test]
    fn epoch_zero_is_unix_epoch() {
        assert_eq!(format_epoch_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_epoch_seconds_format_correctly() {
        // 2023-11-14T22:13:20Z = 1_700_000_000 (verified)
        assert_eq!(format_epoch_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    // ── ps_escape ────────────────────────────────────────────────────────────

    #[test]
    fn ps_escape_doubles_apostrophes() {
        assert_eq!(ps_escape("E:"), "E:");
        assert_eq!(ps_escape("it's"), "it''s");
        assert_eq!(ps_escape("a''b"), "a''''b");
    }
}
