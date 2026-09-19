use super::*;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::{WebviewBuilder, WebviewUrl, WebviewWindowBuilder, WindowBuilder};

fn local_url() -> &'static str {
    if cfg!(windows) {
        "http://tauri.localhost/"
    } else {
        "tauri://localhost/"
    }
}

fn request(command: &str, body: Value, url: &str) -> tauri::webview::InvokeRequest {
    tauri::webview::InvokeRequest {
        cmd: command.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: url.parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.into(),
    }
}

fn app(guarded: bool) -> (tauri::App<MockRuntime>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let recorded = calls.clone();
    let dispatch = move |invoke: Invoke<MockRuntime>| {
        recorded.fetch_add(1, Ordering::SeqCst);
        invoke.resolver.resolve("dispatched");
        true
    };
    let builder = if guarded {
        mock_builder().invoke_handler(guard(dispatch))
    } else {
        mock_builder().invoke_handler(dispatch)
    };
    (builder.build(mock_context(noop_assets())).unwrap(), calls)
}

#[test]
fn alert_cannot_reach_protected_dispatch_even_when_local() {
    // Reproduce the framework default without the application guard.
    let (unrestricted, _) = app(false);
    let alert = WebviewWindowBuilder::new(&unrestricted, "notification-alerts", Default::default())
        .build()
        .unwrap();
    assert!(get_ipc_response(
        &alert,
        request("protected_mutation", json!({}), local_url())
    )
    .is_ok());

    let (restricted, calls) = app(true);
    let alert = WebviewWindowBuilder::new(&restricted, "notification-alerts", Default::default())
        .build()
        .unwrap();
    for command in [
        "protected_mutation",
        "patch_settings_cmd",
        "run_backend_script",
        "get_settings",
        "exit_app",
    ] {
        let result = get_ipc_response(&alert, request(command, json!({}), local_url()));
        assert_eq!(
            result.unwrap_err(),
            json!("command is not available to this webview")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn main_and_quick_search_keep_existing_backend_authorization_paths() {
    let (app, calls) = app(true);
    for label in ["main", "search-overlay"] {
        let view = WebviewWindowBuilder::new(&app, label, Default::default())
            .build()
            .unwrap();
        assert!(
            get_ipc_response(&view, request("protected_mutation", json!({}), local_url())).is_ok()
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn embedded_child_does_not_inherit_custom_commands_from_parent_main() {
    let (app, calls) = app(true);
    let parent = WindowBuilder::new(&app, "main").build().unwrap();
    let child = parent
        .add_child(
            WebviewBuilder::new("server-app-test", WebviewUrl::App("index.html".into())),
            tauri::LogicalPosition::new(0, 0),
            tauri::LogicalSize::new(100, 100),
        )
        .unwrap();
    assert!(get_ipc_response(
        &child,
        request("protected_mutation", json!({}), local_url())
    )
    .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn remote_origin_is_rejected_before_the_custom_command_guard() {
    let (app, calls) = app(true);
    let view = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    assert!(get_ipc_response(
        &view,
        request("protected_mutation", json!({}), "https://example.com/")
    )
    .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn alerts_can_render_and_read_only_their_theme_setting() {
    let (app, calls) = app(true);
    let view = WebviewWindowBuilder::new(&app, "notification-alerts", Default::default())
        .build()
        .unwrap();
    for command in [
        "notification_renderer_ready",
        "present_notification_window",
        "write_log_record",
        "open_log_file",
    ] {
        assert!(get_ipc_response(&view, request(command, json!({}), local_url())).is_ok());
    }
    assert!(get_ipc_response(
        &view,
        request("get_setting", json!({"path":"app.theme"}), local_url())
    )
    .is_ok());
    for body in [
        json!({}),
        json!({"path":"app"}),
        json!({"path":"app.fleet"}),
        json!({"path":["app.theme"]}),
    ] {
        assert!(get_ipc_response(&view, request("get_setting", body, local_url())).is_err());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

#[test]
fn unknown_and_lookalike_labels_fail_closed() {
    for label in [
        "",
        "main-child",
        "Main",
        "notification-alerts-other",
        "mesh-login-tailscale",
        "productivity-aw",
    ] {
        assert!(!permits(label, "get_settings", &InvokeBody::default()));
    }
    assert!(!permits(
        "notification-alerts",
        "get_setting",
        &InvokeBody::Raw(vec![])
    ));
}
