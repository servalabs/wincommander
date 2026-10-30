// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);
pub(super) static CANCELLED: AtomicBool = AtomicBool::new(false);
pub(super) static FILES: AtomicU64 = AtomicU64::new(0);
pub(super) static FOLDERS: AtomicU64 = AtomicU64::new(0);
pub(super) static BYTES: AtomicU64 = AtomicU64::new(0);
pub(super) static SKIPPED: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    running: bool,
    files_scanned: u64,
    folders_scanned: u64,
    bytes_scanned: u64,
    skipped_entries: u64,
    cancelled: bool,
}

pub(super) struct ScanGuard;

impl ScanGuard {
    pub(super) fn begin() -> Result<Self, String> {
        RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                "A disk scan is already running. Cancel it or wait for completion.".to_string()
            })?;
        CANCELLED.store(false, Ordering::Release);
        for counter in [&FILES, &FOLDERS, &BYTES, &SKIPPED] {
            counter.store(0, Ordering::Release);
        }
        Ok(Self)
    }
}

impl Drop for ScanGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

pub(super) fn cancel() {
    if RUNNING.load(Ordering::Acquire) {
        CANCELLED.store(true, Ordering::Release);
    }
}

pub(super) fn snapshot() -> ScanProgress {
    ScanProgress {
        running: RUNNING.load(Ordering::Acquire),
        files_scanned: FILES.load(Ordering::Acquire),
        folders_scanned: FOLDERS.load(Ordering::Acquire),
        bytes_scanned: BYTES.load(Ordering::Acquire),
        skipped_entries: SKIPPED.load(Ordering::Acquire),
        cancelled: CANCELLED.load(Ordering::Acquire),
    }
}
