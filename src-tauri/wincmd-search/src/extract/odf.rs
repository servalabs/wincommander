// SPDX-License-Identifier: AGPL-3.0-or-later
//! OpenDocument Format (.odt/.ods/.odp) content + metadata extraction.
//!
//! ODF is a zip container (same shape as OOXML): `content.xml` holds the
//! document body, `meta.xml` holds author/title/keyword properties. This
//! reuses the zip + quick-xml machinery already in `office.rs` — no new
//! dependency.

use quick_xml::events::Event as XmlEvent;
use quick_xml::Reader as XmlReader;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::error::{Result, SearchError};
use crate::types::DocProps;

/// Extract text + props from a .odt/.ods/.odp file.
pub fn extract_odf(path: &Path) -> Result<(String, DocProps)> {
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|e| SearchError::Extract(e.to_string()))?;

    let body_xml = read_zip_part(&mut archive, "content.xml", MAX_CONTENT_XML_BYTES)
        .ok_or_else(|| SearchError::Extract("content.xml missing or oversize".to_string()))?;
    let text = extract_body_text(&body_xml);

    let props = read_zip_part(&mut archive, "meta.xml", MAX_META_XML_BYTES)
        .map(|bytes| parse_meta_xml(&bytes))
        .unwrap_or_default();

    Ok((text, props))
}

// Zip-bomb caution (same rationale as office.rs::extract_office): bound the
// decompressed read so a tiny-compressed/huge-decompressed part can't expand
// unbounded in memory for the background crawler.
const MAX_CONTENT_XML_BYTES: u64 = 64 * 1024 * 1024;
const MAX_META_XML_BYTES: u64 = 1024 * 1024;

/// Read one zip part fully into memory, capped at `max_bytes`. Returns `None`
/// on a missing part, a read error, or an oversize part — callers treat that
/// as "nothing to extract" rather than a hard failure.
fn read_zip_part(
    archive: &mut ZipArchive<std::fs::File>,
    name: &str,
    max_bytes: u64,
) -> Option<Vec<u8>> {
    let mut entry = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    let read = entry
        .by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if read as u64 > max_bytes {
        return None;
    }
    Some(bytes)
}

/// Extract human text from `content.xml`, restricted to `<office:body>` so
/// automatic/named style definitions (which live alongside the body in the
/// same part, unlike OOXML) never leak into the indexed text.
fn extract_body_text(bytes: &[u8]) -> String {
    let mut reader = XmlReader::from_reader(bytes);
    // Preserve real whitespace inside runs; separators are emitted explicitly
    // at structural boundaries below (same approach as office.rs).
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    let mut buf = Vec::new();
    let mut body_depth: u32 = 0; // >0 while inside <office:body>

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) => {
                if e.local_name().as_ref() == b"body" {
                    body_depth += 1;
                }
            }
            Ok(XmlEvent::End(e)) => {
                let local = e.local_name();
                let local = local.as_ref();
                if body_depth > 0 {
                    match local {
                        // Paragraph / heading end → hard separator.
                        b"p" | b"h" => out.push('\n'),
                        // Table cell end → soft separator (adjacent cells
                        // must not fuse into one word).
                        b"table-cell" => out.push(' '),
                        _ => {}
                    }
                }
                if local == b"body" {
                    body_depth = body_depth.saturating_sub(1);
                }
            }
            Ok(XmlEvent::Empty(e)) => {
                if body_depth > 0 {
                    match e.local_name().as_ref() {
                        b"tab" => out.push(' '),
                        b"line-break" => out.push('\n'),
                        _ => {}
                    }
                }
            }
            Ok(XmlEvent::Text(e)) => {
                if body_depth > 0 {
                    if let Ok(decoded) = e.decode() {
                        if let Ok(unescaped) = quick_xml::escape::unescape(&decoded) {
                            out.push_str(&unescaped);
                        }
                    }
                }
            }
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

#[derive(Clone, Copy)]
enum MetaField {
    Author,
    Title,
    Keyword,
    Description,
}

