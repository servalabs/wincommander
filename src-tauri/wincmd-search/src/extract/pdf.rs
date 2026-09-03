use crate::error::{Result, SearchError};
use crate::types::DocProps;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashSet;
use std::path::Path;

/// Extract plain text from a PDF via lopdf.
pub fn extract_pdf(path: &Path) -> Result<(String, DocProps)> {
    let mut doc = Document::load(path).map_err(|e| SearchError::Extract(e.to_string()))?;
    materialize_inherited_resources(&mut doc);
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    let page_count = pages.len();
    let mut out = String::new();
    let mut ok_count = 0usize;
    for page_num in pages {
        if let Ok(text) = doc.extract_text(&[page_num]) {
            ok_count += 1;
            out.push_str(&dehyphenate(&text));
            out.push('\n');
        }
    }
    // KT: a single damaged page must not sink the whole file (partially-corrupt
    // PDFs are common), but if every page failed the doc is unreadable — surface
    // it as a per-file Extract error so the crawler counts a skip instead of
    // silently indexing an empty document.
    if page_count > 0 && ok_count == 0 {
        return Err(SearchError::Extract(format!(
            "all {page_count} page(s) failed text extraction"
        )));
    }
    // AcroForm field values (typed form input) live outside the page content
    // streams extract_text above walks, so a filled-in form's values would
    // otherwise never reach the index even though static labels around them do.
    out.push_str(&extract_acroform_text(&doc));
    let props = read_info_props(&doc);
    Ok((out, props))
}

/// `/Resources` is an inheritable page attribute (PDF 32000-1 §7.7.3.4): a page
/// may omit it and inherit from an ancestor `/Pages` node. lopdf 0.43's
/// `get_page_resources` only follows that inheritance when the ancestor's
/// `/Resources` is an *indirect reference* — an *inline* `/Resources` dictionary
/// on a parent node is ignored. Generators that put one shared `/Resources`
/// inline on the root `/Pages` node and omit per-page Resources (common for
/// filled forms and report templates) then extract as zero fonts: every `Tj`/
/// `TJ` fails to decode and lopdf yields only line-break whitespace, so the body
/// indexes empty and only the filename stays searchable. Copy the nearest
/// inherited `/Resources` down onto each page that lacks its own so lopdf's font
/// lookup — and therefore text decoding — succeeds.
fn materialize_inherited_resources(doc: &mut Document) {
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page_id in page_ids {
        let has_own = doc
            .get_dictionary(page_id)
            .map(|dict| dict.has(b"Resources"))
            .unwrap_or(false);
        if has_own {
            continue;
        }
        if let Some(resources) = inherited_resources(doc, page_id) {
            if let Ok(page) = doc.get_dictionary_mut(page_id) {
                page.set("Resources", resources);
            }
        }
    }
}

