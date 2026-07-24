use calamine::{open_workbook_auto, Reader};
use quick_xml::events::Event as XmlEvent;
use quick_xml::Reader as XmlReader;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::error::{Result, SearchError};
use crate::types::DocProps;

/// Returns true for OOXML parts that carry real user-authored text.
/// Excludes styles, themes, fonts, settings, numbering, etc.
fn is_content_part(name: &str) -> bool {
    // DOCX main story + headers/footers/notes (real user text)
    name == "word/document.xml"
        || (name.starts_with("word/header") && name.ends_with(".xml"))
        || (name.starts_with("word/footer") && name.ends_with(".xml"))
        || name == "word/footnotes.xml"
        || name == "word/endnotes.xml"
        // PPTX slide text (+ speaker notes)
        || (name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        || (name.starts_with("ppt/notesSlides/notesSlide") && name.ends_with(".xml"))
}

/// Extract text from .docx or .pptx (OOXML zip containers).
pub fn extract_office(path: &Path) -> Result<(String, DocProps)> {
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|e| SearchError::Extract(e.to_string()))?;

    let mut text = String::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| SearchError::Extract(e.to_string()))?;
        let name = entry.name().to_owned();
        // Only extract content-carrying parts; skip styles/themes/settings/etc.
        if is_content_part(&name) {
            // Cap decompressed size (audit M2): a zip bomb (tiny compressed,
            // huge decompressed) dropped into a watched folder that the
            // background crawler auto-indexes would otherwise expand unbounded
            // in memory. Bound the read; skip an oversize part rather than
            // accumulate gigabytes. The 50 MB on-disk cap does not bound this.
            const MAX_OOXML_PART_BYTES: u64 = 64 * 1024 * 1024;
            let mut xml_bytes = Vec::new();
            let read = entry
                .by_ref()
                .take(MAX_OOXML_PART_BYTES + 1)
                .read_to_end(&mut xml_bytes)?;
            if read as u64 > MAX_OOXML_PART_BYTES {
                continue;
            }
            text.push_str(&extract_xml_text(&xml_bytes));
            text.push('\n');
        }
    }
    let props = read_core_props(&mut archive);
    Ok((text, props))
}

/// Read `docProps/core.xml` (author/title/keywords/subject), the OOXML part
/// shared by docx/pptx/xlsx. Missing or corrupt core.xml must never fail
/// extraction — body text above already stands on its own; props just default
/// to empty.
fn read_core_props(archive: &mut ZipArchive<std::fs::File>) -> DocProps {
    // Same zip-bomb caution as the content-part loop above, scaled down: a
    // properties part has no legitimate reason to be large.
    const MAX_CORE_XML_BYTES: u64 = 1024 * 1024;
    let mut entry = match archive.by_name("docProps/core.xml") {
        Ok(e) => e,
        Err(_) => return DocProps::default(),
    };
    let mut bytes = Vec::new();
    let read = match entry
        .by_ref()
        .take(MAX_CORE_XML_BYTES + 1)
        .read_to_end(&mut bytes)
    {
        Ok(n) => n,
        Err(_) => return DocProps::default(),
    };
    if read as u64 > MAX_CORE_XML_BYTES {
        return DocProps::default();
    }
    parse_core_xml(&bytes)
}

#[derive(Clone, Copy)]
enum CoreField {
    Author,
    Title,
    Keywords,
    Subject,
}

