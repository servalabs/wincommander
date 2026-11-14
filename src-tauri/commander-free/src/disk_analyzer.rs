// SPDX-License-Identifier: AGPL-3.0-or-later
mod explorer;
mod file_types;
mod large;
#[cfg(test)]
mod live_tests;
mod progress;
mod walker;

pub use progress::ScanProgress;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::SystemTime;
use walker::{walk_dir, WalkAccum};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskNode {
    pub entry_id: u64,
    pub name: String,
    pub full_path: String,
    pub size: u64,
    pub allocated: Option<u64>,
    pub is_dir: bool,
    pub last_modified: String,
    pub file_count: u64,
    pub folder_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeDiskItem {
    pub entry_id: u64,
    pub name: String,
    pub full_path: String,
    pub size: u64,
    pub allocated: Option<u64>,
    pub is_dir: bool,
    pub last_modified: String,
    pub file_count: u64,
    pub folder_count: u64,
    pub item_type: String,
    pub cleanup_hint: String,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMeta {
    pub scan_id: String,
    pub scan_root: String,
    pub total_size: u64,
    pub free_space: u64,
    pub drive_capacity: u64,
    pub file_count: u64,
    pub folder_count: u64,
    /// Retained for frontend compatibility — always true now since the
    /// scan is in-process and always available. The field was named
    /// `wiztree_found` when the analyzer shelled out to WizTree.
    pub wiztree_found: bool,
    pub complete: bool,
    pub skipped_entries: u64,
    pub skipped_links: u64,
    pub truncated: bool,
    pub cancelled: bool,
    pub size_basis: String,
    pub allocated_size_available: bool,
}

// ═══════════════════════════════════════════════════════════════════════
// In-memory cache populated by run_disk_scan and read by the rest.
// ═══════════════════════════════════════════════════════════════════════

struct ScanCache {
    #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
    folder_identities: HashMap<u64, (u32, u64)>,
    /// folder absolute path → its immediate children, sorted by size desc.
    children: HashMap<String, Vec<DiskNode>>,
    /// flat list of all items (files + folders) ≥ MIN_LARGE_BYTES, sorted
    /// by size desc. Used by get_large_disk_items.
    large_items: Vec<LargeDiskItem>,
    /// last scan's metadata, for sanity / debug.
    meta: Option<ScanMeta>,
}

impl ScanCache {
    fn empty() -> Self {
        Self {
            #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
            folder_identities: HashMap::new(),
            children: HashMap::new(),
            large_items: Vec::new(),
            meta: None,
        }
    }
}

static CACHE: OnceLock<Mutex<ScanCache>> = OnceLock::new();

fn cache() -> &'static Mutex<ScanCache> {
    CACHE.get_or_init(|| Mutex::new(ScanCache::empty()))
}

// Cutoff below which the large-items list ignores entries. The frontend
// filters again with the user's chosen min size — this just keeps the
// in-memory list bounded on huge volumes.
const MIN_LARGE_BYTES: u64 = 16 * 1024 * 1024; // 16 MB

fn format_mtime(t: Option<SystemTime>) -> String {
    match t {
        Some(st) => match st.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => format!("{}", d.as_secs()),
            Err(_) => String::new(),
        },
        None => String::new(),
    }
}

fn classify_file(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "mp4" | "mkv" | "mov" | "avi" | "wmv" | "webm" | "m4v" => "Video".into(),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp" | "heic" => "Image".into(),
        "mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac" => "Audio".into(),
        "zip" | "rar" | "7z" | "tar" | "gz" | "cab" => "Archive".into(),
        "exe" | "msi" | "appx" | "msix" => "Installer".into(),
        "iso" => "Disk image".into(),
        "vhd" | "vhdx" | "vmdk" => "Virtual disk".into(),
        "log" | "tmp" | "bak" | "old" => "Temp / log".into(),
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" => {
            "Document".into()
        }
        _ => "File".into(),
    }
}

