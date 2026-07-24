// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/decoy_monitor.rs
//
// ═══════════════════════════════════════════════════════════════════════
// DECOY FILE MONITOR — filesystem honeypots (F-2)
// ═══════════════════════════════════════════════════════════════════════
//
// Watches a user-enrolled set of "decoy" files for filesystem activity.
// Decoys are plausibly-named files (passwords.txt, bitcoin-wallet.txt,
// aws-credentials.csv, etc) that the legitimate user never touches; if
// anything reads or modifies them, fire a `decoy-accessed` event +
// danger-severity OS notification + in-app toast.
//
// Threat model — early-warning signal for:
//   - Local malware grepping for "AKIA", "secret", "password" files.
//   - Person physically at the unlocked laptop browsing for
//     "interesting" files.
//   - Ransomware enumerating documents to encrypt.
//
// Detection coverage (v1):
//   - Modify / rename / remove events via the `notify` crate (cross-
//     platform, well-tested, uses ReadDirectoryChangesW on Windows).
//   - Pure read-access detection is NOT in v1. Windows requires NTFS
//     last-access tracking (often disabled by default for performance)
//     or kernel-mode ETW; a v2 ETW path captures every file open
//     regardless of last-access setting.
//
// Watch granularity: notify watches directories, not individual files.
// We watch the parent directory of every enrolled decoy (with
// RecursiveMode::NonRecursive — avoids spurious events for unrelated
// files in subfolders). On each event, we filter to paths in the
// enrolled decoy set.
//
// Privacy / safety:
//   - Decoy files are written ONCE on `drop_standard_decoys`; we never
//     touch them again (otherwise we'd fire on our own writes).
//   - Recent-events ring buffer holds path + event-kind + timestamp;
//     no file CONTENT.
//   - Removing a decoy via `remove_decoy` only stops watching it; the
//     file stays. `delete_decoy` actually deletes (separate UI button).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use notify::{Event, EventKind};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

// ── Watcher state ───────────────────────────────────────────────────
//
// Post-refactor 2026-05: the per-module notify watcher has moved to
// `crate::services::fs_watcher`. We hold one consumer task per watched
// parent directory; the shared service deduplicates the underlying
// `RecommendedWatcher` instance when other modules (flow_engine,
// ransomware_monitor) happen to subscribe to the same dir.

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Per-watched-dir consumer tasks. Each subscribes to fs_watcher and
/// forwards events into `handle_fs_event`. Aborting the task drops the
/// FsWatchHandle inside, which the shared service uses to decide
/// whether the underlying notify watcher can come down.
static DIR_TASKS: Lazy<Mutex<HashMap<PathBuf, tauri::async_runtime::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static WATCHED_DECOYS: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static WATCHED_DIRS: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static RECENT_ACCESS: Lazy<Mutex<VecDeque<DecoyAccessEvent>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(RECENT_CAP)));
/// Per-path debounce — filesystem event bursts often produce 2-5
/// duplicates per actual change (notify, atime granularity, etc).
/// Suppress same-decoy fires within 2 seconds.
static LAST_FIRE: Lazy<Mutex<HashMap<PathBuf, Instant>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Last-access time baseline per enrolled decoy. The atime polling task
/// fires "opened" when the OS-reported atime moves past the baseline.
/// Set on enroll + on first poll if missing; updated after every fire.
/// Read access detection only works when NTFS last-access tracking is
/// enabled (registry: NtfsDisableLastAccessUpdate). See
/// `get_last_access_tracking_status` / `enable_last_access_tracking`.
static ATIME_BASELINES: Lazy<Mutex<HashMap<PathBuf, SystemTime>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

