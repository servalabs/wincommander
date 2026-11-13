use super::CachedFile;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn remove_files(
    files: Vec<CachedFile>,
    keepers: Vec<CachedFile>,
    cancelled: &AtomicBool,
) -> super::DuplicateRemoveResult {
    let mut result = super::DuplicateRemoveResult {
        files_removed: 0,
        bytes_recovered: 0,
        cancelled: false,
        errors: Vec::new(),
    };
    let policy = RemovalPolicy::load();
    for file in files {
        if cancelled.load(Ordering::Acquire) {
            result.cancelled = true;
            break;
        }
        if let Some(reason) = policy.protected_reason(&file.path) {
            result.errors.push(format!("refused removal: {reason}"));
            continue;
        }
        let keeper = keepers
            .iter()
            .filter(|keeper| keeper.group_id == file.group_id)
            .find_map(|keeper| super::verified_file::open_verified(keeper, false, cancelled).ok());
        let Some(_keeper_guard) = keeper else {
            result
                .errors
                .push("refused removal: no unchanged verified copy could be retained".into());
            continue;
        };
        match super::verified_file::open_verified(&file, true, cancelled) {
            Ok(handle) => match super::verified_file::delete_verified(handle, &file) {
                Ok(()) => {
                    result.files_removed += 1;
                    result.bytes_recovered = result.bytes_recovered.saturating_add(file.bytes);
                }
                Err(error) => result.errors.push(error.to_string()),
            },
            Err(error) => result.errors.push(error),
        }
    }
    result.cancelled |= cancelled.load(Ordering::Acquire);
    result
}

pub(super) fn validate_roots(roots: Vec<String>) -> Result<Vec<PathBuf>, String> {
    if roots.is_empty()
        || roots.len() > super::MAX_ROOTS
        || roots.iter().any(|root| root.len() > 4096)
    {
        return Err("provide between one and thirty-two valid folders".into());
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

pub(super) fn validated_file(file: &CachedFile) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(&file.path).map_err(|error| error.to_string())?;
    if !meta.is_file()
        || is_link_or_reparse(&meta)
        || meta.len() != file.bytes
        || meta.modified().ok() != file.modified
    {
        return Err("refused linked or changed duplicate file".into());
    }
    let path = fs::canonicalize(&file.path).map_err(|error| error.to_string())?;
    if !path.starts_with(&file.root) || is_inside_system_root(&path) {
        return Err("refused file outside its scanned safe root".into());
    }
    let mut ancestor = file.path.parent();
    while let Some(parent) = ancestor {
        let metadata = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
        if is_link_or_reparse(&metadata) {
            return Err("refused linked duplicate parent folder".into());
        }
        if parent == file.root {
            break;
        }
        ancestor = parent.parent();
    }
    Ok(path)
}

pub(super) fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: duplicate removal is unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err(
            "Refused: investigator mode forbids duplicate removal because it would taint evidence."
                .into(),
        );
    }
    Ok(())
}

pub(super) fn is_inside_system_root(path: &Path) -> bool {
    protected_system_roots()
        .iter()
        .any(|root| path == root || path.starts_with(root))
}

pub(super) struct RemovalPolicy {
    system_roots: Vec<PathBuf>,
    #[cfg(windows)]
    profiles: super::profile_policy::Snapshot,
}

pub(super) fn removal_block_code(reason: &str) -> &'static str {
    match reason {
        "Windows or application location is protected" => "protected_location",
        "application, executable or codec file is protected" => "protected_type",
        "application data is protected" => "application_data",
        "Git metadata is protected" => "git_metadata",
        "another user profile is protected" => "other_profile",
        "user profile locations could not be verified" => "profiles_unavailable",
        "file ownership could not be verified as the current user" => "owner_unavailable",
        _ => "unknown",
    }
}

impl RemovalPolicy {
    pub(super) fn profile(&self, path: &Path) -> (&'static str, Option<String>) {
        #[cfg(windows)]
        {
            let (scope, root) = self.profiles.describe(path);
            (scope, root.and_then(super::scope::display_path))
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            ("unavailable", None)
        }
    }
    pub(super) fn load() -> Self {
        Self {
            system_roots: protected_system_roots(),
            #[cfg(windows)]
            profiles: super::profile_policy::Snapshot::load(),
        }
    }

    /// Paths remain reviewable even when removal is prohibited.
    pub(super) fn protected_reason(&self, path: &Path) -> Option<&'static str> {
        // Protect literal .git directory components, including case variants.
        // This does not identify bare repositories or external Git directories.
        if super::git_metadata::contains_git_metadata_component(path) {
            return Some("Git metadata is protected");
        }
        if self
            .system_roots
            .iter()
            .any(|root| path == root || path.starts_with(root))
        {
            return Some("Windows or application location is protected");
        }
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "exe"
                | "dll"
                | "sys"
                | "ocx"
                | "ax"
                | "acm"
                | "drv"
                | "cpl"
                | "scr"
                | "com"
                | "msi"
                | "msp"
                | "msix"
                | "appx"
                | "bat"
                | "cmd"
                | "ps1"
                | "vbs"
                | "js"
                | "jar"
        ) {
            return Some("application, executable or codec file is protected");
        }
        if path.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("AppData")
        }) {
            return Some("application data is protected");
        }
        #[cfg(windows)]
        if let Err(reason) = self.profiles.check(path) {
            return Some(reason);
        }
        None
    }
}

#[cfg(test)]
pub(super) fn protected_removal_reason(path: &Path) -> Option<&'static str> {
    RemovalPolicy::load().protected_reason(path)
}

/// Validates a cached duplicate only for opening it in Explorer.  This is
/// deliberately narrower than deletion validation: protected Windows and
/// application locations may be inspected, but they remain unavailable for
/// duplicate removal.  The frontend supplies only an opaque scan/file ID; the
/// path always comes from the current scan cache.
pub(super) fn validated_review_file(file: &CachedFile) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(&file.path).map_err(|error| error.to_string())?;
    if !meta.is_file()
        || is_link_or_reparse(&meta)
        || meta.len() != file.bytes
        || meta.modified().ok() != file.modified
    {
        return Err("duplicate file changed after scanning; scan again".into());
    }
    let path = fs::canonicalize(&file.path).map_err(|error| error.to_string())?;
    if !path.starts_with(&file.root) {
        return Err("refused file outside its scanned location".into());
    }
    let mut ancestor = file.path.parent();
    while let Some(parent) = ancestor {
        let metadata = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
        if is_link_or_reparse(&metadata) {
            return Err("refused linked duplicate parent folder".into());
        }
        if parent == file.root {
            break;
        }
        ancestor = parent.parent();
    }
    Ok(path)
}

pub(super) fn protected_system_roots() -> Vec<PathBuf> {
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
