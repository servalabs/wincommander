use crate::error::Result;
use std::path::Path;

/// Extract text from plain-text and HTML files.
pub fn extract_plain(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let raw = decode_text_bytes(&bytes);
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == "html" || ext == "htm" {
        Ok(strip_html_tags(&raw))
    } else {
        Ok(raw)
    }
}

/// Longest header sample checked by the BOM-less UTF-16 heuristic.
const UTF16_HEURISTIC_SAMPLE_MAX: usize = 4096;

/// Decode raw file bytes to text. Handles UTF-8 (with or without BOM) and
/// UTF-16 (LE/BE, with or without BOM) — Windows text files are frequently
/// UTF-16LE (PowerShell 5 `Out-File`/`>` and Notepad "Unicode" both default to
/// it). Decoding a UTF-16LE file as UTF-8 leaves a NUL byte after every ASCII
/// char; the tokenizer splits on NUL, so the whole file indexes as
/// single-letter garbage without this.
pub(crate) fn decode_text_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16le(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16be(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    // KT: BOM-less UTF-16LE shows up from some Windows tools (older
    // redirection, some editors); sample the header for the NUL-alternation
    // pattern before assuming UTF-8, or the file indexes as single-letter
    // garbage (NUL is a tokenizer split point).
    let sample_len = bytes.len().min(UTF16_HEURISTIC_SAMPLE_MAX);
    match detect_bomless_utf16(&bytes[..sample_len]) {
        Some(true) => decode_utf16le(bytes),
        Some(false) => decode_utf16be(bytes),
        // Lossy UTF-8: a stray non-UTF-8 byte in a log/csv must not fail the
        // whole file — real words survive; invalid bytes become replacement
        // chars.
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// `Some(true)` = looks like BOM-less UTF-16LE, `Some(false)` = UTF-16BE,
/// `None` = neither (treat as UTF-8). ASCII text encoded as UTF-16LE alternates
/// [char-byte, 0x00]; UTF-16BE alternates [0x00, char-byte]. A >40% zero rate
/// on one parity with a mostly-nonzero rate on the other is decisive; real
/// UTF-8 text has ~0% zero bytes on either parity.
///
/// `pub(crate)`: also used by `extract::sniff_text` (the unknown-extension
/// path) so a BOM-less UTF-16 file with a novel suffix isn't misclassified as
/// binary and skipped — see that call site for why.
pub(crate) fn detect_bomless_utf16(sample: &[u8]) -> Option<bool> {
    let n = sample.len() - (sample.len() % 2);
    let pairs = n / 2;
    // Below this many pairs, a handful of coincidental zero bytes (common in
    // small binary blobs — padding, short opcodes) can cross the 40%
    // threshold purely by chance. `sniff_text` uses `None` here to mean
    // "binary, skip" for an unknown extension, so a false positive on a tiny
    // file would wrongly index binary junk as text instead of skipping it.
    const MIN_PAIRS_FOR_CONFIDENCE: usize = 16;
    if pairs < MIN_PAIRS_FOR_CONFIDENCE {
        return None;
    }
    let mut even_zero = 0usize;
    let mut odd_zero = 0usize;
    for (i, &b) in sample[..n].iter().enumerate() {
        if b == 0 {
            if i % 2 == 0 {
                even_zero += 1;
            } else {
                odd_zero += 1;
            }
        }
    }
    let even_zero_frac = even_zero as f64 / pairs as f64;
    let odd_zero_frac = odd_zero as f64 / pairs as f64;
    if odd_zero_frac > 0.4 && even_zero_frac < 0.4 {
        Some(true) // UTF-16LE: high byte (odd index) is the zero one
    } else if even_zero_frac > 0.4 && odd_zero_frac < 0.4 {
        Some(false) // UTF-16BE: high byte (even index) is the zero one
    } else {
        None
    }
}

fn decode_utf16le(bytes: &[u8]) -> String {
    // `chunks_exact` silently drops a trailing odd byte, which is correct
    // here: a lone byte can't be a UTF-16 code unit.
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn decode_utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    // Accumulates the tag name (and attributes) while parsing inside `<…>`
    let mut tag_buf = String::new();
    // When true, we're inside a <script> or <style> block; text is dropped
    let mut skip_content = false;
    // The element name we're skipping (so we know which </…> ends the block)
    let mut skip_tag: String = String::new();

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' => {
                in_tag = false;
                // tag_buf now holds everything between `<` and `>`
                // e.g. "script", "/script", "style type='text/css'", etc.
                let tag_content = tag_buf.trim();
                if let Some(rest) = tag_content.strip_prefix('/') {
                    // Closing tag — check if it ends a skip block
                    let close_name = rest.split_whitespace().next().unwrap_or("").to_lowercase();
                    if skip_content && close_name == skip_tag {
                        skip_content = false;
                        skip_tag.clear();
                    }
                } else {
                    // Opening tag — check if it starts a skip block
                    let open_name = tag_content
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_lowercase();
                    if open_name == "script" || open_name == "style" {
                        skip_content = true;
                        skip_tag = open_name;
                    }
                }
            }
            _ => {
                if in_tag {
                    tag_buf.push(ch);
                } else if !skip_content {
                    out.push(ch);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn txt_returns_raw_content() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        write!(f, "hello world").unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn md_returns_raw_content() {
        let mut f = NamedTempFile::with_suffix(".md").unwrap();
        write!(f, "# Title\n\nsome text").unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert!(result.contains("Title"));
        assert!(result.contains("some text"));
    }

    #[test]
    fn html_strips_tags_preserves_text() {
        let mut f = NamedTempFile::with_suffix(".html").unwrap();
        write!(
            f,
            "<html><body><h1>Welcome</h1><p>body text</p></body></html>"
        )
        .unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert!(result.contains("Welcome"));
        assert!(result.contains("body text"));
        assert!(!result.contains("<h1>"));
        assert!(!result.contains("<p>"));
    }

    #[test]
    fn html_excludes_script_and_style_content() {
        let html =
            "<p>keep</p><script>var secret=1;</script><style>.x{color:red}</style><p>alsokeep</p>";
        let result = strip_html_tags(html);
        assert!(result.contains("keep"), "expected 'keep' in: {result:?}");
        assert!(
            result.contains("alsokeep"),
            "expected 'alsokeep' in: {result:?}"
        );
        assert!(
            !result.contains("secret"),
            "script content leaked: {result:?}"
        );
        assert!(
            !result.contains("color"),
            "style content leaked: {result:?}"
        );
    }

    #[test]
    fn missing_file_returns_error() {
        let result = extract_plain(std::path::Path::new("no_such_file_xyz.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_utf8_decodes_lossily_not_error() {
        let mut f = NamedTempFile::with_suffix(".log").unwrap();
        // Valid ASCII words around an invalid UTF-8 byte (0xFF).
        f.write_all(b"errorword \xFF trailingword").unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert!(
            result.contains("errorword"),
            "valid words must survive lossy decode"
        );
        assert!(
            result.contains("trailingword"),
            "words after a bad byte must survive too"
        );
    }

    #[test]
    fn utf16le_bom_decodes_without_nulls() {
        // PowerShell 5 `Out-File`/`>` and Notepad "Unicode" both default to
        // UTF-16LE with a BOM.
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(
            "utfsixteenword"
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes()),
        );
        f.write_all(&bytes).unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert!(
            result.contains("utfsixteenword"),
            "word missing from decoded UTF-16LE content: {result:?}"
        );
        assert!(
            !result.contains('\0'),
            "decoded text must not contain NUL bytes (tokenizer splits on NUL): {result:?}"
        );
    }

    #[test]
    fn utf16be_bom_decodes_without_nulls() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(
            "utfsixteenword"
                .encode_utf16()
                .flat_map(|u| u.to_be_bytes()),
        );
        f.write_all(&bytes).unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert!(
            result.contains("utfsixteenword"),
            "word missing from decoded UTF-16BE content: {result:?}"
        );
        assert!(
            !result.contains('\0'),
            "decoded text must not contain NUL bytes: {result:?}"
        );
    }

    #[test]
    fn bomless_utf16le_log_is_heuristically_decoded() {
        // Some Windows tools write UTF-16LE with no BOM; the tokenizer splits
        // on NUL, so without heuristic detection this indexes as garbage.
        let mut f = NamedTempFile::with_suffix(".log").unwrap();
        let bytes: Vec<u8> = "bomlessword sixteen text content here"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        f.write_all(&bytes).unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert!(
            result.contains("bomlessword"),
            "word missing from heuristically-decoded UTF-16LE content: {result:?}"
        );
        assert!(
            !result.contains('\0'),
            "decoded text must not contain NUL bytes: {result:?}"
        );
    }

    #[test]
    fn utf8_bom_is_stripped() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"utfeightword content");
        f.write_all(&bytes).unwrap();
        let result = extract_plain(f.path()).unwrap();
        assert_eq!(result, "utfeightword content");
    }
}
