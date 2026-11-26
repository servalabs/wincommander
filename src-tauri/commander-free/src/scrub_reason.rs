// SPDX-License-Identifier: AGPL-3.0-or-later
// Outcome and reason codes for one file in a Share Safely run.

use crate::scrub_content_type as content;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const MAX_PATH_CHARS: usize = 260;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrubOutcome {
    Cleaned,
    AlreadyClean,
    Residual,
    NotCleaned,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrubReason {
    UnsupportedFormat,
    NothingToRemove,
    ContentMismatch,
    Locked,
    AccessDenied,
    ReadFailed,
    WriteFailed,
    EmptyFile,
    TooLongPath,
}

/// What Free could observe about a file the engine did not clean.
#[derive(Debug, Clone, Copy, Default)]
pub struct Probe {
    pub len: Option<u64>,
    pub open_os_error: Option<i32>,
    pub path_chars: usize,
}

impl Probe {
    pub fn of(path: &Path) -> Self {
        Probe {
            len: fs::metadata(path).ok().map(|m| m.len()),
            open_os_error: fs::File::open(path)
                .err()
                .map(|e| e.raw_os_error().unwrap_or(-1)),
            path_chars: path.as_os_str().len(),
        }
    }
}

/// Reason a path cannot be opened, from the Windows error code.
pub fn access_reason(os_error: Option<i32>, path_chars: usize) -> Option<ScrubReason> {
    Some(match os_error? {
        32 | 33 => ScrubReason::Locked,
        5 => ScrubReason::AccessDenied,
        206 => ScrubReason::TooLongPath,
        2 | 3 if path_chars >= MAX_PATH_CHARS => ScrubReason::TooLongPath,
        _ => ScrubReason::ReadFailed,
    })
}

/// A file the engine skipped or never reported.
pub fn classify_unprocessed(
    ext: &str,
    detected: Option<&str>,
    probe: Probe,
) -> (ScrubOutcome, ScrubReason) {
    if let Some(reason) = access_reason(probe.open_os_error, probe.path_chars) {
        return (ScrubOutcome::Failed, reason);
    }
    if probe.len == Some(0) {
        return (ScrubOutcome::AlreadyClean, ScrubReason::EmptyFile);
    }
    if content::is_mismatch(ext, detected) {
        return (ScrubOutcome::NotCleaned, ScrubReason::ContentMismatch);
    }
    if content::carries_no_hidden_metadata(ext) {
        return (ScrubOutcome::AlreadyClean, ScrubReason::NothingToRemove);
    }
    (ScrubOutcome::NotCleaned, ScrubReason::UnsupportedFormat)
}

/// A file the engine tried and failed on, from its message plus Free's probe.
pub fn classify_error(message: &str, ext: &str, probe: Probe) -> (ScrubOutcome, ScrubReason) {
    if probe.len == Some(0) {
        return (ScrubOutcome::AlreadyClean, ScrubReason::EmptyFile);
    }
    if let Some(reason) = access_reason(probe.open_os_error, probe.path_chars) {
        return (ScrubOutcome::Failed, reason);
    }
    let m = message.to_ascii_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    let reason = if any(&[
        "os error 32",
        "os error 33",
        "being used by another process",
    ]) {
        ScrubReason::Locked
    } else if any(&["os error 5)", "access is denied", "permission denied"]) {
        ScrubReason::AccessDenied
    } else if any(&["os error 206", "too long"]) {
        ScrubReason::TooLongPath
    } else if m.contains("can't write") {
        return if content::carries_no_hidden_metadata(ext) {
            (ScrubOutcome::AlreadyClean, ScrubReason::NothingToRemove)
        } else {
            (ScrubOutcome::NotCleaned, ScrubReason::UnsupportedFormat)
        };
    } else if any(&[
        "error opening file",
        "wildcards don't work",
        "file not found",
        "format error",
    ]) {
        ScrubReason::ReadFailed
    } else {
        ScrubReason::WriteFailed
    };
    (ScrubOutcome::Failed, reason)
}

#[cfg(test)]
#[path = "scrub_reason_tests.rs"]
mod tests;