/// Parse docProps/core.xml text nodes for the 4 properties we care about.
/// KT: match on LOCAL name (`e.local_name()`), not the qualified name — a
/// producer that declares core-properties/dc as the DEFAULT namespace emits
/// unprefixed `<creator>`/`<title>`/etc, and a literal `b"dc:creator"` match
/// silently drops those. creator/title/keywords/subject are collision-free
/// local names within core.xml.
fn parse_core_xml(bytes: &[u8]) -> DocProps {
    let mut reader = XmlReader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut props = DocProps::default();
    let mut keywords = String::new();
    let mut subject = String::new();
    let mut current: Option<CoreField> = None;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) => {
                current = match e.local_name().as_ref() {
                    b"creator" => Some(CoreField::Author),
                    b"title" => Some(CoreField::Title),
                    b"keywords" => Some(CoreField::Keywords),
                    b"subject" => Some(CoreField::Subject),
                    _ => None,
                };
            }
            Ok(XmlEvent::Text(e)) => {
                if let Some(field) = current {
                    if let Ok(decoded) = e.decode() {
                        if let Ok(unescaped) = quick_xml::escape::unescape(&decoded) {
                            match field {
                                CoreField::Author => props.author.push_str(&unescaped),
                                CoreField::Title => props.doc_title.push_str(&unescaped),
                                CoreField::Keywords => keywords.push_str(&unescaped),
                                CoreField::Subject => subject.push_str(&unescaped),
                            }
                        }
                    }
                }
            }
            Ok(XmlEvent::End(_)) => current = None,
            // Malformed core.xml: keep whatever was parsed before the error,
            // same fail-soft contract as a missing part.
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    props.tags = [keywords, subject]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    props
}

fn extract_xml_text(bytes: &[u8]) -> String {
    let mut reader = XmlReader::from_reader(bytes);
    // KT: Word/PowerPoint routinely split one word across several <w:t>/<a:t>
    // runs (rsid, spell-check, formatting boundaries). Pushing a space after
    // every text event breaks such words apart ("commit ment"). Concatenate
    // text nodes with no inserted space, and instead emit separators only at
    // structural boundaries (paragraph/table-cell/tab/break) below. trim_text
    // must be false so a genuine xml:space="preserve" trailing space (the
    // only thing keeping adjacent runs like "hello "+"world" from fusing into
    // "helloworld") survives.
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Text(e)) => {
                // quick-xml 0.41 dropped BytesText::unescape(): decode the raw
                // bytes (charset) then unescape XML entities separately.
                if let Ok(decoded) = e.decode() {
                    if let Ok(unescaped) = quick_xml::escape::unescape(&decoded) {
                        out.push_str(&unescaped);
                    }
                }
            }
            Ok(XmlEvent::End(e)) => match e.name().as_ref() {
                b"w:p" | b"a:p" => out.push('\n'),
                b"w:tc" => out.push(' '),
                _ => {}
            },
            Ok(XmlEvent::Empty(e)) => match e.name().as_ref() {
                b"w:tab" => out.push(' '),
                b"w:br" | b"a:br" => out.push('\n'),
                _ => {}
            },
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Extract text from .xlsx spreadsheets via calamine.
///
/// calamine 0.26 renamed `DataType` to `Data` — use `calamine::Data` here.
pub fn extract_xlsx(path: &Path) -> Result<(String, DocProps)> {
    let mut workbook = open_workbook_auto(path).map_err(|e| SearchError::Extract(e.to_string()))?;
    let sheet_names: Vec<String> = workbook.sheet_names();
    let mut out = String::new();
    for name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(name) {
            for row in range.rows() {
                for cell in row {
                    use calamine::Data;
                    let s = match cell {
                        Data::String(s) => s.clone(),
                        Data::Float(f) => f.to_string(),
                        Data::Int(i) => i.to_string(),
                        Data::Bool(b) => b.to_string(),
                        // KT: dates/durations were silently skipped, making them
                        // unsearchable even though they render as visible text.
                        Data::DateTime(dt) => dt.to_string(),
                        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
                        _ => continue,
                    };
                    out.push_str(&s);
                    out.push(' ');
                }
                out.push('\n');
            }
        }
    }
    let props = read_core_props_from_path(path);
    Ok((out, props))
}

