// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/file_search.rs
//
// ═══════════════════════════════════════════════════════════════════════
// FILE-CONTENT SEARCH — Free-tier full-text search (FTS) commands
// ═══════════════════════════════════════════════════════════════════════
//
// Five Tauri commands wired into the `wincmd-search` crate SearchEngine.
// Index lives under %LOCALAPPDATA%\WinCommander\file-search\fts (per-user,
// isolated to the current Windows account — other local users can't read
// the indexed file paths/content snippets).
//
// Decoy integration: enrolled honeypot paths (file_monitor) are injected
// as `skip_paths` in every IndexConfig so reading them never updates
// last-access time and trips tamper detection.
//
// Engine lifecycle: module-global OnceCell<RwLock<Option<Arc<SearchEngine>>>>.
// First command call that touches the engine opens it if it isn't already
// open; content_index_configure, content_rescan, and content_reindex drop it
// to force re-creation (rescan reopens the same dir; reindex deletes it first).
//
// Lock discipline: every read/replacement first acquires a bounded, session-
// local Windows named mutex shared by the GUI and headless process. It then takes the
// in-process RwLock: reads retain its shared guard for the engine call, while
// configure/rescan/reindex hold the writer guard while they stop or replace
// the engine and, for a full reindex, remove its directory. A replacement
// therefore cannot delete files beneath an active reader in either process.

use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use std::sync::RwLock;
use wincmd_search::{
    types::{Chunk, ContentHit, ContentQuery, IndexConfig, IndexStatus},
    SearchEngine,
};

use crate::settings::{read_settings, write_settings, AppSettings, FileSearchSettings};

// ---------------------------------------------------------------------------
// Module-level engine singleton
// ---------------------------------------------------------------------------

static ENGINE: OnceCell<RwLock<Option<Arc<SearchEngine>>>> = OnceCell::new();

// The index itself is under the current user's LOCALAPPDATA. Match that
// ownership boundary with an unprefixed/session-local mutex, rather than a
// Global namespace object that could create cross-user contention or DACL
// surprises. The GUI and its headless invocation run in the same session.
const INDEX_PROCESS_LOCK_NAME: &str = "WinCommander_ContentIndex_lock";
const INDEX_READ_LOCK_TIMEOUT_MS: u32 = 5_000;
const INDEX_REPLACEMENT_LOCK_TIMEOUT_MS: u32 = 15_000;

#[derive(Clone, Copy)]
enum IndexLockMode {
    Read,
    Replacement,
}

impl IndexLockMode {
    fn timeout_ms(self) -> u32 {
        match self {
            Self::Read => INDEX_READ_LOCK_TIMEOUT_MS,
            Self::Replacement => INDEX_REPLACEMENT_LOCK_TIMEOUT_MS,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Replacement => "replacement",
        }
    }
}

fn abandoned_index_lock_error() -> String {
    "content index lock was abandoned; repair or rescan is required before reading the index"
        .to_string()
}

/// Cross-process gate for the session-local index directory. A Windows mutex is
/// deliberately exclusive (rather than a faux reader/writer protocol): it
/// gives a crash-safe, fail-closed boundary between GUI and CLI without
/// introducing an unaudited shared-memory state machine.
struct IndexProcessLock {
    #[cfg(windows)]
    handle: isize,
}

