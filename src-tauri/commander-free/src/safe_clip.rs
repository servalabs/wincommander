// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/safe_clip.rs
//
// ═══════════════════════════════════════════════════════════════════════
// SAFE COPY / SAFE PASTE — context-menu clipboard + copy engine
// ═══════════════════════════════════════════════════════════════════════
//
// Safe Copy (right-click a file/folder) stages and scrubs the selection into a
// per-user cache, then records ONLY those cleaned cache paths in a small
// "safe clipboard" JSON and on the Windows file clipboard. Safe Paste
// (right-click a destination folder) copies each cached source into that
// folder KEEPING THE EXACT NAME (never renamed — a name collision is skipped,
// not suffixed), then independently re-scrubs and verifies it before commit.
// Raw input never becomes a clipboard or destination copy.
//
// Safe Copy runs headless (no window) directly off the CLI flag so it works
// whether or not the app is already running; it starts the Pro scrub sidecar
// only long enough to create the cleaned cache. Safe Paste's Explorer entry is
// headless, while the in-app command calls the same engine. Both verify the
// cache again before committing it.
//
// The pure logic (coalesce, resolve_targets) is platform-agnostic and unit-
// tested; only the named-mutex serialisation is Windows-gated.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// Explorer only recognises a file copy as a `CF_HDROP` file-list on the
// Windows clipboard. The paths supplied below are always scrubbed cache paths,
// never the user-selected originals.
#[cfg(windows)]
use clipboard_win::{formats::FileList, Clipboard, Setter};

/// Multi-select launches the verb once per item, near-simultaneously. A Safe
/// Copy within this window of the previous one APPENDS (same batch); a later
/// one REPLACES (a fresh copy). Clipboard-like otherwise: persists until the
/// next Safe Copy — Safe Paste does not consume it.
const COALESCE_WINDOW_MS: u128 = 4_000;
// Version 2 stores paths below the scrubbed cache rather than raw selections.
// Legacy records are intentionally not eligible for system-clipboard publish.
const CLIP_VERSION: u32 = 2;
const CLIP_FILE: &str = "safe-clip.json";
const CACHE_DIR: &str = "safe-clip-cache";

/// The persisted "safe clipboard".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeClip {
    pub version: u32,
    pub sources: Vec<String>,
    /// UNIX-epoch millis of the last write (drives the coalescing window).
    pub stamped_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeClipStatus {
    pub count: usize,
    pub stamped_at_ms: u128,
}

/// One source that was NOT pasted, with a human-readable reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafeSkip {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SafePasteResult {
    /// Absolute destination paths of the scrubbed copies committed to the target.
    pub copied: Vec<String>,
    pub skipped: Vec<SafeSkip>,
    /// How many sources were on the safe clipboard.
    pub source_count: usize,
}

// ── Pure helpers (platform-agnostic, unit-tested) ───────────────────────────

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Merge `new_paths` into any `existing` clip, honouring the coalescing window.
/// Within the window the paths append (dedup, order-preserving); outside it they
/// replace. Pure — `now` is injected so it's deterministic in tests.
pub fn coalesce(existing: Option<SafeClip>, new_paths: &[String], now: u128) -> SafeClip {
    let mut sources: Vec<String> = match existing {
        Some(prev) if now.saturating_sub(prev.stamped_at_ms) <= COALESCE_WINDOW_MS => prev.sources,
        _ => Vec::new(),
    };
    for p in new_paths {
        if !p.trim().is_empty() && !sources.iter().any(|s| paths_equal(s, p)) {
            sources.push(p.clone());
        }
    }
    SafeClip {
        version: CLIP_VERSION,
        sources,
        stamped_at_ms: now,
    }
}

