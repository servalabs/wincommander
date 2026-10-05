// SPDX-License-Identifier: AGPL-3.0-or-later
//! Isolated native lifecycle reproduction; never reads installed preferences.
//! The small JSON preference fixture does not exercise the personal-settings service.
#[allow(dead_code)] // This lifecycle probe intentionally does not crash its renderer.
#[path = "../src/startup_window.rs"]
mod startup_window;
#[path = "../src/window_placement.rs"]
mod window_placement;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::Manager;

fn calc_mode_active(_: &tauri::AppHandle) -> bool {
    false
}
fn log_message_src(level: &str, _: &str, message: &str) {
    println!("{level}: {message}");
}
fn set_wincommander_window_icon(window: &tauri::WebviewWindow) {
    if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png")) {
        let _ = window.set_icon(icon);
    }
}
mod startup_trace {
    pub fn milestone(_: &tauri::AppHandle, _: &str) {}
}
struct Counts {
    focus: AtomicUsize,
    close: AtomicUsize,
}

fn main() {
    let profile = if let Some(parent) = std::env::args_os().nth(1) {
        tempfile::Builder::new()
            .prefix("webview-")
            .tempdir_in(parent)
    } else {
        tempfile::tempdir()
    }
    .expect("isolated profile");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.servalabs.wincommander.lifecycle-probe".into();
    tauri::Builder::default()
        .manage(startup_window::StartupWindow::new())
        .manage(Counts { focus: AtomicUsize::new(0), close: AtomicUsize::new(0) })
        .invoke_handler(tauri::generate_handler![startup_window::startup_window_ready])
        .register_uri_scheme_protocol("lifecycleprobe", |_, _| {
            tauri::http::Response::builder().header("Content-Type", "text/html").body(
                b"<html><body style='background:#0a0f12;color:white'>Disposable native lifecycle probe<script>if(document.body.offsetWidth)window.__TAURI_INTERNALS__.invoke('startup_window_ready',{isLight:false,generation:0});</script></body></html>".to_vec()
            ).unwrap()
        })
        .setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(app, "main",
                tauri::WebviewUrl::CustomProtocol("lifecycleprobe://localhost".parse().unwrap()))
                .title("Disposable WinCommander lifecycle probe").decorations(false).visible(false)
                .inner_size(1200.0, 800.0).data_directory(profile.path().to_path_buf()).build()?;
            let event_window = window.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    println!("close handler entered");
                    api.prevent_close();
                    let _ = event_window.set_skip_taskbar(true);
                    let _ = event_window.hide();
                    event_window.state::<Counts>().close.fetch_add(1, Ordering::Release);
                    println!("close handler completed");
                }
                tauri::WindowEvent::Focused(true) => {
                    println!("focus handler entered");
                    let is_calc = event_window.title().map(|title| title == "Calculator").unwrap_or(false);
                    let bytes: &[u8] = if is_calc { include_bytes!("../icons/calc.png") } else { include_bytes!("../icons/128x128.png") };
                    if let Ok(icon) = tauri::image::Image::from_bytes(bytes) { let _ = event_window.set_icon(icon); }
                    event_window.state::<Counts>().focus.fetch_add(1, Ordering::Release);
                    println!("focus handler completed");
                }
                _ => {}
            });
            let settings_file = profile.path().join("probe-preference.json");
            app.manage(profile);
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    tokio::time::timeout(std::time::Duration::from_secs(12), async {
                        while !handle.state::<startup_window::StartupWindow>().is_ready() {
                            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                        }
                    }).await.map_err(|_| "DOM readiness timed out")?;
                    for cycle in 0..8 {
                        let silent = cycle % 2 != 0;
                        std::fs::write(&settings_file, silent.to_string()).map_err(|e| e.to_string())?;
                        let persisted: bool = serde_json::from_slice(&std::fs::read(&settings_file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                        let args = vec!["probe.exe".into(), "--autostart".into()];
                        if !startup_window::should_start_hidden(&args, persisted) {
                            window_placement::show_maximized(&window).await?;
                        } else if window.is_visible().map_err(|e| e.to_string())? {
                            return Err("Silent intent exposed the window".to_string());
                        }
                        if startup_window::defer_reveal_until_ready(&window) { return Err("Ready tray open deferred".into()); }
                        window.set_skip_taskbar(false).map_err(|e| e.to_string())?;
                        window_placement::show_maximized(&window).await?;
                        window.set_focus().map_err(|e| e.to_string())?;
                        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
                        window.minimize().map_err(|e| e.to_string())?;
                        if startup_window::should_hide_on_tray_click(window.is_visible().map_err(|e| e.to_string())?, window.is_minimized().map_err(|e| e.to_string())?) {
                            return Err("Minimized tray click selected hide".into());
                        }
                        window_placement::show_maximized(&window).await?;
                        window.set_focus().map_err(|e| e.to_string())?;
                        let closes = handle.state::<Counts>().close.load(Ordering::Acquire);
                        window.close().map_err(|e| e.to_string())?;
                        tokio::time::timeout(std::time::Duration::from_secs(3), async {
                            while handle.state::<Counts>().close.load(Ordering::Acquire) == closes {
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            }
                        }).await.map_err(|_| "Close event handler stalled")?;
                        if window.is_visible().map_err(|e| e.to_string())? { return Err("Close did not hide".into()); }
                        println!("PASS cycle={cycle} silent={silent} native show/focus/minimize/restore/close");
                    }
                    let focus = handle.state::<Counts>().focus.load(Ordering::Acquire);
                    if focus == 0 { return Err("Focus handler was never exercised".into()); }
                    println!("PASS completed focus handlers={focus}, close handlers={}", handle.state::<Counts>().close.load(Ordering::Acquire));
                    Ok::<(), String>(())
                }.await;
                if let Err(error) = &result { eprintln!("FAIL {error}"); }
                handle.exit(if result.is_ok() { 0 } else { 1 });
            });
            Ok(())
        }).run(context).expect("isolated lifecycle runtime");
}
