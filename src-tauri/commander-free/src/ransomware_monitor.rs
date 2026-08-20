// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/ransomware_monitor.rs
//
// ═══════════════════════════════════════════════════════════════════════
// ANTI-RANSOMWARE BEHAVIOURAL MONITOR (F-3)
// ═══════════════════════════════════════════════════════════════════════
//
// Watches user-content directories (Documents, Pictures, Desktop,
// Downloads) for mass-modification patterns. If more than `threshold`
// files are modified within `window_seconds`, fire a loud notification
// — this is the textbook ransomware signature (encrypt-rewrite over
// every document the malware can reach).
//
// Detection is intentionally pattern-based, not signature-based: we
// don't recognise specific ransomware families, we just count the
// rate of modify events. Catches novel families because the
// behavioural signature (50+ file rewrites in 30 seconds) is the same
// regardless of which encryptor is doing it.
//
// What we DON'T do in v1:
//   - Process attribution. The notify crate doesn't tell us which PID
//     modified a file. Without ETW or a kernel filter, we can't pin the
//     event to a specific process. v2 wires up ETW for PID-aware
//     detection + suspend-process actions.
//   - Auto-kill / auto-suspend. The toast tells the user what to do
//     (disconnect network, kill the process). Acting automatically
//     without process attribution risks killing the wrong thing.
//   - Entropy delta on writes. The roadmap mentions this but in v1
//     it'd require reading file contents before AND after each write —
//     too heavy for a monitor. Volume alone catches real ransomware;
//     entropy is v2 polish.
//
// Sliding window: VecDeque<(path, Instant)>. On every Modify event we
// drop expired entries, push the new one, and check if the deque
// length crossed the threshold. After firing, a 5-minute snooze
// prevents toast-spam from a sustained attack.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notify::{Event, EventKind};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

// ── State ───────────────────────────────────────────────────────────
//
// Post-refactor 2026-05: the per-module notify watcher has moved to
// `crate::services::fs_watcher`. We hold one consumer task per
// recursively-watched root dir; the shared service deduplicates the
// underlying `RecommendedWatcher` instance when other modules subscribe
// to the same dir (only happens if a user has e.g. a decoy file in
// their Documents directory — both modules then share one watcher).

static RUNNING: AtomicBool = AtomicBool::new(false);
/// True when the Pro ETW attribution feed (F-3 v2) started successfully
/// (sidecar up + Administrator). The notify watcher is ALWAYS the detector;
/// this flag only decides whether, when the watcher trips, we ask Pro to
/// name + neutralise the culprit. Cleared when the feed is unavailable, in
/// which case we still fire the alarm — just without attribution (v1).
static ATTRIB_READY: AtomicBool = AtomicBool::new(false);
/// Per-root consumer tasks. Aborting drops the FsWatchHandle inside,
/// which the shared service uses to decide whether the underlying
/// notify watcher comes down.
static DIR_TASKS: Lazy<Mutex<HashMap<PathBuf, tauri::async_runtime::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static APP_HANDLE: Lazy<Mutex<Option<AppHandle>>> = Lazy::new(|| Mutex::new(None));
static EVENTS: Lazy<Mutex<VecDeque<(PathBuf, Instant)>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
static LAST_FIRE: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));
static CONFIG: Lazy<Mutex<RansomwareConfig>> =
    Lazy::new(|| Mutex::new(RansomwareConfig::default()));
static RECENT_FIRES: Lazy<Mutex<VecDeque<RansomwareDetection>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
/// User-added watch directories on top of the standard set
/// (Documents/Pictures/Desktop/Downloads). Persisted in settings;
/// runtime authority is set via `set_ransomware_watch_dirs`.
static EXTRA_WATCH_DIRS: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));

const DEFAULT_ALERT_COOLDOWN_SECS: u32 = 300;
const DEFAULT_ATTRIBUTION_MIN_FILES: u32 = 5;
const RECENT_CAP: usize = 5;

