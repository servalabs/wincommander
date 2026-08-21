// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rich Text Format (.rtf) plain-text extraction.
//!
//! RTF is a 7-bit-ASCII control-word format: everything outside the ASCII
//! range travels as `\'hh` (one byte in the current ANSI code page) or `\uN`
//! (a Unicode scalar value) escapes. This walks the raw bytes directly (not
//! a pre-decoded string) per the RTF control-word grammar, drops non-content
//! destination groups (font/color/style tables, document info, embedded
//! objects/pictures), and turns `\par`/`\line`/`\row` into newlines and
//! `\tab`/`\cell` into a space so words don't fuse across a structural
//! boundary — the same goal as the OOXML/ODF extractors' paragraph/cell
//! handling.
//!
//! # Known limitations (documented, not silently wrong)
//! - `\'hh` and any raw high byte are decoded as Windows-1252 (RTF's default
//!   ANSI code page). A document that declares a different `\ansicpg` (e.g.
//!   1251 Cyrillic, Shift-JIS) will have its non-ASCII bytes decoded wrong.
//! - `\uN` Unicode escapes assume the default `\uc1` (skip exactly one
//!   fallback replacement character); a document that sets `\ucN` with
//!   N != 1 will have its fallback-skip count wrong.
//! - No author/title/keywords extraction from the `\info` destination in
//!   this pass — `\info` is a skip destination (its content never reaches
//!   the index), and no separate `DocProps` are parsed from it. Callers
//!   pair `extract_rtf`'s output with `DocProps::default()`.

use std::path::Path;

use crate::error::Result;

/// Windows-1252 (RTF's default ANSI code page) mapping for byte range
/// 0x80..=0x9F, where it diverges from Latin-1 (0xA0..=0xFF and 0x00..=0x7F
/// are identical to Latin-1, so no table is needed for those). Verified
/// against Python's `cp1252` codec; the 5 undefined slots decode to U+FFFD,
/// matching that reference.
const CP1252_HIGH: [u32; 32] = [
    0x20AC, 0xFFFD, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, // 80-87
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0xFFFD, 0x017D, 0xFFFD, // 88-8F
    0xFFFD, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, // 90-97
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0xFFFD, 0x017E, 0x0178, // 98-9F
];

/// Decode one byte to its Windows-1252 code point.
fn cp1252_to_char(byte: u8) -> char {
    let code = if (0x80..=0x9F).contains(&byte) {
        CP1252_HIGH[(byte - 0x80) as usize]
    } else {
        byte as u32
    };
    char::from_u32(code).unwrap_or('\u{FFFD}')
}

/// Destination control words whose group text must never reach the index —
/// font/color/style/list tables, document metadata, and embedded binary
/// objects. Not an exhaustive list of every RTF destination, but covers what
/// real-world Word/LibreOffice RTF export actually emits.
fn is_skip_destination(word: &str) -> bool {
    matches!(
        word,
        "fonttbl"
            | "colortbl"
            | "stylesheet"
            | "info"
            | "generator"
            | "pict"
            | "object"
            | "themedata"
            | "colorschememapping"
            | "latentstyles"
            | "listtable"
            | "listoverridetable"
            | "rsidtbl"
            | "xmlnstbl"
            | "datastore"
            | "nonshppict"
            | "shppict"
    )
}

