// SPDX-License-Identifier: AGPL-3.0-or-later
//! Invalidate stale readiness when WebView2 loses the main document process.

use tauri::Manager;
use webview2_com_runtime::{Microsoft::Web::WebView2::Win32::*, ProcessFailedEventHandler};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn recovery_kind(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> Option<bool> {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Some(true),
        // WebView2 documents Reload as a supported recovery for an unresponsive
        // renderer. Treat it like an exited renderer while preserving the same
        // single-retry budget.
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => Some(true),
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Some(false),
        _ => None,
    }
}

fn claim_reload(recovery_in_progress: &AtomicBool, renderer_was_ready: bool) -> bool {
    // Readiness belongs to the newly loaded document. It also closes the tiny
    // interval between its acknowledgement and the async recovery monitor.
    if renderer_was_ready {
        recovery_in_progress.store(false, Ordering::Release);
    }
    recovery_in_progress
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

pub(crate) fn install(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let target = window.clone();
    let recovery_in_progress = Arc::new(AtomicBool::new(false));
    window.with_webview(move |webview| {
        let recovery_in_progress = recovery_in_progress.clone();
        let result = unsafe {
            webview.controller().CoreWebView2().and_then(|core| {
                let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                    args.ProcessFailedKind(&mut kind)?;
                    if let Some(can_reload) = recovery_kind(kind) {
                        if !can_reload {
                            // A browser-process loss cannot be repaired by the
                            // renderer reload already in flight. Always make
                            // that terminal failure authoritative.
                            recover(&target, false, None);
                            return Ok(());
                        }
                        // WebView2 may emit the same failure repeatedly until
                        // navigation starts. Only the first notification may
                        // consume the bounded retry; later independent failures
                        // are admitted after this recovery settles.
                        let renderer_was_ready = target
                            .try_state::<crate::startup_window::StartupWindow>()
                            .is_some_and(|state| state.is_ready());
                        if claim_reload(&recovery_in_progress, renderer_was_ready) {
                            recover(&target, true, Some(recovery_in_progress.clone()));
                        }
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

fn recover(
    window: &tauri::WebviewWindow,
    can_reload: bool,
    recovery_in_progress: Option<Arc<AtomicBool>>,
) {
    let Some(state) = window.try_state::<crate::startup_window::StartupWindow>() else {
        if let Some(flag) = recovery_in_progress {
            flag.store(false, Ordering::Release);
        }
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
        if let Some(flag) = recovery_in_progress {
            flag.store(false, Ordering::Release);
        }
        crate::startup_window::warn_if_unready(window);
        return;
    }
    let target = window.clone();
    tauri::async_runtime::spawn(async move {
        let reload_target = target.clone();
        let _ = target.run_on_main_thread(move || {
            let state = reload_target.state::<crate::startup_window::StartupWindow>();
            if let Err(error) = state.replace_document(|generation| {
                let url = crate::startup_window::recovery_url(
                    reload_target.url().map_err(|e| e.to_string())?,
                    generation,
                );
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
        let state = target.state::<crate::startup_window::StartupWindow>();
        let ready = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while !state.is_ready() {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .is_ok();
        if !ready {
            state.fail_recovery_if_unready();
        }
        crate::startup_window::warn_if_unready(&target);
        if let Some(flag) = recovery_in_progress {
            flag.store(false, Ordering::Release);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn main_renderer_failure_reloads_and_browser_exit_requires_restart() {
        assert_eq!(
            recovery_kind(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            Some(true)
        );
        assert_eq!(
            recovery_kind(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED),
            Some(false)
        );
        for kind in [
            COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED,
        ] {
            assert_eq!(recovery_kind(kind), None);
        }
        assert_eq!(
            recovery_kind(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE),
            Some(true)
        );
    }

    #[test]
    fn duplicate_unready_events_coalesce_but_a_ready_document_admits_a_later_failure() {
        let in_progress = AtomicBool::new(false);
        assert!(claim_reload(&in_progress, true));
        assert!(!claim_reload(&in_progress, false));
        assert!(claim_reload(&in_progress, true));
    }
}
