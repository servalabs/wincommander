// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/safe_clip.rs
//
// ═══════════════════════════════════════════════════════════════════════
// SAFE COPY / SAFE PASTE — context-menu clipboard + copy engine
// ═══════════════════════════════════════════════════════════════════════
//
// Safe Copy (right-click a file/folder) records the selection into a small
// per-user "safe clipboard" JSON. Safe Paste (right-click a destination
// folder) copies each recorded source into that folder KEEPING THE EXACT
// NAME (never renamed — a name collision is skipped, not suffixed) and then
// the frontend scrubs the fresh copies in place via the paid metadata
// scrubber.
//
// Safe Copy runs headless (no window, no sidecar) directly off the CLI flag
// so it works whether or not the app is already running. Safe Paste routes
// through the app because the scrub engine lives in the Pro sidecar.
//
// The pure logic (coalesce, resolve_targets) is platform-agnostic and unit-
// tested; only the named-mutex serialisation is Windows-gated.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Multi-select launches the verb once per item, near-simultaneously. A Safe
/// Copy within this window of the previous one APPENDS (same batch); a later
/// one REPLACES (a fresh copy). Clipboard-like otherwise: persists until the
/// next Safe Copy — Safe Paste does not consume it.
const COALESCE_WINDOW_MS: u128 = 4_000;
const CLIP_VERSION: u32 = 1;
const CLIP_FILE: &str = "safe-clip.json";

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
    /// Absolute destination paths of the fresh copies (to be scrubbed).
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

/// Record `paths` onto the safe clipboard (coalescing under a cross-process
/// mutex so concurrent multi-select launches don't clobber each other).
/// Decoy and empty paths are filtered out. Returns the resulting source count.
pub fn record_sources(paths: &[String]) -> Result<usize, String> {
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
    if filtered.is_empty() {
        return Ok(read_clip().map(|c| c.sources.len()).unwrap_or(0));
    }

    let _guard = ClipLock::acquire();
    let merged = coalesce(read_clip(), &filtered, now_ms());
    let count = merged.sources.len();
    write_clip_atomic(&merged)?;
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
    match record_sources(&paths) {
        Ok(n) => crate::log_message("info", &format!("[SafeCopy] recorded {n} item(s)")),
        Err(e) => crate::log_message("warn", &format!("[SafeCopy] record failed: {e}")),
    }
}

// ── Tauri commands ──────────────────────────────────────────────────────────

/// Record a selection onto the safe clipboard from inside the app (parity with
/// the headless CLI path; handy for an in-app "Safe Copy" action and tests).
#[tauri::command]
pub async fn safe_copy_record(paths: Vec<String>) -> Result<usize, String> {
    record_sources(&paths)
}

/// Current safe-clipboard status (for a "N items ready to Safe Paste" hint).
#[tauri::command]
pub async fn safe_clip_status() -> Result<SafeClipStatus, String> {
    Ok(read_clip()
        .map(|c| SafeClipStatus {
            count: c.sources.len(),
            stamped_at_ms: c.stamped_at_ms,
        })
        .unwrap_or(SafeClipStatus {
            count: 0,
            stamped_at_ms: 0,
        }))
}

/// Copy every recorded source into `dest_dir`, preserving exact names (never
/// renamed — collisions are skipped). Returns the fresh copies for the frontend
/// to scrub in place. PAID: the value is the metadata scrub, so the gate runs
/// BEFORE any file operation — a free user gets a clean upsell with zero files
/// touched.
#[tauri::command]
pub async fn safe_paste_prepare(dest_dir: String) -> Result<SafePasteResult, String> {
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

    let mut copied: Vec<String> = Vec::new();
    let mut nested_decoys_skipped = 0u32;
    for (src, target) in to_copy {
        match copy_tree(&src, &target, &decoy_skip) {
            Ok(n) => {
                nested_decoys_skipped += n;
                copied.push(target.to_string_lossy().to_string());
            }
            Err(e) => {
                // Roll back a partial directory copy so a failed paste never
                // leaves an unscrubbed half-tree behind.
                let _ = if target.is_dir() {
                    std::fs::remove_dir_all(&target)
                } else {
                    std::fs::remove_file(&target)
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

    crate::log_message(
        "info",
        &format!(
            "[SafePaste] copied {} item(s), skipped {}",
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
}
