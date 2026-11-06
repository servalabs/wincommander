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
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    super::runtime_fence::require_runtime_fence()?;
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
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    super::runtime_fence::require_runtime_fence()?;
    // Another request may have initialized the same configuration while this
    // request waited for the replacement lock.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
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
    let result = operation(&write.as_ref().ok_or("Search state is unavailable.")?.engine);
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    {
        drop(write);
        if result.is_ok() { schedule_legacy_cleanup(config, protected.to_vec()); }
    }
    result
}

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
static AUTO_CLEANUP_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
static AUTO_CLEANUP_MESSAGE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(super) fn automatic_cleanup_message() -> Option<String> {
    AUTO_CLEANUP_MESSAGE.lock().ok().and_then(|message| message.clone())
}

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
fn legacy_data_may_remain(directory: &std::path::Path) -> bool {
    match std::fs::read_dir(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
        Ok(entries) => entries.into_iter().any(|entry| match entry {
            Err(_) => true,
            Ok(entry) => !matches!(entry.file_name().to_str(),
                Some("scope-v2.dpapi" | ".tantivy-writer.lock" | ".tantivy-meta.lock")),
        }),
    }
}

#[cfg(all(test, windows, feature = "experimental-encrypted-search"))]
mod cleanup_tests {
    use super::*;
    #[test]
    fn automatic_cleanup_detects_legacy_but_not_protected_scope_only() {
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        assert!(!legacy_data_may_remain(&old));
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("scope-v2.dpapi"), b"protected").unwrap();
        std::fs::write(old.join(".tantivy-writer.lock"), b"").unwrap();
        assert!(!legacy_data_may_remain(&old));
        std::fs::write(old.join("scope-v1.json"), b"old").unwrap();
        assert!(legacy_data_may_remain(&old));
        std::fs::remove_file(old.join("scope-v1.json")).unwrap();
        std::fs::write(old.join("unexpected"), b"do not delete").unwrap();
        assert!(legacy_data_may_remain(&old));
    }
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
fn schedule_legacy_cleanup(config: IndexConfig, protected: Vec<PathBuf>) {
    use std::sync::atomic::Ordering;
    // A protected scope and Tantivy lock alone contain no old index data.
    let requires_cleanup = legacy_data_may_remain(&config.index_dir);
    if !requires_cleanup || AUTO_CLEANUP_STARTED.swap(true, Ordering::AcqRel) { return; }
    let spawned = std::thread::Builder::new().name("search-legacy-cleanup".into()).spawn(move || {
        let outcome = (|| {
            let _replacement = super::locks::IndexReplacementLock::acquire()?;
            cleanup_legacy(&config, &protected)
        })();
        let message = match outcome {
            Ok((_, 0)) => None,
            Ok((_, count)) => Some(format!("Old search backups remain in WinCommander storage ({count} folders).")),
            Err(error) => Some(format!("Old search data could not be cleaned: {error}")),
        };
        if let Ok(mut slot) = AUTO_CLEANUP_MESSAGE.lock() { *slot = message; }
    });
    if spawned.is_err() {
        if let Ok(mut slot) = AUTO_CLEANUP_MESSAGE.lock() { *slot = Some("Old search data cleanup could not start.".into()); }
    }
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(super) fn cleanup_legacy(config: &IndexConfig, protected: &[PathBuf]) -> Result<(usize, usize), String> {
    let expected = serde_json::to_string(&(config, protected)).map_err(|e| e.to_string())?;
    let mut write = cell().write().map_err(|_| "Search state is unavailable.")?;
    let current = write.as_mut().ok_or("Open search and let it finish indexing before cleanup.")?;
    if current.signature != expected { return Err("Search settings changed. Reopen search before cleanup.".into()); }
    let parent = config.index_dir.parent().ok_or("Search location unavailable.")?;
    // Quiesce the watcher while the shared process lease is still held.
    current.engine.stop();
    let result = super::runtime_fence::with_exclusive(|fence| {
        let mut backup_count = 0usize;
        for entry in std::fs::read_dir(parent).map_err(|_| "Cannot inspect old search backups.")? {
            let entry = entry.map_err(|_| "Cannot inspect old search backups.")?;
            if entry.file_name().to_string_lossy().starts_with(".wincommander-legacy-index-backup-") { backup_count += 1; }
        }
        let removed = current.engine.cleanup_legacy_cache_after_reconciliation(fence).map_err(|e| e.to_string())?;
        Ok((removed, backup_count))
    });
    // Cleanup stops its watcher, and even failed maintenance must reopen it.
    if let Some(old) = write.take() { old.engine.stop(); }
    result
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

#[cfg(all(test, windows, not(feature = "experimental-encrypted-search")))]
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

#[cfg(all(test, windows, feature = "experimental-encrypted-search"))]
mod encrypted_scope_tests {
    use super::*;
    #[test]
    fn scope_update_writes_protected_data_and_preserves_legacy_file() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("file-search").join("fts");
        std::fs::create_dir_all(&directory).unwrap();
        let old = directory.join("scope-v1.json");
        std::fs::write(&old, b"legacy retained").unwrap();
        publish_scope(&directory, &[root.path().join("sensitive-name")], &[], &[]).unwrap();
        assert_eq!(std::fs::read(old).unwrap(), b"legacy retained");
        let protected = std::fs::read(directory.join("scope-v2.dpapi")).unwrap();
        assert!(!protected.windows(b"sensitive-name".len()).any(|s| s == b"sensitive-name"));
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
