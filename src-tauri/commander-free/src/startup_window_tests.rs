// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn tray_click_restores_hidden_or_minimized_windows_and_only_hides_an_open_window() {
    for (visible, minimized, should_hide) in [
        (false, false, false),
        (false, true, false),
        (true, true, false),
        (true, false, true),
    ] {
        assert_eq!(
            should_hide_on_tray_click(visible, minimized),
            should_hide,
            "visible={visible} minimized={minimized}",
        );
    }
}

#[test]
fn logon_launches_remain_hidden_before_and_after_elevation() {
    for background_arg in ["--autostart", "--minimized"] {
        for elevated in [false, true] {
            let mut args = vec!["WinCommander.exe".to_string(), background_arg.to_string()];
            if elevated {
                args.push("--elevated-relaunch".to_string());
            }
            assert!(should_start_hidden(&args), "{args:?}");
        }
    }
}

#[test]
fn manual_launches_open_normally_including_elevated_relaunch() {
    for args in [
        vec!["WinCommander.exe".to_string()],
        vec![
            "WinCommander.exe".to_string(),
            "--elevated-relaunch".to_string(),
        ],
    ] {
        assert!(!should_start_hidden(&args), "{args:?}");
    }
}

#[test]
fn early_tray_open_survives_until_a_hidden_startup_is_ready() {
    let state = StartupWindow::new();
    assert!(state.defer_reveal());
    assert!(!state.is_ready());
    assert!(state.accept_ready(0));
    assert!(state.take_reveal());
    assert!(!state.take_reveal());
}

#[test]
fn repeated_early_tray_clicks_coalesce_into_one_reveal() {
    let state = StartupWindow::new();
    assert!(state.defer_reveal());
    assert!(state.defer_reveal());
    assert!(state.accept_ready(0));
    assert!(state.take_reveal());
    assert!(!state.take_reveal());
}

#[test]
fn readiness_before_open_allows_immediate_reveal_without_leaving_a_pending_one() {
    let state = StartupWindow::new();
    assert!(state.accept_ready(0));
    assert!(!state.take_reveal());
    assert!(!state.defer_reveal());
    assert!(!state.take_reveal());
}

#[test]
fn concurrent_readiness_and_tray_open_never_lose_the_open_request() {
    use std::sync::{Arc, Barrier};
    for _ in 0..64 {
        let state = Arc::new(StartupWindow::new());
        let barrier = Arc::new(Barrier::new(2));
        let opening_state = state.clone();
        let opening_barrier = barrier.clone();
        let opening = std::thread::spawn(move || {
            opening_barrier.wait();
            opening_state.defer_reveal()
        });
        barrier.wait();
        assert!(state.accept_ready(0));
        let consumed_reveal = state.take_reveal();
        let deferred = opening.join().unwrap();
        // Either readiness consumes the queued request or the caller can open now.
        assert_eq!(deferred, consumed_reveal);
        assert!(!state.take_reveal());
    }
}

#[test]
fn tray_open_arms_recovery_for_an_unready_background_launch() {
    let state = StartupWindow::new();
    assert!(!state.begin_recovery());
    assert!(state.defer_reveal());
    assert!(state.begin_recovery());
    assert!(!state.begin_recovery());
    assert!(state.accept_ready(0));
    assert!(state.take_reveal());
}

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

#[test]
fn timeout_warns_once_without_consuming_a_late_reveal() {
    let state = StartupWindow::new();
    assert!(!state.needs_warning());
    state.arm();
    assert!(state.needs_warning());
    assert!(!state.needs_warning());
    state.ready.store(true, Ordering::Release);
    assert!(state.take_reveal());
}

#[test]
fn ready_or_suppressed_windows_never_show_timeout_errors() {
    let state = StartupWindow::new();
    assert!(!state.needs_warning());
    state.arm();
    state.ready.store(true, Ordering::Release);
    assert!(!state.needs_warning());
}

#[test]
fn recovery_is_once_only_and_preserves_late_readiness() {
    let state = StartupWindow::new();
    assert!(!state.begin_recovery());
    state.arm();
    assert!(state.begin_recovery());
    assert!(!state.begin_recovery());
    state.ready.store(true, Ordering::Release);
    assert!(!state.needs_warning());
    assert!(state.take_reveal());
}

#[test]
fn a_ready_document_is_never_reloaded() {
    let state = StartupWindow::new();
    state.arm();
    state.ready.store(true, Ordering::Release);
    assert!(!state.begin_recovery());
}

#[test]
fn old_document_readiness_cannot_reveal_a_replacement() {
    let state = StartupWindow::new();
    state.arm();
    state
        .replace_document(|generation| {
            assert_eq!(generation, 1);
            Ok(())
        })
        .unwrap();
    assert!(!state.accept_ready(0));
    assert!(!state.is_ready());
    assert!(state.accept_ready(1));
    assert!(state.is_ready());
}

#[test]
fn failed_navigation_preserves_late_readiness_from_the_original_document() {
    let state = StartupWindow::new();
    assert!(state.defer_reveal());
    assert!(state
        .replace_document(|_| Err("navigation rejected".into()))
        .is_err());
    assert!(state.accept_ready(0));
    assert!(state.take_reveal());
}

#[test]
fn readiness_arriving_before_navigation_cancels_the_reload() {
    let state = StartupWindow::new();
    assert!(state.accept_ready(0));
    state
        .replace_document(|_| panic!("ready document must not navigate"))
        .unwrap();
    assert!(state.accept_ready(0));
}
