// src-tauri/commander-free/src/services/fs_watcher.rs
//
// ═══════════════════════════════════════════════════════════════════════
// SHARED FILESYSTEM WATCHER
// ═══════════════════════════════════════════════════════════════════════
//
// Single `notify::RecommendedWatcher` per (path, recursive) tuple,
// fanned out to any number of subscribers. Replaces three nearly-
// identical per-module watcher instances that lived in:
//
//   - `decoy_monitor.rs`        (parent dirs, non-recursive, Modify/Remove)
//   - `ransomware_monitor.rs`   (user dirs, RECURSIVE, Modify only)
//   - `flow_engine.rs`           (parent dir, non-recursive, Modify/Remove/Rename)
//
// Each module continues to do its own event filtering (by path / kind /
// debounce / sliding window) — the service is intentionally dumb. Its
// job is purely to:
//
//   1. Install exactly ONE `RecommendedWatcher` per (path, recursive)
//      key, no matter how many modules subscribe to that root.
//   2. Fan out received events to all live subscribers of that root.
//   3. Uninstall the watcher when the last subscriber on that root
//      drops its handle.
//
// ── API
//
//   fn subscribe(path: PathBuf, recursive: bool) -> Result<FsWatchHandle, String>;
//
// Returns a guard with a `tokio::sync::mpsc::UnboundedReceiver` of
// `notify::Event`s. Drop the guard to unsubscribe.
//
// On Windows the underlying mechanism is `ReadDirectoryChangesW` via
// notify; on Linux it's inotify; macOS uses FSEvents. We pass-through
// the notify event verbatim so consumers have full kind information
// (rename From/To, modify-data vs. modify-metadata, etc.).
//
// ── Non-goals
//
// - This service does NOT do atime/last-access polling. That's a
//   separate concern (still lives in `decoy_monitor.rs`).
// - This service does NOT debounce. Each consumer's debounce window
//   and policy stays where it is — the per-decoy 500ms in
//   `decoy_monitor`, the sliding-window threshold in
//   `ransomware_monitor`, the rename-coalesce 500ms in `flow_engine`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use once_cell::sync::Lazy;
use tokio::sync::mpsc;

// ── Public API ──────────────────────────────────────────────────────

/// Pass through `notify::Event` directly so consumers have full access
/// to `kind` (Modify::Data vs Modify::Name vs Remove vs ...) and the
/// `paths` vector. Re-exporting saves consumers from importing notify.
pub type FsEvent = Event;

/// A live subscription to filesystem events under a single watch root.
///
/// The handle owns:
///   - `rx` — the receiver side of the event channel. Consumers loop
///     `while let Some(ev) = handle.rx.recv().await { ... }`.
///   - An internal drop guard that, when dropped, removes this
///     subscriber from the shared watcher entry. If no subscribers
///     remain on the same `(path, recursive)` root, the underlying
///     `notify::RecommendedWatcher` is dropped too (no orphan threads).
pub struct FsWatchHandle {
    pub rx: mpsc::UnboundedReceiver<FsEvent>,
    _drop_guard: SubscriberGuard,
}

/// Subscribe to filesystem events under `path`.
///
/// `recursive = true`  → watch the directory and ALL its descendants.
///   Used by ransomware-monitor (walks the whole tree under Documents
///   etc. because ransomware itself walks the tree).
/// `recursive = false` → watch only this exact path / its direct
///   children. Used by decoy-monitor and flow-engine's FileTrigger.
///
/// Returns an error if the underlying `notify` watcher can't be
/// created or if `path` can't be watched (e.g. doesn't exist, no
/// permission).
pub fn subscribe(path: PathBuf, recursive: bool) -> Result<FsWatchHandle, String> {
    let root = WatchRoot {
        path: path.clone(),
        recursive,
    };
    let (tx, rx) = mpsc::unbounded_channel::<FsEvent>();

    let mut map = WATCHERS.lock().unwrap();

    if let Some(entry) = map.get_mut(&root) {
        // Reuse the existing watcher — this root is already being
        // watched, we just add ourselves to its subscriber fanout.
        entry.subscribers.push(tx);
    } else {
        // First subscriber for this root. Install the underlying
        // notify watcher. The callback runs on notify's worker thread
        // and acquires the WATCHERS mutex to fan events out — that's
        // why we drop our own `map` guard right after insert (we don't
        // hold the lock while the watcher could fire).
        let root_for_callback = root.clone();
        let watcher_result = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let event = match res {
                Ok(e) => e,
                Err(_) => return, // notify error — best-effort, drop
            };
            let mut map = WATCHERS.lock().unwrap();
            if let Some(entry) = map.get_mut(&root_for_callback) {
                // Send to all subscribers; retain only live ones.
                // `tx.send` only fails if the receiver was dropped,
                // which happens when an FsWatchHandle's drop guard
                // runs — so the retain is a passive sweep that
                // catches subscribers in flight.
                entry.subscribers.retain(|s| s.send(event.clone()).is_ok());
            }
        });

        let mut watcher = match watcher_result {
            Ok(w) => w,
            Err(e) => return Err(format!("create watcher: {}", e)),
        };
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(e) = watcher.watch(&path, mode) {
            return Err(format!("watch {}: {}", path.display(), e));
        }

        map.insert(
            root.clone(),
            WatchEntry {
                _watcher: watcher,
                subscribers: vec![tx],
            },
        );
    }

    Ok(FsWatchHandle {
        rx,
        _drop_guard: SubscriberGuard { root },
    })
}

