// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0\0\x10JFIF\0";
const WEBP: &[u8] = b"RIFF\x1a\0\0\0WEBPVP8L";

fn bmff(brand: &[u8; 4]) -> Vec<u8> {
    [b"\0\0\0\x18ftyp".as_slice(), brand, b"\0\0\0\0mif1"].concat()
}

#[test]
fn magic_bytes_identify_whatsapp_style_renamed_images() {
    assert_eq!(detect_head(PNG), Some("png"));
    assert_eq!(detect_head(WEBP), Some("webp"));
    assert_eq!(detect_head(JPEG), Some("jpeg"));
    assert!(is_mismatch("jpg", detect_head(PNG)));
    assert!(is_mismatch("jpg", detect_head(WEBP)));
    assert!(is_mismatch("png", detect_head(JPEG)));
    assert!(!is_mismatch("jpeg", detect_head(JPEG)));
}

#[test]
fn iso_media_brands_are_not_reported_as_mismatches() {
    assert_eq!(detect_head(&bmff(b"heic")), Some("heic"));
    assert_eq!(detect_head(&bmff(b"avif")), Some("avif"));
    assert_eq!(detect_head(&bmff(b"M4A ")), Some("m4a"));
    assert_eq!(detect_head(&bmff(b"qt  ")), Some("mov"));
    assert!(!is_mismatch("heic", detect_head(&bmff(b"mif1"))));
    assert!(!is_mismatch("avif", detect_head(&bmff(b"avif"))));
    assert!(!is_mismatch("m4a", detect_head(&bmff(b"M4A "))));
    assert!(!is_mismatch("mov", detect_head(&bmff(b"isom"))));
    assert!(is_mismatch("jpg", detect_head(&bmff(b"heic"))));
}

#[test]
fn containers_and_text_are_detected() {
    assert_eq!(detect_head(b"II*\0\x08\0\0\0"), Some("tiff"));
    assert_eq!(detect_head(b"fLaC\0\0\0\x22"), Some("flac"));
    assert_eq!(detect_head(b"ID3\x03\0\0\0\0\0\x10"), Some("mp3"));
    assert_eq!(detect_head(b"RIFF\0\0\0\0WAVEfmt "), Some("wav"));
    assert_eq!(detect_head(b"\x1A\x45\xDF\xA3\xA3"), Some("mkv"));
    assert_eq!(detect_head(b"{\\rtf1\\ansi"), Some("rtf"));
    assert_eq!(detect_head(b"\0\0\x01\0\x01\0"), Some("ico"));
    assert_eq!(
        detect_head(b"<?xml version=\"1.0\"?><svg xmlns=\"x\"/>"),
        Some("svg")
    );
    assert_eq!(detect_head("a,b\r\n1,é\r\n".as_bytes()), Some("text"));
    assert_eq!(detect_head(b"BMW service notes, March"), Some("text"));
    assert_eq!(detect_head(b"BM\x36\0\0\0\0\0\0\0\x36\0\0\0"), Some("bmp"));
    assert_eq!(detect_head(b"\0\x01\x02garbage"), None);
    assert_eq!(detect_head(b""), None);
    assert!(!is_mismatch("dng", Some("tiff")));
    assert!(!is_mismatch("bin", Some("png")));
    assert_eq!(type_for_extension("csv"), Some("text"));
}

#[test]
fn office_and_opendocument_zips_are_told_apart() {
    let odt = [
        b"PK\x03\x04".as_slice(),
        &[0u8; 26],
        b"mimetypeapplication/vnd.oasis.opendocument.text",
    ]
    .concat();
    assert_eq!(detect_head(&odt), Some("zip"));
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    };
    let docx = [
        b"PK\x03\x04".as_slice(),
        &[0u8; 26],
        b"[Content_Types].xml....word/document.xml",
    ]
    .concat();
    assert_eq!(detect_file(&write("a.docx", &docx)), Some("docx"));
    assert_eq!(detect_file(&write("a.odt", &odt)), Some("odt"));
    assert_eq!(
        detect_file(&write("a.zip", b"PK\x03\x04inner.txt")),
        Some("zip")
    );
    assert!(!is_mismatch("docx", Some("zip")));
    assert!(is_mismatch("docx", Some("pdf")));
}
