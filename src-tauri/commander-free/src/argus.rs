// SPDX-License-Identifier: AGPL-3.0-or-later
// commander-free/src/argus.rs
//
// ═══════════════════════════════════════════════════════════════════════
// Argus — Free-side thin wrappers for app-usage monitoring (PAID)
// ═══════════════════════════════════════════════════════════════════════
//
// Each command below is a thin #[tauri::command] shim that:
//   1. Gate-checks the paid entitlement via require_paid (returns Err to
//      the frontend if unpaid — never calls into Pro).
//   2. Delegates to the Pro sidecar via dispatch_paid_command.
//
// The four feature_ids here are the PINNED CONTRACT:
//   "start_argus_app_usage"    → Pro: session_monitor::start
//   "stop_argus_app_usage"     → Pro: session_monitor::stop
//   "argus_app_usage_status"   → Pro: session_monitor::status
//   "get_argus_app_usage_recent" → Pro: session_monitor::recent
//
// Do NOT rename these IDs — fleet_push.rs and the Pro handlers.rs match
// arm both reference them as string literals.
//
// AV hygiene: this file must not contain any strings that appear in
// tools/strings-grep-forbidden.txt. Summaries go to evidence.rs (caller
// side) — nothing here touches the ledger directly.

/// Gate + delegate: start the Pro app-usage collector for this subject.
///
/// `args` forwarded to Pro: { subject_id?, org_id?, interval_ms? }
/// Returns the collector status JSON from Pro.
#[tauri::command]
pub async fn argus_app_usage_start(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus app-usage")?;
    crate::sidecar::dispatch_paid_command(
        "start_argus_app_usage",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Gate + delegate: stop the Pro app-usage collector.
#[tauri::command]
pub async fn argus_app_usage_stop(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus app-usage")?;
    crate::sidecar::dispatch_paid_command(
        "stop_argus_app_usage",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Gate + delegate: query Pro collector running/paused status.
#[tauri::command]
pub async fn argus_app_usage_status() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus app-usage")?;
    crate::sidecar::dispatch_paid_command("argus_app_usage_status", serde_json::Value::Null).await
}

/// Gate + delegate: fetch the most-recent completed productivity windows
/// from Pro. Returns an array of ProductivitySampleData objects.
#[tauri::command]
pub async fn argus_app_usage_recent(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus app-usage")?;
    crate::sidecar::dispatch_paid_command(
        "get_argus_app_usage_recent",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

// ── Argus — print + removable-media monitor (Pro: print_usb_monitor.rs) ──
//
// PINNED feature_ids — do NOT rename. Pro's handlers.rs match arms reference
// exactly these string literals. AV hygiene: only command-id strings and
// generic JSON forwarded here — no flaggable strings.

/// Gate + delegate: start the Pro print+USB monitor.
#[tauri::command]
pub async fn argus_print_usb_start(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus print-usb")?;
    crate::sidecar::dispatch_paid_command(
        "start_argus_print_usb",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Gate + delegate: stop the Pro print+USB monitor.
#[tauri::command]
pub async fn argus_print_usb_stop() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus print-usb")?;
    crate::sidecar::dispatch_paid_command("stop_argus_print_usb", serde_json::Value::Null).await
}

/// Gate + delegate: query Pro print+USB monitor running status.
#[tauri::command]
pub async fn argus_print_usb_status() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus print-usb")?;
    crate::sidecar::dispatch_paid_command("argus_print_usb_status", serde_json::Value::Null).await
}

/// Gate + delegate: fetch recent print+USB signals from Pro.
#[tauri::command]
pub async fn argus_print_usb_recent(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus print-usb")?;
    crate::sidecar::dispatch_paid_command(
        "get_argus_print_usb_recent",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

// ── Argus — tamper/evasion detector (Pro: tamper_monitor.rs) ─────────────
//
// PINNED feature_ids — do NOT rename. AV hygiene: only feature-id strings
// and generic JSON here — no flaggable strings in this file.
// record_argus_tamper_event is also called from hook sites (log.rs, sidecar.rs).

/// Gate + delegate: start the Pro tamper detector.
#[tauri::command]
pub async fn argus_tamper_start(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus tamper")?;
    crate::sidecar::dispatch_paid_command(
        "start_argus_tamper",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Gate + delegate: stop the Pro tamper detector.
#[tauri::command]
pub async fn argus_tamper_stop() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus tamper")?;
    crate::sidecar::dispatch_paid_command("stop_argus_tamper", serde_json::Value::Null).await
}

/// Gate + delegate: query tamper detector running status.
#[tauri::command]
pub async fn argus_tamper_status() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus tamper")?;
    crate::sidecar::dispatch_paid_command("argus_tamper_status", serde_json::Value::Null).await
}

/// Gate + delegate: fetch recent tamper signals from Pro.
#[tauri::command]
pub async fn argus_tamper_recent(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus tamper")?;
    crate::sidecar::dispatch_paid_command(
        "get_argus_tamper_recent",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Hook entry: record a tamper event in Pro. Best-effort — ignores errors.
/// This is called from hook sites (log.rs, sidecar.rs) even when the
/// collector loop is idle. Does NOT require require_paid because it may
/// be called from non-paid paths; the Pro side validates and enqueues.
///
/// AV hygiene: forwards command-id string + generic JSON args only.
/// The `signal` value is an opaque label forwarded to Pro — no flaggable content.
pub async fn record_tamper_event_hook(args: serde_json::Value) {
    // Spawn best-effort: ignore all errors (never panic, never block callers).
    tokio::spawn(async move {
        let _ = crate::sidecar::dispatch_paid_command("record_argus_tamper_event", args).await;
    });
}

// ── Argus — DLP / exfil monitor (Pro: dlp_monitor.rs) ────────────────────

/// Gate + delegate: start the Pro DLP monitor.
#[tauri::command]
pub async fn argus_dlp_start(args: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus dlp")?;
    crate::sidecar::dispatch_paid_command(
        "start_argus_dlp",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Gate + delegate: stop the Pro DLP monitor.
#[tauri::command]
pub async fn argus_dlp_stop() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus dlp")?;
    crate::sidecar::dispatch_paid_command("stop_argus_dlp", serde_json::Value::Null).await
}

/// Gate + delegate: query DLP monitor running status.
#[tauri::command]
pub async fn argus_dlp_status() -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus dlp")?;
    crate::sidecar::dispatch_paid_command("argus_dlp_status", serde_json::Value::Null).await
}

/// Gate + delegate: fetch recent DLP signals from Pro.
#[tauri::command]
pub async fn argus_dlp_recent(
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    crate::license::require_paid("argus dlp")?;
    crate::sidecar::dispatch_paid_command(
        "get_argus_dlp_recent",
        args.unwrap_or(serde_json::Value::Null),
    )
    .await
}
