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
            let result = operation(&current.engine);
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            { drop(read); schedule_legacy_cleanup(config.clone(), protected.to_vec()); }
            return result;
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
        let result = operation(&current.engine);
        drop(write); schedule_legacy_cleanup(config.clone(), protected.to_vec());
        return result;
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
            .map_err(|e| {
                let backups_may_remain = config.index_dir.parent().map(retained_backup_count)
                    .map(|result| result.map(|count| count > 0).unwrap_or(true)).unwrap_or(true);
                if legacy_data_may_remain(&config.index_dir) || backups_may_remain {
                    format!("{e} Old search data may still be present in WinCommander storage.")
                } else { e.to_string() }
            })?
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
        schedule_legacy_cleanup(config, protected.to_vec());
    }
    result
}

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
static AUTO_CLEANUP_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
static AUTO_CLEANUP_MESSAGE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
static AUTO_CLEANUP_LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(super) fn automatic_cleanup_message() -> Option<String> {
    AUTO_CLEANUP_MESSAGE.lock().ok().and_then(|message| message.clone())
}

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
fn legacy_data_may_remain(directory: &std::path::Path) -> bool {
    match std::fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
        Ok(_) => !SearchEngine::experimental_scope_only(directory).unwrap_or(false),
    }
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
fn retained_backup_count(parent: &std::path::Path) -> Result<usize, String> {
    let mut count = 0usize;
    for entry in std::fs::read_dir(parent).map_err(|_| "Cannot inspect old search backups.")? {
        let name = entry.map_err(|_| "Cannot inspect old search backups.")?.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".wincommander-legacy-index-backup-") || name.starts_with(".wincommander-orphaned-index-") { count += 1; }
    }
    Ok(count)
}
#[cfg(all(test, windows, feature = "experimental-encrypted-search"))]
mod cleanup_tests {
    use super::*;
    #[test]
    fn automatic_cleanup_detects_legacy_but_not_protected_scope_only() {
        let root = tempfile::tempdir().unwrap(); let old = root.path().join("fts");
        assert!(!legacy_data_may_remain(&old));
        std::fs::create_dir(&old).unwrap();
        let scope = br#"{"version":1,"roots":[],"exclusions":[],"blocked_roots":[]}"#;
        wincmd_search::publish_experimental_scope(&old, scope, |_| Ok(())).unwrap();
        std::fs::write(old.join(".tantivy-writer.lock"), b"").unwrap();
        assert!(!legacy_data_may_remain(&old));
        std::fs::write(old.join(".tantivy-writer.lock"), b"not empty").unwrap();
        assert!(legacy_data_may_remain(&old));
        std::fs::write(old.join(".tantivy-writer.lock"), b"").unwrap();
        let protected = std::fs::read(old.join("scope-v2.dpapi")).unwrap();
        std::fs::write(old.join("scope-v2.dpapi"), b"invalid").unwrap();
        assert!(legacy_data_may_remain(&old));
        std::fs::write(old.join("scope-v2.dpapi"), protected).unwrap();
        std::fs::create_dir(root.path().join(".wincommander-legacy-index-backup-123")).unwrap();
        assert_eq!(retained_backup_count(root.path()).unwrap(), 1);
        assert!(retained_backup_count(&root.path().join("missing")).is_err());
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
    if AUTO_CLEANUP_STARTED.load(Ordering::Acquire) { return; }
    let Ok(mut last) = AUTO_CLEANUP_LAST.lock() else { return; };
    if last.is_some_and(|when| when.elapsed() < std::time::Duration::from_secs(60)) { return; }
    *last = Some(std::time::Instant::now()); drop(last);
    let requires_cleanup = legacy_data_may_remain(&config.index_dir);
    if !requires_cleanup {
        let message = (|| -> Result<Option<String>, String> {
            let parent = config.index_dir.parent().ok_or("Search location unavailable.")?;
            let count = retained_backup_count(parent)?;
            Ok((count > 0).then(|| format!("Old search backups remain in WinCommander storage ({count} folders).")))
        })().unwrap_or_else(|error| Some(error));
        let has_notice = message.is_some();
        if let Ok(mut slot) = AUTO_CLEANUP_MESSAGE.lock() { *slot = message; }
        if has_notice { super::notify_search_protection_pending(); }
        return;
    }
    if AUTO_CLEANUP_STARTED.swap(true, Ordering::AcqRel) { return; }
    let spawned = std::thread::Builder::new().name("search-legacy-cleanup".into()).spawn(move || {
        let outcome = (|| {
            // Prevent new owned Pro workers from starting and stop idle ones;
            // long-lived monitor agents are preserved and cause a visible retry.
            let _pro = tauri::async_runtime::block_on(crate::sidecar::quiesce_idle_pro_for_search())?;
            let service = if SearchEngine::check_legacy_cleanup_readiness().is_ok() {
                None
            } else {
                // First rollout only: older deployed services do not implement
                // this epoch. This does not prove mapped image bytes against
                // deliberate replacement or copied old binaries.
                Some(tauri::async_runtime::block_on(crate::svc_client::experimental_search_service_capability())?)
            };
            let _replacement = super::locks::IndexReplacementLock::acquire()?;
            cleanup_legacy_with_service(&config, &protected, service.as_ref())
        })();
        let message = match outcome {
            Ok((_, 0)) => None,
            Ok((_, count)) => Some(format!("Old search backups remain in WinCommander storage ({count} folders).")),
            Err(error) => {
                crate::log_message("warn", &format!("[FileSearch] old search data cleanup failed: {error}"));
                Some("Old search data could not be cleaned and may still be on this PC. Try again after restarting WinCommander.".into())
            },
        };
        let has_notice = message.is_some();
        if let Ok(mut slot) = AUTO_CLEANUP_MESSAGE.lock() { *slot = message; }
        AUTO_CLEANUP_STARTED.store(false, Ordering::Release);
        if has_notice { super::notify_search_protection_pending(); }
    });
    if spawned.is_err() {
        AUTO_CLEANUP_STARTED.store(false, Ordering::Release);
        if let Ok(mut slot) = AUTO_CLEANUP_MESSAGE.lock() { *slot = Some("Old search data cleanup could not start.".into()); }
        super::notify_search_protection_pending();
    }
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(super) fn cleanup_legacy(config: &IndexConfig, protected: &[PathBuf]) -> Result<(usize, usize), String> {
    cleanup_legacy_with_service(config, protected, None)
}
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
fn cleanup_legacy_with_service(config: &IndexConfig, protected: &[PathBuf], service: Option<&wincmd_service_auth::search_service_pin::PinnedSearchService>) -> Result<(usize, usize), String> {
    let expected = serde_json::to_string(&(config, protected)).map_err(|e| e.to_string())?;
    let mut write = cell().write().map_err(|_| "Search state is unavailable.")?;
    let current = write.as_mut().ok_or("Open search and let it finish indexing before cleanup.")?;
    if current.signature != expected { return Err("Search settings changed. Reopen search before cleanup.".into()); }
    let parent = config.index_dir.parent().ok_or("Search location unavailable.")?;
    // Quiesce the watcher while the shared process lease is still held.
    current.engine.stop();
    let result = super::runtime_fence::with_exclusive(|fence| {
        let backup_count = retained_backup_count(parent)?;
        let removed = match service {
            Some(service) => {
                let pid = service.running_pid().map_err(|_| "The verified WinCommander service changed during cleanup.")?;
                current.engine.cleanup_legacy_cache_with_fenced_service(fence, pid)
            }
            None => current.engine.cleanup_legacy_cache_after_reconciliation(fence),
        }.map_err(|e| e.to_string())?;
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