// ── Internal state ──────────────────────────────────────────────────

#[derive(Clone, Hash, Eq, PartialEq, Debug)]
struct WatchRoot {
    path: PathBuf,
    recursive: bool,
}

struct WatchEntry {
    /// Held to keep the notify watcher alive. Drop = stop watching.
    /// Underscored because Rust would otherwise warn about it being
    /// unused (we never call methods on it after construction).
    _watcher: RecommendedWatcher,
    subscribers: Vec<mpsc::UnboundedSender<FsEvent>>,
}

static WATCHERS: Lazy<Mutex<HashMap<WatchRoot, WatchEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ── Drop-guard cleanup ──────────────────────────────────────────────

struct SubscriberGuard {
    root: WatchRoot,
}

impl Drop for SubscriberGuard {
    fn drop(&mut self) {
        // The FsWatchHandle's `rx` field was dropped first (struct
        // field declaration order). That closes the corresponding tx
        // in the watcher's subscriber list — so our retain-on-send-ok
        // sweep will eventually drop it on the next event. But we
        // proactively prune dead senders right now so the watcher
        // tears down promptly if this was the last subscriber.
        let mut map = WATCHERS.lock().unwrap();
        if let Some(entry) = map.get_mut(&self.root) {
            entry.subscribers.retain(|tx| !tx.is_closed());
            if entry.subscribers.is_empty() {
                // Removing the entry drops `_watcher` which stops the
                // underlying notify worker thread.
                let _ = map.remove(&self.root);
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Two subscriptions to the same (path, recursive) tuple should
    /// share one entry in WATCHERS. We can't easily count notify
    /// worker threads, but we can inspect the subscribers list length —
    /// if dedup is broken there'd be two ENTRIES, not two subscribers
    /// in one entry.
    #[test]
    fn dedup_same_root_reuses_one_watcher() {
        let path = std::env::temp_dir();
        let h1 = subscribe(path.clone(), false);
        let h2 = subscribe(path.clone(), false);
        // Both should succeed (or both fail uniformly — e.g. CI env
        // without temp-dir watch permissions). If they fail, bail.
        if h1.is_err() || h2.is_err() {
            return;
        }
        let map = WATCHERS.lock().unwrap();
        let entry = map
            .get(&WatchRoot {
                path: path.clone(),
                recursive: false,
            })
            .expect("entry for tempdir should exist after subscribe");
        assert!(
            entry.subscribers.len() >= 2,
            "expected ≥2 subscribers in shared entry, got {}",
            entry.subscribers.len()
        );
    }

    /// Different (path, recursive) tuples must NOT share an entry —
    /// otherwise a recursive subscriber would mistakenly see events from
    /// the non-recursive root and vice-versa (different notify worker
    /// semantics).
    #[test]
    fn different_recursive_flag_uses_separate_entries() {
        let path = std::env::temp_dir();
        let h1 = subscribe(path.clone(), false);
        let h2 = subscribe(path.clone(), true);
        if h1.is_err() || h2.is_err() {
            return;
        }
        let map = WATCHERS.lock().unwrap();
        let non_rec = map.get(&WatchRoot {
            path: path.clone(),
            recursive: false,
        });
        let rec = map.get(&WatchRoot {
            path: path.clone(),
            recursive: true,
        });
        assert!(non_rec.is_some() && rec.is_some());
    }
}
