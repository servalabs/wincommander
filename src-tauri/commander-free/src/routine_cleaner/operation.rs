// SPDX-License-Identifier: AGPL-3.0-or-later
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub(super) struct Guard;

pub(super) fn begin() -> Result<Guard, String> {
    ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "A cleanup scan or action is already running.".to_string())?;
    super::CANCELLED.store(false, Ordering::Release);
    Ok(Guard)
}

impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
