use super::filesystem::is_link_or_reparse;
use super::{CachedShortcut, ShortcutRemoveResult};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn remove_shortcuts(
    entries: Vec<CachedShortcut>,
    cancelled: &AtomicBool,
) -> ShortcutRemoveResult {
    let mut result = ShortcutRemoveResult {
        removed: 0,
        cancelled: false,
        errors: Vec::new(),
    };
    for entry in entries {
        if cancelled.load(Ordering::Acquire) {
            result.cancelled = true;
            break;
        }
        match validate_shortcut(&entry)
            .and_then(|path| fs::remove_file(path).map_err(|error| error.to_string()))
        {
            Ok(()) => result.removed += 1,
            Err(error) => result.errors.push(error),
        }
    }
    result
}

fn validate_shortcut(entry: &CachedShortcut) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(&entry.path).map_err(|error| error.to_string())?;
    if !meta.is_file()
        || is_link_or_reparse(&meta)
        || meta.len() != entry.bytes
        || meta.modified().ok() != entry.modified
    {
        return Err("refused linked or changed shortcut".into());
    }
    let path = fs::canonicalize(&entry.path).map_err(|error| error.to_string())?;
    if !path.starts_with(&entry.root) || super::scanner::is_system_shortcut_directory(&path) {
        return Err("refused shortcut outside its safe scan root".into());
    }
    Ok(path)
}
