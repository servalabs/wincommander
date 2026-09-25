// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Native Windows progress UI for the headless Safe Copy Explorer verb.
// This intentionally has no Tauri/AppHandle dependency: Explorer can invoke
// Safe Copy before WinCommander is running.

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, S_FALSE, S_OK, WPARAM};
    use windows_sys::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDCBF_OK_BUTTON, TDE_CONTENT,
        TDF_CALLBACK_TIMER, TDF_SHOW_MARQUEE_PROGRESS_BAR, TDF_SHOW_PROGRESS_BAR, TDM_CLICK_BUTTON,
        TDM_ENABLE_BUTTON, TDM_SET_MARQUEE_PROGRESS_BAR, TDM_SET_PROGRESS_BAR_MARQUEE,
        TDM_SET_PROGRESS_BAR_POS, TDM_UPDATE_ELEMENT_TEXT, TDN_BUTTON_CLICKED, TDN_CREATED,
        TDN_DESTROYED, TDN_TIMER,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageW, SetWindowPos, HWND_TOPMOST, IDCANCEL, IDOK, SWP_NOMOVE, SWP_NOSIZE,
    };

    const AUTO_CLOSE_AFTER: Duration = Duration::from_millis(1_600);
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
    const MARQUEE_INTERVAL_MS: isize = 30;
    const OPERATION_RUNNING: usize = 0;
    const OPERATION_CANCEL_REQUESTED: usize = 1;
    const OPERATION_COMMITTING: usize = 2;
    const CANCEL_PENDING_STATUS: &str =
        "Cancel requested. Finishing the current metadata scrub before stopping…";

    #[derive(Debug)]
    enum StartupState {
        Pending,
        Ready,
        Failed(String),
    }

    #[derive(Debug, Clone)]
    enum ProgressMode {
        Marquee,
        Determinate { current: usize, total: usize },
    }

    #[derive(Debug, Clone)]
    struct ProgressSnapshot {
        content: String,
        mode: ProgressMode,
    }

    #[derive(Debug)]
    struct SharedState {
        startup: Mutex<StartupState>,
        startup_changed: Condvar,
        /// Worker threads only publish snapshots; the Task Dialog callback
        /// applies them on the dialog's own UI thread.
        progress: Mutex<ProgressSnapshot>,
        progress_version: AtomicU64,
        rendered_version: AtomicU64,
        finished: AtomicBool,
        button_enabled: AtomicBool,
        cancel_button_disabled: AtomicBool,
        operation_state: AtomicUsize,
        finish_close_at_ms: AtomicU64,
        close_sent: AtomicBool,
        /// Counts public handles only; the UI thread owns a separate Arc.
        public_handles: AtomicUsize,
        abandoned: AtomicBool,
    }

    /// Handle for a native, centered Windows Task Dialog used by headless Safe
    /// Copy. Clones can safely be shared with the scrub progress observer.
    #[derive(Debug)]
    pub struct SafeCopyProgressDialog {
        shared: Arc<SharedState>,
    }

    impl Clone for SafeCopyProgressDialog {
        fn clone(&self) -> Self {
            self.shared.public_handles.fetch_add(1, Ordering::Relaxed);
            Self {
                shared: Arc::clone(&self.shared),
            }
        }
    }

    impl SafeCopyProgressDialog {
        /// Open the native dialog on a dedicated UI thread and wait until
        /// Windows has created it before returning the update handle.
        pub fn start() -> Result<Self, String> {
            let shared = Arc::new(SharedState {
                startup: Mutex::new(StartupState::Pending),
                startup_changed: Condvar::new(),
                progress: Mutex::new(ProgressSnapshot {
                    content: "Preparing Safe Copy…".to_string(),
                    mode: ProgressMode::Marquee,
                }),
                progress_version: AtomicU64::new(0),
                rendered_version: AtomicU64::new(0),
                finished: AtomicBool::new(false),
                button_enabled: AtomicBool::new(false),
                cancel_button_disabled: AtomicBool::new(false),
                operation_state: AtomicUsize::new(OPERATION_RUNNING),
                finish_close_at_ms: AtomicU64::new(0),
                close_sent: AtomicBool::new(false),
                public_handles: AtomicUsize::new(1),
                abandoned: AtomicBool::new(false),
            });

            let ui_state = Arc::clone(&shared);
            thread::Builder::new()
                .name("wc-safe-copy-progress".to_string())
                .spawn(move || run_dialog(ui_state))
                .map_err(|error| format!("could not start Safe Copy progress window: {error}"))?;

            let deadline = Instant::now() + STARTUP_TIMEOUT;
            let mut startup = shared.startup.lock().map_err(|_| {
                "Safe Copy progress window startup state is unavailable".to_string()
            })?;
            loop {
                match &*startup {
                    StartupState::Ready => {
                        drop(startup);
                        return Ok(Self { shared });
                    }
                    StartupState::Failed(error) => return Err(error.clone()),
                    StartupState::Pending => {}
                }

                let now = Instant::now();
                if now >= deadline {
                    shared.abandoned.store(true, Ordering::Release);
                    drop(startup);
                    return Err(
                        "Windows did not create the Safe Copy progress window in time".into(),
                    );
                }
                let timeout = deadline.saturating_duration_since(now);
                let (next, result) = shared
                    .startup_changed
                    .wait_timeout(startup, timeout)
                    .map_err(|_| {
                        "Safe Copy progress window startup state is unavailable".to_string()
                    })?;
                startup = next;
                if result.timed_out() && matches!(&*startup, StartupState::Pending) {
                    shared.abandoned.store(true, Ordering::Release);
                    drop(startup);
                    return Err(
                        "Windows did not create the Safe Copy progress window in time".into(),
                    );
                }
            }
        }

        /// Show a generic preparation phase with an indeterminate progress
        /// bar. Callers must not pass file names or paths.
        pub fn set_phase(&self, status: impl Into<String>) {
            let status = safe_status(status.into(), "Preparing Safe Copy…");
            let Ok(mut progress) = self.shared.progress.lock() else {
                return;
            };
            if self.shared.finished.load(Ordering::Acquire) {
                return;
            }
            progress.content = if self.is_cancelled() {
                CANCEL_PENDING_STATUS.to_string()
            } else {
                status
            };
            progress.mode = ProgressMode::Marquee;
            self.shared.progress_version.fetch_add(1, Ordering::Release);
        }

        /// Show the scrubber's real file-count progress without exposing its
        /// per-file notification payload. A zero total remains indeterminate.
        pub fn set_scrub_progress(
            &self,
            current: usize,
            total: usize,
            selected_index: usize,
            selected_total: usize,
        ) {
            let current = current.min(total);
            let selected_index = selected_index.min(selected_total);
            let status = if total == 0 {
                format!(
                    "Checking metadata for selected item {} of {}…",
                    selected_index, selected_total
                )
            } else {
                format!(
                    "Scrubbing metadata: {} of {} files. Selected item {} of {}.",
                    current, total, selected_index, selected_total
                )
            };

            let Ok(mut progress) = self.shared.progress.lock() else {
                return;
            };
            if self.shared.finished.load(Ordering::Acquire) {
                return;
            }
            if total == 0 {
                progress.mode = ProgressMode::Marquee;
            } else {
                progress.mode = ProgressMode::Determinate { current, total };
            }
            progress.content = if self.is_cancelled() {
                CANCEL_PENDING_STATUS.to_string()
            } else {
                status
            };
            self.shared.progress_version.fetch_add(1, Ordering::Release);
        }

        /// Returns true after the user requests cancellation. A running
        /// metadata scrub request cannot be interrupted, so its caller checks
        /// this immediately after the request returns.
        pub fn is_cancelled(&self) -> bool {
            self.shared.operation_state.load(Ordering::Acquire) == OPERATION_CANCEL_REQUESTED
        }

        /// Atomically prevent new cancellation requests before the clipboard
        /// commit begins. Cancellation wins if it was requested first.
        pub fn begin_commit(&self) -> bool {
            if begin_commit_operation(&self.shared.operation_state) {
                self.set_phase("Finalizing the clean clipboard…");
                true
            } else {
                false
            }
        }

        /// Show the terminal result briefly, then close the native dialog.
        pub fn finish(&self, succeeded: bool, status: impl Into<String>) {
            let fallback = if succeeded {
                "Safe Copy is ready. You can paste the cleaned files now."
            } else {
                "Safe Copy could not finish. Please try again."
            };
            let status = safe_status(status.into(), fallback);

            if self.shared.finished.swap(true, Ordering::AcqRel) {
                return;
            }
            let Ok(mut progress) = self.shared.progress.lock() else {
                return;
            };
            progress.mode = if succeeded {
                ProgressMode::Determinate {
                    current: 1,
                    total: 1,
                }
            } else if matches!(progress.mode, ProgressMode::Marquee) {
                ProgressMode::Determinate {
                    current: 0,
                    total: 1,
                }
            } else {
                progress.mode.clone()
            };
            progress.content = status;
            self.shared.progress_version.fetch_add(1, Ordering::Release);
            self.shared.finish_close_at_ms.store(
                now_ms().saturating_add(AUTO_CLOSE_AFTER.as_millis() as u64),
                Ordering::Release,
            );
        }
    }

    impl Drop for SafeCopyProgressDialog {
        fn drop(&mut self) {
            if self.shared.public_handles.fetch_sub(1, Ordering::AcqRel) == 1 {
                self.finish(false, "Safe Copy ended before the clipboard was ready.");
            }
        }
    }

    fn run_dialog(shared: Arc<SharedState>) {
        let title = wide("WinCommander Safe Copy");
        let instruction = wide("Preparing a safe copy");
        let content = wide("Preparing Safe Copy…");
        let mut config = TASKDIALOGCONFIG::default();
        config.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
        config.hwndParent = std::ptr::null_mut();
        config.dwFlags = TDF_SHOW_PROGRESS_BAR | TDF_SHOW_MARQUEE_PROGRESS_BAR | TDF_CALLBACK_TIMER;
        config.dwCommonButtons = TDCBF_OK_BUTTON | TDCBF_CANCEL_BUTTON;
        config.pszWindowTitle = title.as_ptr();
        config.pszMainInstruction = instruction.as_ptr();
        config.pszContent = content.as_ptr();
        config.pfCallback = Some(task_dialog_callback);
        config.lpCallbackData = Arc::as_ptr(&shared) as isize;

        let result = unsafe {
            TaskDialogIndirect(
                &config,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };

        if result < 0 {
            let message = format!(
                "Windows could not open the Safe Copy progress dialog (0x{:08X})",
                result as u32
            );
            if let Ok(mut startup) = shared.startup.lock() {
                if matches!(*startup, StartupState::Pending) {
                    *startup = StartupState::Failed(message);
                }
            }
        }
        shared.startup_changed.notify_all();
    }

    unsafe extern "system" fn task_dialog_callback(
        hwnd: HWND,
        notification: u32,
        wparam: WPARAM,
        _lparam: LPARAM,
        callback_data: isize,
    ) -> windows_sys::core::HRESULT {
        if callback_data == 0 {
            return S_OK;
        }
        let shared = unsafe { &*(callback_data as *const SharedState) };

        match notification {
            value if value == TDN_CREATED as u32 => {
                unsafe {
                    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                }
                set_progress_mode(hwnd, true);
                unsafe {
                    SendMessageW(hwnd, TDM_ENABLE_BUTTON as u32, IDOK as WPARAM, 0);
                }
                if let Ok(mut startup) = shared.startup.lock() {
                    *startup = StartupState::Ready;
                }
                shared.startup_changed.notify_all();

                // If the caller timed out waiting for window creation, do
                // not leave an unmanaged modal behind.
                if shared.abandoned.load(Ordering::Acquire) {
                    request_close(hwnd, shared);
                }
            }
            value if value == TDN_DESTROYED as u32 => {
                shared.startup_changed.notify_all();
            }
            value if value == TDN_BUTTON_CLICKED as u32 => {
                if wparam == IDCANCEL as WPARAM && !shared.finished.load(Ordering::Acquire) {
                    let _ = request_cancel(&shared.operation_state);
                    let operation = shared.operation_state.load(Ordering::Acquire);
                    if operation == OPERATION_CANCEL_REQUESTED {
                        if let Ok(mut progress) = shared.progress.lock() {
                            progress.content = CANCEL_PENDING_STATUS.to_string();
                            shared.progress_version.fetch_add(1, Ordering::Release);
                        }
                    }
                    if operation != OPERATION_RUNNING {
                        unsafe {
                            SendMessageW(hwnd, TDM_ENABLE_BUTTON as u32, IDCANCEL as WPARAM, 0);
                        }
                        shared.cancel_button_disabled.store(true, Ordering::Release);
                    }
                    return S_FALSE;
                }
                if !shared.finished.load(Ordering::Acquire) {
                    return S_FALSE;
                }
            }
            value if value == TDN_TIMER as u32 => {
                let version = shared.progress_version.load(Ordering::Acquire);
                if version != shared.rendered_version.load(Ordering::Relaxed) {
                    if let Ok(progress) = shared.progress.lock() {
                        render_progress(hwnd, &progress);
                        shared.rendered_version.store(version, Ordering::Release);
                    }
                }

                if shared.finished.load(Ordering::Acquire)
                    && !shared.button_enabled.swap(true, Ordering::AcqRel)
                {
                    unsafe {
                        SendMessageW(hwnd, TDM_ENABLE_BUTTON as u32, IDOK as WPARAM, 1);
                    }
                }

                if shared.operation_state.load(Ordering::Acquire) != OPERATION_RUNNING
                    && !shared.cancel_button_disabled.swap(true, Ordering::AcqRel)
                {
                    unsafe {
                        SendMessageW(hwnd, TDM_ENABLE_BUTTON as u32, IDCANCEL as WPARAM, 0);
                    }
                }

                let close_at = shared.finish_close_at_ms.load(Ordering::Acquire);
                if close_at != 0
                    && now_ms() >= close_at
                    && !shared.close_sent.swap(true, Ordering::AcqRel)
                {
                    unsafe {
                        SendMessageW(hwnd, TDM_CLICK_BUTTON as u32, IDOK as WPARAM, 0);
                    }
                }
            }
            _ => {}
        }
        S_OK
    }

    fn set_progress_mode(hwnd: HWND, marquee: bool) {
        unsafe {
            if marquee {
                // Change the hosted control's style before starting its
                // animation; otherwise the first marquee request can be
                // ignored while the bar is still in determinate mode.
                SendMessageW(hwnd, TDM_SET_MARQUEE_PROGRESS_BAR as u32, 1, 0);
                SendMessageW(
                    hwnd,
                    TDM_SET_PROGRESS_BAR_MARQUEE as u32,
                    1,
                    MARQUEE_INTERVAL_MS,
                );
            } else {
                SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_MARQUEE as u32, 0, 0);
                SendMessageW(hwnd, TDM_SET_MARQUEE_PROGRESS_BAR as u32, 0, 0);
            }
        }
    }

    fn render_progress(hwnd: HWND, progress: &ProgressSnapshot) {
        match &progress.mode {
            ProgressMode::Marquee => set_progress_mode(hwnd, true),
            ProgressMode::Determinate { current, total } => {
                set_progress_mode(hwnd, false);
                let position =
                    ((*current as u128 * 100) / (*total).max(1) as u128).min(100) as usize;
                unsafe {
                    SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_POS as u32, position, 0);
                }
            }
        }
        update_content(hwnd, &progress.content);
    }

    fn update_content(hwnd: HWND, content: &str) {
        let content = wide(content);
        unsafe {
            // SendMessageW is synchronous, so the temporary UTF-16 buffer
            // remains alive until the Task Dialog has copied the text.
            SendMessageW(
                hwnd,
                TDM_UPDATE_ELEMENT_TEXT as u32,
                TDE_CONTENT as WPARAM,
                content.as_ptr() as LPARAM,
            );
        }
    }

    fn request_close(hwnd: HWND, shared: &SharedState) {
        shared.finished.store(true, Ordering::Release);
        shared.button_enabled.store(true, Ordering::Release);
        unsafe {
            SendMessageW(hwnd, TDM_ENABLE_BUTTON as u32, IDOK as WPARAM, 1);
            SendMessageW(hwnd, TDM_CLICK_BUTTON as u32, IDOK as WPARAM, 0);
        }
    }

    fn safe_status(status: String, fallback: &str) -> String {
        let status = status.trim();
        if status.is_empty()
            || status.contains(['\\', '/', '\0'])
            || status.chars().any(char::is_control)
        {
            return fallback.to_string();
        }

        let compact = status.split_whitespace().collect::<Vec<_>>().join(" ");
        let compact = compact.chars().take(240).collect::<String>();
        if compact.is_empty() {
            fallback.to_string()
        } else {
            compact
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
            .unwrap_or(0)
    }

    fn request_cancel(operation_state: &AtomicUsize) -> bool {
        operation_state
            .compare_exchange(
                OPERATION_RUNNING,
                OPERATION_CANCEL_REQUESTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn begin_commit_operation(operation_state: &AtomicUsize) -> bool {
        match operation_state.compare_exchange(
            OPERATION_RUNNING,
            OPERATION_COMMITTING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) | Err(OPERATION_COMMITTING) => true,
            Err(OPERATION_CANCEL_REQUESTED) => false,
            Err(_) => false,
        }
    }

    #[cfg(test)]
    mod operation_state_tests {
        use super::{
            begin_commit_operation, request_cancel, OPERATION_CANCEL_REQUESTED,
            OPERATION_COMMITTING, OPERATION_RUNNING,
        };
        use std::sync::atomic::AtomicUsize;

        #[test]
        fn a_cancel_request_prevents_clipboard_commit() {
            let state = AtomicUsize::new(OPERATION_RUNNING);
            assert!(request_cancel(&state));
            assert!(!begin_commit_operation(&state));
            assert_eq!(
                state.load(std::sync::atomic::Ordering::Acquire),
                OPERATION_CANCEL_REQUESTED
            );
        }

        #[test]
        fn clipboard_commit_rejects_late_cancellation() {
            let state = AtomicUsize::new(OPERATION_RUNNING);
            assert!(begin_commit_operation(&state));
            assert!(!request_cancel(&state));
            assert_eq!(
                state.load(std::sync::atomic::Ordering::Acquire),
                OPERATION_COMMITTING
            );
        }
    }

    pub use SafeCopyProgressDialog as Dialog;
}

#[cfg(windows)]
pub use platform::Dialog as SafeCopyProgressDialog;

#[cfg(not(windows))]
#[derive(Debug, Clone, Default)]
pub struct SafeCopyProgressDialog;

#[cfg(not(windows))]
impl SafeCopyProgressDialog {
    pub fn start() -> Result<Self, String> {
        Ok(Self)
    }

    pub fn set_phase(&self, _status: impl Into<String>) {}

    pub fn is_cancelled(&self) -> bool {
        false
    }

    pub fn begin_commit(&self) -> bool {
        true
    }

    pub fn set_scrub_progress(
        &self,
        _current: usize,
        _total: usize,
        _selected_index: usize,
        _selected_total: usize,
    ) {
    }

    pub fn finish(&self, _succeeded: bool, _status: impl Into<String>) {}
}

#[cfg(all(test, windows))]
mod manual_smoke_test {
    use super::SafeCopyProgressDialog;

    /// Manual visual check: starts the real Windows Task Dialog, sends two
    /// scrub-progress updates, and closes after showing the terminal result.
    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn safe_copy_progress_dialog_opens_and_updates() {
        let dialog = SafeCopyProgressDialog::start().expect("Task Dialog should be created");
        dialog.set_scrub_progress(1, 4, 1, 1);
        std::thread::sleep(std::time::Duration::from_millis(250));
        dialog.set_scrub_progress(2, 4, 1, 1);
        dialog.finish(true, "Safe Copy is ready to paste.");
        std::thread::sleep(std::time::Duration::from_millis(1_900));
    }
}