/// Parse `meta.xml` (`<office:document-meta><office:meta>…`) for
/// `dc:creator`/`dc:title`/`dc:description` + repeatable `meta:keyword`.
/// Malformed or partial meta.xml must never fail extraction — the body text
/// already stands on its own; props just default to whatever was parsed
/// before the error (same fail-soft contract as office.rs::parse_core_xml).
fn parse_meta_xml(bytes: &[u8]) -> DocProps {
    let mut reader = XmlReader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut props = DocProps::default();
    let mut keywords: Vec<String> = Vec::new();
    let mut description = String::new();
    let mut current: Option<MetaField> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) => {
                current = match e.local_name().as_ref() {
                    b"creator" => Some(MetaField::Author),
                    b"title" => Some(MetaField::Title),
                    b"keyword" => Some(MetaField::Keyword),
                    b"description" => Some(MetaField::Description),
                    _ => None,
                };
                // A new <meta:keyword> starts a fresh accumulator slot.
                if matches!(current, Some(MetaField::Keyword)) {
                    keywords.push(String::new());
                }
            }
            Ok(XmlEvent::Text(e)) => {
                if let Some(field) = current {
                    if let Ok(decoded) = e.decode() {
                        if let Ok(unescaped) = quick_xml::escape::unescape(&decoded) {
                            match field {
                                MetaField::Author => props.author.push_str(&unescaped),
                                MetaField::Title => props.doc_title.push_str(&unescaped),
                                MetaField::Keyword => {
                                    if let Some(last) = keywords.last_mut() {
                                        last.push_str(&unescaped);
                                    }
                                }
                                MetaField::Description => description.push_str(&unescaped),
                            }
                        }
                    }
                }
            }
            Ok(XmlEvent::End(_)) => current = None,
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    let mut tags: Vec<String> = keywords.into_iter().filter(|s| !s.is_empty()).collect();
    if !description.is_empty() {
        tags.push(description);
    }
    props.tags = tags.join(" ");
    props
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Build a minimal valid ODF file (.odt/.ods/.odp share the same zip
    /// shape) with the given `content.xml` body and optional `meta.xml`.
    fn make_odf(suffix: &str, body_xml: &str, meta_xml: Option<&str>) -> NamedTempFile {
        let content = format!(
            r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:automatic-styles><style:style style:name="ignoreme"/></office:automatic-styles><office:body>{}</office:body></office:document-content>"#,
            body_xml
        );
        let f = NamedTempFile::with_suffix(suffix).unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("content.xml", options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
        if let Some(meta) = meta_xml {
            zip.start_file("meta.xml", options).unwrap();
            zip.write_all(meta.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        f
    }

    #[test]
    fn odt_extracts_known_words() {
        let odt = make_odf(
            ".odt",
            r#"<office:text><text:p>hello odt document</text:p></office:text>"#,
            None,
        );
        let (result, _props) = extract_odf(odt.path()).unwrap();
        assert!(result.contains("hello odt document"), "got: {result:?}");
    }

    #[test]
    fn automatic_styles_excluded_from_body() {
        let odt = make_odf(
            ".odt",
            r#"<office:text><text:p>realcontent</text:p></office:text>"#,
            None,
        );
        let (result, _props) = extract_odf(odt.path()).unwrap();
        assert!(result.contains("realcontent"), "got: {result:?}");
        assert!(
            !result.contains("ignoreme"),
            "automatic-styles text leaked into body: {result:?}"
        );
    }

    #[test]
    fn paragraph_boundary_keeps_words_separate() {
        let odt = make_odf(
            ".odt",
            r#"<office:text><text:p>fooend</text:p><text:p>Startbar</text:p></office:text>"#,
            None,
        );
        let (result, _props) = extract_odf(odt.path()).unwrap();
        assert!(
            !result.contains("fooendStartbar"),
            "words across a paragraph boundary must not concatenate: {result:?}"
        );
        assert!(result.contains("fooend"));
        assert!(result.contains("Startbar"));
    }

    #[test]
    fn ods_table_cells_extracted_and_separated() {
        let ods = make_odf(
            ".ods",
            r#"<office:spreadsheet><table:table><table:table-row>
                <table:table-cell><text:p>cellone</text:p></table:table-cell>
                <table:table-cell><text:p>celltwo</text:p></table:table-cell>
            </table:table-row></table:table></office:spreadsheet>"#,
            None,
        );
        let (result, _props) = extract_odf(ods.path()).unwrap();
        assert!(
            !result.contains("cellonecelltwo"),
            "adjacent table cells must not fuse: {result:?}"
        );
        assert!(result.contains("cellone"));
        assert!(result.contains("celltwo"));
    }

    #[test]
    fn odp_slide_text_extracted() {
        let odp = make_odf(
            ".odp",
            r#"<office:presentation><draw:page><draw:frame><draw:text-box>
                <text:p>slidewords here</text:p>
            </draw:text-box></draw:frame></draw:page></office:presentation>"#,
            None,
        );
        let (result, _props) = extract_odf(odp.path()).unwrap();
        assert!(result.contains("slidewords here"), "got: {result:?}");
    }

    #[test]
    fn meta_xml_props_extracted() {
        let meta = r#"<?xml version="1.0"?><office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0"><office:meta><dc:creator>Jane Smith</dc:creator><dc:title>Quarterly Report</dc:title><meta:keyword>finance</meta:keyword><meta:keyword>quarterly</meta:keyword><dc:description>Q3 Results</dc:description></office:meta></office:document-meta>"#;
        let odt = make_odf(
            ".odt",
            r#"<office:text><text:p>body text</text:p></office:text>"#,
            Some(meta),
        );
        let (_body, props) = extract_odf(odt.path()).unwrap();
        assert_eq!(props.author, "Jane Smith");
        assert_eq!(props.doc_title, "Quarterly Report");
        assert_eq!(
            props.tags, "finance quarterly Q3 Results",
            "keywords + description must be space-joined: {props:?}"
        );
    }

    #[test]
    fn missing_meta_xml_yields_default_props_but_still_extracts_body() {
        let odt = make_odf(
            ".odt",
            r#"<office:text><text:p>hello odt</text:p></office:text>"#,
            None,
        );
        let (body, props) = extract_odf(odt.path()).unwrap();
        assert_eq!(props, DocProps::default());
        assert!(body.contains("hello odt"));
    }

    #[test]
    fn missing_file_returns_error() {
        let result = extract_odf(std::path::Path::new("no_such_file.odt"));
        assert!(result.is_err());
    }

    #[test]
    fn tab_and_line_break_become_separators() {
        let odt = make_odf(
            ".odt",
            r#"<office:text><text:p>col1<text:tab/>col2<text:line-break/>next</text:p></office:text>"#,
            None,
        );
        let (result, _props) = extract_odf(odt.path()).unwrap();
        assert!(
            !result.contains("col1col2"),
            "tab must separate: {result:?}"
        );
        assert!(
            !result.contains("col2next"),
            "line-break must separate: {result:?}"
        );
        assert!(result.contains("col1"));
        assert!(result.contains("col2"));
        assert!(result.contains("next"));
    }
}