const RECENT_CAP: usize = 10;
const DEBOUNCE_MS: u128 = 2000;
// 60s is plenty for honeypot read-access detection. Previous 5s polled
// MFT atime on every enrolled decoy 12× per minute, on top of the
// notify watcher already covering the parent dirs.
const ATIME_POLL_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DecoyAccessEvent {
    pub path: String,
    /// "modified" | "removed" | "renamed" | "created" — uppercased on
    /// the wire to camelCase via serde rename_all in the consumer.
    pub kind: String,
    pub detected_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DecoyInfo {
    pub path: String,
    /// True if the file currently exists on disk. False = the user
    /// either deleted it or it was never written.
    pub exists: bool,
}

// ── Event dispatch ──────────────────────────────────────────────────

fn handle_fs_event(app: &AppHandle, event: Event) {
    let kind = match event.kind {
        EventKind::Modify(_) => "modified",
        EventKind::Remove(_) => "removed",
        // We deliberately ignore Create — notify fires it for our own
        // initial decoy write. Subsequent Modify events are the real
        // attacker signal.
        EventKind::Create(_) => return,
        _ => return,
    };

    let watched = WATCHED_DECOYS.lock().unwrap().clone();
    for p in event.paths {
        if !watched.contains(&p) {
            continue;
        }

        // Per-path debounce shared with atime poller.
        {
            let mut last_fires = LAST_FIRE.lock().unwrap();
            let now = Instant::now();
            if let Some(prev) = last_fires.get(&p) {
                if now.duration_since(*prev).as_millis() < DEBOUNCE_MS {
                    continue;
                }
            }
            last_fires.insert(p.clone(), now);
        }

        fire_decoy_event(app, &p, kind);
    }
}

// ── Read-access detection via NTFS last-access polling ─────────────
//
// Windows updates `LastAccessTime` on file open if NTFS last-access
// tracking is enabled (HKLM\SYSTEM\CurrentControlSet\Control\FileSystem
// \NtfsDisableLastAccessUpdate = 0 or 2). On modern Windows the default
// is "system-managed-disabled" (3) for volumes >= 128 GB, which means
// pure-read detection won't work without the user explicitly enabling
// it via `enable_last_access_tracking`.
//
// Reading metadata via `std::fs::metadata` does NOT update atime — it
// uses GetFileAttributesEx which queries the MFT entry without
// "opening" the file. Safe to poll.
//
// NTFS atime updates can be lazy (deferred up to 1 hour by default),
// so detection latency is "anywhere from immediate to ~1 hour after
// the read", but it WILL fire eventually, which is good enough for a
// monitor: once is enough to know someone is poking around.

fn capture_atime_baseline(path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        if let Ok(atime) = metadata.accessed() {
            ATIME_BASELINES
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), atime);
        }
    }
}

fn poll_atimes(app: &AppHandle) {
    let watched: Vec<PathBuf> = WATCHED_DECOYS.lock().unwrap().iter().cloned().collect();
    for path in watched {
        let new_atime = match std::fs::metadata(&path).and_then(|m| m.accessed()) {
            Ok(t) => t,
            Err(_) => continue, // file missing or unreadable
        };

        let prev = ATIME_BASELINES.lock().unwrap().get(&path).copied();
        match prev {
            None => {
                // First observation — set baseline, don't fire.
                ATIME_BASELINES
                    .lock()
                    .unwrap()
                    .insert(path.clone(), new_atime);
            }
            Some(prev_atime) if new_atime > prev_atime => {
                // atime moved forward → someone read the file. Update
                // baseline first so we don't double-fire on a slow
                // poll round.
                ATIME_BASELINES
                    .lock()
                    .unwrap()
                    .insert(path.clone(), new_atime);

                // Per-path debounce shared with notify-event path so
                // we don't double-toast on a single read that also
                // produced a Modify/Touch event.
                {
                    let mut last_fires = LAST_FIRE.lock().unwrap();
                    let now = Instant::now();
                    if let Some(prev_fire) = last_fires.get(&path) {
                        if now.duration_since(*prev_fire).as_millis() < DEBOUNCE_MS {
                            continue;
                        }
                    }
                    last_fires.insert(path.clone(), now);
                }

                fire_decoy_event(app, &path, "opened");
            }
            _ => { /* atime unchanged */ }
        }
    }
}

fn fire_decoy_event(app: &AppHandle, path: &Path, kind: &str) {
    let path_str = path.to_string_lossy().to_string();
    let payload = DecoyAccessEvent {
        path: path_str.clone(),
        kind: kind.to_string(),
        detected_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = app.emit("decoy-accessed", &payload);

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path_str.clone());
    let title_str = format!(
        "{} · ⚠ Decoy file accessed",
        crate::paths::app_display_name()
    );
    let title = title_str.as_str();
    let body = format!(
        "Decoy '{}' was just {}. Investigate — this may be malware \
        or someone scanning for sensitive files.",
        file_name, kind
    );
    if let Err(e) = crate::native_notify::show_native_notification(app, title, &body) {
        crate::log_message(
            "warn",
            &format!("[HoneypotMonitor] notification failed: {}", e),
        );
    }

    {
        let mut recent = RECENT_ACCESS.lock().unwrap();
        if recent.len() == RECENT_CAP {
            recent.pop_front();
        }
        recent.push_back(payload);
    }

    crate::log_message(
        "warn",
        &format!("[HoneypotMonitor] {} : {}", kind, path_str),
    );
}

