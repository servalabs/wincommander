// SPDX-License-Identifier: AGPL-3.0-or-later
//! `wincmd-search` — full-text file-content search for WinCommander (Free tier).
//!
//! Unit A foundation: types, error, and backend extension traits.
//! Unit B: crawler + watch.
//! Unit C: text extractors (plain, office, pdf) and chunker.
//! Unit D: tantivy index, hit-merge helper, and SearchEngine facade.

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
mod encrypted_search_maintenance;
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
mod encrypted_legacy_cleanup;
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
#[allow(dead_code)]
mod encrypted_migration_transaction;
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub mod encrypted_search_fence;
pub mod backend;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_index_prototype;
#[cfg(test)]
mod encrypted_cutover_prototype;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_key_prototype;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_scope;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_legacy_quarantine;
#[cfg(test)]
mod encrypted_migration_prototype;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_generation_prototype;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_strict_crawl;
pub mod chunk;
pub mod crawler;
#[cfg(windows)]
mod direct_directory;
pub mod error;
pub mod extract;
pub mod filters;
pub mod index;
mod indexing;
pub mod query;
mod read_only;
pub mod reconcile;
mod restricted;
pub mod semantic_model;
pub mod tokenize;
pub mod types;
pub mod watch;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use tantivy::schema::Value;
use tantivy::IndexWriter;

use backend::{ForensicBackend, SemanticBackend};
use chunk::chunk_doc;
use crawler::{build_globset, collect_files};
use error::{Result, SearchError};
use index::ContentIndex;
use indexing::index_file;
use query::merge_hits;
use types::{ContentHit, ContentQuery, FileMeta, IndexConfig, IndexStatus};
use watch::{watch_roots, FsEventAction};

/// Explicit manual recovery of a quarantined legacy cache. Caller must hold
/// the replacement lock and stop old binaries. It never overwrites a new
/// legacy directory or falls back automatically to plaintext.
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub fn restore_experimental_legacy_backup(old: &std::path::Path, index_id: &[u8; 16]) -> Result<std::path::PathBuf> {
    Ok(crate::encrypted_legacy_quarantine::restore(old, index_id)?)
}
/// Application policy, checked immediately before extraction and persistence.
pub type PathPolicy = Arc<dyn Fn(&std::path::Path) -> bool + Send + Sync>;

/// Relocate stored paths only after the caller verifies the index's volume identity.
pub type PathMapper = Arc<dyn Fn(&std::path::Path) -> Option<std::path::PathBuf> + Send + Sync>;