/// Case-insensitive path comparison (Windows filesystems are case-preserving,
/// not case-sensitive), tolerant of trailing separators.
fn paths_equal(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

/// True when `child` is `ancestor` itself or lives underneath it (case-
/// insensitive, component-aware). Prevents Safe Paste from copying a folder
/// into itself or a subfolder of itself.
fn is_within(child: &Path, ancestor: &Path) -> bool {
    let a: Vec<String> = ancestor
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let c: Vec<String> = child
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    c.len() >= a.len() && a.iter().zip(c.iter()).all(|(x, y)| x == y)
}

/// Decide the copy plan: pair each existing source with `dest/<exact-name>`,
/// dropping (never renaming) any that collide, are decoys, are missing, or
/// would copy the destination into itself. `decoys` and `exists` are injected
/// for testability; `exists` reports whether a candidate target already exists.
///
/// Returns `(to_copy: (source, target)[], skipped[])`.
pub fn resolve_targets(
    sources: &[PathBuf],
    dest: &Path,
    decoys: &HashSet<PathBuf>,
    exists: &dyn Fn(&Path) -> bool,
) -> (Vec<(PathBuf, PathBuf)>, Vec<SafeSkip>) {
    let mut to_copy: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut skipped: Vec<SafeSkip> = Vec::new();
    // Names already claimed in THIS batch — a second source with the same
    // basename is skipped, never suffixed (never-rename invariant).
    let mut claimed: HashSet<String> = HashSet::new();

    for src in sources {
        let name = match src.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => {
                skipped.push(SafeSkip {
                    name: src.to_string_lossy().to_string(),
                    reason: "invalid path".into(),
                });
                continue;
            }
        };
        if decoys
            .iter()
            .any(|d| paths_equal(&d.to_string_lossy(), &src.to_string_lossy()))
        {
            skipped.push(SafeSkip {
                name,
                reason: "decoy (skipped for safety)".into(),
            });
            continue;
        }
        if !exists(src) {
            skipped.push(SafeSkip {
                name,
                reason: "source no longer exists".into(),
            });
            continue;
        }
        // Never copy the destination into itself or a subfolder of itself.
        if is_within(dest, src) || paths_equal(&src.to_string_lossy(), &dest.to_string_lossy()) {
            skipped.push(SafeSkip {
                name,
                reason: "source contains the destination".into(),
            });
            continue;
        }
        let target = dest.join(&name);
        let name_key = name.to_lowercase();
        if claimed.contains(&name_key) || exists(&target) {
            // NEVER rename — a name that already exists (on disk or claimed by an
            // earlier source this batch) is skipped.
            skipped.push(SafeSkip {
                name,
                reason: "a file with that name already exists".into(),
            });
            continue;
        }
        claimed.insert(name_key);
        to_copy.push((src.clone(), target));
    }

    (to_copy, skipped)
}

/// Recursively copy `src` → `dst`, preserving names. Files use `fs::copy`;
/// directories are created and their entries copied. `dst` must not already
/// exist (the caller's collision check guarantees this).
///
/// `skip` is consulted for EVERY node (the root and every descendant) BEFORE it
/// is stat-ed or read, so an enrolled decoy nested inside a copied folder is
/// never opened (which would bump its atime and trip the honeypot) nor
/// propagated into the scrubbed output. Returns the number of nodes skipped.
pub fn copy_tree(src: &Path, dst: &Path, skip: &dyn Fn(&Path) -> bool) -> std::io::Result<u32> {
    if skip(src) {
        return Ok(1);
    }
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        let mut skipped = 0;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            skipped += copy_tree(&child_src, &child_dst, skip)?;
        }
        Ok(skipped)
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst).map(|_| 0)
    }
}

/// List files below a staged root without asking the scrubber to recurse over
/// a directory that it could otherwise populate with `_scrubbed` output.
fn staged_files(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        for entry in std::fs::read_dir(path)? {
            staged_files(&entry?.path(), files)?;
        }
    } else {
        files.push(path.to_path_buf());
    }
    Ok(())
}

/// Reject every scrub outcome that could leave an identifying or unprocessed
/// file. Safe Paste has no "best effort" commit mode.
fn validate_scrub_report(
    report: &crate::file_metadata::ScrubReport,
    expected_files: usize,
) -> Result<(), String> {
    if let Some(error) = report.errors.first() {
        return Err(error.message.clone());
    }
    if let Some(path) = report.skipped_files.first() {
        return Err(format!("unsupported or uncleaned file: {path}"));
    }
    if report.skipped_count > 0 {
        return Err("one or more files were not scrubbed".into());
    }
    if report.residual_count > 0 {
        return Err("identifying metadata remains after scrub".into());
    }
    if report.scrubbed.len() != expected_files {
        return Err("the scrubber did not return a result for every staged file".into());
    }
    Ok(())
}

// ── State file I/O ──────────────────────────────────────────────────────────

fn clip_path() -> Result<PathBuf, String> {
    Ok(crate::paths::user_data_dir()?.join(CLIP_FILE))
}

