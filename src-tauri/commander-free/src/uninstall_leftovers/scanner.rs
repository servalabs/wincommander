use super::filesystem::is_link_or_reparse;
use super::installed::installed_tokens;
use super::{
    CachedFolder, UninstallLeftover, UninstallLeftoverScan, MAX_CANDIDATES, MIN_AGE, MIN_BYTES,
    SAFE_NAMES,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use uuid::Uuid;

pub(super) fn scan_leftovers(
    cancelled: &AtomicBool,
) -> (UninstallLeftoverScan, HashMap<String, CachedFolder>) {
    let installed = installed_tokens();
    let mut public = Vec::new();
    let mut cached = HashMap::new();
    let mut scanned = 0;
    let mut skipped = 0;
    let mut truncated = false;
    for (root, scope) in app_data_roots() {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if public.len() >= MAX_CANDIDATES {
                truncated = true;
                break;
            }
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                skipped += 1;
                continue;
            };
            if !meta.is_dir() || is_link_or_reparse(&meta) {
                skipped += 1;
                continue;
            }
            scanned += 1;
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_safe_name(&name) || matches_installed(&name, &installed) || is_recent(&meta) {
                skipped += 1;
                continue;
            }
            let Ok(bytes) = directory_size(&path, cancelled) else {
                skipped += 1;
                continue;
            };
            if bytes < MIN_BYTES {
                skipped += 1;
                continue;
            }
            let id = Uuid::new_v4().to_string();
            public.push(UninstallLeftover {
                id: id.clone(),
                name,
                path: path.to_string_lossy().into_owned(),
                bytes,
                scope: scope.into(),
            });
            cached.insert(
                id,
                CachedFolder {
                    path,
                    root: root.clone(),
                    modified: meta.modified().ok(),
                },
            );
        }
        if cancelled.load(Ordering::Acquire) || truncated {
            break;
        }
    }
    public.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    (
        UninstallLeftoverScan {
            entries: public,
            scanned_folders: scanned,
            skipped_folders: skipped,
            cancelled: cancelled.load(Ordering::Acquire),
            truncated,
        },
        cached,
    )
}

fn app_data_roots() -> Vec<(PathBuf, &'static str)> {
    [
        (std::env::var("LOCALAPPDATA").ok(), "localAppData"),
        (std::env::var("APPDATA").ok(), "roamingAppData"),
    ]
    .into_iter()
    .filter_map(|(value, scope)| {
        let path = PathBuf::from(value?);
        let meta = fs::symlink_metadata(&path).ok()?;
        (!is_link_or_reparse(&meta) && meta.is_dir())
            .then(|| fs::canonicalize(path).ok())
            .flatten()
            .map(|path| (path, scope))
    })
    .collect()
}

pub(super) fn is_safe_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with('.')
        || lower.starts_with('{')
        || SAFE_NAMES.contains(&lower.as_str())
        || [
            "microsoft",
            "windows",
            "system",
            "program",
            "intel",
            "nvidia",
            "node-v",
            "python",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}
pub(super) fn is_recent(meta: &fs::Metadata) -> bool {
    meta.modified()
        .ok()
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_none_or(|age| age < MIN_AGE)
}
fn directory_size(path: &Path, cancelled: &AtomicBool) -> Result<u64, String> {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(directory) = stack.pop() {
        if cancelled.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        for entry in fs::read_dir(directory)
            .map_err(|error| error.to_string())?
            .flatten()
        {
            let child = entry.path();
            let meta = fs::symlink_metadata(&child).map_err(|error| error.to_string())?;
            if is_link_or_reparse(&meta) {
                return Err("refused linked or reparse descendant".into());
            }
            if meta.is_dir() {
                stack.push(child);
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}
pub(super) fn matches_installed(name: &str, tokens: &HashSet<String>) -> bool {
    let name = name.to_ascii_lowercase();
    tokens.iter().any(|token| {
        token.len() >= 4 && (name == *token || name.contains(token) || token.contains(&name))
    })
}
