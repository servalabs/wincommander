// SPDX-License-Identifier: AGPL-3.0-or-later
//! Crashes only this isolated probe's renderer; never opens installed app data.
#[path = "../src/startup_renderer.rs"]
mod startup_renderer;
#[path = "../src/startup_window.rs"]
mod startup_window;
#[path = "../src/window_placement.rs"]
mod window_placement;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tauri::Manager;
use webview2_com_runtime::{
    CallDevToolsProtocolMethodCompletedHandler, CapturePreviewCompletedHandler,
    Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
};
use windows_runtime::{
    core::w,
    Win32::{
        System::Com::{STREAM_SEEK_END, STREAM_SEEK_SET},
        UI::Shell::SHCreateMemStream,
    },
};

fn set_wincommander_window_icon(_: &tauri::WebviewWindow) {}
fn calc_mode_active(app: &tauri::AppHandle) -> bool {
    app.state::<AtomicBool>().load(Ordering::Acquire)
}
fn log_message_src(level: &str, _: &str, message: &str) {
    println!("{level}: {message}");
}
mod startup_trace {
    pub fn milestone(_: &tauri::AppHandle, _: &str) {}
}

#[tauri::command]
fn probe_ready(state: tauri::State<'_, AtomicU32>, generation: u32) {
    state.store(generation, Ordering::Release);
}

async fn capture(window: &tauri::WebviewWindow) -> Result<Vec<u8>, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |webview| unsafe {
            let stream = SHCreateMemStream(None).expect("probe screenshot memory stream");
            let read_stream = stream.clone();
            let callback = CapturePreviewCompletedHandler::create(Box::new(move |result| {
                let bytes = (|| {
                    result?;
                    let mut length = 0;
                    read_stream.Seek(0, STREAM_SEEK_END, Some(&mut length))?;
                    read_stream.Seek(0, STREAM_SEEK_SET, None)?;
                    let mut bytes = vec![0u8; length as usize];
                    read_stream
                        .Read(bytes.as_mut_ptr().cast(), bytes.len() as u32, None)
                        .ok()?;
                    Ok::<_, windows_runtime::core::Error>(bytes)
                })();
                let _ = send.send(bytes.map_err(|e| e.to_string()));
                Ok(())
            }));
            let result = webview.controller().CoreWebView2().and_then(|core| {
                core.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &stream,
                    &callback,
                )
            });
            if let Err(error) = result {
                eprintln!("Capture request failed: {error}");
            }
        })
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(10), receive)
        .await
        .map_err(|_| "renderer capture timed out".to_string())?
        .map_err(|_| "renderer capture channel closed".to_string())?
}

async fn verify_pixels(window: &tauri::WebviewWindow, name: &str) -> Result<(), String> {
    let bytes = capture(window).await?;
    let image = tauri::image::Image::from_bytes(&bytes).map_err(|e| e.to_string())?;
    let green = image
        .rgba()
        .chunks_exact(4)
        .filter(|p| p[..3] == [65, 165, 48])
        .count();
    let blue = image
        .rgba()
        .chunks_exact(4)
        .filter(|p| p[..3] == [18, 52, 86])
        .count();
    if green < 1000 || blue < 1000 {
        return Err(format!("paint missing: green={green} blue={blue}"));
    }
    if let Some(directory) = std::env::var_os("WC_RENDERER_PROBE_OUTPUT") {
        std::fs::write(
            std::path::PathBuf::from(directory).join(format!("{name}.png")),
            bytes,
        )
        .map_err(|e| e.to_string())?;
    }
    println!("PASS {name}: actual rendered pixels green={green} blue={blue}");
    Ok(())
}

async fn crash(window: &tauri::WebviewWindow) -> Result<(), String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |webview| unsafe {
            let result = webview.controller().CoreWebView2().and_then(|core| {
                core.CallDevToolsProtocolMethod(
                    w!("Page.crash"),
                    w!("{}"),
                    &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(()))),
                )
            });
            let _ = send.send(result.map_err(|e| e.to_string()));
        })
        .map_err(|e| e.to_string())?;
    receive.await.map_err(|e| e.to_string())?
}

async fn wait_ready(app: &tauri::AppHandle, generation: u32) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(12), async {
        while app.state::<AtomicU32>().load(Ordering::Acquire) != generation
            || !app.state::<startup_window::StartupWindow>().is_ready()
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| format!("generation {generation} never acknowledged DOM/readiness"))
}