/// Open the .xlsx as a zip solely to read `docProps/core.xml` — calamine's
/// `Reader` trait has no zip-part access. Any failure to open/parse falls
/// back to Default props; the body extraction above already succeeded
/// independently of this.
fn read_core_props_from_path(path: &Path) -> DocProps {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return DocProps::default(),
    };
    let mut archive = match ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return DocProps::default(),
    };
    read_core_props(&mut archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Build a minimal valid .docx in memory: a zip with word/document.xml
    /// containing a `<w:t>known words</w:t>` element.
    fn make_docx(words: &str) -> NamedTempFile {
        let body = format!(r#"<w:p><w:r><w:t>{}</w:t></w:r></w:p>"#, words);
        make_docx_body(&body)
    }

    /// Build a minimal valid .docx from a raw `<w:body>` inner XML string, for
    /// tests that need to control run/paragraph splitting directly.
    fn make_docx_body(body_xml: &str) -> NamedTempFile {
        let xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}</w:body></w:document>"#,
            body_xml
        );
        let f = NamedTempFile::with_suffix(".docx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        f
    }

    /// Build a minimal valid .pptx in memory: a zip with ppt/slides/slide1.xml
    /// containing `<a:t>` element with the given words.
    fn make_pptx(words: &str) -> NamedTempFile {
        let xml = format!(
            r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            words
        );
        let f = NamedTempFile::with_suffix(".pptx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("ppt/slides/slide1.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        f
    }

    /// Build a .docx with both word/document.xml (content) and word/styles.xml
    /// (non-content) to verify that style part is excluded from extraction.
    fn make_docx_with_styles(content_words: &str, style_words: &str) -> NamedTempFile {
        let doc_xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#,
            content_words
        );
        let style_xml = format!(
            r#"<?xml version="1.0"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style><w:name w:val="{}"/></w:style></w:styles>"#,
            style_words
        );
        let f = NamedTempFile::with_suffix(".docx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(doc_xml.as_bytes()).unwrap();
        zip.start_file("word/styles.xml", options).unwrap();
        zip.write_all(style_xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        f
    }

    /// Build a .docx with both word/document.xml and a docProps/core.xml part
    /// carrying author/title/keywords/subject, to exercise props extraction.
    fn make_docx_with_core_props(
        content_words: &str,
        author: &str,
        title: &str,
        keywords: &str,
        subject: &str,
    ) -> NamedTempFile {
        let doc_xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#,
            content_words
        );
        let core_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>{}</dc:creator><dc:title>{}</dc:title><cp:keywords>{}</cp:keywords><dc:subject>{}</dc:subject></cp:coreProperties>"#,
            author, title, keywords, subject
        );
        let f = NamedTempFile::with_suffix(".docx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(doc_xml.as_bytes()).unwrap();
        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(core_xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        f
    }

    #[test]
    fn docx_extracts_known_words() {
        let docx = make_docx("hello docx");
        let (result, _props) = extract_office(docx.path()).unwrap();
        assert!(result.contains("hello"), "expected 'hello' in: {result:?}");
        assert!(result.contains("docx"), "expected 'docx' in: {result:?}");
    }

    #[test]
    fn docx_styles_xml_excluded_from_body() {
        let docx = make_docx_with_styles("realcontent", "stylename");
        let (result, _props) = extract_office(docx.path()).unwrap();
        assert!(
            result.contains("realcontent"),
            "expected 'realcontent' in: {result:?}"
        );
        assert!(
            !result.contains("stylename"),
            "style part text leaked into body: {result:?}"
        );
    }

    #[test]
    fn pptx_extracts_slide_text() {
        let pptx = make_pptx("slidewords here");
        let (result, _props) = extract_office(pptx.path()).unwrap();
        assert!(
            result.contains("slidewords"),
            "expected 'slidewords' in: {result:?}"
        );
    }

    #[test]
    fn missing_file_returns_error() {
        let result = extract_office(std::path::Path::new("no_such_file.docx"));
        assert!(result.is_err());
    }

    #[test]
    fn xlsx_missing_file_returns_error() {
        let result = extract_xlsx(std::path::Path::new("no_such_file.xlsx"));
        assert!(result.is_err());
    }

    #[test]
    fn docx_word_split_across_runs_joins_into_one_word() {
        // Word fragments a single word across multiple <w:r> runs (rsid /
        // spell-check / formatting boundaries) — this must not become "commit ment".
        let docx =
            make_docx_body(r#"<w:p><w:r><w:t>commit</w:t></w:r><w:r><w:t>ment</w:t></w:r></w:p>"#);
        let (result, _props) = extract_office(docx.path()).unwrap();
        assert!(
            result.contains("commitment"),
            "expected 'commitment' as one word in: {result:?}"
        );
        assert!(
            !result.contains("commit ment"),
            "run-split word wrongly kept a space: {result:?}"
        );
    }

    #[test]
    fn docx_paragraph_boundary_keeps_words_separate() {
        // A paragraph break must become a separator so words from adjacent
        // paragraphs never fuse ("endStart").
        let docx = make_docx_body(
            r#"<w:p><w:r><w:t>fooend</w:t></w:r></w:p><w:p><w:r><w:t>Startbar</w:t></w:r></w:p>"#,
        );
        let (result, _props) = extract_office(docx.path()).unwrap();
        assert!(
            !result.contains("fooendStartbar"),
            "words across a paragraph boundary must not concatenate: {result:?}"
        );
        assert!(
            result.contains("fooend"),
            "expected 'fooend' in: {result:?}"
        );
        assert!(
            result.contains("Startbar"),
            "expected 'Startbar' in: {result:?}"
        );
    }

    #[test]
    fn docx_preserved_space_survives_trim_text_false() {
        // xml:space="preserve" trailing space must reach the output so
        // adjacent runs don't fuse into "helloworld" once trim_text(false).
        let docx = make_docx_body(
            r#"<w:p><w:r><w:t xml:space="preserve">hello </w:t></w:r><w:r><w:t>world</w:t></w:r></w:p>"#,
        );
        let (result, _props) = extract_office(docx.path()).unwrap();
        assert!(
            result.contains("hello world"),
            "expected 'hello world' with the preserved space intact in: {result:?}"
        );
        assert!(
            !result.contains("helloworld"),
            "preserved space was dropped, words fused: {result:?}"
        );
    }

    #[test]
    fn docx_tab_and_break_become_separators() {
        let docx = make_docx_body(
            r#"<w:p><w:r><w:t>col1</w:t><w:tab/><w:t>col2</w:t><w:br/><w:t>next</w:t></w:r></w:p>"#,
        );
        let (result, _props) = extract_office(docx.path()).unwrap();
        assert!(
            !result.contains("col1col2"),
            "tab must separate: {result:?}"
        );
        assert!(!result.contains("col2next"), "br must separate: {result:?}");
        assert!(result.contains("col1"), "expected 'col1' in: {result:?}");
        assert!(result.contains("col2"), "expected 'col2' in: {result:?}");
        assert!(result.contains("next"), "expected 'next' in: {result:?}");
    }

    #[test]
    fn docx_table_cell_boundary_separates_words() {
        let docx = make_docx_body(
            r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cellone</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>celltwo</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let (result, _props) = extract_office(docx.path()).unwrap();
        assert!(
            !result.contains("cellonecelltwo"),
            "adjacent table cells must not fuse: {result:?}"
        );
        assert!(
            result.contains("cellone"),
            "expected 'cellone' in: {result:?}"
        );
        assert!(
            result.contains("celltwo"),
            "expected 'celltwo' in: {result:?}"
        );
    }

    #[test]
    fn pptx_word_split_across_runs_joins_into_one_word() {
        let xml = r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>commit</a:t></a:r><a:r><a:t>ment</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let f = NamedTempFile::with_suffix(".pptx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("ppt/slides/slide1.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();

        let (result, _props) = extract_office(f.path()).unwrap();
        assert!(
            result.contains("commitment"),
            "expected 'commitment' as one word in: {result:?}"
        );
        assert!(
            !result.contains("commit ment"),
            "run-split word wrongly kept a space: {result:?}"
        );
    }

    #[test]
    fn pptx_paragraph_boundary_keeps_words_separate() {
        let xml = r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>fooend</a:t></a:r></a:p><a:p><a:r><a:t>Startbar</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let f = NamedTempFile::with_suffix(".pptx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("ppt/slides/slide1.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();

        let (result, _props) = extract_office(f.path()).unwrap();
        assert!(
            !result.contains("fooendStartbar"),
            "words across a paragraph boundary must not concatenate: {result:?}"
        );
        assert!(result.contains("fooend"));
        assert!(result.contains("Startbar"));
    }

    #[test]
    fn docx_core_props_extracted() {
        let docx = make_docx_with_core_props(
            "body text",
            "Jane Smith",
            "Quarterly Report",
            "finance quarterly",
            "Q3 Results",
        );
        let (_body, props) = extract_office(docx.path()).unwrap();
        assert_eq!(props.author, "Jane Smith");
        assert_eq!(props.doc_title, "Quarterly Report");
        assert_eq!(
            props.tags, "finance quarterly Q3 Results",
            "keywords + subject must be space-joined: {props:?}"
        );
    }

    #[test]
    fn docx_core_props_with_default_namespace_still_extracted() {
        // Some producers declare core-properties/dc as the DEFAULT namespace,
        // emitting unprefixed <creator>/<title>/<keywords>/<subject> — a
        // literal b"dc:creator" match would silently drop these.
        let doc_xml = r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>body text</w:t></w:r></w:p></w:body></w:document>"#;
        let core_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><coreProperties xmlns="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><creator>Default NS Author</creator><title>Default NS Title</title><keywords>alpha</keywords><subject>beta</subject></coreProperties>"#;
        let f = NamedTempFile::with_suffix(".docx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(doc_xml.as_bytes()).unwrap();
        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(core_xml.as_bytes()).unwrap();
        zip.finish().unwrap();

        let (_body, props) = extract_office(f.path()).unwrap();
        assert_eq!(props.author, "Default NS Author");
        assert_eq!(props.doc_title, "Default NS Title");
        assert_eq!(props.tags, "alpha beta");
    }

    #[test]
    fn docx_without_core_xml_yields_default_props_but_still_extracts_body() {
        let docx = make_docx("hello docx");
        let (body, props) = extract_office(docx.path()).unwrap();
        assert_eq!(
            props,
            DocProps::default(),
            "docx with no docProps/core.xml part must yield Default props"
        );
        assert!(
            body.contains("hello"),
            "body extraction must still work: {body:?}"
        );
    }

    #[test]
    fn xlsx_core_props_read_via_zip_helper() {
        // Building a fully valid xlsx workbook (sheet1.xml/workbook.xml/rels)
        // just to exercise props parsing is unnecessary — read_core_props_from_path
        // only opens the file as a zip and reads docProps/core.xml, the exact
        // same code path extract_xlsx delegates to.
        let core_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Sheet Author</dc:creator><dc:title>Budget</dc:title><cp:keywords>numbers</cp:keywords><dc:subject>2026</dc:subject></cp:coreProperties>"#;
        let f = NamedTempFile::with_suffix(".xlsx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(core_xml.as_bytes()).unwrap();
        zip.finish().unwrap();

        let props = read_core_props_from_path(f.path());
        assert_eq!(props.author, "Sheet Author");
        assert_eq!(props.doc_title, "Budget");
        assert_eq!(props.tags, "numbers 2026");
    }

    #[test]
    fn xlsx_missing_core_xml_yields_default_props() {
        let f = NamedTempFile::with_suffix(".xlsx").unwrap();
        let file = std::fs::File::create(f.path()).unwrap();
        let zip = zip::ZipWriter::new(file);
        zip.finish().unwrap();

        let props = read_core_props_from_path(f.path());
        assert_eq!(props, DocProps::default());
    }
}
