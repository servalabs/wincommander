// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn recovery_replaces_all_stale_generation_values_preserving_route_and_options() {
    let url = recovery_url(
        "http://tauri.localhost/settings?theme=dark&wc-startup-generation=0&view=secret&wc-startup-generation=1#startup"
            .parse().unwrap(),
        2,
    );
    assert_eq!(url.path(), "/settings");
    assert_eq!(url.fragment(), Some("startup"));
    let pairs: Vec<_> = url.query_pairs().collect();
    assert_eq!(
        pairs,
        [
            ("theme".into(), "dark".into()),
            ("view".into(), "secret".into()),
            ("wc-startup-generation".into(), "2".into())
        ]
    );
    let state = StartupWindow::new();
    assert!(state.invalidate_renderer(false, true));
    state.replace_document(|_| Ok(())).unwrap();
    let frontend_generation = url
        .query_pairs()
        .find(|(key, _)| key == "wc-startup-generation")
        .unwrap()
        .1
        .parse()
        .unwrap();
    assert!(state.accept_ready(frontend_generation));
}

#[test]
fn renderer_failure_invalidates_readiness_and_rejects_late_old_document_ack() {
    for reveal in [false, true] {
        let state = StartupWindow::new();
        assert!(state.accept_ready(0));
        assert!(state.invalidate_renderer(reveal, true));
        assert!(!state.is_ready());
        assert!(!state.accept_ready(0));
        state
            .replace_document(|generation| {
                assert_eq!(generation, 2);
                Ok(())
            })
            .unwrap();
        assert!(state.accept_ready(2));
        assert_eq!(state.take_reveal(), reveal);
        assert!(!state.invalidate_renderer(reveal, true));
        assert!(!state.accept_ready(3));
    }
}

#[test]
fn browser_failure_is_terminal_and_background_failure_never_arms_a_reveal() {
    let state = StartupWindow::new();
    assert!(state.accept_ready(0));
    assert!(!state.invalidate_renderer(false, false));
    assert!(!state.take_reveal());
    assert!(!state.accept_ready(1));
    state
        .replace_document(|_| panic!("browser failure must never navigate"))
        .unwrap();
    assert!(state.defer_reveal());
    assert!(state.needs_warning());
    assert!(!state.needs_warning());
}

#[test]
fn failed_or_timed_out_hidden_recovery_warns_on_the_next_explicit_open() {
    for navigation_fails in [false, true] {
        let state = StartupWindow::new();
        assert!(state.accept_ready(0));
        assert!(state.invalidate_renderer(false, true));
        let result = state.replace_document(|_| {
            if navigation_fails {
                Err("navigation rejected".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.is_err(), navigation_fails);
        state.fail_recovery_if_unready();
        assert!(!state.needs_warning(), "background errors stay silent");
        assert!(state.defer_reveal());
        assert!(state.terminal_failure.load(Ordering::Acquire));
        assert!(
            state.needs_warning(),
            "next explicit open receives a native explanation"
        );
        assert!(!state.accept_ready(if navigation_fails { 1 } else { 2 }));
    }
}

#[test]
fn recovery_deadline_cannot_invalidate_a_document_that_became_ready() {
    let state = StartupWindow::new();
    assert!(state.invalidate_renderer(false, true));
    state.replace_document(|_| Ok(())).unwrap();
    assert!(state.accept_ready(2));
    state.fail_recovery_if_unready();
    assert!(!state.terminal_failure.load(Ordering::Acquire));
    assert!(!state.defer_reveal());
}

#[test]
fn visible_locked_renderer_failure_can_explain_failure_without_accepting_content() {
    let state = StartupWindow::new();
    assert!(state.accept_ready(0));
    assert!(!state.invalidate_renderer(true, false));
    assert!(state.needs_warning());
    assert!(!state.accept_ready(1));
    state
        .replace_document(|_| panic!("locked renderer must not navigate"))
        .unwrap();
}

#[test]
fn a_resolved_startup_warning_does_not_silence_a_later_renderer_failure() {
    let state = StartupWindow::new();
    state.arm();
    assert!(state.needs_warning());
    assert!(state.accept_ready(0));
    assert!(state.take_reveal());
    assert!(!state.invalidate_renderer(true, false));
    assert!(state.needs_warning());
}

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
            assert!(should_start_hidden(&args, true), "{args:?}");
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
        for silent in [false, true] {
            assert!(!should_start_hidden(&args, silent), "{args:?}");
        }
    }
}

#[test]
fn disabling_silent_start_opens_logon_launches_but_honors_explicit_minimized() {
    for elevated in [false, true] {
        let mut args = vec!["WinCommander.exe".into(), "--autostart".into()];
        if elevated {
            args.push("--elevated-relaunch".into());
        }
        assert!(!should_start_hidden(&args, false));
        args.push("--minimized".into());
        assert!(should_start_hidden(&args, false));
        assert!(should_start_hidden(&args, true));
    }
    assert!(should_start_hidden(&["--minimized".into()], false));
}

#[test]
fn sign_in_visibility_choice_waits_for_renderer_readiness_before_reveal() {
    for silent in [false, true] {
        let state = StartupWindow::new();
        if !should_start_hidden(&["--autostart".into()], silent) {
            assert!(state.defer_reveal());
        }
        assert!(!state.is_ready());
        assert!(state.accept_ready(0));
        assert_eq!(state.take_reveal(), !silent);
        assert!(!state.take_reveal());
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
