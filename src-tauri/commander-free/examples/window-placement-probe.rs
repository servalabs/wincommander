// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disposable native-window check; no application settings, tasks or services.
#[path = "../src/startup_window.rs"]
mod startup_window;
#[path = "../src/window_placement.rs"]
mod window_placement;
use std::sync::atomic::{AtomicBool, Ordering};

fn set_wincommander_window_icon(_: &tauri::WebviewWindow) {}
fn log_message_src(level: &str, _: &str, message: &str) {
    println!("{level}: {message}");
}
mod startup_trace {
    pub fn milestone(_: &tauri::AppHandle, _: &str) {}
}

#[tauri::command]
fn probe_ready(state: tauri::State<'_, AtomicBool>) {
    state.store(true, Ordering::Release);
}

fn calc_mode_active(_: &tauri::AppHandle) -> bool {
    false
}

fn main() {
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.servalabs.wincommander.placement-probe".into();
    tauri::Builder::default()
        .manage(AtomicBool::new(false))
        .manage(startup_window::StartupWindow::new())
        .invoke_handler(tauri::generate_handler![probe_ready, startup_window::startup_window_ready])
        .register_uri_scheme_protocol("placement", |_, _| {
            tauri::http::Response::builder().header("Content-Type", "text/html")
                .body("<html><body style='background:#0a0f12;color:white;font:24px system-ui'><p id='ready'>WinCommander window placement test</p><p>This disposable window closes automatically.</p><script>if(document.getElementById('ready').textContent) { window.__TAURI_INTERNALS__.invoke('probe_ready'); const generation=Number(new URLSearchParams(location.search).get('wc-startup-generation')||0); if(generation) window.__TAURI_INTERNALS__.invoke('startup_window_ready',{isLight:false,generation}); }</script></body></html>".as_bytes()).unwrap()
        })
        .setup(|app| {
            let window = tauri::WebviewWindowBuilder::new(
                app, "main",
                tauri::WebviewUrl::CustomProtocol("placement://localhost".parse().unwrap()),
            ).title("WinCommander placement test")
                .decorations(false).visible(false).inner_size(1200.0, 800.0)
                .min_inner_size(900.0, 600.0).build()?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    use tauri::Manager;
                    for flag in ["--autostart", "--minimized"] {
                        for elevated in [false, true] {
                            let mut args = vec!["WinCommander.exe".into(), flag.into()];
                            if elevated { args.push("--elevated-relaunch".into()); }
                            if !startup_window::should_start_hidden(&args, true) {
                                return Err(format!("Background launch would reveal: {args:?}"));
                            }
                            if startup_window::should_start_hidden(&args, false) != (flag == "--minimized") {
                                return Err(format!("Visible sign-in preference was ignored: {args:?}"));
                            }
                        }
                    }
                    if window.is_visible().map_err(|e| e.to_string())? {
                        return Err("Background startup exposed its native window".into());
                    }
                    println!("PASS current and legacy background flags stay hidden with either elevation state");
                    if !startup_window::defer_reveal_until_ready(&window)
                        || !startup_window::defer_reveal_until_ready(&window)
                        || window.is_visible().map_err(|e| e.to_string())?
                    {
                        return Err("Early repeated tray requests exposed an unready window".into());
                    }
                    println!("PASS early repeated tray opens queue without exposing a blank native window");
                    tokio::time::timeout(std::time::Duration::from_secs(15), async {
                        while !handle.state::<AtomicBool>().load(Ordering::Acquire) {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                    }).await.map_err(|_| "WebView did not confirm its actual DOM was loaded")?;
                    println!("PASS native WebView JavaScript confirms document content before reveal");
                    let recovering = window.clone();
                    tauri::async_runtime::spawn(async move { startup_window::recover_if_unready(&recovering).await; });
                    tokio::time::timeout(std::time::Duration::from_secs(12), async {
                        while !handle.state::<startup_window::StartupWindow>().is_ready()
                            || !window.is_visible().unwrap_or(false)
                            || !window.is_maximized().unwrap_or(false)
                        {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                    }).await.map_err(|_| "Automatic document recovery did not complete")?;
                    println!("PASS automatic native document recovery consumes the queued tray reveal after DOM readiness");
                    println!("PASS cold hidden reveal: visible={} maximized={} scale={} elevated={}", window.is_visible().unwrap_or(false), window.is_maximized().unwrap_or(false), window.scale_factor().unwrap_or(0.0), unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 });
                    window.minimize().map_err(|e| e.to_string())?;
                    if startup_window::should_hide_on_tray_click(
                        window.is_visible().map_err(|e| e.to_string())?,
                        window.is_minimized().map_err(|e| e.to_string())?,
                    ) {
                        return Err("Tray click would hide a minimized window instead of restoring it".into());
                    }
                    window_placement::show_maximized(&window).await?;
                    println!("PASS minimized reveal");
                    window.hide().map_err(|e| e.to_string())?;
                    if startup_window::startup_window_ready(window.clone(), false, Some(1)).await? {
                        return Err("Repeated document readiness reopened a window closed to tray".into());
                    }
                    if startup_window::defer_reveal_until_ready(&window) {
                        return Err("An already ready document discarded a tray request".into());
                    }
                    window_placement::show_maximized(&window).await?;
                    println!("PASS repeated readiness stays hidden; next tray open restores immediately");
                    let monitor = window.current_monitor().map_err(|e| e.to_string())?.ok_or("No monitor")?;
                    let work = monitor.work_area();
                    window.unmaximize().map_err(|e| e.to_string())?;
                    window.set_min_size(Some(tauri::PhysicalSize::new(work.size.width + 200, work.size.height + 200))).map_err(|e| e.to_string())?;
                    window_placement::show_maximized(&window).await?;
                    println!("PASS oversized previous minimum repaired; client={:?} size={:?} work={:?}", window.inner_position().unwrap(), window.inner_size().unwrap(), work);
                    window.hide().map_err(|e| e.to_string())?;
                    handle.state::<startup_window::StartupWindow>().invalidate_renderer(false, true);
                    if window_placement::show_maximized(&window).await.is_ok()
                        || window.is_visible().map_err(|e| e.to_string())? {
                        return Err("A queued reveal exposed an invalidated renderer".into());
                    }
                    println!("PASS invalidated renderer rejects native placement and stays hidden");
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
