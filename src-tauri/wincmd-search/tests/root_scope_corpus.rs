// SPDX-License-Identifier: AGPL-3.0-or-later
//! Real-world validation of the content-search folder-scoping feature
//! (`ContentQuery.roots`) against THIS machine's actual filesystem.
//!
//! `search_parity.rs` proves the engine works correctly against a
//! synthetic multi-format corpus built purely of tempdir fixtures. This file
//! is deliberately different: every path fed into `path_in_roots` /
//! `SearchEngine::search` here is either (a) read from a real filesystem
//! snapshot (`corpus.json`, captured once for this validation pass — see
//! `WCS_ROOT_SCOPE_CORPUS_JSON` below) or (b) a real directory this dev
//! machine actually has on disk (this repo and its sibling `wincommander-pro`
//! checkout), re-verified live via `std::fs` so a stale snapshot never
//! produces a false pass.
//!
//! Five concerns, matching the battery this file was written for:
//!   1. Root scoping correctness AT SCALE — hundreds/thousands of real
//!      directory strings, not the handful of synthetic `C:\Foo` cases
//!      `index.rs`'s own unit tests use — including the sibling-PREFIX bug
//!      class called out in `index.rs`'s comments, exercised against real
//!      pairs this filesystem actually contains (`wincommander` /
//!      `wincommander-pro`, `Program Files` / `Program Files (x86)`, …).
//!   2. The post-hoc-filter over-fetch (`scoped_fetch`) DOES silently
//!      under-report once a narrow scope's matches rank below the fetch
//!      window — quantified at the actual limits production uses (50 for
//!      the Search Files panel, 5 for the Ctrl+Space overlay; see
//!      `useContentIndex.ts` / `useChipSearch.ts`).
//!   3. `ext:`/`after:`/`before:`/`size:` filters against real file
//!      metadata, plus the comma-vs-semicolon split between this backend
//!      (`filters.rs`, comma) and the Everything backend (semicolon).
//!   4. Real-document extraction success, per format, using real files
//!      already on this disk (corpus.json turned out to carry zero real
//!      office/PDF samples despite the brief describing some — see the
//!      note on `real_document_extraction_success_rates_by_format` — so
//!      those formats are sourced directly from disk via a fixed candidate
//!      list, each entry re-verified to exist before use).
//!   5. Offset/paging while a scope is active, and case-insensitivity of
//!      both the query term and the scope path.
//!
//! Tests that only call the pure `path_in_roots`/`scoped_fetch` functions run
//! in the normal (non-ignored) suite — they're fast and soft-skip (print and
//! return, never fail) when `corpus.json` isn't present, so a checkout
//! without that ephemeral scratch file still passes CI.
//!
//! Tests that build a real tantivy index are `#[ignore]`d (mirrors
//! `search_parity.rs`'s `parity_perf`): run them with
//!   cargo test -p wincmd-search --release --test root_scope_corpus -- --ignored --nocapture

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;
use tempfile::TempDir;
use wincmd_search::{
    crawler::doc_id_for,
    extract::extract_text,
    index::{path_in_roots, scoped_fetch},
    types::{ContentQuery, FileMeta, IndexConfig},
    SearchEngine,
};

// ── Corpus loading ───────────────────────────────────────────────────────────

/// One entry of the pre-built real-filesystem snapshot. Only the fields this
/// file actually uses are declared — serde ignores the rest (`size`, `mtime`,
/// `stratum`, …) — deliberately: metadata (size, mtime, existence) is always
/// re-read live via `std::fs` below rather than trusted from the snapshot,
/// so a test never asserts against a file that has since moved or vanished.
#[derive(Debug, Clone, Deserialize)]
struct CorpusEntry {
    full: String,
    dir: String,
    #[serde(rename = "isDir")]
    is_dir: bool,
    ext: String,
}

/// Overridable so a re-run of this validation on a fresher snapshot (or a
/// different machine) doesn't require editing the test file.
fn corpus_json_path() -> PathBuf {
    std::env::var("WCS_ROOT_SCOPE_CORPUS_JSON")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(
                r"C:\Users\Admin\AppData\Local\Temp\claude\D--GitHub\2496e247-cb26-4132-bfd0-088c93fd61a3\scratchpad\corpus.json",
            )
        })
}

/// Loads the snapshot; `None` (never a panic) when it's absent, so every
/// caller can soft-skip instead of failing CI on a machine/checkout that
/// never had this ephemeral scratch file.
fn load_corpus() -> Option<Vec<CorpusEntry>> {
    let path = corpus_json_path();
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(entries) => Some(entries),
            Err(e) => {
                eprintln!("[skip] corpus.json at {path:?} failed to parse: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("[skip] corpus.json not readable at {path:?}: {e}");
            None
        }
    }
}

/// Text-bearing extensions `extract_text` supports via the plain-text path —
/// the set actually usable for a content-search fixture. corpus.json (see
/// module doc) sampled none of the office/PDF/ODF/RTF formats, so those are
/// covered separately in `real_document_extraction_success_rates_by_format`.
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "json", "xml", "yaml", "yml", "toml", "log", "rs", "py", "js", "ts", "tsx",
    "jsx", "html", "css", "sh", "ini", "csv", "c", "cpp", "h", "java", "go", "sql", "ps1", "conf",
    "rst",
];

// ── Real-file helpers ────────────────────────────────────────────────────────

/// Builds a `FileMeta` from a REAL file's current on-disk metadata (never
/// from the snapshot's possibly-stale `size`/`mtime`) — mirrors
/// `crawler::collect_files`'s own field population exactly.
fn meta_for_real(path: &Path) -> Option<FileMeta> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(FileMeta {
        doc_id: doc_id_for(path),
        path: path.to_path_buf(),
        name,
        ext,
        mtime,
        size: metadata.len(),
    })
}

