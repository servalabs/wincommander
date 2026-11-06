// SPDX-License-Identifier: AGPL-3.0-or-later
//! Feature-gated per-user installer/startup coordination.
use std::{path::PathBuf, time::{Duration, Instant}};
use wincmd_search::encrypted_search_fence::SearchOperationFence;

fn known_base() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath}};
    let mut raw = std::ptr::null_mut();
    // SAFETY: the API allocates a terminated path, which is freed below.
    let status = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut raw) };
    if status < 0 || raw.is_null() {
        if !raw.is_null() { unsafe { CoTaskMemFree(raw.cast()) }; }
        return Err("Cannot verify your Windows profile for search migration.".into());
    }
    let known = unsafe {
        let mut len = 0usize;
        while *raw.add(len) != 0 { len += 1; }
        let value = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw.cast());
        PathBuf::from(value)
    };
    let configured = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .ok_or("Cannot verify your Windows profile for search migration.")?;
    // Both startup and installer must use the same profile. Refuse overrides.
    if !known.is_absolute() || !configured.is_absolute()
        || !known.to_string_lossy().trim_end_matches(['\\', '/']).eq_ignore_ascii_case(
            configured.to_string_lossy().trim_end_matches(['\\', '/'])) {
        return Err("Windows profile location does not match the search configuration.".into());
    }
    Ok(known)
}

pub(crate) fn verified_parent() -> Result<PathBuf, String> {
    let known = known_base()?;
    let app_name = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    let mut parts = std::path::Path::new(app_name).components();
    if !matches!(parts.next(), Some(std::path::Component::Normal(_))) || parts.next().is_some() {
        return Err("Invalid application data directory.".into());
    }
    Ok(known.join(app_name).join("file-search"))
}

pub(crate) fn prepare_verified_parent() -> Result<PathBuf, String> {
    let known = known_base()?;
    let app_name = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    SearchOperationFence::prepare_from_known_folder(&known, app_name)
        .map_err(|_| "Cannot prepare the protected search migration lock.".into())
}
pub(crate) fn acquire() -> Result<SearchOperationFence, String> {
    let known = known_base()?;
    let app_name = option_env!("WINCMD_APP_DIR").unwrap_or("WinCommander");
    let parent = SearchOperationFence::prepare_from_known_folder(&known, app_name)
        .map_err(|_| "Cannot prepare the protected search migration lock.")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match SearchOperationFence::acquire_shared(&parent) {
            Ok(guard) => {
                // A helper marks the roll-forward intent before handing its
                // exclusive lock to the installer. If startup wins the small
                // release-to-commit gap, do not treat the update as complete.
                let selection = parent.join(".wincommander-encrypted-selection");
                match std::fs::symlink_metadata(&selection) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                    Err(_) => return Err("Cannot inspect pending search migration.".into()),
                    Ok(_) => match wincmd_search::SearchEngine::inspect_search_migration(&selection) {
                        Ok(None) => {},
                        Ok(Some(_)) => return Err("WinCommander update needs to finish or be retried before search can start.".into()),
                        Err(_) => return Err("Cannot verify the pending search migration; retry the update.".into()),
                    },
                }
                return Ok(guard);
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Err("WinCommander is being updated. Please try again when the installer finishes.".into()),
            Err(_) => return Err("Cannot verify the protected search migration lock.".into()),
        }
    }
}

// The application keeps one process-wide shared lease. Search operations check
// it before using the cached engine. An explicit cleanup briefly swaps it for
// an exclusive lease while the global engine write lock is held.
static ACTIVE_RUNTIME_FENCE: std::sync::Mutex<Option<SearchOperationFence>> = std::sync::Mutex::new(None);

pub(crate) fn install_runtime_fence() -> Result<(), String> {
    let guard = acquire()?;
    let mut slot = ACTIVE_RUNTIME_FENCE.lock().map_err(|_| "Search maintenance state is unavailable.")?;
    if slot.is_some() { return Err("Search maintenance state is already initialized.".into()); }
    *slot = Some(guard); Ok(())
}