fn cleanup_hint(name: &str, path: &str, is_dir: bool) -> String {
    let lower_name = name.to_ascii_lowercase();
    let lower_path = path.to_ascii_lowercase();
    if is_dir {
        if lower_path.contains("\\temp") || lower_path.contains("/temp") {
            return "Temporary folder — close applications and review its contents.".into();
        }
        if lower_path.contains("appdata\\local\\temp") {
            return "User Temp folder — clear when programs are closed.".into();
        }
        if lower_name == "node_modules" || lower_name == "target" || lower_name == "dist" {
            return "Possible project artifacts — verify the project and build configuration before removal.".into();
        }
        return "Folder — review before deleting.".into();
    }
    if lower_name.ends_with(".log") || lower_name.ends_with(".tmp") || lower_name.ends_with(".bak")
    {
        return "Log, temporary or backup file — may contain diagnostics or recoverable work; review before removal.".into();
    }
    if lower_name.ends_with(".iso") || lower_name.ends_with(".vhd") || lower_name.ends_with(".vhdx")
    {
        return "Disk image — keep if you still need it.".into();
    }
    "Inspect before deleting.".into()
}

fn risk_for(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.contains("\\windows\\")
        || lower.contains("/windows/")
        || lower.contains("\\program files")
    {
        return "high".into();
    }
    if lower.contains("\\users\\") || lower.contains("/users/") {
        return "medium".into();
    }
    "medium".into()
}

#[cfg(target_os = "windows")]
fn drive_free_and_total(root: &Path) -> (u64, u64) {
    // GetDiskFreeSpaceExW accepts any path that lives on the target volume
    // (it resolves the volume itself) and returns the caller-visible free
    // bytes + the volume's total capacity. `Win32_Storage_FileSystem` is
    // already an enabled windows-sys feature for this crate (storage_probe.rs
    // uses the same module for its own unsafe FFI), so no new dependency is
    // needed here.
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut free_available_to_caller: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free_bytes: u64 = 0;

    // SAFETY: `wide` is a valid, null-terminated UTF-16 string that outlives
    // this call. The three out-parameters are valid, non-null, properly
    // aligned `u64` locals — GetDiskFreeSpaceExW only ever writes through
    // pointers that are non-null, per its documented contract.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_available_to_caller,
            &mut total_bytes,
            &mut total_free_bytes,
        )
    };

    if ok == 0 {
        // Path doesn't exist, isn't a resolvable volume, or access was
        // denied — fall back to "unknown" (0, 0) rather than failing the
        // whole scan; the frontend already handles a zeroed free-space value.
        return (0, 0);
    }

    // Use the caller-visible free bytes (respects per-user disk quotas, the
    // same value Explorer/PowerShell surface as "free") against the volume's
    // raw total capacity.
    (free_available_to_caller, total_bytes)
}

#[cfg(not(target_os = "windows"))]
fn drive_free_and_total(_root: &Path) -> (u64, u64) {
    (0, 0)
}

