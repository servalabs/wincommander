// SPDX-License-Identifier: AGPL-3.0-or-later
//! `wincmd-search` — full-text file-content search for WinCommander (Free tier).
//!
//! Unit A foundation: types, error, and backend extension traits.
//! Unit B: crawler + watch.
//! Unit C: text extractors (plain, office, pdf) and chunker.
//! Unit D: tantivy index, hit-merge helper, and SearchEngine facade.

pub mod backend;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_index_prototype;
#[cfg(test)]
mod encrypted_cutover_prototype;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_key_prototype;
#[cfg(any(test, all(windows, feature = "experimental-encrypted-search")))]
mod encrypted_scope;
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
        // Generation creation is exclusive; interrupted candidates remain.
        if !existing {
            std::fs::create_dir(&generation)?;
        }
        // Avoid ingesting either index if a user's search root contains them.
        config.skip_paths.push(old);
        config.skip_paths.push(generation.clone());
        config.index_dir = generation;
        let ci = Arc::new(ContentIndex::open_experimental_encrypted(&config.index_dir, index_id, existing)?);
        let mut engine = Self::from_index(config, ci, path_policy)?;
        engine.experimental_legacy_lease = legacy_lease;
        engine.experimental_legacy_pins = legacy_pins;
        engine.experimental_fresh = !existing;
        engine.experimental_id = Some(index_id);
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
        engine.select_experimental_candidate(&selection)?;
        engine.start_indexing_internal(true)?;
        Ok(engine)
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