fn read_clip() -> Option<SafeClip> {
    let path = clip_path().ok()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_clip_atomic(clip: &SafeClip) -> Result<(), String> {
    let path = clip_path()?;
    let parent = path.parent().ok_or("safe-clip path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    let bytes = serde_json::to_vec_pretty(clip).map_err(|e| format!("serialize: {e}"))?;
    let tmp = parent.join(".safe-clip.json.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write temp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("atomic rename: {e}")
    })
}

/// Mirror the scrubbed cache entries onto the Windows file clipboard. This is
/// a *copy* operation, never cut. `SafeClip` is deliberately not reconstructed
/// from this mutable system clipboard when Safe Paste runs.
///
/// Keeping this small Win32 boundary here means the headless Explorer verb and
/// the in-app command have identical clipboard behaviour.
#[cfg(windows)]
fn publish_windows_file_clipboard(paths: &[String]) -> Result<(), String> {
    let native_result: Result<(), String> = (|| {
        let clipboard = Clipboard::new_attempts(10)
            .map_err(|error| format!("could not open the clipboard: {error}"))?;
        clipboard_win::empty()
            .map_err(|error| format!("could not clear the clipboard: {error}"))?;

        let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        FileList
            .write_clipboard(&path_refs)
            .map_err(|error| format!("could not place files on the clipboard: {error}"))?;
        let drop_effect = clipboard_win::register_format("Preferred DropEffect")
            .ok_or_else(|| "could not register the Explorer copy format".to_string())?;
        clipboard_win::raw::set_without_clear(drop_effect.get(), &1_u32.to_le_bytes())
            .map_err(|error| format!("could not set the clipboard copy operation: {error}"))?;
        // Explicitly close before returning from this headless process. This
        // matters for Explorer verbs, which have no Tauri window/message loop.
        drop(clipboard);
        let readback: Vec<String> = clipboard_win::get_clipboard(FileList)
            .map_err(|error| format!("could not read back the file clipboard: {error}"))?;
        if readback.len() != paths.len()
            || !readback
                .iter()
                .zip(paths)
                .all(|(actual, expected)| paths_equal(actual, expected))
        {
            return Err("file clipboard readback did not match the cleaned selection".into());
        }
        Ok(())
    })();

    // `FileList::write_clipboard` uses SetClipboardData(CF_HDROP) with a
    // movable HGLOBAL, transferring ownership to Windows. That data remains
    // available after this short-lived Explorer process exits. Do not replace
    // it with an OLE/WPF clipboard owner: if that helper exits before Explorer
    // requests delayed-rendered data, the file list disappears from clipboard.
    native_result
}

#[cfg(not(windows))]
fn publish_windows_file_clipboard(_paths: &[String]) -> Result<(), String> {
    // Safe Copy is an Explorer integration. Keeping this a no-op allows the
    // pure copy-plan tests to run on non-Windows builders.
    Ok(())
}

fn cache_root() -> Result<PathBuf, String> {
    let root = crate::paths::user_data_dir()?.join(CACHE_DIR);
    std::fs::create_dir_all(&root).map_err(|error| format!("create Safe Copy cache: {error}"))?;
    Ok(root)
}

/// Return the batch directory only when every source is a direct, expected
/// cache leaf. This prevents a legacy/tampered JSON record from ever becoming
/// a `CF_HDROP` list of arbitrary original files.
fn cache_batch_for_clip(clip: &SafeClip, root: &Path) -> Option<PathBuf> {
    if clip.version != CLIP_VERSION || clip.sources.is_empty() {
        return None;
    }
    let mut batch: Option<PathBuf> = None;
    for source in &clip.sources {
        let source = PathBuf::from(source);
        let relative = source.strip_prefix(root).ok()?;
        let mut components = relative.components();
        let batch_name = components.next()?.as_os_str();
        let item_id = components.next()?.as_os_str();
        let file_name = components.next()?.as_os_str();
        // Cache layout is `<root>/<batch UUID>/<item UUID>/<original name>`.
        if components.next().is_some()
            || batch_name.is_empty()
            || item_id.is_empty()
            || file_name.is_empty()
        {
            return None;
        }
        let candidate = root.join(batch_name);
        if !source.is_file() && !source.is_dir() {
            return None;
        }
        match &batch {
            Some(current) if current != &candidate => return None,
            None => batch = Some(candidate),
            _ => {}
        }
    }
    batch
}

fn remove_old_cache_batches(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != keep && path.is_dir() {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                crate::log_message(
                    "warn",
                    &format!(
                        "[SafeCopy] could not remove expired scrub cache {}: {error}",
                        path.display()
                    ),
                );
            }
        }
    }
}

/// Copy one raw source into an unpublished staging directory, scrub every
/// file, validate the report, and atomically promote only the cleaned tree
/// into the cache batch. Its returned path is safe for CF_HDROP.
async fn cache_scrubbed_source(
    source: &Path,
    batch: &Path,
    decoys: &HashSet<PathBuf>,
) -> Result<PathBuf, String> {
    let name = source
        .file_name()
        .ok_or_else(|| "Safe Copy source has no file name".to_string())?;
    if !source.exists() {
        return Err(format!(
            "Safe Copy source no longer exists: {}",
            source.display()
        ));
    }

    let staging = tempfile::Builder::new()
        .prefix(".wincommander-safe-copy-")
        .tempdir_in(batch)
        .map_err(|error| format!("Safe Copy could not create a scrub staging area: {error}"))?;
    let staged = staging.path().join(name);
    let decoy_skip = |path: &Path| {
        decoys
            .iter()
            .any(|decoy| paths_equal(&decoy.to_string_lossy(), &path.to_string_lossy()))
    };
    let skipped_decoys = copy_tree(source, &staged, &decoy_skip)
        .map_err(|error| format!("Safe Copy could not stage {}: {error}", source.display()))?;
    if skipped_decoys > 0 {
        return Err("Safe Copy refused a protected decoy inside the selection".into());
    }

    let mut files = Vec::new();
    staged_files(&staged, &mut files)
        .map_err(|error| format!("Safe Copy could not inspect its staged selection: {error}"))?;
    if !files.is_empty() {
        let report = crate::file_metadata::scrub_metadata_paths_headless(
            files
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            Some(crate::file_metadata::ScrubOptions {
                output_dir: None,
                dry_run: false,
                recursive: false,
                paranoid: crate::file_metadata::ParanoidOptions::default(),
                replace_originals: true,
            }),
        )
        .await?;
        validate_scrub_report(&report, files.len()).map_err(|detail| {
            format!("Safe Copy scrub failed; originals were not placed on the clipboard: {detail}")
        })?;
    }

    let item_dir = batch.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&item_dir)
        .map_err(|error| format!("Safe Copy could prepare its scrub cache: {error}"))?;
    let cached = item_dir.join(name);
    std::fs::rename(&staged, &cached)
        .map_err(|error| format!("Safe Copy could commit its scrubbed cache: {error}"))?;
    Ok(cached)
}