impl IndexProcessLock {
    #[cfg(windows)]
    fn acquire(mode: IndexLockMode) -> Result<Self, String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            CreateMutexW, ReleaseMutex, WaitForSingleObject,
        };

        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_ABANDONED: u32 = 0x80;
        const WAIT_TIMEOUT: u32 = 0x102;

        let name: Vec<u16> = format!("{INDEX_PROCESS_LOCK_NAME}\0")
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err("content index lock is unavailable; refusing the operation".to_string());
        }
        match unsafe { WaitForSingleObject(handle, mode.timeout_ms()) } {
            WAIT_OBJECT_0 => Ok(Self {
                handle: handle as isize,
            }),
            // WAIT_ABANDONED transfers mutex ownership to this process, but
            // also proves the prior holder exited mid-operation. Release that
            // ownership before closing the handle and refuse to read a
            // potentially half-replaced Tantivy directory.
            WAIT_ABANDONED => {
                unsafe {
                    ReleaseMutex(handle);
                    CloseHandle(handle);
                }
                Err(abandoned_index_lock_error())
            }
            WAIT_TIMEOUT => {
                unsafe { CloseHandle(handle) };
                Err(format!(
                    "content index {} lock is busy; refusing after {} ms",
                    mode.label(),
                    mode.timeout_ms()
                ))
            }
            _ => {
                unsafe { CloseHandle(handle) };
                Err("content index lock wait failed; refusing the operation".to_string())
            }
        }
    }

    #[cfg(not(windows))]
    fn acquire(_mode: IndexLockMode) -> Result<Self, String> {
        // The Windows-only index directory is only enforced on Windows; this
        // keeps compile-only non-Windows targets from claiming runtime safety.
        Ok(Self {})
    }
}

impl Drop for IndexProcessLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.handle != 0 {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            unsafe {
                ReleaseMutex(self.handle as _);
                CloseHandle(self.handle as _);
            }
        }
    }
}

fn engine_cell() -> &'static RwLock<Option<Arc<SearchEngine>>> {
    ENGINE.get_or_init(|| RwLock::new(None))
}

