use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

static TOAST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, serde::Serialize)]
struct CustomNotificationPayload {
    id: u64,
    title: String,
    body: String,
    severity: String,
    source: String,
}

pub fn show_native_notification(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    if crate::settings::is_native_notifications_disabled() {
        return Ok(());
    }
    if crate::settings::is_decoy_mode() {
        crate::log_message(
            "info",
            "[Notify] suppressed native notification in decoy mode",
        );
        return Ok(());
    }

    let (severity, source) = infer_presentation(title, body);
    show_custom_notification(app, title, body, severity, source)
}

fn show_custom_notification(
    app: &AppHandle,
    title: &str,
    body: &str,
    severity: &str,
    source: &str,
) -> Result<(), String> {
    crate::log_message(
        "info",
        &format!(
            "[Notify] show external custom alert source={} title={}",
            source, title
        ),
    );

    ensure_notification_window(app)?;
    promote_notification_window(app)?;

    let toast_id = TOAST_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let payload = CustomNotificationPayload {
        id: toast_id,
        title: title.to_string(),
        body: body.to_string(),
        severity: severity.to_string(),
        source: source.to_string(),
    };

    app.emit("wc-native-notification", payload)
        .map_err(|error| format!("could not emit external notification: {error}"))
}

/// Privacy Shield uses a separate topmost overlay. Re-promote the alert window
/// for every delivery so a detection alert is visible above that overlay instead
/// of appearing only after the shield is disabled. `SWP_NOACTIVATE` preserves
/// the user's current focus and keeps the alert click-through until its renderer
/// decides it has content to show.
fn promote_notification_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("notification-alerts")
        .ok_or_else(|| "notification window was not created".to_string())?;

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };

        let raw = window
            .hwnd()
            .map_err(|error| format!("notification hwnd: {error}"))?;
        let promoted = unsafe {
            SetWindowPos(
                raw.0 as HWND,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        if promoted == 0 {
            return Err("could not promote notification window above Privacy Shield".to_string());
        }
    }

    #[cfg(not(windows))]
    let _ = window;

    Ok(())
}

fn ensure_notification_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("notification-alerts").is_some() {
        return Ok(());
    }

    WebviewWindowBuilder::new(
        app,
        "notification-alerts",
        WebviewUrl::App("index.html".into()),
    )
    .title("WinCommander Alerts")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .inner_size(440.0, 520.0)
    .build()
    .map(|_| ())
    .map_err(|error| format!("could not create notification window: {error}"))
}

fn infer_presentation(title: &str, body: &str) -> (&'static str, &'static str) {
    let text = format!("{} {}", title, body).to_ascii_lowercase();
    if text.contains("ransomware") || text.contains("disconnect network") {
        ("danger", "Ransomware Monitor")
    } else if text.contains("decoy") || text.contains("honeypot") {
        ("danger", "Privacy Tripwire")
    } else if text.contains("powershell")
        || text.contains("clipboard")
        || text.contains("paste")
        || text.contains("crypto address")
    {
        ("danger", "Paste Monitor")
    } else if text.contains("rdp") || text.contains("are you still there") {
        ("warning", "Remote Guard")
    } else if text.contains("traffic") || text.contains("upload") || text.contains("download") {
        ("warning", "Network Control")
    } else if text.contains("cpu") || text.contains("memory") || text.contains("disk") {
        ("warning", "System Radar")
    } else {
        ("info", "WinCommander")
    }
}

#[tauri::command]
pub fn show_native_test_notification(app: AppHandle) -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Ok(());
    }

    show_custom_notification(
        &app,
        "WinCommander - Alert",
        "CPU hit 92.0% (limit 85%).",
        "warning",
        "System Radar",
    )?;

    Ok(())
}

#[tauri::command]
pub fn dismiss_notification_toast(app: AppHandle) -> Result<(), String> {
    let _ = app.emit("wc-native-notification-dismiss", ());
    Ok(())
}

