//! Experimental, bounded native confirmation. Not wired to IPC.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, S_OK, WPARAM};
use windows_sys::Win32::UI::Controls::{TaskDialogIndirect, TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDCBF_OK_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER, TDM_CLICK_BUTTON, TDN_TIMER};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, IDCANCEL, IDOK};

use crate::disk_tree_delete::PreparedTree;

const DIALOG_TTL: Duration = Duration::from_secs(30);
static PENDING: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Permit;
impl Permit {
    pub(crate) fn acquire() -> Result<Self, String> {
        PENDING.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < 4).then_some(n + 1))
            .map(|_| Self).map_err(|_| "Too many pending folder confirmations".into())
    }
}
impl Drop for Permit { fn drop(&mut self) { PENDING.fetch_sub(1, Ordering::Release); } }

unsafe extern "system" fn callback(hwnd: HWND, message: u32, _wparam: WPARAM, _lparam: LPARAM, data: isize) -> i32 {
    if message == TDN_TIMER as u32 && data != 0 {
        let state = unsafe { &*(data as *const DialogState) };
        if state.started.elapsed() >= DIALOG_TTL {
            if unsafe { PostMessageW(hwnd, TDM_CLICK_BUTTON as u32, IDCANCEL as WPARAM, 0) } == 0 {
                state.dismiss_failed.store(true, Ordering::Release);
            }
        }
    }
    S_OK
}

struct DialogState { started: Instant, dismiss_failed: AtomicBool }

fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }

fn expire_retained<T: Send + 'static>(retained: Arc<Mutex<Option<T>>>, timeout: Duration) -> Result<mpsc::Sender<()>, String> {
    let (finished_tx, finished_rx) = mpsc::channel::<()>();
    std::thread::Builder::new().name("folder-dialog-expiry".into()).spawn(move || {
        if finished_rx.recv_timeout(timeout).is_err() {
            retained.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        }
    }).map_err(|_| "Could not arrange folder confirmation expiry".to_string())?;
    Ok(finished_tx)
}

fn safe_target(value: &str) -> String {
    value.chars().flat_map(|c| {
        if c == '\\' {
            vec!['\\', '\\']
        } else if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}') {
            format!("\\u{{{:04X}}}", c as u32).chars().collect::<Vec<_>>()
        } else { vec![c] }
    }).collect()
}

/// The worker owns both the permit and root until the dialog actually closes,
/// even if the caller abandons the async request.
fn shared_ownership_warning(has_shared_items: bool) -> &'static str {
    if has_shared_items {
        "\n\nSome items are owned by the shared Administrators group and may be used by other administrators. Confirm only if you intend to delete these shared items."
    } else { "" }
}

fn confirmation_detail(path: &str, files: usize, folders: usize, shared_warning: &str) -> String {
    format!("Permanently delete this folder?\n\nTarget: {}\nBackslashes in the target are displayed twice; control characters are escaped.\nFreshly verified current tree: {} files · {} folders (including the selected folder). These counts are independent of the earlier scan.{}\n\nNew items are not included and may cause partial deletion. This confirmation expires after 30 seconds.", safe_target(path), files, folders, shared_warning)
}