fn ensure_dir_watched(parent: &PathBuf) -> Result<(), String> {
    let mut dirs = WATCHED_DIRS.lock().unwrap();
    if dirs.contains(parent) {
        return Ok(());
    }
    dirs.insert(parent.clone());
    drop(dirs);

    // If the watcher is running, spawn a consumer task for this dir
    // right away. If it isn't yet, the start command will spawn one
    // for every enrolled dir.
    if RUNNING.load(Ordering::SeqCst) {
        spawn_dir_consumer(parent.clone())?;
    }
    Ok(())
}

/// Spawn the consumer task for a single watched parent dir. Subscribes
/// to fs_watcher and forwards events through `handle_fs_event`. Stores
/// the JoinHandle in DIR_TASKS so stop / remove can abort it.
fn spawn_dir_consumer(parent: PathBuf) -> Result<(), String> {
    let mut tasks = DIR_TASKS.lock().unwrap();
    if tasks.contains_key(&parent) {
        return Ok(());
    }

    let app_for_task = match APP_HANDLE.lock().unwrap().clone() {
        Some(a) => a,
        None => return Err("APP_HANDLE not set — call start_decoy_monitor first".to_string()),
    };
    let parent_for_task = parent.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut handle =
            match crate::services::fs_watcher::subscribe(parent_for_task.clone(), false) {
                Ok(h) => h,
                Err(e) => {
                    crate::log_message(
                        "warn",
                        &format!(
                            "[HoneypotMonitor] fs_watcher subscribe {} failed: {}",
                            parent_for_task.display(),
                            e
                        ),
                    );
                    return;
                }
            };
        while let Some(event) = handle.rx.recv().await {
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            handle_fs_event(&app_for_task, event);
        }
    });

    tasks.insert(parent, task);
    Ok(())
}

/// Held so `spawn_dir_consumer` can emit toasts without re-receiving
/// the app handle through every call. Set on start, cleared on stop.
static APP_HANDLE: Lazy<Mutex<Option<AppHandle>>> = Lazy::new(|| Mutex::new(None));

// ── Decoy-path accessor (for file-search indexer) ──────────────────

/// Snapshot of currently-enrolled decoy paths, for the file-search indexer to
/// skip (reading a decoy updates atime and would trip tamper detection).
pub(crate) fn enrolled_decoy_paths() -> Vec<std::path::PathBuf> {
    WATCHED_DECOYS
        .lock()
        .map(|set| set.iter().cloned().collect())
        .unwrap_or_default()
}

// ── Tauri command surface ───────────────────────────────────────────

#[tauri::command]
pub async fn start_decoy_monitor(app: AppHandle) -> Result<(), String> {
    crate::license::require_paid("honeypot monitor")?;
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(()); // idempotent
    }

    *APP_HANDLE.lock().unwrap() = Some(app.clone());

    // One-time surfacing: read-access detection via atime polling
    // silently does nothing if NTFS last-access tracking is disabled
    // (the default on modern Windows for volumes >= 128GB — see module
    // comment above). Check once per monitor start so the frontend can
    // proactively prompt the user to run `enable_last_access_tracking`
    // instead of the feature quietly under-delivering.
    match get_last_access_tracking_status().await {
        Ok(status) if atime_tracking_needs_warning(&status) => {
            let _ = app.emit("decoy-atime-tracking-disabled", &status);
            crate::log_message(
                "warn",
                "[HoneypotMonitor] NTFS last-access tracking is disabled — \
                read-access detection on decoys will not fire until the \
                user runs enable_last_access_tracking",
            );
        }
        Ok(_) => {}
        Err(e) => {
            crate::log_message(
                "warn",
                &format!(
                    "[HoneypotMonitor] could not query last-access tracking status: {}",
                    e
                ),
            );
        }
    }

    // Spawn a consumer task for every pre-enrolled directory. Settings
    // restore + this command order means decoys may already be enrolled
    // before the watcher started.
    let dirs: Vec<PathBuf> = WATCHED_DIRS.lock().unwrap().iter().cloned().collect();
    for d in dirs {
        if let Err(e) = spawn_dir_consumer(d.clone()) {
            crate::log_message(
                "warn",
                &format!(
                    "[HoneypotMonitor] spawn consumer for {} failed: {}",
                    d.display(),
                    e
                ),
            );
        }
    }

    // Spawn the atime polling task. Fires on read access if NTFS
    // last-access tracking is enabled. Cheap (just stat each enrolled
    // path every 5s).
    let app_for_poll = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(ATIME_POLL_INTERVAL_SECS));
        interval.tick().await; // discard initial immediate tick
        while RUNNING.load(Ordering::SeqCst) {
            interval.tick().await;
            poll_atimes(&app_for_poll);
        }
    });

    crate::log_message("debug", "[HoneypotMonitor] watcher started");
    Ok(())
}

