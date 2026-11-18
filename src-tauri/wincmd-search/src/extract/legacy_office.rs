//! Bounded best-effort extraction for legacy OLE Office documents.
//!
//! Classic `.doc`, `.xls`, and `.ppt` files use the Compound File Binary
//! Format.  This deliberately does not try to interpret every application
//! record: it checks the container marker and some encryption markers, then
//! returns bounded printable ANSI/UTF-16 runs. The marker check is incomplete:
//! a successful result does not prove that the file is valid or unencrypted.

use std::io::Read;
use std::path::Path;

use crate::error::{Result, SearchError};

const COMPOUND_FILE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MAX_LEGACY_FILE_BYTES: usize = 32 * 1024 * 1024;
const MIN_RUN_CHARS: usize = 4;

/// Extract searchable text from a classic OLE Office file without executing or
/// expanding embedded objects. Some encrypted formats are not detected.
pub fn extract_legacy_office(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(1024 * 1024);
    file.by_ref()
        .take((MAX_LEGACY_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_LEGACY_FILE_BYTES {
        return Err(SearchError::Extract(
            "legacy Office file exceeds 32 MiB extraction limit".into(),
        ));
    }
    if !bytes.starts_with(&COMPOUND_FILE_MAGIC) {
        return Err(SearchError::Extract(
            "legacy Office file is not a Compound File container".into(),
        ));
    }
    if has_encryption_marker(&bytes) {
        return Err(SearchError::Unsupported(
            "encrypted legacy Office input".into(),
        ));
    }

    let mut runs = Vec::new();
    printable_ansi_runs(&bytes, &mut runs);
    printable_utf16le_runs(&bytes, &mut runs);
    runs.sort_unstable_by(|a, b| a.bytes(&bytes).cmp(b.bytes(&bytes)));
    runs.dedup_by(|a, b| a.bytes(&bytes).eq(b.bytes(&bytes)));
    if runs.is_empty() {
        return Err(SearchError::Extract(
            "legacy Office file contains no readable text".into(),
        ));
    }
    let capacity = runs.iter().map(|run| run.len as usize).sum::<usize>() + runs.len() - 1;
    let mut text = String::with_capacity(capacity);
    for (index, run) in runs.iter().enumerate() {
        if index != 0 {
            text.push('\n');
        }
        text.extend(run.bytes(&bytes).map(char::from));
    }
    Ok(text)
}

fn has_encryption_marker(bytes: &[u8]) -> bool {
    [
        b"EncryptedPackage".as_slice(),
        b"EncryptionInfo".as_slice(),
        b"StrongEncryptionDataSpace".as_slice(),
    ]
    .iter()
    .any(|needle| bytes.windows(needle.len()).any(|window| window == *needle))
}

fn is_printable(byte: u8) -> bool {
    matches!(byte, b' '..=b'~' | b'\t')
}

// Offsets point into the bounded input, avoiding a heap allocation per run.
#[derive(Clone, Copy)]
struct TextRun {
    start: u32,
    len: u32,
    stride: u8,
}

impl TextRun {
    fn bytes<'a>(&self, source: &'a [u8]) -> impl Iterator<Item = u8> + 'a {
        let start = self.start as usize;
        let len = self.len as usize;
        let stride = self.stride as usize;
        (0..len).map(move |index| source[start + index * stride])
    }
}

fn record_run(source: &[u8], mut start: usize, end: usize, stride: usize, runs: &mut Vec<TextRun>) {
    let mut len = (end - start) / stride;
    while len > 0 && source[start].is_ascii_whitespace() {
        start += stride;
        len -= 1;
    }
    while len > 0 && source[start + (len - 1) * stride].is_ascii_whitespace() {
        len -= 1;
    }
    if len >= MIN_RUN_CHARS {
        // The input is capped at 32 MiB, so offsets and lengths fit u32.
        runs.push(TextRun {
            start: start as u32,
            len: len as u32,
            stride: stride as u8,
        });
    }
}