pub(super) async fn confirm(folder: PreparedTree, permit: Permit) -> Result<Option<PreparedTree>, String> {
    if crate::cli::tauri_runtime_active() { return Ok(None); }
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let state = DialogState { started: Instant::now(), dismiss_failed: AtomicBool::new(false) };
        let target = folder.authorization_target().0.to_str().ok_or("Folder path cannot be displayed accurately.")?;
        let (files, folders) = folder.item_counts();
        let shared_warning = shared_ownership_warning(folder.has_shared_administrator_owned_items());
        let detail_text = confirmation_detail(target, files, folders, shared_warning);
        // Release the retained directory handles even if the dialog's message
        // loop stalls and cannot process the automatic cancel message.
        let retained = Arc::new(Mutex::new(Some(folder)));
        let finished_tx = expire_retained(Arc::clone(&retained), DIALOG_TTL)?;
        let title = wide("WinCommander — confirm folder deletion");
        let detail = wide(&detail_text);
        let config = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_CALLBACK_TIMER,
            dwCommonButtons: TDCBF_OK_BUTTON | TDCBF_CANCEL_BUTTON,
            nDefaultButton: IDCANCEL,
            pszWindowTitle: title.as_ptr(),
            pszContent: detail.as_ptr(),
            pfCallback: Some(callback),
            lpCallbackData: (&state as *const DialogState) as isize,
            ..Default::default()
        };
        let mut clicked = IDCANCEL;
        let result = unsafe { TaskDialogIndirect(&config, &mut clicked, std::ptr::null_mut(), std::ptr::null_mut()) };
        let _ = finished_tx.send(());
        let folder = retained.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        if result >= 0 && clicked == IDOK && state.started.elapsed() < DIALOG_TTL && !state.dismiss_failed.load(Ordering::Acquire) { Ok(folder) } else { Ok(None) }
    }).await.map_err(|_| "Folder confirmation failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::{confirmation_detail, expire_retained, safe_target, shared_ownership_warning};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    #[test]
    fn confirmation_escapes_direction_controls_and_newlines() {
        assert_eq!(safe_target("a\u{202e}b\nc"), "a\\u{202E}b\\u{000A}c");
    }

    #[test]
    fn confirmation_distinguishes_controls_from_literal_escape_text() {
        let control = "C:\\vault\\foo\u{202e}bar";
        let literal = "C:\\vault\\foo\\u{202E}bar";
        assert_ne!(safe_target(control), safe_target(literal));
        assert_eq!(safe_target("C:\\vault\\file"), "C:\\\\vault\\\\file");
    }

    #[test]
    fn confirmation_detail_explains_the_rendered_path() {
        let detail = confirmation_detail("C:\\vault\\foo\u{202e}bar", 2, 1, "");
        assert!(detail.contains("Target: C:\\\\vault\\\\foo\\u{202E}bar"));
        assert!(detail.contains("Backslashes in the target are displayed twice; control characters are escaped."));
        assert!(detail.contains("2 files · 1 folders"));
    }

    #[test]
    fn shared_ownership_warning_requires_explicit_confirmation() {
        assert!(shared_ownership_warning(true).contains("shared Administrators"));
        assert!(shared_ownership_warning(true).contains("other administrators"));
        assert!(shared_ownership_warning(false).is_empty());
    }

    #[test]
    fn stalled_dialog_releases_retained_target() {
        let target = Arc::new(Mutex::new(Some(42)));
        let _dialog = expire_retained(Arc::clone(&target), Duration::from_millis(10)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while target.lock().unwrap().is_some() {
            assert!(std::time::Instant::now() < deadline, "expiry worker did not release target");
            std::thread::yield_now();
        }
    }

    #[tokio::test]
    #[ignore = "requires an interactive desktop and disposable WINCOMMANDER_DISK_TREE_TEST_BASE"]
    async fn interactive_timeout_preserves_the_prepared_folder() {
        use crate::disk_tree_delete::{AuthorizedRoot, ProtectedRoots, prepare_authorized};
        let base = std::env::var_os("WINCOMMANDER_DISK_TREE_TEST_BASE").expect("a disposable test base is required");
        let fixture = tempfile::Builder::new().prefix("folder-confirmation-").tempdir_in(base).unwrap();
        let target = fixture.path().join("selected");
        std::fs::create_dir(&target).unwrap();
        let item = target.join("item.txt");
        std::fs::write(&item, b"disposable confirmation fixture").unwrap();
        let root = AuthorizedRoot::open(&target, ProtectedRoots::fixture(fixture.path()).unwrap()).unwrap();
        let prepared = prepare_authorized(root).unwrap();
        let started = std::time::Instant::now();
        let result = super::confirm(prepared, super::Permit::acquire().unwrap()).await.unwrap();
        assert!(result.is_none());
        assert!(started.elapsed() >= Duration::from_secs(25), "dialog closed before the expiry interval");
        assert_eq!(std::fs::read(item).unwrap(), b"disposable confirmation fixture");
    }
}