/// Strip RTF control markup, returning the human-readable body text.
fn rtf_to_text(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() / 2);
    // One skip flag per open brace depth; index 0 is the implicit top level,
    // never popped (an unbalanced trailing `}` must not panic or desync).
    let mut skip_stack: Vec<bool> = vec![false];
    let mut i = 0usize;
    let n = bytes.len();

    macro_rules! skipping {
        () => {
            *skip_stack.last().unwrap()
        };
    }

    while i < n {
        let b = bytes[i];
        match b {
            b'{' => {
                skip_stack.push(skipping!());
                i += 1;
            }
            b'}' => {
                if skip_stack.len() > 1 {
                    skip_stack.pop();
                }
                i += 1;
            }
            b'\r' | b'\n' => {
                // Raw line breaks in the source are formatting whitespace
                // only — real paragraph breaks travel as \par control words.
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= n {
                    break;
                }
                match bytes[i] {
                    b'\\' | b'{' | b'}' => {
                        if !skipping!() {
                            out.push(bytes[i] as char);
                        }
                        i += 1;
                    }
                    b'\'' => {
                        // \'hh — one hex-escaped byte in the current code page.
                        i += 1;
                        if i + 2 <= n {
                            if let Ok(hex) = std::str::from_utf8(&bytes[i..i + 2]) {
                                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                                    if !skipping!() {
                                        out.push(cp1252_to_char(byte));
                                    }
                                }
                            }
                            i += 2;
                        }
                    }
                    b'*' => {
                        // Ignorable-destination marker: skip this whole
                        // group whether or not the control word that
                        // follows is one this parser recognises by name —
                        // correct RTF-consumer behaviour for `\*`.
                        *skip_stack.last_mut().unwrap() = true;
                        i += 1;
                    }
                    c if c.is_ascii_alphabetic() => {
                        let word_start = i;
                        while i < n && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        let word = std::str::from_utf8(&bytes[word_start..i]).unwrap_or("");
                        let param_start = i;
                        if i < n && bytes[i] == b'-' {
                            i += 1;
                        }
                        while i < n && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                        let param: Option<i32> = if i > param_start {
                            std::str::from_utf8(&bytes[param_start..i])
                                .ok()
                                .and_then(|s| s.parse().ok())
                        } else {
                            None
                        };
                        // Exactly one trailing space is the control-word
                        // delimiter and is consumed, never emitted.
                        if i < n && bytes[i] == b' ' {
                            i += 1;
                        }

                        match word {
                            "par" | "line" | "row" => {
                                if !skipping!() {
                                    out.push('\n');
                                }
                            }
                            "tab" | "cell" => {
                                if !skipping!() {
                                    out.push(' ');
                                }
                            }
                            "u" => {
                                if let Some(code) = param {
                                    let code = if code < 0 { code + 65536 } else { code };
                                    if !skipping!() {
                                        if let Some(ch) = char::from_u32(code as u32) {
                                            out.push(ch);
                                        }
                                    }
                                    // Default \uc1: skip exactly one fallback
                                    // replacement character, if present.
                                    if i < n
                                        && bytes[i] != b'\\'
                                        && bytes[i] != b'{'
                                        && bytes[i] != b'}'
                                    {
                                        i += 1;
                                    }
                                }
                            }
                            _ if is_skip_destination(word) => {
                                *skip_stack.last_mut().unwrap() = true;
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        // Unhandled control symbol (\~, \-, \_, etc.) — a
                        // single character, no output.
                        i += 1;
                    }
                }
            }
            _ => {
                if !skipping!() {
                    out.push(cp1252_to_char(b));
                }
                i += 1;
            }
        }
    }
    out
}

