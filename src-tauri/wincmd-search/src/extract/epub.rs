//! Bounded EPUB extraction.
//!
//! EPUB is a ZIP container.  We accept only the package-defined spine order,
//! cap every read, and extract text from XHTML/XML markup locally.  No book
//! text leaves the device and malformed/encrypted archives stay explicit skips.

use quick_xml::events::Event as XmlEvent;
use quick_xml::Reader as XmlReader;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::error::{Result, SearchError};

const MAX_EPUB_PART_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EPUB_SPINE_ITEMS: usize = 256;

pub fn extract_epub(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|e| SearchError::Extract(e.to_string()))?;
    let container = read_part(&mut archive, "META-INF/container.xml")?;
    let package_path = rootfile_path(&container)
        .ok_or_else(|| SearchError::Extract("EPUB package rootfile is missing".into()))?;
    let package = read_part(&mut archive, &package_path)?;
    let (manifest, spine) = parse_package(&package);
    if spine.is_empty() || spine.len() > MAX_EPUB_SPINE_ITEMS {
        return Err(SearchError::Extract(
            "EPUB spine is missing or exceeds the item limit".into(),
        ));
    }

    let base = package_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut text = String::new();
    for idref in spine {
        let Some(href) = manifest.get(&idref) else {
            continue;
        };
        let part_path = resolve_part(base, href)?;
        let part = read_part(&mut archive, &part_path)?;
        append_markup_text(&part, &mut text);
        text.push('\n');
    }
    if text.trim().is_empty() {
        return Err(SearchError::Extract(
            "EPUB contains no readable spine text".into(),
        ));
    }
    Ok(text)
}

fn read_part(archive: &mut ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut entry = archive
        .by_name(name)
        .map_err(|_| SearchError::Extract(format!("EPUB part is missing: {name}")))?;
    if entry.encrypted() {
        return Err(SearchError::Unsupported("encrypted EPUB input".into()));
    }
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(MAX_EPUB_PART_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(SearchError::Io)?;
    if bytes.len() as u64 > MAX_EPUB_PART_BYTES {
        return Err(SearchError::Extract(format!(
            "EPUB part exceeds {} MiB",
            MAX_EPUB_PART_BYTES / 1024 / 1024
        )));
    }
    Ok(bytes)
}

fn rootfile_path(xml: &[u8]) -> Option<String> {
    let mut reader = XmlReader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Empty(e)) | Ok(XmlEvent::Start(e))
                if e.local_name().as_ref() == b"rootfile" =>
            {
                return e.attributes().flatten().find_map(|attr| {
                    (attr.key.local_name().as_ref() == b"full-path")
                        .then(|| String::from_utf8_lossy(attr.value.as_ref()).to_string())
                });
            }
            Ok(XmlEvent::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

fn parse_package(xml: &[u8]) -> (HashMap<String, String>, Vec<String>) {
    let mut reader = XmlReader::from_reader(xml);
    let mut manifest = HashMap::new();
    let mut spine = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Empty(e)) if e.local_name().as_ref() == b"item" => {
                let attrs: HashMap<_, _> = e
                    .attributes()
                    .flatten()
                    .filter_map(|attr| {
                        std::str::from_utf8(attr.key.local_name().as_ref())
                            .ok()
                            .map(|key| {
                                (
                                    key.to_string(),
                                    String::from_utf8_lossy(attr.value.as_ref()).to_string(),
                                )
                            })
                    })
                    .collect();
                if attrs.get("media-type").is_some_and(|kind| {
                    kind.contains("xhtml") || kind.contains("html") || kind.contains("xml")
                }) {
                    if let (Some(id), Some(href)) = (attrs.get("id"), attrs.get("href")) {
                        manifest.insert(id.clone(), href.clone());
                    }
                }
            }
            Ok(XmlEvent::Empty(e)) if e.local_name().as_ref() == b"itemref" => {
                if let Some(idref) = e.attributes().flatten().find_map(|attr| {
                    (attr.key.local_name().as_ref() == b"idref")
                        .then(|| String::from_utf8_lossy(attr.value.as_ref()).to_string())
                }) {
                    spine.push(idref);
                }
            }
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    (manifest, spine)
}

fn resolve_part(base: &str, href: &str) -> Result<String> {
    if href.starts_with('/') || href.contains("\\") || href.split('/').any(|part| part == "..") {
        return Err(SearchError::Extract(
            "EPUB manifest path escapes its package".into(),
        ));
    }
    Ok(if base.is_empty() {
        href.to_string()
    } else {
        format!("{base}/{href}")
    })
}

fn append_markup_text(xml: &[u8], out: &mut String) {
    let mut reader = XmlReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Text(e)) => {
                if let Ok(decoded) = e.decode() {
                    if let Ok(unescaped) = quick_xml::escape::unescape(&decoded) {
                        out.push_str(&unescaped);
                    }
                }
            }
            Ok(XmlEvent::End(e))
                if matches!(
                    e.local_name().as_ref(),
                    b"p" | b"div" | b"h1" | b"h2" | b"h3" | b"li"
                ) =>
            {
                out.push('\n')
            }
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn epub(parts: &[(&str, &str)]) -> NamedTempFile {
        let file = NamedTempFile::with_suffix(".epub").unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(file.path()).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        file
    }

    #[test]
    fn extracts_only_manifest_spine_text_in_reading_order() {
        let file = epub(&[
            (
                "META-INF/container.xml",
                r#"<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#,
            ),
            (
                "OPS/book.opf",
                r#"<package><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="two"/><itemref idref="one"/></spine></package>"#,
            ),
            (
                "OPS/one.xhtml",
                "<html><body><p>first chapter</p></body></html>",
            ),
            (
                "OPS/two.xhtml",
                "<html><body><p>second chapter</p></body></html>",
            ),
            ("outside.xhtml", "<p>must not be indexed</p>"),
        ]);
        let text = extract_epub(file.path()).unwrap();
        assert!(text.find("second chapter").unwrap() < text.find("first chapter").unwrap());
        assert!(!text.contains("must not be indexed"));
    }

    #[test]
    fn rejects_manifest_traversal() {
        let err = resolve_part("OPS", "../outside.xhtml").unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }
}
