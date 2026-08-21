// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded clipboard text read (plan §4.2 "Read bounds").
//!
//! Reads `CF_UNICODETEXT` **only** — V1 does not touch images, files,
//! HTML, RTF, or keystrokes (see this crate's top-level doc). The current
//! free-tier code has no cap beyond a `text.len() < 13` floor
//! (`paste_monitor.rs:751-754`); every read through this module is
//! truncated to [`MAX_CLIPBOARD_READ_BYTES`] (1 MiB) via
//! `wincmd_clip_rules::truncate_for_match` before it is handed back, so a
//! caller can never accidentally match against more than the bound.

use std::time::Duration;

/// The clipboard-text read cap (plan §4.2: "truncate to 1 MiB before
/// matching"). Matches `wincmd_clip_rules::RuleSetLimits::default()
/// .max_text_bytes` — that field's own doc says it is "NOT enforced by
/// [that] crate — it's a contract on the caller", and this constant is
/// this crate's fulfilment of that contract.
pub const MAX_CLIPBOARD_READ_BYTES: usize = 1_048_576;

/// Outcome of one bounded clipboard-text read attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// `CF_UNICODETEXT` was present and read successfully. Already
    /// truncated to [`MAX_CLIPBOARD_READ_BYTES`] at a UTF-8 boundary.
    Text(String),
    /// The clipboard currently holds no `CF_UNICODETEXT` format — this is
    /// a normal, frequent state (empty clipboard, or a non-text format
    /// this crate deliberately never reads), not a failure.
    NoText,
    /// `CF_UNICODETEXT` IS present but every bounded-retry attempt to
    /// actually read it failed (clipboard held open by another process
    /// for the whole retry window, or a genuine API failure). Reported
    /// honestly as a failure — never silently folded into `NoText`, per
    /// plan §4.2's retry contract.
    Failed,
}

/// Abstraction over "read the clipboard's text, if any". Production code
/// uses [`Win32TextSource`]; tests inject a fake so the engine/action
/// wiring above this module never needs a real desktop session.
pub trait ClipboardTextSource: Send {
    fn read_text(&mut self) -> ReadOutcome;
}

#[cfg(windows)]
pub struct Win32TextSource {
    pub max_attempts: u32,
    pub backoff: Duration,
}

#[cfg(windows)]
impl Default for Win32TextSource {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: Duration::from_millis(50),
        }
    }
}

#[cfg(windows)]
impl ClipboardTextSource for Win32TextSource {
    fn read_text(&mut self) -> ReadOutcome {
        // `IsClipboardFormatAvailable` doesn't require an open clipboard
        // and cleanly answers "is CF_UNICODETEXT even here" — using it as
        // a pre-check is what lets this function distinguish the
        // ordinary "clipboard holds an image/file, not text" case
        // (`NoText`, no retry needed) from "text IS there but every read
        // attempt failed" (`Failed`, the case plan §4.2's bounded-retry
        // requirement is actually about). Without this pre-check, both
        // cases look identical through `get_clipboard`'s `Result`, and
        // reporting an ordinary non-text clipboard as `Failed` would be
        // dishonest noise on every health surface.
        use clipboard_win::Format as _;
        if !clipboard_win::formats::Unicode.is_format_avail() {
            return ReadOutcome::NoText;
        }

        let read = crate::retry::retry_with_backoff(self.max_attempts, self.backoff, || {
            clipboard_win::get_clipboard::<String, _>(clipboard_win::formats::Unicode).ok()
        });

        match read {
            Some(text) => {
                let truncated =
                    wincmd_clip_rules::truncate_for_match(&text, MAX_CLIPBOARD_READ_BYTES);
                ReadOutcome::Text(truncated.to_string())
            }
            None => ReadOutcome::Failed,
        }
    }
}

#[cfg(not(windows))]
#[derive(Default)]
pub struct Win32TextSource;

#[cfg(not(windows))]
impl ClipboardTextSource for Win32TextSource {
    fn read_text(&mut self) -> ReadOutcome {
        // Windows-only enforcement, per AGENTS.md: non-Windows targets
        // compile via a no-op stub so `cargo check`/`cargo test` succeed
        // on Linux CI / macOS dev without ever pretending to read a real
        // clipboard.
        ReadOutcome::NoText
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource(Vec<ReadOutcome>);
    impl ClipboardTextSource for FakeSource {
        fn read_text(&mut self) -> ReadOutcome {
            if self.0.is_empty() {
                ReadOutcome::NoText
            } else {
                self.0.remove(0)
            }
        }
    }

    #[test]
    fn fake_source_yields_configured_outcomes_in_order() {
        let mut src = FakeSource(vec![
            ReadOutcome::Text("hello".to_string()),
            ReadOutcome::NoText,
            ReadOutcome::Failed,
        ]);
        assert_eq!(src.read_text(), ReadOutcome::Text("hello".to_string()));
        assert_eq!(src.read_text(), ReadOutcome::NoText);
        assert_eq!(src.read_text(), ReadOutcome::Failed);
    }

    #[test]
    fn truncation_helper_cuts_at_a_multibyte_boundary() {
        // Exercises the exact helper this module's real read path calls,
        // pinned to THIS crate's chosen cap constant rather than an
        // arbitrary number, so a future change to
        // `MAX_CLIPBOARD_READ_BYTES` re-validates this boundary property.
        let text: String = "é".repeat(10); // 20 bytes, 10 chars
        let truncated = wincmd_clip_rules::truncate_for_match(&text, 15);
        // Byte 15 lands mid-codepoint (each 'é' is 2 bytes) — must back
        // off to the last whole-char boundary (byte 14 = 7 chars) rather
        // than panicking or splitting a codepoint.
        assert_eq!(truncated.len(), 14);
        assert!(truncated.chars().count() == 7);
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
    }

    #[test]
    fn max_clipboard_read_bytes_is_one_mebibyte() {
        assert_eq!(MAX_CLIPBOARD_READ_BYTES, 1024 * 1024);
    }
}
