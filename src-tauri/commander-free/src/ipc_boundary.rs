// SPDX-License-Identifier: AGPL-3.0-or-later
use tauri::ipc::{Invoke, InvokeBody};
use tauri::Runtime;

// Tauri checks the request origin before this handler. Its plugin capabilities
// do not, by themselves, restrict custom commands in local auxiliary webviews.
fn permits(label: &str, command: &str, body: &InvokeBody) -> bool {
    match label {
        // Quick Search includes native-confirmed file actions; the headless CLI
        // uses the native-created main webview and the same command boundaries.
        "main" | "search-overlay" => true,
        "notification-alerts" => match command {
            "notification_renderer_ready"
            | "present_notification_window"
            | "write_log_record"
            | "open_log_file" => true,
            "get_setting" => matches!(body, InvokeBody::Json(value)
                if value.get("path").and_then(serde_json::Value::as_str) == Some("app.theme")),
            _ => false,
        },
        _ => false,
    }
}

pub(crate) fn guard<R, F>(handler: F) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static
where
    R: Runtime,
    F: Fn(Invoke<R>) -> bool + Send + Sync + 'static,
{
    move |invoke| {
        if !permits(
            invoke.message.webview_ref().label(),
            invoke.message.command(),
            invoke.message.payload(),
        ) {
            invoke
                .resolver
                .reject("command is not available to this webview");
            return true;
        }
        handler(invoke)
    }
}

#[cfg(test)]
#[path = "ipc_boundary_tests.rs"]
mod tests;