/// Extracts + indexes one real file end-to-end (crawl→extract→index, matching
/// `SearchEngine::index_files_sync`'s own per-file handling). Returns `Ok(())`
/// even when extraction is a deliberate per-file skip (unsupported/malformed)
/// — callers that care distinguish via `extract_text` directly.
fn index_real_files(engine: &SearchEngine, paths: &[PathBuf]) -> usize {
    let metas: Vec<FileMeta> = paths.iter().filter_map(|p| meta_for_real(p)).collect();
    let n = metas.len();
    engine.index_files_sync(metas).expect("index_files_sync");
    n
}

fn open_engine(index_dir: &Path) -> SearchEngine {
    SearchEngine::open(IndexConfig {
        roots: vec![],
        exclusions: vec![],
        skip_paths: vec![],
        max_file_bytes: 50_000_000,
        index_dir: index_dir.to_path_buf(),
    })
    .expect("SearchEngine::open")
}

fn query(terms: &str, roots: &[PathBuf], limit: usize, offset: usize) -> ContentQuery {
    ContentQuery {
        terms: terms.to_owned(),
        roots: roots.to_vec(),
        limit,
        offset,
        keyword_only: true,
    }
}

/// Independent cross-check for `path_in_roots`, deliberately NOT sharing a
/// code path with it: lowercase (Windows paths are case-insensitive) then use
/// `Path::starts_with`, which is component-aware on its own — `"C:\Foobar"`
/// does not start-with `"C:\Foo"` as a `Path`, because the last COMPONENT
/// (`"foobar"` vs `"foo"`) differs, with no manual separator arithmetic
/// needed on this side. A finding that only `path_in_roots` agrees with
/// itself is not a finding; this gives every scale assertion below a second,
/// differently-implemented opinion.
fn independent_scope_check(path: &str, root: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().replace('/', "\\");
    let p = norm(path);
    let r = norm(root);
    if r.trim_matches('\\').is_empty() {
        return false; // degenerate root matches nothing, mirrors path_in_roots
    }
    Path::new(&p).starts_with(Path::new(&r))
}

/// Longest ASCII-alphabetic run of at least `min_len` chars — an
/// auto-derived, content-dependent search probe so extraction tests don't
/// require pre-knowing each real file's exact words.
fn probe_word(body: &str, min_len: usize) -> Option<String> {
    body.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| w.len() >= min_len)
        .max_by_key(|w| w.len())
        .map(|w| w.to_lowercase())
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Root scoping correctness AT SCALE (pure `path_in_roots`, no indexing)
// ═══════════════════════════════════════════════════════════════════════════

/// Positive case at scale: every real (file, its real parent directory) pair
/// from the snapshot must be in-scope when scoped to that directory — checked
/// against hundreds of genuinely-existing files with the full variety of
/// real Windows names (spaces, parens, unicode, trailing dots, …) rather than
/// a handful of hand-typed `C:\Foo\bar.txt` fixtures.
#[test]
fn path_in_roots_matches_hundreds_of_real_nested_paths() {
    let Some(corpus) = load_corpus() else {
        return;
    };
    let mut checked = 0usize;
    let mut mismatches = Vec::new();
    for e in corpus.iter().filter(|e| !e.is_dir) {
        if e.dir.is_empty() || !Path::new(&e.full).exists() {
            continue; // re-verify live; a stale snapshot entry proves nothing
        }
        checked += 1;
        let root = vec![PathBuf::from(&e.dir)];
        let prod = path_in_roots(&e.full, &root);
        let indep = independent_scope_check(&e.full, &e.dir);
        if !prod || !indep {
            mismatches.push(format!(
                "{:?} should be in scope of its own real parent {:?} (path_in_roots={prod}, independent={indep})",
                e.full, e.dir
            ));
        }
    }
    println!("real (file, real parent dir) pairs checked: {checked}");
    assert!(checked > 100, "expected a substantial real sample, got {checked}");
    assert!(
        mismatches.is_empty(),
        "{} of {checked} real files were wrongly scoped OUT of their own parent:\n  {}",
        mismatches.len(),
        mismatches.join("\n  ")
    );
}

