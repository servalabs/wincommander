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
// Engine lifecycle: module-global OnceCell<Mutex<Option<Arc<SearchEngine>>>>.
// First command call that touches the engine opens it if it isn't already
// open; content_index_configure, content_rescan, and content_reindex drop it
// to force re-creation (rescan reopens the same dir; reindex deletes it first).
//
// Lock discipline: search/status/get_chunks acquire the lock ONLY to clone
// the Arc, then drop the guard before calling the (potentially slow) engine
// method — so a running FTS search never blocks configure/reindex.

use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use std::sync::Mutex;
use wincmd_search::{
    types::{Chunk, ContentHit, ContentQuery, IndexConfig, IndexStatus},
    SearchEngine,
};

use crate::settings::{read_settings, write_settings, AppSettings, FileSearchSettings};

// ---------------------------------------------------------------------------
// Module-level engine singleton
// ---------------------------------------------------------------------------

static ENGINE: OnceCell<Mutex<Option<Arc<SearchEngine>>>> = OnceCell::new();

fn engine_cell() -> &'static Mutex<Option<Arc<SearchEngine>>> {
    ENGINE.get_or_init(|| Mutex::new(None))
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

/// Open the engine if it isn't already open. Idempotent.
/// Holds the lock only for the duration of open+start_indexing (fast).
/// The slow background crawl runs in the spawned thread after we release.
fn open_engine(config: IndexConfig) -> Result<(), String> {
    let mut guard = engine_cell().lock().map_err(|e| e.to_string())?;
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

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Full-text search across indexed file contents.
///
/// Async so the IPC dispatch doesn't run on the main thread; the actual
/// engine/settings work is synchronous (file IO + tantivy), so it runs on a
/// blocking-pool thread via `spawn_blocking`.
#[tauri::command]
pub async fn search_content(
    terms: String,
    limit: Option<usize>,
    offset: Option<usize>,
    keyword_only: Option<bool>,
) -> Result<Vec<ContentHit>, String> {
    tokio::task::spawn_blocking(move || {
        let settings = ensure_initialized(read_settings()?)?;
        let fs = settings.app.file_search;
        let config = build_index_config(&fs)?;
        open_engine(config.clone())?;

        let engine = {
            let guard = engine_cell().lock().map_err(|e| e.to_string())?;
            guard.as_ref().cloned()
        };
        let engine = engine.ok_or_else(|| "search engine not initialized".to_string())?;

        let query = build_content_query(terms, config.roots.clone(), limit, offset, keyword_only);
        engine.search(&query).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("search_content task failed: {e}"))?
}

/// Return a snapshot of the current index state (doc count, progress, errors).
#[tauri::command]
pub async fn content_index_status() -> Result<IndexStatus, String> {
    tokio::task::spawn_blocking(move || {
        let settings = ensure_initialized(read_settings()?)?;
        let fs = settings.app.file_search;
        let config = build_index_config(&fs)?;
        open_engine(config)?;

        let engine = {
            let guard = engine_cell().lock().map_err(|e| e.to_string())?;
            guard.as_ref().cloned()
        };
        let engine = engine.ok_or_else(|| "search engine not initialized".to_string())?;
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
        let old = { engine_cell().lock().map_err(|e| e.to_string())?.take() };
        if let Some(old) = old {
            old.stop();
        }
        // Build config from the already-written settings — no redundant read.
        let config = build_index_config(&settings.app.file_search)?;
        open_engine(config)
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
        // Stop + join the old worker so its watcher/writer are released before a
        // new engine opens on the same dir (tantivy permits one writer at a
        // time). The index dir is deliberately left in place — the reopened
        // engine serves all existing docs immediately, so content search never
        // goes blank the way it does during a full reindex.
        let old = { engine_cell().lock().map_err(|e| e.to_string())?.take() };
        if let Some(old) = old {
            old.stop();
        }
        let settings = ensure_initialized(read_settings()?)?;
        let config = build_index_config(&settings.app.file_search)?;
        open_engine(config)
    })
    .await
    .map_err(|e| format!("content_rescan task failed: {e}"))?
}

/// Drop the engine and rebuild the index from scratch (full re-crawl).
#[tauri::command]
pub async fn content_reindex() -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        // Stop + join the old worker BEFORE deleting the index dir so the running
        // crawl never hits a directory that was removed under it.
        let old = { engine_cell().lock().map_err(|e| e.to_string())?.take() };
        if let Some(old) = old {
            old.stop();
        }
        // Remove index directory to force a full re-crawl.
        if let Ok(dir) = fts_index_dir() {
            let _ = std::fs::remove_dir_all(dir);
        }

        let settings = ensure_initialized(read_settings()?)?;
        let config = build_index_config(&settings.app.file_search)?;
        open_engine(config)
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
        let id: u64 = doc_id.parse().map_err(|_| "invalid doc_id".to_string())?;

        let engine = {
            let guard = engine_cell().lock().map_err(|e| e.to_string())?;
            guard.as_ref().cloned()
        };
        let engine = engine.ok_or_else(|| "search engine not initialized".to_string())?;
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

    /// build_index_config maps roots + exclusions correctly and hard-codes 50 MB.
    #[test]
    fn build_index_config_maps_fields() {
        let fs = FileSearchSettings {
            roots: vec![PathBuf::from("C:\\Users\\test")],
            exclusions: vec!["*.tmp".to_string(), "node_modules".to_string()],
            initialized: true,
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

    /// initialized field round-trips through serde correctly.
    #[test]
    fn file_search_settings_initialized_serde() {
        let fs = FileSearchSettings {
            roots: vec![],
            exclusions: vec![],
            initialized: true,
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
    }
}