/// Scrub `paths` into the Safe Copy cache and record those cache paths. The
/// named mutex serialises Explorer's one-launch-per-selected-item behaviour,
/// so a multi-select stays one batch without ever racing raw files onto the
/// system clipboard.
pub async fn record_sources(paths: &[String]) -> Result<usize, String> {
    let decoys: HashSet<PathBuf> = crate::file_monitor::enrolled_decoy_paths()
        .into_iter()
        .collect();
    let filtered: Vec<String> = paths
        .iter()
        .filter(|p| !p.trim().is_empty())
        .filter(|p| {
            let pb = PathBuf::from(p.as_str());
            !decoys
                .iter()
                .any(|d| paths_equal(&d.to_string_lossy(), &pb.to_string_lossy()))
        })
        .cloned()
        .collect();
    // Authorize before `cache_scrubbed_source` makes even a transient raw
    // staging copy. The scrub dispatcher also gates defensively, but that
    // happens after its caller has prepared inputs.
    if !filtered.is_empty() {
        crate::license::require_paid("Safe Copy")?;
    }
    let _guard = ClipLock::acquire();
    let root = cache_root()?;
    let existing = read_clip();
    if filtered.is_empty() {
        return existing
            .as_ref()
            .filter(|clip| cache_batch_for_clip(clip, &root).is_some())
            .map(|clip| clip.sources.len())
            .ok_or_else(|| "Safe Copy requires at least one non-protected item".to_string());
    }

    let now = now_ms();
    let append_to_existing = existing.as_ref().is_some_and(|clip| {
        cache_batch_for_clip(clip, &root).is_some()
            && now.saturating_sub(clip.stamped_at_ms) <= COALESCE_WINDOW_MS
    });
    let batch = if append_to_existing {
        cache_batch_for_clip(existing.as_ref().expect("checked above"), &root)
            .expect("checked above")
    } else {
        root.join(uuid::Uuid::new_v4().to_string())
    };
    std::fs::create_dir_all(&batch)
        .map_err(|error| format!("Safe Copy could create its scrub cache: {error}"))?;

    // The licence and scrub complete before `write_clip_atomic` or CF_HDROP.
    // Thus an error leaves the prior clean clipboard selection intact.
    let mut cached_sources = Vec::with_capacity(filtered.len());
    for raw in &filtered {
        cached_sources.push(cache_scrubbed_source(Path::new(raw), &batch, &decoys).await?);
    }
    let cached_strings: Vec<String> = cached_sources
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();
    let merged = coalesce(
        if append_to_existing { existing } else { None },
        &cached_strings,
        now,
    );
    let count = merged.sources.len();
    write_clip_atomic(&merged)?;
    // `merged.sources` passed validation in `cache_batch_for_clip` above and
    // consists only of scrubbed cache copies, never the original selection.
    publish_windows_file_clipboard(&merged.sources)?;
    if !append_to_existing {
        remove_old_cache_batches(&root, &batch);
    }
    Ok(count)
}

/// Headless CLI entry for `--safe-copy <path> [<path>…]`. Extracts non-flag
/// args, records them, and never touches a window. Called from `run()` BEFORE
/// the single-instance guard so it always acts locally.
pub fn handle_safe_copy_cli(args: &[String]) {
    let paths: Vec<String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .collect();
    let result = run_headless_with_pro_shutdown("Safe Copy", record_sources(&paths));
    match result {
        Ok(n) => crate::log_message("info", &format!("[SafeCopy] recorded {n} item(s)")),
        Err(e) => crate::log_message("warn", &format!("[SafeCopy] record failed: {e}")),
    }
}

