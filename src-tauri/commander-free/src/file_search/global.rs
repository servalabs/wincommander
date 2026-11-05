// SPDX-License-Identifier: AGPL-3.0-or-later
//! Only ordinary, currently accessible files may enter the host index.
use super::privacy;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use wincmd_search::{types::IndexConfig, SearchEngine};

struct GlobalIndex {
    signature: String,
    engine: SearchEngine,
}
fn cell() -> &'static RwLock<Option<GlobalIndex>> {
    static ENGINE: OnceLock<RwLock<Option<GlobalIndex>>> = OnceLock::new();
    ENGINE.get_or_init(|| RwLock::new(None))
}

pub(super) fn with_engine<T>(
    config: IndexConfig,
    protected: &[PathBuf],
    operation: impl FnOnce(&SearchEngine) -> Result<T, String>,
) -> Result<T, String> {
    let signature = serde_json::to_string(&(&config, protected)).map_err(|e| e.to_string())?;
    {
        let read = cell().read().map_err(|_| "Search state is unavailable.")?;
        if let Some(current) = read.as_ref().filter(|c| c.signature == signature) {
            return operation(&current.engine);
        }
    }
    // Match other replacement paths: take the process mutex before the
    // in-process engine lock to avoid lock-order inversion.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    let _replacement = super::locks::IndexReplacementLock::acquire()?;
    let mut write = cell().write().map_err(|_| "Search state is unavailable.")?;
    // Another request may have initialized the same configuration while this
    // request waited for the replacement lock.
    if let Some(current) = write.as_ref().filter(|c| c.signature == signature) {
        return operation(&current.engine);
    }
    if let Some(old) = write.take() {
        old.engine.stop();
    }
    // Legacy setup can create files and modify ACLs, so the experimental
    // encrypted path must not run it before its own pinned-root checks.
    #[cfg(not(all(windows, feature = "experimental-encrypted-search")))]
    {
        std::fs::create_dir_all(&config.index_dir)
            .map_err(|_| "Cannot create the ordinary search index.")?;
        validate_directory(&config.index_dir)?;
        crate::paths::harden_dir_acl(&config.index_dir);
    }
    let blocked = protected.to_vec();
    let policy: wincmd_search::PathPolicy = Arc::new(move |path| privacy::ordinary_allowed(path, &blocked));
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    let engine = {
        // This feature remains disabled until native/recovery/Pro validation.
        SearchEngine::open_or_rebuild_experimental_with_policy(config.clone(), policy)
            .map_err(|e| e.to_string())?
    };
    #[cfg(not(all(windows, feature = "experimental-encrypted-search")))]
    let engine = SearchEngine::open_with_policy(config.clone(), policy)
        .map_err(|e| e.to_string())?;
    // Restore freshly read policy after any ordinary schema migration.
    super::prepare(false)?;
    #[cfg(not(all(windows, feature = "experimental-encrypted-search")))]
    engine.start_indexing().map_err(|e| e.to_string())?;
    *write = Some(GlobalIndex { signature, engine });
    operation(&write.as_ref().ok_or("Search state is unavailable.")?.engine)
}

pub(super) fn stop() -> Result<(), String> {
    let mut guard = cell().write().map_err(|_| "Search state is unavailable.")?;
    if let Some(old) = guard.take() {
        old.engine.stop();
    }
    Ok(())
}

fn validate_directory(directory: &std::path::Path) -> Result<(), String> {
    let volume = wincmd_volume::inspect_path(directory)?;
    if volume.is_private {
        return Err("The ordinary index requires a local non-private directory.".into());
    }
    Ok(())
}

/// Publish removals and remembered private roots even when ordinary indexing is
/// disabled. An incomplete/missing manifest is a denial to remote consumers.
pub(super) fn publish_scope(
    directory: &std::path::Path,
    roots: &[PathBuf],
    exclusions: &[String],
    protected: &[PathBuf],
) -> Result<(), String> {
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    {
        let bytes = serde_json::to_vec(&serde_json::json!({"version": 1, "roots": roots,
            "exclusions": exclusions, "blocked_roots": protected}))
            .map_err(|e| e.to_string())?;
        return wincmd_search::publish_experimental_scope(directory, &bytes, |path| {
            validate_directory(path).map_err(std::io::Error::other)
        }).map_err(|error| format!("Cannot publish the search scope: {error}"));
    }
    #[cfg(not(all(windows, feature = "experimental-encrypted-search")))]
    {
    std::fs::create_dir_all(directory).map_err(|_| "Cannot store the search scope.")?;
    validate_directory(directory)?;
    let path = directory.join("scope-v1.json");
    let bytes = serde_json::to_vec(&serde_json::json!({"version": 1, "roots": roots,
        "exclusions": exclusions, "blocked_roots": protected}))
    .map_err(|e| e.to_string())?;
    if path.exists() {
        wincmd_volume::inspect_path(&path)?;
    }
    if std::fs::read(&path).is_ok_and(|previous| previous == bytes) {
        return Ok(());
    }
    std::fs::write(path, bytes).map_err(|_| "Cannot publish the ordinary search scope.".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn removing_all_folders_publishes_an_empty_content_scope_and_retains_private_blocks() {
        let dir = tempfile::tempdir().unwrap();
        publish_scope(dir.path(), &[dir.path().join("Documents")], &[], &[]).unwrap();
        publish_scope(dir.path(), &[], &[], &[PathBuf::from(r"V:\")]).unwrap();
        let policy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("scope-v1.json")).unwrap())
                .unwrap();
        assert_eq!(policy["roots"], serde_json::json!([]));
        assert_eq!(policy["blocked_roots"], serde_json::json!([r"V:\"]));
        assert_eq!(policy["version"], 1);
    }
}

pub(super) fn remove(directory: &std::path::Path) -> Result<(), String> {
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    { let _ = directory; return Err("Encrypted search reset is not available yet.".into()); }
    #[cfg(not(all(windows, feature = "experimental-encrypted-search")))]
    {
    let mut guard = cell().write().map_err(|_| "Search state is unavailable.")?;
    if let Some(old) = guard.take() {
        old.engine.stop();
    }
    if directory.exists() {
        validate_directory(directory)?;
        std::fs::remove_dir_all(directory)
            .map_err(|_| "Old index could not be removed; rebuild stopped.")?;
    }
    Ok(())
    }
}
