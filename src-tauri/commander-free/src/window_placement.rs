// SPDX-License-Identifier: AGPL-3.0-or-later
//! Main-window placement is completed on its owning event loop, not a tray worker.

use tauri::Manager;

fn minimum_size(work_width: u32, work_height: u32, scale: f64) -> tauri::PhysicalSize<u32> {
    tauri::PhysicalSize::new(
        ((900.0 * scale).round() as u32).min(work_width),
        ((600.0 * scale).round() as u32).min(work_height),
    )
}

fn fits_work_area(
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    work: &tauri::PhysicalRect<i32, u32>,
) -> bool {
    let (x, y) = (i64::from(position.x), i64::from(position.y));
    x >= i64::from(work.position.x)
        && y >= i64::from(work.position.y)
        && x + i64::from(size.width) <= i64::from(work.position.x) + i64::from(work.size.width)
        && y + i64::from(size.height) <= i64::from(work.position.y) + i64::from(work.size.height)
}

fn place(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" && crate::calc_mode_active(window.app_handle()) {
        return Err("Window reveal cancelled because the session was locked.".into());
    }
    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("Windows could not identify the display for WinCommander.")?;
    let work = monitor.work_area();
    // At 175%, the old 600-DIP minimum can exceed the entire usable screen.
    window
        .set_min_size(Some(minimum_size(
            work.size.width,
            work.size.height,
            monitor.scale_factor(),
        )))
        .map_err(|e| e.to_string())?;
    window.unminimize().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.maximize().map_err(|e| e.to_string())?;
    for attempt in 0..2 {
        let size = window.inner_size().map_err(|e| e.to_string())?;
        let position = window.inner_position().map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let maximized = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::IsZoomed(
                window.hwnd().map_err(|e| e.to_string())?.0 as _,
            ) != 0
        };
        #[cfg(not(windows))]
        let maximized = window.is_maximized().map_err(|e| e.to_string())?;
        if maximized
            && window.is_visible().map_err(|e| e.to_string())?
            && fits_work_area(position, size, work)
        {
            return Ok(());
        }
        if attempt == 0 {
            // Discard stale hidden/minimized placement before asking Windows again.
            window.unmaximize().map_err(|e| e.to_string())?;
            window
                .set_position(work.position)
                .map_err(|e| e.to_string())?;
            window.maximize().map_err(|e| e.to_string())?;
        }
    }
    Err("Windows could not maximize WinCommander inside the display's usable area. Check display scaling and retry opening the window.".into())
}

pub(crate) async fn show_maximized(window: &tauri::WebviewWindow) -> Result<(), String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let target = window.clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    window
        .run_on_main_thread(move || {
            if std::time::Instant::now() >= deadline {
                return;
            }
            let _ = sender.send(place(&target));
        })
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(5), receiver)
        .await
        .map_err(|_| {
            "Windows did not respond while opening the window. Restart WinCommander and retry."
                .to_string()
        })?
        .map_err(|_| "The window event loop stopped before placement completed.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn high_dpi_minimum_never_extends_under_the_taskbar() {
        for scale in [1.0, 1.5, 1.75, 2.0] {
            let size = minimum_size(1920, 1000, scale);
            assert!(size.width <= 1920 && size.height <= 1000);
        }
        assert_eq!(minimum_size(1920, 1000, 1.75).height, 1000);
        assert_eq!(
            minimum_size(800, 560, 1.5),
            tauri::PhysicalSize::new(800, 560)
        );
    }
    #[test]
    fn verifies_client_area_with_bottom_or_left_taskbar_and_negative_monitor_origin() {
        let work = tauri::PhysicalRect {
            position: tauri::PhysicalPosition::new(-1840, 0),
            size: tauri::PhysicalSize::new(1840, 1000),
        };
        assert!(fits_work_area(work.position, work.size, &work));
        assert!(!fits_work_area(
            work.position,
            tauri::PhysicalSize::new(1840, 1080),
            &work
        ));
        assert!(!fits_work_area(
            tauri::PhysicalPosition::new(-1920, 0),
            work.size,
            &work
        ));
    }
}