#[tauri::command]
pub fn dismiss_notification_toast_id(app: AppHandle, id: u64) -> Result<(), String> {
    let _ = app.emit("wc-native-notification-dismiss-id", id);
    Ok(())
}

#[tauri::command]
pub fn show_test_notification_kind(app: AppHandle, kind: String) -> Result<String, String> {
    if crate::settings::is_decoy_mode() {
        return Ok("suppressed:decoy".to_string());
    }

    let normalized = kind.trim().to_ascii_lowercase();
    let (title, body, severity, source) = match normalized.as_str() {
        "cpu" | "radar" | "system" => (
            "WinCommander - Alert",
            "CPU hit 92.0% (limit 85%).",
            "warning",
            "System Radar",
        ),
        "tripwire" | "clipboard" | "paste" => (
            "WinCommander · Dangerous PowerShell command",
            "Clipboard contains a PowerShell-style payload (Encoded PowerShell payload). Do not paste it into Win+R, Terminal, or PowerShell unless you wrote it yourself.",
            "danger",
            "Paste Monitor",
        ),
        "rdp" | "remote" | "idle" => (
            "Are you still there?",
            "Inactive for 12m 30s. RDP closes in 60s. Move mouse to cancel.",
            "warning",
            "Remote Guard",
        ),
        "network" | "traffic" => (
            "WinCommander - Alert",
            "Download hit 120.0Mbps (limit 100Mbps).",
            "warning",
            "Network Control",
        ),
        "decoy" | "honeypot" => (
            "WinCommander · ⚠ Decoy file accessed",
            "Decoy 'passwords.txt' was modified. Investigate — this may be malware or someone scanning for sensitive files.",
            "danger",
            "Privacy Tripwire",
        ),
        "ransomware" => (
            "WinCommander · 🚨 Possible ransomware",
            "34 files were modified in 10 seconds — looks like mass encryption. DISCONNECT NETWORK NOW (unplug Ethernet / disable WiFi). Then open Task Manager and end any process you don't recognise.",
            "danger",
            "Ransomware Monitor",
        ),
        other => return Err(format!("unknown notification kind: {}", other)),
    };

    show_custom_notification(&app, title, body, severity, source)?;

    Ok(format!("shown:{}", normalized))
}

#[tauri::command]
pub fn show_rdp_idle_warning_native(
    app: AppHandle,
    idle_time: String,
    seconds_left: u32,
) -> Result<(), String> {
    let idle = if idle_time.trim().is_empty() {
        "unknown"
    } else {
        idle_time.trim()
    };
    show_native_notification(
        &app,
        "Are you still there?",
        &format!(
            "Inactive for {}. RDP closes in {}s. Move mouse to cancel.",
            idle, seconds_left
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{infer_presentation, promote_notification_window};

    #[test]
    fn ransomware_alerts_have_danger_presentation() {
        assert_eq!(
            infer_presentation("Possible ransomware", "Disconnect network now"),
            ("danger", "Ransomware Monitor")
        );
    }

    #[test]
    fn generic_alerts_use_the_neutral_presentation() {
        assert_eq!(
            infer_presentation("Update available", "A new build is ready."),
            ("info", "WinCommander")
        );
    }

    #[test]
    fn custom_notifications_are_promoted_above_privacy_shield_without_activation() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/native_notify.rs"));
        let start = source
            .find("fn show_custom_notification")
            .expect("custom notification path must exist");
        let body = &source[start..start + 900.min(source.len() - start)];
        assert!(body.contains("ensure_notification_window(app)?;"));
        assert!(body.contains("promote_notification_window(app)?;"));
        assert!(source.contains("HWND_TOPMOST"));
        assert!(source.contains("SWP_NOACTIVATE"));
        let _ = promote_notification_window as fn(&tauri::AppHandle) -> Result<(), String>;
    }
}
