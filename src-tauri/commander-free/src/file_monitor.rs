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
// content changes, renames, or removals fire a `decoy-accessed` event +
// danger-severity OS notification + in-app toast.
//
// Threat model — early-warning signal for:
//   - Local malware grepping for "AKIA", "secret", "password" files.
//   - Person physically at the unlocked laptop browsing for
//     "interesting" files.
//   - Ransomware enumerating documents to encrypt.
//
// Detection coverage:
//   - Modify / rename / remove events via the `notify` crate (cross-
//     platform, well-tested, uses ReadDirectoryChangesW on Windows).
//   - Read/open events via opt-in Windows Security Log auditing. The audit
//     SACL is applied only to enrolled decoys.
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use notify::event::ModifyKind;
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
static READ_AUDIT_ENABLED: AtomicBool = AtomicBool::new(false);
static READ_AUDIT_TASK: Lazy<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(None));
static SEEN_AUDIT_RECORDS: Lazy<Mutex<VecDeque<String>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(128)));
static AUDITED_DECOYS: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));
/// Last-access time baseline per enrolled decoy. This is a local fallback for
/// read/open detection on NTFS volumes where last-access updates are enabled.
/// Security event 4663 remains the authoritative source for actor/process
/// details when the process has the privilege to read the Security log.
static ATIME_BASELINES: Lazy<Mutex<HashMap<PathBuf, SystemTime>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static ATIME_POLL_TASK: Lazy<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(None));
/// The Security event-record cursor is monotonic within the active log. It
/// prevents a busy host from losing a decoy read behind the old 64-event poll
/// window and avoids replaying historical records on every poll.
static LAST_AUDIT_RECORD_ID: AtomicU64 = AtomicU64::new(0);

const RECENT_CAP: usize = 10;
const DEBOUNCE_MS: u128 = 2000;
/// Statting the small enrolled set is inexpensive and gives a prompt fallback
/// alert on hosts which deliberately enable NTFS access timestamps.
const ATIME_POLL_INTERVAL_SECS: u64 = 2;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DecoyAccessEvent {
    pub path: String,
    /// "modified" | "removed" | "renamed" | "created" — uppercased on
    /// the wire to camelCase via serde rename_all in the consumer.
    pub kind: String,
    pub detected_at: String,
    /// Security Event 4663 fields are populated only for opted-in read audit.
    pub user_name: Option<String>,
    pub domain: Option<String>,
    pub sid: Option<String>,
    pub process_name: Option<String>,
    pub is_administrator: Option<bool>,
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
    let kind = match decoy_event_kind(event.kind) {
        Some(kind) => kind,
        None => return,
    };

    let watched = WATCHED_DECOYS.lock().unwrap().clone();
    for p in event.paths {
        if !watched.iter().any(|decoy| paths_match(decoy, &p)) {
            continue;
        }

        // Filesystems can emit several notifications for one real change.
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

/// ReadDirectoryChangesW can return a case variant or an extended-length
/// (`\\?\`) version of the path the user enrolled. Windows treats those as
/// the same file, so an exact `PathBuf` comparison silently lost real alerts.
fn paths_match(enrolled: &Path, observed: &Path) -> bool {
    #[cfg(windows)]
    {
        fn normalized(path: &Path) -> String {
            let rendered = path.to_string_lossy();
            rendered
                .strip_prefix(r"\\?\")
                .unwrap_or(rendered.as_ref())
                .to_ascii_lowercase()
        }
        normalized(enrolled) == normalized(observed)
    }
    #[cfg(not(windows))]
    {
        enrolled == observed
    }
}

fn decoy_event_kind(event_kind: EventKind) -> Option<&'static str> {
    match event_kind {
        // Access-time and other metadata updates commonly originate from
        // Explorer, indexing, and antivirus. They do not prove the decoy's
        // contents were changed, so never turn them into an alert.
        EventKind::Modify(ModifyKind::Data(_)) => Some("modified"),
        EventKind::Modify(ModifyKind::Name(_)) => Some("renamed"),
        EventKind::Remove(_) => Some("removed"),
        // We deliberately ignore Create — notify fires it for our own
        // initial decoy write. Subsequent Modify events are the real
        // attacker signal.
        EventKind::Create(_)
        | EventKind::Modify(_)
        | EventKind::Access(_)
        | EventKind::Any
        | EventKind::Other => None,
    }
}

fn fire_decoy_event(app: &AppHandle, path: &Path, kind: &str) {
    let path_str = path.to_string_lossy().to_string();
    let payload = DecoyAccessEvent {
        path: path_str.clone(),
        kind: kind.to_string(),
        detected_at: chrono::Utc::now().to_rfc3339(),
        user_name: None,
        domain: None,
        sid: None,
        process_name: None,
        is_administrator: None,
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

    report_tripwire_to_fleet(&payload);
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

// ── NTFS last-access fallback ──────────────────────────────────────
//
// The Windows Security log provides the best read evidence (including user and
// process), but querying it requires elevation. A standard development launch
// cannot install/read that audit path. When NTFS access-time updates are already
// enabled, metadata polling restores immediate local open/read notifications
// without broadening the watched scope or inspecting any file content.

fn capture_atime_baseline(path: &Path) {
    if let Ok(atime) = std::fs::metadata(path).and_then(|metadata| metadata.accessed()) {
        ATIME_BASELINES
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), atime);
    }
}

fn poll_atimes(app: &AppHandle) {
    let watched: Vec<PathBuf> = WATCHED_DECOYS.lock().unwrap().iter().cloned().collect();
    for path in watched {
        let Ok(atime) = std::fs::metadata(&path).and_then(|metadata| metadata.accessed()) else {
            continue;
        };
        let previous = ATIME_BASELINES.lock().unwrap().get(&path).copied();
        match previous {
            None => capture_atime_baseline(&path),
            Some(previous) if atime > previous => {
                ATIME_BASELINES.lock().unwrap().insert(path.clone(), atime);
                let mut last_fires = LAST_FIRE.lock().unwrap();
                let now = Instant::now();
                if last_fires.get(&path).is_some_and(|previous_fire| {
                    now.duration_since(*previous_fire).as_millis() < DEBOUNCE_MS
                }) {
                    continue;
                }
                last_fires.insert(path.clone(), now);
                drop(last_fires);
                // Fleet owns a closed event vocabulary; an inferred local open
                // is reported as the same read class as Security event 4663.
                fire_decoy_event(app, &path, "read");
            }
            Some(_) => {}
        }
    }
}

fn start_atime_fallback(app: AppHandle) {
    let mut task = ATIME_POLL_TASK.lock().unwrap();
    if task.is_some() {
        return;
    }
    *task = Some(tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(ATIME_POLL_INTERVAL_SECS));
        interval.tick().await;
        while RUNNING.load(Ordering::SeqCst) {
            interval.tick().await;
            poll_atimes(&app);
        }
    }));
}