fn printable_ansi_runs(bytes: &[u8], runs: &mut Vec<TextRun>) {
    let mut start = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if !is_printable(byte) {
            record_run(bytes, start, index, 1, runs);
            start = index + 1;
        }
    }
    record_run(bytes, start, bytes.len(), 1, runs);
}

fn printable_utf16le_runs(bytes: &[u8], runs: &mut Vec<TextRun>) {
    let mut start = 0;
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index + 1] == 0 && is_printable(bytes[index]) {
            index += 2;
        } else {
            record_run(bytes, start, index, 2, runs);
            index += 1;
            start = index;
        }
    }
    record_run(bytes, start, index, 2, runs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn compound_file(contents: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(".doc").unwrap();
        file.write_all(&COMPOUND_FILE_MAGIC).unwrap();
        file.write_all(contents).unwrap();
        file
    }

    #[test]
    fn extracts_ansi_and_utf16_runs_without_parsing_untrusted_records() {
        let mut bytes = b"\0supplier termination agreement\0".to_vec();
        bytes.extend(
            "legacy spreadsheet text"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        let file = compound_file(&bytes);
        let text = extract_legacy_office(file.path()).unwrap();
        assert!(text.contains("supplier termination agreement"));
        assert!(text.contains("legacy spreadsheet text"));
    }

    #[test]
    fn encrypted_legacy_input_is_explicitly_skipped() {
        let file = compound_file(b"\0EncryptionInfo\0secret");
        let err = extract_legacy_office(file.path()).unwrap_err();
        assert!(err.is_unsupported());
        assert!(err.to_string().contains("encrypted"));
    }

    #[test]
    fn non_compound_input_is_not_misrepresented_as_legacy_office() {
        let mut file = NamedTempFile::with_suffix(".doc").unwrap();
        file.write_all(b"not a compound file").unwrap();
        assert!(matches!(
            extract_legacy_office(file.path()),
            Err(SearchError::Extract(_))
        ));
    }
    #[test]
    fn many_duplicate_runs_are_returned_once() {
        let file = compound_file(&b"same\0".repeat(100_001));
        assert_eq!(extract_legacy_office(file.path()).unwrap(), "same");
    }

    #[test]
    fn finds_utf16_at_odd_offset_between_high_bytes() {
        let file = compound_file(b"\xffh\0e\0l\0l\0o\0\xff");
        assert_eq!(extract_legacy_office(file.path()).unwrap(), "hello");
    }

    #[test]
    fn mixed_encodings_trim_sort_and_deduplicate_as_before() {
        let mut contents = b" zulu \0alpha\t\0".to_vec();
        contents.extend("alpha".encode_utf16().flat_map(u16::to_le_bytes));
        contents.extend_from_slice(b"\0\0  abc  \0");
        let file = compound_file(&contents);
        assert_eq!(extract_legacy_office(file.path()).unwrap(), "alpha\nzulu");
    }

    #[test]
    fn many_unique_runs_are_not_silently_truncated() {
        let mut bytes = Vec::new();
        for index in 0..100_001 {
            bytes.extend_from_slice(format!("term{index:06}\0").as_bytes());
        }
        let file = compound_file(&bytes);
        let result = extract_legacy_office(file.path()).unwrap();
        assert_eq!(result.lines().count(), 100_001);
        assert!(result.starts_with("term000000\n"));
        assert!(result.ends_with("term100000"));
    }

    #[test]
    fn input_size_limit_still_accepts_exact_boundary_and_rejects_one_byte_more() {
        let contents = vec![b'x'; MAX_LEGACY_FILE_BYTES - COMPOUND_FILE_MAGIC.len()];
        let file = compound_file(&contents);
        assert_eq!(
            extract_legacy_office(file.path()).unwrap().len(),
            contents.len()
        );
        drop(file);
        let file = compound_file(&[contents, vec![b'x']].concat());
        assert!(matches!(
            extract_legacy_office(file.path()),
            Err(SearchError::Extract(_))
        ));
    }
}