/// Restrict the FTS index dir to SYSTEM / Administrators / owner. Called only
/// from `open_engine`'s engine-creation path (first open + after a
/// reindex/reconfigure drops the engine) — NOT per search — so spawning
/// `icacls` here is cheap, and it re-runs whenever the dir is recreated. A
/// process-global `Once` (the previous guard) hardened once then never again,
/// so a post-`content_reindex` dir was left with default inherited ACLs — other
/// local users could then read indexed paths/snippets until the next restart.
fn harden_fts_dir(dir: &std::path::Path) {
    if dir.exists() {
        crate::paths::harden_dir_acl(dir);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Absolute path to the FTS index directory:
/// `%LOCALAPPDATA%\WinCommander\file-search\fts`
/// (user_data_dir() creates the base dir and resolves LOCALAPPDATA).
/// Per-user, not machine-wide: the index holds indexed file paths and
/// content snippets, so keeping it under the current account's LOCALAPPDATA
/// stops other local users from reading it.
pub fn fts_index_dir() -> Result<PathBuf, String> {
    let fts_dir = crate::paths::user_data_dir()?
        .join("file-search")
        .join("fts");
    Ok(fts_dir)
}

/// Paths enrolled as decoys — must never be indexed (reading them updates
/// atime and would trip the decoy monitor's tamper detection).
pub(crate) fn decoy_skip_paths() -> Vec<PathBuf> {
    crate::file_monitor::enrolled_decoy_paths()
}

/// Returns the user's personal folders that actually exist on disk.
/// Only `%USERPROFILE%\Desktop`, `\Downloads`, `\Documents` are seeded —
/// AppData is intentionally excluded (it's not under these roots anyway).
pub(crate) fn default_roots() -> Vec<PathBuf> {
    default_roots_from(std::env::var("USERPROFILE").ok())
}

/// Testable core of `default_roots`: accepts the USERPROFILE value directly
/// so unit tests can supply an arbitrary path without mutating the environment.
fn default_roots_from(profile: Option<String>) -> Vec<PathBuf> {
    let profile = match profile {
        Some(p) => p,
        None => return Vec::new(),
    };
    ["Desktop", "Downloads", "Documents"]
        .iter()
        .map(|name| PathBuf::from(&profile).join(name))
        .filter(|p| p.exists())
        .collect()
}

/// Default exclusion globs — keep the index clean without burdening the user.
/// Applied only when the caller has an empty exclusions list.
const DEFAULT_EXCLUSIONS: &[&str] = &["node_modules", ".git", "*.tmp", "*.temp", "~$*"];

/// On the very first use (settings.app.file_search.initialized == false),
/// seeds roots from the user's personal folders and writes back to settings.
/// Returns the (potentially updated) settings so the caller can use them immediately.
fn ensure_initialized(mut settings: AppSettings) -> Result<AppSettings, String> {
    if !settings.app.file_search.initialized {
        settings.app.file_search.roots = default_roots();
        settings.app.file_search.initialized = true;
        if settings.app.file_search.exclusions.is_empty() {
            settings.app.file_search.exclusions =
                DEFAULT_EXCLUSIONS.iter().map(|s| s.to_string()).collect();
        }
        write_settings(&settings)?;
    }
    Ok(settings)
}

/// Map persisted FileSearchSettings into a wincmd_search IndexConfig.
fn build_index_config(fs: &FileSearchSettings) -> Result<IndexConfig, String> {
    Ok(IndexConfig {
        roots: fs.roots.clone(),
        exclusions: fs.exclusions.clone(),
        skip_paths: decoy_skip_paths(),
        max_file_bytes: 50 * 1024 * 1024, // 50 MB per file
        index_dir: fts_index_dir()?,
    })
}

/// Build a ContentQuery from frontend parameters with safe defaults.
fn build_content_query(
    terms: String,
    roots: Vec<PathBuf>,
    limit: Option<usize>,
    offset: Option<usize>,
    keyword_only: Option<bool>,
) -> ContentQuery {
    ContentQuery {
        terms,
        roots,
        limit: limit.unwrap_or(50),
        offset: offset.unwrap_or(0),
        keyword_only: keyword_only.unwrap_or(true),
    }
}

/// Validate a folder scope for `ContentQuery.roots`.
///
/// KT: this is NOT `backend.rs::validate_es_scope_path` reused — that function
/// rejects a leading `-`/`/` because its value becomes an es.exe argv entry
/// and would otherwise be read as a command-line switch. This value never
/// reaches a process argv: it only ever becomes a `PathBuf` fed to
/// `wincmd_search::index::path_in_roots`, a pure case-insensitive string
/// prefix comparison (see `index.rs`), so a leading `-`/`/` is inert here —
/// just a folder name that won't prefix-match any real Windows path. What DOES
/// still matter, and what this mirrors from `validate_es_scope_path`: reject
/// empty (an empty scope is a caller bug — "no scope" must be `None`, not
/// `Some("")`) and reject control characters (can't appear in a real path and
/// would otherwise flow unmodified into error strings/logs).
fn validate_content_scope_path(scope: &str) -> Result<String, String> {
    let trimmed = scope.trim();
    if trimmed.is_empty() {
        return Err("Search folder is empty.".to_string());
    }
    if trimmed.chars().any(char::is_control) {
        return Err(format!(
            "Invalid search folder '{scope}': control characters are not allowed."
        ));
    }
    Ok(trimmed.to_string())
}

/// Resolve which roots a `search_content` call should scope to.
///
/// `scope_path`, when present, REPLACES the configured roots for this one
/// query — it becomes the sole entry in `ContentQuery.roots`. A scope that
/// lies outside every currently-indexed root is deliberately not rejected: no
/// indexed document's path can prefix-match it, so `path_in_roots` naturally
/// filters the result set down to nothing — the same "legitimately zero hits"
/// outcome as any other over-narrow scope, not a distinct error. `None` keeps
/// today's behaviour byte-for-byte: every configured root, i.e. unscoped.
fn resolve_content_roots(
    scope_path: Option<String>,
    configured_roots: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    match scope_path {
        Some(raw) => {
            let validated = validate_content_scope_path(&raw)?;
            Ok(vec![PathBuf::from(validated)])
        }
        None => Ok(configured_roots),
    }
}

/// Open the engine if it isn't already open. Idempotent.
/// Holds the lock only for the duration of open+start_indexing (fast).
/// The slow background crawl runs in the spawned thread after we release.
fn open_engine_locked(
    guard: &mut Option<Arc<SearchEngine>>,
    config: IndexConfig,
) -> Result<(), String> {
    if guard.is_none() {
        // Create + harden the index dir BEFORE opening. harden_dir_acl sets
        // inheritable ACEs but does not recurse, so the dir must be restricted
        // before tantivy writes any files (meta.json, segments) — they then
        // inherit the restriction. This runs on every engine (re)creation —
        // first open AND after content_reindex/content_index_configure drop the
        // engine and (for reindex) delete the dir — so a freshly-recreated
        // index is never left readable by other local users.
        let index_dir = config.index_dir.clone();
        let _ = std::fs::create_dir_all(&index_dir);
        harden_fts_dir(&index_dir);
        let engine = SearchEngine::open(config).map_err(|e| e.to_string())?;
        engine.start_indexing().map_err(|e| e.to_string())?;
        *guard = Some(Arc::new(engine));
    }
    Ok(())
}

fn open_engine(config: IndexConfig) -> Result<(), String> {
    let mut guard = engine_cell().write().map_err(|e| e.to_string())?;
    open_engine_locked(&mut guard, config)
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Full-text search across indexed file contents.
///
/// Async so the IPC dispatch doesn't run on the main thread; the actual
/// engine/settings work is synchronous (file IO + tantivy), so it runs on a
/// blocking-pool thread via `spawn_blocking`.
///
/// `scope_path` (JS sends camelCase `scopePath`; Tauri maps it to this
/// snake_case parameter) optionally narrows the query to one folder — the
/// same "in this folder" scope the filename search's `search_everything`
/// already honours via its own `scope_path`. Omitted/`None` reproduces
/// exactly today's behaviour: every configured content-search root.
#[tauri::command]
pub async fn search_content(
    terms: String,
    limit: Option<usize>,
    offset: Option<usize>,
    keyword_only: Option<bool>,
    scope_path: Option<String>,
) -> Result<Vec<ContentHit>, String> {
    tokio::task::spawn_blocking(move || {
        let _process_lock = IndexProcessLock::acquire(IndexLockMode::Read)?;
        let settings = ensure_initialized(read_settings()?)?;
        let fs = settings.app.file_search;
        let config = build_index_config(&fs)?;
        open_engine(config.clone())?;

        let guard = engine_cell().read().map_err(|e| e.to_string())?;
        let engine = guard
            .as_ref()
            .ok_or_else(|| "search engine not initialized".to_string())?;

        let roots = resolve_content_roots(scope_path, config.roots.clone())?;
        let query = build_content_query(terms, roots, limit, offset, keyword_only);
        engine.search(&query).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("search_content task failed: {e}"))?
}

/// Return a snapshot of the current index state (doc count, progress, errors).
#[tauri::command]
pub async fn content_index_status() -> Result<IndexStatus, String> {
    tokio::task::spawn_blocking(move || {
        let _process_lock = IndexProcessLock::acquire(IndexLockMode::Read)?;
        let settings = ensure_initialized(read_settings()?)?;
        let fs = settings.app.file_search;
        let config = build_index_config(&fs)?;
        open_engine(config)?;

        let guard = engine_cell().read().map_err(|e| e.to_string())?;
        let engine = guard
            .as_ref()
            .ok_or_else(|| "search engine not initialized".to_string())?;
        Ok(engine.status())
    })
    .await
    .map_err(|e| format!("content_index_status task failed: {e}"))?
}

/// Persist new roots/exclusions and restart indexing with the updated config.
#[tauri::command]
pub async fn content_index_configure(
    roots: Vec<PathBuf>,
    exclusions: Vec<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _process_lock = IndexProcessLock::acquire(IndexLockMode::Replacement)?;
        // Persist into the encrypted settings store (app.fileSearch.*).
        let settings = {
            let mut settings = read_settings()?;
            settings.app.file_search.roots = roots;
            settings.app.file_search.exclusions = exclusions;
            settings.app.file_search.initialized = true; // user explicitly configured
            write_settings(&settings)?;
            settings
        };
        // Stop the old engine explicitly before releasing the Arc.  An in-flight
        // search_content call may hold a clone, deferring Drop — stop() joins the
        // worker immediately so there is never a second writer on the index dir.
        let mut guard = engine_cell().write().map_err(|e| e.to_string())?;
        if let Some(old) = guard.take() {
            old.stop();
        }
        // Build config from the already-written settings — no redundant read.
        let config = build_index_config(&settings.app.file_search)?;
        open_engine_locked(&mut guard, config)
    })
    .await
    .map_err(|e| format!("content_index_configure task failed: {e}"))?
}

/// Re-crawl the indexed roots WITHOUT deleting the index first — an incremental
/// rescan. Unlike `content_reindex` (which wipes the dir and rebuilds), this
/// keeps every already-indexed document searchable while a fresh crawl
/// re-extracts and upserts every file: new files are added, and missed or
/// changed files are refreshed in place (doc_id is a stable path hash, so a
/// re-crawl upserts rather than duplicating). Trade-off vs a full reindex:
/// entries for files deleted while the app was closed are NOT pruned here —
/// only `content_reindex` clears those.
#[tauri::command]
pub async fn content_rescan() -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _process_lock = IndexProcessLock::acquire(IndexLockMode::Replacement)?;
        // Stop + join the old worker so its watcher/writer are released before a
        // new engine opens on the same dir (tantivy permits one writer at a
        // time). The index dir is deliberately left in place — the reopened
        // engine serves all existing docs immediately, so content search never
        // goes blank the way it does during a full reindex.
        let mut guard = engine_cell().write().map_err(|e| e.to_string())?;
        if let Some(old) = guard.take() {
            old.stop();
        }
        let settings = ensure_initialized(read_settings()?)?;
        let config = build_index_config(&settings.app.file_search)?;
        open_engine_locked(&mut guard, config)
    })
    .await
    .map_err(|e| format!("content_rescan task failed: {e}"))?
}