/// Headless CLI entry for `--safe-paste <destination>`. Explorer context-menu
/// paste must not wait for the Tauri window, React sidebar, or a queued UI
/// event to become ready. It uses the same scrubbed cache and commit path as
/// the in-app command, then exits when the operation has a definitive result.
pub fn handle_safe_paste_cli(args: &[String]) -> i32 {
    let dest = args
        .iter()
        .skip(1)
        .filter(|arg| !arg.starts_with("--"))
        .next()
        .cloned();
    let Some(dest) = dest else {
        let error = "Safe Paste did not receive a destination folder. Refresh the context-menu integration and try again.";
        crate::log_message("warn", &format!("[SafePaste] failed: {error}"));
        show_safe_paste_error(error);
        return 1;
    };

    crate::log_message("info", "[SafePaste] headless operation started");
    let result = run_headless_with_pro_shutdown("Safe Paste", safe_paste_prepare_headless(dest));
    match result {
        Ok(result) if !result.copied.is_empty() => {
            crate::log_message(
                "info",
                &format!(
                    "[SafePaste] completed: copied {} item(s), skipped {}",
                    result.copied.len(),
                    result.skipped.len()
                ),
            );
            0
        }
        Ok(result) => {
            let reason = result
                .skipped
                .first()
                .map(|skip| format!("{} ({})", skip.name, skip.reason))
                .unwrap_or_else(|| "Nothing is ready to paste. Use Safe Copy first.".into());
            let message = format!("Safe Paste did not add any files. {reason}");
            crate::log_message("warn", &format!("[SafePaste] failed: {message}"));
            show_safe_paste_error(&message);
            1
        }
        Err(error) => {
            crate::log_message("warn", &format!("[SafePaste] failed: {error}"));
            show_safe_paste_error(&error);
            1
        }
    }
}

/// Run a one-shot Explorer verb on its own current-thread runtime, then close
/// any Pro sidecar it opened before the runtime and process are torn down.
/// The long-lived GUI pools workers for reuse; a headless copy/paste process
/// has no later request, so returning the worker to that pool can leave both
/// helper processes alive after the files have already been committed.
fn run_headless_with_pro_shutdown<T, F>(operation: &str, task: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("{operation} could start its scrub runtime: {error}"))?;

    runtime.block_on(async move {
        let result = task.await;
        crate::sidecar::close_pro_session().await;
        result
    })
}

fn show_safe_paste_error(message: &str) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let text: Vec<u16> = format!("Safe Paste could not complete.\r\n\r\n{message}\0")
            .encode_utf16()
            .collect();
        let title: Vec<u16> = "WinCommander Safe Paste\0".encode_utf16().collect();
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
    #[cfg(not(windows))]
    let _ = message;
}

// ── Tauri commands ──────────────────────────────────────────────────────────

/// Record a selection onto the safe clipboard from inside the app (parity with
/// the headless CLI path; handy for an in-app "Safe Copy" action and tests).
#[tauri::command]
pub async fn safe_copy_record(paths: Vec<String>) -> Result<usize, String> {
    record_sources(&paths).await
}

/// Current safe-clipboard status (for a "N items ready to Safe Paste" hint).
#[tauri::command]
pub async fn safe_clip_status() -> Result<SafeClipStatus, String> {
    let cache = cache_root()?;
    Ok(read_clip()
        .filter(|clip| cache_batch_for_clip(clip, &cache).is_some())
        .map(|c| SafeClipStatus {
            count: c.sources.len(),
            stamped_at_ms: c.stamped_at_ms,
        })
        .unwrap_or(SafeClipStatus {
            count: 0,
            stamped_at_ms: 0,
        }))
}

/// Stage every recorded source beside `dest_dir`, scrub and validate the staged
/// files, then commit only clean copies into `dest_dir`. This is one backend
/// operation so an Explorer launch cannot depend on a frontend event listener
/// being mounted, and a scrub failure cannot leave raw content in the target.
/// PAID: the value is the metadata scrub, so the gate runs BEFORE any file
/// operation — a free user gets a clean upsell with zero files touched.
#[tauri::command]
pub async fn safe_paste_prepare(
    _app: tauri::AppHandle,
    dest_dir: String,
) -> Result<SafePasteResult, String> {
    safe_paste_prepare_headless(dest_dir).await
}

