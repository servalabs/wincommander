// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reveal the desktop only after its startup content has been painted.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::Manager;

pub(crate) fn should_start_hidden(args: &[String]) -> bool {
    args.iter()
        .any(|arg| matches!(arg.as_str(), "--autostart" | "--minimized"))
}

pub(crate) fn should_hide_on_tray_click(visible: bool, minimized: bool) -> bool {
    visible && !minimized
}

pub(crate) struct StartupWindow {
    armed: AtomicBool,
    ready: AtomicBool,
    warned: AtomicBool,
    recovery_scheduled: AtomicBool,
    recovery_started: AtomicBool,
    document_generation: Mutex<u32>,
}

impl StartupWindow {
    pub(crate) fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            warned: AtomicBool::new(false),
            recovery_scheduled: AtomicBool::new(false),
            recovery_started: AtomicBool::new(false),
            document_generation: Mutex::new(0),
        }
    }

    pub(crate) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    fn defer_reveal(&self) -> bool {
        // Readiness and reveal intent must be ordered: an early tray click
        // cannot be lost between the document's acknowledgement and its reveal.
        let Ok(_generation) = self.document_generation.lock() else {
            return true;
        };
        if self.is_ready() {
            return false;
        }
        self.arm();
        true
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

    fn replace_document(
        &self,
        navigate: impl FnOnce(u32) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut generation = self
            .document_generation
            .lock()
            .map_err(|error| error.to_string())?;
        if self.is_ready() {
            return Ok(());
        }
        let next_generation = *generation + 1;
        // A failed navigation leaves the original document eligible to become ready.
        navigate(next_generation)?;
        *generation = next_generation;
        Ok(())
    }
}

/// Queue an explicit open request while keeping an unpainted window hidden.
pub(crate) fn defer_reveal_until_ready(window: &tauri::WebviewWindow) -> bool {
    let Some(state) = window.try_state::<StartupWindow>() else {
        return false;
    };
    if !state.defer_reveal() {
        return false;
    }
    if !state.recovery_scheduled.swap(true, Ordering::AcqRel) {
        let target = window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            recover_if_unready(&target).await;
        });
    }
    true
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
            if crate::calc_mode_active(target.app_handle()) {
                return;
            }
            if let Err(error) = state.replace_document(|generation| {
                let mut url = target.url().map_err(|error| error.to_string())?;
                url.query_pairs_mut()
                    .append_pair("wc-startup-generation", &generation.to_string());
                crate::log_message_src(
                    "warn",
                    "core",
                    "[Startup] retrying stalled interface document once",
                );
                target.navigate(url).map_err(|error| error.to_string())
            }) {
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
#[path = "startup_window_tests.rs"]
mod tests;
