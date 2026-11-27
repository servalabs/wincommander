// SPDX-License-Identifier: AGPL-3.0-or-later
// Content-based file type detection for the Share Safely report. The table
// lives in the wincmd-filetype crate, which the Pro cleaner also uses.

pub use wincmd_filetype::{
    carries_no_hidden_metadata, detect_file, is_mismatch, is_plain_text_file,
    text_carries_no_hidden_metadata,
};