#[tauri::command]
pub async fn stop_decoy_monitor() -> Result<(), String> {
    RUNNING.store(false, Ordering::SeqCst);
    // Abort every per-dir consumer task. Aborting causes each task's
    // FsWatchHandle to drop, which the shared service uses to decide
    // whether the underlying notify watcher needs to come down.
    let mut tasks = DIR_TASKS.lock().unwrap();
    for (_dir, handle) in tasks.drain() {
        handle.abort();
    }
    *APP_HANDLE.lock().unwrap() = None;
    crate::log_message("debug", "[HoneypotMonitor] watcher stopped");
    Ok(())
}

#[tauri::command]
pub async fn decoy_monitor_status() -> Result<bool, String> {
    Ok(RUNNING.load(Ordering::SeqCst))
}

#[tauri::command]
pub async fn enroll_decoy(path: String) -> Result<(), String> {
    let pb = PathBuf::from(&path);
    let parent = pb
        .parent()
        .ok_or_else(|| format!("path has no parent dir: {}", path))?
        .to_path_buf();
    if !parent.exists() {
        return Err(format!(
            "parent directory does not exist: {}",
            parent.display()
        ));
    }
    WATCHED_DECOYS.lock().unwrap().insert(pb.clone());
    capture_atime_baseline(&pb);
    ensure_dir_watched(&parent)?;
    Ok(())
}

#[tauri::command]
pub async fn remove_decoy(path: String) -> Result<(), String> {
    let pb = PathBuf::from(&path);
    WATCHED_DECOYS.lock().unwrap().remove(&pb);
    LAST_FIRE.lock().unwrap().remove(&pb);
    ATIME_BASELINES.lock().unwrap().remove(&pb);
    // Don't unwatch the parent dir — there might be other enrolled
    // decoys in it. The watcher just won't fire for non-enrolled
    // paths now.
    Ok(())
}

#[tauri::command]
pub async fn list_decoys() -> Result<Vec<DecoyInfo>, String> {
    let decoys = WATCHED_DECOYS.lock().unwrap();
    let mut result: Vec<DecoyInfo> = decoys
        .iter()
        .map(|p| DecoyInfo {
            path: p.to_string_lossy().to_string(),
            exists: p.exists(),
        })
        .collect();
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

/// Drops a small set of plausibly-named decoys into Documents + Desktop
/// and enrols them. Skips any path that already exists (don't clobber
/// the user's real files). Returns the list of paths actually created.
///
/// Decoy contents are designed to look real at a glance but explicitly
/// not match any F-1 paste-monitor patterns — so if the user later
/// copies a decoy's contents to the clipboard while triaging, F-1
/// doesn't double-fire on its own bait.
#[tauri::command]
pub async fn drop_standard_decoys() -> Result<Vec<String>, String> {
    let user_profile =
        std::env::var("USERPROFILE").map_err(|_| "USERPROFILE env var not set".to_string())?;
    let documents = PathBuf::from(&user_profile).join("Documents");
    let desktop = PathBuf::from(&user_profile).join("Desktop");

    let decoys: Vec<(PathBuf, &str)> = vec![
        (
            documents.join("bitcoin-wallet-backup.txt"),
            DECOY_BITCOIN_WALLET,
        ),
        (
            documents.join("2024-tax-return-FINAL.txt"),
            DECOY_TAX_RETURN,
        ),
        (
            documents.join("client-list-confidential.csv"),
            DECOY_CLIENT_LIST,
        ),
        (desktop.join("personal-passwords.txt"), DECOY_PASSWORDS),
        (desktop.join("aws-credentials.csv"), DECOY_AWS_CREDENTIALS),
    ];

    let mut created = Vec::new();
    for (path, content) in decoys {
        if path.exists() {
            continue;
        } // never clobber a real user file
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                continue;
            } // skip if Documents/Desktop missing
        }
        std::fs::write(&path, content).map_err(|e| format!("write {}: {}", path.display(), e))?;
        // Enroll AFTER writing so the watcher doesn't fire on our own
        // initial Create event.
        let path_str = path.to_string_lossy().to_string();
        enroll_decoy(path_str.clone()).await?;
        created.push(path_str);
    }
    Ok(created)
}

