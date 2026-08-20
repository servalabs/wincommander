mod epub;
mod legacy_office;
mod odf;
mod office;
mod pdf;
mod plain;
mod rtf;

pub use epub::extract_epub;
pub use legacy_office::extract_legacy_office;
pub use odf::extract_odf;
pub use office::{extract_office, extract_xlsx};
pub use pdf::extract_pdf;
pub use plain::extract_plain;
use plain::{decode_text_bytes, detect_bomless_utf16};
pub use rtf::extract_rtf;

use std::path::Path;

use crate::error::{Result, SearchError};
use crate::types::{DocProps, ExtractedDoc, FileMeta};

const MAX_BODY_BYTES: usize = 4 * 1024 * 1024; // 4 MB

/// Dispatch to the correct extractor based on file extension.
pub fn extract_text(meta: FileMeta) -> Result<ExtractedDoc> {
    let (body, props) = match meta.ext.as_str() {
        // Plain-text: markup / docs, config / data, source code, scripts.
        // Plain text carries no document-internal properties.
        "txt" | "text" | "md" | "markdown" | "rst" | "adoc" | "asciidoc" | "org" | "tex"
        | "html" | "htm" | "xhtml" | "log" | "csv" | "tsv" | "json" | "jsonl" | "ndjson"
        | "xml" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "config" | "properties"
        | "env" | "rs" | "py" | "pyw" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "css"
        | "scss" | "less" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "psm1" | "psd1" | "bat"
        | "cmd" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hxx" | "java" | "kt" | "kts"
        | "go" | "rb" | "php" | "swift" | "scala" | "sql" | "r" | "lua" | "pl" | "pm" | "dart"
        | "vue" | "svelte" | "gradle" | "groovy" | "srt" | "vtt" => {
            (extract_plain(&meta.path)?, DocProps::default())
        }
        "docx" | "pptx" => extract_office(&meta.path)?,
        "xlsx" => extract_xlsx(&meta.path)?,
        // Bounded legacy Compound File extraction. Encrypted files remain an
        // explicit unsupported input rather than being represented as indexed.
        "doc" | "xls" | "ppt" => (extract_legacy_office(&meta.path)?, DocProps::default()),
        "pdf" => extract_pdf(&meta.path)?,
        // OpenDocument Format — zip-of-XML like OOXML, see extract/odf.rs.
        "odt" | "ods" | "odp" => extract_odf(&meta.path)?,
        "epub" => (extract_epub(&meta.path)?, DocProps::default()),
        // Rich Text Format — control-word markup, see extract/rtf.rs. No
        // document-internal properties extracted (see that module's docs).
        "rtf" => (extract_rtf(&meta.path)?, DocProps::default()),
        // Unknown / no extension: index it iff it sniffs as UTF-8 text. Covers
        // README, Dockerfile, .gitignore, novel config suffixes;
        // binaries (NUL in the header) are skipped as Unsupported.
        other => match sniff_text(&meta.path)? {
            Some(text) => (text, DocProps::default()),
            None => return Err(SearchError::Unsupported(other.to_string())),
        },
    };

    let body = if body.len() > MAX_BODY_BYTES {
        // Truncate at a UTF-8 char boundary.
        let mut end = MAX_BODY_BYTES;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body[..end].to_owned()
    } else {
        body
    };

    let title = meta.name.clone();
    Ok(ExtractedDoc {
        meta,
        title,
        body,
        props,
    })
}

