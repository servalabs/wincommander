// SPDX-License-Identifier: AGPL-3.0-or-later
//! Always-on smart-glasses monitor: one supervised BLE watcher that restarts
//! when the radio returns, plus a bounded recent-detections ring.

use super::alerts::GlassesDetection;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

const RECENT_CAP: usize = 30;

struct Running {
    stop: Arc<AtomicBool>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static RADIO_STATE: Mutex<&'static str> = Mutex::new("stopped");
static RECENT: Mutex<VecDeque<GlassesDetection>> = Mutex::new(VecDeque::new());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlassesMonitorStatus {
    pub running: bool,
    /// stopped | starting | scanning | radio_unavailable | error
    pub radio: &'static str,
    pub recent_count: usize,
}

fn set_radio(state: &'static str) {
    if let Ok(mut r) = RADIO_STATE.lock() {
        *r = state;
    }
}

fn record(app: &AppHandle, detection: GlassesDetection) {
    use tauri::Emitter;
    if let Ok(mut recent) = RECENT.lock() {
        if recent.len() == RECENT_CAP {
            recent.pop_front();
        }
        recent.push_back(detection.clone());
    }
    let _ = app.emit("glasses-detected", &detection);
    let body = format!(
        "{} detected nearby (signal {} dBm, roughly within 10 m). \
         Camera glasses can photograph your screen; check who is around you.",
        super::alerts::vendor_label(detection.vendor),
        detection.rssi
    );
    if let Err(e) = crate::native_notify::show_native_notification(
        app,
        "WinCommander · Smart glasses nearby",
        &body,
    ) {
        crate::log_message(
            "warn",
            &format!("[GlassesMonitor] notification failed: {e}"),
        );
    }
}

#[cfg(windows)]
fn supervise(app: AppHandle, stop: Arc<AtomicBool>) {
    use super::alerts::AlertGate;
    use super::radio::{is_radio_unavailable, Watcher};
    use std::time::{Duration, Instant};

    const RETRY: Duration = Duration::from_secs(30);
    let epoch = Instant::now();
    let gate = Arc::new(Mutex::new(AlertGate::default()));
    while !stop.load(Ordering::SeqCst) {
        set_radio("starting");
        let (gate, app_for_events) = (Arc::clone(&gate), app.clone());
        let started = Watcher::start(true, move |obs| {
            let now_ms = epoch.elapsed().as_millis() as u64;
            let hit = gate
                .lock()
                .ok()
                .and_then(|mut g| g.observe(&obs, now_ms, || chrono::Utc::now().to_rfc3339()));
            if let Some(detection) = hit {
                record(&app_for_events, detection);
            }
        });
        match started {
            Ok(watcher) => {
                set_radio("scanning");
                while !stop.load(Ordering::SeqCst) {
                    if let Some(reason) = watcher.stop_reason() {
                        set_radio(if is_radio_unavailable(&reason) {
                            "radio_unavailable"
                        } else {
                            "error"
                        });
                        crate::log_message(
                            "warn",
                            &format!("[GlassesMonitor] watcher stopped: {reason}"),
                        );
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            Err(reason) => {
                set_radio("error");
                crate::log_message("warn", &format!("[GlassesMonitor] start failed: {reason}"));
            }
        }
        // Wait for the radio to come back without blocking a stop request.
        let resume = Instant::now() + RETRY;
        while !stop.load(Ordering::SeqCst) && Instant::now() < resume {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    set_radio("stopped");
}

/// Idempotent: a second start keeps the existing watcher.
#[tauri::command]
pub async fn start_glasses_monitor(app: AppHandle) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Smart-glasses detection needs Windows Bluetooth LE".into())
    }
    #[cfg(windows)]
    {
        let mut running = RUNNING.lock().map_err(|_| "monitor state poisoned")?;
        if running.is_some() {
            return Ok(());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("glasses-monitor".into())
            .spawn(move || supervise(app, thread_stop))
            .map_err(|e| format!("could not start glasses monitor: {e}"))?;
        *running = Some(Running { stop });
        Ok(())
    }
}

#[tauri::command]
pub async fn stop_glasses_monitor() -> Result<(), String> {
    if let Some(running) = RUNNING.lock().map_err(|_| "monitor state poisoned")?.take() {
        running.stop.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn glasses_monitor_status() -> Result<GlassesMonitorStatus, String> {
    let running = RUNNING.lock().map(|r| r.is_some()).unwrap_or(false);
    Ok(GlassesMonitorStatus {
        running,
        radio: if running {
            RADIO_STATE.lock().map(|r| *r).unwrap_or("error")
        } else {
            "stopped"
        },
        recent_count: RECENT.lock().map(|r| r.len()).unwrap_or(0),
    })
}

#[tauri::command]
pub async fn get_glasses_monitor_recent() -> Result<Vec<GlassesDetection>, String> {
    Ok(RECENT
        .lock()
        .map(|r| r.iter().rev().cloned().collect())
        .unwrap_or_default())
}

#[tauri::command]
pub async fn clear_glasses_monitor_recent() -> Result<(), String> {
    if let Ok(mut r) = RECENT.lock() {
        r.clear();
    }
    Ok(())
}
