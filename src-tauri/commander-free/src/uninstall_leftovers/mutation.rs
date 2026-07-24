use super::filesystem::is_link_or_reparse;
use super::scanner::is_safe_name;
use super::{CachedFolder, UninstallLeftoverRemoveResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn remove_leftovers(
    entries: Vec<CachedFolder>,
    cancelled: &AtomicBool,
) -> UninstallLeftoverRemoveResult {
    let mut result = UninstallLeftoverRemoveResult {
        removed: 0,
        bytes_recovered: 0,
        cancelled: false,
        errors: Vec::new(),
    };
    for entry in entries {
        if cancelled.load(Ordering::Acquire) {
            result.cancelled = true;
            break;
        }
        match validate_folder(&entry).and_then(|path| remove_tree(&path, cancelled)) {
            Ok(bytes) => {
                result.removed += 1;
                result.bytes_recovered = result.bytes_recovered.saturating_add(bytes);
            }
            Err(error) => result.errors.push(error),
        }
    }
    result
}
fn validate_folder(entry: &CachedFolder) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(&entry.path).map_err(|error| error.to_string())?;
    if !meta.is_dir() || is_link_or_reparse(&meta) || meta.modified().ok() != entry.modified {
        return Err("refused linked or changed leftover folder".into());
    }
    let path = fs::canonicalize(&entry.path).map_err(|error| error.to_string())?;
    if path.parent() != Some(entry.root.as_path())
        || is_safe_name(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
        )
    {
        return Err("refused folder outside its safe scan root".into());
    }
    Ok(path)
}
fn remove_tree(path: &Path, cancelled: &AtomicBool) -> Result<u64, String> {
    let mut stack = vec![(path.to_path_buf(), false)];
    let mut bytes = 0u64;
    while let Some((current, visited)) = stack.pop() {
        if cancelled.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        let meta = fs::symlink_metadata(&current).map_err(|error| error.to_string())?;
        if is_link_or_reparse(&meta) {
            return Err("refused linked or reparse descendant".into());
        }
        if visited {
            fs::remove_dir(&current).map_err(|error| error.to_string())?;
            continue;
        }
        stack.push((current.clone(), true));
        for entry in fs::read_dir(&current)
            .map_err(|error| error.to_string())?
            .flatten()
        {
            let child = entry.path();
            let child_meta = fs::symlink_metadata(&child).map_err(|error| error.to_string())?;
            if is_link_or_reparse(&child_meta) {
                return Err("refused linked or reparse descendant".into());
            }
            if child_meta.is_dir() {
                stack.push((child, false));
            } else if child_meta.is_file() {
                bytes = bytes.saturating_add(child_meta.len());
                fs::remove_file(child).map_err(|error| error.to_string())?;
            } else {
                return Err("refused special filesystem entry".into());
            }
        }
    }
    Ok(bytes)
}
