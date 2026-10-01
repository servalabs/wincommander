// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reveal the desktop only after its startup content has been painted.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;

pub(crate) struct StartupWindow {
    armed: AtomicBool,
    ready: AtomicBool,
    warned: AtomicBool,
}

impl StartupWindow {
    pub(crate) fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            warned: AtomicBool::new(false),
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
}

async fn reveal_armed_startup_window(window: &tauri::WebviewWindow) -> Result<bool, String> {
    let Some(state) = window.try_state::<StartupWindow>() else {
        return Ok(false);
    };
    if !state.take_reveal() {
        return window.is_visible().map_err(|error| error.to_string());
    }
    if let Err(error) = crate::window_placement::show_maximized(window).await {
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

#[tauri::command]
pub(crate) async fn startup_window_ready(
    window: tauri::WebviewWindow,
    is_light: bool,
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
    window
        .state::<StartupWindow>()
        .ready
        .store(true, Ordering::Release);
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
}