/// Send the administrator-enabled Fleet alert with the exact decoy event
/// context. Optional actor/process fields are omitted when the non-elevated
/// NTFS timestamp fallback was the detector, rather than inventing an owner.
fn report_tripwire_to_fleet(event: &DecoyAccessEvent) {
    let fleet_alert_enabled = crate::settings::read_settings()
        .ok()
        .and_then(|settings| settings.ideal.privacy.decoy_monitor.fleet_alert_enabled)
        .unwrap_or(false);
    if !fleet_alert_enabled {
        return;
    }

    let event = event.clone();
    tauri::async_runtime::spawn(async move {
        let _ = crate::sidecar::dispatch_paid_command(
            "record_argus_decoy_tripwire",
            serde_json::json!({
                "kind": event.kind,
                "path": event.path,
                "userName": event.user_name,
                "domain": event.domain,
                "processName": event.process_name,
                "observedAt": event.detected_at,
            }),
        )
        .await;
    });
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

    match get_last_access_tracking_status().await {
        Ok(status) if status.enabled => start_atime_fallback(app.clone()),
        Ok(_) => crate::log_message(
            "warn",
            "[HoneypotMonitor] NTFS last-access fallback unavailable; Security Log auditing is required for read/open alerts",
        ),
        Err(error) => crate::log_message(
            "warn",
            &format!("[HoneypotMonitor] couldn't query NTFS last-access tracking: {error}"),
        ),
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

    crate::log_message("debug", "[HoneypotMonitor] watcher started");
    Ok(())
}

#[tauri::command]
pub async fn stop_decoy_monitor() -> Result<(), String> {
    RUNNING.store(false, Ordering::SeqCst);
    READ_AUDIT_ENABLED.store(false, Ordering::SeqCst);
    if let Some(task) = READ_AUDIT_TASK.lock().unwrap().take() {
        task.abort();
    }
    if let Some(task) = ATIME_POLL_TASK.lock().unwrap().take() {
        task.abort();
    }
    remove_read_audit_rules();
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
    if READ_AUDIT_ENABLED.load(Ordering::SeqCst) && pb.exists() {
        add_read_audit_rule(&pb)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_decoy(path: String) -> Result<(), String> {
    let pb = PathBuf::from(&path);
    let enrolled = WATCHED_DECOYS
        .lock()
        .unwrap()
        .iter()
        .find(|candidate| paths_match(candidate, &pb))
        .cloned();
    let Some(enrolled) = enrolled else {
        return Ok(());
    };
    WATCHED_DECOYS.lock().unwrap().remove(&enrolled);
    LAST_FIRE.lock().unwrap().remove(&enrolled);
    ATIME_BASELINES.lock().unwrap().remove(&enrolled);
    if AUDITED_DECOYS.lock().unwrap().remove(&enrolled) && enrolled.exists() {
        set_read_audit_rule(&enrolled, false)?;
    }
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

// ── Opt-in Windows Security Log read auditing ──────────────────────
//
// ReadDirectoryChangesW cannot tell a real read from harmless metadata churn.
// Windows records a trustworthy 4663 event only when both the local File
// System audit policy and a matching SACL are present. We add the SACL solely
// to enrolled decoys; no directories or user data folders are audited.

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DecoyReadAuditStatus {
    pub enabled: bool,
    pub running: bool,
}

fn run_hidden(program: &str, args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let output = Command::new(program)
        .args(args)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("run {program}: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn enable_file_system_auditing() -> Result<(), String> {
    run_hidden(
        "auditpol.exe",
        &[
            "/set",
            "/subcategory:File System",
            "/success:enable",
            "/failure:disable",
        ],
    )
    .map_err(|e| {
        if e.to_ascii_lowercase().contains("access") || e.to_ascii_lowercase().contains("privilege")
        {
            "Requires administrator approval to enable Windows File System auditing.".to_string()
        } else {
            format!("Couldn't enable Windows File System auditing: {e}")
        }
    })?;
    Ok(())
}

fn set_read_audit_rule(path: &Path, add: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let script = if add {
        "$a=Get-Acl -LiteralPath $env:WINCOMMANDER_DECOY_AUDIT_PATH;$rights=[System.Security.AccessControl.FileSystemRights]::Read;$r=[System.Security.AccessControl.FileSystemAuditRule]::new('S-1-1-0',$rights,[System.Security.AccessControl.AuditFlags]::Success);$a.AddAuditRule($r);Set-Acl -LiteralPath $env:WINCOMMANDER_DECOY_AUDIT_PATH -AclObject $a"
    } else {
        "$a=Get-Acl -LiteralPath $env:WINCOMMANDER_DECOY_AUDIT_PATH;$rights=[System.Security.AccessControl.FileSystemRights]::Read;$r=[System.Security.AccessControl.FileSystemAuditRule]::new('S-1-1-0',$rights,[System.Security.AccessControl.AuditFlags]::Success);$a.RemoveAuditRuleAll($r);Set-Acl -LiteralPath $env:WINCOMMANDER_DECOY_AUDIT_PATH -AclObject $a"
    };
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("WINCOMMANDER_DECOY_AUDIT_PATH", path)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("start audit rule update: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if error.to_ascii_lowercase().contains("access")
        || error.to_ascii_lowercase().contains("privilege")
    {
        Err(format!(
            "Requires administrator approval to audit {}.",
            path.display()
        ))
    } else {
        Err(format!(
            "Couldn't update read auditing for {}: {}",
            path.display(),
            error
        ))
    }
}

fn add_read_audit_rule(path: &Path) -> Result<(), String> {
    if AUDITED_DECOYS.lock().unwrap().contains(path) {
        return Ok(());
    }
    set_read_audit_rule(path, true)?;
    AUDITED_DECOYS.lock().unwrap().insert(path.to_path_buf());
    Ok(())
}

fn remove_read_audit_rules() {
    let paths: Vec<PathBuf> = AUDITED_DECOYS.lock().unwrap().iter().cloned().collect();
    for path in paths {
        if path.exists() {
            if let Err(error) = set_read_audit_rule(&path, false) {
                crate::log_message(
                    "warn",
                    &format!("[HoneypotMonitor] couldn't remove read audit rule: {error}"),
                );
                continue;
            }
        }
        AUDITED_DECOYS.lock().unwrap().remove(&path);
    }
}

fn add_rules_for_enrolled_decoys() -> Result<(), String> {
    for path in WATCHED_DECOYS.lock().unwrap().iter() {
        if path.exists() {
            add_read_audit_rule(path)?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct SecurityReadEvent {
    record_id: String,
    path: String,
    user_name: String,
    domain: String,
    sid: String,
    process_name: String,
    detected_at: String,
}

fn xml_field(xml: &str, field: &str) -> String {
    let pattern = format!(
        r#"<Data Name=['\"]{}['\"]>(.*?)</Data>"#,
        regex::escape(field)
    );
    regex::Regex::new(&pattern)
        .ok()
        .and_then(|re| re.captures(xml))
        .and_then(|c| c.get(1))
        .map(|m| {
            m.as_str()
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
        })
        .unwrap_or_default()
}

fn parse_security_reads(xml: &str) -> Vec<SecurityReadEvent> {
    let event_re = regex::Regex::new(r"(?s)<Event[^>]*>.*?</Event>").expect("valid event regex");
    let record_re =
        regex::Regex::new(r"<EventRecordID>([^<]+)</EventRecordID>").expect("valid record regex");
    let time_re = regex::Regex::new(r#"SystemTime=['\"]([^'\"]+)['\"]"#).expect("valid time regex");
    event_re
        .find_iter(xml)
        .filter_map(|m| {
            let event = m.as_str();
            let path = xml_field(event, "ObjectName");
            if path.is_empty() {
                return None;
            }
            Some(SecurityReadEvent {
                record_id: record_re
                    .captures(event)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())?,
                path,
                user_name: xml_field(event, "SubjectUserName"),
                domain: xml_field(event, "SubjectDomainName"),
                sid: xml_field(event, "SubjectUserSid"),
                process_name: xml_field(event, "ProcessName"),
                detected_at: time_re
                    .captures(event)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
            })
        })
        .collect()
}

fn remember_audit_record(record_id: &str) -> bool {
    let mut seen = SEEN_AUDIT_RECORDS.lock().unwrap();
    if seen.iter().any(|id| id == record_id) {
        return false;
    }
    if seen.len() == 128 {
        seen.pop_front();
    }
    seen.push_back(record_id.to_string());
    true
}

fn user_is_local_administrator(_domain: &str, _user: &str) -> Option<bool> {
    // Resolving arbitrary domain identities reliably requires directory access.
    // Don't guess: a SID or account remains useful, and this stays null unless
    // Windows exposes a local account lookup in a later native implementation.
    None
}

fn poll_security_log(app: &AppHandle) {
    let floor = LAST_AUDIT_RECORD_ID.load(Ordering::SeqCst);
    let query = format!("*[System[(EventID=4663) and (EventRecordID > {floor})]]");
    let query_arg = format!("/q:{query}");
    let xml = match run_hidden(
        "wevtutil.exe",
        &[
            "qe",
            "Security",
            query_arg.as_str(),
            "/f:xml",
            "/rd:false",
            "/c:256",
        ],
    ) {
        Ok(xml) => xml,
        Err(e) => {
            crate::log_message(
                "warn",
                &format!("[HoneypotMonitor] Security Log query failed: {e}"),
            );
            return;
        }
    };
    let watched = WATCHED_DECOYS.lock().unwrap().clone();
    let events = parse_security_reads(&xml);
    let highest_record_id = events
        .iter()
        .filter_map(|event| event.record_id.parse::<u64>().ok())
        .max()
        .unwrap_or(floor);
    for event in events {
        let matching_path = watched
            .iter()
            .find(|p| paths_match(p, Path::new(&event.path)));
        if matching_path.is_none() || !remember_audit_record(&event.record_id) {
            continue;
        }
        let is_administrator = user_is_local_administrator(&event.domain, &event.user_name);
        let payload = DecoyAccessEvent {
            path: event.path.clone(),
            kind: "read".to_string(),
            detected_at: event.detected_at,
            user_name: (!event.user_name.is_empty()).then_some(event.user_name.clone()),
            domain: (!event.domain.is_empty()).then_some(event.domain.clone()),
            sid: (!event.sid.is_empty()).then_some(event.sid),
            process_name: (!event.process_name.is_empty()).then_some(event.process_name.clone()),
            is_administrator,
        };
        let _ = app.emit("decoy-accessed", &payload);
        let actor = if event.user_name.is_empty() {
            "an unknown account".to_string()
        } else {
            format!("{}\\{}", event.domain, event.user_name)
        };
        let file_name = matching_path
            .unwrap()
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let body = format!(
            "Decoy '{}' was read by {}. Process: {}",
            file_name,
            actor,
            if event.process_name.is_empty() {
                "unknown"
            } else {
                &event.process_name
            }
        );
        let _ = crate::native_notify::show_native_notification(
            app,
            &format!("{} · ⚠ Decoy file read", crate::paths::app_display_name()),
            &body,
        );
        report_tripwire_to_fleet(&payload);
        let mut recent = RECENT_ACCESS.lock().unwrap();
        if recent.len() == RECENT_CAP {
            recent.pop_front();
        }
        recent.push_back(payload);
        drop(recent);
    }
    LAST_AUDIT_RECORD_ID.fetch_max(highest_record_id, Ordering::SeqCst);
}

fn prime_security_log_cursor() {
    if LAST_AUDIT_RECORD_ID.load(Ordering::SeqCst) != 0 {
        return;
    }
    let Ok(xml) = run_hidden(
        "wevtutil.exe",
        &[
            "qe",
            "Security",
            "/q:*[System[(EventID=4663)]",
            "/f:xml",
            "/rd:true",
            "/c:1",
        ],
    ) else {
        return;
    };
    if let Some(record_id) = parse_security_reads(&xml)
        .into_iter()
        .filter_map(|event| event.record_id.parse::<u64>().ok())
        .max()
    {
        LAST_AUDIT_RECORD_ID.store(record_id, Ordering::SeqCst);
    }
}

fn start_read_audit_listener(app: AppHandle) {
    let mut task = READ_AUDIT_TASK.lock().unwrap();
    if task.is_some() {
        return;
    }
    prime_security_log_cursor();
    *task = Some(tauri::async_runtime::spawn(async move {
        while RUNNING.load(Ordering::SeqCst) && READ_AUDIT_ENABLED.load(Ordering::SeqCst) {
            let app = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || poll_security_log(&app)).await;
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        }
    }));
}

#[tauri::command]
pub async fn set_decoy_read_audit_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<DecoyReadAuditStatus, String> {
    crate::license::require_paid("honeypot read monitoring")?;
    if !enabled {
        READ_AUDIT_ENABLED.store(false, Ordering::SeqCst);
        if let Some(task) = READ_AUDIT_TASK.lock().unwrap().take() {
            task.abort();
        }
        remove_read_audit_rules();
        return Ok(DecoyReadAuditStatus {
            enabled: false,
            running: false,
        });
    }
    // Keep the basic monitor armed even if this privileged enhancement cannot
    // be configured (for example, a `tauri dev` process running asInvoker).
    // The caller receives the error, and the log makes the otherwise-silent
    // fallback decision diagnosable on the affected machine.
    if let Err(error) = enable_file_system_auditing() {
        crate::log_message(
            "warn",
            &format!("[HoneypotMonitor] Security Log read auditing unavailable: {error}"),
        );
        return Err(error);
    }
    if let Err(error) = add_rules_for_enrolled_decoys() {
        crate::log_message(
            "warn",
            &format!("[HoneypotMonitor] couldn't apply a decoy read-audit rule: {error}"),
        );
        return Err(error);
    }
    READ_AUDIT_ENABLED.store(true, Ordering::SeqCst);
    start_read_audit_listener(app);
    crate::log_message(
        "info",
        "[HoneypotMonitor] enabled Security Log read auditing for enrolled decoys",
    );
    Ok(DecoyReadAuditStatus {
        enabled: true,
        running: true,
    })
}

#[tauri::command]
pub async fn decoy_read_audit_status() -> Result<DecoyReadAuditStatus, String> {
    Ok(DecoyReadAuditStatus {
        enabled: READ_AUDIT_ENABLED.load(Ordering::SeqCst),
        running: READ_AUDIT_TASK.lock().unwrap().is_some(),
    })
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
Wallet backup checklist
=======================
Updated: 2025-11-18

- Hardware wallet: Ledger Nano X (spare is in the fire safe)
- Recovery phrase: sealed envelope, home safe folder 4
- Exchange account: recovery codes printed with tax papers
- Purchase records: Banking/Investments/crypto-purchases.xlsx

Do not keep recovery words or private keys in this folder.
";

const DECOY_TAX_RETURN: &str = "\
2024 TAX RETURN - Filing Summary
=================================
Filed: 2025-04-12
Prepared with: TurboTax Premier

Documents retained:
- W-2 and year-end pay statements
- 1099-INT / 1099-DIV forms
- Mortgage interest statement
- Charitable donation receipts

Federal return: accepted
State return: accepted
Supporting documents: Tax/2024/Filed
";

const DECOY_CLIENT_LIST: &str = "\
Client,Contact,Owner,Renewal month,Status
Northstar Office Supply,Accounts Payable,J. Kim,March,Active
Harborlight Services,Operations,A. Patel,June,Active
Bluefern Consulting,Finance,M. Chen,September,Review
Westbridge Manufacturing,Procurement,J. Kim,November,Active
Oakwell Property Group,Facilities,A. Patel,January,Pending
";

const DECOY_PASSWORDS: &str = "\
Personal accounts - migration notes
====================================
Updated: 2025-10-03

Email, banking, shopping, and developer accounts are stored in the family
password manager. The emergency-kit envelope has the recovery instructions.

Home network: router label is on the equipment shelf
Mobile backup: verified after phone upgrade
Password-manager subscription: renews in February

Do not store passwords, recovery codes, or security answers in this file.
";

const DECOY_AWS_CREDENTIALS: &str = "\
Profile,Region,Authentication,Notes
personal,ap-south-1,AWS IAM Identity Center,Use browser sign-in
billing,ap-south-1,AWS IAM Identity Center,Monthly cost review
home-lab,ap-south-1,AWS IAM Identity Center,Least-privilege role only
";

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{DataChange, MetadataKind};

    #[test]
    fn ignores_metadata_events_that_background_services_create() {
        assert_eq!(
            decoy_event_kind(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::AccessTime
            ))),
            None
        );
    }

    #[test]
    fn accepts_only_real_content_name_and_removal_events() {
        assert_eq!(
            decoy_event_kind(EventKind::Modify(ModifyKind::Data(DataChange::Content))),
            Some("modified")
        );
        assert_eq!(
            decoy_event_kind(EventKind::Modify(ModifyKind::Name(
                notify::event::RenameMode::Any
            ))),
            Some("renamed")
        );
        assert_eq!(
            decoy_event_kind(EventKind::Remove(notify::event::RemoveKind::File)),
            Some("removed")
        );
        assert_eq!(
            decoy_event_kind(EventKind::Access(notify::event::AccessKind::Read)),
            None
        );
    }

    #[test]
    fn matches_windows_case_and_extended_path_forms() {
        let enrolled = Path::new(r"C:\Users\Alex\Desktop\Passwords.txt");
        let observed = Path::new(r"\\?\c:\users\alex\desktop\passwords.txt");
        #[cfg(windows)]
        assert!(paths_match(enrolled, observed));
        #[cfg(not(windows))]
        assert!(!paths_match(enrolled, observed));
    }

    #[test]
    fn parses_security_read_record_with_actor_and_process() {
        let xml = r#"
<Events><Event><System><EventRecordID>42</EventRecordID><TimeCreated SystemTime="2026-08-12T12:00:00Z"/></System><EventData>
<Data Name="ObjectName">C:\Users\Admin\Desktop\decoy.txt</Data><Data Name="SubjectUserName">Admin</Data><Data Name="SubjectDomainName">WORKGROUP</Data><Data Name="SubjectUserSid">S-1-5-21-1</Data><Data Name="ProcessName">C:\Windows\notepad.exe</Data>
</EventData></Event></Events>"#;

        let events = parse_security_reads(xml);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].record_id, "42");
        assert_eq!(events[0].user_name, "Admin");
        assert_eq!(events[0].process_name, r"C:\Windows\notepad.exe");
    }

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

    #[test]
    fn parses_4663_read_identity_without_file_contents() {
        let xml = r#"<Event><System><EventRecordID>41</EventRecordID><TimeCreated SystemTime='2026-08-11T08:00:00Z'/></System><EventData><Data Name='SubjectUserSid'>S-1-5-21-1</Data><Data Name='SubjectUserName'>alex</Data><Data Name='SubjectDomainName'>DESKTOP</Data><Data Name='ObjectName'>C:\Users\alex\Desktop\notes.txt</Data><Data Name='ProcessName'>C:\Windows\notepad.exe</Data></EventData></Event>"#;
        let events = parse_security_reads(xml);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].record_id, "41");
        assert_eq!(events[0].user_name, "alex");
        assert_eq!(events[0].process_name, "C:\\Windows\\notepad.exe");
    }
}
