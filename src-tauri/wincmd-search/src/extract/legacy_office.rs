//! Bounded best-effort extraction for legacy OLE Office documents.
//!
//! Classic `.doc`, `.xls`, and `.ppt` files use the Compound File Binary
//! Format.  This deliberately does not try to interpret every application
//! record: it validates the container marker, refuses encryption markers, and
//! returns only bounded printable ANSI/UTF-16 runs.  That is enough for
//! discovery without pretending an encrypted or malformed legacy file was
//! indexed completely.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use crate::error::{Result, SearchError};

const COMPOUND_FILE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MAX_LEGACY_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_LEGACY_UNIQUE_RUNS: usize = 100_000;
const MAX_LEGACY_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MIN_RUN_CHARS: usize = 4;

/// Extract searchable text from a classic OLE Office file without executing or
/// expanding embedded objects.  Encrypted input remains explicit and skipped.
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

    let mut runs = TextRuns::default();
    printable_ansi_runs(&bytes, &mut runs)?;
    printable_utf16le_runs(&bytes, &mut runs)?;
    if runs.values.is_empty() {
        return Err(SearchError::Extract(
            "legacy Office file contains no readable text".into(),
        ));
    }
    Ok(runs.values.into_iter().collect::<Vec<_>>().join("\n"))
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

#[derive(Default)]
struct TextRuns {
    values: BTreeSet<String>,
    bytes: usize,
}

impl TextRuns {
    fn flush(&mut self, run: &mut Vec<u8>) -> Result<()> {
        let decoded = String::from_utf8_lossy(run);
        let text = decoded.trim();
        if text.len() >= MIN_RUN_CHARS && !self.values.contains(text) {
            let bytes = self
                .bytes
                .saturating_add(text.len())
                .saturating_add(usize::from(!self.values.is_empty()));
            if self.values.len() >= MAX_LEGACY_UNIQUE_RUNS || bytes > MAX_LEGACY_TEXT_BYTES {
                return Err(SearchError::ExtractionLimit(
                    "legacy Office text exceeds extraction limit".into(),
                ));
            }
            self.values.insert(text.to_owned());
            self.bytes = bytes;
        }
        run.clear();
        Ok(())
    }
}

fn printable_ansi_runs(bytes: &[u8], out: &mut TextRuns) -> Result<()> {
    let mut run = Vec::new();
    for &byte in bytes {
        if is_printable(byte) {
            run.push(byte);
        } else {
            out.flush(&mut run)?;
        }
    }
    out.flush(&mut run)
}

fn printable_utf16le_runs(bytes: &[u8], out: &mut TextRuns) -> Result<()> {
    let mut run = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        let low = bytes[index];
        let high = bytes[index + 1];
        if high == 0 && is_printable(low) {
            run.push(low);
            index += 2;
        } else {
            out.flush(&mut run)?;
            index += 1;
        }
    }
    out.flush(&mut run)
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
    fn excessive_unique_runs_are_rejected_instead_of_partially_indexed() {
        let mut bytes = Vec::new();
        for index in 0..100_001 {
            bytes.extend_from_slice(format!("term{index:06}\0").as_bytes());
        }
        let file = compound_file(&bytes);
        let error = match extract_legacy_office(file.path()) {
            Ok(_) => panic!("excessive runs were accepted"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("legacy Office text exceeds extraction limit"));
    }

    #[test]
    fn text_budget_accepts_exact_limit_and_refuses_one_byte_more() {
        let mut runs = TextRuns::default();
        let mut at_limit = vec![b'x'; MAX_LEGACY_TEXT_BYTES];
        assert!(runs.flush(&mut at_limit).is_ok());
        assert_eq!(runs.bytes, MAX_LEGACY_TEXT_BYTES);
        let mut additional = b"other".to_vec();
        assert!(runs.flush(&mut additional).is_err());
        let mut too_long = vec![b'x'; MAX_LEGACY_TEXT_BYTES + 1];
        assert!(TextRuns::default().flush(&mut too_long).is_err());
    }
}
