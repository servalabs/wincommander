//! Bounded best-effort extraction for legacy OLE Office documents.
//!
//! Classic `.doc`, `.xls`, and `.ppt` files use the Compound File Binary
//! Format.  This deliberately does not try to interpret every application
//! record: it validates the container marker, refuses encryption markers, and
//! returns only bounded printable ANSI/UTF-16 runs.  That is enough for
//! discovery without pretending an encrypted or malformed legacy file was
//! indexed completely.

use std::io::Read;
use std::path::Path;

use crate::error::{Result, SearchError};

const COMPOUND_FILE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MAX_LEGACY_FILE_BYTES: usize = 32 * 1024 * 1024;
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

    let mut runs = printable_ansi_runs(&bytes);
    runs.extend(printable_utf16le_runs(&bytes));
    runs.sort();
    runs.dedup();
    if runs.is_empty() {
        return Err(SearchError::Extract(
            "legacy Office file contains no readable text".into(),
        ));
    }
    Ok(runs.join("\n"))
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

fn flush_run(out: &mut Vec<String>, run: &mut Vec<u8>) {
    if run.len() >= MIN_RUN_CHARS {
        let text = String::from_utf8_lossy(run).trim().to_owned();
        if text.len() >= MIN_RUN_CHARS {
            out.push(text);
        }
    }
    run.clear();
}

fn printable_ansi_runs(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = Vec::new();
    for &byte in bytes {
        if is_printable(byte) {
            run.push(byte);
        } else {
            flush_run(&mut out, &mut run);
        }
    }
    flush_run(&mut out, &mut run);
    out
}

fn printable_utf16le_runs(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        let low = bytes[index];
        let high = bytes[index + 1];
        if high == 0 && is_printable(low) {
            run.push(low);
            index += 2;
        } else {
            flush_run(&mut out, &mut run);
            index += 1;
        }
    }
    flush_run(&mut out, &mut run);
    out
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
}
