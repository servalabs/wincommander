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

pub(crate) fn log_result(result: Result<(), String>) {
    match result {
        Ok(()) => crate::log_message_src("info", "core", "[ContextShred] completed"),
        Err(error) => {
            commander_context_shred::log_result(&error);
            crate::log_message_src("warn", "core", &format!("[ContextShred] {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn explorer_shred_is_handled_before_single_instance_forwarding() {
        let app_startup = include_str!("lib.rs");
        let direct_runner = app_startup
            .find("context_menu_shred::execute_cli(paths)")
            .expect("Explorer shred must have a standalone runner");
        let instance_guard = app_startup
            .find("session_instance::acquire(&cli_args)")
            .expect("single-instance guard must remain present");

        assert!(direct_runner < instance_guard);
        assert!(!app_startup.contains("app_handle.exit(0);"));
    }

    #[test]
    fn in_app_and_explorer_paths_share_the_same_native_core() {
        let source = include_str!("context_menu_shred.rs");
        assert!(source.contains("commander_context_shred::execute_cli(raw_paths)"));
    }
}
