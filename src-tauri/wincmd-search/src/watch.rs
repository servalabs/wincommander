// SPDX-License-Identifier: AGPL-3.0-or-later
//! Filesystem watcher for wincmd-search.
//!
//! Wraps `notify` v8 to classify raw filesystem events into
//! `FsEventAction` values the index layer can act on directly.

use globset::GlobSet;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;

use crate::crawler::{doc_id_for, path_is_skipped};
use crate::error::{Result, SearchError};
use crate::types::{DocId, FileMeta};

/// Pure classifier: what should the index do with this FS event?
#[derive(Debug)]
pub enum FsEventAction {
    Upsert(FileMeta),
    Delete(DocId),
}

/// True if any component of `path` matches an exclusion glob — mirrors
/// `crawler::collect_recursive`, which skips a whole subtree the moment a
/// directory component (e.g. `node_modules`, `.git`) matches, not just the
/// final file name. Without this a save deep inside an excluded tree still
/// reaches the index writer on every watcher event.
fn path_is_excluded(path: &std::path::Path, exclusions: &GlobSet) -> bool {
    path.components()
        .any(|c| exclusions.is_match(c.as_os_str()))
}

/// Classify a single notify event into an index action (if any).
///
/// Applies the same exclusion globs the crawler uses (`exclusions`) plus
/// `skip_paths` and the size cap — otherwise churn under an excluded tree
/// (`node_modules`, `.git`, downloads-in-progress) triggers a full
/// extract+commit per event even though the crawler never indexed it.
pub fn apply_fs_event(
    event: &Event,
    skip_paths: &[PathBuf],
    exclusions: &GlobSet,
    max_file_bytes: u64,
) -> Option<FsEventAction> {
    let path = event.paths.first()?;

    if path_is_skipped(path, skip_paths) {
        return None;
    }
    if path_is_excluded(path, exclusions) {
        return None;
    }
    if path.is_dir() {
        return None;
    }

    match &event.kind {
        EventKind::Remove(_) => Some(FsEventAction::Delete(doc_id_for(path))),
        EventKind::Create(_) | EventKind::Modify(_) => {
            let meta = std::fs::metadata(path).ok()?;
            if meta.len() > max_file_bytes {
                return None;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Some(FsEventAction::Upsert(FileMeta {
                doc_id: doc_id_for(path),
                path: path.to_path_buf(),
                name,
                ext,
                mtime,
                size: meta.len(),
            }))
        }
        _ => None,
    }
}

/// Spawn a notify watcher over `roots` and forward classified actions to `tx`.
pub fn watch_roots(
    roots: &[PathBuf],
    skip_paths: Vec<PathBuf>,
    exclusions: GlobSet,
    max_file_bytes: u64,
    tx: mpsc::Sender<FsEventAction>,
) -> Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if let Some(action) = apply_fs_event(&event, &skip_paths, &exclusions, max_file_bytes) {
                let _ = tx.send(action);
            }
        }
    })
    .map_err(|e| SearchError::Io(std::io::Error::other(e.to_string())))?;

    for root in roots {
        if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
            eprintln!("wincmd-search: skipping unwatchable root {:?}: {}", root, e);
            continue;
        }
    }
    Ok(watcher)
}

// Kept behind an opt-in feature until all writers and restart recovery use
// the same guard. A full queue is never mistaken for a complete event stream.
#[cfg(feature = "experimental-space-guard")]
fn dispatch_bounded(
    result: notify::Result<Event>,
    skip_paths: &[PathBuf],
    exclusions: &GlobSet,
    max_file_bytes: u64,
    tx: &mpsc::SyncSender<FsEventAction>,
    guard: &std::sync::Mutex<crate::space_guard::SpaceGuard>,
) {
    let mut state = match guard.lock() {
        Ok(state) => state,
        Err(poisoned) => {
            poisoned.into_inner().require_reconciliation();
            return;
        }
    };
    let event = match result {
        Ok(event) if !event.need_rescan() => event,
        _ => {
            state.require_reconciliation();
            return;
        }
    };
    let Some(action) = apply_fs_event(&event, skip_paths, exclusions, max_file_bytes) else {
        // Directory changes, renames and unknown events can invalidate more
        // than one indexed file. Avoid treating them as a complete stream.
        if !matches!(event.kind, EventKind::Access(_))
            && event.paths.iter().any(|path| !path_is_skipped(path, skip_paths) && !path_is_excluded(path, exclusions))
        {
            state.require_reconciliation();
        }
        return;
    };
    state.note_change();
    if let Err(mpsc::TrySendError::Full(_)) = tx.try_send(action) {
        state.require_reconciliation();
    }
}

