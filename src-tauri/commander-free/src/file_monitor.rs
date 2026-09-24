// SPDX-License-Identifier: AGPL-3.0-or-later
// commander-free/src/file_monitor.rs
//
// Pro boundary for the decoy-file feature. The Free binary intentionally has
// no filesystem watcher, Security-log parser, SACL writer, alert history, or
// Fleet reporter. Those implementation details live in the proprietary Pro
// sidecar. This small registry is neutral: Search and Safe Clip use it only to
// avoid reading a configured decoy themselves.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

static ENROLLED_PATHS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

fn enrolled() -> &'static Mutex<HashSet<PathBuf>> {
    ENROLLED_PATHS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A minimal neutral query used by Free's content-search and Safe Clip
/// exclusions. It neither opens nor observes the returned paths.
pub(crate) fn enrolled_decoy_paths() -> Vec<PathBuf> {
    enrolled()
        .lock()
        .map(|paths| paths.iter().cloned().collect())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoyInfo {
    pub path: String,
    pub exists: bool,
    /// The Pro registry supplies this provenance so the UI can offer a disk
    /// delete only for the safe, WinCommander-created placeholders. Older Pro
    /// sidecars omit it and therefore safely decode as a custom decoy.
    #[serde(default)]
    pub standard: bool,
}

/// The Pro sidecar persists this small, machine-shared registration record
/// before it replies. Free uses it only to recover a completed Drop action
/// whose reply pipe disappeared; it never treats the record as monitor health.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurableDecoyState {
    #[serde(default)]
    registrations: Vec<DurableDecoyRegistration>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurableDecoyRegistration {
    path: String,
    #[serde(default)]
    standard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoyAccessEvent {
    pub path: String,
    pub kind: String,
    pub detected_at: String,
    pub user_name: Option<String>,
    pub domain: Option<String>,
    pub sid: Option<String>,
    pub process_name: Option<String>,
    pub is_administrator: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecoyReadAuditStatus {
    pub enabled: bool,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastAccessStatus {
    pub enabled: bool,
    pub raw_value: u8,
    pub system_managed: bool,
}

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        let normalize = |path: &Path| {
            let rendered = path.to_string_lossy();
            rendered
                .strip_prefix(r"\\?\")
                .unwrap_or(rendered.as_ref())
                .replace('/', "\\")
                .to_ascii_lowercase()
        };
        normalize(left) == normalize(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn remember(path: PathBuf) {
    enrolled()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(path);
}

fn forget(path: &Path) {
    let mut paths = enrolled()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = paths
        .iter()
        .find(|candidate| same_path(candidate, path))
        .cloned()
    {
        paths.remove(&existing);
    }
}

fn replace_registry(items: &[DecoyInfo]) {
    let mut paths = enrolled()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    paths.clear();
    paths.extend(items.iter().map(|item| PathBuf::from(&item.path)));
}

fn replace_registry_paths(items: &[String]) {
    let mut paths = enrolled()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    paths.clear();
    paths.extend(items.iter().map(PathBuf::from));
}

fn is_transport_error(error: &str) -> bool {
    error.starts_with("agent transport error:")
        || error.starts_with("Pro transport error:")
        || error.starts_with("Pro reader exited")
        || error.starts_with("write request:")
}

fn standard_decoys_from_state(state: &str) -> Result<Vec<String>, String> {
    let state: DurableDecoyState = serde_json::from_str(state)
        .map_err(|error| format!("parse durable decoy registry: {error}"))?;
    Ok(state
        .registrations
        .into_iter()
        .filter(|registration| registration.standard && Path::new(&registration.path).is_file())
        .map(|registration| registration.path)
        .collect())
}

/// A Drop request creates and registers files durably before the Pro process
/// writes its response. If that final response is lost, recover only the
/// already-registered standard files rather than telling the user nothing
/// happened or replaying a file-creation command blindly.
fn recover_completed_standard_drop() -> Result<Vec<String>, String> {
    let base = std::env::var_os("ProgramData")
        .or_else(|| std::env::var_os("ALLUSERSPROFILE"))
        .ok_or_else(|| "ProgramData is unavailable for the Decoy File Monitor".to_string())?;
    let state_path = PathBuf::from(base)
        .join("WinCommander")
        .join("decoy-monitor")
        .join("decoy-monitor.json");
    let state = std::fs::read_to_string(&state_path)
        .map_err(|error| format!("read durable decoy registry: {error}"))?;
    standard_decoys_from_state(&state)
}

async fn pro(feature: &str, args: Value) -> Result<Value, String> {
    crate::license::require_paid("Decoy File Monitor")?;
    crate::sidecar::dispatch_paid_command(feature, args).await
}

/// Removing protection remains possible after a licence lapse. This uses the
/// matching Pro-side lifecycle-cleanup allowlist; it must never gain a broad
/// feature-id parameter or become a bypass for inspection/configuration.
async fn stop_pro_after_expiry() -> Result<Value, String> {
    crate::sidecar::dispatch_paid_command("stop_decoy_monitor", Value::Null).await
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value, label: &str) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("invalid Pro {label} response: {error}"))
}

#[tauri::command]
pub async fn start_decoy_monitor(
    _app: AppHandle,
    paths: Vec<String>,
    read_audit_enabled: bool,
    fleet_alert_enabled: Option<bool>,
) -> Result<(), String> {
    let registry_paths = paths.clone();
    let status = pro(
        "start_decoy_monitor",
        json!({
            "paths": paths,
            "readAuditEnabled": read_audit_enabled,
            "fleetAlertEnabled": fleet_alert_enabled.unwrap_or(false),
        }),
    )
    .await?;
    // Pro returns its canonical additive registry, which may include an RDS
    // session's earlier registration absent from this Free settings snapshot.
    let canonical_paths = status
        .get("paths")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or(registry_paths);
    replace_registry_paths(&canonical_paths);
    Ok(())
}

#[tauri::command]
pub async fn stop_decoy_monitor() -> Result<(), String> {
    stop_pro_after_expiry().await?;
    Ok(())
}

#[tauri::command]
pub async fn decoy_monitor_status() -> Result<bool, String> {
    let status = pro("decoy_monitor_status", Value::Null).await?;
    status
        .get("running")
        .and_then(Value::as_bool)
        .ok_or_else(|| "invalid Pro decoy status response".to_string())
}

#[tauri::command]
pub async fn enroll_decoy(path: String) -> Result<(), String> {
    pro("enroll_decoy", json!({ "path": path })).await?;
    remember(PathBuf::from(path));
    Ok(())
}

#[tauri::command]
pub async fn remove_decoy(path: String) -> Result<(), String> {
    pro("remove_decoy", json!({ "path": path })).await?;
    forget(Path::new(&path));
    Ok(())
}

#[tauri::command]
pub async fn list_decoys() -> Result<Vec<DecoyInfo>, String> {
    let items: Vec<DecoyInfo> = match pro("list_decoys", Value::Null).await {
        Ok(value) => decode(value, "decoy list")?,
        Err(error) if is_transport_error(&error) => recover_completed_standard_drop()?
            .into_iter()
            .map(|path| DecoyInfo {
                exists: Path::new(&path).is_file(),
                path,
                standard: true,
            })
            .collect(),
        Err(error) => return Err(error),
    };
    replace_registry(&items);
    Ok(items)
}

#[tauri::command]
pub async fn drop_standard_decoys() -> Result<Vec<String>, String> {
    let paths: Vec<String> = match pro("drop_standard_decoys", Value::Null).await {
        Ok(value) => decode(value, "standard decoys")?,
        Err(error) if is_transport_error(&error) => {
            let recovered = recover_completed_standard_drop()?;
            if recovered.is_empty() {
                return Err(error);
            }
            crate::log_message(
                "warn",
                "[Decoy] recovered a completed standard-decoy drop after the Pro reply pipe closed",
            );
            recovered
        }
        Err(error) => return Err(error),
    };
    for path in &paths {
        remember(PathBuf::from(path));
    }
    Ok(paths)
}

#[tauri::command]
pub async fn delete_decoy(path: String, capability_token: Option<String>) -> Result<(), String> {
    let canonical_path = crate::authz::canonical_path(&path);
    crate::authz::consume_required(
        capability_token.as_deref(),
        crate::authz::DestructiveAction::DecoyDelete,
        &crate::authz::decoy_delete_args(&canonical_path),
    )?;
    pro("delete_decoy", json!({ "path": canonical_path })).await?;
    forget(Path::new(&path));
    Ok(())
}

#[tauri::command]
pub async fn get_decoy_recent() -> Result<Vec<DecoyAccessEvent>, String> {
    decode(
        pro("get_decoy_recent", Value::Null).await?,
        "decoy recent events",
    )
}

#[tauri::command]
pub async fn clear_decoy_recent() -> Result<(), String> {
    pro("clear_decoy_recent", Value::Null).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_decoy_read_audit_enabled(
    _app: AppHandle,
    enabled: bool,
) -> Result<DecoyReadAuditStatus, String> {
    decode(
        pro(
            "set_decoy_read_audit_enabled",
            json!({ "enabled": enabled }),
        )
        .await?,
        "read-audit status",
    )
}

#[tauri::command]
pub async fn decoy_read_audit_status() -> Result<DecoyReadAuditStatus, String> {
    decode(
        pro("decoy_read_audit_status", Value::Null).await?,
        "read-audit status",
    )
}

#[tauri::command]
pub async fn get_last_access_tracking_status() -> Result<LastAccessStatus, String> {
    decode(
        pro("get_last_access_tracking_status", Value::Null).await?,
        "last-access status",
    )
}

#[tauri::command]
pub async fn enable_last_access_tracking() -> Result<(), String> {
    pro("enable_last_access_tracking", Value::Null).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_drop_recovery_keeps_only_standard_registered_files() {
        let existing = tempfile::NamedTempFile::new().unwrap();
        let missing = existing.path().with_extension("missing");
        let escaped = existing.path().to_string_lossy().replace('\\', "\\\\");
        let missing = missing.to_string_lossy().replace('\\', "\\\\");
        let state = format!(
            r#"{{"registrations":[{{"path":"{escaped}","standard":true}},{{"path":"{missing}","standard":true}},{{"path":"{escaped}","standard":false}}]}}"#
        );

        assert_eq!(
            standard_decoys_from_state(&state).unwrap(),
            vec![existing.path().to_string_lossy().to_string()]
        );
    }

    #[test]
    fn old_pro_decoy_lists_decode_as_non_deletable_custom_rows() {
        let rows: Vec<DecoyInfo> = decode(
            serde_json::json!([{ "path": r"C:\\Users\\Alex\\Desktop\\custom.txt", "exists": true }]),
            "decoy list",
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert!(!rows[0].standard);
    }

    #[test]
    fn neutral_registry_does_not_treat_case_variants_as_distinct_on_windows() {
        let first = Path::new(r"C:\Users\Alex\Desktop\decoy.txt");
        let second = Path::new(r"\\?\c:\users\alex\desktop\DECOY.txt");
        #[cfg(windows)]
        assert!(same_path(first, second));
        #[cfg(not(windows))]
        assert!(!same_path(first, second));
    }

    #[test]
    fn advanced_bridge_has_a_single_paid_choke_point() {
        let source = include_str!("file_monitor.rs");
        assert!(source.contains("crate::license::require_paid(\"Decoy File Monitor\")"));
        let watcher_namespace = ["notify", "::"].concat();
        assert!(!source.contains(&watcher_namespace));
        let event_utility = ["wevtutil", ".exe"].concat();
        let audit_utility = ["auditpol", ".exe"].concat();
        assert!(!source.contains(&event_utility));
        assert!(!source.contains(&audit_utility));
    }

    #[test]
    fn only_the_monitor_stop_can_bypass_the_paid_bridge_after_expiry() {
        let source = include_str!("file_monitor.rs");
        assert!(source.contains("dispatch_paid_command(\"stop_decoy_monitor\""));
        assert!(source.contains("crate::license::require_paid(\"Decoy File Monitor\")"));
    }

    #[test]
    fn atomic_start_forwards_the_complete_persisted_configuration() {
        let source = include_str!("file_monitor.rs");
        assert!(source.contains("paths: Vec<String>"));
        assert!(source.contains("read_audit_enabled: bool"));
        assert!(source.contains("\"readAuditEnabled\": read_audit_enabled"));
        assert!(source.contains("\"fleetAlertEnabled\": fleet_alert_enabled"));
        assert!(source.contains("let registry_paths = paths.clone()"));
        assert!(source.contains("let canonical_paths = status"));
        assert!(source.contains("replace_registry_paths(&canonical_paths)"));
    }
}