/// Outcome of an experimental eventually consistent cache rebuild.
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExperimentalRebuildReport { pub indexed: usize, pub omitted: usize, pub skipped_links: usize }
/// Experimental scope publication while holding each existing ancestor. The
/// caller must authorize the user root. The legacy plaintext scope is retained.
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub fn publish_experimental_scope(
    directory: &std::path::Path,
    bytes: &[u8],
    validate: impl FnOnce(&std::path::Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use crate::read_only::{AccessMode, CheckedDirectory};
    use crate::encrypted_key_prototype::{Protector, UserDpapi};
    use tantivy::directory::error::OpenReadError;
    let plaintext = crate::encrypted_scope::envelope(bytes)?;
    let parent = directory.parent().ok_or_else(|| std::io::Error::other("invalid scope path"))?;
    let user_root = parent.parent().ok_or_else(|| std::io::Error::other("invalid scope parent"))?;
    let _user_pins = CheckedDirectory::pin_root_path(user_root)?;
    match std::fs::create_dir(parent) {
        Ok(()) => {}, Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(error) => return Err(error),
    }
    let _parent_pins = CheckedDirectory::pin_root_path(parent)?;
    match std::fs::create_dir(directory) {
        Ok(()) => {}, Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(error) => return Err(error),
    }
    let _root_pins = CheckedDirectory::pin_root_path(directory)?;
    validate(directory)?;
    let checked = CheckedDirectory::open(directory, AccessMode::Writable)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let path = std::path::Path::new("scope-v2.dpapi");
    match checked.atomic_read_bounded(path, crate::encrypted_scope::MAX_PROTECTED_SCOPE) {
        Ok(previous) => {
            let clear = UserDpapi.unprotect(&previous)?;
            if crate::encrypted_scope::same_scope(&clear, bytes)? { return Ok(()); }
        }
        Err(OpenReadError::FileDoesNotExist(_)) => {},
        Err(error) => return Err(std::io::Error::other(error.to_string())),
    }
    let protected = UserDpapi.protect(&plaintext)?;
    if protected.len() > crate::encrypted_scope::MAX_PROTECTED_SCOPE {
        return Err(std::io::Error::other("protected scope exceeds limit"));
    }
    checked.atomic_write_pinned(path, &protected)
}
/// Facade: owns the index and orchestrates crawl → extract → chunk → index.
pub struct SearchEngine {
    path_policy: PathPolicy,
    watcher_invalid: Arc<AtomicBool>,
    watcher_generation: Arc<AtomicU64>,
    path_mapper: Option<PathMapper>,
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    experimental_fresh: bool,
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    experimental_legacy_lease: Option<tantivy::directory::DirectoryLock>,
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    experimental_legacy_pins: Option<Vec<std::fs::File>>,
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    experimental_id: Option<[u8; 16]>,
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    experimental_legacy_path: Option<std::path::PathBuf>,
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    experimental_verified: AtomicBool,
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    experimental_selection: Option<crate::encrypted_generation_prototype::PinnedSelectionStore>,
    config: IndexConfig,
    ci: Arc<ContentIndex>,
    status: Arc<Mutex<IndexStatus>>,
    semantic: Option<Arc<dyn SemanticBackend>>,
    forensic: Option<Arc<dyn ForensicBackend>>,
    // Shutdown primitives — interior-mutable so methods can take &self while
    // the engine lives behind an Arc.
    stop: Arc<AtomicBool>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

const WRITER_WAIT_MESSAGE: &str = "The search index is in use by another WinCommander window. Indexing will resume automatically.";

fn wait_for_index_writer(
    index: &ContentIndex,
    status: &Mutex<IndexStatus>,
    stop: &AtomicBool,
) -> Option<IndexWriter> {
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match index.writer() {
            Ok(writer) => {
                let mut state = status.lock().unwrap();
                if state.last_error.as_deref() == Some(WRITER_WAIT_MESSAGE) {
                    state.last_error = None;
                }
                return Some(writer);
            }
            Err(SearchError::Index(tantivy::TantivyError::LockFailure(
                tantivy::directory::error::LockError::LockBusy, _,
            ))) => {
                status.lock().unwrap().last_error = Some(WRITER_WAIT_MESSAGE.into());
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(error) => {
                status.lock().unwrap().last_error = Some(error.to_string());
                return None;
            }
        }
    }
}

impl SearchEngine {
    /// Open (or create) the index described by `config`.
    pub fn open(config: IndexConfig) -> Result<Self> {
        Self::open_with_policy(config, Arc::new(|_| true))
    }

    pub fn open_with_policy(config: IndexConfig, path_policy: PathPolicy) -> Result<Self> {
        let ci = Arc::new(ContentIndex::open_or_create(&config.index_dir)?);
        Self::from_index(config, ci, path_policy)
    }

    /// Opens a separate encrypted generation and rebuilds it through the normal
    /// search engine. This is always an unselected candidate, including on
    /// reopen; it does not certify that crawling or migration completed.
    /// Interrupted candidates are retained; use a fresh identity to rebuild.
    /// This does not select it for the app or delete old data.
    /// The caller must authorize the parent and hold the cross-process lease
    /// against every legacy and new writer, and protect the identity durably.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn open_experimental_encrypted_with_policy(
        mut config: IndexConfig,
        path_policy: PathPolicy,
        index_id: [u8; 16],
        existing: bool,
    ) -> Result<Self> {
        use std::path::Component;
        let old = config.index_dir.clone();
        if !old.is_absolute() || old.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir))
            || index_id == [0; 16] {
            return Err(SearchError::Config("invalid encrypted index generation".into()));
        }
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid old index path".into()))?;
        let generation = parent.join(format!(".wincommander-encrypted-{}", hex::encode(index_id)));
        if generation.to_string_lossy().eq_ignore_ascii_case(&old.to_string_lossy()) {
            return Err(SearchError::Config("encrypted generation must differ from old index".into()));
        }
        // Pin parent before any creation. Only an unselected rebuild needs
        // the legacy writer lock; selected readers use the encrypted index.
        let _parent_pins = crate::read_only::CheckedDirectory::pin_root_path(parent)?;
        let (legacy_pins, legacy_lease) = if existing {
            (None, None)
        } else {
            match std::fs::create_dir(&old) {
                Ok(()) => {},
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
                Err(e) => return Err(e.into()),
            }
            // Legacy direct writes/schema wipes are not fenced by this lock.
            let pins = crate::read_only::CheckedDirectory::pin_root_path(&old)?;
            let legacy = crate::read_only::CheckedDirectory::open(&old, crate::read_only::AccessMode::Writable)?;
            let lease = legacy.acquire_lock_pinned(&tantivy::directory::INDEX_WRITER_LOCK)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            (Some(pins), Some(lease))
        };
        // Retained plaintext recovery backups must never be crawled, including
        // after restart when this engine no longer has their in-memory paths.
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with(".wincommander-legacy-index-backup-") {
                config.skip_paths.push(entry.path());
            }
        }
        // Generation creation is exclusive; interrupted candidates remain.
        if !existing {
            std::fs::create_dir(&generation)?;
        }
        // Avoid ingesting either index if a user's search root contains them.
        config.skip_paths.push(old.clone());
        config.skip_paths.push(generation.clone());
        config.index_dir = generation;
        let ci = Arc::new(ContentIndex::open_experimental_encrypted(&config.index_dir, index_id, existing)?);
        let mut engine = Self::from_index(config, ci, path_policy)?;
        engine.experimental_legacy_lease = legacy_lease;
        engine.experimental_legacy_pins = legacy_pins;
        engine.experimental_fresh = !existing;
        engine.experimental_id = Some(index_id);
        engine.experimental_legacy_path = Some(old);
        Ok(engine)
    }
    /// Build only a newly created, unselected candidate. Traversal and
    /// traversal errors abort; unsupported or temporarily unreadable files
    /// are omitted and reported. This is an eventually consistent cache,
    /// not a stable snapshot of source files.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn rebuild_experimental_candidate(&mut self) -> Result<ExperimentalRebuildReport> {
        if !self.experimental_fresh {
            return Err(SearchError::Config("encrypted candidate is not fresh".into()));
        }
        // A failed or interrupted pass is never retried in-place.
        self.experimental_fresh = false;
        let exclusions = build_globset(&self.config.exclusions)?;
        let (files, skipped_links) = crate::encrypted_strict_crawl::collect(
            &self.config.roots, &exclusions, &self.config.skip_paths, self.config.max_file_bytes,
        )?;
        let mut seen = std::collections::HashSet::new();
        let mut writer = self.ci.writer()?;
        let mut indexed = 0usize;
        let mut omitted = 0usize;
        for meta in files {
            if !(self.path_policy)(&meta.path) { continue; }
            if !seen.insert(meta.doc_id) {
                return Err(SearchError::Config("duplicate source document identity".into()));
            }
            let doc = match crate::extract::extract_text(meta) {
                Ok(doc) => doc,
                Err(SearchError::Unsupported(_)) => continue,
                Err(error) if error.is_per_file_skip() || matches!(&error, SearchError::Io(_)) => { omitted += 1; continue; }
                Err(error) => return Err(error),
            };
            if !(self.path_policy)(&doc.meta.path) {
                return Err(SearchError::Config("source policy changed during rebuild".into()));
            }
            self.ci.upsert(&mut writer, &doc.meta, &doc.title, &doc.body, &doc.props)?;
            indexed += 1;
        }
        self.ci.commit(&mut writer)?;
        let reader = self.ci.index.reader()?;
        if reader.searcher().num_docs() != indexed as u64 {
            return Err(SearchError::Config("encrypted candidate document count mismatch".into()));
        }
        self.experimental_verified.store(true, Ordering::Release);
        Ok(ExperimentalRebuildReport { indexed, omitted, skipped_links })
    }
    /// Atomically attempts to select a completed encrypted cache. Missing or
    /// invalid selection on restart must be an error, never plaintext fallback.
    /// Caller authorizes the selection root and stops legacy direct mutators.
    /// Directory-entry durability and anti-downgrade remain unverified.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn select_experimental_candidate(&mut self, selection_dir: &std::path::Path) -> Result<()> {
        self.select_experimental_candidate_with_origin(selection_dir, false)
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    fn select_experimental_candidate_with_origin(&mut self, selection_dir: &std::path::Path, fresh: bool) -> Result<()> {
        use sha2::{Digest, Sha256};
        use tantivy::directory::Directory;
        use crate::encrypted_generation_prototype::{PinnedSelectionStore, UserProtector};
        let id = self.experimental_id.ok_or_else(|| SearchError::Config("not an encrypted candidate".into()))?;
        if self.experimental_legacy_lease.is_none() || !self.experimental_verified.load(Ordering::Acquire) {
            return Err(SearchError::Config("encrypted candidate rebuild is incomplete".into()));
        }
        let directory = crate::encrypted_index_prototype::tantivy_trial::TrialDirectory::open_protected(
            &self.config.index_dir, crate::read_only::AccessMode::GuardedReader, &id, true,
        )?;
        let manifest = directory.atomic_read(std::path::Path::new("meta.json"))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let digest: [u8; 32] = Sha256::digest(&manifest).into();
        let source = self.experimental_legacy_path.as_ref().ok_or_else(|| SearchError::Config("legacy search source missing".into()))?;
        crate::encrypted_generation_prototype::record_origin(selection_dir, &id, fresh, &digest, source)?;
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        crate::encrypted_generation_prototype::publish(&mut selection, &UserProtector, id, || Ok(digest))?;
        self.config.skip_paths.push(selection_dir.to_path_buf());
        self.experimental_selection = Some(selection);
        // The marker is committed; other selected processes no longer need
        // the exclusive legacy writer lease. Old binaries must be stopped.
        self.experimental_legacy_lease = None;
        self.experimental_legacy_pins = None;
        Ok(())
    }

    /// Reopen only the DPAPI-selected encrypted generation. A missing,
    /// corrupt or inaccessible record/key/generation is returned as an error.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn open_experimental_selected_with_policy(config: IndexConfig, path_policy: PathPolicy, selection_dir: &std::path::Path) -> Result<Self> {
        use crate::encrypted_generation_prototype::{inspect, PinnedSelectionStore, UserProtector};
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        let selected = inspect(&mut selection, &UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        let mut engine = Self::open_experimental_encrypted_with_policy(config, path_policy, selected.index_id, true)?;
        engine.config.skip_paths.push(selection_dir.to_path_buf());
        engine.experimental_selection = Some(selection);
        // Register the watcher before reconciling offline changes so events
        // occurring during the scan queue until the worker starts.
        engine.start_indexing_internal(true)?;
        Ok(engine)
    }
    /// Experimental application entry point. A missing selector starts a new
    /// encrypted cache; a present but invalid selector is always an error.
    /// Caller must serialize initialization and authorize the per-user parent.
    /// The old index and interrupted candidates are retained.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn open_or_rebuild_experimental_with_policy(mut config: IndexConfig, path_policy: PathPolicy) -> Result<Self> {
        use rand::{rngs::OsRng, RngCore};
        use crate::encrypted_generation_prototype::{inspect, PinnedSelectionStore, UserProtector};
        let parent = config.index_dir.parent()
            .ok_or_else(|| SearchError::Config("invalid index parent".into()))?;
        let _pins = crate::read_only::CheckedDirectory::pin_root_path(parent)?;
        let selection = parent.join(".wincommander-encrypted-selection");
        match std::fs::create_dir(&selection) {
            Ok(()) => {},
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(error) => return Err(error.into()),
        }
        config.skip_paths.push(selection.clone());
        let mut store = PinnedSelectionStore::open(&selection)?;
        let exists = inspect(&mut store, &UserProtector)?.is_some();
        drop(store);
        if exists {
            return Self::open_experimental_selected_with_policy(config, path_policy, &selection);
        }
        // Recover a crash after protected origin publication but before the
        // selection record: reuse only its authenticated generation, after
        // opening and authenticating the existing candidate. Never rebuild it.
        if let Some(origin) = crate::encrypted_generation_prototype::read_origin(&selection)? {
            let id = origin.id; let fresh_origin = origin.fresh;
            let source_identity = crate::encrypted_migration_transaction::Intent::capture([1; 16], id, &config.index_dir)?.identity();
            if source_identity != origin.source { return Err(SearchError::Config("Original search source changed; recovery required.".into())); }
            use sha2::{Digest, Sha256};
            use tantivy::directory::Directory;
            use crate::read_only::{AccessMode, CheckedDirectory};
            if fresh_origin {
                for entry in std::fs::read_dir(parent)? {
                    let name = entry?.file_name(); let name = name.to_string_lossy();
                    if name.starts_with(".wincommander-legacy-index-backup-")
                        || name.strip_prefix(".wincommander-encrypted-").is_some_and(|value|
                            value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()) && value != hex::encode(id)) {
                        return Err(SearchError::Config("Prior search data needs recovery.".into()));
                    }
                }
                match std::fs::symlink_metadata(&config.index_dir) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                    Err(e) => return Err(e.into()),
                    Ok(_) => {
                        let _old_pins = CheckedDirectory::pin_root_path(&config.index_dir)?;
                        let mut names = Vec::new();
                        for entry in std::fs::read_dir(&config.index_dir)? { names.push(entry?.file_name()); }
                        let only_empty_lock = if names.len() == 1 && names[0] == std::ffi::OsStr::new(".tantivy-writer.lock") {
                            use std::os::windows::io::AsRawHandle;
                            use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
                            let checked = CheckedDirectory::open(&config.index_dir, AccessMode::GuardedReader)?;
                            let file = checked.open_read_pinned(std::path::Path::new(".tantivy-writer.lock"))
                                .map_err(|e| std::io::Error::other(e.to_string()))?;
                            let mut info = BY_HANDLE_FILE_INFORMATION::default();
                            // SAFETY: the file and output remain valid through the call.
                            (unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) }) != 0
                                && info.nNumberOfLinks == 1 && file.metadata()?.len() == 0
                        } else { false };
                        if !names.is_empty() && !only_empty_lock && !Self::experimental_scope_only(&config.index_dir)? {
                            return Err(SearchError::Config("Legacy search data appeared during recovery.".into()));
                        }
                    },
                }
            }
            let generation = parent.join(format!(".wincommander-encrypted-{}", hex::encode(id)));
            let directory = crate::encrypted_index_prototype::tantivy_trial::TrialDirectory::open_protected(&generation, AccessMode::GuardedReader, &id, true)?;
            let manifest = directory.atomic_read(std::path::Path::new("meta.json")).map_err(|e| std::io::Error::other(e.to_string()))?;
            tantivy::Index::open(directory).map_err(|_| SearchError::Config("Interrupted encrypted search candidate is invalid.".into()))?;
            let digest: [u8; 32] = Sha256::digest(&manifest).into();
            if digest != origin.digest { return Err(SearchError::Config("Interrupted search manifest changed; recovery required.".into())); }
            let mut recovered = PinnedSelectionStore::open(&selection)?;
            crate::encrypted_generation_prototype::publish(&mut recovered, &UserProtector, id, || Ok(digest))?;
            drop(recovered);
            return Self::open_experimental_selected_with_policy(config, path_policy, &selection);
        }
        // Only a missing/empty directory or the authenticated scope-only
        // directory can establish that there was no legacy index.
        let fresh = match std::fs::symlink_metadata(&config.index_dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => return Err(e.into()),
            Ok(_) => {
                let _old_pins = crate::read_only::CheckedDirectory::pin_root_path(&config.index_dir)?;
                if std::fs::read_dir(&config.index_dir)?.next().transpose()?.is_none() { true }
                else { Self::experimental_scope_only(&config.index_dir)? }
            },
        };
        if fresh {
            for entry in std::fs::read_dir(parent)? {
                let name = entry?.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".wincommander-legacy-index-backup-")
                    || name.strip_prefix(".wincommander-encrypted-").is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())) {
                    return Err(SearchError::Config("Prior encrypted search data needs recovery; no fresh origin was created.".into()));
                }
            }
        }
        let mut identity = [0u8; 16];
        OsRng.fill_bytes(&mut identity);
        if identity == [0; 16] { return Err(SearchError::Config("cannot create encrypted index identity".into())); }
        let mut engine = Self::open_experimental_encrypted_with_policy(config, path_policy, identity, false)?;
        let report = engine.rebuild_experimental_candidate()?;
        if report.omitted > 0 || report.skipped_links > 0 {
            engine.status.lock().unwrap().last_error = Some(format!(
                "Encrypted search rebuild skipped {} unreadable files and {} links; some results may be missing",
                report.omitted, report.skipped_links,
            ));
        }
        engine.select_experimental_candidate_with_origin(&selection, fresh)?;
        engine.start_indexing_internal(true)?;
        Ok(engine)
    }
    /// Move the old plaintext cache to a reversible sibling backup. This does
    /// not erase data or free space. Call only after stopping all old binaries;
    /// an old process can otherwise bypass the cooperating writer lock.
    /// Search is stopped and must be reopened afterward. No normal application
    /// path invokes this experimental operation.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn quarantine_experimental_legacy_cache(&mut self) -> Result<std::path::PathBuf> {
        self.quarantine_experimental_inner(None)
    }
    /// Offline helper variant: reuse its exclusive fence without reacquiring it.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn quarantine_experimental_with_fence(&mut self, fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<std::path::PathBuf> {
        self.quarantine_experimental_inner(Some(fence))
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    fn quarantine_experimental_inner(&mut self, fence: Option<&crate::encrypted_search_fence::SearchOperationFence>) -> Result<std::path::PathBuf> {
        if self.experimental_selection.is_none() || self.experimental_id.is_none() {
            return Err(SearchError::Config("encrypted generation is not selected".into()));
        }
        // Stop worker and watcher before changing exclusion paths. Reopening
        // reconstructs exclusions from retained backup names.
        self.stop();
        // Reopen metadata through the active encrypted index before any move.
        self.ci.index.reader()?;
        let old = self.experimental_legacy_path.as_ref()
            .ok_or_else(|| SearchError::Config("legacy index path unavailable".into()))?;
        let id = self.experimental_id.ok_or_else(|| SearchError::Config("index identity unavailable".into()))?;
        let backup = match fence {
            Some(guard) => crate::encrypted_legacy_quarantine::quarantine_with_fence(old, &id, guard)?,
            None => crate::encrypted_legacy_quarantine::quarantine(old, &id)?,
        };
        // Prevent a later search root from ingesting the retained backup.
        self.config.skip_paths.push(backup.clone());
        Ok(backup)
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    fn check_offline_rebuild_space(old: &std::path::Path, parent: &std::path::Path) -> Result<()> {
        use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, GetDiskFreeSpaceExW};
        let _pins = crate::read_only::CheckedDirectory::pin_root_path(old)?;
        let mut old_bytes = 0u64;
        for entry in std::fs::read_dir(old)? {
            let entry = entry?;
            let meta = std::fs::symlink_metadata(entry.path())?;
            if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(SearchError::Config("Old search index contains unsupported entries; existing files were retained.".into()));
            }
            old_bytes = old_bytes.checked_add(meta.len())
                .ok_or_else(|| SearchError::Config("Cannot estimate search rebuild size.".into()))?;
        }
        let required = old_bytes.checked_mul(2).and_then(|n| n.checked_add(512 * 1024 * 1024))
            .ok_or_else(|| SearchError::Config("Cannot estimate search rebuild size.".into()))?;
        let wide: Vec<u16> = parent.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut available = 0u64;
        // SAFETY: the UTF-16 path is terminated and output pointers are valid.
        if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if available < required {
            return Err(SearchError::Config("Not enough free space to rebuild the search cache; existing files were retained.".into()));
        }
        Ok(())
    }
    /// Rebuild a disposable encrypted cache under an offline exclusive fence.
    /// Failed candidates remain in place and the original cache is untouched.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn rebuild_offline_and_select(config: IndexConfig, policy: PathPolicy,
        selection_dir: &std::path::Path, fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<ExperimentalRebuildReport> {
        use rand::{rngs::OsRng, RngCore};
        use crate::read_only::CheckedDirectory;
        let old = &config.index_dir;
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid legacy index path".into()))?;
        if !old.is_absolute() || selection_dir.parent() != Some(parent) {
            return Err(SearchError::Config("invalid search selection parent".into()));
        }
        fence.require_exclusive_for(parent)?;
        let _pins = CheckedDirectory::pin_root_path(parent)?;
        crate::encrypted_search_maintenance::verify_known_writers_stopped().map_err(SearchError::Config)?;
        // Do not accumulate a new full cache after an interrupted attempt.
        // Existing candidates are retained for explicit recovery/cleanup.
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name.strip_prefix(".wincommander-encrypted-") {
                if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(SearchError::Config("A previous encrypted search rebuild needs recovery before retry; existing files were retained.".into()));
                }
            }
        }
        Self::check_offline_rebuild_space(old, parent)?;
        match std::fs::create_dir(selection_dir) {
            Ok(()) => {}, Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}, Err(e) => return Err(e.into()),
        }
        let _selection_pins = CheckedDirectory::pin_root_path(selection_dir)?;
        if std::fs::read_dir(selection_dir)?.next().transpose()?.is_some() {
            return Err(SearchError::Config("search selection already exists; recovery required".into()));
        }
        let mut id = [0u8; 16]; OsRng.fill_bytes(&mut id);
        if id == [0; 16] { return Err(SearchError::Config("cannot create encrypted index identity".into())); }
        let mut engine = Self::open_experimental_encrypted_with_policy(config, policy, id, false)?;
        let report = engine.rebuild_experimental_candidate()?;
        engine.select_experimental_candidate(selection_dir)?;
        Ok(report)
    }
    /// Authenticate and open the selected encrypted index without moving or
    /// changing the existing legacy cache.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn verify_selected_generation(old: &std::path::Path, selection_dir: &std::path::Path) -> Result<()> {
        use crate::encrypted_generation_prototype::{inspect, PinnedSelectionStore, UserProtector};
        use crate::encrypted_index_prototype::tantivy_trial::TrialDirectory;
        use crate::read_only::{AccessMode, CheckedDirectory};
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid legacy index path".into()))?;
        if !old.is_absolute() || selection_dir.parent() != Some(parent) {
            return Err(SearchError::Config("invalid selection parent".into()));
        }
        let _pins = CheckedDirectory::pin_root_path(parent)?;
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        let selected = inspect(&mut selection, &UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        let generation = parent.join(format!(".wincommander-encrypted-{}", hex::encode(selected.index_id)));
        let directory = TrialDirectory::open_protected(&generation, AccessMode::GuardedReader, &selected.index_id, true)?;
        tantivy::Index::open(directory).map_err(|_| SearchError::Config("encrypted index cannot be opened".into()))?;
        Ok(())
    }
    /// Verify a scope-only directory without a backup only when the selected
    /// generation has authenticated proof that no legacy index existed.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn verify_fresh_scope_selection(old: &std::path::Path, selection_dir: &std::path::Path) -> Result<()> {
        use crate::encrypted_generation_prototype::{inspect, inspect_origin, PinnedSelectionStore, UserProtector};
        if !Self::experimental_scope_only(old)? { return Err(SearchError::Config("legacy index remains".into())); }
        Self::verify_selected_generation(old, selection_dir)?;
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        let selected = inspect(&mut selection, &UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        if inspect_origin(selection_dir, &selected.index_id)? != Some(true) {
            return Err(SearchError::Config("fresh search provenance unavailable".into()));
        }
        Ok(())
    }
    /// Offline phase: authenticate the selected encrypted generation before
    /// moving the legacy cache under the helper's own exclusive fence.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn quarantine_selected_with_fence(old: &std::path::Path, selection_dir: &std::path::Path,
        fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<std::path::PathBuf> {
        Self::quarantine_selected_inner(old, selection_dir, fence, None)
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    fn quarantine_selected_inner(old: &std::path::Path, selection_dir: &std::path::Path,
        fence: &crate::encrypted_search_fence::SearchOperationFence, expected: Option<(u32, u32, u32)>) -> Result<std::path::PathBuf> {
        use tantivy::directory::Directory;
        use crate::encrypted_generation_prototype::{inspect, PinnedSelectionStore, UserProtector};
        use crate::encrypted_index_prototype::tantivy_trial::TrialDirectory;
        use crate::read_only::{AccessMode, CheckedDirectory};
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid legacy index path".into()))?;
        if !old.is_absolute() || selection_dir.parent() != Some(parent) {
            return Err(SearchError::Config("invalid selection parent".into()));
        }
        fence.require_exclusive_for(parent)?;
        let _pins = CheckedDirectory::pin_root_path(parent)?;
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        let selected = inspect(&mut selection, &UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        let generation = parent.join(format!(".wincommander-encrypted-{}", hex::encode(selected.index_id)));
        let directory = TrialDirectory::open_protected(&generation, AccessMode::GuardedReader, &selected.index_id, true)?;
        let manifest = directory.atomic_read(std::path::Path::new("meta.json"))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let _: serde_json::Value = serde_json::from_slice(&manifest)
            .map_err(|_| SearchError::Config("encrypted index manifest invalid".into()))?;
        tantivy::Index::open(directory).map_err(|_| SearchError::Config("encrypted index cannot be opened".into()))?;
        Ok(match expected {
            Some(identity) => crate::encrypted_legacy_quarantine::quarantine_expected(old, &selected.index_id, fence, identity)?,
            None => crate::encrypted_legacy_quarantine::quarantine_with_fence(old, &selected.index_id, fence)?,
        })
    }
    /// Accept only the authenticated, protected scope file recreated by a
    /// selected app; any other legacy-directory entry remains a conflict.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn experimental_scope_only(old: &std::path::Path) -> Result<bool> {
        use crate::encrypted_key_prototype::{Protector, UserDpapi};
        use crate::read_only::{AccessMode, CheckedDirectory};
        let _pins = CheckedDirectory::pin_root_path(old)?;
        let mut scope_present = false;
        let mut lock_present = false;
        for entry in std::fs::read_dir(old)? {
            let name = entry?.file_name();
            if name == std::ffi::OsStr::new("scope-v2.dpapi") && !scope_present { scope_present = true; }
            else if name == std::ffi::OsStr::new(".tantivy-writer.lock") && !lock_present { lock_present = true; }
            else { return Ok(false); }
        }
        if !scope_present { return Ok(false); }
        let dir = CheckedDirectory::open(old, AccessMode::GuardedReader)?;
        if lock_present {
            let lock = dir.atomic_read_bounded(std::path::Path::new(".tantivy-writer.lock"), 1)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if !lock.is_empty() { return Ok(false); }
        }
        let protected = dir.atomic_read_bounded(std::path::Path::new("scope-v2.dpapi"), crate::encrypted_scope::MAX_PROTECTED_SCOPE)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let clear = UserDpapi.unprotect(&protected)?;
        if clear.len() < 8 { return Ok(false); }
        Ok(crate::encrypted_scope::same_scope(&clear, &clear[8..]).unwrap_or(false))
    }
    /// Read-only post-handoff check. The installer must hold its exclusive
    /// operation fence for the entire call; this helper never owns that lock.
    /// It verifies the DPAPI selector and authenticates the encrypted manifest,
    /// but does not claim every segment or every source file has been checked.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn verify_quarantined_selection(old: &std::path::Path, selection_dir: &std::path::Path) -> Result<()> {
        use tantivy::directory::Directory;
        use crate::encrypted_generation_prototype::{inspect, PinnedSelectionStore, UserProtector};
        use crate::encrypted_index_prototype::tantivy_trial::TrialDirectory;
        use crate::read_only::{AccessMode, CheckedDirectory};
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid legacy index path".into()))?;
        if !old.is_absolute() || selection_dir.parent() != Some(parent) {
            return Err(SearchError::Config("invalid selection parent".into()));
        }
        let _pins = CheckedDirectory::pin_root_path(parent)?;
        let absent = || -> Result<()> {
            match std::fs::symlink_metadata(old) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
                Ok(_) if Self::experimental_scope_only(old)? => Ok(()),
                Ok(_) => Err(SearchError::Config("legacy index still present".into())),
            }
        };
        absent()?;
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        let selected = inspect(&mut selection, &UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        let backup = parent.join(crate::encrypted_legacy_quarantine::backup_name(&selected.index_id)?);
        let _backup_pins = CheckedDirectory::pin_root_path(&backup)?;
        let generation = parent.join(format!(".wincommander-encrypted-{}", hex::encode(selected.index_id)));
        let directory = TrialDirectory::open_protected(&generation, AccessMode::GuardedReader, &selected.index_id, true)?;
        let manifest = directory.atomic_read(std::path::Path::new("meta.json"))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let _: serde_json::Value = serde_json::from_slice(&manifest)
            .map_err(|_| SearchError::Config("encrypted index manifest invalid".into()))?;
        tantivy::Index::open(directory).map_err(|_| SearchError::Config("encrypted index cannot be opened".into()))?;
        absent()?;
        Ok(())
    }
    /// Restore only the authenticated generation's backup, never overwriting
    /// an existing legacy path. The helper must own the exclusive fence.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn restore_quarantined_with_fence(old: &std::path::Path, selection_dir: &std::path::Path,
        fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<std::path::PathBuf> {
        use crate::encrypted_generation_prototype::{inspect, PinnedSelectionStore, UserProtector};
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid legacy index path".into()))?;
        fence.require_exclusive_for(parent)?;
        Self::verify_quarantined_selection(old, selection_dir)?;
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        let selected = inspect(&mut selection, &UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        Ok(crate::encrypted_legacy_quarantine::restore_with_fence(old, &selected.index_id, fence)?)
    }
    /// Inspect a pending transaction after installer restart. The parent must
    /// provide its established maintenance fence around subsequent actions.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn inspect_search_migration(selection_dir: &std::path::Path) -> Result<Option<([u8; 16], bool)>> {
        let store = crate::encrypted_migration_transaction::Store::open(selection_dir)?;
        Ok(store.load()?.map(|intent| (intent.transaction, intent.replacement_pending)))
    }

    /// Persist intent before moving the old cache. Same-ID retries are
    /// idempotent only when the original directory identity still matches.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn begin_search_migration(old: &std::path::Path, selection_dir: &std::path::Path, transaction: [u8; 16],
        fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<()> {
        use crate::encrypted_generation_prototype::{inspect, PinnedSelectionStore, UserProtector};
        use crate::encrypted_migration_transaction::{Intent, Store};
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid legacy index path".into()))?;
        if selection_dir.parent() != Some(parent) { return Err(SearchError::Config("invalid selection parent".into())); }
        fence.require_exclusive_for(parent)?;
        let mut selection = PinnedSelectionStore::open(selection_dir)?;
        let selected = inspect(&mut selection, &UserProtector)?.ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        let backup = parent.join(crate::encrypted_legacy_quarantine::backup_name(&selected.index_id)?);
        let store = Store::open(selection_dir)?;
        let existing = store.load()?;
        let intent = match existing {
            Some(value) if value.transaction == transaction && value.generation == selected.index_id && !value.replacement_pending => value,
            Some(_) => return Err(SearchError::Config("another search migration needs recovery".into())),
            None => { let value = Intent::capture(transaction, selected.index_id, old)?; store.prepare(&value)?; value },
        };
        let old_present = old.try_exists()?;
        let backup_present = backup.try_exists()?;
        match (old_present, backup_present) {
            (true, false) if intent.matches_path(old)? => { Self::quarantine_selected_inner(old, selection_dir, fence, Some(intent.identity()))?; },
            (false, true) if intent.matches_path(&backup)? => {},
            _ => return Err(SearchError::Config("search migration paths changed; recovery required".into())),
        }
        Self::verify_quarantined_selection(old, selection_dir)?;
        Ok(())
    }

    /// Set the roll-forward boundary before installer payload replacement.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn mark_search_replacement(old: &std::path::Path, selection_dir: &std::path::Path, transaction: [u8; 16],
        fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<()> {
        fence.require_exclusive_for(old.parent().ok_or_else(|| SearchError::Config("invalid search parent".into()))?)?;
        Self::verify_quarantined_selection(old, selection_dir)?;
        let store = crate::encrypted_migration_transaction::Store::open(selection_dir)?;
        let intent = store.load()?.ok_or_else(|| SearchError::Config("search migration intent missing".into()))?;
        let mut selection = crate::encrypted_generation_prototype::PinnedSelectionStore::open(selection_dir)?;
        let selected = crate::encrypted_generation_prototype::inspect(&mut selection, &crate::encrypted_generation_prototype::UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        if selected.index_id != intent.generation { return Err(SearchError::Config("search generation changed; recovery required".into())); }
        let backup = old.parent().unwrap().join(crate::encrypted_legacy_quarantine::backup_name(&intent.generation)?);
        if intent.transaction != transaction || !intent.matches_path(&backup)? { return Err(SearchError::Config("search migration identity changed".into())); }
        store.mark_replacement(&transaction)?; Ok(())
    }

    /// Complete only a marked and still authenticated migration.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn commit_search_migration(old: &std::path::Path, selection_dir: &std::path::Path, transaction: [u8; 16],
        fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<()> {
        fence.require_exclusive_for(old.parent().ok_or_else(|| SearchError::Config("invalid search parent".into()))?)?;
        Self::verify_quarantined_selection(old, selection_dir)?;
        let store = crate::encrypted_migration_transaction::Store::open(selection_dir)?;
        let intent = store.load()?.ok_or_else(|| SearchError::Config("search migration intent missing".into()))?;
        let mut selection = crate::encrypted_generation_prototype::PinnedSelectionStore::open(selection_dir)?;
        let selected = crate::encrypted_generation_prototype::inspect(&mut selection, &crate::encrypted_generation_prototype::UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        if selected.index_id != intent.generation { return Err(SearchError::Config("search generation changed; recovery required".into())); }
        let backup = old.parent().unwrap().join(crate::encrypted_legacy_quarantine::backup_name(&intent.generation)?);
        if intent.transaction != transaction || !intent.replacement_pending || !intent.matches_path(&backup)? {
            return Err(SearchError::Config("search migration is not ready to commit".into()));
        }
        store.clear_matching(&transaction)?; Ok(())
    }

    /// Abort is allowed only before payload replacement, and only restores the
    /// exact original directory without overwriting any current legacy path.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn abort_search_migration(old: &std::path::Path, selection_dir: &std::path::Path, transaction: [u8; 16],
        fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<()> {
        let parent = old.parent().ok_or_else(|| SearchError::Config("invalid search parent".into()))?;
        fence.require_exclusive_for(parent)?;
        let store = crate::encrypted_migration_transaction::Store::open(selection_dir)?;
        let intent = store.load()?.ok_or_else(|| SearchError::Config("search migration intent missing".into()))?;
        let mut selection = crate::encrypted_generation_prototype::PinnedSelectionStore::open(selection_dir)?;
        let selected = crate::encrypted_generation_prototype::inspect(&mut selection, &crate::encrypted_generation_prototype::UserProtector)?
            .ok_or_else(|| SearchError::Config("encrypted selection missing".into()))?;
        if selected.index_id != intent.generation { return Err(SearchError::Config("search generation changed; recovery required".into())); }
        if intent.transaction != transaction || intent.replacement_pending { return Err(SearchError::Config("search migration must roll forward".into())); }
        let backup = parent.join(crate::encrypted_legacy_quarantine::backup_name(&intent.generation)?);
        match (old.try_exists()?, backup.try_exists()?) {
            (true, false) if intent.matches_path(old)? => {},
            (false, true) if intent.matches_path(&backup)? => {
                crate::encrypted_legacy_quarantine::restore_expected(old, &intent.generation, fence, intent.identity())?;
                if !intent.matches_path(old)? { return Err(SearchError::Config("restored search identity changed".into())); }
            },
            _ => return Err(SearchError::Config("search migration paths changed; recovery required".into())),
        }
        store.clear_matching(&transaction)?; Ok(())
    }
    /// Explicit experimental cleanup for a disposable legacy cache after a
    /// full reconciliation with authoritative user roots. This is not secure
    /// erasure and does not remove backups, snapshots or SSD remanence.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn cleanup_legacy_cache_after_reconciliation(&mut self,
        fence: &crate::encrypted_search_fence::SearchOperationFence) -> Result<usize> {
        if self.experimental_selection.is_none() || self.experimental_id.is_none() || self.config.roots.is_empty() {
            return Err(SearchError::Config("A selected index and authoritative search roots are required.".into()));
        }
        if self.watcher_invalid.load(Ordering::Acquire) {
            return Err(SearchError::Config("Search requires recovery before legacy cleanup.".into()));
        }
        self.stop();
        // The caller must reopen after every outcome; never leave a stopped
        // watcher serving potentially stale results.
        self.watcher_invalid.store(true, Ordering::Release);
        let report = self.reconcile_sync(&std::sync::atomic::AtomicBool::new(false), usize::MAX)?;
        if !report.complete { return Err(SearchError::Config("Search reconciliation is incomplete; old cache retained.".into())); }
        self.ci.index.reader()?;
        let old = self.experimental_legacy_path.as_ref()
            .ok_or_else(|| SearchError::Config("legacy search location unavailable".into()))?;
        let _old_pins = crate::read_only::CheckedDirectory::pin_root_path(old)?;
        let selection = self.experimental_selection.as_ref().ok_or_else(|| SearchError::Config("search selection missing".into()))?;
        let id = self.experimental_id.ok_or_else(|| SearchError::Config("search identity missing".into()))?;
        let origin = crate::encrypted_generation_prototype::read_origin(selection.root())?
            .ok_or_else(|| SearchError::Config("legacy search ownership proof missing".into()))?;
        let actual = crate::encrypted_migration_transaction::Intent::capture([1; 16], id, old)?.identity();
        if origin.id != id || origin.fresh || origin.source != actual {
            return Err(SearchError::Config("legacy search ownership could not be verified".into()));
        }
        Ok(crate::encrypted_legacy_cleanup::remove_managed(old, fence)?)
    }
    /// Locate the generation-specific quarantine path for inspection after an
    /// interrupted move. Existence alone is not proof of successful migration.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    pub fn experimental_legacy_backup_path(&self) -> Result<std::path::PathBuf> {
        let old = self.experimental_legacy_path.as_ref()
            .ok_or_else(|| SearchError::Config("legacy index path unavailable".into()))?;
        let parent = old.parent().ok_or_else(|| SearchError::Config("legacy index parent unavailable".into()))?;
        let id = self.experimental_id.ok_or_else(|| SearchError::Config("index identity unavailable".into()))?;
        Ok(parent.join(crate::encrypted_legacy_quarantine::backup_name(&id)?))
    }
    /// Caller must hold its cross-process volume guard until this engine is dropped.
    pub fn open_existing(config: IndexConfig) -> Result<Self> {
        Self::open_existing_with_policy(config, Arc::new(|_| true))
    }

    pub fn open_existing_with_policy(config: IndexConfig, path_policy: PathPolicy) -> Result<Self> {
        let ci = Arc::new(ContentIndex::open_existing(&config.index_dir)?);
        Self::from_index(config, ci, path_policy)
    }

    /// Read an identity-verified private index after its mount path changes, without writes.
    /// Mapped paths must still pass the current configured roots and native path policy.
    pub fn open_existing_with_mapped_paths(
        config: IndexConfig,
        path_policy: PathPolicy,
        path_mapper: PathMapper,
    ) -> Result<Self> {
        let mut engine = Self::open_existing_with_policy(config, path_policy)?;
        engine.path_mapper = Some(path_mapper);
        Ok(engine)
    }

    /// Open an ordinary index concurrently with its writer. May create metadata lockfiles,
    /// but never creates an index, migrates its schema, or permits indexing operations.
    pub fn open_existing_shared(config: IndexConfig) -> Result<Self> {
        Self::open_existing_shared_with_policy(config, Arc::new(|_| true))
    }

    pub fn open_existing_shared_with_policy(
        config: IndexConfig,
        path_policy: PathPolicy,
    ) -> Result<Self> {
        let ci = Arc::new(ContentIndex::open_existing_shared(&config.index_dir)?);
        Self::from_index(config, ci, path_policy)
    }

    fn from_index(
        mut config: IndexConfig,
        ci: Arc<ContentIndex>,
        path_policy: PathPolicy,
    ) -> Result<Self> {
        // Index metadata changes on every commit. Never crawl or watch the
        // index itself, even when an allowed root contains it.
        config.skip_paths.push(config.index_dir.clone());
        if let Ok(physical) = config.index_dir.canonicalize() {
            if physical != config.index_dir {
                config.skip_paths.push(physical);
            }
        }
        let status = Arc::new(Mutex::new(IndexStatus::default()));
        Ok(Self {
            path_policy,
            watcher_invalid: Arc::new(AtomicBool::new(false)),
            watcher_generation: Arc::new(AtomicU64::new(0)),
            path_mapper: None,
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            experimental_fresh: false,
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            experimental_legacy_lease: None,
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            experimental_legacy_pins: None,
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            experimental_id: None,
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            experimental_legacy_path: None,
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            experimental_verified: AtomicBool::new(false),
            #[cfg(all(windows, feature = "experimental-encrypted-search"))]
            experimental_selection: None,
            config,
            ci,
            status,
            semantic: None,
            forensic: None,
            stop: Arc::new(AtomicBool::new(false)),
            watcher: Mutex::new(None),
            worker: Mutex::new(None),
        })
    }

    /// Wire in an optional semantic (vector) backend.
    pub fn attach_semantic(&mut self, backend: Arc<dyn SemanticBackend>) {
        self.semantic = Some(backend);
    }

    /// Wire in an optional forensic backend.
    pub fn attach_forensic(&mut self, backend: Arc<dyn ForensicBackend>) {
        self.forensic = Some(backend);
    }

    /// Index `files` synchronously in the calling thread.
    ///
    /// Used internally and in tests where background-thread timing is undesirable.
    pub fn index_files_sync(&self, files: Vec<FileMeta>) -> Result<()> {
        #[cfg(all(windows, feature = "experimental-encrypted-search"))]
        if self.experimental_fresh { return Err(SearchError::Config("strict candidate rebuild required".into())); }
        #[cfg(all(windows, feature = "experimental-encrypted-search"))]
        self.experimental_verified.store(false, Ordering::Release);
        let mut writer = self.ci.writer()?;
        {
            let mut s = self.status.lock().unwrap();
            s.is_indexing = true;
            s.pending_docs = files.len() as u64;
        }
        for meta in files {
            if crawler::path_is_skipped(&meta.path, &self.config.skip_paths) || !(self.path_policy)(&meta.path) {
                let mut status = self.status.lock().unwrap();
                status.pending_docs = status.pending_docs.saturating_sub(1);
                continue;
            }
            if let Err(e) = index_file(&self.ci, &mut writer, meta, &self.path_policy) {
                self.status.lock().unwrap().last_error = Some(e.to_string());
            }
            let mut s = self.status.lock().unwrap();
            s.pending_docs = s.pending_docs.saturating_sub(1);
            s.indexed_docs += 1;
        }
        self.ci.commit(&mut writer)?;
        self.status.lock().unwrap().is_indexing = false;
        Ok(())
    }

    /// Begin background indexing (full crawl + incremental watcher).
    ///
    /// A single background thread owns the entire write path sequentially:
    ///
    /// 1. Register the watcher first so FS events during the crawl accumulate
    ///    in the (unbounded) channel without being dropped.
    /// 2. Crawl the roots AND do the full index with ONE `IndexWriter`; commit
    ///    and drop it. Set `is_indexing = false`. The recursive directory walk
    ///    itself runs inside this thread (not the caller) — `collect_files`
    ///    over a large tree can take seconds, and this method is called while
    ///    `file_search::open_engine` holds the engine mutex, so walking here
    ///    would block every other FTS command until the crawl finished.
    /// 3. Drain the channel: for each buffered/live event acquire a fresh
    ///    writer, apply, commit.  No `LockBusy` is possible because the crawl
    ///    writer is already gone.
    pub fn start_indexing(&self) -> Result<()> {
        self.start_indexing_internal(false)
    }

    fn start_indexing_internal(&self, reconcile_first: bool) -> Result<()> {
        #[cfg(all(windows, feature = "experimental-encrypted-search"))]
        if self.experimental_fresh { return Err(SearchError::Config("strict candidate rebuild required".into())); }
        #[cfg(all(windows, feature = "experimental-encrypted-search"))]
        self.experimental_verified.store(false, Ordering::Release);
        let ci = Arc::clone(&self.ci);
        let path_policy = Arc::clone(&self.path_policy);
        let status = Arc::clone(&self.status);
        let stop = Arc::clone(&self.stop);
        let roots = self.config.roots.clone();
        let skip = self.config.skip_paths.clone();
        let max_bytes = self.config.max_file_bytes;
        // Building the GlobSet itself is cheap (just compiling patterns) —
        // only the recursive directory walk is slow, and that still happens
        // inside the spawned thread below. Shared by both the watcher
        // (applied to every FS event) and the crawl (applied per directory).
        let exclusions = build_globset(&self.config.exclusions)?;

        {
            let mut s = status.lock().unwrap();
            s.is_indexing = true;
            s.pending_docs = 0;
        }

        // ── Step 1: register watcher before spawning the worker ──────────
        // The engine owns the watcher so dropping/taking it closes `tx` and
        // wakes the drain loop.  On error, proceed crawl-only (as before).
        let (rx, watching) = if reconcile_first {
            // Bounded queue: overflow invalidates the cache and triggers a
            // full reconciliation rather than dropping events silently.
            let (tx, rx) = mpsc::sync_channel::<FsEventAction>(1024);
            let watching = watch::watch_roots_strict(&roots, skip.clone(), exclusions.clone(), max_bytes, tx,
                Arc::clone(&self.watcher_invalid), Arc::clone(&self.watcher_generation));
            (rx, watching)
        } else {
            let (tx, rx) = mpsc::channel::<FsEventAction>();
            let watching = watch_roots(&roots, skip.clone(), exclusions.clone(), max_bytes, tx);
            (rx, watching)
        };
        match watching {
            Ok(w) => { *self.watcher.lock().unwrap() = Some(w); }
            Err(e) if reconcile_first => {
                status.lock().unwrap().is_indexing = false;
                return Err(e);
            }
            Err(e) => { status.lock().unwrap().last_error = Some(e.to_string()); }
        }
        if reconcile_first {
            // Another window can temporarily own the writer. Wait a bounded
            // period while the watcher queues events, then fail explicitly.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let reconciled = loop {
                if self.watcher_invalid.load(Ordering::Acquire) {
                    break Err(SearchError::Config("search watcher requires reconciliation; retry".into()));
                }
                match self.reconcile_sync(&AtomicBool::new(false), usize::MAX) {
                    Err(SearchError::Index(tantivy::TantivyError::LockFailure(
                        tantivy::directory::error::LockError::LockBusy, _,
                    ))) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    result => break result,
                }
            };
            let error = match reconciled {
                Ok(report) if report.complete && !self.watcher_invalid.load(Ordering::Acquire) => None,
                Ok(_) => Some(SearchError::Config("encrypted cache reconciliation incomplete; retry".into())),
                Err(error) => Some(error),
            };
            if let Some(error) = error {
                self.watcher.lock().unwrap().take();
                status.lock().unwrap().is_indexing = false;
                return Err(error);
            }
        }

        let invalid = Arc::clone(&self.watcher_invalid);
        let invalid_generation = Arc::clone(&self.watcher_generation);
        let recovery_config = self.config.clone();
        #[cfg(all(windows, feature = "experimental-encrypted-search"))]
        let recovery_index_id = self.experimental_id;
        let handle = thread::spawn(move || {
            let recovery = if reconcile_first {
                match SearchEngine::from_index(recovery_config, Arc::clone(&ci), Arc::clone(&path_policy)) {
                    #[allow(unused_mut)]
                    Ok(mut engine) => {
                        #[cfg(all(windows, feature = "experimental-encrypted-search"))]
                        { engine.experimental_id = recovery_index_id; }
                        Some(engine)
                    }
                    Err(error) => { status.lock().unwrap().last_error = Some(error.to_string()); return; }
                }
            } else { None };
            // ── Step 2: crawl (moved off the caller thread) + full index ─
            let files = collect_files(&roots, &exclusions, &skip, max_bytes);
            {
                let mut s = status.lock().unwrap();
                s.pending_docs = files.len() as u64;
            }

            let mut writer = match wait_for_index_writer(&ci, &status, &stop) {
                Some(w) => w,
                None => {
                    let mut s = status.lock().unwrap();
                    s.is_indexing = false;
                    s.pending_docs = 0;
                    return;
                }
            };
            for meta in files {
                // Bail promptly if stop was requested.
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if !path_policy(&meta.path) {
                    let mut s = status.lock().unwrap();
                    s.pending_docs = s.pending_docs.saturating_sub(1);
                    continue;
                }
                if let Err(e) = index_file(&ci, &mut writer, meta, &path_policy) {
                    status.lock().unwrap().last_error = Some(e.to_string());
                }
                let mut s = status.lock().unwrap();
                s.pending_docs = s.pending_docs.saturating_sub(1);
                s.indexed_docs += 1;
            }
            if let Err(e) = ci.commit(&mut writer) {
                status.lock().unwrap().last_error = Some(e.to_string());
            }
            drop(writer); // release the crawl writer before entering the drain loop
            status.lock().unwrap().is_indexing = false;

            // ── Step 3: drain buffered + live watcher events ─────────────
            // Events are coalesced under one writer and committed at most once per
            // `COMMIT_DEBOUNCE` window instead of once per event, so a burst
            // of saves/downloads shares a single commit. Release it after each
            // batch so another app instance can update the same index.
            //
            // The loop exits naturally when tx is dropped (engine drops the
            // watcher) OR when the stop flag is set.
            const COMMIT_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(3);

            // Applies one classified FS action against the shared writer,
            // routing extract/upsert/delete errors into `status.last_error`
            // rather than aborting the drain loop.
            let apply_action = |w: &mut IndexWriter, action: FsEventAction| match action {
                FsEventAction::Upsert(meta) => {
                    if !path_policy(&meta.path) {
                        return;
                    }
                    if let Err(e) = index_file(&ci, w, meta, &path_policy) {
                        status.lock().unwrap().last_error = Some(e.to_string());
                    }
                }
                FsEventAction::Delete(id) => {
                    ci.delete(w, id);
                }
            };

            let mut writer: Option<IndexWriter> = None;
            let mut dirty = false;
            let mut last_event_at = std::time::Instant::now();

            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                match rx.recv_timeout(COMMIT_DEBOUNCE) {
                    Ok(action) => {
                        let w = match writer.as_mut() {
                            Some(w) => w,
                            None => match wait_for_index_writer(&ci, &status, &stop) {
                                Some(w) => writer.insert(w),
                                None => break,
                            },
                        };
                        apply_action(w, action);
                        dirty = true;
                        last_event_at = std::time::Instant::now();

                        // Drain any further already-queued events without
                        // committing between them — a burst of N events
                        // (e.g. an npm install) shares one writer + one commit.
                        while last_event_at.elapsed() < COMMIT_DEBOUNCE {
                            match rx.try_recv() {
                                Ok(action) => {
                                    apply_action(w, action);
                                    last_event_at = std::time::Instant::now();
                                }
                                Err(
                                    mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected,
                                ) => break,
                            }
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // No new events within the debounce window — fall
                        // through to the commit-if-dirty check below.
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        // Watcher dropped (engine stopping) — commit any
                        // pending writes, then exit.
                        if dirty {
                            if let Some(mut w) = writer.take() {
                                if let Err(e) = ci.commit(&mut w) {
                                    status.lock().unwrap().last_error = Some(e.to_string());
                                }
                            }
                        }
                        break;
                    }
                }

                // Reconcile after uncertain events once pending writes have
                // committed. The generation check prevents a concurrent event
                // from being cleared by an older completed scan.
                if invalid.load(Ordering::SeqCst) && !dirty {
                    if let Some(engine) = &recovery {
                        let observed = invalid_generation.load(Ordering::SeqCst);
                        match engine.reconcile_sync(&stop, usize::MAX) {
                            Ok(report) if report.complete => {
                                if invalid_generation.load(Ordering::SeqCst) == observed {
                                    invalid.store(false, Ordering::SeqCst);
                                    if invalid_generation.load(Ordering::SeqCst) != observed {
                                        invalid.store(true, Ordering::SeqCst);
                                    } else {
                                        let mut state = status.lock().unwrap();
                                        if state.last_error.as_deref().is_some_and(|message|
                                            message.starts_with("Search reconciliation failed; retrying:")
                                                || message == "Search reconciliation incomplete; retrying") {
                                            state.last_error = None;
                                        }
                                    }
                                }
                            }
                            Ok(_) => { status.lock().unwrap().last_error = Some("Search reconciliation incomplete; retrying".into()); }
                            Err(error) => { status.lock().unwrap().last_error = Some(format!("Search reconciliation failed; retrying: {error}")); }
                        }
                    }
                }
                // Commit once the debounce window has elapsed since the last
                // event, releasing the writer so other instances can proceed.
                if dirty && last_event_at.elapsed() >= COMMIT_DEBOUNCE {
                    if let Some(mut w) = writer.take() {
                        if let Err(e) = ci.commit(&mut w) {
                            status.lock().unwrap().last_error = Some(e.to_string());
                        }
                    }
                    dirty = false;
                }
            }

            // Final flush on shutdown so no buffered writes are lost.
            if dirty {
                if let Some(mut w) = writer.take() {
                    let _ = ci.commit(&mut w);
                }
            }
        });

        *self.worker.lock().unwrap() = Some(handle);
        Ok(())
    }

    /// Signal the worker to stop, drop the watcher (closes the channel so the
    /// drain loop wakes), and join the thread.  Safe to call more than once.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        // Drop the watcher to close tx so the for-in drain loop exits.
        let _ = self.watcher.lock().unwrap().take();
        // Join the worker; the stop flag ensures the loops exit promptly.
        if let Some(h) = self.worker.lock().unwrap().take() {
            let _ = h.join();
        }
    }

    /// Snapshot of current indexer state.
    pub fn status(&self) -> IndexStatus {
        let mut status = self.status.lock().unwrap().clone();
        if self.watcher_invalid.load(Ordering::Acquire) {
            status.last_error = Some("Search changes require reconciliation; reopen search".into());
        }
        status
    }

    fn ensure_watcher_valid(&self) -> Result<()> {
        if self.watcher_invalid.load(Ordering::Acquire) {
            return Err(SearchError::Config("Search changes require reconciliation; reopen search".into()));
        }
        Ok(())
    }

    /// Run a search, blending keyword and (optional) semantic results.
    ///
    /// `query.roots` (when non-empty) scopes BOTH rungs to files under one of
    /// those folders — `search_keyword` enforces it on the keyword side, and the
    /// semantic side is re-filtered here.
    pub fn search(&self, query: &ContentQuery) -> Result<Vec<ContentHit>> {
        self.ensure_watcher_valid()?;
        let kw_hits = self.ci.search_keyword(query)?;
        if query.keyword_only || self.semantic.is_none() {
            return Ok(kw_hits);
        }
        // KT: a SemanticBackend receives the whole ContentQuery (roots
        // included) but nothing OBLIGES it to honour the scope — an
        // implementation that ignores `roots` would blend out-of-scope docs
        // straight back in and silently defeat the folder scope the keyword
        // rung just enforced. Re-filter here so the scope can't be bypassed,
        // and over-fetch first (the filter runs after the backend's own limit)
        // so scoping doesn't truncate the semantic contribution to nothing.
        let sem_fetch = index::scoped_fetch(query.limit, &query.roots);
        let mut sem_hits = self.semantic.as_ref().unwrap().search(query, sem_fetch)?;
        if !query.roots.is_empty() {
            sem_hits.retain(|hit| index::path_in_roots(&hit.path, &query.roots));
        }
        Ok(merge_hits(kw_hits, sem_hits, query.limit))
    }

    /// Re-derive chunks for a stored document on demand (no separate chunk store in P1).
    pub fn get_chunks(&self, doc_id: crate::types::DocId) -> Result<Vec<crate::types::Chunk>> {
        self.get_chunks_inner(doc_id, false)
    }

    fn get_chunks_inner(
        &self,
        doc_id: crate::types::DocId,
        restricted: bool,
    ) -> Result<Vec<crate::types::Chunk>> {
        self.ensure_watcher_valid()?;
        use tantivy::query::TermQuery;
        use tantivy::schema::IndexRecordOption;

        let reader = self.ci.index_reader()?;
        let searcher = reader.searcher();

        let term = tantivy::Term::from_field_u64(self.ci.f_doc_id, doc_id);
        let tq = TermQuery::new(term, IndexRecordOption::Basic);
        let found = searcher
            .search(
                &tq,
                &tantivy::collector::TopDocs::with_limit(1).order_by_score(),
            )
            .map_err(SearchError::Index)?;

        if let Some((_score, addr)) = found.first() {
            let doc: tantivy::TantivyDocument = searcher.doc(*addr).map_err(SearchError::Index)?;

            let get_str = |f: tantivy::schema::Field| -> String {
                match doc.get_first(f).and_then(|value| value.as_str()) {
                    Some(s) => s.to_owned(),
                    _ => String::new(),
                }
            };
            let get_u64 = |f: tantivy::schema::Field| -> u64 {
                doc.get_first(f)
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default()
            };

            let mut meta = FileMeta {
                doc_id,
                path: std::path::PathBuf::from(get_str(self.ci.f_path)),
                name: get_str(self.ci.f_name),
                ext: get_str(self.ci.f_ext),
                mtime: get_u64(self.ci.f_mtime),
                size: get_u64(self.ci.f_size),
            };
            if restricted {
                if let Some(mapper) = &self.path_mapper {
                    let Some(path) = mapper(&meta.path) else {
                        return Ok(vec![]);
                    };
                    meta.path = path;
                }
                if !self.permits_path(&meta.path) {
                    return Ok(vec![]);
                }
            }
            let extracted = types::ExtractedDoc {
                meta,
                title: get_str(self.ci.f_title),
                body: get_str(self.ci.f_body),
                props: types::DocProps {
                    author: get_str(self.ci.f_author),
                    doc_title: get_str(self.ci.f_doc_title),
                    tags: get_str(self.ci.f_tags),
                },
            };
            return Ok(chunk_doc(&extracted));
        }
        Ok(vec![])
    }
}

