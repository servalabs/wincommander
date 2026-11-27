// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shared table against Free's previous `scrub_content_type` module
//! (frozen in tests/legacy/). Every extension crossed with a set of file
//! heads must give the same answers, except the documented corrections:
//!
//! | content                                   | before | now  |
//! |-------------------------------------------|--------|------|
//! | RF64 WAV (WAV over 4 GB)                  | none   | wav  |
//! | AVIF with major brand mif1 (compat. avif) | heic   | avif |
//! | FLAC with a leading ID3 tag               | mp3    | flac |
//! | MP4 whose ftyp box is 256 bytes           | ico    | mp4  |
//! | OpenDocument drawing/formula/... ZIP      | zip    | odf  |
//! | `.mka` extension                          | none   | mkv  |
//! | SVG markup is plain text (a `.tsx` with an inline `<svg>` has nothing to remove; `.svg` itself still does) | | |
//!
//! Each is what the Pro cleaner already did, so the Free report now agrees
//! with it. `WINCMD_FILETYPE_CORPUS=<dir>` also compares every file below
//! that folder (the shared real-file corpus).

#[path = "legacy/free_scrub_content_type.rs"]
#[allow(dead_code)]
mod legacy;

use std::path::Path;

const CORRECTIONS: &[(Option<&str>, Option<&str>)] = &[
    (None, Some("wav")),
    (Some("heic"), Some("avif")),
    (Some("mp3"), Some("flac")),
    (Some("ico"), Some("mp4")),
    (Some("zip"), Some("odf")),
];

fn ftyp(major: &[u8], compatible: &[&[u8]], size: usize) -> Vec<u8> {
    let mut out = (size as u32).to_be_bytes().to_vec();
    out.extend_from_slice(b"ftyp");
    out.extend_from_slice(major);
    out.extend_from_slice(&[0, 0, 0, 0]);
    for brand in compatible {
        out.extend_from_slice(brand);
    }
    out.resize(size.max(out.len()), 0);
    out
}

fn zip_with(first: &[u8], tail: &[u8]) -> Vec<u8> {
    [b"PK\x03\x04".as_slice(), &[0u8; 26], first, b"....", tail].concat()
}