/// Extract plain text from a `.rtf` file.
pub fn extract_rtf(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(rtf_to_text(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_rtf(rtf: &str) -> NamedTempFile {
        let mut f = NamedTempFile::with_suffix(".rtf").unwrap();
        f.write_all(rtf.as_bytes()).unwrap();
        f
    }

    #[test]
    fn extracts_plain_words() {
        let f = write_rtf(r"{\rtf1\ansi\deff0 hello rtf document}");
        let result = extract_rtf(f.path()).unwrap();
        assert!(result.contains("hello rtf document"), "got: {result:?}");
    }

    #[test]
    fn fonttbl_colortbl_stylesheet_and_info_excluded() {
        let f = write_rtf(
            r"{\rtf1\ansi{\fonttbl{\f0 Calibri;}}{\colortbl;\red0\green0\blue0;}{\stylesheet{\s0 Normal;}}{\info{\title Secret Title}{\author Jane Doe}} realcontent here}",
        );
        let result = extract_rtf(f.path()).unwrap();
        assert!(result.contains("realcontent here"), "got: {result:?}");
        assert!(!result.contains("Calibri"), "fonttbl leaked: {result:?}");
        assert!(!result.contains("red0"), "colortbl leaked: {result:?}");
        assert!(!result.contains("Normal"), "stylesheet leaked: {result:?}");
        assert!(
            !result.contains("Secret Title"),
            "info/title leaked: {result:?}"
        );
        assert!(
            !result.contains("Jane Doe"),
            "info/author leaked: {result:?}"
        );
    }

    #[test]
    fn ignorable_destination_via_star_excluded() {
        let f = write_rtf(
            r"{\rtf1{\*\generator Msftedit 5.41;}{\*\wgrffmtfilter hidden filter junk}visible text}",
        );
        let result = extract_rtf(f.path()).unwrap();
        assert!(result.contains("visible text"), "got: {result:?}");
        assert!(
            !result.contains("Msftedit"),
            "\\*-marked generator leaked: {result:?}"
        );
        assert!(
            !result.contains("hidden filter junk"),
            "\\*-marked group leaked: {result:?}"
        );
    }

    #[test]
    fn par_becomes_newline_and_keeps_paragraphs_separate() {
        let f = write_rtf(r"{\rtf1 fooend\par Startbar}");
        let result = extract_rtf(f.path()).unwrap();
        assert!(
            !result.contains("fooendStartbar"),
            "words across \\par must not concatenate: {result:?}"
        );
        assert!(result.contains("fooend"));
        assert!(result.contains("Startbar"));
    }

    #[test]
    fn tab_and_cell_become_separators() {
        let f = write_rtf(r"{\rtf1 col1\tab col2\cell col3}");
        let result = extract_rtf(f.path()).unwrap();
        assert!(
            !result.contains("col1col2"),
            "tab must separate: {result:?}"
        );
        assert!(
            !result.contains("col2col3"),
            "cell must separate: {result:?}"
        );
    }

    #[test]
    fn escaped_braces_and_backslash_are_literal_chars() {
        let f = write_rtf(r"{\rtf1 a\{b\}c\\d}");
        let result = extract_rtf(f.path()).unwrap();
        assert_eq!(result.trim(), r"a{b}c\d");
    }

    #[test]
    fn hex_escape_decodes_accented_char() {
        // \'e9 is Windows-1252 (and Latin-1) for U+00E9 'é'.
        let f = write_rtf(r"{\rtf1 caf\'e9 marker}");
        let result = extract_rtf(f.path()).unwrap();
        assert!(result.contains("caf\u{00E9} marker"), "got: {result:?}");
    }

    #[test]
    fn hex_escape_uses_cp1252_not_plain_latin1_for_high_range() {
        // \'93 is a CP1252 left double curly quote (U+201C), NOT the Latin-1
        // C1 control code point U+0093 a naive byte-as-char mapping would give.
        let f = write_rtf(r"{\rtf1 \'93quoted\'94}");
        let result = extract_rtf(f.path()).unwrap();
        assert!(
            result.contains('\u{201C}') && result.contains('\u{201D}'),
            "expected CP1252 curly quotes, got: {result:?}"
        );
        assert!(
            !result.contains('\u{0093}'),
            "must not be raw Latin-1 C1 byte: {result:?}"
        );
    }

    #[test]
    fn unicode_escape_decodes_and_skips_one_fallback_char() {
        // Decimal 8364 is the euro sign's code point (U+20AC); the
        // trailing '?' is the mandatory ASCII fallback for old readers
        // and must be consumed, not emitted.
        let f = write_rtf("{\\rtf1 \\u8364?euro marker}");
        let result = extract_rtf(f.path()).unwrap();
        assert!(result.contains("\u{20AC}euro marker"), "got: {result:?}");
        assert!(
            !result.contains("?euro"),
            "fallback char must be consumed: {result:?}"
        );
    }

    #[test]
    fn negative_unicode_escape_wraps_to_valid_code_point() {
        // RTF encodes code points >= 0x8000 as a signed 16-bit value.
        // -3728 + 65536 = 61808 = 0xF170 (a PUA code point) — must not panic.
        let f = write_rtf(r"{\rtf1 \u-3728?marker}");
        let result = extract_rtf(f.path()).unwrap();
        assert!(result.contains("marker"), "got: {result:?}");
    }

    #[test]
    fn missing_file_returns_error() {
        let result = extract_rtf(std::path::Path::new("no_such_file.rtf"));
        assert!(result.is_err());
    }

    #[test]
    fn unbalanced_closing_brace_does_not_panic() {
        let f = write_rtf(r"{\rtf1 content}}}}");
        let result = extract_rtf(f.path());
        assert!(result.is_ok());
    }

    #[test]
    fn truncated_hex_escape_at_eof_does_not_panic() {
        let f = write_rtf(r"{\rtf1 trailing\'e");
        let result = extract_rtf(f.path());
        assert!(result.is_ok());
    }
}
