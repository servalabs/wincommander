// SPDX-License-Identifier: AGPL-3.0-or-later
// Content-based file type detection for the Share Safely report.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const HEAD_BYTES: usize = 4096;
const TEXT_PROBE_BYTES: usize = 8192;
const ZIP_TAIL_BYTES: u64 = 65536;

/// Canonical type for a lowercase extension (no dot), or None when unknown.
pub fn type_for_extension(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "jpg" | "jpeg" | "jpe" | "jfif" | "mpo" => "jpeg",
        "png" => "png",
        "gif" => "gif",
        "webp" => "webp",
        "bmp" | "dib" => "bmp",
        "tif" | "tiff" => "tiff",
        "dng" | "cr2" | "nef" | "nrw" | "arw" | "sr2" | "orf" | "rw2" | "pef" | "srw" | "erf"
        | "3fr" | "fff" | "iiq" | "mef" | "mos" | "rwl" | "gpr" => "raw",
        "heic" | "heif" | "hif" => "heic",
        "avif" => "avif",
        "mp4" | "m4v" => "mp4",
        "mov" | "qt" => "mov",
        "m4a" | "m4b" => "m4a",
        "3gp" | "3g2" => "3gp",
        "cr3" => "cr3",
        "pdf" | "ai" => "pdf",
        "docx" | "docm" | "dotx" => "docx",
        "xlsx" | "xlsm" | "xltx" => "xlsx",
        "pptx" | "pptm" | "potx" => "pptx",
        "odt" => "odt",
        "ods" => "ods",
        "odp" => "odp",
        "zip" => "zip",
        "doc" | "xls" | "ppt" | "msg" => "ole",
        "rtf" => "rtf",
        "mp3" => "mp3",
        "wav" => "wav",
        "flac" => "flac",
        "ogg" | "opus" | "oga" => "ogg",
        "mkv" | "webm" => "mkv",
        "ico" => "ico",
        "svg" => "svg",
        "psd" | "psb" => "psd",
        "txt" | "csv" | "tsv" | "md" | "json" | "log" | "ini" | "xml" | "yaml" | "yml" => "text",
        _ => return None,
    })
}

/// Formats whose bytes are the user's content with no separate metadata block.
pub fn carries_no_hidden_metadata(ext: &str) -> bool {
    matches!(type_for_extension(ext), Some("text" | "bmp" | "ico"))
}

/// Whether text content under this extension has nothing to remove. These
/// text formats keep author, file path or location fields of their own.
pub fn text_carries_no_hidden_metadata(ext: &str) -> bool {
    !matches!(
        ext,
        "svg" | "htm" | "html" | "xhtml" | "rtf" | "eml" | "mht" | "mhtml" | "xmp" | "gpx" | "kml"
    )
}

/// True when the file starts as plain text: UTF-8 without NULs, or UTF-16
/// with a byte-order mark. Unreadable files are not text.
pub fn is_plain_text_file(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut head = vec![0u8; TEXT_PROBE_BYTES];
    let n = read_up_to(&mut file, &mut head);
    is_plain_text(&head[..n])
}

fn is_plain_text(b: &[u8]) -> bool {
    let (body, little_endian) = match b {
        [0xFF, 0xFE, rest @ ..] => (rest, true),
        [0xFE, 0xFF, rest @ ..] => (rest, false),
        _ => return detect_head(b) == Some("text"),
    };
    let mut units: Vec<u16> = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| {
            if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        })
        .collect();
    // The probe can end between the two halves of a surrogate pair.
    if units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
        units.pop();
    }
    char::decode_utf16(units).all(|c| c.is_ok_and(|c| c != '\0'))
}

/// Detect the real type of a file from its leading bytes (and, for ZIP, its
/// central directory). Returns None for unreadable or unrecognised content.
pub fn detect_file(path: &Path) -> Option<&'static str> {
    let mut file = File::open(path).ok()?;
    let mut head = vec![0u8; HEAD_BYTES];
    let n = read_up_to(&mut file, &mut head);
    head.truncate(n);
    let mut kind = detect_head(&head)?;
    if kind == "zip" {
        let len = file.metadata().ok()?.len();
        let mut tail = Vec::new();
        if file
            .seek(SeekFrom::Start(len.saturating_sub(ZIP_TAIL_BYTES)))
            .is_ok()
        {
            let _ = file.take(ZIP_TAIL_BYTES).read_to_end(&mut tail);
        }
        kind = zip_subtype(&head, &tail);
    }
    Some(kind)
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> usize {
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => total += n,
        }
    }
    total
}