fn heads() -> Vec<(&'static str, Vec<u8>)> {
    let mut all: Vec<(&str, Vec<u8>)> = vec![
        ("jpeg", b"\xFF\xD8\xFF\xE0\0\x10JFIF\0".to_vec()),
        ("png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec()),
        ("gif89", b"GIF89a\x01\0\x01\0".to_vec()),
        ("gif87", b"GIF87a\x01\0\x01\0".to_vec()),
        ("webp", b"RIFF\x1a\0\0\0WEBPVP8L".to_vec()),
        ("wav", b"RIFF\0\0\0\0WAVEfmt ".to_vec()),
        ("bmp", b"BM\x36\0\0\0\0\0\0\0\x36\0\0\0".to_vec()),
        ("bmw-text", b"BMW service notes, March".to_vec()),
        ("tiff-le", b"II*\0\x08\0\0\0".to_vec()),
        ("tiff-be", b"MM\0*\0\0\0\x08".to_vec()),
        ("orf", b"IIRO\x08\0\0\0".to_vec()),
        ("pdf", b"%PDF-1.7\n".to_vec()),
        ("zip", b"PK\x03\x04inner.txt".to_vec()),
        ("zip-empty", b"PK\x05\x06\0\0\0\0".to_vec()),
        (
            "docx",
            zip_with(b"[Content_Types].xml", b"word/document.xml"),
        ),
        ("xlsx", zip_with(b"[Content_Types].xml", b"xl/workbook.xml")),
        (
            "pptx",
            zip_with(b"[Content_Types].xml", b"ppt/presentation.xml"),
        ),
        (
            "odt",
            zip_with(b"mimetypeapplication/vnd.oasis.opendocument.text", b""),
        ),
        (
            "ods",
            zip_with(
                b"mimetypeapplication/vnd.oasis.opendocument.spreadsheet",
                b"",
            ),
        ),
        (
            "odp",
            zip_with(
                b"mimetypeapplication/vnd.oasis.opendocument.presentation",
                b"",
            ),
        ),
        (
            "ott",
            zip_with(
                b"mimetypeapplication/vnd.oasis.opendocument.text-template",
                b"",
            ),
        ),
        ("ole", b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1\0\0".to_vec()),
        ("rtf", b"{\\rtf1\\ansi".to_vec()),
        (
            "id3-mp3",
            b"ID3\x03\0\0\0\0\0\x02\0\0\xFF\xFB\x90\0".to_vec(),
        ),
        (
            "id3-v9",
            b"ID3\x09\0\0\0\0\0\x02\0\0\xFF\xFB\x90\0".to_vec(),
        ),
        ("utf16le", b"\xFF\xFE[\0.\0S\0h\0".to_vec()),
        ("utf16be", b"\xFE\xFF\0[\0.\0S".to_vec()),
        ("utf8-bom", b"\xEF\xBB\xBF[.ShellClassInfo]".to_vec()),
        ("flac", b"fLaC\0\0\0\x22".to_vec()),
        ("ogg", b"OggS\0\x02\0\0".to_vec()),
        ("mkv", b"\x1A\x45\xDF\xA3\x9f".to_vec()),
        ("psd", b"8BPS\0\x01".to_vec()),
        ("ico", b"\0\0\x01\0\x01\0\x10\x10".to_vec()),
        ("mp3-sync", b"\xFF\xFB\x90\x00".to_vec()),
        ("text", "a,b\r\n1,\u{e9}\r\n".as_bytes().to_vec()),
        ("svg", b"<?xml version=\"1.0\"?><svg xmlns=\"x\"/>".to_vec()),
        (
            "html",
            b"<html><meta name=\"author\" content=\"A\"></html>".to_vec(),
        ),
        ("binary", b"\0\x01\x02garbage".to_vec()),
        ("empty", Vec::new()),
        // Documented corrections.
        ("rf64", b"RF64\xff\xff\xff\xffWAVEds64".to_vec()),
        ("avif-mif1", ftyp(b"mif1", &[b"mif1", b"avif", b"miaf"], 28)),
        (
            "id3-flac",
            b"ID3\x03\0\0\0\0\0\x02\0\0fLaC\0\0\0\x22".to_vec(),
        ),
        ("mp4-ftyp256", ftyp(b"isom", &[b"isom", b"mp41"], 256)),
        (
            "odg",
            zip_with(b"mimetypeapplication/vnd.oasis.opendocument.graphics", b""),
        ),
    ];
    let brands: [(&str, &[u8], &[&[u8]]); 8] = [
        ("heic", b"heic", &[b"mif1", b"heic"]),
        ("heif-mif1", b"mif1", &[b"mif1", b"heic"]),
        ("avif", b"avif", &[b"avif", b"mif1"]),
        ("m4a", b"M4A ", &[b"M4A ", b"mp42"]),
        ("mov", b"qt  ", &[b"qt  "]),
        ("cr3", b"crx ", &[b"crx "]),
        ("3gp", b"3gp4", &[b"3gp4"]),
        ("mp4", b"isom", &[b"isom", b"mp41"]),
    ];
    for (name, major, compatible) in brands {
        all.push((name, ftyp(major, compatible, 16 + 4 * compatible.len())));
    }
    all
}

fn extensions() -> Vec<&'static str> {
    let mut exts = vec![
        "jpg", "jpeg", "jpe", "jfif", "mpo", "png", "gif", "webp", "bmp", "dib", "tif", "tiff",
        "dng", "cr2", "nef", "nrw", "arw", "sr2", "orf", "rw2", "pef", "srw", "erf", "3fr", "fff",
        "iiq", "mef", "mos", "rwl", "gpr", "heic", "heif", "hif", "avif", "mp4", "m4v", "mov",
        "qt", "m4a", "m4b", "3gp", "3g2", "cr3", "pdf", "ai", "docx", "docm", "dotx", "xlsx",
        "xlsm", "xltx", "pptx", "pptm", "potx", "odt", "ods", "odp", "zip", "doc", "xls", "ppt",
        "msg", "rtf", "mp3", "wav", "flac", "ogg", "opus", "oga", "mkv", "mka", "webm", "ico",
        "svg", "psd", "psb", "txt", "csv", "tsv", "md", "json", "log", "ini", "xml", "yaml", "yml",
        "htm", "html", "xhtml", "eml", "mht", "mhtml", "xmp", "gpx", "kml", "cfg", "rdp", "",
        "bin", "eps", "jxl", "jp2",
    ];
    exts.sort();
    exts.dedup();
    exts
}

/// Differences in the decisions Free makes from one file.
fn compare(path: &Path, ext: &str, label: &str, out: &mut Vec<String>) {
    let (old, new) = (
        legacy::detect_file(path),
        wincmd_filetype::detect_file(path),
    );
    if old != new && !CORRECTIONS.contains(&(old, new)) {
        out.push(format!("{label}: detected {old:?} -> {new:?}"));
    }
    // `.mka` is newly in the table (Matroska audio), so its mismatch answers are new.
    let corrected = old != new || ext == "mka";
    if !corrected && legacy::is_mismatch(ext, old) != wincmd_filetype::is_mismatch(ext, new) {
        out.push(format!("{label} as .{ext}: mismatch changed"));
    }
    // SVG markup is now plain text (Pro already copied such files unchanged).
    let head: Vec<u8> = std::fs::read(path)
        .unwrap_or_default()
        .into_iter()
        .take(8192)
        .collect();
    let svg = String::from_utf8_lossy(&head)
        .to_ascii_lowercase()
        .contains("<svg");
    if !svg && legacy::is_plain_text_file(path) != wincmd_filetype::is_plain_text_file(path) {
        out.push(format!("{label}: plain text changed"));
    }
}

#[test]
fn every_extension_and_head_decides_as_before_except_the_documented_corrections() {
    let dir = tempfile::tempdir().unwrap();
    let mut differences = Vec::new();
    for ext in extensions() {
        if legacy::type_for_extension(ext) != wincmd_filetype::type_for_extension(ext)
            && ext != "mka"
        {
            differences.push(format!(".{ext}: extension type changed"));
        }
        if legacy::carries_no_hidden_metadata(ext)
            != wincmd_filetype::carries_no_hidden_metadata(ext)
            || legacy::text_carries_no_hidden_metadata(ext)
                != wincmd_filetype::text_carries_no_hidden_metadata(ext)
        {
            differences.push(format!(".{ext}: nothing-to-remove rule changed"));
        }
        for (name, head) in heads() {
            let path = dir.path().join(format!("{name}.{ext}"));
            std::fs::write(&path, &head).unwrap();
            compare(&path, ext, name, &mut differences);
        }
    }
    assert!(differences.is_empty(), "{differences:#?}");
}

#[test]
fn the_corrections_change_what_they_claim() {
    let dir = tempfile::tempdir().unwrap();
    let mut changed = Vec::new();
    for (name, head) in heads() {
        let path = dir.path().join(name);
        std::fs::write(&path, &head).unwrap();
        if legacy::detect_file(&path) != wincmd_filetype::detect_file(&path) {
            changed.push(name);
        }
    }
    assert_eq!(
        changed,
        ["rf64", "avif-mif1", "id3-flac", "mp4-ftyp256", "odg"]
    );
}

#[test]
fn real_corpus_files_decide_as_before() {
    let Some(root) = std::env::var_os("WINCMD_FILETYPE_CORPUS") else {
        eprintln!("WINCMD_FILETYPE_CORPUS not set; corpus comparison skipped");
        return;
    };
    let mut differences = Vec::new();
    let mut seen = 0;
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            compare(&path, &ext, &path.display().to_string(), &mut differences);
            seen += 1;
        }
    }
    eprintln!("compared {seen} corpus files");
    assert!(differences.is_empty(), "{differences:#?}");
}
