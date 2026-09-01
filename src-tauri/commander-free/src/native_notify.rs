use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

static TOAST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const NOTIFICATION_WINDOW_LABEL: &str = "notification-alerts";
const NOTIFICATION_DELIVERY_EVENT: &str = "wc-custom-notification";
const MAX_PENDING_NOTIFICATIONS: usize = 32;

#[derive(Clone, serde::Serialize)]
struct CustomNotificationPayload {
    id: u64,
    title: String,
    body: String,
    severity: String,
    source: String,
}

#[derive(Default)]
struct NotificationDeliveryState {
    renderer_ready: bool,
    pending: VecDeque<CustomNotificationPayload>,
}

fn notification_delivery_state() -> &'static Mutex<NotificationDeliveryState> {
    static STATE: OnceLock<Mutex<NotificationDeliveryState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(NotificationDeliveryState::default()))
}

fn enqueue_pending_notification(
    state: &mut NotificationDeliveryState,
    payload: CustomNotificationPayload,
) {
    if state.pending.len() == MAX_PENDING_NOTIFICATIONS {
        state.pending.pop_front();
    }
    state.pending.push_back(payload);
}

fn queue_or_deliver_notification(
    window: &WebviewWindow,
    payload: CustomNotificationPayload,
) -> Result<(), String> {
    let renderer_ready = {
        let mut state = notification_delivery_state()
            .lock()
            .map_err(|_| "notification delivery state is unavailable".to_string())?;
        if state.renderer_ready {
            true
        } else {
            enqueue_pending_notification(&mut state, payload.clone());
            false
        }
    };

    if renderer_ready {
        window
            .emit(NOTIFICATION_DELIVERY_EVENT, payload)
            .map_err(|error| format!("could not deliver notification: {error}"))?;
    }
    Ok(())
}

/// The notification renderer invokes this only after its local event listener
/// has been installed. A Tauri command is a point-to-point acknowledgement;
/// unlike a broadcast ready event, it cannot be lost when the app shell and
/// the hidden alert window are starting at the same time.
#[tauri::command]
pub fn notification_renderer_ready(app: AppHandle) -> Result<(), String> {
    let pending = {
        let mut state = notification_delivery_state()
            .lock()
            .map_err(|_| "notification delivery state is unavailable".to_string())?;
        state.renderer_ready = true;
        state.pending.drain(..).collect::<Vec<_>>()
    };

    let window = app
        .get_webview_window(NOTIFICATION_WINDOW_LABEL)
        .ok_or_else(|| "notification window was not created".to_string())?;
    for payload in pending {
        window
            .emit(NOTIFICATION_DELIVERY_EVENT, payload)
            .map_err(|error| format!("could not deliver queued notification: {error}"))?;
    }
    Ok(())
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

    let window = ensure_notification_window(app)?;

    let toast_id = TOAST_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let payload = CustomNotificationPayload {
        id: toast_id,
        title: title.to_string(),
        body: body.to_string(),
        severity: severity.to_string(),
        source: source.to_string(),
    };

    queue_or_deliver_notification(&window, payload)
}

/// Privacy Shield uses a separate topmost overlay. This must run after the
/// alert renderer has shown its window: promoting an invisible HWND leaves the
/// shield above it when it later becomes visible. `SWP_NOACTIVATE` preserves
/// the user's current focus while `SWP_SHOWWINDOW` puts the visible alert above
/// the shield.
fn promote_notification_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("notification-alerts")
        .ok_or_else(|| "notification window was not created".to_string())?;

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
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
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
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

/// Called by the notification renderer after it has received and shown a toast.
/// Keeping this as a renderer acknowledgement prevents Privacy Shield's PyQt
/// overlay from taking the topmost slot during the hidden-window hand-off.
#[tauri::command]
pub fn present_notification_window(app: AppHandle) -> Result<(), String> {
    promote_notification_window(&app)
}

fn ensure_notification_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(NOTIFICATION_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        NOTIFICATION_WINDOW_LABEL,
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
    use super::{
        enqueue_pending_notification, infer_presentation, promote_notification_window,
        CustomNotificationPayload, NotificationDeliveryState, MAX_PENDING_NOTIFICATIONS,
    };

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
    fn custom_notifications_are_promoted_after_the_renderer_shows_them() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/native_notify.rs"));
        let start = source
            .find("fn show_custom_notification")
            .expect("custom notification path must exist");
        let body = &source[start..start + 900.min(source.len() - start)];
        assert!(body.contains("ensure_notification_window(app)?;"));
        assert!(!body.contains("promote_notification_window(app)?;"));
        assert!(body.contains("queue_or_deliver_notification(&window, payload)"));
        assert!(!body.contains("app.emit(\"wc-native-notification\", payload)"));
        assert!(source.contains("pub fn notification_renderer_ready"));
        assert!(source.contains("pub fn present_notification_window"));
        assert!(source.contains("HWND_TOPMOST"));
        assert!(source.contains("SWP_NOACTIVATE"));
        assert!(source.contains("SWP_SHOWWINDOW"));
        let _ = promote_notification_window as fn(&tauri::AppHandle) -> Result<(), String>;
    }

    #[test]
    fn pending_alerts_are_retained_until_the_notification_renderer_is_ready() {
        let mut state = NotificationDeliveryState::default();
        for id in 0..=MAX_PENDING_NOTIFICATIONS {
            enqueue_pending_notification(
                &mut state,
                CustomNotificationPayload {
                    id: id as u64,
                    title: "Privacy Shield".to_string(),
                    body: "Look away".to_string(),
                    severity: "info".to_string(),
                    source: "WinCommander".to_string(),
                },
            );
        }
        assert!(!state.renderer_ready);
        assert_eq!(state.pending.len(), MAX_PENDING_NOTIFICATIONS);
        assert_eq!(state.pending.front().map(|payload| payload.id), Some(1));

        state.renderer_ready = true;
        let ready_delivery: Vec<_> = state.pending.drain(..).collect();
        assert_eq!(ready_delivery.len(), MAX_PENDING_NOTIFICATIONS);
        assert_eq!(ready_delivery[0].id, 1);
        assert!(state.pending.is_empty());
    }
}
