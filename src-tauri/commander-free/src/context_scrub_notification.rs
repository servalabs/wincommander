// SPDX-License-Identifier: AGPL-3.0-or-later
//! Warm a hidden notification-only host while the one-shot scrub runs.
//! It does not acquire the main-instance mutex or start desktop services/pollers.
use crate::context_scrub_outcome::ScrubOutcome;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

struct HostState {
    presented: Mutex<Option<Instant>>,
    completed: Mutex<Option<Instant>>,
    theme: String,
    started: Instant,
}

#[tauri::command]
fn get_setting(path: String, state: State<'_, HostState>) -> Result<serde_json::Value, String> {
    if path != "app.theme" {
        return Err("setting is unavailable to this notification".into());
    }
    Ok(serde_json::Value::String(state.theme.clone()))
}

#[tauri::command]
fn present_notification_window(app: AppHandle, state: State<'_, HostState>) -> Result<(), String> {
    crate::native_notify::present_notification_window(app)?;
    let mut presented = state
        .presented
        .lock()
        .map_err(|_| "notification presentation unavailable")?;
    if presented.is_none() {
        let now = Instant::now();
        *presented = Some(now);
        let gap = state
            .completed
            .lock()
            .ok()
            .and_then(|time| *time)
            .map(|time| now.duration_since(time).as_millis());
        crate::log_message_src(
            "info",
            "core",
            &format!(
                "[ContextScrub] notification presented elapsed_ms={} verified_to_present_ms={}",
                now.duration_since(state.started).as_millis(),
                gap.unwrap_or(0)
            ),
        );
    }
    Ok(())
}

fn preferences() -> Result<(bool, bool, String), &'static str> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("scrub-notification-preferences".into())
        .spawn(move || {
            let _ = send.send(crate::settings::context_notification_preferences());
        })
        .map_err(|_| "preferences_worker_unavailable")?;
    receive
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => "preferences_worker_timeout",
            std::sync::mpsc::RecvTimeoutError::Disconnected => "preferences_worker_unavailable",
        })?
        .map_err(|error| crate::context_scrub_failure::preference_code(&error))
}

fn record_outcome(outcome: &ScrubOutcome) {
    crate::log_message_src(
        if outcome.exit_code() == 0 {
            "info"
        } else {
            "warn"
        },
        "core",
        &outcome.diagnostic(),
    );
    crate::log_message_src(
        if outcome.exit_code() == 0 {
            "info"
        } else {
            "warn"
        },
        "core",
        match outcome.state {
            crate::context_scrub_outcome::ScrubState::Completed => {
                "[ContextScrub] verified completion"
            }
            crate::context_scrub_outcome::ScrubState::Partial => {
                "[ContextScrub] partial output; sharing not verified"
            }
            crate::context_scrub_outcome::ScrubState::Failed => {
                "[ContextScrub] failed; sharing not verified"
            }
        },
    );
}

fn finish_worker(worker: std::thread::JoinHandle<i32>) -> i32 {
    worker.join().unwrap_or_else(|_| {
        crate::log_message_src(
            "warn",
            "core",
            "[ContextScrub] worker completion unconfirmed",
        );
        1
    })
}

pub(crate) fn run(paths: Vec<String>) -> i32 {
    let started = Instant::now();
    let (send, result) = std::sync::mpsc::sync_channel(1);
    let worker = match std::thread::Builder::new()
        .name("context-scrub".into())
        .spawn(move || {
            crate::context_menu_scrub::execute_cli(paths, move |outcome| {
                let _ = send.send((outcome, Instant::now()));
            })
        }) {
        Ok(worker) => worker,
        Err(_) => {
            crate::log_message_src("warn", "core", "[ContextScrub] worker unavailable");
            return 1;
        }
    };
    let initial = preferences();
    let theme = match initial {
        Ok((false, false, theme)) => theme,
        other => {
            if let Err(code) = other {
                crate::log_message_src(
                    "warn",
                    "core",
                    &format!("[ContextScrub] {code}; display suppressed"),
                );
            }
            let code = finish_worker(worker);
            if let Ok((outcome, _)) = result.recv() {
                record_outcome(&outcome);
            }
            return code;
        }
    };
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    if !cfg!(debug_assertions) {
        context.config_mut().build.dev_url = None;
    }
    let host = tauri::Builder::default()
        .manage(HostState {
            presented: Mutex::new(None),
            completed: Mutex::new(None),
            theme,
            started,
        })
        .invoke_handler(crate::ipc_boundary::guard(tauri::generate_handler![
            crate::native_notify::notification_renderer_ready,
            present_notification_window,
            get_setting,
            crate::log::write_log_record,
            crate::open_log_file,
        ]))
        .setup(move |app| {
            let handle = app.handle().clone();
            crate::native_notify::warm_up_notification_window(&handle);
            tauri::async_runtime::spawn(async move {
                let received = tauri::async_runtime::spawn_blocking(move || result.recv()).await;
                let (outcome, completed) = match received {
                    Ok(Ok(result)) => result,
                    _ => {
                        handle.exit(1);
                        return;
                    }
                };
                record_outcome(&outcome);
                if let Ok(mut time) = handle.state::<HostState>().completed.lock() {
                    *time = Some(completed);
                }
                // Recheck after long scrubs: a newer suppression choice must win.
                let fresh = tauri::async_runtime::spawn_blocking(preferences).await;
                match fresh {
                    Ok(Ok((false, false, _))) => {}
                    other => {
                        if let Ok(Err(code)) = other {
                            crate::log_message_src(
                                "warn",
                                "core",
                                &format!("[ContextScrub] {code}; display suppressed"),
                            );
                        }
                        handle.exit(outcome.exit_code());
                        return;
                    }
                }
                let shown = crate::native_notify::show_operation_notification(
                    &handle,
                    outcome.title(),
                    &outcome.message(),
                    outcome.severity(),
                    "Metadata Scrubber",
                );
                if shown.is_err() {
                    crate::log_message_src(
                        "warn",
                        "core",
                        "[ContextScrub] notification delivery unavailable",
                    );
                    handle.exit(outcome.exit_code());
                    return;
                }
                // This is a delivery deadline, never a deadline for scrubbing.
                let deadline = Instant::now() + Duration::from_secs(45);
                loop {
                    let presented = handle
                        .state::<HostState>()
                        .presented
                        .lock()
                        .ok()
                        .and_then(|time| *time)
                        .is_some();
                    // Detailed scrub receipts need reading time. Once shown,
                    // only a verified hide (Dismiss or auto-hide) ends delivery;
                    // unknown visibility retains the bounded delivery deadline.
                    let hidden = presented
                        && handle
                            .get_webview_window("notification-alerts")
                            .and_then(|window| window.is_visible().ok())
                            == Some(false);
                    if hidden || Instant::now() >= deadline {
                        handle.exit(outcome.exit_code());
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            });
            Ok(())
        })
        .build(context);
    match host {
        Ok(app) => {
            // run() exits the process directly and would skip the worker join.
            let _ = app.run_return(move |_app, _event| {});
        }
        Err(_) => crate::log_message_src(
            "warn",
            "core",
            "[ContextScrub] notification host unavailable",
        ),
    }
    // Even a failed or suppressed UI must not abandon an operation or its cleanup.
    finish_worker(worker)
}
