// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disposable hidden-startup visibility check. No installed settings or services.
#![windows_subsystem = "windows"]

#[path = "../src/startup_visibility.rs"]
mod startup_visibility;

use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    System::Threading::GetCurrentProcessId,
    UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, IsZoomed, ShowWindow, GWL_EXSTYLE, GWL_STYLE, SW_SHOWNORMAL,
    },
};

static READY: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn probe_ready() {
    READY.store(true, Ordering::Release);
}

unsafe extern "system" fn enumerate(hwnd: HWND, data: LPARAM) -> i32 {
    let mut process = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut process) };
    if process != unsafe { GetCurrentProcessId() } {
        return 1;
    }
    let mut title = [0u16; 256];
    let mut class = [0u16; 128];
    let title_len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
    let class_len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    let output = unsafe { &mut *(data as *mut Vec<serde_json::Value>) };
    output.push(serde_json::json!({
        "hwnd": format!("{:x}", hwnd as usize), "thread": thread,
        "title": String::from_utf16_lossy(&title[..title_len as usize]),
        "class": String::from_utf16_lossy(&class[..class_len as usize]),
        "visible": unsafe { IsWindowVisible(hwnd) != 0 },
        "style": format!("{:x}", unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) }),
        "exstyle": format!("{:x}", unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) }),
    }));
    1
}

fn snapshot(window: &tauri::WebviewWindow, phase: &str) -> serde_json::Value {
    let mut windows = Vec::<serde_json::Value>::new();
    unsafe { EnumWindows(Some(enumerate), &mut windows as *mut _ as LPARAM) };
    let hwnd = window.hwnd().ok().map(|handle| handle.0 as HWND);
    serde_json::json!({
        "phase": phase,
        "tauriVisible": window.is_visible().unwrap_or(false),
        "nativeVisible": hwnd.map(|handle| unsafe { IsWindowVisible(handle) != 0 }),
        "nativeMaximized": hwnd.map(|handle| unsafe { IsZoomed(handle) != 0 }),
        "mainHwnd": hwnd.map(|handle| format!("{:x}", handle as usize)),
        "windows": windows,
    })
}

fn require_visibility(state: &serde_json::Value, visible: bool, errors: &mut Vec<String>) {
    if state["nativeVisible"].as_bool() != Some(visible) {
        errors.push(format!(
            "{}: expected nativeVisible={visible}, got {}",
            state["phase"], state["nativeVisible"]
        ));
    }
}

fn main() {
    let Some(output) = std::env::args().nth(1) else {
        return;
    };
    let without_guard = std::env::args().any(|arg| arg == "--without-native-guard");
    let error_output = output.clone();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.servalabs.wincommander.visibility-probe".into();
    context.config_mut().build.dev_url = None;
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![probe_ready])
        .register_uri_scheme_protocol("visibility", |_, _| {
            tauri::http::Response::builder().header("Content-Type", "text/html")
                .body(b"<html><body style='background:#0a0f12;color:white'>Visibility probe<script>window.__TAURI_INTERNALS__.invoke('probe_ready');</script></body></html>".to_vec()).unwrap()
        })
        .setup(move |app| {
            let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
            let data_dir = std::path::PathBuf::from(&output).with_extension(format!("{}.{unique}.webview", std::process::id()));
            let window = tauri::WebviewWindowBuilder::new(app, "main",
                tauri::WebviewUrl::CustomProtocol("visibility://localhost".parse().unwrap()))
                .title("WinCommander visibility probe")
                .decorations(false).visible(false).inner_size(1200.0, 800.0)
                .background_color(tauri::window::Color(10, 15, 18, 255))
                .data_directory(data_dir).build()?;
            let mut snapshots = vec![snapshot(&window, "created")];
            let mut errors = Vec::new();
            // Inject native-visible/Tao-cached-hidden divergence independently
            // of its field trigger. Deliberately bypass Tao for this negative control.
            unsafe { ShowWindow(window.hwnd()?.0 as HWND, SW_SHOWNORMAL); }
            snapshots.push(snapshot(&window, "injected_native_show"));
            require_visibility(&snapshots[snapshots.len() - 1], true, &mut errors);
            if let Err(error) = window.hide() { errors.push(format!("Tauri hide: {error}")); }
            // Observation only: a future Tao fix may make this negative control hide.
            snapshots.push(snapshot(&window, "tauri_hide_negative_control"));
            if !without_guard {
                if let Err(error) = startup_visibility::enforce_hidden_before_setup(&window) {
                    errors.push(format!("native guard: {error}"));
                }
            }
            snapshots.push(snapshot(&window, "startup_visibility_checked"));
            require_visibility(&snapshots[snapshots.len() - 1], false, &mut errors);
            let close_window = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = close_window.hide();
                }
            });
            let handle = app.handle().clone();
            let output = output.clone();
            tauri::async_runtime::spawn(async move {
                if tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    while !READY.load(Ordering::Acquire) {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }).await.is_err() {
                    errors.push("document readiness timeout".into());
                }
                snapshots.push(snapshot(&window, "document_ready"));
                require_visibility(&snapshots[snapshots.len() - 1], false, &mut errors);
                let _ = window.set_background_color(Some(tauri::window::Color(10, 15, 18, 255)));
                let _ = window.set_title("WinCommander Pro");
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png")).unwrap();
                let _ = window.set_icon(icon);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                snapshots.push(snapshot(&window, "title_icon_background_set"));
                require_visibility(&snapshots[snapshots.len() - 1], false, &mut errors);
                let _ = window.close();
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                snapshots.push(snapshot(&window, "close_hidden"));
                require_visibility(&snapshots[snapshots.len() - 1], false, &mut errors);
                let _ = window.set_skip_taskbar(false);
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.maximize();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                snapshots.push(snapshot(&window, "tray_reveal"));
                require_visibility(&snapshots[snapshots.len() - 1], true, &mut errors);
                if snapshots[snapshots.len() - 1]["nativeMaximized"].as_bool() != Some(true) {
                    errors.push("tray_reveal: expected nativeMaximized=true".into());
                }
                let _ = window.close();
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                snapshots.push(snapshot(&window, "close_visible"));
                require_visibility(&snapshots[snapshots.len() - 1], false, &mut errors);
                let result = serde_json::json!({ "process": std::process::id(), "withoutNativeGuard": without_guard, "errors": errors, "snapshots": snapshots });
                let written = serde_json::to_vec_pretty(&result).ok().is_some_and(|bytes| std::fs::write(&output, bytes).is_ok());
                handle.exit(if errors.is_empty() && written { 0 } else { 1 });
            });
            Ok(())
        })
        .run(context);
    if let Err(error) = result {
        let report = serde_json::json!({"errors": [format!("probe initialization: {error}")]});
        let _ = std::fs::write(error_output, report.to_string());
        std::process::exit(1);
    }
}
