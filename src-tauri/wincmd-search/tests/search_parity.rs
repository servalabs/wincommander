// SPDX-License-Identifier: AGPL-3.0-or-later
//! Coverage and quality harness for the wincmd-search content engine.
//!
//! Answers the question "does in-file content search actually work, across the
//! formats a real user has on disk, and is it fast + accurate?" Three concerns
//! are measured automatically:
//!
//! COVERAGE — every format we claim to support extracts real body text (direct
//! `extract_text`), and textual files with odd/unknown extensions are still
//! indexed via the sniff fallback.
//!
//! ACCURACY — a labelled multi-format corpus yields recall@k = 1.0, an MRR, 0
//! false positives, and non-empty snippets.
//!
//! ROBUSTNESS — a battery of "nasty" query strings (paths, operators, stray
//! quotes) never returns an Err; a search box must not crash.
//!
//! Fast tests run in CI (`cargo test -p wincmd-search`). The heavy measurement
//! test is `#[ignore]`:
//!   cargo test -p wincmd-search --release parity_perf -- --ignored --nocapture
//! Set WCS_CORPUS=<dir> to fold a real directory tree (e.g. D:\GitHub) into the
//! perf run; synthetic dummy files are generated for any format the real tree
//! is missing, so every format is always exercised.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tempfile::TempDir;
use wincmd_search::{
    crawler::{build_globset, collect_files, doc_id_for},
    extract::extract_text,
    types::{ContentQuery, FileMeta, IndexConfig},
    SearchEngine,
};

// ── Corpus builders ─────────────────────────────────────────────────────────

/// Write a UTF-8 text file verbatim; returns the path.
fn write_text(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    p
}

/// Zip up a set of (part-name, bytes) into an OOXML container (docx/pptx/xlsx).
fn write_ooxml(dir: &Path, name: &str, parts: &[(&str, String)]) -> PathBuf {
    let p = dir.join(name);
    let file = fs::File::create(&p).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (part, xml) in parts {
        zip.start_file(*part, opts).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    p
}

/// Minimal valid .docx carrying `words` in the main story.
fn write_docx(dir: &Path, name: &str, words: &str) -> PathBuf {
    let doc = format!(
        r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{words}</w:t></w:r></w:p></w:body></w:document>"#
    );
    write_ooxml(dir, name, &[("word/document.xml", doc)])
}

/// Minimal valid .pptx with one slide carrying `words`.
fn write_pptx(dir: &Path, name: &str, words: &str) -> PathBuf {
    let slide = format!(
        r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{words}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
    );
    write_ooxml(dir, name, &[("ppt/slides/slide1.xml", slide)])
}

/// Minimal valid .xlsx (single inline-string cell) readable by calamine.
fn write_xlsx(dir: &Path, name: &str, words: &str) -> PathBuf {
    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_string();
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string();
    let workbook = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_string();
    let wb_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string();
    let sheet = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{words}</t></is></c></row></sheetData></worksheet>"#
    );
    write_ooxml(
        dir,
        name,
        &[
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", root_rels),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", wb_rels),
            ("xl/worksheets/sheet1.xml", sheet),
        ],
    )
}

/// Minimal valid ODF file (.odt/.ods/.odp share the same zip-of-XML shape) —
/// a `content.xml` part with the given `<office:body>` inner XML. Mirrors
/// `extract/odf.rs`'s own fixture builder.
fn write_odf(dir: &Path, name: &str, body_xml: &str) -> PathBuf {
    let content = format!(
        r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body>{body_xml}</office:body></office:document-content>"#
    );
    write_ooxml(dir, name, &[("content.xml", content)])
}

/// Minimal valid single-page .pdf with `words` drawn as a text object.
fn write_pdf(dir: &Path, name: &str, words: &str) -> PathBuf {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    let p = dir.join(name);
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![72.into(), 720.into()]),
            Operation::new("Tj", vec![Object::string_literal(words)]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(&p).unwrap();
    p
}

/// A format under test: extension, the unique body token, and how to build it.
struct FmtCase {
    ext: &'static str,
    token: &'static str,
    build: fn(&Path, &str, &str) -> PathBuf,
}