#[cfg(feature = "experimental-space-guard")]
pub(crate) fn watch_roots_bounded(
    roots: &[PathBuf],
    skip_paths: Vec<PathBuf>,
    exclusions: GlobSet,
    max_file_bytes: u64,
    tx: mpsc::SyncSender<FsEventAction>,
    guard: std::sync::Arc<std::sync::Mutex<crate::space_guard::SpaceGuard>>,
) -> Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |result| {
        dispatch_bounded(result, &skip_paths, &exclusions, max_file_bytes, &tx, &guard);
    }).map_err(|e| SearchError::Io(std::io::Error::other(e.to_string())))?;
    for root in roots {
        // Unlike the existing watcher, a partially registered watcher cannot
        // claim its event stream is complete.
        watcher.watch(root, RecursiveMode::Recursive)
            .map_err(|e| SearchError::Io(std::io::Error::other(e.to_string())))?;
    }
    Ok(watcher)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};
    use notify::{Event, EventKind};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_event(kind: EventKind, path: &Path) -> Event {
        Event {
            kind,
            paths: vec![path.to_path_buf()],
            attrs: Default::default(),
        }
    }

    fn no_exclusions() -> GlobSet {
        crate::crawler::build_globset(&[]).unwrap()
    }

    #[cfg(feature = "experimental-space-guard")]
    #[test]
    fn bounded_overflow_and_watcher_errors_require_reconciliation() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("document.txt");
        fs::write(&file, b"hello").unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        let guard = std::sync::Mutex::new(crate::space_guard::SpaceGuard::default());
        let event = || make_event(EventKind::Create(CreateKind::File), &file);
        dispatch_bounded(Ok(event()), &[], &no_exclusions(), u64::MAX, &tx, &guard);
        assert!(guard.lock().unwrap().reconciliation_started().is_none());
        dispatch_bounded(Ok(event()), &[], &no_exclusions(), u64::MAX, &tx, &guard);
        let token = guard.lock().unwrap().reconciliation_started().unwrap();
        assert!(rx.try_recv().is_ok());
        dispatch_bounded(Ok(event()), &[], &no_exclusions(), u64::MAX, &tx, &guard);
        assert!(!guard.lock().unwrap().reconciliation_finished(token, true, Ok(u64::MAX)));
        dispatch_bounded(Err(notify::Error::generic("watcher failure")), &[], &no_exclusions(), u64::MAX, &tx, &guard);
        assert!(guard.lock().unwrap().reconciliation_started().is_some());
    }

    // ── Create → Upsert ──────────────────────────────────────────────

    #[test]
    fn create_event_on_existing_file_yields_upsert() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new.txt");
        fs::write(&file, b"hello").unwrap();

        let ev = make_event(EventKind::Create(CreateKind::File), &file);
        let action = apply_fs_event(&ev, &[], &no_exclusions(), u64::MAX);
        assert!(matches!(action, Some(FsEventAction::Upsert(_))));
    }

    // ── Modify → Upsert ──────────────────────────────────────────────

    #[test]
    fn modify_event_yields_upsert() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("modified.txt");
        fs::write(&file, b"content").unwrap();

        let ev = make_event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            &file,
        );
        let action = apply_fs_event(&ev, &[], &no_exclusions(), u64::MAX);
        assert!(matches!(action, Some(FsEventAction::Upsert(_))));
    }

    // ── Remove → Delete ───────────────────────────────────────────────

    #[test]
    fn remove_event_yields_delete() {
        let dir = TempDir::new().unwrap();
        // File does NOT need to exist for Remove — we only hash the path.
        let file = dir.path().join("gone.txt");

        let ev = make_event(EventKind::Remove(RemoveKind::File), &file);
        let action = apply_fs_event(&ev, &[], &no_exclusions(), u64::MAX);
        assert!(matches!(action, Some(FsEventAction::Delete(_))));
    }

    // ── skip_paths → None ────────────────────────────────────────────

    #[test]
    fn skip_paths_returns_none() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("secret.txt");
        fs::write(&file, b"s").unwrap();

        let ev = make_event(EventKind::Create(CreateKind::File), &file);
        let action = apply_fs_event(&ev, &[dir.path().to_path_buf()], &no_exclusions(), u64::MAX);
        assert!(action.is_none(), "file under skip_path should return None");
    }

    // ── glob exclusions → None ────────────────────────────────────────

    #[test]
    fn excluded_dir_component_returns_none() {
        let dir = TempDir::new().unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir(&nm).unwrap();
        let file = nm.join("pkg.json");
        fs::write(&file, b"{}").unwrap();

        let exclusions = crate::crawler::build_globset(&["node_modules".into()]).unwrap();
        let ev = make_event(EventKind::Create(CreateKind::File), &file);
        let action = apply_fs_event(&ev, &[], &exclusions, u64::MAX);
        assert!(
            action.is_none(),
            "file under an excluded dir component should return None"
        );
    }

    #[test]
    fn complex_exclusions_match_crawl_and_watcher_on_nested_files() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let excluded = nested.join("secret-a.txt");
        let included = nested.join("public-a.txt");
        fs::write(&excluded, b"secret").unwrap();
        fs::write(&included, b"public").unwrap();
        let exclusions = crate::crawler::build_globset(&["{secret,private}-?.txt".into()]).unwrap();
        let files =
            crate::crawler::collect_files(&[dir.path().to_path_buf()], &exclusions, &[], u64::MAX);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, included);
        let excluded_event = make_event(EventKind::Create(CreateKind::File), &excluded);
        assert!(apply_fs_event(&excluded_event, &[], &exclusions, u64::MAX).is_none());
        let included_event = make_event(EventKind::Create(CreateKind::File), &included);
        assert!(matches!(
            apply_fs_event(&included_event, &[], &exclusions, u64::MAX),
            Some(FsEventAction::Upsert(_))
        ));
    }

    #[test]
    fn non_excluded_file_still_matches_when_exclusions_present() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("keep.rs");
        fs::write(&file, b"fn main() {}").unwrap();

        let exclusions =
            crate::crawler::build_globset(&["node_modules".into(), "*.tmp".into()]).unwrap();
        let ev = make_event(EventKind::Create(CreateKind::File), &file);
        let action = apply_fs_event(&ev, &[], &exclusions, u64::MAX);
        assert!(matches!(action, Some(FsEventAction::Upsert(_))));
    }

    // ── oversized file → None ────────────────────────────────────────

    #[test]
    fn oversized_create_returns_none() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("big.bin");
        // Write 10 bytes, cap is 5.
        fs::write(&file, b"0123456789").unwrap();

        let ev = make_event(EventKind::Create(CreateKind::File), &file);
        let action = apply_fs_event(&ev, &[], &no_exclusions(), 5);
        assert!(action.is_none(), "oversized file should return None");
    }

    // ── directory path → None ────────────────────────────────────────

    #[test]
    fn directory_path_returns_none() {
        let dir = TempDir::new().unwrap();

        // Use a Modify event pointing at the directory itself.
        let ev = make_event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            dir.path(),
        );
        let action = apply_fs_event(&ev, &[], &no_exclusions(), u64::MAX);
        assert!(action.is_none(), "directory path should return None");
    }

    // ── non-existent path on Create → None ───────────────────────────

    #[test]
    fn nonexistent_file_on_create_returns_none() {
        let dir = TempDir::new().unwrap();
        let ghost = dir.path().join("ghost.txt");
        // Do NOT create the file — metadata() will fail → None.

        let ev = make_event(EventKind::Create(CreateKind::File), &ghost);
        let action = apply_fs_event(&ev, &[], &no_exclusions(), u64::MAX);
        assert!(
            action.is_none(),
            "nonexistent file on Create should return None"
        );
    }

    // ── doc_id in Upsert matches doc_id_for ──────────────────────────

    #[test]
    fn upsert_doc_id_matches_doc_id_for() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("check.txt");
        fs::write(&file, b"data").unwrap();

        let ev = make_event(EventKind::Create(CreateKind::File), &file);
        if let Some(FsEventAction::Upsert(meta)) =
            apply_fs_event(&ev, &[], &no_exclusions(), u64::MAX)
        {
            assert_eq!(meta.doc_id, doc_id_for(&file));
        } else {
            panic!("expected Upsert");
        }
    }

    // ── Other event kinds → None ──────────────────────────────────────

    #[test]
    fn other_event_kind_returns_none() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("other.txt");
        fs::write(&file, b"x").unwrap();

        let ev = make_event(EventKind::Other, &file);
        let action = apply_fs_event(&ev, &[], &no_exclusions(), u64::MAX);
        assert!(action.is_none(), "Other event kind should return None");
    }

    // ── watch_roots: missing root is skipped, Ok still returned ──────

    #[test]
    fn watch_roots_skips_missing_root_and_returns_ok() {
        let dir = TempDir::new().unwrap();
        let valid_root = dir.path().to_path_buf();
        let missing_root = dir.path().join("does_not_exist");

        let (tx, _rx) = std::sync::mpsc::channel();
        let roots = vec![missing_root, valid_root];
        let result = watch_roots(&roots, vec![], no_exclusions(), u64::MAX, tx);
        assert!(
            result.is_ok(),
            "watch_roots should return Ok even when one root is missing"
        );
    }
}
