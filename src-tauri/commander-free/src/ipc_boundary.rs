// SPDX-License-Identifier: AGPL-3.0-or-later
//! Custom application commands need a webview boundary as well as plugin ACLs.
//! Tauri's origin checks still apply; a webview label is supplied by Tauri,
//! never by the command payload. Per-command authorization remains unchanged.

use tauri::ipc::{Invoke, InvokeBody};
use tauri::Runtime;

fn allowed(label: &str, command: &str, payload: Option<&serde_json::Value>) -> bool {
    if matches!(label, "main" | "search-overlay") {
        return true;
    }
    if label != "notification-alerts" {
        return false;
    }
    match command {
        "notification_renderer_ready" | "present_notification_window" => true,
        "get_setting" => {
            payload
                .and_then(|value| value.get("path"))
                .and_then(serde_json::Value::as_str)
                == Some("app.theme")
        }
        _ => false,
    }
}

pub(crate) fn guard<R: Runtime>(
    handler: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let payload = match invoke.message.payload() {
            InvokeBody::Json(value) => Some(value),
            InvokeBody::Raw(_) => None,
        };
        if !allowed(
            invoke.message.webview_ref().label(),
            invoke.message.command(),
            payload,
        ) {
            invoke
                .resolver
                .reject("Command is not available to this window");
            return true;
        }
        handler(invoke)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_and_search_keep_their_command_surface() {
        for label in ["main", "search-overlay"] {
            assert!(allowed(label, "get_settings", None));
        }
    }

    #[test]
    fn other_webviews_cannot_invoke_application_commands() {
        for label in [
            "server-app-test",
            "productivity-test",
            "mesh-login-test",
            "main-child",
            "",
        ] {
            for command in [
                "get_settings",
                "patch_settings_cmd",
                "write_settings_export_file",
            ] {
                assert!(!allowed(label, command, None));
            }
        }
    }

    #[test]
    fn notifications_can_present_and_read_only_their_theme() {
        assert!(allowed(
            "notification-alerts",
            "present_notification_window",
            None
        ));
        assert!(allowed(
            "notification-alerts",
            "notification_renderer_ready",
            None
        ));
        assert!(allowed(
            "notification-alerts",
            "get_setting",
            Some(&serde_json::json!({"path":"app.theme"}))
        ));
        for path in ["device", "fleet", "app", "app.theme.extra", ""] {
            assert!(!allowed(
                "notification-alerts",
                "get_setting",
                Some(&serde_json::json!({"path":path}))
            ));
        }
        assert!(!allowed("notification-alerts", "get_setting", None));
        assert!(!allowed("notification-alerts", "patch_settings_cmd", None));
        assert!(!allowed(
            "notification-alerts",
            "write_settings_export_file",
            None
        ));
    }
}