/// Application-independent Safe Paste operation shared by Explorer's
/// headless verb and the Tauri command. All staged copies are scrubbed and
/// validated before any file is committed to the requested destination.
pub async fn safe_paste_prepare_headless(dest_dir: String) -> Result<SafePasteResult, String> {
    crate::license::require_paid("Safe Paste")?;

    let dest = PathBuf::from(&dest_dir);
    if !dest.is_dir() {
        return Err("Safe Paste destination is not a folder".into());
    }
    let clip = read_clip().unwrap_or(SafeClip {
        version: CLIP_VERSION,
        sources: vec![],
        stamped_at_ms: 0,
    });
    let source_count = clip.sources.len();
    if source_count == 0 {
        return Ok(SafePasteResult {
            copied: vec![],
            skipped: vec![],
            source_count: 0,
        });
    }
    // Version-1 clips stored raw selection paths. Refuse them rather than
    // making an old Safe Copy silently bypass the scrubbed-cache invariant.
    let cache = cache_root()?;
    if cache_batch_for_clip(&clip, &cache).is_none() {
        return Err("This Safe Copy selection predates the scrubbed clipboard cache. Use Safe Copy again before pasting.".into());
    }
    let sources: Vec<PathBuf> = clip.sources.iter().map(PathBuf::from).collect();
    let decoys: HashSet<PathBuf> = crate::file_monitor::enrolled_decoy_paths()
        .into_iter()
        .collect();

    let (to_copy, mut skipped) = resolve_targets(&sources, &dest, &decoys, &|p: &Path| p.exists());

    // Enrolled decoys nested INSIDE a copied folder must never be opened or
    // propagated — resolve_targets only vets the top-level sources.
    let decoy_skip = |p: &Path| {
        decoys
            .iter()
            .any(|d| paths_equal(&d.to_string_lossy(), &p.to_string_lossy()))
    };

    // Staging beside the destination keeps the final commit a same-volume
    // rename rather than a second, raw cross-volume copy.
    let staging = tempfile::Builder::new()
        .prefix(".wincommander-safe-paste-")
        .tempdir_in(&dest)
        .map_err(|e| format!("Safe Paste couldn't create its staging area: {e}"))?;

    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut nested_decoys_skipped = 0u32;
    for (src, target) in to_copy {
        let stage_target = staging.path().join(
            target
                .file_name()
                .expect("resolve_targets always produces a filename"),
        );
        match copy_tree(&src, &stage_target, &decoy_skip) {
            Ok(n) => {
                nested_decoys_skipped += n;
                staged.push((stage_target, target));
            }
            Err(e) => {
                // Nothing has reached the requested folder yet.
                let _ = if stage_target.is_dir() {
                    std::fs::remove_dir_all(&stage_target)
                } else {
                    std::fs::remove_file(&stage_target)
                };
                skipped.push(SafeSkip {
                    name: src
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    reason: format!("copy failed: {e}"),
                });
            }
        }
    }
    if nested_decoys_skipped > 0 {
        skipped.push(SafeSkip {
            name: format!("{nested_decoys_skipped} protected item(s)"),
            reason: "decoy (skipped for safety)".into(),
        });
    }

    let mut files_to_scrub = Vec::new();
    for (stage_target, target) in &staged {
        staged_files(stage_target, &mut files_to_scrub).map_err(|e| {
            format!(
                "Safe Paste couldn't inspect staged {}: {e}",
                target.to_string_lossy()
            )
        })?;
    }

    if !files_to_scrub.is_empty() {
        let report = crate::file_metadata::scrub_metadata_paths_headless(
            files_to_scrub
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            Some(crate::file_metadata::ScrubOptions {
                output_dir: None,
                dry_run: false,
                recursive: false,
                paranoid: crate::file_metadata::ParanoidOptions::default(),
                replace_originals: true,
            }),
        )
        .await?;

        // A skipped, failed, or residual-bearing staged file is not safe to
        // publish. TempDir removes the complete staging area on return.
        validate_scrub_report(&report, files_to_scrub.len())
            .map_err(|detail| format!("Safe Paste scrub failed; no files were pasted: {detail}"))?;
    }

    // A folder can change while the scrubber runs. Re-check collisions before
    // the commit so Safe Paste never overwrites a newly-created target.
    let mut copied: Vec<String> = Vec::new();
    for (stage_target, target) in staged {
        if target.exists() {
            skipped.push(SafeSkip {
                name: target
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
                reason: "a file with that name already exists".into(),
            });
            continue;
        }
        std::fs::rename(&stage_target, &target).map_err(|e| {
            format!(
                "Safe Paste couldn't commit cleaned {}: {e}",
                target.to_string_lossy()
            )
        })?;
        copied.push(target.to_string_lossy().to_string());
    }

    crate::log_message(
        "info",
        &format!(
            "[SafePaste] scrubbed and committed {} item(s), skipped {}",
            copied.len(),
            skipped.len()
        ),
    );
    Ok(SafePasteResult {
        copied,
        skipped,
        source_count,
    })
}

// ── Cross-process serialisation ─────────────────────────────────────────────

/// RAII guard around a named mutex so concurrent Safe Copy launches (one per
/// selected item) serialise their read-modify-write of the clip file. On
/// non-Windows it is a no-op (Safe Copy is a Windows Explorer feature).
struct ClipLock {
    #[cfg(windows)]
    handle: isize,
}

impl ClipLock {
    #[cfg(windows)]
    fn acquire() -> Self {
        use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
        let name: Vec<u16> = "WinCommander_SafeClip_lock\0".encode_utf16().collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if !handle.is_null() {
            // Bounded wait so a crashed holder can't hang a headless launch
            // forever; 5 s is far beyond a legitimate read-modify-write.
            unsafe { WaitForSingleObject(handle, 5_000) };
        } else {
            // True OS failure (handle exhaustion). We proceed unlocked rather
            // than block the copy, but log it so the (rare) fail-open — under
            // which a concurrent multi-select could drop an entry — is visible.
            crate::log_message(
                "warn",
                "[SafeCopy] clip mutex unavailable; recording without cross-process lock",
            );
        }
        ClipLock {
            handle: handle as isize,
        }
    }