// ═══════════════════════════════════════════════════════════════════════
// TAURI COMMANDS — native (no Pro / WizTree dispatch)
// ═══════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn run_disk_scan(path: String) -> Result<ScanMeta, String> {
    let root = PathBuf::from(&path);
    if !root.is_absolute() || !root.is_dir() {
        return Err("Choose an existing absolute folder path.".into());
    }
    let metadata = fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
    if walker::is_link(&metadata) {
        return Err("Choose a real folder rather than a link or reparse point.".into());
    }
    explorer::reject_link_components(&root)?;
    let root = explorer::display_path(fs::canonicalize(root).map_err(|error| error.to_string())?);
    let scan_guard = progress::ScanGuard::begin()?;
    *cache()
        .lock()
        .map_err(|_| "scan cache lock poisoned".to_string())? = ScanCache::empty();
    let root_clone = root.clone();
    // Walk on a blocking-friendly task — recursive read_dir is sync.
    let result = tokio::task::spawn_blocking(move || {
        let mut acc = WalkAccum::new();
        let root_node = walk_dir(&root_clone, &mut acc, &progress::CANCELLED, 0, 0);
        (acc, root_node, scan_guard)
    })
    .await
    .map_err(|e| format!("scan task failed: {}", e))?;

    let (mut acc, root_node, _scan_guard) = result;
    // Sort large items by size descending.
    acc.large.sort_by_key(|b| std::cmp::Reverse(b.size));

    let (free, capacity) = drive_free_and_total(&root);

    let meta = ScanMeta {
        scan_id: uuid::Uuid::new_v4().to_string(),
        scan_root: root.to_string_lossy().into_owned(),
        total_size: root_node.size,
        free_space: free,
        drive_capacity: capacity,
        file_count: root_node.file_count,
        folder_count: root_node.folder_count,
        wiztree_found: true,
        complete: acc.skipped_entries == 0
            && !acc.truncated
            && !progress::CANCELLED.load(Ordering::Acquire),
        skipped_entries: acc.skipped_entries,
        skipped_links: acc.skipped_links,
        truncated: acc.truncated,
        cancelled: progress::CANCELLED.load(Ordering::Acquire),
        size_basis: "logical".into(),
        allocated_size_available: false,
    };

    let mut guard = cache().lock().map_err(|e| format!("cache lock: {}", e))?;
    #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
    { guard.folder_identities = acc.folder_identities; }
    guard.children = acc.children_by_dir;
    guard.large_items = acc.large;
    guard.meta = Some(meta.clone());
    Ok(meta)
}

#[tauri::command]
pub fn cancel_disk_scan() {
    progress::cancel();
}

#[tauri::command]
pub fn get_disk_scan_progress() -> ScanProgress {
    progress::snapshot()
}

#[tauri::command]
pub async fn get_disk_children(path: String) -> Result<Vec<DiskNode>, String> {
    let guard = cache().lock().map_err(|e| format!("cache lock: {}", e))?;
    match guard.children.get(&path) {
        Some(v) => Ok(v.clone()),
        None => Err("No scan data. Run a scan first.".into()),
    }
}

/// Lowercased, separator-normalized path string for case-insensitive Windows
/// comparison (trailing separators dropped).
fn norm_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\");
    // Strip the \\?\ verbatim prefix that std::fs::canonicalize returns on
    // Windows so a canonicalized target compares equal to env-derived roots.
    let s = s.strip_prefix("\\\\?\\").map(str::to_string).unwrap_or(s);
    s.trim_end_matches('\\').to_lowercase()
}

/// System-critical locations whose destruction would brick the OS or the app.
/// Sourced from the environment + the running exe's directory.
fn protected_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in [
        "SystemRoot",
        "windir",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "PUBLIC",
        "USERPROFILE",
    ] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                roots.push(PathBuf::from(v));
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    roots
}

/// The subset of protected roots that are OS/app SYSTEM directories (as opposed
/// to user-data roots like the profile or Public). Deleting anything *inside*
/// these — not just the root itself — can brick Windows or the app, so the disk
/// analyzer refuses their descendants too. Legitimate large-file cleanup inside
/// USER roots (USERPROFILE, PUBLIC) stays allowed.
fn system_protected_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in [
        "SystemRoot",
        "windir",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                roots.push(PathBuf::from(v));
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    roots
}

/// True if `target` lies strictly INSIDE one of the given system roots (a
/// descendant, not the root itself). `is_protected_against` covers the root
/// itself, its ancestors, and bare drive roots; this closes the gap where a
/// large file the analyzer surfaced under e.g. `C:\Windows\System32\...` or
/// `C:\Program Files\...` could be deleted and break the OS or an installed
/// app. Injected roots keep it unit-testable.
fn is_inside_protected_root(target: &Path, system_roots: &[PathBuf]) -> bool {
    let t = norm_path(target);
    if t.is_empty() {
        return false;
    }
    system_roots.iter().any(|root| {
        let r = std::fs::canonicalize(root)
            .map(|c| norm_path(&c))
            .unwrap_or_else(|_| norm_path(root));
        if r.is_empty() {
            return false;
        }
        let r_prefix = format!("{r}\\");
        t.starts_with(&r_prefix)
    })
}