/// Drop the engine and rebuild the index from scratch (full re-crawl).
#[tauri::command]
pub async fn content_reindex() -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _process_lock = IndexProcessLock::acquire(IndexLockMode::Replacement)?;
        // Stop + join the old worker BEFORE deleting the index dir so the running
        // crawl never hits a directory that was removed under it.
        let mut guard = engine_cell().write().map_err(|e| e.to_string())?;
        if let Some(old) = guard.take() {
            old.stop();
        }
        // Remove index directory to force a full re-crawl.
        if let Ok(dir) = fts_index_dir() {
            let _ = std::fs::remove_dir_all(dir);
        }

        let settings = ensure_initialized(read_settings()?)?;
        let config = build_index_config(&settings.app.file_search)?;
        open_engine_locked(&mut guard, config)
    })
    .await
    .map_err(|e| format!("content_reindex task failed: {e}"))?
}

/// Return all indexed chunks for a document (for preview / context display).
/// `doc_id` arrives as a string from JS — Tauri can't safely deserialize a
/// 64-bit integer from JSON without precision loss on the JS side.
///
/// Does NOT call ensure_initialized: by the time the user can click a result
/// to preview, search_content has already seeded the index.  Calling
/// ensure_initialized here would trigger first-run seeding from a read-only
/// document-preview call, which is not an intentional user opt-in.
#[tauri::command]
pub async fn content_get_doc(doc_id: String) -> Result<Vec<Chunk>, String> {
    tokio::task::spawn_blocking(move || {
        let _process_lock = IndexProcessLock::acquire(IndexLockMode::Read)?;
        let id: u64 = doc_id.parse().map_err(|_| "invalid doc_id".to_string())?;

        let guard = engine_cell().read().map_err(|e| e.to_string())?;
        let engine = guard
            .as_ref()
            .ok_or_else(|| "search engine not initialized".to_string())?;
        engine.get_chunks(id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("content_get_doc task failed: {e}"))?
}

// ---------------------------------------------------------------------------
// Tests — pure helper coverage
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_content_index_read_and_replacement_uses_the_named_process_lock() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/file_search.rs"));
        for (operation, mode) in [
            ("search_content", "IndexLockMode::Read"),
            ("content_index_status", "IndexLockMode::Read"),
            ("content_get_doc", "IndexLockMode::Read"),
            ("content_index_configure", "IndexLockMode::Replacement"),
            ("content_rescan", "IndexLockMode::Replacement"),
            ("content_reindex", "IndexLockMode::Replacement"),
        ] {
            let signature = format!("pub async fn {operation}");
            let start = source
                .find(&signature)
                .unwrap_or_else(|| panic!("missing {operation}"));
            let body = &source[start..start + 700.min(source.len() - start)];
            assert!(
                body.contains(&format!("IndexProcessLock::acquire({mode})")),
                "{operation} must take the cross-process lock"
            );
        }
        assert_eq!(INDEX_PROCESS_LOCK_NAME, "WinCommander_ContentIndex_lock");
        assert!(!INDEX_PROCESS_LOCK_NAME.starts_with("Global\\"));
        assert!(INDEX_READ_LOCK_TIMEOUT_MS > 0);
        assert!(INDEX_REPLACEMENT_LOCK_TIMEOUT_MS >= INDEX_READ_LOCK_TIMEOUT_MS);
        assert!(abandoned_index_lock_error().contains("repair or rescan"));
        assert!(source.contains("WAIT_ABANDONED => {"));
        assert!(source.contains("ReleaseMutex(handle);"));
    }

    /// build_index_config maps roots + exclusions correctly and hard-codes 50 MB.
    #[test]
    fn build_index_config_maps_fields() {
        let fs = FileSearchSettings {
            roots: vec![PathBuf::from("C:\\Users\\test")],
            exclusions: vec!["*.tmp".to_string(), "node_modules".to_string()],
            initialized: true,
            result_limit: 200,
        };
        // Call the real function; skip if LOCALAPPDATA is unset (no index_dir).
        match build_index_config(&fs) {
            Ok(ic) => {
                assert_eq!(ic.roots, vec![PathBuf::from("C:\\Users\\test")]);
                assert_eq!(ic.exclusions, vec!["*.tmp", "node_modules"]);
                assert_eq!(ic.max_file_bytes, 50 * 1024 * 1024);
                let index_dir_str = ic.index_dir.to_string_lossy().to_lowercase();
                assert!(
                    index_dir_str.ends_with("fts"),
                    "index_dir must end with fts, got: {index_dir_str}"
                );
            }
            Err(_) => {
                // LOCALAPPDATA unset or user_data_dir failed — skip.
            }
        }
    }

    /// build_content_query applies defaults when optional params are None.
    #[test]
    fn build_content_query_defaults() {
        let q = build_content_query("hello world".into(), vec![], None, None, None);
        assert_eq!(q.terms, "hello world");
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
        assert!(q.keyword_only, "keyword_only should default to true");
    }

    /// build_content_query respects explicit params.
    #[test]
    fn build_content_query_explicit_params() {
        let root = PathBuf::from("C:\\Data");
        let q = build_content_query(
            "quantum".into(),
            vec![root.clone()],
            Some(25),
            Some(10),
            Some(false),
        );
        assert_eq!(q.limit, 25);
        assert_eq!(q.offset, 10);
        assert!(!q.keyword_only);
        assert_eq!(q.roots, vec![root]);
    }

    /// fts_index_dir ends with the expected path suffix.
    /// Only runs when LOCALAPPDATA is set (always true on Windows; CI skips gracefully).
    #[test]
    fn fts_index_dir_suffix() {
        // Skip if LOCALAPPDATA env var is absent (unlikely but safe).
        if std::env::var("LOCALAPPDATA").is_err() {
            return;
        }
        let dir = fts_index_dir().expect("fts_index_dir must succeed on Windows");
        let s = dir.to_string_lossy().to_lowercase();
        assert!(
            s.contains("file-search"),
            "index dir must contain file-search, got: {s}"
        );
        assert!(s.ends_with("fts"), "index dir must end with fts, got: {s}");
        // Also verify parent contains "file-search" and leaf is "fts".
        let parent = dir
            .parent()
            .map(|p| p.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        assert!(
            parent.contains("file-search"),
            "parent of index dir must contain file-search, got: {parent}"
        );
    }

    /// decoy_skip_paths returns a Vec (may be empty) without panicking.
    #[test]
    fn decoy_skip_paths_does_not_panic() {
        let paths = decoy_skip_paths();
        // No assertion on contents — the decoy set is empty in test context.
        let _ = paths;
    }

    /// FileSearchSettings::default yields empty roots, exclusions, and initialized=false.
    #[test]
    fn file_search_settings_default() {
        let fs = FileSearchSettings::default();
        assert!(fs.roots.is_empty());
        assert!(fs.exclusions.is_empty());
        assert!(!fs.initialized, "initialized must default to false");
    }

    /// default_roots returns only paths that exist; every returned path must exist.
    #[test]
    fn default_roots_only_existing_paths() {
        let roots = default_roots();
        for p in &roots {
            assert!(
                p.exists(),
                "default_roots returned a non-existent path: {p:?}"
            );
        }
    }

    /// default_roots_from(None) returns an empty vec — missing USERPROFILE branch.
    #[test]
    fn default_roots_no_userprofile() {
        let roots = default_roots_from(None);
        assert!(
            roots.is_empty(),
            "expected empty vec when USERPROFILE is absent, got: {roots:?}"
        );
    }

    /// default_roots_from returns only the subdirs that actually exist.
    #[test]
    fn default_roots_from_existing_subdir() {
        use std::fs;
        let tmp = std::env::temp_dir().join(format!(
            "commander_test_roots_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        fs::create_dir_all(tmp.join("Desktop")).expect("create Desktop");
        // Downloads and Documents are intentionally absent.

        let profile = tmp.to_string_lossy().to_string();
        let roots = default_roots_from(Some(profile));

        assert_eq!(
            roots.len(),
            1,
            "only Desktop should be returned, got: {roots:?}"
        );
        assert!(roots[0].ends_with("Desktop"));

        // Cleanup — ignore errors (temp dir may already be cleaned up by OS).
        let _ = fs::remove_dir_all(&tmp);
    }

    // ── scope_path (FIX-E: content-search folder scoping) ──────────────────

    /// A valid scope path becomes the sole ContentQuery root, replacing —
    /// not merging with — the configured roots.
    #[test]
    fn resolve_content_roots_scope_overrides_configured() {
        let configured = vec![
            PathBuf::from(r"C:\Users\test\Desktop"),
            PathBuf::from(r"C:\Users\test\Documents"),
        ];
        let roots =
            resolve_content_roots(Some(r"D:\Projects\wincommander".to_string()), configured)
                .expect("valid scope must resolve");
        assert_eq!(roots, vec![PathBuf::from(r"D:\Projects\wincommander")]);
    }

    /// `None` must reproduce today's exact behaviour: every configured root,
    /// unchanged — this is the no-regression guarantee for existing callers.
    #[test]
    fn resolve_content_roots_none_keeps_configured_roots() {
        let configured = vec![
            PathBuf::from(r"C:\Users\test\Desktop"),
            PathBuf::from(r"C:\Users\test\Documents"),
        ];
        let roots = resolve_content_roots(None, configured.clone()).expect("None must never error");
        assert_eq!(roots, configured);
    }

    /// A scope outside every indexed root is NOT an error — resolve_content_roots
    /// passes it through as-is; `path_in_roots` (wincmd-search) naturally yields
    /// zero hits since no indexed document's path can prefix-match it. This test
    /// pins that "not our job to detect" choice at the boundary we own.
    #[test]
    fn resolve_content_roots_scope_outside_indexed_roots_is_not_an_error() {
        let configured = vec![PathBuf::from(r"C:\Users\test\Desktop")];
        let roots = resolve_content_roots(Some(r"Z:\Never\Indexed".to_string()), configured)
            .expect("an out-of-index scope must resolve, not error");
        assert_eq!(roots, vec![PathBuf::from(r"Z:\Never\Indexed")]);
    }

    /// Empty (or all-whitespace) scope is a caller bug, not "no scope" — the
    /// frontend must send `None` to mean unscoped, matching
    /// `backend.rs::validate_es_scope_path`'s empty rejection.
    #[test]
    fn resolve_content_roots_rejects_empty_scope() {
        let err = resolve_content_roots(Some("   ".to_string()), vec![]).unwrap_err();
        assert!(
            err.contains("empty"),
            "expected an empty-scope error, got: {err}"
        );
    }

    /// Control characters can't appear in a real Windows path — reject, same
    /// as `backend.rs::validate_es_scope_path`.
    #[test]
    fn resolve_content_roots_rejects_control_chars() {
        let err =
            resolve_content_roots(Some("C:\\Users\\test\u{0007}".to_string()), vec![]).unwrap_err();
        assert!(
            err.contains("control characters"),
            "expected a control-char error, got: {err}"
        );
    }

    /// Leading `-`/`/` is inert here (unlike `validate_es_scope_path`, this
    /// value never becomes an es.exe argv entry) — it must NOT be rejected.
    #[test]
    fn resolve_content_roots_allows_leading_dash_and_slash() {
        let roots = resolve_content_roots(Some("-weird-folder".to_string()), vec![])
            .expect("leading '-' must not be rejected for content scope");
        assert_eq!(roots, vec![PathBuf::from("-weird-folder")]);

        let roots = resolve_content_roots(Some("/mnt/data".to_string()), vec![])
            .expect("leading '/' must not be rejected for content scope");
        assert_eq!(roots, vec![PathBuf::from("/mnt/data")]);
    }

    /// Surrounding whitespace is trimmed, mirroring `validate_es_scope_path`.
    #[test]
    fn resolve_content_roots_trims_whitespace() {
        let roots = resolve_content_roots(Some("  D:\\My Files\\notes  ".to_string()), vec![])
            .expect("whitespace-padded scope must resolve");
        assert_eq!(roots, vec![PathBuf::from(r"D:\My Files\notes")]);
    }

    /// initialized field round-trips through serde correctly.
    #[test]
    fn file_search_settings_initialized_serde() {
        let fs = FileSearchSettings {
            roots: vec![],
            exclusions: vec![],
            initialized: true,
            result_limit: 200,
        };
        let json = serde_json::to_string(&fs).expect("serialize");
        assert!(
            json.contains("\"initialized\":true"),
            "initialized must serialize"
        );
        let back: FileSearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(back.initialized);

        // Old JSON without the field must deserialize to initialized=false.
        let old = r#"{"roots":[],"exclusions":[]}"#;
        let old_fs: FileSearchSettings = serde_json::from_str(old).expect("old deserialize");
        assert!(!old_fs.initialized, "missing field must default to false");
        assert_eq!(old_fs.result_limit, 200, "missing limit must retain the shipped default");
    }
}
