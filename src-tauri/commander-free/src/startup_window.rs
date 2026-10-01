// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reveal the desktop only after its startup content has been painted.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::Manager;

pub(crate) struct StartupWindow {
    armed: AtomicBool,
    ready: AtomicBool,
    warned: AtomicBool,
    recovery_started: AtomicBool,
    document_generation: Mutex<u32>,
}

impl StartupWindow {
    pub(crate) fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            warned: AtomicBool::new(false),
            recovery_started: AtomicBool::new(false),
            document_generation: Mutex::new(0),
        }
    }

    pub(crate) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    fn take_reveal(&self) -> bool {
        self.armed.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    fn needs_warning(&self) -> bool {
        self.armed.load(Ordering::Acquire)
            && !self.is_ready()
            && !self.warned.swap(true, Ordering::AcqRel)
    }

    fn begin_recovery(&self) -> bool {
        self.armed.load(Ordering::Acquire)
            && !self.is_ready()
            && !self.recovery_started.swap(true, Ordering::AcqRel)
    }

    fn accept_ready(&self, generation: u32) -> bool {
        let Ok(current) = self.document_generation.lock() else {
            return false;
        };
        if *current != generation {
            return false;
        }
        self.ready.store(true, Ordering::Release);
        true
    }
}

async fn reveal_armed_startup_window(window: &tauri::WebviewWindow) -> Result<bool, String> {
    let Some(state) = window.try_state::<StartupWindow>() else {
        return Ok(false);
    };
    if !state.take_reveal() {
        return window.is_visible().map_err(|error| error.to_string());
    }
    let mut result = crate::window_placement::show_maximized(window).await;
    // A display/session transition can briefly reject placement. Let Windows
    // finish processing it before reporting a persistent failure.
    if result.is_err() {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        result = crate::window_placement::show_maximized(window).await;
    }
    if let Err(error) = result {
        state.arm();
        if !state.warned.swap(true, Ordering::AcqRel) {
            show_native_startup_error(&format!("WinCommander could not open its window. {error}"));
        }
        return Err(error);
    }
    crate::set_wincommander_window_icon(window);
    let _ = window.set_focus();
    crate::startup_trace::milestone(window.app_handle(), "main_window_show_requested");
    Ok(true)
}

/// Recover a stalled first document once, without resetting settings or
/// revealing an unpainted/locked window. Never enter an infinite reload loop.
pub(crate) async fn recover_if_unready(window: &tauri::WebviewWindow) {
    let Some(state) = window.try_state::<StartupWindow>() else {
        return;
    };
    if !state.begin_recovery() {
        return;
    }
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        // Readiness may arrive while this callback is queued.
        if let Some(state) = target.try_state::<StartupWindow>() {
            // Serialize generation change with readiness so a delayed IPC from
            // the previous document cannot reveal its unpainted replacement.
            let Ok(mut generation) = state.document_generation.lock() else {
                return;
            };
            if state.is_ready() || crate::calc_mode_active(target.app_handle()) {
                return;
            }
            let Ok(mut url) = target.url() else {
                return;
            };
            *generation += 1;
            url.query_pairs_mut()
                .append_pair("wc-startup-generation", &generation.to_string());
            crate::log_message_src(
                "warn",
                "core",
                "[Startup] retrying stalled interface document once",
            );
            if let Err(error) = target.navigate(url) {
                crate::log_message_src(
                    "error",
                    "core",
                    &format!("[Startup] interface retry failed: {error}"),
                );
            }
        }
    });
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    if !crate::calc_mode_active(window.app_handle()) {
        warn_if_unready(window);
    }
}

#[tauri::command]
pub(crate) async fn startup_window_ready(
    window: tauri::WebviewWindow,
    is_light: bool,
    generation: Option<u32>,
) -> Result<bool, String> {
    if window.label() != "main" {
        return Err("Startup readiness is only available to the main window".into());
    }
    if window.try_state::<StartupWindow>().is_none() {
        return Ok(false);
    }
    let background = if is_light {
        tauri::window::Color(255, 255, 255, 255)
    } else {
        tauri::window::Color(10, 15, 18, 255)
    };
    window
        .set_background_color(Some(background))
        .map_err(|error| error.to_string())?;
    if !window
        .state::<StartupWindow>()
        .accept_ready(generation.unwrap_or(0))
    {
        return Ok(false);
    }
    reveal_armed_startup_window(&window).await
}

/// A broken renderer cannot draw its own error. Never expose its blank HWND.
pub(crate) fn warn_if_unready(window: &tauri::WebviewWindow) {
    let Some(state) = window.try_state::<StartupWindow>() else {
        return;
    };
    if !state.needs_warning() {
        return;
    }
    crate::log_message_src(
        "error",
        "core",
        "[Startup] interface readiness was not received; blank window remains hidden",
    );
    show_native_startup_error("WinCommander could not finish loading its interface. The empty window has been kept hidden. Close WinCommander from its tray menu and reopen it. If this continues, repair or reinstall the application.");
}

fn show_native_startup_error(message: &str) {
    let message = message.to_owned();
    tauri::async_runtime::spawn_blocking(move || show_initialization_error(&message));
}

pub(crate) fn show_initialization_error(message: &str) {
    show_error_dialog(
        "WinCommander startup",
        &format!("{message}\nYour saved settings have not been reset. Close this message and retry opening WinCommander."),
    );
}

// Call before GUI setup or on a blocking worker; never wait for a dialog on the event thread.
pub(crate) fn show_error_dialog(title: &str, message: &str) {
    #[cfg(windows)]
    {
        let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    crate::log_message_src("error", "core", &format!("{title}: {message}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_does_not_reveal_a_suppressed_launch() {
        let state = StartupWindow::new();
        assert!(!state.take_reveal());
    }

    #[test]
    fn repeated_readiness_does_not_reopen_the_window() {
        let state = StartupWindow::new();
        state.arm();
        assert!(state.take_reveal());
        assert!(!state.take_reveal());
    }

    #[test]
    fn timeout_warns_once_without_consuming_a_late_reveal() {
        let state = StartupWindow::new();
        assert!(!state.needs_warning());
        state.arm();
        assert!(state.needs_warning());
        assert!(!state.needs_warning());
        state.ready.store(true, Ordering::Release);
        assert!(state.take_reveal());
    }

    #[test]
    fn ready_or_suppressed_windows_never_show_timeout_errors() {
        let state = StartupWindow::new();
        assert!(!state.needs_warning());
        state.arm();
        state.ready.store(true, Ordering::Release);
        assert!(!state.needs_warning());
    }

    #[test]
    fn recovery_is_once_only_and_preserves_late_readiness() {
        let state = StartupWindow::new();
        assert!(!state.begin_recovery());
        state.arm();
        assert!(state.begin_recovery());
        assert!(!state.begin_recovery());
        state.ready.store(true, Ordering::Release);
        assert!(!state.needs_warning());
        assert!(state.take_reveal());
    }

    #[test]
    fn a_ready_document_is_never_reloaded() {
        let state = StartupWindow::new();
        state.arm();
        state.ready.store(true, Ordering::Release);
        assert!(!state.begin_recovery());
    }

    #[test]
    fn old_document_readiness_cannot_reveal_a_replacement() {
        let state = StartupWindow::new();
        state.arm();
        *state.document_generation.lock().unwrap() = 1;
        assert!(!state.accept_ready(0));
        assert!(!state.is_ready());
        assert!(state.accept_ready(1));
        assert!(state.is_ready());
    }
}