/// True if deleting `target` would take out a system-critical location — it
/// IS a protected root, an ANCESTOR of one (recursive delete destroys it), or
/// a bare drive/filesystem root. Deleting a specific file *inside* a root is
/// still allowed here (legitimate large-file cleanup); descendants of SYSTEM
/// roots are additionally blocked by `is_inside_protected_root` at the call
/// site. `roots` is injected so this is unit-testable without the real
/// filesystem. The frontend confirm() dialog is not a security control — this
/// is the backend re-check.
fn is_protected_against(target: &Path, roots: &[PathBuf]) -> bool {
    if target.parent().is_none() {
        return true; // bare root: C:\, D:\
    }
    let t = norm_path(target);
    if t.is_empty() {
        return true;
    }
    let t_prefix = format!("{t}\\");
    roots.iter().any(|root| {
        // Canonicalize the root where possible so it compares against a
        // canonicalized target (short-name/symlink/junction robustness); fall
        // back to the lexical form for roots that don't exist on this machine.
        let r = std::fs::canonicalize(root)
            .map(|c| norm_path(&c))
            .unwrap_or_else(|_| norm_path(root));
        !r.is_empty() && (t == r || r.starts_with(&t_prefix))
    })
}

/// A structured receipt distinguishes requests accepted by Windows from
/// actual space reclaimed; only available when the experimental feature is built.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FolderDeleteReceipt {
    pub status: &'static str,
    pub processed: usize,
    pub cache_invalidated: bool,
    pub message: Option<String>,
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
struct FolderCacheInvalidator;

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
impl FolderCacheInvalidator {
    fn invalidate(&self) {
        let mut guard = cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = ScanCache::empty();
    }
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
impl Drop for FolderCacheInvalidator {
    fn drop(&mut self) { self.invalidate(); }
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
fn folder_delete_receipt(result: Result<usize, crate::disk_tree_delete::DeleteFailure>) -> FolderDeleteReceipt {
    let (status, processed, message) = match result {
        Ok(n) => ("delete_requested", n, None),
        Err(failure) => (if failure.processed == 0 { "failed" } else { "partial" }, failure.processed, Some(failure.error.to_string())),
    };
    FolderDeleteReceipt { status, processed, cache_invalidated: true, message }
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
async fn delete_confirmed_folder(binding: String, capability_token: Option<String>) -> Result<FolderDeleteReceipt, String> {
    // Consume before enumeration so a forged request cannot lock/traverse a tree.
    let tree = crate::authz::consume_folder(capability_token.as_deref(), &binding)?;
    tokio::task::spawn_blocking(move || {
        let invalidator = FolderCacheInvalidator;
        let result = crate::disk_tree_delete::delete_prepared(tree);
        invalidator.invalidate();
        Ok(folder_delete_receipt(result))
    }).await.map_err(|_| "Folder deletion worker failed; rescan to determine what remains".to_string())?
}

// The same capability guard applies to the UI and direct IPC requests.
#[tauri::command]
pub fn disk_folder_delete_available() -> bool {
    // Acceptance for destructive folder deletion is incomplete.
    false
}

#[cfg(test)]
#[test]
fn folder_deletion_stays_unavailable_in_every_build() {
    assert!(!disk_folder_delete_available());
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
fn scanned_folder(scan_id: &str, item_id: u64) -> Result<(PathBuf, (u32, u64)), String> {
    if item_id == 0 { return Err("The scan root cannot be deleted.".into()); }
    let (_, path, is_dir) = explorer::scanned_path(scan_id, item_id)?;
    if !is_dir { return Err("This scan item is not a folder.".into()); }
    explorer::reject_link_components(&path)?;
    let guard = cache().lock().map_err(|_| "Scan cache unavailable.")?;
    if guard.meta.as_ref().map(|meta| meta.scan_id.as_str()) != Some(scan_id) { return Err("This scan is no longer current.".into()); }
    let identity = *guard.folder_identities.get(&item_id).ok_or("Folder identity was unavailable during the scan. Analyze it again.")?;
    Ok((path, identity))
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
fn folder_binding(scan_id: &str, item_id: u64) -> String {
    serde_json::to_string(&(scan_id, item_id)).expect("scan identity serializes")
}

#[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
fn verify_scanned_folder_identity(expected: (u32, u64), actual: crate::disk_tree_delete::Identity) -> Result<(), String> {
    if (actual.volume, actual.index) != expected { return Err("Folder changed since the scan. Analyze it again.".into()); }
    Ok(())
}

#[cfg(all(test, windows, feature = "experimental-disk-tree-delete"))]
#[test]
fn folder_replaced_after_scan_has_a_different_identity() {
    let base = tempfile::Builder::new().prefix("wincmd-folder-identity-").tempdir().unwrap();
    let selected = base.path().join("selected");
    std::fs::create_dir(&selected).unwrap();
    let expected = crate::routine_cleaner::file_identity(&selected).unwrap();
    std::fs::rename(&selected, base.path().join("original")).unwrap();
    std::fs::create_dir(&selected).unwrap();
    let (volume, index) = crate::routine_cleaner::file_identity(&selected).unwrap();
    assert!(verify_scanned_folder_identity(expected, crate::disk_tree_delete::Identity { volume, index }).is_err());
}

#[cfg(all(test, windows, feature = "experimental-disk-tree-delete"))]
#[test]
fn scanned_alias_retargeted_to_another_folder_is_rejected() {
    let base = tempfile::Builder::new().prefix("wincmd-folder-alias-test-").tempdir().unwrap();
    let first = base.path().join("first");
    let second = base.path().join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(first.join("keep.txt"), b"first").unwrap();
    std::fs::write(second.join("keep.txt"), b"second").unwrap();
    let scanned_identity = crate::routine_cleaner::file_identity(&first).unwrap();
    let (volume, index) = crate::routine_cleaner::file_identity(&second).unwrap();
    assert!(verify_scanned_folder_identity(scanned_identity, crate::disk_tree_delete::Identity { volume, index }).is_err());
    assert_eq!(std::fs::read(first.join("keep.txt")).unwrap(), b"first");
    assert_eq!(std::fs::read(second.join("keep.txt")).unwrap(), b"second");
}

#[tauri::command]
pub async fn disk_confirm_folder_delete(scan_id: String, item_id: u64) -> Result<Option<String>, String> {
    if !disk_folder_delete_available() { return Err("Folder deletion is unavailable in this build.".into()); }
    #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
    {
        crate::duplicate_finder::safety::ensure_mutation_allowed()?;
        let (path, expected) = scanned_folder(&scan_id, item_id)?;
        let permit = crate::authz::FolderPreparationPermit::acquire()?;
        let (folder, permit) = tokio::task::spawn_blocking(move || {
            let protected = crate::disk_tree_delete::ProtectedRoots::system().map_err(|error| format!("Protected folders could not be verified: {error}"))?;
            let folder = crate::disk_tree_delete::AuthorizedRoot::open(&path, protected).map_err(|error| format!("Folder cannot be safely selected: {error}"))?;
            let (_, actual) = folder.target();
            verify_scanned_folder_identity(expected, actual)?;
            let tree = crate::disk_tree_delete::prepare_authorized(folder).map_err(|error| format!("Folder could not be safely prepared: {error}"))?;
            Ok::<_, String>((tree, permit))
        }).await.map_err(|_| "Folder verification worker failed".to_string())??;
        crate::authz::confirm_folder(folder_binding(&scan_id, item_id), folder, permit).await
    }
    #[cfg(not(all(windows, feature = "experimental-disk-tree-delete")))]
    { let _ = (scan_id, item_id); Err("Folder deletion is unavailable in this build.".into()) }
}

#[tauri::command]
pub async fn disk_delete_confirmed_folder(scan_id: String, item_id: u64, capability_token: Option<String>) -> Result<FolderDeleteReceipt, String> {
    if !disk_folder_delete_available() { return Err("Folder deletion is unavailable in this build.".into()); }
    #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
    {
        crate::duplicate_finder::safety::ensure_mutation_allowed()?;
        let _ = scanned_folder(&scan_id, item_id)?;
        delete_confirmed_folder(folder_binding(&scan_id, item_id), capability_token).await
    }
    #[cfg(not(all(windows, feature = "experimental-disk-tree-delete")))]
    { let _ = (scan_id, item_id, capability_token); Err("Folder deletion is unavailable in this build.".into()) }
}

#[tauri::command]
pub async fn disk_delete_item(
    path: String,
    capability_token: Option<String>,
) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    // Canonicalize BEFORE the protected-root check so a `..`/symlink/junction
    // traversal (e.g. C:\Users\Public\..\..\Windows\System32) can't slip a
    // protected location past the purely-lexical check while the OS delete
    // resolves it at the filesystem layer (audit finding H1).
    let p =
        std::fs::canonicalize(&p).map_err(|e| format!("cannot resolve path '{}': {}", path, e))?;
    if is_protected_against(&p, &protected_roots()) {
        return Err(format!(
            "Refused: '{}' is a protected system location — the disk analyzer will not delete it.",
            path
        ));
    }
    // Also refuse anything INSIDE a system root (Windows / Program Files /
    // ProgramData / the app's own folder). is_protected_against deliberately
    // allows deleting files inside a root for large-file cleanup, but that is
    // only safe under USER roots (profile / Public); a descendant of a SYSTEM
    // root — e.g. a big file the scan surfaced under C:\Windows\System32 — must
    // never be deletable, or a single confirm click could break the OS.
    if is_inside_protected_root(&p, &system_protected_roots()) {
        return Err(format!(
            "Refused: '{}' is inside a protected system directory (Windows / Program Files / ProgramData / the app folder) — the disk analyzer will not delete files there.",
            path
        ));
    }
    if p.is_dir() {
        return Err(
            "Directory deletion is disabled because handle-safe recursive deletion is unavailable. Delete individual files instead."
                .to_string(),
        );
    }
    let identity = crate::path_identity::ExpectedFileIdentity::capture(&p)?;
    crate::authz::consume_required(
        capability_token.as_deref(),
        crate::authz::DestructiveAction::DiskDelete,
        &crate::authz::disk_delete_args(&p.to_string_lossy()),
    )?;
    identity.delete_file()?;

    if let Ok(mut guard) = cache().lock() {
        *guard = ScanCache::empty();
    }
    Ok(())
}

#[tauri::command]
pub async fn open_disk_item(scan_id: String, item_id: u64) -> Result<(), String> {
    explorer::open_disk_item(scan_id, item_id).await
}

#[tauri::command]
pub async fn get_large_disk_items(
    min_size_bytes: Option<u64>,
    limit: Option<usize>,
    include_dirs: Option<bool>,
) -> Result<large::LargeDiskResult, String> {
    large::get_large_disk_items(min_size_bytes, limit, include_dirs).await
}

#[tauri::command]
pub async fn get_disk_file_types(
    scan_id: String,
    path: String,
) -> Result<file_types::DiskFileTypes, String> {
    file_types::get_disk_file_types(scan_id, path).await
}

#[cfg(test)]
mod tests {
    #[cfg(all(windows, feature = "experimental-disk-tree-delete"))]
    #[test]
    fn folder_receipt_distinguishes_partial_mutation_from_failure() {
        use super::folder_delete_receipt;
        use crate::disk_tree_delete::DeleteFailure;
        let partial = folder_delete_receipt(Err(DeleteFailure { processed: 2, error: std::io::Error::other("fixture") }));
        assert_eq!(partial.status, "partial");
        assert_eq!(partial.processed, 2);
        assert!(partial.cache_invalidated);
        let failed = folder_delete_receipt(Err(DeleteFailure { processed: 0, error: std::io::Error::other("fixture") }));
        assert_eq!(failed.status, "failed");
        assert_eq!(folder_delete_receipt(Ok(2)).status, "delete_requested");
    }
    use super::*;

    fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from("C:\\Windows"),
            PathBuf::from("C:\\Program Files"),
            PathBuf::from("C:\\ProgramData"),
        ]
    }

    #[test]
    fn refuses_protected_roots_and_ancestors() {
        let r = roots();
        // Exact root, with case + forward-slash variants.
        assert!(is_protected_against(Path::new("C:\\Windows"), &r));
        assert!(is_protected_against(Path::new("c:/windows"), &r));
        assert!(is_protected_against(Path::new("C:\\Program Files"), &r));
        // An ancestor of a protected root (deleting it recurses into Windows).
        assert!(is_protected_against(Path::new("C:\\"), &r));
    }

    #[test]
    fn canonicalized_traversal_into_protected_root_is_caught() {
        // H1: a `..` traversal that lexically misses the protected-root check
        // but resolves into it must be caught after canonicalization.
        let tmp = std::env::temp_dir().join("wc_disk_h1_test");
        let protected = tmp.join("windows");
        let pubdir = tmp.join("pub");
        let _ = std::fs::create_dir_all(&protected);
        let _ = std::fs::create_dir_all(&pubdir);
        let roots = vec![protected.clone()];
        let raw = pubdir.join("..").join("windows");
        // Pre-fix: the raw traversal string does NOT lexically match the root.
        assert!(!is_protected_against(&raw, &roots));
        // Post-fix: after canonicalization it does (norm_path strips \\?\).
        let canonical = std::fs::canonicalize(&raw).unwrap();
        assert!(is_protected_against(&canonical, &roots));
    }

    #[test]
    fn allows_specific_files_inside_or_outside_roots() {
        let r = roots();
        // A large file deep under the user profile is a legitimate target.
        assert!(!is_protected_against(
            Path::new("C:\\Users\\me\\Downloads\\big.iso"),
            &r
        ));
        // A sibling dir that merely shares a prefix is NOT C:\Windows.
        assert!(!is_protected_against(
            Path::new("C:\\Windows.old\\junk"),
            &r
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn drive_free_and_total_reports_real_nonzero_values() {
        // Any existing directory resolves to a real volume; the previous stub
        // always returned (0, 0), so this pins the fix: a real drive reports a
        // nonzero, internally-consistent capacity/free pair.
        let root = std::env::temp_dir();
        let (free, total) = drive_free_and_total(&root);
        assert!(total > 0, "expected nonzero drive capacity, got {total}");
        assert!(free > 0, "expected nonzero free space, got {free}");
        assert!(
            free <= total,
            "free space {free} should never exceed capacity {total}"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn drive_free_and_total_falls_back_to_zero_for_bogus_path() {
        // A path with no resolvable volume must fail soft (0, 0), not panic
        // or return garbage.
        let (free, total) = drive_free_and_total(Path::new("Q:\\this\\does\\not\\exist"));
        assert_eq!((free, total), (0, 0));
    }

    #[test]
    fn refuses_files_inside_system_roots_but_allows_user_roots() {
        // Treat the test roots as SYSTEM roots (Windows / Program Files /
        // ProgramData). Descendants of these must be refused so a big file the
        // scan surfaced under e.g. C:\Windows\System32 can't be one-click deleted.
        let sys = roots();
        assert!(is_inside_protected_root(
            Path::new("C:\\Windows\\System32\\DriverStore\\big.sys"),
            &sys
        ));
        assert!(is_inside_protected_root(
            Path::new("C:\\Program Files\\SomeApp\\huge.dat"),
            &sys
        ));
        // The root itself is not a strict descendant (is_protected_against
        // already covers the root and its ancestors).
        assert!(!is_inside_protected_root(Path::new("C:\\Windows"), &sys));
        // A user-profile file is NOT inside a system root — cleanup stays allowed.
        assert!(!is_inside_protected_root(
            Path::new("C:\\Users\\me\\Downloads\\big.iso"),
            &sys
        ));
        // A sibling sharing a prefix is not inside the root.
        assert!(!is_inside_protected_root(
            Path::new("C:\\Windows.old\\junk"),
            &sys
        ));
    }
}
