// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::settings::AppSettings;
use std::path::PathBuf;
/// Returns the user's personal folders that actually exist on disk.
/// Only `%USERPROFILE%\Desktop`, `\Downloads`, `\Documents` are seeded —
/// AppData is intentionally excluded (it's not under these roots anyway).
pub(crate) fn default_roots() -> Vec<PathBuf> {
    default_roots_from(std::env::var("USERPROFILE").ok())
}

/// Testable core of `default_roots`: accepts the USERPROFILE value directly
/// so unit tests can supply an arbitrary path without mutating the environment.
pub(super) fn default_roots_from(profile: Option<String>) -> Vec<PathBuf> {
    let profile = match profile {
        Some(p) => p,
        None => return Vec::new(),
    };
    ["Desktop", "Downloads", "Documents"]
        .iter()
        .map(|name| PathBuf::from(&profile).join(name))
        .filter(|p| p.exists())
        .collect()
}

/// Default exclusion globs — keep the index clean without burdening the user.
/// Applied only when the caller has an empty exclusions list.
const DEFAULT_EXCLUSIONS: &[&str] = &["node_modules", ".git", "*.tmp", "*.temp", "~$*"];

/// On the very first use (settings.app.file_search.initialized == false),
/// seeds roots from the user's personal folders and writes back to settings.
/// Returns the (potentially updated) settings so the caller can use them immediately.
pub(super) fn ensure_initialized(mut settings: AppSettings) -> Result<AppSettings, String> {
    if !settings.app.file_search.initialized {
        settings.app.file_search.roots = default_roots();
        settings.app.file_search.initialized = true;
        if settings.app.file_search.exclusions.is_empty() {
            settings.app.file_search.exclusions =
                DEFAULT_EXCLUSIONS.iter().map(|s| s.to_string()).collect();
        }
        settings = crate::settings::patch_settings(serde_json::json!({
            "app": {"fileSearch": {
                "roots": settings.app.file_search.roots,
                "exclusions": settings.app.file_search.exclusions,
                "initialized": true
            }}
        }))?;
    }
    Ok(settings)
}
