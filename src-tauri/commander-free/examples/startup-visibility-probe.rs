// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disposable hidden-startup visibility check. No installed settings or services.
#![windows_subsystem = "windows"]

use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    System::Threading::GetCurrentProcessId,
    UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, GWL_EXSTYLE, GWL_STYLE,
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
    let native_visible = unsafe { IsWindowVisible(window.hwnd().unwrap().0 as HWND) != 0 };
    serde_json::json!({
        "phase": phase,
        "tauriVisible": window.is_visible().unwrap_or(false),
        "nativeVisible": native_visible,
        "mainHwnd": format!("{:x}", window.hwnd().unwrap().0 as usize),
        "windows": windows,
    })
}

fn main() {
    let output = std::env::args().nth(1).expect("output JSON path required");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.servalabs.wincommander.visibility-probe".into();
    context.config_mut().build.dev_url = None;
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![probe_ready])
        .register_uri_scheme_protocol("visibility", |_, _| {
            tauri::http::Response::builder().header("Content-Type", "text/html")
                .body(b"<html><body style='background:#0a0f12;color:white'>Visibility probe<script>window.__TAURI_INTERNALS__.invoke('probe_ready');</script></body></html>".to_vec()).unwrap()
        })
        .setup(move |app| {
            let data_dir = std::path::PathBuf::from(&output).with_extension("webview");
            let window = tauri::WebviewWindowBuilder::new(app, "main",
                tauri::WebviewUrl::CustomProtocol("visibility://localhost".parse().unwrap()))
                .title("WinCommander visibility probe")
                .decorations(false).visible(false).inner_size(1200.0, 800.0)
                .background_color(tauri::window::Color(10, 15, 18, 255))
                .data_directory(data_dir).build()?;
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
                let mut snapshots = vec![snapshot(&window, "created")];
                let mut error = None;
                if tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    while !READY.load(Ordering::Acquire) {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }).await.is_err() {
                    error = Some("document readiness timeout");
                }
                snapshots.push(snapshot(&window, "document_ready"));
                let _ = window.set_background_color(Some(tauri::window::Color(10, 15, 18, 255)));
                let _ = window.set_title("WinCommander Pro");
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png")).unwrap();
                let _ = window.set_icon(icon);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                snapshots.push(snapshot(&window, "title_icon_background_set"));
                let _ = window.close();
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                snapshots.push(snapshot(&window, "close_hidden"));
                let _ = window.set_skip_taskbar(false);
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.maximize();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                snapshots.push(snapshot(&window, "tray_reveal"));
                let _ = window.close();
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                snapshots.push(snapshot(&window, "close_visible"));
                let result = serde_json::json!({ "process": std::process::id(), "error": error, "snapshots": snapshots });
                std::fs::write(&output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
                handle.exit(if error.is_none() { 0 } else { 1 });
            });
            Ok(())
        })
        .run(context).expect("disposable startup visibility probe");
}
