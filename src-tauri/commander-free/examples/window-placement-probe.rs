// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disposable native-window check; no application settings, tasks or services.
#[path = "../src/window_placement.rs"]
mod window_placement;

fn calc_mode_active(_: &tauri::AppHandle) -> bool {
    false
}

fn main() {
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.servalabs.wincommander.placement-probe".into();
    tauri::Builder::default()
        .register_uri_scheme_protocol("placement", |_, _| {
            tauri::http::Response::builder().header("Content-Type", "text/html")
                .body("<html><body style='background:#0a0f12;color:white;font:24px system-ui'><p>WinCommander window placement test</p><p>This disposable window closes automatically.</p></body></html>".as_bytes()).unwrap()
        })
        .setup(|app| {
            let window = tauri::WebviewWindowBuilder::new(
                app, "placement-probe",
                tauri::WebviewUrl::CustomProtocol("placement://localhost".parse().unwrap()),
            ).title("WinCommander placement test")
                .decorations(false).visible(false).inner_size(1200.0, 800.0)
                .min_inner_size(900.0, 600.0).build()?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    window_placement::show_maximized(&window).await?;
                    println!("PASS cold hidden reveal: visible={} maximized={} scale={} elevated={}", window.is_visible().unwrap_or(false), window.is_maximized().unwrap_or(false), window.scale_factor().unwrap_or(0.0), unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 });
                    window.minimize().map_err(|e| e.to_string())?;
                    window_placement::show_maximized(&window).await?;
                    println!("PASS minimized reveal");
                    window.hide().map_err(|e| e.to_string())?;
                    window_placement::show_maximized(&window).await?;
                    println!("PASS tray-style hidden reveal");
                    let monitor = window.current_monitor().map_err(|e| e.to_string())?.ok_or("No monitor")?;
                    let work = monitor.work_area();
                    window.unmaximize().map_err(|e| e.to_string())?;
                    window.set_min_size(Some(tauri::PhysicalSize::new(work.size.width + 200, work.size.height + 200))).map_err(|e| e.to_string())?;
                    window_placement::show_maximized(&window).await?;
                    println!("PASS oversized previous minimum repaired; client={:?} size={:?} work={:?}", window.inner_position().unwrap(), window.inner_size().unwrap(), work);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    Ok::<(), String>(())
                }.await;
                if let Err(error) = &result { eprintln!("FAIL {error}"); }
                handle.exit(if result.is_ok() { 0 } else { 1 });
            });
            Ok(())
        })
        .run(context)
        .expect("start disposable native placement probe");
}
