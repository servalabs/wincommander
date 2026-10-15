// SPDX-License-Identifier: AGPL-3.0-or-later
//! A bounded notification-only host after Explorer's headless scrub finishes.
//! It does not acquire the main-instance mutex or start desktop services/pollers.
use crate::context_scrub_outcome::ScrubOutcome;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

struct HostState {
    presented: Mutex<Option<Instant>>,
    theme: String,
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
    presented.get_or_insert_with(Instant::now);
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

pub(crate) fn show(outcome: ScrubOutcome) {
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
    let (disabled, configured_decoy, theme) = match preferences() {
        Ok(preferences) => preferences,
        Err(code) => {
            crate::log_message_src(
                "warn",
                "core",
                &format!("[ContextScrub] {code}; display suppressed"),
            );
            return;
        }
    };
    if disabled || configured_decoy {
        return;
    }
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    // Debug contexts use the existing local Vite assets; their generated
    // context may not embed a release bundle. Release hosts use bundled assets.
    if !cfg!(debug_assertions) {
        context.config_mut().build.dev_url = None;
    }
    let exit_code = outcome.exit_code();
    let host = tauri::Builder::default()
        .manage(HostState {
            presented: Mutex::new(None),
            theme,
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
                handle.exit(exit_code);
                return Ok(());
            }
            tauri::async_runtime::spawn(async move {
                let deadline = Instant::now() + Duration::from_secs(45);
                loop {
                    let elapsed = handle
                        .state::<HostState>()
                        .presented
                        .lock()
                        .ok()
                        .and_then(|time| *time)
                        .map(|time| time.elapsed());
                    if elapsed.is_some_and(|elapsed| elapsed >= Duration::from_secs(10))
                        || Instant::now() >= deadline
                    {
                        handle.exit(exit_code);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            });
            Ok(())
        })
        .build(context);
    match host {
        Ok(app) => app.run(move |_app, _event| {}),
        Err(_) => crate::log_message_src(
            "warn",
            "core",
            "[ContextScrub] notification host unavailable",
        ),
    }
}