fn main() {
    let hidden = std::env::args().any(|arg| arg == "--hidden");
    let minimized = std::env::args().any(|arg| arg == "--minimized");
    let locked = std::env::args().any(|arg| arg == "--locked");
    let baseline = std::env::args().any(|arg| arg == "--without-recovery");
    let profile = tempfile::tempdir().expect("isolated probe profile");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.servalabs.wincommander.renderer-probe".into();
    tauri::Builder::default()
        .manage(AtomicU32::new(u32::MAX))
        .manage(AtomicBool::new(false))
        .manage(startup_window::StartupWindow::new())
        .invoke_handler(tauri::generate_handler![probe_ready, startup_window::startup_window_ready])
        .register_uri_scheme_protocol("rendererprobe", |_, _| {
            tauri::http::Response::builder().header("Content-Type", "text/html").body(
                b"<html><body style='margin:0;background:#123456'><div id='paint' style='position:fixed;width:50vw;height:50vh;background:#41a530'></div><script>const generation=Number(new URLSearchParams(location.search).get('wc-startup-generation')||0);if(document.getElementById('paint').offsetWidth){window.__TAURI_INTERNALS__.invoke('startup_window_ready',{isLight:false,generation}).then(()=>window.__TAURI_INTERNALS__.invoke('probe_ready',{generation}));}</script></body></html>".to_vec()
            ).unwrap()
        })
        .setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(app, "main",
                tauri::WebviewUrl::CustomProtocol("rendererprobe://localhost".parse().unwrap()))
                .title("Disposable renderer recovery probe").decorations(false)
                .visible(false).inner_size(1000.0, 700.0).data_directory(profile.path().to_path_buf()).build()?;
            app.manage(profile);
            if !baseline { startup_renderer::install(&window)?; }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    wait_ready(&handle, 0).await?;
                    window_placement::show_maximized(&window).await?;
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    verify_pixels(&window, "before-crash").await?;
                    if hidden || locked { window.hide().map_err(|e| e.to_string())?; }
                    if minimized { window.minimize().map_err(|e| e.to_string())?; }
                    handle.state::<AtomicBool>().store(locked, Ordering::Release);
                    crash(&window).await?;
                    if locked {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        if handle.state::<startup_window::StartupWindow>().is_ready()
                            || window.is_visible().map_err(|e| e.to_string())?
                            || startup_window::startup_window_ready(window.clone(), false, Some(0)).await? {
                            return Err("locked renderer failure exposed or acknowledged old content".into());
                        }
                        println!("PASS locked renderer remains hidden; no reload or stale readiness accepted");
                        return Ok(());
                    }
                    if baseline {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        if !handle.state::<startup_window::StartupWindow>().is_ready() {
                            return Err("baseline unexpectedly invalidated readiness".into());
                        }
                        println!("PASS reproduced original defect: crashed renderer retains ready=true");
                        return Ok(());
                    }
                    wait_ready(&handle, 2).await?;
                    if window.is_visible().map_err(|e| e.to_string())? == (hidden || minimized) {
                        return Err("recovery did not preserve the prior reveal intent".into());
                    }
                    if startup_window::defer_reveal_until_ready(&window) {
                        return Err("recovered renderer did not accept tray reveal".into());
                    }
                    window_placement::show_maximized(&window).await?;
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    verify_pixels(&window, "after-crash").await?;
                    window.minimize().map_err(|e| e.to_string())?;
                    window_placement::show_maximized(&window).await?;
                    verify_pixels(&window, "after-restore").await?;
                    window.hide().map_err(|e| e.to_string())?;
                    crash(&window).await?;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    if handle.state::<startup_window::StartupWindow>().is_ready()
                        || window.is_visible().map_err(|e| e.to_string())? {
                        return Err("second renderer crash was exposed or retried".into());
                    }
                    println!("PASS second crash remains hidden with readiness invalidated; no reload loop");
                    Ok::<(), String>(())
                }.await;
                if let Err(error) = &result { eprintln!("FAIL {error}"); }
                handle.exit(if result.is_ok() { 0 } else { 1 });
            });
            Ok(())
        }).run(context).expect("isolated renderer probe runtime");
}