pub(crate) fn require_runtime_fence() -> Result<(), String> {
    let slot = ACTIVE_RUNTIME_FENCE.lock().map_err(|_| "Search maintenance state is unavailable.")?;
    if slot.is_some() { Ok(()) } else { Err("Search is unavailable until WinCommander is restarted.".into()) }
}

pub(crate) fn with_exclusive<T>(operation: impl FnOnce(&SearchOperationFence) -> Result<T, String>) -> Result<T, String> {
    let parent = verified_parent()?;
    switch_fence(&ACTIVE_RUNTIME_FENCE,
        || {
            let guard = SearchOperationFence::acquire_exclusive(&parent)
                .map_err(|_| "Close other WinCommander windows before cleaning the old search cache.".to_string())?;
            let selection = parent.join(".wincommander-encrypted-selection");
            match std::fs::symlink_metadata(&selection) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                Err(_) => return Err("Cannot inspect pending search migration.".into()),
                Ok(_) => match wincmd_search::SearchEngine::inspect_search_migration(&selection) {
                    Ok(None) => {},
                    Ok(Some(_)) => return Err("Finish the pending WinCommander update before cleaning search data.".into()),
                    Err(_) => return Err("Cannot verify pending search migration.".into()),
                },
            }
            Ok(guard)
        },
        acquire, operation)
}

fn switch_fence<T>(slot: &std::sync::Mutex<Option<SearchOperationFence>>,
    exclusive: impl FnOnce() -> Result<SearchOperationFence, String>,
    restore: impl FnOnce() -> Result<SearchOperationFence, String>,
    operation: impl FnOnce(&SearchOperationFence) -> Result<T, String>) -> Result<T, String> {
    let mut slot = slot.lock().map_err(|_| "Search maintenance state is unavailable.")?;
    let shared = slot.take().ok_or("Search is unavailable until WinCommander is restarted.")?;
    drop(shared);
    let attempted = (|| { let guard = exclusive()?; operation(&guard) })();
    // If restoration fails, leave the slot empty so search fails closed.
    match restore() {
        Ok(shared) => { *slot = Some(shared); attempted },
        Err(_) => Err("Search maintenance could not restore its safety lock. Restart WinCommander before searching.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_handoff_restores_shared_lock_on_success_and_contention() {
        let root = tempfile::tempdir().unwrap();
        let slot = std::sync::Mutex::new(Some(SearchOperationFence::acquire_shared(root.path()).unwrap()));
        let peer = SearchOperationFence::acquire_shared(root.path()).unwrap();
        let result = switch_fence(&slot,
            || SearchOperationFence::acquire_exclusive(root.path()).map_err(|e| e.to_string()),
            || SearchOperationFence::acquire_shared(root.path()).map_err(|e| e.to_string()),
            |_| Ok(()));
        assert!(result.is_err()); assert!(slot.lock().unwrap().is_some());
        drop(peer);
        let result = switch_fence(&slot,
            || SearchOperationFence::acquire_exclusive(root.path()).map_err(|e| e.to_string()),
            || SearchOperationFence::acquire_shared(root.path()).map_err(|e| e.to_string()),
            |_| Ok(7));
        assert_eq!(result.unwrap(), 7); assert!(slot.lock().unwrap().is_some());
    }
    #[test]
    fn failed_shared_restoration_leaves_search_fence_inactive() {
        let root = tempfile::tempdir().unwrap();
        let slot = std::sync::Mutex::new(Some(SearchOperationFence::acquire_shared(root.path()).unwrap()));
        let result = switch_fence(&slot,
            || SearchOperationFence::acquire_exclusive(root.path()).map_err(|e| e.to_string()),
            || Err("injected restore failure".into()), |_| Ok(()));
        assert!(result.is_err()); assert!(slot.lock().unwrap().is_none());
    }
}