/// Magic-byte detection over the first bytes of a file.
pub fn detect_head(b: &[u8]) -> Option<&'static str> {
    if b.is_empty() {
        return None;
    }
    let at = |off: usize, sig: &[u8]| b.len() >= off + sig.len() && &b[off..off + sig.len()] == sig;
    Some(match () {
        _ if at(0, b"\xFF\xD8\xFF") => "jpeg",
        _ if at(0, b"\x89PNG\r\n\x1a\n") => "png",
        _ if at(0, b"GIF87a") || at(0, b"GIF89a") => "gif",
        _ if at(0, b"RIFF") && at(8, b"WEBP") => "webp",
        _ if at(0, b"RIFF") && at(8, b"WAVE") => "wav",
        _ if at(0, b"BM") && at(6, b"\0\0\0\0") => "bmp",
        _ if at(0, b"II*\0") || at(0, b"MM\0*") => "tiff",
        _ if at(0, b"%PDF") => "pdf",
        _ if at(0, b"PK\x03\x04") || at(0, b"PK\x05\x06") => "zip",
        _ if at(0, b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") => "ole",
        _ if at(0, b"{\\rtf") => "rtf",
        _ if at(0, b"ID3") && b.len() > 3 && (2..=4).contains(&b[3]) => "mp3",
        // A UTF-16 BOM says nothing about the format, so it must not count against the extension.
        _ if at(0, b"\xFF\xFE") || at(0, b"\xFE\xFF") => return None,
        _ if at(0, b"fLaC") => "flac",
        _ if at(0, b"OggS") => "ogg",
        _ if at(0, b"\x1A\x45\xDF\xA3") => "mkv",
        _ if at(0, b"8BPS") => "psd",
        _ if at(0, b"\0\0\x01\0") => "ico",
        _ if at(4, b"ftyp") && b.len() >= 12 => bmff_brand(&b[8..12]),
        _ => return detect_text(b),
    })
}

fn bmff_brand(brand: &[u8]) -> &'static str {
    match brand {
        b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => "heic",
        b"avif" | b"avis" => "avif",
        b"qt  " => "mov",
        b"M4A " | b"M4B " | b"M4P " => "m4a",
        b"crx " => "cr3",
        _ if brand.starts_with(b"3g") => "3gp",
        _ => "mp4",
    }
}

fn detect_text(b: &[u8]) -> Option<&'static str> {
    let body = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    if body.contains(&0) || std::str::from_utf8(body).is_err() && !utf8_cut_at_end(body) {
        return None;
    }
    let lower = String::from_utf8_lossy(body).to_ascii_lowercase();
    if lower.contains("<svg") {
        return Some("svg");
    }
    Some("text")
}

// A UTF-8 sequence can be split by the fixed-size head read.
fn utf8_cut_at_end(b: &[u8]) -> bool {
    matches!(std::str::from_utf8(b), Err(e) if e.error_len().is_none())
}

fn zip_subtype(head: &[u8], tail: &[u8]) -> &'static str {
    if let Some(odf) = odf_mimetype(head) {
        return odf;
    }
    let has = |needle: &[u8]| {
        [head, tail]
            .iter()
            .any(|b| b.windows(needle.len()).any(|w| w == needle))
    };
    if has(b"[Content_Types].xml") {
        if has(b"word/") {
            return "docx";
        }
        if has(b"xl/") {
            return "xlsx";
        }
        if has(b"ppt/") {
            return "pptx";
        }
    }
    "zip"
}

// ODF stores an uncompressed "mimetype" entry first, so its value sits at a fixed offset.
fn odf_mimetype(head: &[u8]) -> Option<&'static str> {
    let rest = head.get(30..)?;
    let value = rest.strip_prefix(b"mimetype")?;
    let kinds: [(&[u8], &str); 3] = [
        (b"application/vnd.oasis.opendocument.text", "odt"),
        (b"application/vnd.oasis.opendocument.spreadsheet", "ods"),
        (b"application/vnd.oasis.opendocument.presentation", "odp"),
    ];
    kinds
        .iter()
        .find(|(mime, _)| value.starts_with(mime))
        .map(|(_, kind)| *kind)
}

/// True when the content type contradicts the extension. Unknown on either
/// side is never a mismatch, and same-container variants are compatible.
pub fn is_mismatch(ext: &str, detected: Option<&str>) -> bool {
    let (Some(expected), Some(actual)) = (type_for_extension(ext), detected) else {
        return false;
    };
    let bmff_av = |t: &str| matches!(t, "mp4" | "mov" | "m4a" | "3gp");
    let compatible = expected == actual
        || (bmff_av(expected) && bmff_av(actual))
        || matches!(
            (expected, actual),
            ("raw", "tiff")
                | ("cr3", "mp4")
                | ("text", "svg")
                | ("svg", "text")
                | ("zip", "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp")
                | ("docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp", "zip")
        );
    !compatible
}

#[cfg(test)]
#[path = "scrub_content_type_tests.rs"]
mod tests;