/// Walk `page_id`'s `/Parent` chain and return a clone of the first `/Resources`
/// entry found. The entry may be an inline dictionary or a reference — either is
/// valid to set back on the page, since lopdf resolves both from a page's own
/// `/Resources`. `seen` guards against a malformed `/Parent` cycle.
fn inherited_resources(doc: &Document, page_id: ObjectId) -> Option<Object> {
    let mut seen = HashSet::new();
    let mut current = page_id;
    loop {
        if !seen.insert(current) {
            return None;
        }
        let dict = doc.get_dictionary(current).ok()?;
        if let Ok(resources) = dict.get(b"Resources") {
            return Some(resources.clone());
        }
        current = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
}

/// Harvest text from `/Root /AcroForm /Fields[*]`: each terminal field's `/V`
/// (value), and its `/TU` (tooltip) + `/T` (name) for good measure, so a form
/// filled via a PDF viewer is as searchable as static page text. Any failure
/// to locate/dereference the AcroForm tree just yields no extra text — forms
/// are optional and must never fail extraction that otherwise succeeded above.
fn extract_acroform_text(doc: &Document) -> String {
    let mut out = String::new();
    let Ok(catalog) = doc.catalog() else {
        return out;
    };
    let Ok(acroform_obj) = catalog.get(b"AcroForm") else {
        return out;
    };
    let Ok((_, acroform_obj)) = doc.dereference(acroform_obj) else {
        return out;
    };
    let Ok(acroform) = acroform_obj.as_dict() else {
        return out;
    };
    let Ok(fields_obj) = acroform.get(b"Fields") else {
        return out;
    };
    let Ok((_, fields_obj)) = doc.dereference(fields_obj) else {
        return out;
    };
    let Ok(fields) = fields_obj.as_array() else {
        return out;
    };
    let mut seen = HashSet::new();
    for field in fields {
        walk_field(doc, field, &mut out, &mut seen);
    }
    out
}

/// Recursively walk a field and its `/Kids` (radio/checkbox groups and
/// hierarchical field names nest values under kid dictionaries), collecting
/// `/V`, `/TU`, and `/T` off every dictionary visited along the way. `seen`
/// guards against a `/Kids` cycle (malformed or adversarial PDF) sending this
/// into infinite recursion.
fn walk_field(doc: &Document, field: &Object, out: &mut String, seen: &mut HashSet<ObjectId>) {
    let Ok((id, field_obj)) = doc.dereference(field) else {
        return;
    };
    // Only referenced objects (Some(id)) can be reached twice and form a cycle;
    // inline/direct dictionaries dereference to None and are inherently unique,
    // so they skip the visited-set guard.
    if let Some(id) = id {
        if !seen.insert(id) {
            return;
        }
    }
    let Ok(dict) = field_obj.as_dict() else {
        return;
    };
    push_field_string(dict, b"V", out);
    push_field_string(dict, b"TU", out);
    push_field_string(dict, b"T", out);
    if let Ok(kids_obj) = dict.get(b"Kids") {
        if let Ok((_, kids_obj)) = doc.dereference(kids_obj) {
            if let Ok(kids) = kids_obj.as_array() {
                for kid in kids {
                    walk_field(doc, kid, out, seen);
                }
            }
        }
    }
}

/// Append a field dictionary entry's decoded text to `out`, if present. `/V`
/// may be a plain string, a `/Name`, or (for multi-select choice fields) an
/// array of such values — handle all three.
fn push_field_string(dict: &Dictionary, key: &[u8], out: &mut String) {
    let Ok(value) = dict.get(key) else {
        return;
    };
    match value {
        Object::String(bytes, _) => {
            out.push_str(&decode_pdf_string(bytes));
            out.push('\n');
        }
        Object::Name(bytes) => {
            out.push_str(&decode_pdf_string(bytes));
            out.push('\n');
        }
        Object::Array(items) => {
            for item in items {
                match item {
                    Object::String(bytes, _) | Object::Name(bytes) => {
                        out.push_str(&decode_pdf_string(bytes));
                        out.push('\n');
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Read Author/Title/Keywords/Subject from the trailer's `/Info` dictionary.
/// A missing trailer entry, missing dict, or unresolvable reference must never
/// fail extraction — body text above already stands on its own; props just
/// default to empty.
fn read_info_props(doc: &Document) -> DocProps {
    let info_obj = match doc.trailer.get(b"Info") {
        Ok(obj) => obj,
        Err(_) => return DocProps::default(),
    };
    let dict = match doc
        .dereference(info_obj)
        .ok()
        .and_then(|(_, obj)| obj.as_dict().ok())
    {
        Some(d) => d,
        None => return DocProps::default(),
    };
    let get = |key: &[u8]| -> String {
        dict.get(key)
            .ok()
            .and_then(|o| o.as_str().ok())
            .map(decode_pdf_string)
            .unwrap_or_default()
    };
    let keywords = get(b"Keywords");
    let subject = get(b"Subject");
    DocProps {
        author: get(b"Author"),
        doc_title: get(b"Title"),
        tags: [keywords, subject]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// PDF text strings are either PDFDocEncoding (roughly latin-1, plain bytes)
/// or UTF-16BE with a leading FE FF BOM — decode both rather than assume one.
fn decode_pdf_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (pairs, _) = rest.as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
        String::from_utf16_lossy(&units)
    } else {
        // KT: from_utf8_lossy mangled latin-1/PDFDocEncoding bytes (e.g. 0xE9
        // "é") to U+FFFD. Try UTF-8 first (some tools emit it without a BOM),
        // then fall back to latin-1 — every byte 0x00-0xFF maps 1:1 to a
        // Unicode scalar there, so this never fails or loses data.
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        }
    }
}

/// Join hyphenated line breaks (justified-text artifact: "commit-\nment") so the
/// tokenizer sees one word instead of two unmatchable fragments. Only joins when
/// the char before '-' is alphabetic and the first non-whitespace char after the
/// line break is lowercase alphabetic — this leaves list dashes and structured
/// tokens like "ISO-\n9001" untouched.
fn dehyphenate(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && i > 0 && chars[i - 1].is_alphabetic() {
            let mut j = i + 1;
            if chars.get(j) == Some(&'\r') {
                j += 1;
            }
            if chars.get(j) == Some(&'\n') {
                j += 1;
                while matches!(chars.get(j), Some(' ') | Some('\t')) {
                    j += 1;
                }
                if let Some(next) = chars.get(j) {
                    if next.is_lowercase() && next.is_alphabetic() {
                        i = j;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pdf_returns_error() {
        let result = extract_pdf(std::path::Path::new("no_such_file.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_pdf_bytes_returns_error() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        let mut f = NamedTempFile::with_suffix(".pdf").unwrap();
        write!(f, "not a pdf").unwrap();
        let result = extract_pdf(f.path());
        assert!(result.is_err());
    }

    /// A PDF that loads (valid xref/trailer) but whose only page's `/Parent`
    /// points back at itself. lopdf's resource-inheritance walk detects the
    /// cycle and `get_page_fonts` (called from `extract_text`) fails with
    /// `Error::ReferenceCycle` — a real "page loads but extraction fails"
    /// case, unlike a malformed-content stream, which lopdf's lenient
    /// non-strict content parser accepts as an empty page.
    fn write_pdf_with_self_referential_parent(dir: &std::path::Path) -> std::path::PathBuf {
        use lopdf::{dictionary, Document, Object};

        let p = dir.join("all_pages_broken.pdf");
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let page_id = doc.new_object_id();
        doc.objects.insert(
            page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => page_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        doc.save(&p).unwrap();
        p
    }

    /// A minimal valid one-page PDF (no self-reference) with no `/Info` set —
    /// the shared base for the props tests below.
    fn build_minimal_valid_pdf() -> Document {
        use lopdf::{dictionary, Object};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let page_id = doc.new_object_id();
        doc.objects.insert(
            page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    #[test]
    fn pdf_info_props_extracted() {
        use lopdf::{dictionary, Object, StringFormat};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mut doc = build_minimal_valid_pdf();
        // Title as UTF-16BE with a FE FF BOM — the other encoding PDF viewers
        // write for non-ASCII text strings; Author/Keywords/Subject stay
        // plain PDFDocEncoding-ish bytes to cover both decode paths at once.
        let mut title_bytes = vec![0xFEu8, 0xFF];
        for unit in "Quarterly Report".encode_utf16() {
            title_bytes.extend_from_slice(&unit.to_be_bytes());
        }
        let info_id = doc.add_object(dictionary! {
            "Author" => Object::string_literal("Jane Smith"),
            "Title" => Object::String(title_bytes, StringFormat::Literal),
            "Keywords" => Object::string_literal("alpha beta"),
            "Subject" => Object::string_literal("budgets"),
        });
        doc.trailer.set("Info", info_id);
        let path = dir.path().join("with_info.pdf");
        doc.save(&path).unwrap();

        let (_body, props) = extract_pdf(&path).unwrap();
        assert_eq!(props.author, "Jane Smith");
        assert_eq!(
            props.doc_title, "Quarterly Report",
            "UTF-16BE BOM title must decode: {props:?}"
        );
        assert_eq!(
            props.tags, "alpha beta budgets",
            "keywords + subject must be space-joined: {props:?}"
        );
    }

    #[test]
    fn author_latin1_byte_decodes_via_fallback_not_replacement_char() {
        use lopdf::{dictionary, Object, StringFormat};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mut doc = build_minimal_valid_pdf();
        // "José" as PDFDocEncoding/latin-1 bytes (no UTF-16BE BOM): 'é' = 0xE9.
        let author_bytes = vec![0x4A, 0x6F, 0x73, 0xE9];
        let info_id = doc.add_object(dictionary! {
            "Author" => Object::String(author_bytes, StringFormat::Literal),
        });
        doc.trailer.set("Info", info_id);
        let path = dir.path().join("latin1_author.pdf");
        doc.save(&path).unwrap();

        let (_body, props) = extract_pdf(&path).unwrap();
        assert_eq!(
            props.author, "José",
            "non-BOM latin-1 bytes must decode via the fallback, not become U+FFFD"
        );
    }

    #[test]
    fn pdf_without_info_yields_default_props() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mut doc = build_minimal_valid_pdf();
        let path = dir.path().join("no_info.pdf");
        doc.save(&path).unwrap();

        let (_body, props) = extract_pdf(&path).unwrap();
        assert_eq!(
            props,
            DocProps::default(),
            "PDF with no /Info dict must yield Default props"
        );
    }

    #[test]
    fn all_pages_failing_returns_extract_error_not_empty_ok() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let path = write_pdf_with_self_referential_parent(dir.path());
        let result = extract_pdf(&path);
        assert!(
            matches!(result, Err(SearchError::Extract(_))),
            "expected Extract error when every page fails, got {result:?}"
        );
    }

    #[test]
    fn dehyphenate_joins_hyphenated_linebreak() {
        assert_eq!(dehyphenate("commit-\nment"), "commitment");
    }

    #[test]
    fn dehyphenate_joins_crlf_with_leading_spaces() {
        assert_eq!(dehyphenate("commit-\r\n  ment"), "commitment");
    }

    #[test]
    fn dehyphenate_leaves_non_lowercase_continuation_unchanged() {
        assert_eq!(dehyphenate("ISO-\n9001"), "ISO-\n9001");
    }

    #[test]
    fn dehyphenate_joins_mid_sentence_hyphenation() {
        assert_eq!(
            dehyphenate("reads well-\nformed text"),
            "reads wellformed text"
        );
    }

    #[test]
    fn dehyphenate_trailing_hyphen_newline_does_not_panic() {
        assert_eq!(dehyphenate("foo-\n"), "foo-\n");
    }

    #[test]
    fn dehyphenate_no_hyphens_passes_through_unchanged() {
        let text = "a plain sentence with no hyphens at all";
        assert_eq!(dehyphenate(text), text);
    }

    /// A page with no `/Contents` at all (so `extract_text` yields nothing)
    /// but an `/AcroForm` with a single terminal `Tx` field carrying a `/V`
    /// marker string — the AcroForm walk must be the only source of that
    /// marker, proving it runs independently of page content extraction.
    #[test]
    fn acroform_field_value_extracted_when_page_has_no_contents() {
        use lopdf::{dictionary, Object};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mut doc = build_minimal_valid_pdf();
        let field_id = doc.add_object(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("comment"),
            "V" => Object::string_literal("ACROFORM_MARKER_VALUE"),
        });
        let acroform_id = doc.add_object(dictionary! {
            "Fields" => vec![field_id.into()],
        });
        let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
            catalog.set("AcroForm", acroform_id);
        }
        let path = dir.path().join("acroform_no_contents.pdf");
        doc.save(&path).unwrap();

        let (body, _props) = extract_pdf(&path).unwrap();
        assert!(
            body.contains("ACROFORM_MARKER_VALUE"),
            "AcroForm field /V must be extracted even when the page has no /Contents: {body:?}"
        );
    }

    /// A page with NO `/Resources` of its own whose font lives in an *inline*
    /// `/Resources` dictionary on the parent `/Pages` node — the exact shape
    /// (shared resources on the root, per-page Resources omitted) that lopdf
    /// 0.43 fails to inherit, dropping every `TJ` string and yielding only
    /// whitespace. `materialize_inherited_resources` must push the parent's
    /// resources down so the page's text decodes.
    #[test]
    fn inline_parent_resources_let_page_text_extract() {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Object, Stream};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mut doc = Document::with_version("1.7");

        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });

        // Show a marker via the TJ *array* operator (`[(text)] TJ`) — the form
        // that decodes to nothing when the page's fonts can't be resolved.
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                Operation::new("Td", vec![100.into(), 700.into()]),
                Operation::new(
                    "TJ",
                    vec![Object::Array(vec![Object::string_literal(
                        "INHERITEDMARKER",
                    )])],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));

        let pages_id = doc.new_object_id();
        let page_id = doc.new_object_id();
        doc.objects.insert(
            page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Contents" => content_id,
                // Deliberately no /Resources — it must be inherited.
            }),
        );
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
                // Inline dict (NOT an indirect reference) — the case lopdf 0.43
                // does not inherit on its own.
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => font_id },
                },
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let path = dir.path().join("inline_parent_resources.pdf");
        doc.save(&path).unwrap();

        let (body, _props) = extract_pdf(&path).unwrap();
        assert!(
            body.contains("INHERITEDMARKER"),
            "text drawn under a font from the parent's inline /Resources must \
             extract; got: {body:?}"
        );
    }
}
