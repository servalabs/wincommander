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
    let mut write = cell().write().map_err(|_| "Search state is unavailable.")?;
    if let Some(old) = write.take() {
        old.engine.stop();
    }
    std::fs::create_dir_all(&config.index_dir)
        .map_err(|_| "Cannot create the ordinary search index.")?;
    validate_directory(&config.index_dir)?;
    crate::paths::harden_dir_acl(&config.index_dir);
    let blocked = protected.to_vec();
    let engine = SearchEngine::open_with_policy(
        config.clone(),
        Arc::new(move |path| privacy::ordinary_allowed(path, &blocked)),
    )
    .map_err(|e| e.to_string())?;
    // A schema migration may remove the manifest. Restore freshly read policy,
    // never this request's older snapshot after a concurrent folder removal.
    super::prepare(false)?;
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
