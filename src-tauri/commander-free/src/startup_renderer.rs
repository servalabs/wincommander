// SPDX-License-Identifier: AGPL-3.0-or-later
//! Invalidate stale readiness when WebView2 loses the main document process.

use tauri::Manager;
use webview2_com_runtime::{Microsoft::Web::WebView2::Win32::*, ProcessFailedEventHandler};

fn recovery_kind(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> Option<bool> {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Some(true),
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Some(false),
        _ => None,
    }
}

pub(crate) fn install(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let target = window.clone();
    window.with_webview(move |webview| {
        let result = unsafe {
            webview.controller().CoreWebView2().and_then(|core| {
                let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                    args.ProcessFailedKind(&mut kind)?;
                    if let Some(can_reload) = recovery_kind(kind) {
                        recover(&target, can_reload);
                    }
                    Ok(())
                }));
                let mut token = 0;
                core.add_ProcessFailed(&handler, &mut token)
            })
        };
        if let Err(error) = result {
            crate::log_message_src(
                "error",
                "core",
                &format!("[Startup] renderer recovery registration failed: {error}"),
            );
        }
    })
}

fn recover(window: &tauri::WebviewWindow, can_reload: bool) {
    let Some(state) = window.try_state::<crate::startup_window::StartupWindow>() else {
        return;
    };
    let was_open = window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true);
    let locked = crate::calc_mode_active(window.app_handle());
    let _ = window.hide();
    // A lost browser process needs a new controller; never reload or reveal a locked session.
    let retry = state.invalidate_renderer(was_open, can_reload && !locked);
    crate::log_message_src(
        "warn",
        "core",
        &format!("[Startup] main renderer lost; bounded reload={retry}"),
    );
    if !retry {
        crate::startup_window::warn_if_unready(window);
        return;
    }
    let target = window.clone();
    tauri::async_runtime::spawn(async move {
        let reload_target = target.clone();
        let _ = target.run_on_main_thread(move || {
            let state = reload_target.state::<crate::startup_window::StartupWindow>();
            if let Err(error) = state.replace_document(|generation| {
                let mut url = reload_target.url().map_err(|e| e.to_string())?;
                let retained: Vec<(String, String)> = url
                    .query_pairs()
                    .filter(|(key, _)| key != "wc-startup-generation")
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect();
                url.set_query(None);
                url.query_pairs_mut()
                    .extend_pairs(retained)
                    .append_pair("wc-startup-generation", &generation.to_string());
                reload_target.navigate(url).map_err(|e| e.to_string())
            }) {
                state.fail_recovery_if_unready();
                crate::log_message_src(
                    "error",
                    "core",
                    &format!("[Startup] renderer reload failed: {error}"),
                );
            }
        });
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        target
            .state::<crate::startup_window::StartupWindow>()
            .fail_recovery_if_unready();
        crate::startup_window::warn_if_unready(&target);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_main_renderer_exit_reloads_and_browser_exit_requires_restart() {
        assert_eq!(
            recovery_kind(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            Some(true)
        );
        assert_eq!(
            recovery_kind(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED),
            Some(false)
        );
        for kind in [
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
            COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED,
        ] {
            assert_eq!(recovery_kind(kind), None);
        }
    }
}