/// Best-effort text sniff for files with no recognised extension. Reads an
/// 8 KiB header; a NUL byte there means "binary → skip" (`Ok(None)`) UNLESS
/// the header starts with a UTF-16 BOM, OR the header passes the same
/// alternating-NUL BOM-less-UTF-16 heuristic `decode_text_bytes` uses — a
/// UTF-16 file has a NUL in every other byte by design, so both checks must
/// run before the NUL-means-binary conclusion, or a BOM'd (or BOM-less)
/// UTF-16 file with an unknown extension gets skipped as "binary". Otherwise
/// returns the whole file decoded (UTF-8 lossily, or UTF-16 if detected) —
/// real words survive and odd bytes become harmless replacement chars.
fn sniff_text(path: &Path) -> Result<Option<String>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut head = [0u8; 8192];
    let n = file.read(&mut head)?;
    let has_utf16_bom =
        head[..n].starts_with(&[0xFF, 0xFE]) || head[..n].starts_with(&[0xFE, 0xFF]);
    let looks_bomless_utf16 = !has_utf16_bom && detect_bomless_utf16(&head[..n]).is_some();
    if !has_utf16_bom && !looks_bomless_utf16 && head[..n].contains(&0) {
        return Ok(None);
    }
    let mut rest = Vec::new();
    file.read_to_end(&mut rest)?;
    let mut all = Vec::with_capacity(n + rest.len());
    all.extend_from_slice(&head[..n]);
    all.extend_from_slice(&rest);
    Ok(Some(decode_text_bytes(&all)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn make_meta(path: PathBuf, ext: &str, name: &str) -> FileMeta {
        FileMeta {
            doc_id: 1,
            path,
            name: name.to_owned(),
            ext: ext.to_owned(),
            mtime: 0,
            size: 0,
        }
    }

    #[test]
    fn txt_extraction_works() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        write!(f, "some content").unwrap();
        let meta = make_meta(f.path().to_path_buf(), "txt", "test.txt");
        let doc = extract_text(meta).unwrap();
        assert_eq!(doc.title, "test.txt");
        assert_eq!(doc.body, "some content");
    }

    #[test]
    fn txt_extraction_yields_default_props() {
        // Plain text has no document-internal properties to extract.
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        write!(f, "some content").unwrap();
        let meta = make_meta(f.path().to_path_buf(), "txt", "test.txt");
        let doc = extract_text(meta).unwrap();
        assert_eq!(doc.props, crate::types::DocProps::default());
    }

    #[test]
    fn unknown_ext_binary_is_unsupported() {
        // A binary blob (NUL bytes) with an unrecognised extension is skipped.
        let mut f = NamedTempFile::with_suffix(".xyz").unwrap();
        f.write_all(&[0u8, 1, 2, 0xFF, b'h', b'i', 0u8]).unwrap();
        let meta = make_meta(f.path().to_path_buf(), "xyz", "fake.xyz");
        let err = extract_text(meta).unwrap_err();
        assert!(
            err.is_unsupported(),
            "binary unknown-ext file must be Unsupported, got: {err}"
        );
    }

    #[test]
    fn unknown_ext_text_is_sniffed_and_extracted() {
        let mut f = NamedTempFile::with_suffix(".xyz").unwrap();
        write!(f, "plain text content with a marker sniffword here").unwrap();
        let meta = make_meta(f.path().to_path_buf(), "xyz", "note.xyz");
        let doc = extract_text(meta).unwrap();
        assert!(
            doc.body.contains("sniffword"),
            "unknown-ext text must be sniffed + indexed"
        );
    }

    #[test]
    fn extended_code_extension_extracts() {
        let mut f = NamedTempFile::with_suffix(".cpp").unwrap();
        write!(f, "// header\nint main() {{ return 0; }} // cppmarker").unwrap();
        let meta = make_meta(f.path().to_path_buf(), "cpp", "main.cpp");
        let doc = extract_text(meta).unwrap();
        assert!(
            doc.body.contains("cppmarker"),
            ".cpp must extract as plain text"
        );
    }

    #[test]
    fn unknown_ext_utf16_bom_is_sniffed_via_bom() {
        // An unknown-extension file with a UTF-16 BOM has NULs throughout its
        // header (every other byte, by design) — the plain NUL-in-header
        // check must not treat it as binary.
        let mut f = NamedTempFile::with_suffix(".wcsunknown3").unwrap();
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(
            "bomsniffword here"
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes()),
        );
        f.write_all(&bytes).unwrap();
        let meta = make_meta(f.path().to_path_buf(), "wcsunknown3", "note.wcsunknown3");
        let doc = extract_text(meta).unwrap();
        assert!(
            doc.body.contains("bomsniffword"),
            "UTF-16 BOM unknown-ext file must be sniffed via the BOM check: {:?}",
            doc.body
        );
    }

    #[test]
    fn unknown_ext_bomless_utf16_is_sniffed_via_heuristic() {
        // A BOM-less UTF-16LE file (some Windows tools write this) with an
        // unknown extension also has NULs throughout its header, but no BOM
        // to short-circuit the binary check — the alternating-NUL heuristic
        // must catch it too, or it's misclassified as binary and skipped.
        let mut f = NamedTempFile::with_suffix(".wcsunknown4").unwrap();
        let bytes: Vec<u8> = "bomlesssniffword here in unknown extension"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        f.write_all(&bytes).unwrap();
        let meta = make_meta(f.path().to_path_buf(), "wcsunknown4", "note.wcsunknown4");
        let doc = extract_text(meta).unwrap();
        assert!(
            doc.body.contains("bomlesssniffword"),
            "BOM-less UTF-16 unknown-ext file must be sniffed via the heuristic: {:?}",
            doc.body
        );
        assert!(
            !doc.body.contains('\0'),
            "decoded text must not contain NUL bytes: {:?}",
            doc.body
        );
    }

    #[test]
    fn body_capped_at_4mb() {
        let mut f = NamedTempFile::with_suffix(".txt").unwrap();
        // Write slightly more than 4 MB.
        let chunk = "a".repeat(1024);
        for _ in 0..(4 * 1024 + 10) {
            write!(f, "{}", chunk).unwrap();
        }
        let meta = make_meta(f.path().to_path_buf(), "txt", "big.txt");
        let doc = extract_text(meta).unwrap();
        assert!(doc.body.len() <= 4 * 1024 * 1024);
    }
}
