// SPDX-License-Identifier: AGPL-3.0-or-later
//! Accuracy and performance benchmarks for the wincmd-search engine.
//!
//! Both tests are `#[ignore]` — run with:
//!   cargo test -p wincmd-search --release accuracy_labeled -- --ignored --nocapture
//!   cargo test -p wincmd-search --release perf -- --ignored --nocapture
//!
//! Set WCS_CORPUS=<path> to index a real directory tree in the perf test;
//! omit it for a portable synthetic-corpus baseline.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::TempDir;
use wincmd_search::{
    crawler::{build_globset, collect_files},
    types::{ContentQuery, IndexConfig},
    SearchEngine,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_config(index_dir: PathBuf, roots: Vec<PathBuf>) -> IndexConfig {
    IndexConfig {
        roots,
        exclusions: vec![],
        skip_paths: vec![],
        max_file_bytes: 10_000_000,
        index_dir,
    }
}

fn make_config_with_exclusions(
    index_dir: PathBuf,
    roots: Vec<PathBuf>,
    exclusions: Vec<String>,
    max_file_bytes: u64,
) -> IndexConfig {
    IndexConfig {
        roots,
        exclusions,
        skip_paths: vec![],
        max_file_bytes,
        index_dir,
    }
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

/// Index `files` via `index_files_sync` and return the engine ready to search.
fn index_sync(config: IndexConfig, files: Vec<wincmd_search::types::FileMeta>) -> SearchEngine {
    let engine = SearchEngine::open(config).unwrap();
    engine.index_files_sync(files).unwrap();
    engine
}

// ── Test 1: accuracy_labeled ──────────────────────────────────────────────────

/// Ground-truth accuracy test over a controlled synthetic corpus.
///
/// Metrics printed (visible with --nocapture):
///   Recall@k for each query group, Mean Reciprocal Rank, false-positive count.
///
/// Hard assertions (must all pass):
///   • unique-phrase doc is rank #1 and snippet contains the phrase
///   • absent term → 0 results
///   • multi-word query returns expected docs
///   • body-only term is found (content extraction works)
///   • common term recalls all expected docs
#[test]
#[ignore]
fn accuracy_labeled() {
    // ── Corpus setup ──────────────────────────────────────────────────────────
    let data_dir = TempDir::new().unwrap();
    let index_dir = TempDir::new().unwrap();

    // Doc A — unique phrase in body; filename is generic.
    fs::write(
        data_dir.path().join("report_alpha.txt"),
        "The quarterly review concluded with a unique phrase: \
         xylophagous_harbinger_9472 was identified as the primary anomaly. \
         Further analysis is pending.",
    )
    .unwrap();

    // Doc B — common term "revenue" in body; also appears in C.
    fs::write(
        data_dir.path().join("finance_report.txt"),
        "Annual revenue for fiscal year 2025 exceeded projections by 12 percent. \
         Revenue growth was driven by enterprise contracts and recurring licences.",
    )
    .unwrap();

    // Doc C — common term "revenue" in body; longer doc.
    fs::write(
        data_dir.path().join("summary.md"),
        "# Executive Summary\n\
         Total revenue reached a record high. The revenue pipeline looks healthy. \
         Key risks include supply chain disruption and regulatory changes in Q3.",
    )
    .unwrap();

    // Doc D — common term "revenue" also appears here (third doc for recall test).
    fs::write(
        data_dir.path().join("projections.txt"),
        "Revenue projections for 2026 depend on market share growth. \
         The finance team has prepared three revenue scenarios: base, bull, bear.",
    )
    .unwrap();

    // Doc E — body-only term "nebulosity"; filename has no hint.
    fs::write(
        data_dir.path().join("science_notes.txt"),
        "Atmospheric nebulosity affects stellar photometry significantly. \
         Observers must correct for nebulosity in their calibration pipeline.",
    )
    .unwrap();

    // Doc F — filename contains "nebula" but body does NOT contain "nebulosity".
    // Used to verify that a body-content search doesn't confuse filename tokens.
    fs::write(
        data_dir.path().join("nebula_catalog.txt"),
        "This catalog lists deep-sky objects including galaxies and star clusters. \
         No atmospheric correction data is included.",
    )
    .unwrap();

    // Doc G — multi-word query target: body contains "consensus mechanism".
    fs::write(
        data_dir.path().join("blockchain_primer.txt"),
        "A consensus mechanism is the core protocol by which distributed nodes \
         agree on the canonical state of a blockchain ledger.",
    )
    .unwrap();

    // Doc H — multi-word adjacent decoy: has "consensus" and "mechanism" but not adjacent.
    fs::write(
        data_dir.path().join("governance.md"),
        "Building consensus among stakeholders requires a clear decision mechanism. \
         The committee reached consensus after a 2-hour session.",
    )
    .unwrap();

    // Doc I — HTML file to test extension diversity.
    fs::write(
        data_dir.path().join("index.html"),
        "<html><body><h1>Search Engine Demo</h1>\
         <p>This page demonstrates the ephemeralquantum search capability.</p>\
         </body></html>",
    )
    .unwrap();

    // Doc J — term that will be absent from every doc.
    // (no file contains "zzznomatch_absent_token_9999")

    // Doc K — second unique-phrase doc (different phrase) — ensures no bleed-through.
    fs::write(
        data_dir.path().join("memo_k.txt"),
        "Operational note: the system deployed coruscant_vesper_7731 in production. \
         No issues were observed during the rollout.",
    )
    .unwrap();

    // ── Index ─────────────────────────────────────────────────────────────────
    let config = make_config(
        index_dir.path().to_path_buf(),
        vec![data_dir.path().to_path_buf()],
    );
    let exclusions = build_globset(&[]).unwrap();
    let files = collect_files(
        &[data_dir.path().to_path_buf()],
        &exclusions,
        &[],
        10_000_000,
    );
    println!("\n[accuracy_labeled] Indexed {} docs", files.len());
    let engine = index_sync(config, files);

    // ── Query set ─────────────────────────────────────────────────────────────
    // Each entry: (label, query_terms, expected_names_in_results, expected_rank1_name)
    // expected_rank1_name = None means "any non-empty result set is fine for rank-1"
    struct LabeledQuery {
        label: &'static str,
        terms: &'static str,
        expected_names: Vec<&'static str>, // all must appear (recall)
        expected_rank1: Option<&'static str>, // must be rank 1 if Some
        absent: bool,                      // if true, expect 0 results
    }

    let queries = vec![
        LabeledQuery {
            label: "unique-phrase body-only",
            terms: "xylophagous_harbinger_9472",
            expected_names: vec!["report_alpha.txt"],
            expected_rank1: Some("report_alpha.txt"),
            absent: false,
        },
        LabeledQuery {
            label: "common term recall (revenue)",
            terms: "revenue",
            expected_names: vec!["finance_report.txt", "summary.md", "projections.txt"],
            expected_rank1: None,
            absent: false,
        },
        LabeledQuery {
            label: "multi-word query (both docs contain both terms)",
            terms: "consensus mechanism",
            // BM25 ranks by term frequency — governance.md repeats "consensus" twice
            // so it may outscore blockchain_primer.txt; we assert recall, not rank-1.
            expected_names: vec!["blockchain_primer.txt", "governance.md"],
            expected_rank1: None,
            absent: false,
        },
        LabeledQuery {
            label: "body-only term (nebulosity)",
            terms: "nebulosity",
            expected_names: vec!["science_notes.txt"],
            expected_rank1: Some("science_notes.txt"),
            absent: false,
        },
        LabeledQuery {
            label: "absent term → 0 results",
            terms: "zzznomatch_absent_token_9999",
            expected_names: vec![],
            expected_rank1: None,
            absent: true,
        },
        LabeledQuery {
            label: "second unique phrase",
            terms: "coruscant_vesper_7731",
            expected_names: vec!["memo_k.txt"],
            expected_rank1: Some("memo_k.txt"),
            absent: false,
        },
        LabeledQuery {
            label: "HTML body extraction (ephemeralquantum)",
            terms: "ephemeralquantum",
            expected_names: vec!["index.html"],
            expected_rank1: Some("index.html"),
            absent: false,
        },
    ];

    // ── Evaluate ──────────────────────────────────────────────────────────────
    let k = 10usize;
    let mut total_rr = 0.0f64; // for MRR
    let mut mrr_count = 0usize;
    let mut fp_count = 0usize;
    let mut recall_sum = 0.0f64;
    let mut recall_n = 0usize;

    for q in &queries {
        let hits = engine.search(&keyword_query(q.terms, k)).unwrap();

        // False positives: any result returned for an absent-term query.
        if q.absent {
            if !hits.is_empty() {
                println!(
                    "  [FAIL] '{}': expected 0 results, got {} — FP!",
                    q.label,
                    hits.len()
                );
                fp_count += hits.len();
            } else {
                println!("  [OK]  '{}': 0 results (correct)", q.label);
            }
            continue;
        }

        // Recall@k
        let hit_names: Vec<String> = hits.iter().map(|h| h.name.clone()).collect();
        let found: Vec<&&str> = q
            .expected_names
            .iter()
            .filter(|n| hit_names.iter().any(|h| h == **n))
            .collect();
        let recall = found.len() as f64 / q.expected_names.len().max(1) as f64;
        recall_sum += recall;
        recall_n += 1;

        // MRR contribution — find the first expected doc's rank.
        let first_rank = q
            .expected_rank1
            .and_then(|name| hits.iter().position(|h| h.name == name).map(|i| i + 1));
        if let Some(rank) = first_rank {
            total_rr += 1.0 / rank as f64;
            mrr_count += 1;
        }

        // Snippet presence for rank-1 assertions.
        let rank1_ok = q
            .expected_rank1
            .is_none_or(|name| hits.first().is_some_and(|h| h.name == name));
        let snippet_ok = q.expected_rank1.is_none_or(|name| {
            hits.first()
                .is_some_and(|h| h.name == name && !h.snippet.is_empty())
        });

        println!(
            "  [{}] '{}': recall@{k}={:.2}, rank1={}, snippet={}  | hits: {:?}",
            if recall >= 1.0 && rank1_ok {
                "OK  "
            } else {
                "FAIL"
            },
            q.label,
            recall,
            if rank1_ok { "OK" } else { "WRONG" },
            if snippet_ok { "OK" } else { "MISSING" },
            hit_names,
        );

        // Hard assertions
        if let Some(name) = q.expected_rank1 {
            assert!(
                rank1_ok,
                "Query '{}': expected rank-1 = '{}', got {:?}",
                q.terms,
                name,
                hit_names.first()
            );
            assert!(
                snippet_ok,
                "Query '{}': rank-1 hit '{}' has empty snippet",
                q.terms, name
            );
        }
        assert!(
            recall >= 1.0,
            "Query '{}': recall@{k} = {:.2} — missing docs: {:?}",
            q.terms,
            recall,
            q.expected_names
                .iter()
                .filter(|n| !hit_names.iter().any(|h| h == **n))
                .collect::<Vec<_>>()
        );
    }

    let mean_recall = if recall_n > 0 {
        recall_sum / recall_n as f64
    } else {
        0.0
    };
    let mrr = if mrr_count > 0 {
        total_rr / mrr_count as f64
    } else {
        0.0
    };

    println!("\n── Accuracy summary ──────────────────────────────────────────");
    println!("  Queries evaluated : {}", queries.len());
    println!("  Mean recall@{k}    : {mean_recall:.3}");
    println!("  MRR               : {mrr:.3}");
    println!("  False positives   : {fp_count}");

    assert_eq!(fp_count, 0, "false positive count must be 0");
    assert!(mrr >= 0.8, "MRR {mrr:.3} below threshold 0.8");
    assert!(mean_recall >= 1.0, "Mean recall {mean_recall:.3} below 1.0");
}

// ── Test 2: perf ──────────────────────────────────────────────────────────────

/// Performance benchmark: measures discovery, index throughput, and query latency.
///
/// If WCS_CORPUS env var is set, indexes that directory tree (exclusions applied).
/// Otherwise generates ~3000 synthetic docs in a tempdir.
///
/// Printed metrics (--nocapture):
///   File count, total bytes, discovery time, index build time, files/sec, MB/sec,
///   index dir size on disk, query latency min/p50/p95/max, avg result count.
#[test]
#[ignore]
fn perf() {
    // ── Corpus selection ──────────────────────────────────────────────────────
    let corpus_path = std::env::var("WCS_CORPUS").ok().map(PathBuf::from);

    let index_dir = TempDir::new().unwrap();
    // Kept alive for lifetime — must outlive collect_files; only created for synthetic path.
    let _synthetic_dir: Option<TempDir>;

    let (roots, exclusions, max_file_bytes, corpus_label) = if let Some(ref p) = corpus_path {
        println!("\n[perf] Using real corpus: {}", p.display());
        let excl = vec![
            "node_modules".into(),
            "target".into(),
            "target-bench".into(),
            ".git".into(),
            "dist".into(),
            "build".into(),
            ".pytest_cache".into(),
            "obsidian-vault".into(),
        ];
        _synthetic_dir = None;
        (
            vec![p.clone()],
            excl,
            5_000_000u64,
            format!("real:{}", p.display()),
        )
    } else {
        println!("\n[perf] WCS_CORPUS not set — generating synthetic corpus (~3000 docs)");
        let tmp = TempDir::new().unwrap();
        generate_synthetic_corpus(tmp.path(), 3000);
        let root = tmp.path().to_path_buf();
        _synthetic_dir = Some(tmp);
        (vec![root], vec![], 10_000_000u64, "synthetic:3000".into())
    };

    // ── Discovery ─────────────────────────────────────────────────────────────
    let gs = build_globset(&exclusions).unwrap();
    let t_discover = Instant::now();
    let files = collect_files(&roots, &gs, &[], max_file_bytes);
    let discovery_ms = t_discover.elapsed().as_millis();

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let file_count = files.len();

    println!("  Corpus label : {corpus_label}");
    println!("  Files found  : {file_count}");
    println!(
        "  Total bytes  : {:.2} MB",
        total_bytes as f64 / 1_048_576.0
    );
    println!("  Discovery    : {discovery_ms} ms");

    if file_count == 0 {
        println!("  [WARN] No files found — skipping index/query perf");
        return;
    }

    // ── Index build ───────────────────────────────────────────────────────────
    let config = make_config_with_exclusions(
        index_dir.path().to_path_buf(),
        roots.clone(),
        exclusions,
        max_file_bytes,
    );
    let engine = SearchEngine::open(config).unwrap();

    let t_index = Instant::now();
    engine.index_files_sync(files).unwrap();
    let index_ms = t_index.elapsed().as_millis();

    let files_per_sec = if index_ms > 0 {
        file_count as f64 / (index_ms as f64 / 1000.0)
    } else {
        f64::INFINITY
    };
    let mb_per_sec = if index_ms > 0 {
        (total_bytes as f64 / 1_048_576.0) / (index_ms as f64 / 1000.0)
    } else {
        f64::INFINITY
    };

    // Index dir size on disk.
    let index_size = dir_size_bytes(index_dir.path());

    println!("  Index build  : {index_ms} ms");
    println!("  Files/sec    : {files_per_sec:.0}");
    println!("  MB/sec       : {mb_per_sec:.2}");
    println!("  Index size   : {:.2} MB", index_size as f64 / 1_048_576.0);

    // ── Query latency ─────────────────────────────────────────────────────────
    // Representative queries that should return varied result counts.
    let query_terms = [
        "the", "system", "error", "config", "data", "file", "update", "network", "security",
        "process",
    ];
    let repeats = 50usize;
    let limit = 20usize;

    println!("\n  Query latency (each repeated {repeats}×, limit={limit}):");
    let mut all_latencies_us: Vec<u64> = Vec::with_capacity(query_terms.len() * repeats);
    let mut total_results = 0usize;
    let mut query_runs = 0usize;

    for term in &query_terms {
        let q = keyword_query(term, limit);
        let mut latencies: Vec<u64> = Vec::with_capacity(repeats);
        let mut result_count = 0usize;

        for _ in 0..repeats {
            let t0 = Instant::now();
            let hits = engine.search(&q).unwrap();
            let us = t0.elapsed().as_micros() as u64;
            latencies.push(us);
            result_count += hits.len();
        }

        latencies.sort_unstable();
        let p50 = latencies[repeats / 2];
        let p95 = latencies[(repeats as f64 * 0.95) as usize];
        let avg_results = result_count / repeats;

        println!(
            "    {:12} → min={:5}µs  p50={:5}µs  p95={:5}µs  max={:5}µs  avg_results={}",
            term,
            latencies[0],
            p50,
            p95,
            latencies[repeats - 1],
            avg_results,
        );

        all_latencies_us.extend_from_slice(&latencies);
        total_results += result_count;
        query_runs += repeats;
    }

    all_latencies_us.sort_unstable();
    let n = all_latencies_us.len();
    let overall_p50 = all_latencies_us[n / 2];
    let overall_p95 = all_latencies_us[(n as f64 * 0.95) as usize];
    let overall_min = all_latencies_us[0];
    let overall_max = all_latencies_us[n - 1];
    let avg_results = total_results / query_runs;

    println!("\n── Perf summary ───────────────────────────────────────────────");
    println!("  Corpus       : {corpus_label}");
    println!("  Files indexed: {file_count}");
    println!(
        "  Total bytes  : {:.2} MB",
        total_bytes as f64 / 1_048_576.0
    );
    println!("  Discovery    : {discovery_ms} ms");
    println!("  Index build  : {index_ms} ms");
    println!("  Files/sec    : {files_per_sec:.0}");
    println!("  MB/sec       : {mb_per_sec:.2}");
    println!("  Index size   : {:.2} MB", index_size as f64 / 1_048_576.0);
    println!("  Query p50    : {overall_p50} µs");
    println!("  Query p95    : {overall_p95} µs");
    println!("  Query min    : {overall_min} µs");
    println!("  Query max    : {overall_max} µs");
    println!("  Avg results  : {avg_results}");

    // Sanity: query latency should not be pathological.
    assert!(
        overall_p95 < 5_000_000,
        "p95 query latency {overall_p95}µs exceeds 5s — something is very wrong"
    );
}

// ── Synthetic corpus generator ────────────────────────────────────────────────

/// Generate `n` varied-length plaintext files with realistic word distributions.
fn generate_synthetic_corpus(dir: &std::path::Path, n: usize) {
    // Word pool — varied tokens so BM25 scoring exercises real distributions.
    const WORDS: &[&str] = &[
        "the",
        "system",
        "error",
        "config",
        "data",
        "file",
        "update",
        "network",
        "security",
        "process",
        "user",
        "event",
        "log",
        "record",
        "index",
        "search",
        "query",
        "result",
        "match",
        "score",
        "token",
        "field",
        "value",
        "key",
        "hash",
        "path",
        "root",
        "directory",
        "extension",
        "metadata",
        "size",
        "timestamp",
        "version",
        "release",
        "build",
        "target",
        "module",
        "library",
        "function",
        "method",
        "argument",
        "return",
        "type",
        "struct",
        "enum",
        "trait",
        "impl",
        "async",
        "await",
        "thread",
        "channel",
        "mutex",
        "arc",
        "policy",
        "rule",
        "permission",
        "access",
        "role",
        "admin",
        "device",
        "fleet",
        "agent",
        "server",
        "client",
        "request",
        "response",
        "status",
        "health",
        "monitor",
        "alert",
        "incident",
        "audit",
        "compliance",
        "report",
    ];

    let exts = ["txt", "md", "rs", "log", "html"];
    let mut rng = SimpleRng::new(0xDEAD_BEEF_1234_5678u64);

    for i in 0..n {
        let ext = exts[i % exts.len()];
        let fname = format!("doc_{i:04}.{ext}");
        // Varied body length: 200–8000 chars.
        let target_len = 200 + (rng.next_u64() % 7801) as usize;
        let mut body = String::with_capacity(target_len + 20);
        while body.len() < target_len {
            let word = WORDS[rng.next_u64() as usize % WORDS.len()];
            body.push_str(word);
            body.push(' ');
        }
        // Inject a unique identifier in every 10th doc for deterministic recall tests.
        if i % 10 == 0 {
            body.push_str(&format!(" synth_unique_doc_{i} "));
        }
        fs::write(dir.join(fname), body.as_bytes()).unwrap();
    }
}

/// Minimal deterministic PRNG (xorshift64) — avoids pulling in rand crate.
struct SimpleRng(u64);

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// Sum of all file sizes under `dir` (non-recursive would miss subdirs, but
/// our index_dir is flat so one level suffices).
fn dir_size_bytes(dir: &std::path::Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}
