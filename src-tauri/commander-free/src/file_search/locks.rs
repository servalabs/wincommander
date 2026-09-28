// SPDX-License-Identifier: AGPL-3.0-or-later
pub(super) const INDEX_REPLACEMENT_LOCK_NAME: &str = "WinCommander_ContentIndex_replacement_lock";
pub(super) const INDEX_REPLACEMENT_LOCK_TIMEOUT_MS: u32 = 15_000;

pub(super) fn abandoned_index_lock_error() -> String {
    "content index lock was abandoned; repair or rescan is required before reading the index"
        .to_string()
}

/// Cross-process gate for destructive index replacement. Searches do not use
/// this mutex: Tantivy readers are safe while the normal index writer commits,
/// and serializing readers made the search UI fail under ordinary concurrency.
pub(super) struct IndexReplacementLock {
    #[cfg(windows)]
    handle: isize,
}

impl IndexReplacementLock {
    #[cfg(windows)]
    pub(super) fn acquire() -> Result<Self, String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            CreateMutexW, ReleaseMutex, WaitForSingleObject,
        };

        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_ABANDONED: u32 = 0x80;
        const WAIT_TIMEOUT: u32 = 0x102;

        let name: Vec<u16> = format!("{INDEX_REPLACEMENT_LOCK_NAME}\0")
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err("content index lock is unavailable; refusing the operation".to_string());
        }
        match unsafe { WaitForSingleObject(handle, INDEX_REPLACEMENT_LOCK_TIMEOUT_MS) } {
            WAIT_OBJECT_0 => Ok(Self {
                handle: handle as isize,
            }),
            // WAIT_ABANDONED transfers mutex ownership to this process, but
            // also proves the prior holder exited mid-operation. Release that
            // ownership before closing the handle and refuse to read a
            // potentially half-replaced Tantivy directory.
            WAIT_ABANDONED => {
                unsafe {
                    ReleaseMutex(handle);
                    CloseHandle(handle);
                }
                Err(abandoned_index_lock_error())
            }
            WAIT_TIMEOUT => {
                unsafe { CloseHandle(handle) };
                Err(format!(
                    "content index replacement is already running; try again after it finishes ({} ms timeout)",
                    INDEX_REPLACEMENT_LOCK_TIMEOUT_MS
                ))
            }
            _ => {
                unsafe { CloseHandle(handle) };
                Err("content index lock wait failed; refusing the operation".to_string())
            }
        }
    }

    #[cfg(not(windows))]
    pub(super) fn acquire() -> Result<Self, String> {
        // The Windows-only index directory is only enforced on Windows; this
        // keeps compile-only non-Windows targets from claiming runtime safety.
        Ok(Self {})
    }
}

impl Drop for IndexReplacementLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.handle != 0 {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            unsafe {
                ReleaseMutex(self.handle as _);
                CloseHandle(self.handle as _);
            }
        }
    }
}