    #[cfg(not(windows))]
    fn acquire() -> Self {
        ClipLock {}
    }
}

impl Drop for ClipLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            if self.handle != 0 {
                unsafe {
                    ReleaseMutex(self.handle as _);
                    CloseHandle(self.handle as _);
                }
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn coalesce_replaces_outside_window() {
        let prev = SafeClip {
            version: 1,
            sources: s(&["a"]),
            stamped_at_ms: 0,
        };
        // now far past the window → replace.
        let out = coalesce(Some(prev), &s(&["b"]), COALESCE_WINDOW_MS + 1);
        assert_eq!(out.sources, s(&["b"]));
    }

    #[test]
    fn coalesce_appends_within_window() {
        let prev = SafeClip {
            version: 1,
            sources: s(&["a"]),
            stamped_at_ms: 100,
        };
        let out = coalesce(Some(prev), &s(&["b", "c"]), 100 + COALESCE_WINDOW_MS);
        assert_eq!(out.sources, s(&["a", "b", "c"]));
    }

    #[test]
    fn coalesce_dedups_case_insensitively() {
        let prev = SafeClip {
            version: 1,
            sources: s(&["C:\\A\\x.txt"]),
            stamped_at_ms: 10,
        };
        let out = coalesce(Some(prev), &s(&["c:\\a\\x.txt"]), 20);
        assert_eq!(
            out.sources.len(),
            1,
            "same path different case must not duplicate"
        );
    }

    #[test]
    fn coalesce_fresh_when_no_existing() {
        let out = coalesce(None, &s(&["a"]), 999);
        assert_eq!(out.sources, s(&["a"]));
        assert_eq!(out.stamped_at_ms, 999);
    }

