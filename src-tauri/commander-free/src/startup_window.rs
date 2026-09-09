// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reveal the desktop only after its startup content has been painted.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;

pub(crate) struct StartupWindow(AtomicBool);

impl StartupWindow {
    pub(crate) fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub(crate) fn arm(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn take_reveal(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

#[tauri::command]
pub(crate) fn startup_window_ready(
    window: tauri::WebviewWindow,
    is_light: bool,
) -> Result<bool, String> {
    if window.label() != "main" {
        return Err("Startup readiness is only available to the main window".into());
    }
    let Some(state) = window.try_state::<StartupWindow>() else {
        return Ok(false);
    };
    let background = if is_light {
        tauri::window::Color(255, 255, 255, 255)
    } else {
        tauri::window::Color(10, 15, 18, 255)
    };
    window
        .set_background_color(Some(background))
        .map_err(|error| error.to_string())?;
    if !state.take_reveal() {
        return window.is_visible().map_err(|error| error.to_string());
    }
    // Showing before maximizing delivers WebView2's resize on scaled displays.
    if let Err(error) = window.show() {
        state.arm();
        return Err(error.to_string());
    }
    let _ = window.maximize();
    crate::set_wincommander_window_icon(&window);
    let _ = window.set_focus();
    crate::startup_trace::milestone(window.app_handle(), "main_window_show_requested");
    Ok(true)
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
}