impl Drop for SearchEngine {
    fn drop(&mut self) {
        // Always tear down the worker + watcher so no thread outlives the engine.
        self.stop();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use types::{ContentQuery, IndexConfig};

    // `notify` registrations are process-global on some Windows backends.
    // These two lifecycle tests both create a watcher and wait for its crawler;
    // run them one at a time to avoid watcher contention in a busy suite.
    static INDEXING_LIFECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_config(index_dir: std::path::PathBuf, roots: Vec<std::path::PathBuf>) -> IndexConfig {
        IndexConfig {
            roots,
            exclusions: vec![],
            skip_paths: vec![],
            max_file_bytes: 10_000_000,
            index_dir,
        }
    }

    #[test]
    fn index_inside_root_is_excluded_from_crawl_and_events() {
        let root = TempDir::new().unwrap();
        let index_dir = root.path().join("index");
        let document = root.path().join("document.json");
        fs::write(&document, "ordinary document").unwrap();
        let engine = SearchEngine::open(make_config(index_dir.clone(), vec![root.path().to_path_buf()])).unwrap();
        let exclusions = build_globset(&[]).unwrap();
        let files = collect_files(&engine.config.roots, &exclusions, &engine.config.skip_paths, u64::MAX);
        assert!(files.iter().any(|file| file.path == document));
        assert!(!files.iter().any(|file| file.path.starts_with(&index_dir)));
        let event = notify::Event {
            kind: notify::EventKind::Modify(notify::event::ModifyKind::Data(notify::event::DataChange::Content)),
            paths: vec![index_dir.join("meta.json")],
            attrs: Default::default(),
        };
        assert!(watch::apply_fs_event(&event, &engine.config.skip_paths, &exclusions, u64::MAX).is_none());
        let ordinary = notify::Event { paths: vec![document], ..event };
        assert!(watch::apply_fs_event(&ordinary, &engine.config.skip_paths, &exclusions, u64::MAX).is_some());
    }

    /// Build a small tempdir tree, index it synchronously, then search.
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn interrupted_generation_is_refused_and_left_untouched() {
        use crate::encrypted_index_prototype::tantivy_trial::TrialDirectory;
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy");
        fs::create_dir(&old).unwrap();
        fs::write(old.join("meta.json"), b"legacy intact").unwrap();
        let identity = [56; 16];
        let candidate = root.path().join(format!(".wincommander-encrypted-{}", hex::encode(identity)));
        fs::create_dir(&candidate).unwrap();
        let directory = TrialDirectory::open_protected(&candidate, crate::read_only::AccessMode::Writable, &identity, false).unwrap();
        let schema = tantivy::schema::Schema::builder().build();
        drop(tantivy::Index::create(directory, schema, Default::default()).unwrap());
        let before = fs::read(candidate.join("meta.json")).unwrap();
        let config = make_config(old.clone(), vec![root.path().to_path_buf()]);
        assert!(SearchEngine::open_experimental_encrypted_with_policy(config.clone(), Arc::new(|_| true), identity, true).is_err());
        assert!(SearchEngine::open_experimental_encrypted_with_policy(config, Arc::new(|_| true), identity, false).is_err());
        assert_eq!(fs::read(candidate.join("meta.json")).unwrap(), before);
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"legacy intact");
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn search_migration_intent_recovers_and_respects_replacement_boundary() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy"); let selection = root.path().join("selector");
        fs::create_dir(&old).unwrap(); fs::write(old.join("meta.json"), b"original cache").unwrap();
        let fence = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        let config = make_config(old.clone(), vec![]);
        SearchEngine::rebuild_offline_and_select(config, Arc::new(|_| false), &selection, &fence).unwrap();
        let tx = [11; 16];
        SearchEngine::begin_search_migration(&old, &selection, tx, &fence).unwrap();
        assert_eq!(SearchEngine::inspect_search_migration(&selection).unwrap(), Some((tx, false)));
        SearchEngine::begin_search_migration(&old, &selection, tx, &fence).unwrap();
        assert!(SearchEngine::begin_search_migration(&old, &selection, [12; 16], &fence).is_err());
        SearchEngine::abort_search_migration(&old, &selection, tx, &fence).unwrap();
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"original cache");
        SearchEngine::begin_search_migration(&old, &selection, tx, &fence).unwrap();
        let backup = fs::read_dir(root.path()).unwrap().map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(".wincommander-legacy-index-backup-")).unwrap();
        let displaced = root.path().join("displaced-original");
        fs::rename(&backup, &displaced).unwrap();
        fs::create_dir(&backup).unwrap();
        assert!(SearchEngine::mark_search_replacement(&old, &selection, tx, &fence).is_err());
        fs::remove_dir(&backup).unwrap();
        fs::rename(&displaced, &backup).unwrap();
        SearchEngine::mark_search_replacement(&old, &selection, tx, &fence).unwrap();
        assert_eq!(SearchEngine::inspect_search_migration(&selection).unwrap(), Some((tx, true)));
        assert!(SearchEngine::abort_search_migration(&old, &selection, tx, &fence).is_err());
        SearchEngine::commit_search_migration(&old, &selection, tx, &fence).unwrap();
        assert!(SearchEngine::inspect_search_migration(&selection).unwrap().is_none());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn interrupted_origin_rejects_changed_source_or_manifest() {
        use sha2::{Digest, Sha256};
        use tantivy::directory::Directory;
        let root = TempDir::new().unwrap(); let old = root.path().join("fts");
        let selection = root.path().join(".wincommander-encrypted-selection");
        fs::create_dir(&selection).unwrap();
        let config = make_config(old.clone(), vec![]); let id = [46; 16];
        let mut candidate = SearchEngine::open_experimental_encrypted_with_policy(config.clone(), Arc::new(|_| false), id, false).unwrap();
        candidate.rebuild_experimental_candidate().unwrap();
        let generation = root.path().join(format!(".wincommander-encrypted-{}", hex::encode(id)));
        let directory = crate::encrypted_index_prototype::tantivy_trial::TrialDirectory::open_protected(
            &generation, crate::read_only::AccessMode::Writable, &id, true).unwrap();
        let mut manifest = directory.atomic_read(std::path::Path::new("meta.json")).unwrap();
        let digest: [u8; 32] = Sha256::digest(&manifest).into();
        crate::encrypted_generation_prototype::record_origin(&selection, &id, true, &digest, &old).unwrap();
        manifest.push(b' '); // Still JSON, but no longer the approved manifest.
        directory.atomic_write(std::path::Path::new("meta.json"), &manifest).unwrap();
        drop(directory); drop(candidate);
        assert!(SearchEngine::open_or_rebuild_experimental_with_policy(config.clone(), Arc::new(|_| true)).is_err());
        assert!(!selection.join("candidate-proof.v1").exists());
        let original = root.path().join("original");
        fs::rename(&old, &original).unwrap(); fs::create_dir(&old).unwrap();
        assert!(SearchEngine::open_or_rebuild_experimental_with_policy(config, Arc::new(|_| true)).is_err());
        assert!(original.is_dir()); assert!(!selection.join("candidate-proof.v1").exists());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn interrupted_origin_recovers_valid_authenticated_candidate() {
        let root = TempDir::new().unwrap(); let old = root.path().join("fts");
        let selection = root.path().join(".wincommander-encrypted-selection");
        fs::create_dir(&selection).unwrap();
        let config = make_config(old, vec![]); let id = [45; 16];
        let mut candidate = SearchEngine::open_experimental_encrypted_with_policy(config.clone(), Arc::new(|_| false), id, false).unwrap();
        candidate.rebuild_experimental_candidate().unwrap();
        // Simulate interruption immediately after origin publication.
        use sha2::{Digest, Sha256};
        use tantivy::directory::Directory;
        let generation = root.path().join(format!(".wincommander-encrypted-{}", hex::encode(id)));
        let directory = crate::encrypted_index_prototype::tantivy_trial::TrialDirectory::open_protected(
            &generation, crate::read_only::AccessMode::GuardedReader, &id, true).unwrap();
        let manifest = directory.atomic_read(std::path::Path::new("meta.json")).unwrap();
        let digest: [u8; 32] = Sha256::digest(&manifest).into();
        crate::encrypted_generation_prototype::record_origin(&selection, &id, true, &digest, &root.path().join("fts")).unwrap();
        drop(directory);
        drop(candidate);
        assert!(!selection.join("candidate-proof.v1").exists());
        let recovered = SearchEngine::open_or_rebuild_experimental_with_policy(config, Arc::new(|_| true)).unwrap();
        assert!(selection.join("candidate-proof.v1").is_file());
        assert_eq!(crate::encrypted_generation_prototype::inspect_origin(&selection, &id).unwrap(), Some(true));
        recovered.stop();
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn interrupted_origin_without_selector_fails_closed_and_preserves_proof() {
        let root = TempDir::new().unwrap(); let old = root.path().join("fts");
        let selection = root.path().join(".wincommander-encrypted-selection");
        fs::create_dir(&selection).unwrap();
        fs::create_dir(&old).unwrap();
        crate::encrypted_generation_prototype::record_origin(&selection, &[44; 16], true, &[1; 32], &old).unwrap();
        let original = fs::read(selection.join("origin-proof.v1")).unwrap();
        let config = make_config(old, vec![]);
        assert!(SearchEngine::open_or_rebuild_experimental_with_policy(config, Arc::new(|_| true)).is_err());
        assert_eq!(fs::read(selection.join("origin-proof.v1")).unwrap(), original);
        assert!(!selection.join("candidate-proof.v1").exists());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn missing_selector_with_prior_backup_cannot_be_marked_fresh() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("fts");
        let backup = root.path().join(".wincommander-legacy-index-backup-11111111111111111111111111111111");
        fs::create_dir(&backup).unwrap(); fs::write(backup.join("old.txt"), b"preserve").unwrap();
        let config = make_config(old, vec![]);
        assert!(SearchEngine::open_or_rebuild_experimental_with_policy(config, Arc::new(|_| true)).is_err());
        assert_eq!(fs::read(backup.join("old.txt")).unwrap(), b"preserve");
        assert!(!root.path().join(".wincommander-encrypted-selection").join("origin-proof.v1").exists());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn fresh_install_scope_only_remains_verifiable_on_next_update() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("fts"); let data = root.path().join("data");
        fs::create_dir(&data).unwrap(); fs::write(data.join("doc.txt"), b"freshscope").unwrap();
        let config = make_config(old.clone(), vec![data]);
        let engine = SearchEngine::open_or_rebuild_experimental_with_policy(config, Arc::new(|_| true)).unwrap();
        engine.stop(); drop(engine);
        // A fresh setup has no old backup but may later publish its scope.
        // Keep the empty Tantivy writer lock created on first launch.
        publish_experimental_scope(&old, b"scope", |_| Ok(())).unwrap();
        let selection = root.path().join(".wincommander-encrypted-selection");
        SearchEngine::verify_fresh_scope_selection(&old, &selection).unwrap();
        // A quarantined-legacy check still requires its expected backup.
        assert!(SearchEngine::verify_quarantined_selection(&old, &selection).is_err());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn staged_index_opens_after_restart_without_moving_old_cache() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy"); let data = root.path().join("data"); let selection = root.path().join("selector");
        fs::create_dir(&old).unwrap(); fs::create_dir(&data).unwrap();
        fs::write(old.join("meta.json"), b"old app cache").unwrap();
        fs::write(data.join("doc.txt"), b"stagedrestartneedle").unwrap();
        let guard = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        let config = make_config(old.clone(), vec![]);
        SearchEngine::rebuild_offline_and_select(config, Arc::new(|_| false), &selection, &guard).unwrap();
        SearchEngine::verify_selected_generation(&old, &selection).unwrap();
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"old app cache");
        drop(guard);
        let live = SearchEngine::open_experimental_selected_with_policy(make_config(old.clone(), vec![data]), Arc::new(|_| true), &selection).unwrap();
        let query = ContentQuery { terms: "stagedrestartneedle".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(live.search(&query).unwrap().len(), 1);
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"old app cache");
        live.stop();
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn offline_empty_cache_reconciles_with_live_roots_after_restart() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy");
        let data = root.path().join("data");
        let selection = root.path().join("selector");
        fs::create_dir(&old).unwrap(); fs::create_dir(&data).unwrap();
        fs::write(old.join("meta.json"), b"legacy saved").unwrap();
        fs::write(data.join("doc.txt"), b"privatecatchupneedle").unwrap();
        let guard = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        let empty_config = make_config(old.clone(), vec![]);
        let report = SearchEngine::rebuild_offline_and_select(empty_config, Arc::new(|_| false), &selection, &guard).unwrap();
        assert_eq!(report.indexed, 0);
        let generation = fs::read_dir(root.path()).unwrap().map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(".wincommander-encrypted-")).unwrap();
        for entry in fs::read_dir(generation).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap();
            assert!(!bytes.windows(b"privatecatchupneedle".len()).any(|part| part == b"privatecatchupneedle"));
            assert!(!bytes.windows(b"doc.txt".len()).any(|part| part == b"doc.txt"));
        }
        let backup = SearchEngine::quarantine_selected_with_fence(&old, &selection, &guard).unwrap();
        assert_eq!(fs::read(backup.join("meta.json")).unwrap(), b"legacy saved");
        drop(guard);
        // The helper has exited. The selected-open path must reconcile using
        // the real current-user roots before returning a searchable engine.
        let live_config = make_config(old, vec![data]);
        let live = SearchEngine::open_experimental_selected_with_policy(live_config, Arc::new(|_| true), &selection).unwrap();
        let query = ContentQuery { terms: "privatecatchupneedle".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(live.search(&query).unwrap().len(), 1);
        live.stop();
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn offline_rebuild_selects_encrypted_generation_and_preserves_old_cache() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy");
        let data = root.path().join("data");
        let selection = root.path().join("selector");
        fs::create_dir(&old).unwrap(); fs::create_dir(&data).unwrap();
        fs::write(old.join("meta.json"), b"old cache preserved").unwrap();
        fs::write(data.join("doc.txt"), b"confidentialofflinerebuild").unwrap();
        let guard = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        let config = make_config(old.clone(), vec![data]);
        let report = SearchEngine::rebuild_offline_and_select(config, Arc::new(|_| true), &selection, &guard).unwrap();
        assert_eq!(report.indexed, 1);
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"old cache preserved");
        let backup = SearchEngine::quarantine_selected_with_fence(&old, &selection, &guard).unwrap();
        SearchEngine::verify_quarantined_selection(&old, &selection).unwrap();
        assert_eq!(fs::read(backup.join("meta.json")).unwrap(), b"old cache preserved");
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn experimental_scope_publication_requires_validated_pinned_root() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("file-search").join("fts");
        assert!(publish_experimental_scope(&dir, b"scope", |_| Err(std::io::Error::other("denied"))).is_err());
        assert!(!dir.join("scope-v2.dpapi").exists());
        publish_experimental_scope(&dir, b"scope", |_| Ok(())).unwrap();
        let protected = fs::read(dir.join("scope-v2.dpapi")).unwrap();
        assert!(!protected.windows(5).any(|part| part == b"scope"));
        publish_experimental_scope(&dir, b"scope", |_| Ok(())).unwrap();
        let path = dir.join("scope-v2.dpapi");
        fs::write(&path, b"corrupt").unwrap();
        assert!(publish_experimental_scope(&dir, b"replacement", |_| Ok(())).is_err());
        assert_eq!(fs::read(path).unwrap(), b"corrupt");
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn quarantine_requires_selected_cache_and_preserves_legacy_backup() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy"); let data = root.path().join("data");
        let selector = root.path().join("selector");
        fs::create_dir(&old).unwrap(); fs::create_dir(&data).unwrap(); fs::create_dir(&selector).unwrap();
        fs::write(old.join("meta.json"), b"legacy retained").unwrap();
        fs::write(data.join("doc.txt"), b"encryptedbackuptoken").unwrap();
        let config = make_config(old.clone(), vec![data]);
        let mut engine = SearchEngine::open_experimental_encrypted_with_policy(config, Arc::new(|_| true), [90; 16], false).unwrap();
        assert!(engine.quarantine_experimental_legacy_cache().is_err());
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"legacy retained");
        engine.rebuild_experimental_candidate().unwrap();
        engine.select_experimental_candidate(&selector).unwrap();
        let expected_backup = engine.experimental_legacy_backup_path().unwrap();
        // A concurrent cooperating maintenance process must prevent the move.
        let maintenance = std::fs::OpenOptions::new().read(true).write(true).create(true)
            .truncate(false).open(root.path().join(".encrypted-search-maintenance.lock")).unwrap();
        maintenance.lock().unwrap();
        assert!(engine.quarantine_experimental_legacy_cache().is_err());
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"legacy retained");
        drop(maintenance);
        let shared = crate::encrypted_search_fence::SearchOperationFence::acquire_shared(root.path()).unwrap();
        assert!(engine.quarantine_experimental_with_fence(&shared).is_err());
        drop(shared);
        let wrong_parent = TempDir::new().unwrap();
        let wrong = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(wrong_parent.path()).unwrap();
        assert!(engine.quarantine_experimental_with_fence(&wrong).is_err());
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"legacy retained");
        drop(wrong);
        let exclusive = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        let backup = SearchEngine::quarantine_selected_with_fence(&old, &selector, &exclusive).unwrap();
        drop(exclusive);
        assert_eq!(backup, expected_backup);
        assert!(!old.exists());
        fn snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
            fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
                for entry in fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() { walk(root, &path, out); }
                    else { out.push((path.strip_prefix(root).unwrap().to_path_buf(), fs::read(path).unwrap())); }
                }
            }
            let mut result = Vec::new(); walk(root, root, &mut result); result.sort(); result
        }
        let before_verify = snapshot(root.path());
        SearchEngine::verify_quarantined_selection(&old, &selector).unwrap();
        assert_eq!(snapshot(root.path()), before_verify, "read-only verification changed files");
        // A later app start recreates only the encrypted scope file in the old
        // directory. A second upgrade must accept it without moving or deleting it.
        publish_experimental_scope(&old, b"scope", |_| Ok(())).unwrap();
        assert!(SearchEngine::experimental_scope_only(&old).unwrap());
        SearchEngine::verify_quarantined_selection(&old, &selector).unwrap();
        assert!(old.join("scope-v2.dpapi").is_file());
        fs::remove_file(old.join("scope-v2.dpapi")).unwrap();
        fs::remove_dir(&old).unwrap();
        let displaced = root.path().join("displaced-backup");
        fs::rename(&backup, &displaced).unwrap();
        assert!(SearchEngine::verify_quarantined_selection(&old, &selector).is_err());
        fs::rename(&displaced, &backup).unwrap();
        assert_eq!(fs::read(backup.join("meta.json")).unwrap(), b"legacy retained");
        let query = ContentQuery { terms: "encryptedbackuptoken".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(engine.search(&query).unwrap().len(), 1);
        // Explicit recovery never replaces a newly created legacy directory.
        fs::create_dir(&old).unwrap();
        assert!(SearchEngine::verify_quarantined_selection(&old, &selector).is_err());
        assert!(restore_experimental_legacy_backup(&old, &[90; 16]).is_err());
        assert_eq!(fs::read(backup.join("meta.json")).unwrap(), b"legacy retained");
        fs::remove_dir(&old).unwrap();
        let restore_guard = crate::encrypted_search_fence::SearchOperationFence::acquire_exclusive(root.path()).unwrap();
        assert_eq!(SearchEngine::restore_quarantined_with_fence(&old, &selector, &restore_guard).unwrap(), old);
        drop(restore_guard);
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"legacy retained");
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn application_entry_rebuilds_then_refuses_corrupt_selector() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy"); let data = root.path().join("data");
        fs::create_dir(&data).unwrap(); fs::write(data.join("file.txt"), b"runtimecachetoken").unwrap();
        let config = make_config(old, vec![data]);
        let engine = SearchEngine::open_or_rebuild_experimental_with_policy(config.clone(), Arc::new(|_| true)).unwrap();
        let query = ContentQuery { terms: "runtimecachetoken".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(engine.search(&query).unwrap().len(), 1);
        drop(engine);
        let selector = root.path().join(".wincommander-encrypted-selection").join("candidate-proof.v1");
        fs::write(&selector, b"corrupted selector").unwrap();
        assert!(SearchEngine::open_or_rebuild_experimental_with_policy(config, Arc::new(|_| true)).is_err());
        assert_eq!(fs::read(selector).unwrap(), b"corrupted selector");
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn fresh_install_can_build_select_and_reopen_without_legacy_index() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("not-yet-created");
        let data = root.path().join("data"); let proof = root.path().join("proof");
        fs::create_dir(&data).unwrap(); fs::create_dir(&proof).unwrap();
        fs::write(data.join("fresh.txt"), b"freshcachetoken").unwrap();
        let config = make_config(old.clone(), vec![data.clone()]);
        assert!(SearchEngine::open_experimental_selected_with_policy(config.clone(), Arc::new(|_| true), &proof).is_err());
        let mut engine = SearchEngine::open_experimental_encrypted_with_policy(config.clone(), Arc::new(|_| true), [82; 16], false).unwrap();
        assert_eq!(engine.rebuild_experimental_candidate().unwrap().indexed, 1);
        engine.select_experimental_candidate(&proof).unwrap();
        // A selected engine no longer monopolizes the legacy writer lease.
        let other = SearchEngine::open_experimental_selected_with_policy(config.clone(), Arc::new(|_| true), &proof).unwrap();
        drop(other);
        drop(engine);
        fs::remove_file(data.join("fresh.txt")).unwrap();
        fs::write(data.join("replacement.txt"), b"replacementcachetoken").unwrap();
        let reopened = SearchEngine::open_experimental_selected_with_policy(config, Arc::new(|_| true), &proof).unwrap();
        let query = ContentQuery { terms: "freshcachetoken".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert!(reopened.search(&query).unwrap().is_empty());
        let new_query = ContentQuery { terms: "replacementcachetoken".into(), ..query };
        assert_eq!(reopened.search(&new_query).unwrap().len(), 1);
        assert!(old.is_dir());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn candidate_holds_cooperating_legacy_writer_lock() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy");
        fs::create_dir(&old).unwrap();
        let old_index = ContentIndex::open_or_create(&old).unwrap();
        let writer = old_index.writer().unwrap();
        let config = make_config(old.clone(), vec![]);
        assert!(SearchEngine::open_experimental_encrypted_with_policy(config.clone(), Arc::new(|_| true), [81; 16], false).is_err());
        drop(writer);
        let candidate = SearchEngine::open_experimental_encrypted_with_policy(config, Arc::new(|_| true), [81; 16], false).unwrap();
        assert!(old_index.writer().is_err());
        drop(candidate);
        assert!(old_index.writer().is_ok());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn changed_source_is_reconciled_after_cache_selection() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy"); let data = root.path().join("data");
        let proof = root.path().join("proof");
        fs::create_dir(&old).unwrap(); fs::create_dir(&data).unwrap(); fs::create_dir(&proof).unwrap();
        fs::write(data.join("a.txt"), b"original").unwrap();
        let config = make_config(old, vec![data.clone()]);
        let mut engine = SearchEngine::open_experimental_encrypted_with_policy(config, Arc::new(|_| true), [79; 16], false).unwrap();
        engine.rebuild_experimental_candidate().unwrap();
        fs::write(data.join("a.txt"), b"changedcontents").unwrap();
        engine.select_experimental_candidate(&proof).unwrap();
        assert!(proof.join("candidate-proof.v1").exists());
        engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
        let query = ContentQuery { terms: "changedcontents".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(engine.search(&query).unwrap().len(), 1);
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn reconciliation_invalidates_candidate_proof() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy"); let data = root.path().join("data");
        let proof = root.path().join("proof");
        fs::create_dir(&old).unwrap(); fs::create_dir(&data).unwrap(); fs::create_dir(&proof).unwrap();
        fs::write(data.join("a.txt"), b"original").unwrap();
        let config = make_config(old, vec![data.clone()]);
        let mut engine = SearchEngine::open_experimental_encrypted_with_policy(config, Arc::new(|_| true), [78; 16], false).unwrap();
        assert!(engine.reconcile_sync(&AtomicBool::new(false), 1).is_err());
        engine.rebuild_experimental_candidate().unwrap();
        fs::write(data.join("a.txt"), b"changed").unwrap();
        engine.reconcile_sync(&AtomicBool::new(false), 10).unwrap();
        assert!(engine.select_experimental_candidate(&proof).is_err());
        assert!(!proof.join("candidate-proof.v1").exists());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn failed_strict_crawl_cannot_create_candidate_proof() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy");
        let proof = root.path().join("proof");
        fs::create_dir(&old).unwrap(); fs::create_dir(&proof).unwrap();
        let config = make_config(old, vec![root.path().join("missing")]);
        let mut engine = SearchEngine::open_experimental_encrypted_with_policy(config, Arc::new(|_| true), [77; 16], false).unwrap();
        assert!(engine.rebuild_experimental_candidate().is_err());
        assert!(engine.select_experimental_candidate(&proof).is_err());
        assert!(!proof.join("candidate-proof.v1").exists());
    }
    #[cfg(all(windows, feature = "experimental-encrypted-search"))]
    #[test]
    fn experimental_engine_rebuilds_separate_generation_without_touching_legacy() {
        let root = TempDir::new().unwrap();
        let old = root.path().join("legacy");
        let data = root.path().join("documents");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&data).unwrap();
        fs::write(old.join("meta.json"), b"legacyindexsecrettoken").unwrap();
        fs::write(data.join("memo.txt"), b"secret protectedenginetoken").unwrap();
        let config = make_config(old.clone(), vec![root.path().to_path_buf()]);
        let mut engine = SearchEngine::open_experimental_encrypted_with_policy(config.clone(), Arc::new(|_| true), [55; 16], false).unwrap();
        let generation = engine.config.index_dir.clone();
        assert_ne!(generation, old);
        let exclusions = build_globset(&engine.config.exclusions).unwrap();
        let files = collect_files(&engine.config.roots, &exclusions, &engine.config.skip_paths, engine.config.max_file_bytes);
        assert!(files.iter().all(|f| !f.path.starts_with(&old) && !f.path.starts_with(&generation)));
        assert_eq!(engine.rebuild_experimental_candidate().unwrap().indexed, 1);
        assert!(engine.rebuild_experimental_candidate().is_err());
        let proof = root.path().join("proof");
        fs::create_dir(&proof).unwrap();
        engine.select_experimental_candidate(&proof).unwrap();
        assert!(proof.join("candidate-proof.v1").exists());
        // Even directly supplied file metadata must not reintroduce either index.
        let all = collect_files(&engine.config.roots, &exclusions, &[], engine.config.max_file_bytes);
        engine.index_files_sync(all).unwrap();
        for index_path in [old.join("meta.json"), generation.join("meta.json")] {
            let event = notify::Event {
                kind: notify::EventKind::Modify(notify::event::ModifyKind::Data(notify::event::DataChange::Content)),
                paths: vec![index_path], attrs: Default::default(),
            };
            assert!(watch::apply_fs_event(&event, &engine.config.skip_paths, &exclusions, engine.config.max_file_bytes).is_none());
        }
        let query = ContentQuery { terms: "protectedenginetoken".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(engine.search(&query).unwrap().len(), 1);
        let old_query = ContentQuery { terms: "legacyindexsecrettoken".into(), ..query.clone() };
        assert!(engine.search(&old_query).unwrap().is_empty());
        drop(engine);
        assert_eq!(fs::read(old.join("meta.json")).unwrap(), b"legacyindexsecrettoken");
        let reopened = SearchEngine::open_experimental_selected_with_policy(config, Arc::new(|_| true), &proof).unwrap();
        assert_eq!(reopened.search(&query).unwrap().len(), 1);
        assert!(generation.join("protected-search-key.v1").exists());
    }
    #[test]
    fn invalid_watcher_blocks_stale_search_and_reports_reopen() {
        let data = TempDir::new().unwrap(); let index = TempDir::new().unwrap();
        let config = make_config(index.path().into(), vec![data.path().into()]);
        let engine = SearchEngine::open(config).unwrap();
        engine.watcher_invalid.store(true, Ordering::Release);
        let query = ContentQuery { terms: "anything".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert!(engine.search(&query).is_err());
        assert!(engine.get_chunks(1).is_err());
        assert!(engine.status().last_error.unwrap().contains("reopen"));
    }
    #[cfg(unix)]
    #[test]
    fn oversized_update_removes_old_indexed_content() {
        let data = TempDir::new().unwrap(); let index = TempDir::new().unwrap();
        let file = data.path().join("memo.txt");
        fs::write(&file, b"smallcontenttoken").unwrap();
        let mut config = make_config(index.path().into(), vec![data.path().into()]);
        config.max_file_bytes = 64;
        let engine = SearchEngine::open(config).unwrap();
        engine.start_indexing_internal(true).unwrap();
        let query = ContentQuery { terms: "smallcontenttoken".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while engine.search(&query).map(|hits| hits.is_empty()).unwrap_or(true) {
            assert!(std::time::Instant::now() < deadline, "initial indexing did not finish");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        fs::write(&file, vec![b'x'; 128]).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if engine.search(&query).map(|hits| hits.is_empty()).unwrap_or(false) { break; }
            assert!(std::time::Instant::now() < deadline, "oversized update was not reconciled");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    #[cfg(unix)]
    #[test]
    fn strict_watcher_automatically_reconciles_a_deleted_file() {
        let data = TempDir::new().unwrap(); let index = TempDir::new().unwrap();
        let file = data.path().join("memo.txt");
        fs::write(&file, b"watcheddeletiontoken").unwrap();
        let config = make_config(index.path().into(), vec![data.path().into()]);
        let engine = SearchEngine::open(config).unwrap();
        engine.start_indexing_internal(true).unwrap();
        let query = ContentQuery { terms: "watcheddeletiontoken".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while engine.search(&query).map(|hits| hits.is_empty()).unwrap_or(true) {
            assert!(std::time::Instant::now() < deadline, "initial indexing did not finish");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        fs::remove_file(file).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if engine.search(&query).map(|hits| hits.is_empty()).unwrap_or(false) { break; }
            assert!(std::time::Instant::now() < deadline, "deletion was not reconciled");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    #[test]
    fn strict_reconciliation_start_fails_when_watcher_cannot_register() {
        let root = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        let config = make_config(index.path().to_path_buf(), vec![root.path().join("missing")]);
        let engine = SearchEngine::open(config).unwrap();
        assert!(engine.start_indexing_internal(true).is_err());
        assert!(!engine.status().is_indexing);
        assert!(engine.watcher.lock().unwrap().is_none());
        assert!(engine.worker.lock().unwrap().is_none());
    }
    #[cfg(unix)]
    #[test]
    fn native_file_lock_interoperates_with_tantivy_writer_lock_on_unix() {
        use tantivy::directory::{Directory, MmapDirectory, INDEX_WRITER_LOCK};
        let root = TempDir::new().unwrap();
        let dir = MmapDirectory::open(root.path()).unwrap();
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(root.path().join(".tantivy-writer.lock")).unwrap();
        file.try_lock().unwrap();
        assert!(dir.acquire_lock(&INDEX_WRITER_LOCK).is_err());
        drop(file);
        assert!(dir.acquire_lock(&INDEX_WRITER_LOCK).is_ok());
    }
    #[test]
    fn search_engine_open_index_and_search() {
        let data_dir = TempDir::new().unwrap();
        let index_dir = TempDir::new().unwrap();

        // Write a couple of plain-text files.
        let file_a = data_dir.path().join("memo.txt");
        let file_b = data_dir.path().join("notes.txt");
        fs::write(
            &file_a,
            "This document discusses quantum entanglement in detail.",
        )
        .unwrap();
        fs::write(&file_b, "Shopping list: milk eggs bread butter.").unwrap();

        let config = make_config(
            index_dir.path().to_path_buf(),
            vec![data_dir.path().to_path_buf()],
        );
        let engine = SearchEngine::open(config).unwrap();

        // Collect files and index synchronously (avoids thread timing).
        let exclusions = build_globset(&engine.config.exclusions).unwrap();
        let files = collect_files(
            &engine.config.roots,
            &exclusions,
            &engine.config.skip_paths,
            engine.config.max_file_bytes,
        );
        engine.index_files_sync(files).unwrap();

        let q = ContentQuery {
            terms: "quantum".into(),
            roots: vec![],
            limit: 10,
            offset: 0,
            keyword_only: true,
        };
        let hits = engine.search(&q).unwrap();
        assert_eq!(hits.len(), 1, "only memo.txt should match 'quantum'");
        assert!(hits[0].name.contains("memo"), "hit should be memo.txt");
    }

    /// Verify get_chunks returns at least one chunk for a known indexed doc.
    #[test]
    fn get_chunks_returns_chunks_for_indexed_doc() {
        let data_dir = TempDir::new().unwrap();
        let index_dir = TempDir::new().unwrap();

        let file = data_dir.path().join("chunk_test.txt");
        // Long enough body to produce multiple chunks.
        let body = "alpha ".repeat(200);
        fs::write(&file, &body).unwrap();

        let config = make_config(
            index_dir.path().to_path_buf(),
            vec![data_dir.path().to_path_buf()],
        );
        let engine = SearchEngine::open(config).unwrap();

        let exclusions = build_globset(&engine.config.exclusions).unwrap();
        let files = collect_files(
            &engine.config.roots,
            &exclusions,
            &engine.config.skip_paths,
            engine.config.max_file_bytes,
        );
        let doc_id = files[0].doc_id;
        engine.index_files_sync(files).unwrap();

        let chunks = engine.get_chunks(doc_id).unwrap();
        assert!(!chunks.is_empty(), "should return at least one chunk");
    }

    /// Smoke test: start_indexing eventually completes and search finds a body word.
    ///
    /// Verify the crawl waits for the other writer and resumes after release.
    #[test]
    fn competing_index_writer_waits_and_resumes_without_losing_the_crawl() {
        let _serial = INDEXING_LIFECYCLE_TEST_LOCK.lock().unwrap();
        let data = TempDir::new().unwrap();
        let index = TempDir::new().unwrap();
        fs::write(data.path().join("known.txt"), "contentionrecoverytoken").unwrap();
        let config = make_config(index.path().to_path_buf(), vec![data.path().to_path_buf()]);
        let first = SearchEngine::open(config.clone()).unwrap();
        let second = SearchEngine::open(config).unwrap();
        let writer = first.ci.writer().unwrap();
        second.start_indexing().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while second.status().last_error.as_deref() != Some(WRITER_WAIT_MESSAGE) {
            assert!(std::time::Instant::now() < deadline, "writer contention was not reported");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(writer);
        // Contention and resumption need independent budgets on a busy runner.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while second.status().is_indexing {
            assert!(std::time::Instant::now() < deadline, "indexing did not resume");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(second.status().last_error.is_none());
        let q = ContentQuery { terms: "contentionrecoverytoken".into(), roots: vec![], limit: 10, offset: 0, keyword_only: true };
        assert_eq!(second.search(&q).unwrap().len(), 1);
        second.stop();
    }

    #[test]
    fn indexer_can_stop_while_another_writer_is_active() {
        let _serial = INDEXING_LIFECYCLE_TEST_LOCK.lock().unwrap();
        let index = TempDir::new().unwrap();
        let config = make_config(index.path().to_path_buf(), vec![]);
        let first = SearchEngine::open(config.clone()).unwrap();
        let second = SearchEngine::open(config).unwrap();
        let _writer = first.ci.writer().unwrap();
        second.start_indexing().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while second.status().last_error.as_deref() != Some(WRITER_WAIT_MESSAGE) {
            assert!(std::time::Instant::now() < deadline, "writer contention was not reported");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        second.stop();
        assert!(!second.status().is_indexing);
        assert_eq!(second.status().pending_docs, 0);
    }

    #[test]
    fn start_indexing_smoke_search_after_crawl() {
        let _serial = INDEXING_LIFECYCLE_TEST_LOCK.lock().unwrap();
        let data_dir = TempDir::new().unwrap();
        let index_dir = TempDir::new().unwrap();

        let file = data_dir.path().join("smoke.txt");
        fs::write(&file, "The gruntledwombat lives in the southern savannah.").unwrap();

        let config = make_config(
            index_dir.path().to_path_buf(),
            vec![data_dir.path().to_path_buf()],
        );
        let engine = SearchEngine::open(config).unwrap();
        engine.start_indexing().unwrap();

        // Poll until is_indexing == false, up to 5 s.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if !engine.status().is_indexing {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "start_indexing did not finish within 5 s"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let q = ContentQuery {
            terms: "gruntledwombat".into(),
            roots: vec![],
            limit: 10,
            offset: 0,
            keyword_only: true,
        };
        let hits = engine.search(&q).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "start_indexing should have indexed smoke.txt"
        );
        assert!(!hits[0].snippet.is_empty(), "snippet must be non-empty");
    }

    /// status() transitions correctly around index_files_sync.
    #[test]
    fn status_reflects_indexing_progress() {
        let index_dir = TempDir::new().unwrap();
        let config = make_config(index_dir.path().to_path_buf(), vec![]);
        let engine = SearchEngine::open(config).unwrap();
        let s0 = engine.status();
        assert!(!s0.is_indexing);
        assert_eq!(s0.indexed_docs, 0);

        // With no files the sync path still transitions correctly.
        engine.index_files_sync(vec![]).unwrap();
        let s1 = engine.status();
        assert!(!s1.is_indexing);
    }

    #[test]
    fn malformed_pdf_does_not_set_global_last_error() {
        use std::io::Write;

        let data_dir = TempDir::new().unwrap();
        let index_dir = TempDir::new().unwrap();
        let pdf_path = data_dir.path().join("broken.pdf");
        let mut pdf = fs::File::create(&pdf_path).unwrap();
        write!(pdf, "not a real pdf").unwrap();

        let config = make_config(
            index_dir.path().to_path_buf(),
            vec![data_dir.path().to_path_buf()],
        );
        let engine = SearchEngine::open(config).unwrap();
        engine
            .index_files_sync(vec![FileMeta {
                doc_id: 1,
                path: pdf_path,
                name: "broken.pdf".into(),
                ext: "pdf".into(),
                mtime: 0,
                size: 14,
            }])
            .unwrap();

        let status = engine.status();
        assert_eq!(status.indexed_docs, 1);
        assert!(
            status.last_error.is_none(),
            "malformed PDFs should be skipped per file, not surfaced as global status: {:?}",
            status.last_error
        );
    }

    /// Engine stop() tears down the worker and releases the index dir.
    ///
    /// Verifies:
    /// - start_indexing completes (is_indexing goes false within 5 s)
    /// - stop() joins the worker (no lingering IndexWriter lock)
    /// - a fresh open + writer() on the same dir succeeds after stop
    /// - calling stop() a second time does not panic
    #[test]
    fn engine_stop_releases_worker_and_index() {
        let _serial = INDEXING_LIFECYCLE_TEST_LOCK.lock().unwrap();
        let data_dir = TempDir::new().unwrap();
        let index_dir = TempDir::new().unwrap();

        let file = data_dir.path().join("lifecycle.txt");
        fs::write(&file, "The uniquelifecycletoken confirms indexing ran.").unwrap();

        let config = make_config(
            index_dir.path().to_path_buf(),
            vec![data_dir.path().to_path_buf()],
        );
        let engine = SearchEngine::open(config.clone()).unwrap();
        engine.start_indexing().unwrap();

        // Poll until crawl is done (bounded ~5 s).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if !engine.status().is_indexing {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "start_indexing did not finish within 5 s"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Stop the engine — worker must join.
        engine.stop();

        // A fresh open + writer on the same dir must succeed (no lingering lock).
        let ci2 = ContentIndex::open_or_create(&config.index_dir)
            .expect("open_or_create must succeed after stop()");
        let _writer = ci2
            .writer()
            .expect("writer() must succeed after stop() — no lingering IndexWriter lock");

        // Calling stop() a second time must not panic.
        engine.stop();
    }

    /// A semantic backend that ignores `query.roots` entirely (the realistic
    /// failure mode) and records the limit it was asked for.
    struct ScopeBlindSemantic {
        hits: Vec<ContentHit>,
        asked_limit: Mutex<usize>,
    }

    impl backend::SemanticBackend for ScopeBlindSemantic {
        fn search(&self, _query: &ContentQuery, limit: usize) -> Result<Vec<ContentHit>> {
            *self.asked_limit.lock().unwrap() = limit;
            Ok(self.hits.clone())
        }
    }

    fn sem_hit(doc_id: u64, path: &str) -> ContentHit {
        ContentHit {
            doc_id,
            path: path.to_owned(),
            name: "x.txt".into(),
            ext: "txt".into(),
            mtime: 0,
            size: 0,
            score: 1.0,
            match_kind: types::MatchKind::Semantic,
            snippet: String::new(),
            author: String::new(),
            doc_title: String::new(),
            tags: String::new(),
        }
    }

    /// Semantic hits must be re-filtered against `query.roots` — otherwise a
    /// backend that ignores the scope silently reintroduces out-of-scope rows
    /// that the keyword rung had excluded.
    #[test]
    fn hybrid_search_filters_semantic_hits_by_root_scope() {
        let index_dir = TempDir::new().unwrap();
        let config = make_config(index_dir.path().to_path_buf(), vec![]);
        let mut engine = SearchEngine::open(config).unwrap();

        let semantic = Arc::new(ScopeBlindSemantic {
            hits: vec![
                sem_hit(1, r"C:\projects\alpha\in.txt"),
                sem_hit(2, r"C:\projects\alpha-extra\sibling.txt"),
                sem_hit(3, r"C:\projects\beta\out.txt"),
            ],
            asked_limit: Mutex::new(0),
        });
        engine.attach_semantic(semantic.clone());

        let q = ContentQuery {
            terms: "anything".into(),
            roots: vec![std::path::PathBuf::from(r"C:\projects\alpha")],
            limit: 10,
            offset: 0,
            keyword_only: false, // blend in the semantic rung
        };
        let hits = engine.search(&q).unwrap();
        let ids: Vec<u64> = hits.iter().map(|h| h.doc_id).collect();
        assert_eq!(
            ids,
            vec![1],
            "only the in-scope semantic hit may survive (the sibling-prefix folder must not leak): {:?}",
            hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>()
        );
        // And the backend was over-fetched, since the scope is applied after it.
        assert!(
            *semantic.asked_limit.lock().unwrap() > q.limit,
            "a scoped hybrid search must over-fetch the semantic rung"
        );
    }

    /// The unscoped hybrid path must be untouched: no filtering, no over-fetch.
    #[test]
    fn hybrid_search_unscoped_keeps_every_semantic_hit() {
        let index_dir = TempDir::new().unwrap();
        let config = make_config(index_dir.path().to_path_buf(), vec![]);
        let mut engine = SearchEngine::open(config).unwrap();

        let semantic = Arc::new(ScopeBlindSemantic {
            hits: vec![
                sem_hit(1, r"C:\projects\alpha\in.txt"),
                sem_hit(2, r"C:\projects\beta\out.txt"),
            ],
            asked_limit: Mutex::new(0),
        });
        engine.attach_semantic(semantic.clone());

        let q = ContentQuery {
            terms: "anything".into(),
            roots: vec![],
            limit: 10,
            offset: 0,
            keyword_only: false,
        };
        let hits = engine.search(&q).unwrap();
        assert_eq!(hits.len(), 2, "empty roots must not filter anything");
        assert_eq!(
            *semantic.asked_limit.lock().unwrap(),
            q.limit,
            "unscoped hybrid search must ask for exactly `limit` as before"
        );
    }
}