// File extensions ransomware doesn't bother with + things browsers /
// editors / Windows services generate constantly. Skipping these reduces
// false-positive noise without compromising real-attack detection
// (ransomware writes new content under .docx/.pdf/.jpg/etc — covered by
// the user-doc allow-list further down).
const NOISE_EXTENSIONS: &[&str] = &[
    // Generic temp / lock / swap
    "tmp",
    "temp",
    "lock",
    "swp",
    "swo",
    "swx",
    "swn",
    "ini",
    "log",
    "cache",
    "crdownload",
    "partial",
    "lnk",
    "url",
    "etl",
    "evtx",
    "dmp",
    "blf",
    "regtrans-ms",
    "bak",
    // Office autosave / lockfiles + version trail
    "asd",
    "wbk",
    "xlk",
    "laccdb",
    "ldb",
    // OneNote / OneDrive sync helpers
    "one~",
    "onepkg-old",
    "onetoc2",
    // Hash / checksum companions (sync tools touch these in bursts)
    "md5",
    "sha1",
    "sha256",
    "crc",
    // Git pack / object writes (in case Documents holds a repo)
    "pack",
    "idx",
    "rev",
    // Browser profile cache writes that occasionally land in watched dirs
    "ldb",
];

// Files that Windows / Microsoft / sync clients write in bursts —
// touching one of these does not count toward the threshold.
const NOISE_FILENAMES: &[&str] = &[
    "desktop.ini",
    "thumbs.db",
    "ehthumbs.db",
    "ehthumbs_vista.db",
    "iconcache.db",
    "ntuser.dat",
    "ntuser.dat.log",
    "ntuser.ini",
    ".ds_store",
    "$mft",
    // OneDrive marker files
    ".849c9593-d756-4e56-8d6e-42412f2a707b",
];

// Path components that always mean "system / sync / cloud / cache
// activity" — anything happening under here is filtered out regardless of
// extension. These dirs sit under Documents/Pictures on default Windows
// installs (OneDrive, Backup, etc.) so the monitor would otherwise be
// flooded by legitimate sync bursts.
const NOISE_PATH_COMPONENTS: &[&str] = &[
    // Windows system / metadata
    "$RECYCLE.BIN",
    "System Volume Information",
    "WindowsApps",
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    // Dev / cache (carried from v1)
    ".git",
    "node_modules",
    ".vscode",
    ".idea",
    "target",
    "build",
    "dist",
    "obj",
    "bin",
    ".next",
    ".nuxt",
    ".cache",
    // Microsoft / Office app-data
    "AppData",
    "Microsoft",
    "MicrosoftEdgeBackups",
    "OneNote",
    // Cloud sync clients (their sync engines rewrite scores of files
    // when they pull large changes from the cloud)
    "OneDriveTemp",
    ".dropbox.cache",
    ".dropbox",
    ".tmp.drivedownload",
    "GoogleDriveFS",
    "iCloudDrive",
    "CloudStorage",
    // Search / index / defender
    "Search",
    "Defender",
    "WindowsDefender",
    "MpCmdRun",
];

// Extensions that real ransomware actively rewrites. We only count a
// modification toward the threshold if its extension is in this set,
// the file has *no* extension, or its extension is unknown (i.e. NOT
// in the noise list above). This whitelist is intentionally broad —
// the goal is to filter Windows/sync bursts, not to second-guess what
// an encryptor might target. If a ransomware family invents a new
// extension to attack, the file's *original* extension still falls in
// this list because that's what the malware reads from.
const USER_DOC_EXTENSIONS: &[&str] = &[
    // Office / documents
    "doc", "docx", "docm", "dot", "dotx", "rtf", "odt", "pages", "xls", "xlsx", "xlsm", "xlsb",
    "ods", "csv", "numbers", "ppt", "pptx", "pptm", "odp", "key", "pdf", "epub", "mobi", "txt",
    "md", "tex", // Images / RAW
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "raw", "cr2", "nef", "arw",
    "dng", "psd", "ai", "svg", // Audio / video
    "mp3", "wav", "flac", "aac", "ogg", "m4a", "opus", "mp4", "mov", "avi", "mkv", "wmv", "m4v",
    "webm", // Archives / installers (ransomware happily encrypts these too)
    "zip", "rar", "7z", "tar", "gz", "iso", // CAD / engineering
    "dwg", "dxf", "skp", "stp", "step", // Source bundles that often live in Documents
    "json", "xml", "yaml", "yml", "html", "css", "js", "ts", // Email / contact stores
    "pst", "ost", "mbox", "vcf", // Database / project files
    "sqlite", "db", "mdb", "accdb",
];

