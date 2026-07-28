// SPDX-License-Identifier: AGPL-3.0-or-later
//! `wincmd-search` — full-text file-content search for WinCommander (Free tier).
//!
//! Unit A foundation: types, error, and backend extension traits.
//! Unit B: crawler + watch.
//! Unit C: text extractors (plain, office, pdf) and chunker.
//! Unit D: tantivy index, hit-merge helper, and SearchEngine facade.

pub mod backend;
pub mod chunk;
pub mod crawler;
pub mod error;
pub mod extract;
pub mod filters;
pub mod index;
pub mod query;
pub mod tokenize;
pub mod types;
pub mod watch;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use tantivy::schema::Value;
use tantivy::IndexWriter;

use backend::{ForensicBackend, SemanticBackend};
use chunk::chunk_doc;
use crawler::{build_globset, collect_files};
use error::{Result, SearchError};
use extract::extract_text;
use index::ContentIndex;
use query::merge_hits;
use types::{ContentHit, ContentQuery, FileMeta, IndexConfig, IndexStatus};
use watch::{watch_roots, FsEventAction};

/// Facade: owns the index and orchestrates crawl → extract → chunk → index.
pub struct SearchEngine {
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

impl SearchEngine {
    /// Open (or create) the index described by `config`.
    pub fn open(config: IndexConfig) -> Result<Self> {
        let ci = Arc::new(ContentIndex::open_or_create(&config.index_dir)?);
        let status = Arc::new(Mutex::new(IndexStatus::default()));
        Ok(Self {
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
        let mut writer = self.ci.writer()?;
        {
            let mut s = self.status.lock().unwrap();
            s.is_indexing = true;
            s.pending_docs = files.len() as u64;
        }
        for meta in files {
            match extract_text(meta.clone()) {
                Ok(doc) => {
                    if let Err(e) =
                        self.ci
                            .upsert(&mut writer, &doc.meta, &doc.title, &doc.body, &doc.props)
                    {
                        self.status.lock().unwrap().last_error = Some(e.to_string());
                    }
                }
                Err(e) if e.is_per_file_skip() => { /* no extractor or malformed file — skip */ }
                Err(e) => {
                    self.status.lock().unwrap().last_error = Some(e.to_string());
                }
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
        let ci = Arc::clone(&self.ci);
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
        let (tx, rx) = mpsc::channel::<FsEventAction>();
        match watch_roots(&roots, skip.clone(), exclusions.clone(), max_bytes, tx) {
            Ok(w) => {
                *self.watcher.lock().unwrap() = Some(w);
            }
            Err(e) => {
                status.lock().unwrap().last_error = Some(e.to_string());
            }
        }

        let handle = thread::spawn(move || {
            // ── Step 2: crawl (moved off the caller thread) + full index ─
            let files = collect_files(&roots, &exclusions, &skip, max_bytes);
            {
                let mut s = status.lock().unwrap();
                s.pending_docs = files.len() as u64;
            }

            let mut writer = match ci.writer() {
                Ok(w) => w,
                Err(e) => {
                    status.lock().unwrap().last_error = Some(e.to_string());
                    return;
                }
            };
            for meta in files {
                // Bail promptly if stop was requested.
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match extract_text(meta.clone()) {
                    Ok(doc) => {
                        if let Err(e) =
                            ci.upsert(&mut writer, &doc.meta, &doc.title, &doc.body, &doc.props)
                        {
                            status.lock().unwrap().last_error = Some(e.to_string());
                        }
                    }
                    Err(e) if e.is_per_file_skip() => { /* no extractor or malformed file — skip */
                    }
                    Err(e) => {
                        status.lock().unwrap().last_error = Some(e.to_string());
                    }
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
            // ONE persistent IndexWriter is reused across every event instead
            // of opening a fresh ~50 MB writer per filesystem event (that
            // allocation is what made node_modules/.git churn so expensive).
            // Events are coalesced and committed at most once per
            // `COMMIT_DEBOUNCE` window instead of once per event, so a burst
            // of saves/downloads shares a single commit.
            //
            // The loop exits naturally when tx is dropped (engine drops the
            // watcher) OR when the stop flag is set.
            const COMMIT_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(3);

            // Applies one classified FS action against the shared writer,
            // routing extract/upsert/delete errors into `status.last_error`
            // rather than aborting the drain loop.
            let apply_action = |w: &mut IndexWriter, action: FsEventAction| {
                match action {
                    FsEventAction::Upsert(meta) => {
                        match extract_text(meta.clone()) {
                            Ok(doc) => {
                                if let Err(e) =
                                    ci.upsert(w, &doc.meta, &doc.title, &doc.body, &doc.props)
                                {
                                    status.lock().unwrap().last_error = Some(e.to_string());
                                }
                            }
                            Err(e) if e.is_per_file_skip() => { /* no extractor or malformed file — skip */
                            }
                            Err(e) => {
                                status.lock().unwrap().last_error = Some(e.to_string());
                            }
                        }
                    }
                    FsEventAction::Delete(id) => {
                        ci.delete(w, id);
                    }
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
                            None => match ci.writer() {
                                Ok(w) => writer.insert(w),
                                Err(e) => {
                                    status.lock().unwrap().last_error = Some(e.to_string());
                                    continue;
                                }
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

                // Commit once the debounce window has elapsed since the last
                // event, reusing the same writer for the next batch.
                if dirty && last_event_at.elapsed() >= COMMIT_DEBOUNCE {
                    if let Some(w) = writer.as_mut() {
                        if let Err(e) = ci.commit(w) {
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
        self.status.lock().unwrap().clone()
    }

    /// Run a search, blending keyword and (optional) semantic results.
    ///
    /// `query.roots` (when non-empty) scopes BOTH rungs to files under one of
    /// those folders — `search_keyword` enforces it on the keyword side, and the
    /// semantic side is re-filtered here.
    pub fn search(&self, query: &ContentQuery) -> Result<Vec<ContentHit>> {
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

            let meta = FileMeta {
                doc_id,
                path: std::path::PathBuf::from(get_str(self.ci.f_path)),
                name: get_str(self.ci.f_name),
                ext: get_str(self.ci.f_ext),
                mtime: get_u64(self.ci.f_mtime),
                size: get_u64(self.ci.f_size),
            };
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

    fn make_config(index_dir: std::path::PathBuf, roots: Vec<std::path::PathBuf>) -> IndexConfig {
        IndexConfig {
            roots,
            exclusions: vec![],
            skip_paths: vec![],
            max_file_bytes: 10_000_000,
            index_dir,
        }
    }

    /// Build a small tempdir tree, index it synchronously, then search.
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
    /// Polls status().is_indexing with a tight loop bounded to ~5 s so the test
    /// remains fast under normal CI.  Uses a unique token ("gruntledwombat") to
    /// avoid cross-test interference.
    #[test]
    fn start_indexing_smoke_search_after_crawl() {
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