    #[test]
    fn only_current_scrub_cache_paths_are_clipboard_eligible() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join(CACHE_DIR);
        let cached = root.join("batch").join("item").join("clean.pdf");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"clean").unwrap();

        let clean = SafeClip {
            version: CLIP_VERSION,
            sources: vec![cached.to_string_lossy().to_string()],
            stamped_at_ms: 1,
        };
        assert_eq!(
            cache_batch_for_clip(&clean, &root),
            Some(root.join("batch"))
        );

        let legacy_raw = SafeClip {
            version: 1,
            sources: vec![r"C:\Users\Example\original.pdf".into()],
            stamped_at_ms: 1,
        };
        assert!(cache_batch_for_clip(&legacy_raw, &root).is_none());

        let outside_cache = SafeClip {
            version: CLIP_VERSION,
            sources: vec![temp
                .path()
                .join("original.pdf")
                .to_string_lossy()
                .to_string()],
            stamped_at_ms: 1,
        };
        assert!(cache_batch_for_clip(&outside_cache, &root).is_none());
    }

    #[test]
    fn resolve_never_renames_on_collision() {
        let sources = vec![PathBuf::from("C:\\src\\report.pdf")];
        let dest = PathBuf::from("C:\\dst");
        let decoys = HashSet::new();
        // The target already exists → must be SKIPPED, never suffixed.
        let exists = |p: &Path| {
            p == Path::new("C:\\dst\\report.pdf") || p == Path::new("C:\\src\\report.pdf")
        };
        let (to_copy, skipped) = resolve_targets(&sources, &dest, &decoys, &exists);
        assert!(to_copy.is_empty(), "colliding name must not be copied");
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].reason.contains("already exists"));
    }

    #[test]
    fn resolve_preserves_exact_name() {
        let sources = vec![PathBuf::from("C:\\src\\My File 2982.jpg")];
        let dest = PathBuf::from("C:\\dst");
        let decoys = HashSet::new();
        let exists = |p: &Path| p == Path::new("C:\\src\\My File 2982.jpg"); // source exists, target doesn't
        let (to_copy, skipped) = resolve_targets(&sources, &dest, &decoys, &exists);
        assert!(skipped.is_empty());
        assert_eq!(to_copy.len(), 1);
        assert_eq!(to_copy[0].1, PathBuf::from("C:\\dst\\My File 2982.jpg"));
    }

    #[test]
    fn resolve_duplicate_basename_second_is_skipped() {
        // Two different sources with the SAME basename → only the first copies.
        let sources = vec![
            PathBuf::from("C:\\a\\doc.txt"),
            PathBuf::from("C:\\b\\doc.txt"),
        ];
        let dest = PathBuf::from("C:\\dst");
        let decoys = HashSet::new();
        let exists = |p: &Path| p.starts_with("C:\\a") || p.starts_with("C:\\b"); // both sources exist, no target exists
        let (to_copy, skipped) = resolve_targets(&sources, &dest, &decoys, &exists);
        assert_eq!(to_copy.len(), 1, "only the first same-name source copies");
        assert_eq!(skipped.len(), 1, "the second is skipped, not renamed");
    }

    #[test]
    fn resolve_skips_decoys() {
        let decoy = PathBuf::from("C:\\secret\\honey.docx");
        let sources = vec![decoy.clone()];
        let dest = PathBuf::from("C:\\dst");
        let mut decoys = HashSet::new();
        decoys.insert(decoy.clone());
        let exists = |_: &Path| true;
        let (to_copy, skipped) = resolve_targets(&sources, &dest, &decoys, &exists);
        assert!(to_copy.is_empty());
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].reason.contains("decoy"));
    }

    #[test]
    fn resolve_skips_source_containing_destination() {
        // Pasting a folder into a subfolder of itself must be refused.
        let sources = vec![PathBuf::from("C:\\projects")];
        let dest = PathBuf::from("C:\\projects\\backup");
        let decoys = HashSet::new();
        let exists = |_: &Path| true;
        let (to_copy, skipped) = resolve_targets(&sources, &dest, &decoys, &exists);
        assert!(to_copy.is_empty());
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].reason.contains("destination"));
    }

    #[test]
    fn resolve_skips_missing_source() {
        let sources = vec![PathBuf::from("C:\\gone\\x.txt")];
        let dest = PathBuf::from("C:\\dst");
        let decoys = HashSet::new();
        let exists = |_: &Path| false; // nothing exists
        let (to_copy, skipped) = resolve_targets(&sources, &dest, &decoys, &exists);
        assert!(to_copy.is_empty());
        assert!(skipped[0].reason.contains("no longer exists"));
    }

    #[test]
    fn copy_tree_preserves_names_and_structure() {
        let root = tempfile::TempDir::new().unwrap();
        let src = root.path().join("srcdir");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("Invoice 2982.txt"), b"body").unwrap();
        std::fs::write(src.join("sub").join("nested.txt"), b"n").unwrap();

        let dst = root.path().join("dstdir");
        let skipped = copy_tree(&src, &dst, &|_| false).unwrap();

        assert_eq!(skipped, 0);
        assert!(
            dst.join("Invoice 2982.txt").is_file(),
            "exact name preserved"
        );
        assert!(
            dst.join("sub").join("nested.txt").is_file(),
            "structure preserved"
        );
        assert_eq!(
            std::fs::read(dst.join("Invoice 2982.txt")).unwrap(),
            b"body"
        );
    }

    #[test]
    fn copy_tree_single_file() {
        let root = tempfile::TempDir::new().unwrap();
        let src = root.path().join("a.txt");
        std::fs::write(&src, b"hello").unwrap();
        let dst = root.path().join("out").join("a.txt");
        copy_tree(&src, &dst, &|_| false).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello");
    }

    #[test]
    fn copy_tree_skips_nested_decoy() {
        // A decoy enrolled INSIDE a copied folder must never be copied — the
        // whole point of Safe Copy honouring honeypots.
        let root = tempfile::TempDir::new().unwrap();
        let src = root.path().join("Work");
        std::fs::create_dir_all(src.join("Clients")).unwrap();
        std::fs::write(src.join("ok.txt"), b"ok").unwrap();
        let decoy = src.join("Clients").join("honey.docx");
        std::fs::write(&decoy, b"trap").unwrap();

        let dst = root.path().join("out");
        let decoy_lc = decoy.to_string_lossy().to_lowercase();
        let skip = |p: &Path| p.to_string_lossy().to_lowercase() == decoy_lc;
        let skipped = copy_tree(&src, &dst, &skip).unwrap();

        assert_eq!(skipped, 1, "the nested decoy is skipped");
        assert!(dst.join("ok.txt").is_file(), "non-decoy files still copy");
        assert!(
            !dst.join("Clients").join("honey.docx").exists(),
            "decoy must NOT be copied"
        );
    }

    #[test]
    fn staged_files_lists_only_nested_files() {
        let root = tempfile::TempDir::new().unwrap();
        let staged = root.path().join("staged");
        std::fs::create_dir_all(staged.join("nested")).unwrap();
        std::fs::write(staged.join("one.txt"), b"one").unwrap();
        std::fs::write(staged.join("nested").join("two.txt"), b"two").unwrap();

        let mut files = Vec::new();
        staged_files(&staged, &mut files).unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|path| path.is_file()));
    }

    #[test]
    fn scrub_validation_rejects_any_non_clean_outcome() {
        let mut error = crate::file_metadata::ScrubReport::default();
        error.errors.push(crate::file_metadata::ScrubError {
            input_path: "staged.docx".into(),
            message: "engine failed".into(),
        });
        assert!(validate_scrub_report(&error, 1).is_err());

        let skipped = crate::file_metadata::ScrubReport {
            skipped_count: 1,
            ..Default::default()
        };
        assert!(validate_scrub_report(&skipped, 1).is_err());

        let residual = crate::file_metadata::ScrubReport {
            residual_count: 1,
            ..Default::default()
        };
        assert!(validate_scrub_report(&residual, 1).is_err());

        let incomplete = crate::file_metadata::ScrubReport::default();
        assert!(validate_scrub_report(&incomplete, 1).is_err());
        assert!(validate_scrub_report(&incomplete, 0).is_ok());
    }
}