/// Automated response when the Pro ETW detector (v2) attributes a
/// mass-modify burst to a specific process. Only meaningful on the ETW
/// path — the notify watcher (v1) can't attribute a PID, so it always
/// behaves as `Monitor` regardless of this setting.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum RansomwareAction {
    /// Alert only (v1 behaviour, now with the offending process named).
    Monitor,
    /// Freeze the process (reversible — resumable from Task Manager).
    #[default]
    Suspend,
    /// Terminate the process outright (irreversible).
    Kill,
}

impl RansomwareAction {
    fn as_wire(&self) -> &'static str {
        match self {
            RansomwareAction::Monitor => "monitor",
            RansomwareAction::Suspend => "suspend",
            RansomwareAction::Kill => "kill",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RansomwareConfig {
    /// Number of file modifications in the rolling window before we
    /// fire. Default 50 — well above legitimate batch operations
    /// (image export ~10-20 files), well below real ransomware
    /// (typically encrypts 100s/min).
    pub threshold: u32,
    /// Length of the rolling window in seconds. Default 30.
    pub window_seconds: u32,
    /// Suppress duplicate alerts for this many seconds after a detection.
    /// The detector keeps running; only repeated notifications/actions are
    /// suppressed. Default 300, bounded 30..=3600.
    #[serde(default = "default_alert_cooldown_seconds")]
    pub alert_cooldown_seconds: u32,
    /// Minimum distinct user files one PID must modify before Pro may name or
    /// act on it. This is deliberately separate from the overall detector
    /// threshold and is always bounded to that threshold.
    #[serde(default = "default_attribution_min_files")]
    pub attribution_min_files: u32,
    /// Automated response on the ETW (v2) path. `#[serde(default)]` so a
    /// frontend that omits it (or a stored v1 config) decodes to the
    /// Suspend default rather than failing.
    #[serde(default)]
    pub action: RansomwareAction,
}

impl Default for RansomwareConfig {
    fn default() -> Self {
        Self {
            threshold: 50,
            window_seconds: 30,
            alert_cooldown_seconds: DEFAULT_ALERT_COOLDOWN_SECS,
            attribution_min_files: DEFAULT_ATTRIBUTION_MIN_FILES,
            action: RansomwareAction::Suspend,
        }
    }
}

const fn default_alert_cooldown_seconds() -> u32 {
    DEFAULT_ALERT_COOLDOWN_SECS
}

const fn default_attribution_min_files() -> u32 {
    DEFAULT_ATTRIBUTION_MIN_FILES
}

fn bounded_config(config: RansomwareConfig) -> RansomwareConfig {
    let threshold = config.threshold.clamp(10, 500);
    RansomwareConfig {
        threshold,
        window_seconds: config.window_seconds.clamp(5, 300),
        alert_cooldown_seconds: config.alert_cooldown_seconds.clamp(30, 3600),
        attribution_min_files: config.attribution_min_files.clamp(3, threshold),
        action: config.action,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RansomwareDetection {
    /// File-modify count that crossed the threshold.
    pub count: u32,
    /// Window length in seconds (echoed for the toast / log).
    pub window_seconds: u32,
    /// First five paths from the window — non-PII, just file paths
    /// the user can recognise to identify which folder is being
    /// attacked.
    pub sample_paths: Vec<String>,
    pub detected_at: String,
    // ── v2 (Pro ETW) attribution fields ──────────────────────────────
    // Populated only on the ETW path (PID-aware). The notify watcher
    // leaves them at the defaults below, so the frontend can treat a
    // zero `pid` / empty `action_taken` as "v1, no attribution".
    /// Offending process id (0 on the notify path).
    #[serde(default)]
    pub pid: u32,
    /// Offending image file name, e.g. "evil.exe" ("" on the notify path).
    #[serde(default)]
    pub image_name: String,
    /// Full image path ("" on the notify path).
    #[serde(default)]
    pub image_path: String,
    /// "none" | "suspended" | "suspend_failed" | "killed" | "kill_failed".
    /// Empty on the notify path.
    #[serde(default)]
    pub action_taken: String,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn is_noise_path(path: &std::path::Path) -> bool {
    // ── File name filters ──────────────────────────────────────────
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let name_lower = name.to_ascii_lowercase();
        // Hidden / dotfiles
        if name.starts_with('.') {
            return true;
        }
        // Office temp files (~$Document.docx) + Word recovery (~WRLnnnn.tmp)
        if name.starts_with("~$") || name.starts_with("~WRL") || name.starts_with("~wrl") {
            return true;
        }
        // Known Windows / Microsoft / sync side-files
        if NOISE_FILENAMES.contains(&name_lower.as_str()) {
            return true;
        }
    }

    // ── Path component filters ─────────────────────────────────────
    // Catch system / sync / cache dirs anywhere in the ancestry. Using
    // case-insensitive compare because Windows file paths are not case-
    // sensitive (Documents vs documents both occur) and OneDrive nests
    // under user-profile dirs with mixed case.
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            for skip in NOISE_PATH_COMPONENTS {
                if s.eq_ignore_ascii_case(skip) {
                    return true;
                }
            }
        }
    }

    // ── Extension filters ──────────────────────────────────────────
    // The user-doc allow-list IS the primary filter; the noise-extension
    // list is just an extra cheap reject for things that slipped through.
    // Files with no extension at all are kept (could be a new encrypted
    // payload), since real ransomware sometimes strips the original ext.
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if NOISE_EXTENSIONS.iter().any(|e| *e == ext_lower) {
            return true;
        }
        // If we recognise the extension and it's NOT in the doc allow-list,
        // treat it as noise. This filters Defender's quarantine writes
        // (`*.MpDef`), telemetry rollups (`*.aitx`), browser profile writes,
        // etc. — none of which a user typically cares about losing to an
        // encryptor.
        let is_user_doc = USER_DOC_EXTENSIONS.iter().any(|e| *e == ext_lower);
        if !is_user_doc {
            return true;
        }
    }
    false
}

fn handle_fs_event(app: &AppHandle, event: Event) {
    if !matches!(event.kind, EventKind::Modify(_)) {
        return;
    }

    let cfg = *CONFIG.lock().unwrap();
    let now = Instant::now();
    let window = Duration::from_secs(cfg.window_seconds as u64);

    // Snooze check — once we've fired recently, suppress further fires
    // for the configured cooldown so a sustained attack produces one
    // alert, not 100.
    if let Some(last_fire) = *LAST_FIRE.lock().unwrap() {
        if now.duration_since(last_fire).as_secs() < cfg.alert_cooldown_seconds as u64 {
            return;
        }
    }

    let mut events = EVENTS.lock().unwrap();

    // Prune expired entries
    while let Some((_, t)) = events.front() {
        if now.duration_since(*t) > window {
            events.pop_front();
        } else {
            break;
        }
    }

    // Record current event paths (filtering noise)
    for path in event.paths {
        if is_noise_path(&path) {
            continue;
        }
        events.push_back((path, now));
    }

    // Threshold check
    if events.len() < cfg.threshold as usize {
        return;
    }

    // FIRE — collect samples, clear window so we don't re-fire on next event
    let sample_paths: Vec<String> = events
        .iter()
        .rev()
        .take(5)
        .map(|(p, _)| p.to_string_lossy().to_string())
        .collect();
    let count = events.len() as u32;
    events.clear();
    drop(events);

    *LAST_FIRE.lock().unwrap() = Some(now);

    // The notify watcher is the detector; the Pro ETW feed is the
    // attributor/enforcer. Enrich + act asynchronously: ask Pro who the
    // heaviest file-modifier is and (per the action preset) suspend/kill
    // it, then emit the single alarm with the attribution merged in.
    // Best-effort — if Pro is unavailable we still fire the v1-style alarm.
    let app2 = app.clone();
    let cfg2 = cfg;
    let attrib_ready = ATTRIB_READY.load(Ordering::SeqCst);
    let detected_at = chrono::Utc::now().to_rfc3339();
    tauri::async_runtime::spawn(async move {
        let verdict = if attrib_ready {
            crate::sidecar::dispatch_paid_command(
                "attribute_ransomware",
                serde_json::json!({ "action": cfg2.action.as_wire() }),
            )
            .await
            .ok()
        } else {
            None
        };
        let (pid, image_name, image_path, action_taken) = match verdict {
            Some(v) => (
                v.get("pid").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                v.get("image_name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("image_path")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("action_taken")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
            None => (0, String::new(), String::new(), String::new()),
        };

        let detection = RansomwareDetection {
            count,
            window_seconds: cfg2.window_seconds,
            sample_paths,
            detected_at,
            pid,
            image_name: image_name.clone(),
            image_path,
            action_taken: action_taken.clone(),
        };

        {
            let mut recent = RECENT_FIRES.lock().unwrap();
            if recent.len() == RECENT_CAP {
                recent.pop_front();
            }
            recent.push_back(detection.clone());
        }

        let _ = app2.emit("ransomware-detected", &detection);

        // Name the culprit + what we did, when Pro attributed it.
        let culprit = if image_name.is_empty() {
            "the process".to_string()
        } else {
            image_name.clone()
        };
        let action_clause = match action_taken.as_str() {
            "suspended" => format!(" Suspended {} (PID {}).", culprit, pid),
            "killed" => format!(" Stopped {} (PID {}).", culprit, pid),
            "suspend_failed" | "kill_failed" => {
                format!(" Couldn't stop {} — end it in Task Manager.", culprit)
            }
            _ if pid != 0 => format!(" {} (PID {}) is the likely culprit.", culprit, pid),
            _ => " Then open Task Manager and end any process you don't recognise.".to_string(),
        };
        let title = "WinCommander · 🚨 Possible ransomware";
        let body = format!(
            "{} files were modified in {} seconds — looks like mass encryption. \
             DISCONNECT NETWORK NOW (unplug Ethernet / disable WiFi).{}",
            count, cfg2.window_seconds, action_clause
        );
        if let Err(e) = crate::native_notify::show_native_notification(&app2, title, &body) {
            crate::log_message(
                "warn",
                &format!("[RansomwareMonitor] notification failed: {}", e),
            );
        }

        crate::log_message(
            "warn",
            &format!(
                "[RansomwareMonitor] FIRE: {} files in {}s (pid={}, action={})",
                count,
                cfg2.window_seconds,
                pid,
                if action_taken.is_empty() {
                    "none"
                } else {
                    action_taken.as_str()
                }
            ),
        );
    });
}

fn standard_watch_dirs() -> Vec<PathBuf> {
    let user_profile = match std::env::var("USERPROFILE") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    ["Documents", "Pictures", "Desktop", "Downloads"]
        .iter()
        .map(|name| PathBuf::from(&user_profile).join(name))
        .filter(|p| p.exists())
        .collect()
}

/// Compute the target set of dirs to watch: standard ∪ extras, filtered
/// to those that exist on disk. Caller already holds no F-3 mutexes.
fn target_watch_dirs() -> HashSet<PathBuf> {
    let extras = EXTRA_WATCH_DIRS.lock().unwrap().clone();
    standard_watch_dirs()
        .into_iter()
        .chain(extras)
        .filter(|p| p.exists())
        .collect()
}

/// Diff DIR_TASKS keys against the target set and spawn / abort
/// per-root consumer tasks to converge. No-op if the watcher isn't
/// running yet (start() will pick up the target on its first reconcile).
fn reconcile_watch_dirs() {
    if !RUNNING.load(Ordering::SeqCst) {
        return;
    }
    let target = target_watch_dirs();

    let mut tasks = DIR_TASKS.lock().unwrap();
    let app_for_spawn = match APP_HANDLE.lock().unwrap().clone() {
        Some(a) => a,
        None => return,
    };

    // Spawn consumer tasks for any new target dirs.
    for p in &target {
        if !tasks.contains_key(p) {
            let p_for_task = p.clone();
            let app_for_task = app_for_spawn.clone();
            let task = tauri::async_runtime::spawn(async move {
                let mut handle =
                    match crate::services::fs_watcher::subscribe(p_for_task.clone(), true) {
                        Ok(h) => h,
                        Err(e) => {
                            crate::log_message(
                                "warn",
                                &format!(
                                    "[RansomwareMonitor] fs_watcher subscribe {} failed: {}",
                                    p_for_task.display(),
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
            tasks.insert(p.clone(), task);
        }
    }

    // Abort consumer tasks for dirs no longer in the target. Aborting
    // drops the per-task FsWatchHandle; the shared service uninstalls
    // the underlying notify watcher iff no other module still cares.
    let to_remove: Vec<PathBuf> = tasks
        .keys()
        .filter(|p| !target.contains(*p))
        .cloned()
        .collect();
    for p in to_remove {
        if let Some(handle) = tasks.remove(&p) {
            handle.abort();
        }
    }
}

// ── v2 attribution-feed helpers ─────────────────────────────────────
//
// F-3 v2 is one cooperative pipeline: the notify watcher below is always
// the detector; the Pro ETW feed (when ATTRIB_READY) names + neutralises
// the culprit on fire. The Tauri command surface is unchanged for the
// frontend — the commands just also keep the Pro feed started/configured.

/// Build the args the Pro ETW attribution feed expects: window + action
/// preset + the full watch-dir set (standard ∪ extras ∩ exists). Threshold
/// is sent for completeness but the feed ignores it — the watcher owns the
/// firing decision.
fn etw_args() -> serde_json::Value {
    let cfg = *CONFIG.lock().unwrap();
    let dirs: Vec<String> = target_watch_dirs()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    serde_json::json!({
        "threshold": cfg.threshold,
        "windowSeconds": cfg.window_seconds,
        "attributionMinFiles": cfg.attribution_min_files,
        "action": cfg.action.as_wire(),
        "dirs": dirs,
    })
}

/// Start the in-process notify watcher — the always-on detector.
fn start_local_watcher(app: AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return; // idempotent
    }
    *APP_HANDLE.lock().unwrap() = Some(app);
    // Spawn one recursive consumer task per target dir (Documents,
    // Pictures, Desktop, Downloads, plus user-added extras that exist).
    reconcile_watch_dirs();
    // Reset window state so a stale buffer from a previous start
    // doesn't immediately fire.
    EVENTS.lock().unwrap().clear();
    *LAST_FIRE.lock().unwrap() = None;
    let live_count = DIR_TASKS.lock().unwrap().len();
    crate::log_message(
        "info",
        &format!(
            "[RansomwareMonitor] notify watcher started ({} dirs)",
            live_count
        ),
    );
}

/// Stop the in-process notify watcher. Idempotent; safe even when the
/// watcher isn't running.
fn stop_local_watcher() {
    RUNNING.store(false, Ordering::SeqCst);
    // Abort every per-dir consumer task. Each task's FsWatchHandle
    // drops in its destructor; the shared service decides whether the
    // underlying notify watcher comes down.
    let mut tasks = DIR_TASKS.lock().unwrap();
    for (_dir, handle) in tasks.drain() {
        handle.abort();
    }
    *APP_HANDLE.lock().unwrap() = None;
    EVENTS.lock().unwrap().clear();
    *LAST_FIRE.lock().unwrap() = None;
}

// ── Tauri command surface ───────────────────────────────────────────

#[tauri::command]
pub async fn start_ransomware_monitor(app: AppHandle) -> Result<(), String> {
    crate::license::require_paid("ransomware monitor")?;

    // The notify watcher is always the detector — reliable, no admin needed,
    // catches every kind of mass-modify (including in-place overwrites).
    // Start it unconditionally.
    start_local_watcher(app);

    // Layer the Pro ETW attribution feed on top when available (sidecar up
    // + Administrator). It names + neutralises the culprit when the watcher
    // trips. If it can't start, ATTRIB_READY stays false and we simply fire
    // the alarm without attribution (v1 behaviour). The two run together as
    // one pipeline — not rival detectors, so there's still only one alarm.
    let ready = crate::sidecar::dispatch_paid_command("start_ransomware_etw", etw_args())
        .await
        .is_ok();
    ATTRIB_READY.store(ready, Ordering::SeqCst);
    crate::log_message(
        "info",
        if ready {
            "[RansomwareMonitor] notify watcher + Pro ETW attribution active"
        } else {
            "[RansomwareMonitor] notify watcher active (ETW attribution unavailable)"
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn stop_ransomware_monitor() -> Result<(), String> {
    stop_local_watcher();
    // Best-effort stop the Pro feed too so the ETW session comes down.
    if ATTRIB_READY.swap(false, Ordering::SeqCst) {
        let _ =
            crate::sidecar::dispatch_paid_command("stop_ransomware_etw", serde_json::Value::Null)
                .await;
    }
    crate::log_message("debug", "[RansomwareMonitor] watcher stopped");
    Ok(())
}

#[tauri::command]
pub async fn ransomware_monitor_status() -> Result<bool, String> {
    // The notify watcher is the detector, so its RUNNING flag is the
    // feature's status; the Pro feed is just enrichment on top.
    Ok(RUNNING.load(Ordering::SeqCst))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RansomwareMonitorHealth {
    pub detection_running: bool,
    pub process_attribution_ready: bool,
    pub automatic_response_ready: bool,
}

/// Separates the always-available mass-change alarm from the privileged Pro
/// attribution/action layer so the UI never calls a degraded monitor fully
/// operational.
#[tauri::command]
pub async fn ransomware_monitor_health() -> Result<RansomwareMonitorHealth, String> {
    let detection_running = RUNNING.load(Ordering::SeqCst);
    let process_attribution_ready = ATTRIB_READY.load(Ordering::SeqCst);
    Ok(RansomwareMonitorHealth {
        detection_running,
        process_attribution_ready,
        automatic_response_ready: detection_running && process_attribution_ready,
    })
}

#[tauri::command]
pub async fn set_ransomware_config(config: RansomwareConfig) -> Result<(), String> {
    // Hard bounds — don't let the user accidentally configure a
    // detector that never fires (huge threshold) or fires on every
    // save (1 file).
    let bounded = bounded_config(config);
    *CONFIG.lock().unwrap() = bounded;
    // Keep the Pro attribution feed's window + action preset in sync.
    if ATTRIB_READY.load(Ordering::SeqCst) {
        let _ =
            crate::sidecar::dispatch_paid_command("set_ransomware_etw_config", etw_args()).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_ransomware_config() -> Result<RansomwareConfig, String> {
    Ok(*CONFIG.lock().unwrap())
}

#[tauri::command]
pub async fn get_ransomware_recent() -> Result<Vec<RansomwareDetection>, String> {
    // The notify watcher owns the single detection ring buffer; on fire it
    // enriches each entry with Pro's attribution (pid / image / action)
    // before recording it here, so this one source already has everything.
    Ok(RECENT_FIRES.lock().unwrap().iter().cloned().collect())
}

#[tauri::command]
pub async fn clear_ransomware_recent() -> Result<(), String> {
    RECENT_FIRES.lock().unwrap().clear();
    Ok(())
}

/// Returns the directories the watcher monitors (standard ∪ extras
/// ∩ exists). Used by the UI to render "Watching:" + flag any
/// extras that disappeared off disk.
#[tauri::command]
pub async fn get_ransomware_watched_dirs() -> Result<Vec<String>, String> {
    let mut dirs: Vec<String> = target_watch_dirs()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Returns just the user-added extras (regardless of existence).
/// Used by the UI to render "your custom folders" with a remove
/// button per row.
#[tauri::command]
pub async fn get_ransomware_extra_dirs() -> Result<Vec<String>, String> {
    let mut dirs: Vec<String> = EXTRA_WATCH_DIRS
        .lock()
        .unwrap()
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Replaces the user-added extra watch directory set wholesale. The
/// frontend calls this on every settings change with the full target
/// set; we diff against the current LIVE_DIRS and reconcile via the
/// running watcher (or just store for next start if not running).
#[tauri::command]
pub async fn set_ransomware_watch_dirs(dirs: Vec<String>) -> Result<(), String> {
    let new_extras: HashSet<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    *EXTRA_WATCH_DIRS.lock().unwrap() = new_extras;
    reconcile_watch_dirs();
    // Keep the Pro attribution feed watching the same dirs (Pro treats a
    // start while already running as a live dir reconfigure — no restart).
    if ATTRIB_READY.load(Ordering::SeqCst) {
        let _ = crate::sidecar::dispatch_paid_command("start_ransomware_etw", etw_args()).await;
    }
    Ok(())
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn config_bounds_cooldown_and_attribution_floor() {
        let bounded = bounded_config(RansomwareConfig {
            threshold: 10,
            window_seconds: 1,
            alert_cooldown_seconds: 1,
            attribution_min_files: 99,
            action: RansomwareAction::Monitor,
        });
        assert_eq!(bounded.threshold, 10);
        assert_eq!(bounded.window_seconds, 5);
        assert_eq!(bounded.alert_cooldown_seconds, 30);
        assert_eq!(bounded.attribution_min_files, 10);
    }

    #[test]
    fn legacy_config_uses_safe_new_defaults() {
        let decoded: RansomwareConfig = serde_json::from_value(serde_json::json!({
            "threshold": 50,
            "windowSeconds": 30,
            "action": "suspend"
        }))
        .unwrap();
        assert_eq!(decoded.alert_cooldown_seconds, DEFAULT_ALERT_COOLDOWN_SECS);
        assert_eq!(decoded.attribution_min_files, DEFAULT_ATTRIBUTION_MIN_FILES);
    }
}