#[tauri::command]
pub async fn delete_decoy(path: String) -> Result<(), String> {
    // Confine to actually-enrolled decoys (audit H2): a compromised WebView must
    // not be able to use this command to delete arbitrary files. Compare
    // canonicalized paths so `..`/case tricks can't smuggle a non-decoy path.
    let pb = PathBuf::from(&path);
    let target = std::fs::canonicalize(&pb).unwrap_or_else(|_| pb.clone());
    let is_enrolled = WATCHED_DECOYS
        .lock()
        .unwrap()
        .iter()
        .any(|d| std::fs::canonicalize(d).unwrap_or_else(|_| d.clone()) == target);
    if !is_enrolled {
        return Err("Refused: only enrolled decoy files can be deleted here.".to_string());
    }
    remove_decoy(path.clone()).await?;
    if pb.exists() {
        std::fs::remove_file(&pb).map_err(|e| format!("delete {}: {}", pb.display(), e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_decoy_recent() -> Result<Vec<DecoyAccessEvent>, String> {
    Ok(RECENT_ACCESS.lock().unwrap().iter().cloned().collect())
}

#[tauri::command]
pub async fn clear_decoy_recent() -> Result<(), String> {
    RECENT_ACCESS.lock().unwrap().clear();
    Ok(())
}

// ── NTFS last-access tracking — read-detection prerequisite ─────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LastAccessStatus {
    /// True if NTFS is configured to update file access times. False
    /// means the atime polling path can't detect pure-read events.
    pub enabled: bool,
    /// Raw `DisableLastAccess` value (0..=3). See:
    ///   0 = User-managed, ENABLED
    ///   1 = User-managed, DISABLED
    ///   2 = System-managed, ENABLED
    ///   3 = System-managed, DISABLED  (default on most modern Windows)
    pub raw_value: u8,
    /// Whether the system chose the value (2 / 3) or the user did
    /// (0 / 1). Surfaces in the UI to explain the option.
    pub system_managed: bool,
}

/// True when atime-based read detection can't work as configured,
/// meaning the frontend should proactively prompt the user to run
/// `enable_last_access_tracking`.
fn atime_tracking_needs_warning(status: &LastAccessStatus) -> bool {
    !status.enabled
}

fn run_fsutil(args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW = 0x08000000 — keeps the fsutil window from
    // flashing on the user's screen.
    let output = Command::new("fsutil")
        .args(args)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("run fsutil: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "fsutil exited {}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Reads the NTFS last-access tracking setting via fsutil. No admin
/// required for the query — only for the change.
#[tauri::command]
pub async fn get_last_access_tracking_status() -> Result<LastAccessStatus, String> {
    let stdout = run_fsutil(&["behavior", "query", "DisableLastAccess"])?;
    // Output looks like "DisableLastAccess = 3  (System Default)"
    // or "DisableLastAccess = 0".
    let raw = stdout
        .split('=')
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|s| s.parse::<u8>().ok())
        .ok_or_else(|| format!("could not parse fsutil output: {}", stdout.trim()))?;
    Ok(LastAccessStatus {
        enabled: matches!(raw, 0 | 2),
        raw_value: raw,
        system_managed: matches!(raw, 2 | 3),
    })
}

/// Sets `DisableLastAccess = 0` so NTFS updates atime on reads.
/// Requires admin — fsutil refuses without elevation. The caller
/// surfaces a "restart as admin" hint on access-denied errors.
#[tauri::command]
pub async fn enable_last_access_tracking() -> Result<(), String> {
    run_fsutil(&["behavior", "set", "disablelastaccess", "0"]).map_err(|e| {
        if e.to_lowercase().contains("access is denied")
            || e.to_lowercase().contains("requires elevation")
        {
            "Requires admin. Restart WinCommander as administrator and try again.".to_string()
        } else {
            e
        }
    })?;
    crate::log_message(
        "info",
        "[HoneypotMonitor] enabled NTFS last-access tracking",
    );
    Ok(())
}

// ── Standard decoy file contents ────────────────────────────────────
//
// Plausible at a glance, anonymised on close inspection, and
// deliberately NOT matching any F-1 paste-monitor patterns (so
// triage-copying decoy content doesn't trigger a separate alert).

const DECOY_BITCOIN_WALLET: &str = "\
Bitcoin Wallet Recovery Notes
==============================
Created: 2024-03-15
Address: bc1q[redacted-on-paper-only]

Recovery seed (BIP-39, 12 words):
  Stored in safety deposit box at chase branch 4471.
  DO NOT type this anywhere.

Private key (xprv):
  See hardware wallet (Ledger Nano X #2).

Verification phrase: ARGON2-WALLET-3
";

const DECOY_TAX_RETURN: &str = "\
2024 TAX RETURN - FINAL DRAFT
==============================
Filed: 2024-04-12
SSN: ***-**-1234

Total Income:        $128,450
AGI:                 $124,200
Taxable Income:      $98,500
Federal Tax Owed:    $18,672
State Tax Owed:      $4,210

Refund routed to: Chase Total Checking ****6710

Dependents:
  Sarah K. (DOB redacted)
  Michael K. (DOB redacted)

Filed via TurboTax Premier 2024.
";

const DECOY_CLIENT_LIST: &str = "\
ClientName,Email,Phone,Account Manager,Annual Spend
Acme Corp,procurement@acme-redacted.com,+1-555-0100,J.Kim,$240000
Wilcorp Industries,vendors@wilcorp-redacted.com,+1-555-0188,A.Patel,$180000
Nakamura LLC,billing@nakamura-redacted.com,+1-555-0144,J.Kim,$95000
Vector Ltd,accounts@vector-redacted.io,+1-555-0177,M.Chen,$420000
Phoenix Holdings,ar@phoenix-redacted.com,+1-555-0123,A.Patel,$310000
";

const DECOY_PASSWORDS: &str = "\
Personal Logins (last updated 2024-09)
========================================
gmail:        [in 1Password vault]
icloud:       [in 1Password vault]
chase bank:   [in 1Password vault]
amazon:       [in 1Password vault]
work email:   [in 1Password vault]
github:       [in 1Password vault]

Home WiFi (5GHz):  [router label]
Router admin:      [router label]
Garage code:       [in head]

Note: Real passwords live in 1Password vault, not here.
This file is just a memory aid for what accounts I have.
";

const DECOY_AWS_CREDENTIALS: &str = "\
User Name,Access Key ID,Secret Access Key
admin,AKIA-DECOY-NOT-REAL-1,wJalrXUtnFEMI-redacted-DECOY-bPxRfiCYNOTREAL
ci-runner,AKIA-DECOY-NOT-REAL-2,X2YVHpcM5lJrVqYzCgaQB-redacted-DECOY-NR
read-only,AKIA-DECOY-NOT-REAL-3,iQT6lJZ7yK9F-redacted-DECOY-NOTREAL
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_when_system_managed_disabled() {
        // raw_value 3 — the modern-Windows default for volumes >= 128GB.
        let status = LastAccessStatus {
            enabled: false,
            raw_value: 3,
            system_managed: true,
        };
        assert!(atime_tracking_needs_warning(&status));
    }

    #[test]
    fn warns_when_user_managed_disabled() {
        let status = LastAccessStatus {
            enabled: false,
            raw_value: 1,
            system_managed: false,
        };
        assert!(atime_tracking_needs_warning(&status));
    }

    #[test]
    fn no_warning_when_system_managed_enabled() {
        let status = LastAccessStatus {
            enabled: true,
            raw_value: 2,
            system_managed: true,
        };
        assert!(!atime_tracking_needs_warning(&status));
    }

    #[test]
    fn no_warning_when_user_managed_enabled() {
        let status = LastAccessStatus {
            enabled: true,
            raw_value: 0,
            system_managed: false,
        };
        assert!(!atime_tracking_needs_warning(&status));
    }
}