/// Wrap `write_text` so plain formats share one signature. The `body` arg is
/// the token; we surround it with realistic filler so BM25 has a real document.
fn build_plain(dir: &Path, name: &str, token: &str) -> PathBuf {
    let body = format!(
        "Project notes and status update. The unique marker for this file is {token}. \
         Additional context: revenue, network, security, configuration, and report data follow. \
         End of document."
    );
    write_text(dir, name, &body)
}

/// Every format the engine should be able to search inside, with a unique token.
/// `build_plain` covers text formats; office/pdf get their own writers.
fn all_format_cases() -> Vec<FmtCase> {
    vec![
        // ── Currently-supported plain text ──
        FmtCase {
            ext: "txt",
            token: "qzxtxt",
            build: build_plain,
        },
        FmtCase {
            ext: "md",
            token: "qzxmd",
            build: build_plain,
        },
        FmtCase {
            ext: "csv",
            token: "qzxcsv",
            build: build_plain,
        },
        FmtCase {
            ext: "json",
            token: "qzxjson",
            build: build_plain,
        },
        FmtCase {
            ext: "xml",
            token: "qzxxml",
            build: build_plain,
        },
        FmtCase {
            ext: "yaml",
            token: "qzxyaml",
            build: build_plain,
        },
        FmtCase {
            ext: "toml",
            token: "qzxtoml",
            build: build_plain,
        },
        FmtCase {
            ext: "log",
            token: "qzxlog",
            build: build_plain,
        },
        FmtCase {
            ext: "rs",
            token: "qzxrs",
            build: build_plain,
        },
        FmtCase {
            ext: "py",
            token: "qzxpy",
            build: build_plain,
        },
        FmtCase {
            ext: "js",
            token: "qzxjs",
            build: build_plain,
        },
        FmtCase {
            ext: "ts",
            token: "qzxts",
            build: build_plain,
        },
        FmtCase {
            ext: "css",
            token: "qzxcss",
            build: build_plain,
        },
        FmtCase {
            ext: "sh",
            token: "qzxsh",
            build: build_plain,
        },
        FmtCase {
            ext: "html",
            token: "qzxhtml",
            build: |d, n, t| {
                write_text(
                    d,
                    n,
                    &format!(
                        "<html><body><h1>Report</h1><p>marker {t} inside body</p></body></html>"
                    ),
                )
            },
        },
        // ── Binary containers ──
        FmtCase {
            ext: "docx",
            token: "qzxdocx",
            build: |d, n, t| write_docx(d, n, &format!("annual report marker {t} end")),
        },
        FmtCase {
            ext: "pptx",
            token: "qzxpptx",
            build: |d, n, t| write_pptx(d, n, &format!("slide deck marker {t} end")),
        },
        FmtCase {
            ext: "xlsx",
            token: "qzxxlsx",
            build: |d, n, t| write_xlsx(d, n, t),
        },
        FmtCase {
            ext: "pdf",
            token: "qzxpdf",
            build: |d, n, t| write_pdf(d, n, t),
        },
        FmtCase {
            ext: "odt",
            token: "qzxodt",
            build: |d, n, t| {
                write_odf(
                    d,
                    n,
                    &format!(
                        r#"<office:text><text:p>document report marker {t} end</text:p></office:text>"#
                    ),
                )
            },
        },
        FmtCase {
            ext: "ods",
            token: "qzxods",
            build: |d, n, t| {
                write_odf(
                    d,
                    n,
                    &format!(
                        r#"<office:spreadsheet><table:table><table:table-row><table:table-cell><text:p>spreadsheet marker {t} end</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet>"#
                    ),
                )
            },
        },
        FmtCase {
            ext: "odp",
            token: "qzxodp",
            build: |d, n, t| {
                write_odf(
                    d,
                    n,
                    &format!(
                        r#"<office:presentation><draw:page><draw:frame><draw:text-box><text:p>slide marker {t} end</text:p></draw:text-box></draw:frame></draw:page></office:presentation>"#
                    ),
                )
            },
        },
        FmtCase {
            ext: "rtf",
            token: "qzxrtf",
            build: |d, n, t| {
                let body = format!(
                    r"{{\rtf1\ansi{{\fonttbl{{\f0 Calibri;}}}} document report marker {t} end}}"
                );
                write_text(d, n, &body)
            },
        },
        // ── Extended text formats (code / config / docs) — the coverage gap ──
        FmtCase {
            ext: "c",
            token: "qzxclang",
            build: build_plain,
        },
        FmtCase {
            ext: "cpp",
            token: "qzxcpp",
            build: build_plain,
        },
        FmtCase {
            ext: "h",
            token: "qzxhhdr",
            build: build_plain,
        },
        FmtCase {
            ext: "java",
            token: "qzxjava",
            build: build_plain,
        },
        FmtCase {
            ext: "go",
            token: "qzxgo",
            build: build_plain,
        },
        FmtCase {
            ext: "sql",
            token: "qzxsql",
            build: build_plain,
        },
        FmtCase {
            ext: "ps1",
            token: "qzxpsone",
            build: build_plain,
        },
        FmtCase {
            ext: "ini",
            token: "qzxini",
            build: build_plain,
        },
        FmtCase {
            ext: "tsx",
            token: "qzxtsx",
            build: build_plain,
        },
        FmtCase {
            ext: "jsx",
            token: "qzxjsx",
            build: build_plain,
        },
        FmtCase {
            ext: "conf",
            token: "qzxconf",
            build: build_plain,
        },
        FmtCase {
            ext: "rst",
            token: "qzxrst",
            build: build_plain,
        },
    ]
}

