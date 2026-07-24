use super::CachedFolder;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn remove_folders(
    folders: Vec<CachedFolder>,
    cancelled: &AtomicBool,
) -> super::EmptyFolderRemoveResult {
    let mut result = super::EmptyFolderRemoveResult {
        folders_removed: 0,
        cancelled: false,
        errors: Vec::new(),
    };
    // Child folders first: selecting a parent and child works as two safe, non-recursive removes.
    let mut folders = folders;
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.path.components().count()));
    for folder in folders {
        if cancelled.load(Ordering::Acquire) {
            result.cancelled = true;
            break;
        }
        match validated_empty_folder(&folder) {
            Ok(path) => match fs::remove_dir(&path) {
                Ok(()) => result.folders_removed += 1,
                Err(error) => result.errors.push(error.to_string()),
            },
            Err(error) => result.errors.push(error),
        }
    }
    result
}

pub(super) fn validate_roots(roots: Vec<String>) -> Result<Vec<PathBuf>, String> {
    if roots.is_empty()
        || roots.len() > super::MAX_ROOTS
        || roots.iter().any(|root| root.len() > 4096)
    {
        return Err("provide between one and eight valid folders".into());
    }
    roots
        .into_iter()
        .map(|root| {
            let supplied = PathBuf::from(&root);
            let supplied_meta = fs::symlink_metadata(&supplied)
                .map_err(|_| format!("cannot resolve scan folder: {root}"))?;
            if is_link_or_reparse(&supplied_meta) {
                return Err("refused protected, linked, or non-folder scan root".into());
            }
            let path = fs::canonicalize(&root)
                .map_err(|_| format!("cannot resolve scan folder: {root}"))?;
            let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if !meta.is_dir() || is_link_or_reparse(&meta) || is_inside_system_root(&path) {
                Err("refused protected, linked, or non-folder scan root".into())
            } else {
                Ok(path)
            }
        })
        .collect()
}

fn validated_empty_folder(folder: &CachedFolder) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(&folder.path).map_err(|error| error.to_string())?;
    if !meta.is_dir() || is_link_or_reparse(&meta) {
        return Err("refused linked or changed folder".into());
    }
    let path = fs::canonicalize(&folder.path).map_err(|error| error.to_string())?;
    if path == folder.root || !path.starts_with(&folder.root) || is_inside_system_root(&path) {
        return Err("refused folder outside its scanned safe root".into());
    }
    if fs::read_dir(&path)
        .map_err(|error| error.to_string())?
        .next()
        .is_some()
    {
        return Err("refused folder that is no longer empty".into());
    }
    Ok(path)
}

pub(super) fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: empty-folder removal is unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode forbids empty-folder removal because it would taint evidence.".into());
    }
    Ok(())
}

fn is_inside_system_root(path: &Path) -> bool {
    protected_system_roots()
        .iter()
        .any(|root| path == root || path.starts_with(root))
}

fn protected_system_roots() -> Vec<PathBuf> {
    [
        "SystemRoot",
        "windir",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok())
    .filter_map(|value| fs::canonicalize(value).ok())
    .chain(
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .and_then(|path| fs::canonicalize(path).ok()),
    )
    .collect()
}

#[cfg(windows)]
pub(super) fn is_link_or_reparse(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
pub(super) fn is_link_or_reparse(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}
