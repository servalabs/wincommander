//! In-app facade over the same native secure-erase core used by Explorer's
//! non-elevating helper. Keeping the erase algorithm in its own crate prevents
//! the two entry points from drifting apart.

use tauri::AppHandle;

pub(crate) fn execute_cli(raw_paths: Vec<String>) -> Result<(), String> {
    commander_context_shred::execute_cli(raw_paths)
}

pub(crate) async fn execute(_app: AppHandle, raw_paths: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || execute_cli(raw_paths))
        .await
        .map_err(|error| format!("secure erase worker failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn secure_shred(
    path: String,
    capability_token: Option<String>,
) -> Result<(), String> {
    // Keep the original path until the erase library has checked and pinned
    // its namespace. Canonicalizing first could hide a linked ancestor.
    let prepared = commander_context_shred::prepare(vec![path.clone()])?;
    let canonical_path = crate::authz::canonical_path(&path);
    crate::authz::consume_required(
        capability_token.as_deref(),
        crate::authz::DestructiveAction::DiskDelete,
        &crate::authz::secure_erase_args(&canonical_path),
    )?;
    tokio::task::spawn_blocking(move || commander_context_shred::execute_prepared(prepared))
        .await
        .map_err(|error| format!("secure erase worker failed: {error}"))?
}

pub(crate) fn log_result(result: Result<(), String>) {
    match result {
        Ok(()) => crate::log_message_src("info", "core", "[ContextShred] completed"),
        Err(error) => {
            commander_context_shred::log_result(&error);
            crate::log_message_src("warn", "core", &format!("[ContextShred] {error}"));
        }
    }
}