/// Negative case at scale — the sibling-PREFIX bug class, exercised against
/// every genuinely-existing prefix relationship this real filesystem
/// happens to contain (this repo vs `wincommander-pro`, `Program Files` vs
/// `Program Files (x86)`, etc.), not just the 2-3 synthetic examples
/// `index.rs`'s unit tests hand-craft.
#[test]
fn path_in_roots_rejects_real_sibling_prefix_directories_at_scale() {
    let Some(corpus) = load_corpus() else {
        return;
    };
    // Every real directory, plus every real ANCESTOR of every real directory
    // (so a deep sampled path also contributes its shallower real ancestors
    // to the pool — this is how `Program Files` / `wincommander` end up in
    // the set even though no *file* was sampled directly in them).
    let mut dirs: HashSet<String> = HashSet::new();
    for e in &corpus {
        if e.dir.is_empty() {
            continue;
        }
        let mut acc = String::new();
        for part in e.dir.split('\\') {
            if !acc.is_empty() {
                acc.push('\\');
            }
            acc.push_str(part);
            if acc.len() > 3 {
                dirs.insert(acc.clone());
            }
        }
    }
    // Only directories that still exist right now — a renamed/deleted one
    // proves nothing about the live scoping code.
    let dirs: Vec<String> = dirs.into_iter().filter(|d| Path::new(d).is_dir()).collect();
    let lower: Vec<String> = dirs.iter().map(|d| d.to_lowercase()).collect();
    println!("real, currently-existing directories under test: {}", dirs.len());

    let t0 = Instant::now();
    let mut gotcha_pairs = 0usize;
    let mut examples = Vec::new();
    for i in 0..lower.len() {
        for j in 0..lower.len() {
            if i == j {
                continue;
            }
            let (a, b) = (&lower[i], &lower[j]);
            if a.len() >= b.len() || !b.starts_with(a.as_str()) {
                continue;
            }
            // Genuine sibling-prefix gotcha: `b` extends `a`'s STRING but the
            // very next byte isn't a separator, so `b` is not really a
            // descendant of `a` (e.g. a="...\wincommander", b="...\wincommander-pro\...").
            if b.as_bytes()[a.len()] == b'\\' {
                continue; // real descendant — must NOT be rejected, not this check's concern
            }
            gotcha_pairs += 1;
            let root = vec![PathBuf::from(&dirs[i])];
            assert!(
                !path_in_roots(&dirs[j], &root),
                "sibling-prefix leak: scoping to real dir {:?} must not admit real sibling {:?}",
                dirs[i],
                dirs[j]
            );
            assert!(
                !independent_scope_check(&dirs[j], &dirs[i]),
                "independent oracle also disagrees: {:?} wrongly in scope of {:?}",
                dirs[j],
                dirs[i]
            );
            if examples.len() < 5 {
                examples.push((dirs[i].clone(), dirs[j].clone()));
            }
        }
    }
    println!(
        "gotcha sibling-prefix pairs found + verified rejected: {gotcha_pairs} (scan took {:?})",
        t0.elapsed()
    );
    for (a, b) in &examples {
        println!("  example: root {a:?} correctly does NOT admit sibling {b:?}");
    }
    assert!(
        gotcha_pairs > 0,
        "expected at least one real sibling-prefix pair (e.g. this repo vs wincommander-pro) \
         on this machine — if this fires elsewhere, the negative case simply wasn't exercised"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Multi-root indexing over several REAL directory trees
// ═══════════════════════════════════════════════════════════════════════════

/// Groups existing, size-capped, text-bearing corpus entries by real parent
/// directory and returns the `n` directories with the most files (each
/// capped to `per_dir_cap` files) — several genuinely different real trees
/// (a Cargo registry checkout, this machine's own tool caches, another
/// sibling repo, …), not folders picked by hand.
fn pick_real_multi_file_dirs(
    corpus: &[CorpusEntry],
    n: usize,
    per_dir_cap: usize,
) -> Vec<(String, Vec<PathBuf>)> {
    let mut by_dir: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for e in corpus {
        if e.is_dir || !TEXT_EXTS.contains(&e.ext.to_lowercase().as_str()) {
            continue;
        }
        let p = Path::new(&e.full);
        let Ok(meta) = fs::metadata(p) else { continue };
        if !meta.is_file() || meta.len() < 20 || meta.len() > 2_000_000 {
            continue;
        }
        by_dir.entry(e.dir.clone()).or_default().push(p.to_path_buf());
    }
    let mut dirs: Vec<(String, Vec<PathBuf>)> = by_dir
        .into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .collect();
    dirs.sort_by_key(|(_, files)| std::cmp::Reverse(files.len()));
    dirs.truncate(n);
    for (_, files) in &mut dirs {
        files.truncate(per_dir_cap);
    }
    dirs
}

/// The core "at scale, several real trees" assertion: index real files
/// spanning multiple, genuinely unrelated real directories, then confirm
/// every root's scoped result set is EXACTLY its own real files — no leaks
/// in, no drops out — and that the per-root sets partition the whole index
/// (every indexed doc is in exactly one root's result set).
///
/// `#[ignore]`: builds a real tantivy index over ~100-300 real files, which
/// is unnecessary weight for a normal `cargo test` pass. Run with:
///   cargo test -p wincmd-search --release --test root_scope_corpus \
///     multi_root_real_directory_trees_partition_correctly -- --ignored --nocapture
#[test]
#[ignore]
fn multi_root_real_directory_trees_partition_correctly() {
    let Some(corpus) = load_corpus() else {
        println!("[skip] no corpus.json — nothing to index");
        return;
    };
    let chosen = pick_real_multi_file_dirs(&corpus, 6, 25);
    assert!(
        chosen.len() >= 3,
        "expected at least 3 real multi-file directories on this machine, got {}",
        chosen.len()
    );

    let index_dir = TempDir::new().unwrap();
    let engine = open_engine(index_dir.path());

    let mut all_paths: Vec<PathBuf> = Vec::new();
    for (dir, files) in &chosen {
        println!("root {:?}: {} real files", dir, files.len());
        all_paths.extend(files.iter().cloned());
    }
    let t0 = Instant::now();
    let indexed = index_real_files(&engine, &all_paths);
    println!("indexed {indexed}/{} real files in {:?}", all_paths.len(), t0.elapsed());

    // All extensions used, comma-joined — a filters-only query (`ext:` with
    // no free text) matches every doc we just indexed regardless of its
    // (very varied) real body content, isolating the assertion to scoping.
    let exts: HashSet<&str> = chosen
        .iter()
        .flat_map(|(_, files)| files.iter())
        .filter_map(|p| p.extension())
        .map(|e| e.to_str().unwrap_or(""))
        .collect();
    let ext_filter = format!("ext:{}", exts.into_iter().collect::<Vec<_>>().join(","));

    let unscoped = engine.search(&query(&ext_filter, &[], 500, 0)).unwrap();
    assert_eq!(
        unscoped.len(),
        indexed,
        "unscoped filters-only query must return every real indexed doc"
    );

    let mut seen_paths: HashSet<String> = HashSet::new();
    for (dir, files) in &chosen {
        let hits = engine
            .search(&query(&ext_filter, &[PathBuf::from(dir)], 500, 0))
            .unwrap();
        assert_eq!(
            hits.len(),
            files.len(),
            "root {dir:?}: expected exactly its {} real files, got {} ({:?})",
            files.len(),
            hits.len(),
            hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>()
        );
        for h in &hits {
            assert!(
                path_in_roots(&h.path, &[PathBuf::from(dir)]),
                "hit {:?} returned for root {dir:?} but path_in_roots disagrees",
                h.path
            );
            assert!(
                independent_scope_check(&h.path, dir),
                "hit {:?} returned for root {dir:?} but the independent oracle disagrees",
                h.path
            );
            // No cross-root leakage: a path claimed by this root must not
            // also have been claimed by a previously-checked root.
            assert!(
                seen_paths.insert(h.path.to_lowercase()),
                "path {:?} was returned for MORE THAN ONE root scope — cross-root leak",
                h.path
            );
        }
    }
    assert_eq!(
        seen_paths.len(),
        indexed,
        "the per-root scoped sets must partition the whole real corpus with no gaps"
    );
    println!(
        "{} real files across {} real directory trees partition correctly under root scoping",
        indexed,
        chosen.len()
    );
}

/// Offset/paging while a scope is active (item 5), and case-insensitivity of
/// the scope path itself — both against the largest real directory tree
/// chosen above, so the pagination assertion is over genuine files with
/// genuine (non-uniform) BM25 scores, not a synthetic same-score fixture.
///
/// `#[ignore]`: same real-indexing cost as the test above.
#[test]
#[ignore]
fn scoped_paging_and_case_insensitive_scope_path_on_real_files() {
    let Some(corpus) = load_corpus() else {
        println!("[skip] no corpus.json — nothing to index");
        return;
    };
    let chosen = pick_real_multi_file_dirs(&corpus, 1, 30);
    let Some((dir, files)) = chosen.into_iter().next() else {
        println!("[skip] no real multi-file directory found");
        return;
    };
    assert!(files.len() >= 6, "need enough real files to page through, got {}", files.len());

    let index_dir = TempDir::new().unwrap();
    let engine = open_engine(index_dir.path());
    let indexed = index_real_files(&engine, &files);
    println!("paging test: {indexed} real files from {dir:?}");

    let exts: HashSet<&str> = files
        .iter()
        .filter_map(|p| p.extension())
        .map(|e| e.to_str().unwrap_or(""))
        .collect();
    let ext_filter = format!("ext:{}", exts.into_iter().collect::<Vec<_>>().join(","));

    // Page through in scoped pages of 2 and confirm the pages partition the
    // real in-scope set with no overlap and no gap — mirrors
    // `scoped_offset_pages_through_in_scope_hits_only` in index.rs, but over
    // real files with real (non-identical) BM25 scores instead of a
    // hand-crafted same-score fixture.
    let page_size = 2usize;
    let mut collected: Vec<String> = Vec::new();
    let mut offset = 0usize;
    loop {
        let page = engine
            .search(&query(&ext_filter, &[PathBuf::from(&dir)], page_size, offset))
            .unwrap();
        if page.is_empty() {
            break;
        }
        for h in &page {
            assert!(
                collected.iter().all(|p| !p.eq_ignore_ascii_case(&h.path)),
                "path {:?} appeared on more than one scoped page (offset={offset})",
                h.path
            );
            collected.push(h.path.clone());
        }
        offset += page_size;
        assert!(offset <= indexed + page_size, "paging did not terminate");
    }
    assert_eq!(
        collected.len(),
        indexed,
        "paged-through scoped results must cover every real in-scope file exactly once"
    );

    // Case-insensitivity of the SCOPE PATH itself: upper, lower, and
    // as-sampled casing must all return the identical real result set.
    let baseline = engine
        .search(&query(&ext_filter, &[PathBuf::from(&dir)], 500, 0))
        .unwrap();
    for variant in [dir.to_uppercase(), dir.to_lowercase()] {
        let hits = engine
            .search(&query(&ext_filter, &[PathBuf::from(&variant)], 500, 0))
            .unwrap();
        assert_eq!(
            hits.len(),
            baseline.len(),
            "scope path case variant {variant:?} must match the same real files as {dir:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. The real sibling-prefix repos, end-to-end (not just path_in_roots)
// ═══════════════════════════════════════════════════════════════════════════

/// The exact real-world case `index.rs`'s comments call out — indexed and
/// searched through the FULL real pipeline (crawl → extract → tantivy →
/// search), not just the pure path-matching function: this repo
/// (`D:\GitHub\wincommander`) and its real sibling checkout
/// `D:\GitHub\wincommander-pro` are a genuine sibling-PREFIX pair on this
/// machine. Only each repo's OWN top-level docs are indexed (not
/// recursively crawled — this repo's target/node_modules trees are large and
/// irrelevant to the scoping question), so this stays fast despite being a
/// real end-to-end run.
///
/// Soft-skips (prints + returns) if either real repo isn't present — this is
/// real-machine validation, not a portable CI fixture.
#[test]
#[ignore]
fn sibling_prefix_wincommander_vs_pro_real_repos_no_leak() {
    let repo_root = env!("CARGO_MANIFEST_DIR"); // .../wincommander/src-tauri/wincmd-search
    let wincommander = PathBuf::from(repo_root)
        .join("..")
        .join("..")
        .canonicalize()
        .expect("wincommander repo root must exist (this test lives inside it)");
    let wincommander_pro = wincommander.parent().unwrap().join("wincommander-pro");
    if !wincommander_pro.is_dir() {
        println!(
            "[skip] sibling repo {wincommander_pro:?} not present on this machine — \
             the real sibling-prefix pair this test targets doesn't exist here"
        );
        return;
    }

    fn top_level_files(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect()
    }

    let a_files = top_level_files(&wincommander);
    let b_files = top_level_files(&wincommander_pro);
    assert!(!a_files.is_empty() && !b_files.is_empty(), "both real repos must have top-level files");
    println!(
        "indexing {} real top-level files from {:?} and {} from {:?}",
        a_files.len(),
        wincommander,
        b_files.len(),
        wincommander_pro
    );

    let index_dir = TempDir::new().unwrap();
    let engine = open_engine(index_dir.path());
    let mut all = a_files.clone();
    all.extend(b_files.iter().cloned());
    index_real_files(&engine, &all);

    // Every real README/AGENTS/etc. at this level plausibly mentions the
    // product name — a genuinely shared, realistic term across both real
    // trees, exactly the scenario the sibling-prefix bug would corrupt.
    let term = "wincommander";
    let unscoped = engine.search(&query(term, &[], 200, 0)).unwrap();
    let hits_a = unscoped
        .iter()
        .any(|h| path_in_roots(&h.path, std::slice::from_ref(&wincommander)));
    let hits_b = unscoped
        .iter()
        .any(|h| path_in_roots(&h.path, std::slice::from_ref(&wincommander_pro)));
    println!("unscoped {term:?} hits: {} (from wincommander: {hits_a}, from -pro: {hits_b})", unscoped.len());

    let scoped_a = engine
        .search(&query(term, std::slice::from_ref(&wincommander), 200, 0))
        .unwrap();
    assert!(!scoped_a.is_empty(), "scoping to the real wincommander repo must still find real hits");
    for h in &scoped_a {
        assert!(
            !h.path.to_lowercase().contains("wincommander-pro"),
            "LEAK: scoping to {wincommander:?} returned a real file from the sibling repo: {:?}",
            h.path
        );
    }

    let scoped_b = engine
        .search(&query(term, std::slice::from_ref(&wincommander_pro), 200, 0))
        .unwrap();
    assert!(!scoped_b.is_empty(), "scoping to the real wincommander-pro repo must still find real hits");
    for h in &scoped_b {
        assert!(
            path_in_roots(&h.path, std::slice::from_ref(&wincommander_pro)),
            "hit {:?} returned when scoped to wincommander-pro is not really under it",
            h.path
        );
        // The sharper regression check: wincommander-pro's OWN files must
        // never be mistaken for the shorter sibling's — i.e. scoping to the
        // longer path must not accidentally admit the shorter repo's files
        // (the reverse direction of the leak).
        assert!(
            !h.path
                .to_lowercase()
                .starts_with(&wincommander.to_string_lossy().to_lowercase())
                || h.path.to_lowercase().contains("wincommander-pro"),
            "hit {:?} scoped to wincommander-pro looks like it actually came from wincommander",
            h.path
        );
    }
    println!(
        "real sibling-prefix repos: {} in-scope hits for wincommander, {} for wincommander-pro, zero cross-leak in either direction",
        scoped_a.len(),
        scoped_b.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Scoping must not silently truncate — the over-fetch boundary, quantified
// ═══════════════════════════════════════════════════════════════════════════

/// Writes `n` short "out-of-scope" files (all matching `term` once, in a
/// short body so BM25 scores them HIGH) under `dir/out`, and `m` long
/// "in-scope" files (same term, diluted by filler so BM25 scores them LOW)
/// under `dir/in`. Every out-of-scope doc is constructed to outrank every
/// in-scope doc for `term`, so the in-scope docs are GUARANTEED to occupy
/// the bottom `m` ranks of an unscoped search — the worst case for a
/// post-hoc scope filter.
fn write_overfetch_corpus(base: &Path, term: &str, n: usize, m: usize) {
    let out_dir = base.join("out");
    let in_dir = base.join("in");
    fs::create_dir_all(&out_dir).unwrap();
    fs::create_dir_all(&in_dir).unwrap();
    for i in 0..n {
        fs::write(out_dir.join(format!("o{i}.txt")), format!("{term} short doc {i}")).unwrap();
    }
    let filler = "filler word repeated many times to dilute term frequency ".repeat(300);
    for i in 0..m {
        fs::write(in_dir.join(format!("i{i}.txt")), format!("{filler} {term} tail {i}")).unwrap();
    }
}

/// Quantifies the over-fetch boundary at limit=50 (Search Files panel
/// default, `useContentIndex.ts`) and limit=5 (Ctrl+Space overlay default,
/// `useChipSearch.ts`) — the ACTUAL limits production asks for, not an
/// arbitrary number. `scoped_fetch` is called directly (not a hardcoded
/// 20x/5000 guess) so this stays correct if those constants are ever tuned.
///
/// `#[ignore]`: indexes up to ~2500 real-on-disk (if synthetic) files per
/// sub-case. Run with:
///   cargo test -p wincmd-search --release --test root_scope_corpus \
///     scoped_search_underreports_once_fetch_window_is_exceeded -- --ignored --nocapture
#[test]
#[ignore]
fn scoped_search_underreports_once_fetch_window_is_exceeded() {
    // in-scope docs always rank last for `term` by construction (see
    // `write_overfetch_corpus`), so once `n` out-of-scope docs reach or
    // exceed the fetch window, EVERY in-scope doc is pushed out of the
    // ranking tantivy is even asked to consider — not merely "shown lower".
    for (limit, offset, label) in [
        (50usize, 0usize, "Search Files panel default (limit=50)"),
        (5usize, 0usize, "Ctrl+Space overlay default (limit=5)"),
    ] {
        let scope_root = vec![PathBuf::from("PLACEHOLDER")]; // replaced per sub-case below
        let fetch = scoped_fetch(limit + offset, &scope_root);
        println!("\n── {label}: scoped_fetch({}, roots) = {fetch} ──", limit + offset);

        for (n, expect_survives) in [
            (fetch.saturating_sub(20), true),
            (fetch + 200, false),
        ] {
            let data = TempDir::new().unwrap();
            let index_dir = TempDir::new().unwrap();
            let term = "overfetchboundarymarker";
            let m = 3usize;
            write_overfetch_corpus(data.path(), term, n, m);

            let engine = open_engine(index_dir.path());
            let out_dir = data.path().join("out");
            let in_dir = data.path().join("in");
            let mut all: Vec<PathBuf> = fs::read_dir(&out_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect();
            all.extend(
                fs::read_dir(&in_dir)
                    .unwrap()
                    .filter_map(|e| e.ok())
                    .map(|e| e.path()),
            );
            let t0 = Instant::now();
            let indexed = index_real_files(&engine, &all);
            assert_eq!(indexed, n + m);

            let hits = engine
                .search(&query(term, std::slice::from_ref(&in_dir), limit, offset))
                .unwrap();
            let survived = !hits.is_empty();
            println!(
                "  n={n:<6} (out-of-scope, all outranking) m={m} in-scope → \
                 {} in-scope hits returned, indexed+searched in {:?} [{}]",
                hits.len(),
                t0.elapsed(),
                if survived == expect_survives { "as predicted" } else { "UNEXPECTED" }
            );
            assert_eq!(
                survived, expect_survives,
                "at n={n} (fetch window={fetch}), expected in-scope survival={expect_survives} but got {survived}"
            );
        }
    }
}

/// The ABSOLUTE ceiling (`ROOT_SCOPE_MAX_FETCH`, private to `index.rs` — not
/// hardcoded here; derived empirically from `scoped_fetch`'s own return
/// value) applies regardless of how large `limit` is asked to be. A caller
/// requesting a huge `limit` does NOT get a proportionally huge fetch window
/// once the cap is hit — demonstrated with real numbers rather than assumed.
///
/// `#[ignore]`: indexes several thousand files — the heaviest test in this
/// file. Measured wall-clock is printed; sample size is what actually ran.
#[test]
#[ignore]
fn scoped_search_hits_absolute_fetch_ceiling_regardless_of_limit() {
    let huge_limit = 10_000usize;
    let scope_root = vec![PathBuf::from("PLACEHOLDER")];
    let ceiling = scoped_fetch(huge_limit, &scope_root);
    // scoped_fetch(10_000, scoped) == 10_000.max(...).min(cap) — since base
    // itself exceeds the cap, `.max(base)` wins and fetch == base: the cap
    // only bites once `limit` is BELOW it. Use a limit comfortably above the
    // cap-crossing point instead so this test targets the real ceiling.
    let probe_limit = 300usize; // 300*20=6000 > any plausible cap, but < 10_000
    let fetch_at_probe = scoped_fetch(probe_limit, &scope_root);
    println!(
        "scoped_fetch({huge_limit}, scoped) = {ceiling} (base itself dominates); \
         scoped_fetch({probe_limit}, scoped) = {fetch_at_probe} (the real ceiling in effect)"
    );
    assert!(
        fetch_at_probe < probe_limit.saturating_mul(20),
        "expected the absolute cap to actually reduce below the naive 20x multiplier at this limit"
    );

    let n = fetch_at_probe + 200; // comfortably past the ceiling
    let m = 3usize;
    let data = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();
    let term = "absoluteceilingmarker";
    write_overfetch_corpus(data.path(), term, n, m);

    let engine = open_engine(index_dir.path());
    let out_dir = data.path().join("out");
    let in_dir = data.path().join("in");
    let mut all: Vec<PathBuf> = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    all.extend(fs::read_dir(&in_dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()));
    let t0 = Instant::now();
    let indexed = index_real_files(&engine, &all);
    let index_ms = t0.elapsed();

    let t1 = Instant::now();
    let hits = engine.search(&query(term, &[in_dir], probe_limit, 0)).unwrap();
    println!(
        "n={n} out-of-scope + m={m} in-scope, indexed={indexed} in {index_ms:?}, \
         searched (limit={probe_limit}) in {:?} → {} in-scope hits (expected 0: n > ceiling {fetch_at_probe})",
        t1.elapsed(),
        hits.len()
    );
    assert!(
        hits.is_empty(),
        "even a caller asking for limit={probe_limit} must be under-reported once n={n} exceeds \
         the absolute fetch ceiling ({fetch_at_probe}) — got {} unexpected hits",
        hits.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Filter tokens against real file metadata; comma vs semicolon
// ═══════════════════════════════════════════════════════════════════════════

/// `ext:`/`after:`/`before:`/`size:` against REAL files' real extensions,
/// real mtimes, and real sizes — plus the comma/semicolon distinction the
/// frontend depends on (`contentQueryFilters.ts` / `searchQueryPlan.ts` emit
/// commas for this backend; Everything's `es.exe` wants semicolons — see
/// `fileNameSearch.ts`). A semicolon reaching THIS backend must not silently
/// multi-match; `filters.rs::parse_ext` only splits on comma, so a
/// semicolon-joined value becomes one bogus extension string that matches
/// nothing real — exactly what would happen if the wrong separator ever
/// reached `search_content`.
#[test]
fn filter_tokens_on_real_files_ext_comma_vs_semicolon_date_and_size() {
    let Some(corpus) = load_corpus() else {
        return;
    };
    // 2026-01-01T00:00:00Z, independently cross-checked via `Date.UTC(2026,0,1)/1000`
    // in Node (== 1_767_225_600) — not copied from filters.rs's own test, so
    // this partitions real files against a boundary derived a different way.
    const BOUNDARY_2026_01_01: u64 = 1_767_225_600;

    let mut md_files: Vec<PathBuf> = Vec::new();
    let mut py_files: Vec<PathBuf> = Vec::new();
    let mut before_boundary: Vec<PathBuf> = Vec::new();
    let mut after_boundary: Vec<PathBuf> = Vec::new();
    for e in corpus.iter().filter(|e| !e.is_dir) {
        let p = Path::new(&e.full);
        let Ok(meta) = fs::metadata(p) else { continue };
        if !meta.is_file() || meta.len() < 10 || meta.len() > 2_000_000 {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match e.ext.to_lowercase().as_str() {
            "md" if md_files.len() < 10 => md_files.push(p.to_path_buf()),
            "py" if py_files.len() < 10 => py_files.push(p.to_path_buf()),
            _ => {}
        }
        if mtime != 0 && mtime < BOUNDARY_2026_01_01 && before_boundary.len() < 8 {
            before_boundary.push(p.to_path_buf());
        } else if mtime >= BOUNDARY_2026_01_01 && after_boundary.len() < 8 {
            after_boundary.push(p.to_path_buf());
        }
    }
    if md_files.len() < 3 || py_files.len() < 3 {
        println!(
            "[skip] not enough real .md ({}) / .py ({}) files on this machine for the ext: check",
            md_files.len(),
            py_files.len()
        );
        return;
    }
    if before_boundary.len() < 2 || after_boundary.len() < 2 {
        println!(
            "[skip] not enough real files on both sides of the 2026-01-01 boundary \
             (before={}, after={}) for the after:/before: check",
            before_boundary.len(),
            after_boundary.len()
        );
        return;
    }

    let index_dir = TempDir::new().unwrap();
    let engine = open_engine(index_dir.path());
    let mut all = md_files.clone();
    all.extend(py_files.iter().cloned());
    let indexed = index_real_files(&engine, &all);
    println!(
        "ext:/date filter test: {indexed} real files ({} .md, {} .py)",
        md_files.len(),
        py_files.len()
    );

    // ext:, COMMA — must return exactly the real .md + .py files.
    let hits = engine.search(&query("ext:md,py", &[], 500, 0)).unwrap();
    assert_eq!(
        hits.len(),
        indexed,
        "comma-separated ext: must match every real .md and .py file indexed"
    );

    // ext:, SEMICOLON — filters.rs splits only on comma, so this becomes the
    // single bogus extension "md;py", matching no real file. This is the
    // concrete, real-file-backed proof that the frontend's comma choice for
    // this backend (vs. Everything's semicolon) is load-bearing, not cosmetic.
    let hits = engine.search(&query("ext:md;py", &[], 500, 0)).unwrap();
    assert!(
        hits.is_empty(),
        "semicolon-separated ext: must NOT multi-match real files here (got {} hits) — \
         confirms a stray semicolon reaching this backend silently returns nothing, not \
         'both extensions'",
        hits.len()
    );

    // after:/before:, real mtimes, real boundary.
    let index_dir2 = TempDir::new().unwrap();
    let engine2 = open_engine(index_dir2.path());
    let mut boundary_files = before_boundary.clone();
    boundary_files.extend(after_boundary.iter().cloned());
    index_real_files(&engine2, &boundary_files);

    let after_hits = engine2.search(&query("after:2026", &[], 500, 0)).unwrap();
    let after_paths: HashSet<String> = after_hits.iter().map(|h| h.path.to_lowercase()).collect();
    for p in &after_boundary {
        assert!(
            after_paths.contains(&p.to_string_lossy().to_lowercase()),
            "after:2026 must include real file {p:?} (mtime is on/after 2026-01-01)"
        );
    }
    for p in &before_boundary {
        assert!(
            !after_paths.contains(&p.to_string_lossy().to_lowercase()),
            "after:2026 must EXCLUDE real file {p:?} (mtime is before 2026-01-01)"
        );
    }

    let before_hits = engine2.search(&query("before:2026", &[], 500, 0)).unwrap();
    let before_paths: HashSet<String> = before_hits.iter().map(|h| h.path.to_lowercase()).collect();
    for p in &before_boundary {
        assert!(
            before_paths.contains(&p.to_string_lossy().to_lowercase()),
            "before:2026 must include real file {p:?} (mtime is before 2026-01-01)"
        );
    }
    for p in &after_boundary {
        assert!(
            !before_paths.contains(&p.to_string_lossy().to_lowercase()),
            "before:2026 must EXCLUDE real file {p:?} (mtime is on/after 2026-01-01)"
        );
    }
    println!(
        "after:2026 / before:2026 correctly partition {} real before-boundary and {} \
         real after-boundary files by their ACTUAL mtimes",
        before_boundary.len(),
        after_boundary.len()
    );

    // size:, real sizes: split the same real files at their real median size.
    let mut sizes: Vec<u64> = boundary_files.iter().map(|p| fs::metadata(p).unwrap().len()).collect();
    sizes.sort_unstable();
    let median = sizes[sizes.len() / 2];
    let small_q = format!("size:<{median}");
    let hits = engine2.search(&query(&small_q, &[], 500, 0)).unwrap();
    for h in &hits {
        let sz = fs::metadata(&h.path).map(|m| m.len()).unwrap_or(u64::MAX);
        assert!(sz < median, "size:<{median} returned {:?} with real size {sz}", h.path);
    }
    println!("size:<{median} correctly returned only real files below the real median size ({} hits)", hits.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Real-document extraction success, per format
// ═══════════════════════════════════════════════════════════════════════════

/// Bulk real .md/.txt files (from the snapshot) plus a curated, existence-
/// checked list of real office/PDF/RTF files THIS machine actually has on
/// disk right now (corpus.json's own sample turned out to carry zero real
/// .pdf/.docx/.xlsx/.pptx/.odt/.ods/.odp/.rtf entries — verified by grep
/// over the snapshot before writing this test — so those formats are
/// sourced directly from disk instead, each with a graceful skip if the
/// specific candidate path is gone). Reports per-format success rate; a
/// format at 0% here means "Inside files" quietly doesn't work for it.
///
/// `#[ignore]`: builds a real index and calls the real (slow, lopdf/zip-based)
/// extractors on real, sometimes multi-hundred-KB files.
#[test]
#[ignore]
fn real_document_extraction_success_rates_by_format() {
    #[derive(Default)]
    struct FormatTally {
        found_on_disk: usize,
        extracted_ok: usize,
        searchable: usize,
    }
    let mut tally: HashMap<&str, FormatTally> = HashMap::new();

    let index_dir = TempDir::new().unwrap();
    let engine = open_engine(index_dir.path());

    // ── Bulk real .md/.txt from the snapshot ──
    if let Some(corpus) = load_corpus() {
        let mut candidates: Vec<PathBuf> = corpus
            .iter()
            .filter(|e| !e.is_dir && matches!(e.ext.to_lowercase().as_str(), "md" | "txt"))
            .map(|e| PathBuf::from(&e.full))
            .filter(|p| p.exists())
            .take(80)
            .collect();
        candidates.truncate(40);

        for p in &candidates {
            let ext = if p.extension().and_then(|e| e.to_str()) == Some("md") { "md" } else { "txt" };
            let t = tally.entry(ext).or_default();
            t.found_on_disk += 1;
            let Some(meta) = meta_for_real(p) else { continue };
            let Ok(raw) = fs::read_to_string(p) else { continue };
            let Some(word) = probe_word(&raw, 6) else { continue };
            match extract_text(meta.clone()) {
                Ok(doc) if doc.body.to_lowercase().contains(&word) => {
                    t.extracted_ok += 1;
                    let hits = engine.search(&query(&word, &[], 50, 0)).unwrap();
                    if hits.iter().any(|h| h.path.eq_ignore_ascii_case(&p.to_string_lossy())) {
                        t.searchable += 1;
                    } else {
                        // Not yet indexed (batched below) — index individually so
                        // per-file probe words don't collide across a shared batch.
                        engine.index_files_sync(vec![meta]).unwrap();
                        let hits = engine.search(&query(&word, &[], 50, 0)).unwrap();
                        if hits.iter().any(|h| h.path.eq_ignore_ascii_case(&p.to_string_lossy())) {
                            t.searchable += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    } else {
        println!("[note] no corpus.json — md/txt bulk rates skipped, binary-format rates below still run");
    }

    // ── Real office/PDF/RTF files, sourced directly from disk ──
    // Each list is candidates-in-preference-order; the first that exists is
    // used. All were confirmed present on THIS machine via `es.exe` before
    // writing this test; the fallback entries and graceful skip keep the
    // test honest on a machine where a specific cache path has since churned.
    let candidates_by_ext: &[(&str, &[&str])] = &[
        ("pdf", &[
            r"C:\Program Files\smartmontools\doc\smartctl.8.pdf",
            r"C:\Program Files\VeraCrypt\docs\EFI-DCS\dcs_tpm_owner_02.pdf",
        ]),
        ("docx", &[
            r"C:\Users\Admin\.codex\plugins\cache\openai-curated-remote\openai-templates\0.1.0\skills\artifact-template-minimal-letterhead\assets\reference.docx",
        ]),
        ("xlsx", &[
            r"C:\Users\Admin\AppData\Local\Vivaldi\User Data\Default\Extensions\hehggadaopoacecdllhhajmbjkdcmajg\1.2.27221.15725_0\codex-sidepanel\assets\budget-planner-B_xX3PJl.xlsx",
        ]),
        ("pptx", &[
            r"C:\Users\Admin\.codex\plugins\cache\openai-curated-remote\openai-templates\0.1.0\skills\artifact-template-business-review\assets\reference.pptx",
        ]),
        ("rtf", &[
            r"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\IDE\CommonExtensions\Microsoft\VBCSharp\ExpressionEvaluators\EULA.rtf",
        ]),
    ];
    // ODF (odt/ods/odp): confirmed via `es -n 10 ext:odt,ods,odp` that this
    // machine has ZERO real files in any ODF format — honestly reported as
    // untestable-for-real-files here rather than silently omitted; ODF
    // extraction is exercised only synthetically, in `search_parity.rs`.
    tally.entry("odt/ods/odp").or_default(); // 0/0/0 — see note above

    for (ext, candidates) in candidates_by_ext {
        let t = tally.entry(ext).or_default();
        let Some(real_path) = candidates.iter().map(PathBuf::from).find(|p| p.exists()) else {
            println!("[skip] no candidate .{ext} file exists on this machine (tried {candidates:?})");
            continue;
        };
        t.found_on_disk += 1;
        let Some(meta) = meta_for_real(&real_path) else { continue };
        match extract_text(meta.clone()) {
            Ok(doc) => {
                let Some(word) = probe_word(&doc.body, 6) else {
                    println!("[note] .{ext} {real_path:?} extracted but no >=6-char word found to probe with");
                    continue;
                };
                t.extracted_ok += 1;
                engine.index_files_sync(vec![meta]).unwrap();
                let hits = engine.search(&query(&word, &[], 50, 0)).unwrap();
                if hits.iter().any(|h| h.path.eq_ignore_ascii_case(&real_path.to_string_lossy())) {
                    t.searchable += 1;
                }
                println!("  .{ext}: {real_path:?} → probe word {word:?}, searchable={}", t.searchable == t.extracted_ok);
            }
            Err(e) => println!("  .{ext}: {real_path:?} → extract_text FAILED: {e}"),
        }
    }

    println!("\n── Real-document extraction success rates ──");
    let mut exts: Vec<&&str> = tally.keys().collect();
    exts.sort();
    for ext in exts {
        let t = &tally[*ext];
        println!(
            "  .{ext:<10} found_on_disk={:<4} extracted_ok={:<4} end_to_end_searchable={}",
            t.found_on_disk, t.extracted_ok, t.searchable
        );
        if t.found_on_disk > 0 {
            assert_eq!(
                t.searchable, t.extracted_ok,
                ".{ext}: every real file whose extraction succeeded must also be end-to-end \
                 searchable by its own probe word"
            );
        }
    }
}