fn keyword_query(terms: &str, limit: usize) -> ContentQuery {
    ContentQuery {
        terms: terms.into(),
        roots: vec![],
        limit,
        offset: 0,
        keyword_only: true,
    }
}

fn make_config(index_dir: PathBuf, roots: Vec<PathBuf>) -> IndexConfig {
    IndexConfig {
        roots,
        exclusions: vec![],
        skip_paths: vec![],
        max_file_bytes: 50_000_000,
        index_dir,
    }
}

/// FileMeta for direct-extraction tests: real path + real extension, dummy rest.
fn meta_for(path: &Path) -> FileMeta {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    FileMeta {
        doc_id: doc_id_for(path),
        path: path.to_path_buf(),
        name,
        ext,
        mtime: 0,
        size: 0,
    }
}

// ── Test: direct extraction coverage ────────────────────────────────────────

/// Every format in `all_format_cases()` must extract its unique token via the
/// public `extract_text`. This isolates "extraction works" from indexing/query.
/// Pre-fix: the extended text formats (c/cpp/h/…) return Unsupported and FAIL —
/// that failure IS the coverage gap this harness exists to close.
#[test]
fn extraction_covers_every_format() {
    let dir = TempDir::new().unwrap();
    let mut failures = Vec::new();

    for case in all_format_cases() {
        let name = format!("fixture_{}.{}", case.ext, case.ext);
        let path = (case.build)(dir.path(), &name, case.token);
        let meta = meta_for(&path);
        match extract_text(meta) {
            Ok(doc) => {
                let hit = doc.body.to_lowercase().contains(case.token);
                println!(
                    "  [{}] .{:<5} token '{}' in body: {}",
                    if hit { "OK  " } else { "FAIL" },
                    case.ext,
                    case.token,
                    hit
                );
                if !hit {
                    failures.push(format!(
                        ".{} extracted but token '{}' missing",
                        case.ext, case.token
                    ));
                }
            }
            Err(e) => {
                println!("  [FAIL] .{:<5} extract error: {}", case.ext, e);
                failures.push(format!(".{} → {}", case.ext, e));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "extraction gaps:\n  {}",
        failures.join("\n  ")
    );
}

/// A textual file with an UNKNOWN extension must still be indexed while a
/// binary blob with an unknown extension must be skipped.
#[test]
fn extraction_sniffs_unknown_text_and_skips_binary() {
    let dir = TempDir::new().unwrap();

    // Unknown-extension text file → should extract via UTF-8 sniff.
    let text_path = write_text(
        dir.path(),
        "notes.wcsunknown",
        "meeting minutes: the unique marker is sniffabletoken and the action items follow",
    );
    let text = extract_text(meta_for(&text_path));
    assert!(
        text.map(|d| d.body.contains("sniffabletoken"))
            .unwrap_or(false),
        "textual file with unknown extension must be extracted via sniff",
    );

    // Binary blob (NUL bytes) with unknown extension → must NOT be indexed.
    let bin_path = dir.path().join("blob.wcsunknown2");
    fs::write(&bin_path, [0x00, 0x01, 0x02, 0xFF, b'h', b'i', 0x00, 0x7F]).unwrap();
    let bin = extract_text(meta_for(&bin_path));
    assert!(
        bin.is_err(),
        "binary blob must be rejected, not indexed as text"
    );
}

// ── Test: multi-format accuracy ─────────────────────────────────────────────

/// Index the full multi-format corpus and assert, per format, recall@10 = 1.0
/// with a non-empty snippet and zero cross-format false positives. Also prints
/// per-query latency so a plain `--nocapture` run shows accuracy AND speed.
#[test]
fn accuracy_multiformat_corpus() {
    let data = TempDir::new().unwrap();
    let index = TempDir::new().unwrap();

    let cases = all_format_cases();
    for case in &cases {
        let name = format!("doc_{}.{}", case.ext, case.ext);
        (case.build)(data.path(), &name, case.token);
    }

    let gs = build_globset(&[]).unwrap();
    let files = collect_files(&[data.path().to_path_buf()], &gs, &[], 50_000_000);
    let engine = SearchEngine::open(make_config(
        index.path().to_path_buf(),
        vec![data.path().to_path_buf()],
    ))
    .unwrap();
    engine.index_files_sync(files).unwrap();

    let mut missing = Vec::new();
    let mut no_mark: Vec<&str> = Vec::new();
    let mut fp = 0usize;
    let mut latencies_us = Vec::new();

    for case in &cases {
        let t0 = Instant::now();
        let hits = engine.search(&keyword_query(case.token, 10)).unwrap();
        latencies_us.push(t0.elapsed().as_micros() as u64);

        // A unique per-file token must return exactly its one file.
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        let want = format!("doc_{}.{}", case.ext, case.ext);
        let found = names.iter().any(|n| *n == want);
        if !found {
            missing.push(format!(".{} (token {})", case.ext, case.token));
        }
        if hits.len() > 1 {
            fp += hits.len() - 1;
        }
        // The token is a body term, so the snippet must highlight it with <mark>.
        let mark_ok = hits
            .first()
            .map(|h| h.snippet.contains("<mark>"))
            .unwrap_or(false);
        if found && !mark_ok {
            no_mark.push(case.ext);
        }
        println!(
            "  [{}] .{:<5} hits={} mark={} → {:?}",
            if found && hits.len() == 1 && mark_ok {
                "OK  "
            } else {
                "FAIL"
            },
            case.ext,
            hits.len(),
            mark_ok,
            names
        );
    }

    latencies_us.sort_unstable();
    let p50 = latencies_us[latencies_us.len() / 2];
    let p95 = latencies_us[(latencies_us.len() as f64 * 0.95) as usize];
    println!("\n── Multi-format accuracy ──");
    println!("  formats           : {}", cases.len());
    println!(
        "  recall@10         : {:.3}",
        (cases.len() - missing.len()) as f64 / cases.len() as f64
    );
    println!("  false positives   : {fp}");
    println!("  query p50 / p95   : {p50} µs / {p95} µs");

    assert!(
        missing.is_empty(),
        "formats not searchable (recall gap):\n  {}",
        missing.join("\n  ")
    );
    assert_eq!(fp, 0, "unique tokens must not cross-match other formats");
    assert!(
        no_mark.is_empty(),
        "body matches must be <mark>-highlighted; not for: {no_mark:?}"
    );
}

// ── Test: query robustness ──────────────────────────────────────────────────

/// A search box must never crash on user input. Every one of these — paths,
/// operators, stray quotes/brackets, symbols — must return Ok (empty or not),
/// never Err. Pre-fix, tantivy's QueryParser errors on most of these.
#[test]
fn query_robustness_never_errors() {
    let data = TempDir::new().unwrap();
    let index = TempDir::new().unwrap();
    write_text(
        data.path(),
        "a.txt",
        "the quick brown fox jumps over the lazy dog budget report",
    );

    let gs = build_globset(&[]).unwrap();
    let files = collect_files(&[data.path().to_path_buf()], &gs, &[], 50_000_000);
    let engine = SearchEngine::open(make_config(
        index.path().to_path_buf(),
        vec![data.path().to_path_buf()],
    ))
    .unwrap();
    engine.index_files_sync(files).unwrap();

    let nasty = [
        r"C:\Users\me\report", // Windows path (backslashes + colon)
        "foo:bar",             // colon → looks like a field selector
        "\"unclosed phrase",   // dangling quote
        "budget AND report",   // boolean operator
        "term OR",             // trailing operator
        "(oops",               // unbalanced paren
        "a || b",              // pipes
        "C++",                 // trailing plus-plus
        "report -",            // dangling minus
        "+plus",               // leading plus
        "*wild",               // leading wildcard
        "a/b/c",               // slashes
        "user@host.com",       // email
        "v1.2.3",              // dotted version
        "50%off",              // percent
        "a~b",                 // tilde (fuzzy)
        "^caret",              // caret (boost)
        "[bracket",            // unbalanced bracket
        "}brace",              // stray brace
        "!bang",               // bang
        "   ",                 // whitespace only
        "",                    // empty
    ];
    for q in nasty {
        let r = engine.search(&keyword_query(q, 10));
        assert!(r.is_ok(), "query {q:?} must not error, got: {:?}", r.err());
    }

    // And a plain path query should actually FIND the doc via its terms.
    let hits = engine
        .search(&keyword_query(r"C:\Users\budget", 10))
        .unwrap();
    assert!(
        !hits.is_empty(),
        "path-shaped query should still match 'budget' in the doc"
    );
}

// ── Test: perf + real corpus (heavy, ignored) ───────────────────────────────

/// Heavy measurement. Builds one synthetic dummy file per format (so every
/// format is always exercised) and, if WCS_CORPUS is set, folds in that real
/// directory tree. Prints discovery/index throughput and query-latency
/// percentiles. Run:
///   cargo test -p wincmd-search --release parity_perf -- --ignored --nocapture
#[test]
#[ignore]
fn parity_perf() {
    // Synthetic per-format dummy files — the "create missing formats" guarantee.
    let augment = TempDir::new().unwrap();
    let cases = all_format_cases();
    for case in &cases {
        let name = format!("dummy_{}.{}", case.ext, case.ext);
        (case.build)(augment.path(), &name, case.token);
    }

    let mut roots = vec![augment.path().to_path_buf()];
    if let Ok(real) = std::env::var("WCS_CORPUS") {
        println!("[parity_perf] folding in real corpus: {real}");
        roots.push(PathBuf::from(real));
    } else {
        println!("[parity_perf] WCS_CORPUS not set — synthetic per-format corpus only");
    }

    let exclusions: Vec<String> = [
        "node_modules",
        "target",
        ".git",
        "dist",
        "build",
        ".pytest_cache",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let gs = build_globset(&exclusions).unwrap();

    let t_disc = Instant::now();
    let files = collect_files(&roots, &gs, &[], 50_000_000);
    let disc_ms = t_disc.elapsed().as_millis();
    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let n = files.len();
    println!("  files discovered  : {n}");
    println!(
        "  total size        : {:.2} MB",
        total_bytes as f64 / 1_048_576.0
    );
    println!("  discovery         : {disc_ms} ms");
    if n == 0 {
        return;
    }

    let index = TempDir::new().unwrap();
    let engine =
        SearchEngine::open(make_config(index.path().to_path_buf(), roots.clone())).unwrap();
    let t_idx = Instant::now();
    engine.index_files_sync(files).unwrap();
    let idx_ms = t_idx.elapsed().as_millis().max(1);
    println!("  index build       : {idx_ms} ms");
    println!(
        "  throughput        : {:.0} files/s, {:.2} MB/s",
        n as f64 / (idx_ms as f64 / 1000.0),
        (total_bytes as f64 / 1_048_576.0) / (idx_ms as f64 / 1000.0)
    );

    // Every synthetic format token must be findable in the combined index.
    let mut fmt_missing = Vec::new();
    for case in &cases {
        if engine
            .search(&keyword_query(case.token, 5))
            .unwrap()
            .is_empty()
        {
            fmt_missing.push(case.ext);
        }
    }
    println!(
        "  per-format recall : {}/{} formats searchable{}",
        cases.len() - fmt_missing.len(),
        cases.len(),
        if fmt_missing.is_empty() {
            String::new()
        } else {
            format!(" (missing: {fmt_missing:?})")
        }
    );

    // Query-latency distribution over representative terms.
    let terms = [
        "the", "report", "system", "network", "security", "config", "data", "error",
    ];
    let mut lat = Vec::new();
    for _ in 0..30 {
        for t in &terms {
            let t0 = Instant::now();
            let _ = engine.search(&keyword_query(t, 20)).unwrap();
            lat.push(t0.elapsed().as_micros() as u64);
        }
    }
    lat.sort_unstable();
    println!(
        "  query latency     : p50={}µs p95={}µs p99={}µs max={}µs",
        lat[lat.len() / 2],
        lat[(lat.len() as f64 * 0.95) as usize],
        lat[(lat.len() as f64 * 0.99) as usize],
        lat[lat.len() - 1]
    );

    assert!(
        fmt_missing.is_empty(),
        "formats not searchable in combined index: {fmt_missing:?}"
    );
    assert!(
        lat[(lat.len() as f64 * 0.95) as usize] < 1_000_000,
        "p95 query latency > 1s is pathological"
    );
}

// ── PARTIAL-TOKEN MATCHING — end-to-end through crawl → extract → index ──────
//
// Regression for the "ID search" bug: a file named (or containing)
// XXXXXXXX2982 was findable by the full run but NOT by "2982", making
// serial/invoice/ID lookups useless. Pins the whole pipeline, not just
// ContentIndex: real files on disk, crawled and extracted like production.
#[test]
fn partial_token_id_lookup_end_to_end() {
    let data = TempDir::new().unwrap();
    let index = TempDir::new().unwrap();

    write_text(
        data.path(),
        "XXXXXXXX2982.txt",
        "scanned copy, no useful body",
    );
    write_text(
        data.path(),
        "statement.txt",
        "wire transfer reference INV2982 was confirmed today",
    );
    write_text(data.path(), "noise.txt", "completely unrelated content");

    let gs = build_globset(&[]).unwrap();
    let files = collect_files(&[data.path().to_path_buf()], &gs, &[], 50_000_000);
    let engine = SearchEngine::open(make_config(
        index.path().to_path_buf(),
        vec![data.path().to_path_buf()],
    ))
    .unwrap();
    engine.index_files_sync(files).unwrap();

    // Bare numeric ID finds both the named file and the body mention.
    let hits = engine.search(&keyword_query("2982", 10)).unwrap();
    let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
    assert!(
        names.contains(&"XXXXXXXX2982.txt"),
        "'2982' must find the file NAMED with the ID; got {names:?}"
    );
    assert!(
        names.contains(&"statement.txt"),
        "'2982' must find the ID inside body text; got {names:?}"
    );
    assert!(!names.contains(&"noise.txt"), "no false positive");

    // The full original query still works.
    let hits = engine.search(&keyword_query("XXXXXXXX2982", 10)).unwrap();
    assert_eq!(hits.len(), 1, "full-token query still matches exactly");
    assert_eq!(hits[0].name, "XXXXXXXX2982.txt");

    // Mid-token substring (no token boundary) still finds the named file.
    let hits = engine.search(&keyword_query("X2982", 10)).unwrap();
    assert!(
        hits.iter().any(|h| h.name == "XXXXXXXX2982.txt"),
        "mid-token substring must match via the name-substring rung"
    );
}
