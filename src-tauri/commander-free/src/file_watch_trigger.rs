// F-6: file-system event lockdown trigger.
//
// Watches user-configured directory paths for file creation or deletion
// matching a name pattern. When a match fires it emits `lockdown-trigger`
// to the frontend — the same event the panic hotkey sends, so the
// existing 4-second countdown (or silent hotkey path) takes over.
//
// Uses the `notify` crate (already a dependency for decoy monitor / ransomware
// monitor). Each `start_file_watch_triggers` call drops the previous watcher
// and creates a fresh one from the new rule set, so enable/disable/rule-change
// from the UI are all handled by re-calling start.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize, Clone)]
pub struct FileWatchRule {
    pub id: String,
    pub path: String,
    #[serde(rename = "namePattern")]
    pub name_pattern: String,
    pub event: String, // "created" | "deleted"
    pub enabled: bool,
}

#[derive(Default)]
pub struct FileWatchTriggerState {
    watcher: Option<RecommendedWatcher>,
}

/// Simple glob matcher: supports `*` as a wildcard for any sequence of chars.
/// Case-insensitive. An empty pattern or `*` matches everything.
fn matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    let name_lc = name.to_lowercase();
    let pat_lc = pattern.to_lowercase();

    if !pat_lc.contains('*') {
        return name_lc == pat_lc;
    }

    // Split by '*' and check sub-strings in order.
    let parts: Vec<&str> = pat_lc.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !name_lc.starts_with(part as &str) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            if !name_lc.ends_with(part as &str) {
                return false;
            }
        } else {
            match name_lc[pos..].find(part as &str) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

#[tauri::command]
pub fn start_file_watch_triggers(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<FileWatchTriggerState>>,
    rules: Vec<FileWatchRule>,
) -> Result<serde_json::Value, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    // Drop previous watcher first so old paths are unregistered.
    guard.watcher = None;

    let enabled: Vec<FileWatchRule> = rules.into_iter().filter(|r| r.enabled).collect();
    if enabled.is_empty() {
        return Ok(serde_json::json!({ "success": true, "watching": 0 }));
    }

    let rules_arc: Arc<Vec<FileWatchRule>> = Arc::new(enabled);
    let rules_cb = rules_arc.clone();
    let app_cb = app.clone();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let event = match res {
            Ok(e) => e,
            Err(_) => return,
        };

        let event_type = match &event.kind {
            EventKind::Create(_) => "created",
            EventKind::Remove(_) => "deleted",
            _ => return,
        };

        for path in &event.paths {
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            let parent = path.parent().and_then(|p| p.to_str()).unwrap_or("");

            for rule in rules_cb.iter() {
                if rule.event != event_type {
                    continue;
                }
                // Normalise separators for comparison.
                let rule_path_norm = rule
                    .path
                    .replace('/', "\\")
                    .trim_end_matches('\\')
                    .to_string();
                let parent_norm = parent.replace('/', "\\").trim_end_matches('\\').to_string();
                if !parent_norm.eq_ignore_ascii_case(&rule_path_norm) {
                    continue;
                }
                if matches_pattern(file_name, &rule.name_pattern) {
                    crate::log_message(
                        "info",
                        &format!(
                            "[FileWatch] rule '{}' fired: {} '{}' in '{}'",
                            rule.id, event_type, file_name, parent
                        ),
                    );
                    crate::authz::schedule_trusted_lockdown(app_cb.clone());
                    return; // one match is enough
                }
            }
        }
    })
    .map_err(|e| format!("Failed to create file watcher: {}", e))?;

    // Collect unique watch paths and register them.
    let watch_paths: std::collections::HashSet<String> =
        rules_arc.iter().map(|r| r.path.clone()).collect();
    let mut registered = 0usize;
    for p in &watch_paths {
        let path = std::path::Path::new(p);
        if path.is_dir() {
            watcher
                .watch(path, RecursiveMode::NonRecursive)
                .map_err(|e| format!("Cannot watch '{}': {}", p, e))?;
            registered += 1;
        }
        // If the path doesn't exist yet, skip silently — users might type the
        // path before creating it. The watch simply won't fire until the next
        // call to start (after the user clicks Save again).
    }

    guard.watcher = Some(watcher);
    Ok(serde_json::json!({ "success": true, "watching": registered }))
}

#[tauri::command]
pub fn stop_file_watch_triggers(
    state: tauri::State<'_, Mutex<FileWatchTriggerState>>,
) -> Result<serde_json::Value, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.watcher = None;
    Ok(serde_json::json!({ "success": true }))
}
