use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use super::capability::DestructiveAction;
use super::request::safe_detail;

static TRUSTED_LOCKDOWN_GATE: TrustedLockdownGate = TrustedLockdownGate::new();
static TRUSTED_LOCKDOWN_EXECUTING: AtomicBool = AtomicBool::new(false);

pub(super) struct TrustedLockdownGate {
    generation: AtomicU64,
    pending: AtomicBool,
}

impl TrustedLockdownGate {
    pub(super) const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            pending: AtomicBool::new(false),
        }
    }

    pub(super) fn arm(&self) -> Option<u64> {
        if self.pending.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(self.generation.fetch_add(1, Ordering::SeqCst) + 1)
    }

    pub(super) fn cancel(&self) -> bool {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.pending.swap(false, Ordering::SeqCst)
    }

    pub(super) fn take_if_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
            && self
                .pending
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }
}

fn configured_lockdown_countdown() -> u64 {
    crate::settings::read_settings()
        .map(|settings| u64::from(settings.app.lockdown_timer_sec.clamp(3, 30)))
        .unwrap_or(4)
}

fn emit_lockdown_trigger(app: &tauri::AppHandle, countdown_seconds: u64, cancelled: bool) {
    use tauri::Emitter;
    let _ = app.emit(
        "lockdown-trigger",
        serde_json::json!({
            "countdownSeconds": countdown_seconds,
            "cancelled": cancelled,
        }),
    );
}

pub(crate) fn schedule_trusted_lockdown(app: tauri::AppHandle) {
    let Some(generation) = TRUSTED_LOCKDOWN_GATE.arm() else {
        return;
    };
    let countdown_seconds = configured_lockdown_countdown();
    emit_lockdown_trigger(&app, countdown_seconds, false);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(countdown_seconds)).await;
        if TRUSTED_LOCKDOWN_GATE.take_if_current(generation) {
            execute_trusted_lockdown(app);
        }
    });
}

pub(crate) fn toggle_trusted_lockdown(app: tauri::AppHandle) {
    if TRUSTED_LOCKDOWN_GATE.cancel() {
        emit_lockdown_trigger(&app, 0, true);
    } else {
        schedule_trusted_lockdown(app);
    }
}

pub(crate) fn execute_trusted_lockdown(app: tauri::AppHandle) {
    if TRUSTED_LOCKDOWN_EXECUTING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::backend::full_lockdown_internal(app).await {
            crate::log_message(
                "error",
                &format!("[Authz] trusted Lockdown failed: {error}"),
            );
        }
        TRUSTED_LOCKDOWN_EXECUTING.store(false, Ordering::SeqCst);
    });
}

/// Rust-initiated native confirm for a destructive action, exposed for
/// command-level fallback when no capability token is supplied. A WebView
/// script can trigger the dialog but cannot answer it, so it remains a valid
/// control against the assumed-compromised WebView.
pub async fn native_confirm_action(app: &tauri::AppHandle, action: DestructiveAction) -> bool {
    native_confirm(
        app,
        &format!("Confirm {action:?}? This action cannot be undone."),
    )
    .await
}

pub(super) async fn native_confirm(app: &tauri::AppHandle, detail: &str) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
    // The dialog is a control only because a human answers it. The CLI runtime's
    // one window is built invisible and unfocused, and an unattended run has
    // nobody at the keyboard at all, so this await would never complete — the
    // process would hang forever still holding the CLI execution mutex, which
    // then fails every other mutating CLI run with `cli_busy`. Fail closed: an
    // unanswerable confirmation is a denied confirmation.
    if crate::cli::tauri_runtime_active() {
        crate::log_message(
            "warn",
            "[Authz] refused destructive confirmation: interactive confirmation is unavailable in CLI mode",
        );
        return false;
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(detail)
        .title("WinCommander — confirm destructive action")
        .buttons(MessageDialogButtons::OkCancel)
        .show(move |ok| {
            let _ = tx.send(ok);
        });
    rx.await.unwrap_or(false)
}

pub(crate) async fn confirm_backend_dispatch(
    app: &tauri::AppHandle,
    command: &str,
    params: &HashMap<String, String>,
) -> bool {
    let mut fields: Vec<_> = params.iter().collect();
    fields.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let details = fields
        .into_iter()
        .map(|(key, value)| format!("{key}: {}", safe_detail(value)))
        .collect::<Vec<_>>()
        .join("\n");
    native_confirm(
        app,
        &format!(
            "Run destructive backend command {command}?\n\n{}",
            if details.is_empty() {
                "No parameters".to_string()
            } else {
                details
            }
        ),
    )
    .await
}
