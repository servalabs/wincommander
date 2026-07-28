// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tantivy-backed keyword index for wincmd-search.

use std::collections::HashSet;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use tantivy::{
    collector::TopDocs,
    doc,
    query::{
        AllQuery, BooleanQuery, BoostQuery, Occur, Query, QueryParser, RangeQuery, RegexQuery,
        TermQuery,
    },
    schema::{
        IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, FAST, INDEXED, STORED,
        STRING,
    },
    snippet::SnippetGenerator,
    Index, IndexReader, IndexWriter, ReloadPolicy, Searcher, TantivyDocument, Term,
};

use crate::error::Result;
use crate::filters::{self, QueryFilters};
use crate::tokenize::{self, CODE_TOKENIZER, RAW_LC_TOKENIZER};
use crate::types::{ContentHit, ContentQuery, DocId, DocProps, FileMeta, MatchKind};

/// Bumped whenever the schema or an analyzer changes incompatibly. An index
/// carrying an older (or missing) `schema.version` marker is wiped on open and
/// rebuilt by the next crawl — tantivy field handles are positional, so
/// opening an old-generation index with new code would corrupt queries.
/// v4: added `author`/`tags`/`doc_title` metadata fields and made `size`
/// INDEXED (was FAST-only) so `size:` range filters can query it.
/// v5: `wc_raw_lc` (the `name_lc` field's analyzer) gained `AsciiFoldingFilter`
/// so the filename SUBSTRING rung folds accents/ligatures like `wc_code`
/// already does for name/title/body.
/// v6: `wc_code` gained `DigitPrefixFilter` — a long pure-digit token (an
/// enrollment/invoice/phone number with no alpha↔digit boundary to split at)
/// now also indexes its own left-anchored prefixes, so a query for its first
/// N digits matches. Existing installs re-index once to backfill the new
/// prefix postings for already-extracted documents.
const SCHEMA_VERSION: &str = "6";
const SCHEMA_VERSION_FILE: &str = "schema.version";

/// Wraps a tantivy `Index` with typed field handles.
pub struct ContentIndex {
    pub index: Index,
    /// Shared, long-lived reader. Rebuilding a reader per query cost ~11 ms;
    /// this one is kept current by `commit()` calling `reload()`.
    reader: IndexReader,
    pub f_doc_id: tantivy::schema::Field,
    pub f_path: tantivy::schema::Field,
    pub f_name: tantivy::schema::Field,
    pub f_directory: tantivy::schema::Field,
    pub f_ext: tantivy::schema::Field,
    pub f_mtime: tantivy::schema::Field,
    pub f_size: tantivy::schema::Field,
    pub f_title: tantivy::schema::Field,
    pub f_body: tantivy::schema::Field,
    pub f_name_lc: tantivy::schema::Field,
    pub f_author: tantivy::schema::Field,
    pub f_tags: tantivy::schema::Field,
    pub f_doc_title: tantivy::schema::Field,
}

impl ContentIndex {
    /// Open an existing tantivy index at `dir`, or create a new one.
    ///
    /// An index written by an older schema generation is wiped here (contents
    /// only — the dir itself keeps its hardened ACLs) and recreated empty; the
    /// caller's next crawl repopulates it.
    pub fn open_or_create(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;

        let version_file = dir.join(SCHEMA_VERSION_FILE);
        if dir.join("meta.json").exists() {
            let on_disk = std::fs::read_to_string(&version_file).unwrap_or_default();
            if on_disk.trim() != SCHEMA_VERSION {
                wipe_dir_contents(dir)?;
            }
        }

        let code_text = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer(CODE_TOKENIZER)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        // Whole lowercased file name as ONE term; not stored (name is).
        let raw_lc_text = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(RAW_LC_TOKENIZER)
                .set_index_option(IndexRecordOption::Basic),
        );

        let mut sb = Schema::builder();
        let f_doc_id = sb.add_u64_field("doc_id", INDEXED | STORED | FAST);
        let f_path = sb.add_text_field("path", STRING | STORED);
        let f_name = sb.add_text_field("name", code_text.clone());
        let f_directory = sb.add_text_field("directory", STRING | STORED);
        let f_ext = sb.add_text_field("ext", STRING | STORED | FAST);
        let f_mtime = sb.add_u64_field("mtime", INDEXED | STORED | FAST);
        // INDEXED (not just FAST) so `size:` RangeQuery has an inverted-index
        // fallback path, mirroring mtime.
        let f_size = sb.add_u64_field("size", INDEXED | STORED | FAST);
        let f_title = sb.add_text_field("title", code_text.clone());
        let f_body = sb.add_text_field("body", code_text.clone());
        let f_name_lc = sb.add_text_field("name_lc", raw_lc_text);
        let f_author = sb.add_text_field("author", code_text.clone());
        let f_tags = sb.add_text_field("tags", code_text.clone());
        let f_doc_title = sb.add_text_field("doc_title", code_text);
        let schema = sb.build();

        // Re-open if the index already exists (meta.json present), create otherwise.
        let index = if dir.join("meta.json").exists() {
            Index::open_in_dir(dir)?
        } else {
            let index = Index::create_in_dir(dir, schema)?;
            std::fs::write(&version_file, SCHEMA_VERSION)?;
            index
        };
        // Custom analyzers live in the process, not on disk — required after
        // every open/create, before any indexing or query parsing.
        tokenize::register_tokenizers(&index);

        // One reader for the lifetime of the index; `commit()` reloads it so
        // freshness matches the old per-query-reader behaviour without the cost.
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(Self {
            index,
            reader,
            f_doc_id,
            f_path,
            f_name,
            f_directory,
            f_ext,
            f_mtime,
            f_size,
            f_title,
            f_body,
            f_name_lc,
            f_author,
            f_tags,
            f_doc_title,
        })
    }

    /// Acquire an `IndexWriter` with a 50 MB heap.
    pub fn writer(&self) -> Result<IndexWriter> {
        Ok(self.index.writer(50_000_000)?)
    }

    /// Index (or re-index) a single file.
    ///
    /// Deletes any existing entry for `meta.doc_id` before inserting the new one,
    /// so this is safe to call on every crawl pass.
    pub fn upsert(
        &self,
        writer: &mut IndexWriter,
        meta: &FileMeta,
        title: &str,
        body: &str,
        props: &DocProps,
    ) -> Result<()> {
        // Delete any stale version first.
        let term = tantivy::Term::from_field_u64(self.f_doc_id, meta.doc_id);
        writer.delete_term(term);

        let directory = meta
            .path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        writer.add_document(doc!(
            self.f_doc_id    => meta.doc_id,
            self.f_path      => meta.path.to_string_lossy().as_ref(),
            self.f_name      => meta.name.as_str(),
            self.f_directory => directory.as_str(),
            self.f_ext       => meta.ext.as_str(),
            self.f_mtime     => meta.mtime,
            self.f_size      => meta.size,
            self.f_title     => title,
            self.f_body      => body,
            // Analyzer lowercases; feed the raw name.
            self.f_name_lc   => meta.name.as_str(),
            // Empty strings are fine to index — most files have no metadata.
            self.f_author    => props.author.as_str(),
            self.f_tags      => props.tags.as_str(),
            self.f_doc_title => props.doc_title.as_str(),
        ))?;
        Ok(())
    }

    /// Remove a document by its stable `DocId`.
    pub fn delete(&self, writer: &mut IndexWriter, doc_id: crate::types::DocId) {
        let term = tantivy::Term::from_field_u64(self.f_doc_id, doc_id);
        writer.delete_term(term);
    }

    /// Flush buffered writes to the index.
    pub fn commit(&self, writer: &mut IndexWriter) -> Result<()> {
        writer.commit()?;
        // Force the shared reader to the just-committed generation so searches
        // and get_chunks see new/updated/deleted docs immediately.
        self.reader.reload()?;
        Ok(())
    }

    /// Clone of the shared, always-current reader (cheap Arc clone) — used by
    /// `SearchEngine::get_chunks`.
    pub fn index_reader(&self) -> Result<IndexReader> {
        Ok(self.reader.clone())
    }

    /// BM25 keyword search; returns ranked hits with HTML-escaped snippets.
    ///
    /// `ext:`/`after:`/`before:`/`size:` filter tokens (`filters::parse_filters`)
    /// are stripped from the query before it ever reaches tantivy's parser —
    /// they're applied as Must clauses (see `filtered_query`) alongside the
    /// text query's Must scoring clause, never as parseable `field:term` text.
    ///
    /// Single-word remaining-text queries additionally get a file-name
    /// SUBSTRING rung: a regex scan of the `name_lc` term dictionary, so `982`
    /// still finds `XXXXXXXX2982.pdf` even though no token boundary exposes
    /// it. Those hits are appended after the BM25 hits with
    /// `MatchKind::NameSubstring`, filtered post-hoc against the same filters.
    ///
    /// A non-empty `query.roots` scopes every rung to files under one of those
    /// folders (`path_in_roots`); empty `roots` searches the whole index. That
    /// scope is a POST-HOC filter, not a tantivy clause, so `fetch` is
    /// over-fetched via `scoped_fetch` — see both helpers for why.
    pub fn search_keyword(&self, query: &ContentQuery) -> Result<Vec<ContentHit>> {
        let searcher = self.reader.searcher();
        let (text_terms, filters) = filters::parse_filters(&query.terms);
        let text_is_empty = text_terms.trim().is_empty();

        let qp = QueryParser::for_index(
            &self.index,
            vec![
                self.f_title,
                self.f_body,
                self.f_name,
                self.f_author,
                self.f_tags,
                self.f_doc_title,
            ],
        );
        let fetch = scoped_fetch(query.limit + query.offset, &query.roots);

        // `text_query` is the TEXT-only sub-query — used both for scoring
        // (wrapped with filters via `filtered_query`) and, unwrapped, as the
        // snippet generator's query so filter clauses never affect
        // highlighting. It stays `None` for a whitespace-only query or one
        // that fails to parse even after sanitizing — those hits (if any, via
        // a filters-only AllQuery search) fall back to `fallback_snippet`.
        let mut text_query: Option<Box<dyn Query>> = None;
        let mut top_docs: Vec<(tantivy::Score, tantivy::DocAddress)> = Vec::new();

        if !text_is_empty {
            // A search box must never surface a query-syntax error. Parse the
            // remaining text first (power users keep quotes / AND / OR), then
            // fall back to it with tantivy metacharacters neutralised, and
            // finally to no text query at all — never an Err. The sanitized
            // retry also runs when the raw parse SUCCEEDS but matches
            // nothing: `budget-report` parses as `budget AND NOT report` and
            // silently excludes everything the user was looking for.
            if let Ok(q) = qp.parse_query(&text_terms) {
                let searched = self.filtered_query(q.box_clone(), &filters);
                top_docs =
                    searcher.search(&searched, &TopDocs::with_limit(fetch).order_by_score())?;
                text_query = Some(q);
            }
            // `budget-report` can also parse as `budget AND NOT report` and
            // still return SOME hits (docs with "budget" alone) — a non-empty
            // result, so the zero-hit retry below wouldn't fire, yet docs
            // containing BOTH words stay silently excluded by the NOT clause.
            // When the raw text has an unquoted hyphen, always also run the
            // sanitized bag-of-words parse and MERGE its hits in (rather than
            // only falling back on zero hits), so those docs surface too.
            let raw_had_unquoted_hyphen = has_unquoted_hyphen(&text_terms);
            if top_docs.is_empty() || raw_had_unquoted_hyphen {
                let safe = sanitize_query(&text_terms);
                if safe.trim() != text_terms.trim() && !safe.trim().is_empty() {
                    if let Ok(q) = qp.parse_query(&safe) {
                        let searched = self.filtered_query(q.box_clone(), &filters);
                        let td = searcher
                            .search(&searched, &TopDocs::with_limit(fetch).order_by_score())?;
                        if top_docs.is_empty() {
                            if !td.is_empty() || text_query.is_none() {
                                top_docs = td;
                                text_query = Some(q);
                            }
                        } else if raw_had_unquoted_hyphen && !td.is_empty() {
                            let existing: HashSet<tantivy::DocAddress> =
                                top_docs.iter().map(|(_, addr)| *addr).collect();
                            let mut merged_any = false;
                            for (score, addr) in td {
                                if !existing.contains(&addr) {
                                    top_docs.push((score, addr));
                                    merged_any = true;
                                }
                            }
                            if merged_any {
                                top_docs.sort_by(|a, b| {
                                    b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
                                });
                                top_docs.truncate(fetch);
                                // Highlight with the union of both interpretations so a
                                // merged-in doc (which only matches the sanitized parse)
                                // still gets its terms highlighted, not just the raw
                                // NOT-query's surviving positive term.
                                if let Some(raw_q) = text_query.take() {
                                    text_query = Some(Box::new(BooleanQuery::new(vec![
                                        (Occur::Should, raw_q),
                                        (Occur::Should, q.box_clone()),
                                    ]))
                                        as Box<dyn Query>);
                                } else {
                                    text_query = Some(q);
                                }
                            }
                        }
                    }
                }
            }
        } else if !filters.is_empty() {
            // Filters-only query (e.g. `ext:pdf` alone) — legal. Score
            // everything via AllQuery; `text_query` stays `None` since there's
            // nothing to highlight.
            let searched = self.filtered_query(Box::new(AllQuery), &filters);
            top_docs = searcher.search(&searched, &TopDocs::with_limit(fetch).order_by_score())?;
        }

        let mut hits = Vec::new();
        let mut seen: HashSet<DocId> = HashSet::new();

        let snippet_gen = match &text_query {
            Some(q) => {
                let mut sg = SnippetGenerator::create(&searcher, q, self.f_body)?;
                sg.set_max_num_chars(200);
                Some(sg)
            }
            None => None,
        };

        // KT: the root scope is applied HERE, not by `.skip(query.offset)` on
        // the raw ranking — `offset` must count only docs that SURVIVE the
        // scope, or page 2 of a scoped search would page through out-of-scope
        // docs and come back empty. Stored-doc retrieval moved above the skip
        // for the same reason (the path lives in the stored doc); snippets are
        // still only built for the rows actually returned.
        let mut skipped = 0usize;
        for (score, addr) in top_docs {
            if hits.len() >= query.limit {
                break;
            }
            let doc: TantivyDocument = searcher.doc(addr)?;
            let path = owned_str(&doc, self.f_path);
            if !path_in_roots(&path, &query.roots) {
                continue;
            }
            if skipped < query.offset {
                skipped += 1;
                continue;
            }
            let snippet = match &snippet_gen {
                Some(sg) => {
                    let mut snippet = sg.snippet_from_doc(&doc);
                    // tantivy defaults highlights to <b>…</b>; the frontend splits on
                    // <mark> (and the ContentHit contract says <mark>). Emit <mark> so
                    // matches actually render highlighted instead of leaking literal
                    // <b> tags into the snippet text. to_html() escapes the body but
                    // writes the prefix/postfix literally.
                    snippet.set_snippet_prefix_postfix("<mark>", "</mark>");
                    let raw_snippet = snippet.to_html();
                    if raw_snippet.trim().is_empty() {
                        // No body match (term hit only title/name/metadata).
                        self.fallback_snippet(&doc)
                    } else {
                        raw_snippet
                    }
                }
                None => self.fallback_snippet(&doc),
            };
            let doc_id = owned_u64(&doc, self.f_doc_id);
            seen.insert(doc_id);
            hits.push(ContentHit {
                doc_id,
                path,
                name: owned_str(&doc, self.f_name),
                ext: owned_str(&doc, self.f_ext),
                mtime: owned_u64(&doc, self.f_mtime),
                size: owned_u64(&doc, self.f_size),
                score,
                match_kind: MatchKind::Keyword,
                snippet,
                author: owned_str(&doc, self.f_author),
                doc_title: owned_str(&doc, self.f_doc_title),
                tags: owned_str(&doc, self.f_tags),
            });
        }

        // KT: this rung is not offset-paginated — it always regex-scans from
        // the top of the name_lc term dictionary, so a page-2 call would
        // silently re-append page-1's substring hits. All current callers
        // pass offset 0; skip the rung entirely once that's no longer true.
        if query.offset == 0 {
            self.append_name_substring_hits(
                &searcher,
                &text_terms,
                &filters,
                query,
                &mut seen,
                &mut hits,
            );
        }
        hits.truncate(query.limit);
        Ok(hits)
    }

    /// Wraps `text` as a Must scoring clause alongside Must filter clauses
    /// derived from `filters` (ext/mtime/size); returns `text` unwrapped when
    /// `filters.is_empty()` so the filter-free path has zero BooleanQuery
    /// overhead and behaves exactly as before this feature existed.
    fn filtered_query(&self, text: Box<dyn Query>, filters: &QueryFilters) -> Box<dyn Query> {
        if filters.is_empty() {
            return text;
        }
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, text)];
        clauses.extend(self.build_filter_clauses(filters));
        Box::new(BooleanQuery::new(clauses))
    }

    /// Must-clauses for `ext:`/`after:`/`before:`/`size:` filters. Empty when
    /// `filters.is_empty()`.
    ///
    /// KT: every clause is wrapped in `BoostQuery::new(_, 0.0)` — a Must clause
    /// still contributes its own BM25/IDF score by default, so `ext:pdf,docx`
    /// (an OR of two TermQuerys) would add per-extension IDF into the total
    /// and perturb ranking. Zero-boost makes these pure membership constraints
    /// (the doc set is unchanged; the score contribution is forced to 0),
    /// leaving ordering entirely to the text query.
    fn build_filter_clauses(&self, filters: &QueryFilters) -> Vec<(Occur, Box<dyn Query>)> {
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        if !filters.exts.is_empty() {
            // exts are already lowercased/dot-stripped by the filter parser,
            // matching how the crawler writes f_ext.
            let should: Vec<(Occur, Box<dyn Query>)> = filters
                .exts
                .iter()
                .map(|ext| {
                    let term = Term::from_field_text(self.f_ext, ext);
                    let q: Box<dyn Query> =
                        Box::new(TermQuery::new(term, IndexRecordOption::Basic));
                    (Occur::Should, q)
                })
                .collect();
            let ext_query: Box<dyn Query> = Box::new(BooleanQuery::new(should));
            clauses.push((Occur::Must, Box::new(BoostQuery::new(ext_query, 0.0))));
        }

        if filters.after.is_some() || filters.before.is_some() {
            let lo = filters.after.unwrap_or(0);
            let hi = filters.before.unwrap_or(u64::MAX);
            let range_query: Box<dyn Query> = Box::new(RangeQuery::new(
                Bound::Included(Term::from_field_u64(self.f_mtime, lo)),
                Bound::Excluded(Term::from_field_u64(self.f_mtime, hi)),
            ));
            clauses.push((Occur::Must, Box::new(BoostQuery::new(range_query, 0.0))));
        }

        if filters.size_min.is_some() || filters.size_max.is_some() {
            let lo = filters.size_min.unwrap_or(0);
            let hi = filters.size_max.unwrap_or(u64::MAX);
            let range_query: Box<dyn Query> = Box::new(RangeQuery::new(
                Bound::Included(Term::from_field_u64(self.f_size, lo)),
                Bound::Excluded(Term::from_field_u64(self.f_size, hi)),
            ));
            clauses.push((Occur::Must, Box::new(BoostQuery::new(range_query, 0.0))));
        }

        clauses
    }

    /// Run `text` through the registered `wc_raw_lc` analyzer (lowercase +
    /// ASCII-fold) — the exact analyzer that produced `name_lc`'s term
    /// dictionary — so a query needle folds identically to stored names.
    /// Falls back to a plain lowercase if the analyzer is somehow missing
    /// (never happens outside a test double; degrades gracefully either way).
    fn fold_like_name_lc(&self, text: &str) -> String {
        match self.index.tokenizers().get(RAW_LC_TOKENIZER) {
            Some(mut analyzer) => {
                let mut stream = analyzer.token_stream(text);
                let mut out = String::new();
                while stream.advance() {
                    out.push_str(&stream.token().text);
                }
                out
            }
            None => text.to_lowercase(),
        }
    }

    /// Substring rung: for a single-word remaining-text query, regex-scan the
    /// `name_lc` term dictionary for `.*<needle>.*` and append unseen docs
    /// that also satisfy `filters` and `query.roots`. Best-effort — any failure
    /// degrades to "no extra hits", never an Err.
    ///
    /// Takes the whole `query` (not just its `limit`) because the root scope
    /// applies to this rung too: filtering AFTER a `limit`-sized regex fetch
    /// would silently drop scoped hits that ranked below out-of-scope ones, so
    /// the fetch is widened by `scoped_fetch` exactly like the BM25 rung's.
    fn append_name_substring_hits(
        &self,
        searcher: &Searcher,
        text_terms: &str,
        filters: &QueryFilters,
        query: &ContentQuery,
        seen: &mut HashSet<DocId>,
        hits: &mut Vec<ContentHit>,
    ) {
        let limit = query.limit;
        let raw_needle = text_terms.trim();
        if raw_needle.len() < 2
            || raw_needle.len() > 64
            || raw_needle.chars().any(char::is_whitespace)
        {
            return;
        }
        // Fold the needle through the SAME analyzer (`wc_raw_lc`) that indexed
        // `name_lc`, so an accented/ligature needle (or an accented stored
        // name) matches on either side — e.g. "cafe" finds "café.txt" and
        // "café" still finds a plain "cafe.txt".
        let needle = self.fold_like_name_lc(raw_needle);
        if needle.is_empty() {
            return;
        }
        let pattern = format!(".*{}.*", regex_escape(&needle));
        let Ok(rq) = RegexQuery::from_pattern(&pattern, self.f_name_lc) else {
            return;
        };
        let fetch = scoped_fetch(limit, &query.roots);
        let Ok(top) = searcher.search(&rq, &TopDocs::with_limit(fetch).order_by_score()) else {
            return;
        };
        for (score, addr) in top {
            if hits.len() >= limit {
                break;
            }
            let Ok(doc) = searcher.doc::<TantivyDocument>(addr) else {
                continue;
            };
            let doc_id = owned_u64(&doc, self.f_doc_id);
            if seen.contains(&doc_id) {
                continue;
            }
            let ext = owned_str(&doc, self.f_ext);
            let mtime = owned_u64(&doc, self.f_mtime);
            let size = owned_u64(&doc, self.f_size);
            // This rung scores via a raw regex scan with no filter awareness
            // (unlike the BM25 rung's tantivy Musts), so re-check post-hoc.
            if !hit_passes_filters(filters, &ext, mtime, size) {
                continue;
            }
            let path = owned_str(&doc, self.f_path);
            if !path_in_roots(&path, &query.roots) {
                continue;
            }
            seen.insert(doc_id);
            let snippet = self.fallback_snippet(&doc);
            hits.push(ContentHit {
                doc_id,
                path,
                name: owned_str(&doc, self.f_name),
                ext,
                mtime,
                size,
                score,
                match_kind: MatchKind::NameSubstring,
                snippet,
                author: owned_str(&doc, self.f_author),
                doc_title: owned_str(&doc, self.f_doc_title),
                tags: owned_str(&doc, self.f_tags),
            });
        }
    }

    /// Body-prefix (or title) snippet for hits with no highlightable body
    /// match. tantivy's to_html escapes; this fallback must too, or the
    /// "HTML-escaped snippet" contract breaks on files containing <, >, & or "
    /// (common in indexed source code).
    fn fallback_snippet(&self, doc: &TantivyDocument) -> String {
        let body = owned_str(doc, self.f_body);
        let prefix: String = body.chars().take(200).collect();
        let fallback = if prefix.trim().is_empty() {
            owned_str(doc, self.f_title)
                .chars()
                .take(200)
                .collect::<String>()
        } else {
            prefix
        };
        html_escape(&fallback)
    }

    /// Number of documents visible to a fresh reader.
    pub fn doc_count(&self) -> u64 {
        self.index_reader()
            .map(|r| r.searcher().num_docs())
            .unwrap_or(0)
    }
}

// ── Root scoping ──────────────────────────────────────────────────────────────

/// Over-fetch factor applied when a root scope will be enforced post-hoc.
/// A scope like "just this one folder" inside a 200k-doc index can easily have
/// every one of the top-`limit` BM25 hits land outside it, so asking tantivy
/// for exactly `limit` docs and THEN filtering returns a near-empty page even
/// though plenty of in-scope matches exist further down the ranking.
const ROOT_SCOPE_OVERFETCH: usize = 20;

/// Ceiling on the over-fetch so a pathological query can't pull most of the
/// index into a TopDocs heap. A scope narrow enough that 20× `limit` docs
/// still misses it will under-report rather than stall the UI.
const ROOT_SCOPE_MAX_FETCH: usize = 5_000;

/// How many docs to ask tantivy for, given the caller's own `base`
/// (`limit + offset`) and the root scope that will be applied afterwards.
///
/// Unscoped searches fetch exactly `base` — byte-for-byte the pre-scoping
/// behaviour. Scoped searches widen the fetch (see `ROOT_SCOPE_OVERFETCH`) and
/// never shrink below `base`.
///
/// KT: the result is clamped to at least 1 — `TopDocs::with_limit(0)` panics,
/// and a `limit: 0` query used to reach it.
pub fn scoped_fetch(base: usize, roots: &[PathBuf]) -> usize {
    if roots.is_empty() {
        return base.max(1);
    }
    base.saturating_mul(ROOT_SCOPE_OVERFETCH)
        .min(ROOT_SCOPE_MAX_FETCH)
        .max(base)
        .max(1)
}

/// Whether an indexed `path` is one of `roots` or lives underneath one.
///
/// Empty `roots` means "unscoped" and always matches, so callers can pass the
/// query's roots through unconditionally.
///
/// Matching is deliberately NOT `str::starts_with` on the raw strings:
///
/// * Case-insensitive — Windows paths are, and the stored path keeps whatever
///   casing the crawl saw while the scope comes from the UI's current folder.
/// * Component-anchored — `C:\Foo` must match `C:\Foo\bar.txt` but NOT
///   `C:\Foobar\x.txt` or `C:\Foo.txt`. KT: a plain substring/prefix scope is
///   exactly how the Everything backend leaked results from `wincommander`
///   into `wincommander-pro`; do not reintroduce it here.
/// * Separator- and trailing-separator-agnostic (`C:/Foo`, `C:\Foo\`).
pub fn path_in_roots(path: &str, roots: &[PathBuf]) -> bool {
    if roots.is_empty() {
        return true;
    }
    let candidate = norm_scope_path(path);
    roots.iter().any(|root| {
        let root = norm_scope_path(&root.to_string_lossy());
        // A root that normalises away (empty string, or nothing but
        // separators) is treated as matching NOTHING rather than everything —
        // a degenerate scope must never silently widen into "whole index".
        if root.is_empty() || !candidate.starts_with(&root) {
            return false;
        }
        // Equal length = the root itself; otherwise the next character must be
        // a separator, which is what makes this component- rather than
        // substring-anchored.
        candidate.len() == root.len() || candidate.as_bytes()[root.len()] == b'\\'
    })
}

/// Canonical form for scope comparison: lowercased, forward slashes folded to
/// backslashes, any `\\?\` verbatim prefix removed (`\\?\UNC\srv\s` becomes
/// `\\srv\s`), and trailing separators trimmed.
///
/// The verbatim strip matters because `Path::canonicalize` hands back `\\?\`
/// paths on Windows: a scope derived from a canonicalized folder would
/// otherwise match none of the crawler's plainly-spelled stored paths.
fn norm_scope_path(raw: &str) -> String {
    let lowered = raw.to_lowercase().replace('/', "\\");
    let body = match lowered.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix("unc\\") {
            Some(share) => format!(r"\\{share}"),
            None => rest.to_owned(),
        },
        None => lowered,
    };
    body.trim_end_matches('\\').to_owned()
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Whether `text` contains a `-` outside any double-quoted phrase. tantivy's
/// grammar treats a bare `-term` (including mid-word, as in `budget-report`)
/// as an exclusion clause; a `-` inside a quoted phrase is a literal
/// character, not an operator, so quoted hyphens don't count.
fn has_unquoted_hyphen(text: &str) -> bool {
    let mut in_quotes = false;
    for c in text.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            '-' if !in_quotes => return true,
            _ => {}
        }
    }
    false
}

/// Replace every character tantivy's query grammar treats specially with a
/// space, so any user input (a Windows path, an email, `C++`, a stray quote)
/// degrades to a plain bag-of-terms query that always parses.
fn sanitize_query(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '+' | '-' | '&' | '|' | '!' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '"' | '~'
            | '*' | '?' | ':' | '\\' | '/' | '`' => ' ',
            other => other,
        })
        .collect()
}

/// Whether a substring-rung hit's stored metadata satisfies parsed filters
/// (post-hoc — that rung has no tantivy query to attach Musts to).
fn hit_passes_filters(filters: &QueryFilters, ext: &str, mtime: u64, size: u64) -> bool {
    if !filters.exts.is_empty() && !filters.exts.iter().any(|e| e == ext) {
        return false;
    }
    if filters.after.is_some_and(|after| mtime < after) {
        return false;
    }
    if filters.before.is_some_and(|before| mtime >= before) {
        return false;
    }
    if filters.size_min.is_some_and(|min| size < min) {
        return false;
    }
    if filters.size_max.is_some_and(|max| size >= max) {
        return false;
    }
    true
}

/// Escape a literal for tantivy's regex dialect (regex-syntax): backslash
/// every ASCII punctuation char — always a valid literal escape there —
/// and pass everything else (alphanumerics, unicode) through untouched.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Delete everything inside `dir` but keep the dir itself, so hardened ACLs
/// set on it (and inherited by future files) survive a schema migration.
fn wipe_dir_contents(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Escape the 5 HTML-significant characters so a fallback snippet (raw file
/// text) honours the same "HTML-escaped" contract as tantivy's `to_html()`.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// Extract a `String` from the first value of a stored text field.
fn owned_str(doc: &TantivyDocument, field: tantivy::schema::Field) -> String {
    match doc.get_first(field).and_then(|value| value.as_str()) {
        Some(s) => s.to_owned(),
        _ => String::new(),
    }
}

/// Extract a `u64` from the first value of a stored numeric field.
fn owned_u64(doc: &TantivyDocument, field: tantivy::schema::Field) -> u64 {
    doc.get_first(field)
        .and_then(|value| value.as_u64())
        .unwrap_or_default()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentQuery, DocProps, FileMeta};
    use tempfile::TempDir;

    fn make_meta(doc_id: u64, name: &str) -> FileMeta {
        FileMeta {
            doc_id,
            path: std::path::PathBuf::from(format!("C:\\docs\\{}", name)),
            name: name.to_owned(),
            ext: "txt".into(),
            mtime: 1_700_000_000,
            size: 64,
        }
    }

    /// Like `make_meta` but with caller-controlled ext/mtime/size — needed by
    /// the `ext:`/`after:`/`before:`/`size:` filter tests.
    fn make_meta_full(doc_id: u64, name: &str, ext: &str, mtime: u64, size: u64) -> FileMeta {
        FileMeta {
            doc_id,
            path: std::path::PathBuf::from(format!("C:\\docs\\{}", name)),
            name: name.to_owned(),
            ext: ext.to_owned(),
            mtime,
            size,
        }
    }

    /// Like `make_meta` but with a caller-chosen parent directory — the stored
    /// `path` field is what root scoping matches against.
    fn make_meta_in(doc_id: u64, dir: &str, name: &str) -> FileMeta {
        FileMeta {
            doc_id,
            path: std::path::PathBuf::from(format!("{dir}\\{name}")),
            name: name.to_owned(),
            ext: "txt".into(),
            mtime: 1_700_000_000,
            size: 64,
        }
    }

    fn make_props(author: &str, doc_title: &str, tags: &str) -> DocProps {
        DocProps {
            author: author.to_owned(),
            doc_title: doc_title.to_owned(),
            tags: tags.to_owned(),
        }
    }

    fn make_query(terms: &str) -> ContentQuery {
        ContentQuery {
            terms: terms.to_owned(),
            roots: vec![],
            limit: 10,
            offset: 0,
            keyword_only: true,
        }
    }

    #[test]
    fn open_or_create_round_trip() {
        let dir = TempDir::new().unwrap();
        // First open creates the index.
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        // Second open reuses it.
        let _ci2 = ContentIndex::open_or_create(dir.path()).unwrap();
        assert_eq!(ci.doc_count(), 0);
    }

    #[test]
    fn upsert_and_search_keyword_in_body() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();

        // doc 1 — body contains "xylophone"
        ci.upsert(
            &mut w,
            &make_meta(1, "alpha.txt"),
            "Alpha",
            "a document about xylophone music",
            &DocProps::default(),
        )
        .unwrap();
        // doc 2 — no mention of xylophone
        ci.upsert(
            &mut w,
            &make_meta(2, "beta.txt"),
            "Beta",
            "completely unrelated content here",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("xylophone")).unwrap();
        assert_eq!(hits.len(), 1, "only one doc matches xylophone");
        assert_eq!(hits[0].doc_id, 1);
        // Snippet must be non-empty (body stored and tokenised).
        assert!(!hits[0].snippet.is_empty(), "snippet should be populated");
    }

    #[test]
    fn delete_removes_doc_from_results() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();

        // Phase 1: insert and commit, then release the writer.
        {
            let mut w = ci.writer().unwrap();
            ci.upsert(
                &mut w,
                &make_meta(10, "del.txt"),
                "Delete me",
                "unique_token_zephyr",
                &DocProps::default(),
            )
            .unwrap();
            ci.commit(&mut w).unwrap();
        } // w dropped here — releases the index lock

        // Confirm visible.
        let before = ci
            .search_keyword(&make_query("unique_token_zephyr"))
            .unwrap();
        assert_eq!(before.len(), 1);

        // Phase 2: delete then commit.
        {
            let mut w2 = ci.writer().unwrap();
            ci.delete(&mut w2, 10);
            ci.commit(&mut w2).unwrap();
        }

        let after = ci
            .search_keyword(&make_query("unique_token_zephyr"))
            .unwrap();
        assert!(after.is_empty(), "deleted doc must not appear in results");
    }

    #[test]
    fn doc_count_reflects_indexed_state() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        assert_eq!(ci.doc_count(), 0);

        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "a.txt"),
            "T1",
            "body one",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta(2, "b.txt"),
            "T2",
            "body two",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        assert_eq!(ci.doc_count(), 2);
    }

    #[test]
    fn snippet_fallback_when_query_term_in_title_only() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();

        // "quuxzorp" is only in the title, NOT in the body — SnippetGenerator
        // (built over f_body) would return an empty snippet without the fallback.
        ci.upsert(
            &mut w,
            &make_meta(42, "quuxzorp_report.txt"),
            "quuxzorp annual report",
            "This body contains only generic words with no special term.",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("quuxzorp")).unwrap();
        assert_eq!(hits.len(), 1, "quuxzorp should match via title");
        assert!(
            !hits[0].snippet.is_empty(),
            "snippet must be non-empty even when query term appears only in title"
        );
    }

    #[test]
    fn upsert_overwrites_existing_doc() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();

        // Phase 1: insert initial version.
        {
            let mut w = ci.writer().unwrap();
            ci.upsert(
                &mut w,
                &make_meta(99, "upd.txt"),
                "Old",
                "initial_content_word",
                &DocProps::default(),
            )
            .unwrap();
            ci.commit(&mut w).unwrap();
        } // w dropped — releases lock

        // Phase 2: overwrite with new content.
        {
            let mut w2 = ci.writer().unwrap();
            ci.upsert(
                &mut w2,
                &make_meta(99, "upd.txt"),
                "New",
                "updated_content_word",
                &DocProps::default(),
            )
            .unwrap();
            ci.commit(&mut w2).unwrap();
        }

        // Old term gone.
        let old_hits = ci
            .search_keyword(&make_query("initial_content_word"))
            .unwrap();
        assert!(
            old_hits.is_empty(),
            "stale version should be gone after upsert"
        );

        // New term present.
        let new_hits = ci
            .search_keyword(&make_query("updated_content_word"))
            .unwrap();
        assert_eq!(new_hits.len(), 1);
    }

    #[test]
    fn search_never_errors_on_query_metacharacters() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "notes.txt"),
            "Notes",
            "the quarterly budget report is ready",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        // Each of these breaks tantivy's QueryParser; none may return Err.
        for q in [
            r"C:\Users\me",
            "foo:bar",
            "\"unclosed",
            "a AND",
            "(oops",
            "C++",
            "report -",
            "*wild",
            "back`tick",
            "",
            "   ",
        ] {
            let r = ci.search_keyword(&make_query(q));
            assert!(r.is_ok(), "query {q:?} must not error: {:?}", r.err());
        }

        // A path-shaped query still finds the doc via its plain terms.
        let hits = ci.search_keyword(&make_query(r"C:\path\budget")).unwrap();
        assert!(
            !hits.is_empty(),
            "sanitized path query should match 'budget'"
        );

        // A backtick-wrapped term still degrades to a plain-terms match.
        let bt = ci.search_keyword(&make_query("`budget`")).unwrap();
        assert!(!bt.is_empty(), "backtick query should still match 'budget'");
    }

    #[test]
    fn body_match_snippet_uses_mark_highlight() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "a.txt"),
            "A",
            "the quarterly budget report is final",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("budget")).unwrap();
        assert_eq!(hits.len(), 1);
        let snip = &hits[0].snippet;
        assert!(
            snip.contains("<mark>budget</mark>"),
            "body match must be wrapped in <mark>: {snip}"
        );
        assert!(
            !snip.contains("<b>"),
            "must not emit tantivy's default <b> wrapper: {snip}"
        );
    }

    // ── Partial-token / substring matching (the "2982" bug) ─────────────────

    #[test]
    fn partial_token_matches_filename() {
        // The original bug: a file named XXXXXXXX2982.txt was findable by
        // "XXXXXXXX2982" but NOT by "2982" — the default tokenizer kept the
        // whole run as one atomic token.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "XXXXXXXX2982.txt"),
            "XXXXXXXX2982.txt",
            "unrelated body words",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        for q in ["2982", "XXXXXXXX2982", "xxxxxxxx2982", "XXXXXXXX"] {
            let hits = ci.search_keyword(&make_query(q)).unwrap();
            assert_eq!(hits.len(), 1, "query {q:?} must find XXXXXXXX2982.txt");
            assert_eq!(hits[0].doc_id, 1);
        }
    }

    #[test]
    fn partial_token_matches_body_content() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "invoices.txt"),
            "invoices",
            "payment reference XXXXXXXX2982 settled in full",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("2982")).unwrap();
        assert_eq!(hits.len(), 1, "bare ID must match inside body text");
        assert!(
            hits[0].snippet.contains("<mark>2982</mark>"),
            "the matched ID segment must be highlighted: {}",
            hits[0].snippet
        );
    }

    #[test]
    fn digit_prefix_matches_long_id_inside_body_content() {
        // The reported bug: an enrollment number "23002170110112" embedded in
        // a document's body has no alpha↔digit boundary anywhere in it, so
        // the whole 14-digit run indexed as ONE atomic token — a query for
        // its first 8 digits ("23002170") found nothing, even though the
        // document plainly contains it.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "marksheet.pdf"),
            "Examination Form",
            "Enrollment No. 23002170110112 Contact No. 7990383176",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        for q in ["23002170110112", "23002170", "2300"] {
            let hits = ci.search_keyword(&make_query(q)).unwrap();
            assert_eq!(
                hits.len(),
                1,
                "query {q:?} must find the enrollment number: {hits:?}"
            );
            assert_eq!(hits[0].doc_id, 1);
            assert_eq!(
                hits[0].match_kind,
                MatchKind::Keyword,
                "digit-prefix hits are ordinary BM25 term matches, not the NameSubstring rung"
            );
        }
    }

    #[test]
    fn digit_prefix_too_short_does_not_match() {
        // MIN_PREFIX_LEN is 4 — a 3-digit query is too unselective to be
        // indexed as its own token and must not match.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "marksheet.pdf"),
            "Examination Form",
            "Enrollment No. 23002170110112",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("230")).unwrap();
        assert!(hits.is_empty(), "a 3-digit prefix must not match: {hits:?}");
    }

    #[test]
    fn mid_token_substring_matches_filename_via_name_substring() {
        // "982" crosses no token boundary (token is "2982") — only the
        // substring rung can find it, and it must be tagged as such.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "XXXXXXXX2982.pdf"),
            "XXXXXXXX2982.pdf",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta(2, "other.txt"),
            "other",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("982")).unwrap();
        assert_eq!(hits.len(), 1, "mid-token substring must match the name");
        assert_eq!(hits[0].doc_id, 1);
        assert_eq!(hits[0].match_kind, MatchKind::NameSubstring);

        // Case-insensitive too.
        let hits = ci.search_keyword(&make_query("xxx29")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, 1);
    }

    #[test]
    fn name_substring_rung_plain_needle_finds_accented_name() {
        // "café" appears only mid-token (one alphabetic run, folded to one
        // token by `wc_code`) in this filename, so BM25 can't match it as a
        // whole token — only the substring rung (regex over `name_lc`) can.
        // Before folding `name_lc` too, a plain "cafe" needle never matched
        // an accented stored name.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "prefixcaf\u{00e9}suffix.pdf"),
            "prefixcaf\u{00e9}suffix.pdf",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("cafe")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "plain needle must match the accented stored name: {hits:?}"
        );
        assert_eq!(hits[0].doc_id, 1);
        assert_eq!(hits[0].match_kind, MatchKind::NameSubstring);
    }

    #[test]
    fn name_substring_rung_accented_needle_finds_plain_name() {
        // Mirror of the above: an accented "café" needle must fold down to
        // "cafe" before the regex scan, or it would never match a stored
        // plain-ASCII name.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "prefixcafesuffix.pdf"),
            "prefixcafesuffix.pdf",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("caf\u{00e9}")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "accented needle must match the plain stored name: {hits:?}"
        );
        assert_eq!(hits[0].doc_id, 1);
        assert_eq!(hits[0].match_kind, MatchKind::NameSubstring);
    }

    #[test]
    fn substring_rung_skipped_when_offset_nonzero() {
        // The substring rung always regex-scans from the top of the
        // name_lc term dictionary — it has no concept of "page 2". A
        // paginated (offset != 0) request must not re-append page-1 hits.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "XXXXXXXX2982.pdf"),
            "XXXXXXXX2982.pdf",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let mut q = make_query("982");
        q.offset = 1;
        let hits = ci.search_keyword(&q).unwrap();
        assert!(
            hits.is_empty(),
            "offset != 0 must skip the substring rung, not repeat page-1 hits: {hits:?}"
        );
    }

    #[test]
    fn substring_rung_dedupes_against_keyword_hits() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        // "2982" matches doc 1 via BOTH the tokenized name and the name
        // substring — it must appear exactly once, as the keyword hit.
        ci.upsert(
            &mut w,
            &make_meta(1, "XXXXXXXX2982.txt"),
            "XXXXXXXX2982.txt",
            "body",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("2982")).unwrap();
        assert_eq!(hits.len(), 1, "no duplicate row for the same doc");
        assert_eq!(hits[0].match_kind, MatchKind::Keyword);
    }

    #[test]
    fn substring_rung_skipped_for_multi_word_queries() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "abc2982.txt"),
            "abc2982.txt",
            "body",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        // Multi-word queries never reach the substring rung; regex metachars
        // in a single word are escaped literally, never interpreted.
        let hits = ci.search_keyword(&make_query("zz yy xx")).unwrap();
        assert!(hits.is_empty());
        let hits = ci.search_keyword(&make_query("c++")).unwrap();
        assert!(hits.is_empty(), "escaped 'c++' matches nothing here");
        // ".*" escaped is a literal dot-star, not match-all.
        let hits = ci.search_keyword(&make_query(".*")).unwrap();
        assert!(hits.is_empty(), "'.*' must not become match-everything");
        // A dotted query still matches where it really occurs (as a token
        // phrase or a name substring — either rung may win).
        let hits = ci.search_keyword(&make_query("2982.txt")).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn hyphenated_query_falls_back_to_sanitized_terms() {
        // "budget-report" parses as `budget AND NOT report` and matches
        // nothing; the zero-hit retry must degrade it to plain terms.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "finance.txt"),
            "finance",
            "the annual budget report is final",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("budget-report")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "hyphenated query must degrade to plain terms instead of excluding"
        );
    }

    #[test]
    fn hyphenated_query_merges_sanitized_hits_even_when_raw_parse_is_non_empty() {
        // "budget-report" parses as `budget AND NOT report`. With a SECOND doc
        // that contains "budget" but not "report", the raw parse returns a
        // non-empty hit set (that doc alone) — so the old zero-hit-only retry
        // never fires, and the doc containing BOTH words stays silently
        // excluded. The merge must surface both docs.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "finance.txt"),
            "finance",
            "the annual budget report is final",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta(2, "other.txt"),
            "other",
            "budget planning notes for next year",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("budget-report")).unwrap();
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert!(
            names.contains(&"finance.txt"),
            "doc containing BOTH words must not stay excluded by the NOT-interpretation: {names:?}"
        );
        assert!(
            names.contains(&"other.txt"),
            "doc matching the raw NOT-interpretation must still be present: {names:?}"
        );
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn schema_version_mismatch_wipes_and_recreates() {
        let dir = TempDir::new().unwrap();
        {
            let ci = ContentIndex::open_or_create(dir.path()).unwrap();
            let mut w = ci.writer().unwrap();
            ci.upsert(
                &mut w,
                &make_meta(1, "keep.txt"),
                "keep",
                "some body",
                &DocProps::default(),
            )
            .unwrap();
            ci.commit(&mut w).unwrap();
            assert_eq!(ci.doc_count(), 1);
        }
        // Simulate an index written by an older schema generation.
        std::fs::write(dir.path().join(super::SCHEMA_VERSION_FILE), "1").unwrap();

        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        assert_eq!(ci.doc_count(), 0, "stale-generation index must be wiped");
        // And the marker is refreshed so the next open does NOT wipe again.
        let marker = std::fs::read_to_string(dir.path().join(super::SCHEMA_VERSION_FILE)).unwrap();
        assert_eq!(marker.trim(), super::SCHEMA_VERSION);
        // Still writable after migration.
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(2, "new.txt"),
            "new",
            "fresh body",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();
        assert_eq!(ci.doc_count(), 1);
    }

    // ── Diacritic / ligature folding (the "finds some words but not others" bug) ──

    #[test]
    fn ligature_extracted_text_is_findable_by_plain_query() {
        // PDF extractors emit ligature codepoints: "office" comes out as
        // "o\u{FB03}ce" (ﬃ). Without ASCII folding, a plain "office" query
        // never matches text that only ever contains the ligature form.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "memo.txt"),
            "Memo",
            "the ne\u{FB03} o\u{FB03}ce memo",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("office")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "'office' must find the ligature-encoded body"
        );
        assert_eq!(hits[0].doc_id, 1);
    }

    #[test]
    fn accented_body_findable_by_both_bare_and_accented_query() {
        // NFC "café" must be findable by the folded ASCII query "cafe" AND by
        // the accented query "café" itself (both fold to the same token).
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "plan.txt"),
            "Plan",
            "visit the caf\u{E9} tomorrow",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("cafe")).unwrap();
        assert_eq!(hits.len(), 1, "bare 'cafe' must match accented body text");
        assert_eq!(hits[0].doc_id, 1);

        let hits = ci.search_keyword(&make_query("caf\u{E9}")).unwrap();
        assert_eq!(hits.len(), 1, "accented 'café' query must also match");
        assert_eq!(hits[0].doc_id, 1);
    }

    #[test]
    fn fallback_snippet_is_html_escaped() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        // "titlonlytoken" is only in the title, so the raw-body-prefix fallback
        // runs; the body has HTML-significant chars that MUST be escaped.
        ci.upsert(
            &mut w,
            &make_meta(7, "code.rs"),
            "titlonlytoken header",
            "sample: Vec<T> where a && b and \"quoted\" text",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("titlonlytoken")).unwrap();
        assert_eq!(hits.len(), 1);
        let snip = &hits[0].snippet;
        assert!(!snip.contains('<'), "no raw '<' allowed in snippet: {snip}");
        assert!(
            snip.contains("&lt;") && snip.contains("&gt;"),
            "angle brackets must be escaped: {snip}"
        );
        assert!(snip.contains("&amp;"), "ampersand must be escaped: {snip}");
    }

    // ── Metadata fields (author/tags/doc_title) ─────────────────────────────

    #[test]
    fn author_field_and_plain_query_both_match_author() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "a.txt"),
            "A",
            "body one",
            &make_props("Jane Doe", "", ""),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta(2, "b.txt"),
            "B",
            "body two",
            &make_props("John Smith", "", ""),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("author:jane")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "author: field query must match only Jane's doc"
        );
        assert_eq!(hits[0].doc_id, 1);

        let hits = ci.search_keyword(&make_query("jane")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "plain query must also match author via default fields"
        );
        assert_eq!(hits[0].doc_id, 1);
    }

    #[test]
    fn tags_and_doc_title_field_queries_match() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "a.txt"),
            "A",
            "body",
            &make_props("", "Annual Roadmap", "budget keywords"),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta(2, "b.txt"),
            "B",
            "body",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("tags:budget")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "tags: field query must match the keyworded doc"
        );
        assert_eq!(hits[0].doc_id, 1);

        let hits = ci.search_keyword(&make_query("doc_title:roadmap")).unwrap();
        assert_eq!(hits.len(), 1, "doc_title: field query must match");
        assert_eq!(hits[0].doc_id, 1);
    }

    #[test]
    fn content_hit_carries_author_field_for_ui_display() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta(1, "a.txt"),
            "A",
            "body",
            &make_props("Jane Quill", "", ""),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("author:quill")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].author, "Jane Quill",
            "ContentHit.author must surface why the doc matched"
        );
    }

    // ── ext:/after:/before:/size: filters ───────────────────────────────────

    #[test]
    fn ext_filter_limits_results_to_matching_extension() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(1, "a.pdf", "pdf", 1_700_000_000, 64),
            "A",
            "shared_marker_word",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(2, "b.txt", "txt", 1_700_000_000, 64),
            "B",
            "shared_marker_word",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci
            .search_keyword(&make_query("shared_marker_word ext:pdf"))
            .unwrap();
        assert_eq!(hits.len(), 1, "ext: filter must exclude the .txt doc");
        assert_eq!(hits[0].doc_id, 1);
    }

    #[test]
    fn after_and_before_filters_bound_mtime() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        // 1_767_225_600 == 2026-01-01T00:00:00Z (see filters.rs::after_date_vectors).
        let boundary = 1_767_225_600;
        ci.upsert(
            &mut w,
            &make_meta_full(1, "old.txt", "txt", boundary - 1, 64),
            "old",
            "report content",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(2, "new.txt", "txt", boundary, 64),
            "new",
            "report content",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci
            .search_keyword(&make_query("report after:2026-01"))
            .unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.doc_id).collect();
        assert_eq!(ids, vec![2], "after: is inclusive of the boundary instant");

        let hits = ci
            .search_keyword(&make_query("report before:2026-01"))
            .unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.doc_id).collect();
        assert_eq!(ids, vec![1], "before: excludes the boundary instant");
    }

    #[test]
    fn size_filters_bound_byte_size() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(1, "small.txt", "txt", 1_700_000_000, 500),
            "small",
            "report content",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(2, "big.txt", "txt", 1_700_000_000, 5000),
            "big",
            "report content",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("report size:>1000")).unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.doc_id).collect();
        assert_eq!(ids, vec![2], "size:> must exclude the small doc");

        let hits = ci.search_keyword(&make_query("report size:<1000")).unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.doc_id).collect();
        assert_eq!(ids, vec![1], "size:< must exclude the big doc");
    }

    #[test]
    fn filters_only_query_returns_matching_docs_with_no_text() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(1, "a.txt", "txt", 1_700_000_000, 64),
            "A",
            "alpha body",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(2, "b.pdf", "pdf", 1_700_000_000, 64),
            "B",
            "beta body",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci.search_keyword(&make_query("ext:txt")).unwrap();
        assert_eq!(hits.len(), 1, "a filters-only query is legal");
        assert_eq!(hits[0].doc_id, 1);
    }

    #[test]
    fn substring_rung_hit_respects_ext_filter() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(1, "XXXXXXXX2982.pdf", "pdf", 1_700_000_000, 64),
            "XXXXXXXX2982.pdf",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(2, "XXXXXXXX2982.txt", "txt", 1_700_000_000, 64),
            "XXXXXXXX2982.txt",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        // "982" crosses no token boundary — only the substring rung finds it;
        // the ext: filter must still apply to that rung's hits.
        let hits = ci.search_keyword(&make_query("982 ext:pdf")).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "ext: filter must apply to substring-rung hits too"
        );
        assert_eq!(hits[0].doc_id, 1);
        assert_eq!(hits[0].match_kind, MatchKind::NameSubstring);
    }

    #[test]
    fn combined_text_ext_and_after_filters() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        let boundary = 1_767_225_600; // 2026-01-01T00:00:00Z
        ci.upsert(
            &mut w,
            &make_meta_full(1, "old.pdf", "pdf", boundary - 1, 64),
            "old",
            "quarterly budget report",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(2, "new.pdf", "pdf", boundary, 64),
            "new",
            "quarterly budget report",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_full(3, "new.txt", "txt", boundary, 64),
            "new",
            "quarterly budget report",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci
            .search_keyword(&make_query("budget ext:pdf after:2026"))
            .unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.doc_id).collect();
        assert_eq!(
            ids,
            vec![2],
            "must match text AND ext AND mtime lower bound together"
        );
    }

    #[test]
    fn filter_clauses_are_score_neutral_membership_constraints() {
        // Regression: an `ext:` Must clause used to score via its own BM25
        // IDF, so a rarer extension across the corpus (1 pdf doc vs 4 txt
        // docs here) earned a bigger un-boosted score contribution than the
        // common one — outranking an otherwise text-identical match purely
        // because of the filter, not the text query.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        for i in 0..4u64 {
            ci.upsert(
                &mut w,
                &make_meta_full(i, &format!("t{i}.txt"), "txt", 1_700_000_000, 64),
                "T",
                "identical_word",
                &DocProps::default(),
            )
            .unwrap();
        }
        ci.upsert(
            &mut w,
            &make_meta_full(99, "rare.pdf", "pdf", 1_700_000_000, 64),
            "T",
            "identical_word",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let hits = ci
            .search_keyword(&make_query("identical_word ext:pdf,txt"))
            .unwrap();
        assert_eq!(
            hits.len(),
            5,
            "ext: filter must still admit membership for both extensions"
        );
        let scores: Vec<f32> = hits.iter().map(|h| h.score).collect();
        let first = scores[0];
        assert!(
            scores.iter().all(|&s| (s - first).abs() < 1e-6),
            "zero-boosted filter clauses must not perturb ranking via their own IDF: {scores:?}"
        );
    }

    // ── Root scoping: path_in_roots ─────────────────────────────────────────

    fn roots(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn empty_roots_is_unscoped() {
        // The whole-index default must stay reachable — every caller that has
        // no folder scope passes an empty vec.
        assert!(path_in_roots(r"C:\anywhere\at\all.txt", &[]));
        assert!(path_in_roots("", &[]));
    }

    #[test]
    fn root_matches_itself_and_nested_paths() {
        let r = roots(&[r"C:\Foo"]);
        assert!(path_in_roots(r"C:\Foo", &r), "the root itself is in scope");
        assert!(path_in_roots(r"C:\Foo\bar.txt", &r), "direct child");
        assert!(
            path_in_roots(r"C:\Foo\deep\deeper\bar.txt", &r),
            "arbitrarily nested descendant"
        );
    }

    #[test]
    fn sibling_prefix_folder_is_not_in_scope() {
        // The exact bug class found in the Everything backend: a substring
        // scope let `wincommander` leak into `wincommander-pro`.
        let r = roots(&[r"C:\Foo"]);
        assert!(
            !path_in_roots(r"C:\Foobar\x.txt", &r),
            "C:\\Foobar is a SIBLING of C:\\Foo, not a child"
        );
        assert!(
            !path_in_roots(r"C:\Foo.txt", &r),
            "a file whose name merely starts with the root is not under it"
        );
        let r = roots(&[r"C:\repos\wincommander"]);
        assert!(!path_in_roots(r"C:\repos\wincommander-pro\src\a.rs", &r));
        assert!(path_in_roots(r"C:\repos\wincommander\src\a.rs", &r));
    }

    #[test]
    fn scope_matching_is_case_insensitive() {
        // Windows paths are case-insensitive; the stored path keeps the crawl's
        // casing while the scope comes from the UI.
        let r = roots(&[r"c:\foo\bar"]);
        assert!(path_in_roots(r"C:\FOO\BAR\Baz.TXT", &r));
        let r = roots(&[r"C:\Users\Admin\Documents"]);
        assert!(path_in_roots(r"c:\users\admin\documents\notes.txt", &r));
    }

    #[test]
    fn trailing_separator_on_root_is_tolerated() {
        for root in [r"C:\Foo\", r"C:\Foo\\", "C:/Foo/"] {
            let r = roots(&[root]);
            assert!(
                path_in_roots(r"C:\Foo\bar.txt", &r),
                "root {root:?} must still scope to C:\\Foo\\bar.txt"
            );
            assert!(
                !path_in_roots(r"C:\Foobar\x.txt", &r),
                "root {root:?} must not leak into the sibling folder"
            );
        }
    }

    #[test]
    fn mixed_separators_match_on_both_sides() {
        let r = roots(&["C:/Foo/Bar"]);
        assert!(path_in_roots(r"C:\Foo\Bar\baz.txt", &r));
        let r = roots(&[r"C:\Foo\Bar"]);
        assert!(path_in_roots("C:/Foo/Bar/baz.txt", &r));
        assert!(path_in_roots(r"C:/Foo\Bar/baz.txt", &r));
    }

    #[test]
    fn verbatim_and_unc_roots_normalize() {
        // `Path::canonicalize` returns `\\?\`-prefixed paths; a scope derived
        // from one must still match the crawler's plainly-spelled stored paths.
        let r = roots(&[r"\\?\C:\Foo"]);
        assert!(path_in_roots(r"C:\Foo\bar.txt", &r));
        let r = roots(&[r"C:\Foo"]);
        assert!(path_in_roots(r"\\?\C:\Foo\bar.txt", &r));
        // UNC verbatim collapses back to the \\server\share spelling.
        let r = roots(&[r"\\?\UNC\srv\share\dir"]);
        assert!(path_in_roots(r"\\srv\share\dir\file.txt", &r));
        assert!(!path_in_roots(r"\\srv\share\dir2\file.txt", &r));
    }

    #[test]
    fn unrelated_paths_are_out_of_scope() {
        let r = roots(&[r"C:\Foo\Bar"]);
        assert!(!path_in_roots(r"D:\Foo\Bar\x.txt", &r), "different drive");
        assert!(
            !path_in_roots(r"C:\Foo\x.txt", &r),
            "the PARENT of the root is not inside it"
        );
        assert!(!path_in_roots(r"C:\Other\Bar\x.txt", &r));
    }

    #[test]
    fn multiple_roots_are_an_or() {
        let r = roots(&[r"C:\Foo", r"D:\Work\Reports"]);
        assert!(path_in_roots(r"C:\Foo\a.txt", &r), "matches the first root");
        assert!(
            path_in_roots(r"D:\Work\Reports\q3\b.pdf", &r),
            "matches the second root"
        );
        assert!(
            !path_in_roots(r"D:\Work\Other\b.pdf", &r),
            "matching NO root is out of scope"
        );
    }

    #[test]
    fn degenerate_root_matches_nothing_rather_than_everything() {
        // A root that normalises to an empty string must not silently widen
        // back into "whole index" — that would be a scope leak.
        for root in ["", "\\", "/", "//"] {
            let r = roots(&[root]);
            assert!(
                !path_in_roots(r"C:\Foo\bar.txt", &r),
                "degenerate root {root:?} must not match everything"
            );
        }
    }

    // ── Root scoping: scoped_fetch ──────────────────────────────────────────

    #[test]
    fn scoped_fetch_only_overfetches_when_scoped() {
        // Unscoped must be byte-for-byte the old behaviour.
        assert_eq!(scoped_fetch(50, &[]), 50);
        assert_eq!(scoped_fetch(1, &[]), 1);
        // Scoped over-fetches, capped, and never shrinks below `base`.
        let r = roots(&[r"C:\Foo"]);
        assert_eq!(scoped_fetch(50, &r), 50 * ROOT_SCOPE_OVERFETCH);
        assert_eq!(
            scoped_fetch(100_000, &r),
            100_000,
            "cap must not shrink base"
        );
        assert_eq!(scoped_fetch(1_000, &r), ROOT_SCOPE_MAX_FETCH);
        // TopDocs::with_limit(0) panics — never hand it a zero.
        assert_eq!(scoped_fetch(0, &[]), 1);
        assert_eq!(scoped_fetch(0, &r), 1);
    }

    // ── Root scoping through search_keyword ─────────────────────────────────

    fn scoped_query(terms: &str, scope: &[&str], limit: usize) -> ContentQuery {
        ContentQuery {
            terms: terms.to_owned(),
            roots: roots(scope),
            limit,
            offset: 0,
            keyword_only: true,
        }
    }

    /// Index four docs across `alpha`, the sibling-prefix `alpha-extra`, `beta`,
    /// and a nested folder under `alpha`.
    fn index_scope_corpus(ci: &ContentIndex) {
        let mut w = ci.writer().unwrap();
        for (id, dir, name) in [
            (1, r"C:\projects\alpha", "a.txt"),
            (2, r"C:\projects\alpha-extra", "b.txt"),
            (3, r"C:\projects\beta", "c.txt"),
            (4, r"C:\projects\alpha\sub\deep", "d.txt"),
        ] {
            ci.upsert(
                &mut w,
                &make_meta_in(id, dir, name),
                "T",
                "shared_scope_marker in every doc",
                &DocProps::default(),
            )
            .unwrap();
        }
        ci.commit(&mut w).unwrap();
    }

    #[test]
    fn keyword_search_honours_root_scope() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        index_scope_corpus(&ci);

        // Unscoped — the pre-existing behaviour, all four docs.
        let all = ci
            .search_keyword(&make_query("shared_scope_marker"))
            .unwrap();
        assert_eq!(all.len(), 4, "empty roots must still search everything");

        // Scoped to C:\projects\alpha — the nested doc is in, the
        // sibling-prefix folder and the unrelated folder are out.
        let hits = ci
            .search_keyword(&scoped_query(
                "shared_scope_marker",
                &[r"C:\projects\alpha"],
                10,
            ))
            .unwrap();
        let mut ids: Vec<u64> = hits.iter().map(|h| h.doc_id).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![1, 4],
            "scope must admit the folder + descendants only: {:?}",
            hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn keyword_search_scope_accepts_mixed_and_trailing_separators() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        index_scope_corpus(&ci);

        for scope in [
            r"C:\projects\alpha\",
            "C:/projects/alpha",
            "c:/PROJECTS/Alpha/",
        ] {
            let hits = ci
                .search_keyword(&scoped_query("shared_scope_marker", &[scope], 10))
                .unwrap();
            assert_eq!(hits.len(), 2, "scope {scope:?} should match alpha + nested");
        }
    }

    #[test]
    fn keyword_search_multiple_roots_union() {
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        index_scope_corpus(&ci);

        let hits = ci
            .search_keyword(&scoped_query(
                "shared_scope_marker",
                &[r"C:\projects\beta", r"C:\projects\alpha-extra"],
                10,
            ))
            .unwrap();
        let mut ids: Vec<u64> = hits.iter().map(|h| h.doc_id).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![2, 3]);
    }

    #[test]
    fn substring_rung_hits_honour_root_scope() {
        // "982" crosses no token boundary, so only the NameSubstring rung finds
        // these — that rung must respect the scope too, or the "Inside files"
        // section would show out-of-scope rows.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        ci.upsert(
            &mut w,
            &make_meta_in(1, r"C:\projects\alpha", "XXXXXXXX2982.txt"),
            "T",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.upsert(
            &mut w,
            &make_meta_in(2, r"C:\projects\alpha-extra", "XXXXXXXX2982.txt"),
            "T",
            "body text",
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        let unscoped = ci.search_keyword(&make_query("982")).unwrap();
        assert_eq!(unscoped.len(), 2, "both docs match the substring rung");

        let hits = ci
            .search_keyword(&scoped_query("982", &[r"C:\projects\alpha"], 10))
            .unwrap();
        assert_eq!(hits.len(), 1, "scope must drop the sibling-prefix folder");
        assert_eq!(hits[0].doc_id, 1);
        assert_eq!(hits[0].match_kind, MatchKind::NameSubstring);
    }

    #[test]
    fn scoped_search_overfetches_so_low_ranked_in_scope_hits_survive() {
        // Regression guard for the "filter after the limit" bug class: the ONE
        // in-scope doc scores below every out-of-scope doc (long body → small
        // BM25 contribution), so fetching exactly `limit` docs and filtering
        // afterwards would return an empty page.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        for i in 0..60u64 {
            ci.upsert(
                &mut w,
                &make_meta_in(i, r"C:\out", &format!("o{i}.txt")),
                "T",
                "overfetchmarker",
                &DocProps::default(),
            )
            .unwrap();
        }
        let long_body = format!("{} overfetchmarker", "filler ".repeat(400));
        ci.upsert(
            &mut w,
            &make_meta_in(999, r"C:\in", "deep.txt"),
            "T",
            &long_body,
            &DocProps::default(),
        )
        .unwrap();
        ci.commit(&mut w).unwrap();

        // Precondition: the in-scope doc really is below the unscoped top 5.
        let mut top5 = make_query("overfetchmarker");
        top5.limit = 5;
        let top5 = ci.search_keyword(&top5).unwrap();
        assert_eq!(top5.len(), 5);
        assert!(
            !top5.iter().any(|h| h.doc_id == 999),
            "test is only meaningful if the in-scope doc ranks below the cut"
        );

        let hits = ci
            .search_keyword(&scoped_query("overfetchmarker", &[r"C:\in"], 5))
            .unwrap();
        assert_eq!(
            hits.len(),
            1,
            "scoping must not truncate the result set to near-zero: {hits:?}"
        );
        assert_eq!(hits[0].doc_id, 999);
    }

    #[test]
    fn scoped_offset_pages_through_in_scope_hits_only() {
        // `offset` must count rows that SURVIVE the scope; if it skipped raw
        // ranking positions instead, page 2 of a scoped search would come back
        // empty while in-scope matches were still pending.
        let dir = TempDir::new().unwrap();
        let ci = ContentIndex::open_or_create(dir.path()).unwrap();
        let mut w = ci.writer().unwrap();
        for i in 0..20u64 {
            ci.upsert(
                &mut w,
                &make_meta_in(i, r"C:\out", &format!("o{i}.txt")),
                "T",
                "pagemarker body",
                &DocProps::default(),
            )
            .unwrap();
        }
        for i in 100..103u64 {
            ci.upsert(
                &mut w,
                &make_meta_in(i, r"C:\in", &format!("i{i}.txt")),
                "T",
                "pagemarker body",
                &DocProps::default(),
            )
            .unwrap();
        }
        ci.commit(&mut w).unwrap();

        let page1 = ci
            .search_keyword(&scoped_query("pagemarker", &[r"C:\in"], 2))
            .unwrap();
        assert_eq!(page1.len(), 2, "first scoped page must be full");

        let mut q2 = scoped_query("pagemarker", &[r"C:\in"], 2);
        q2.offset = 2;
        let page2 = ci.search_keyword(&q2).unwrap();
        assert_eq!(page2.len(), 1, "third in-scope doc must appear on page 2");

        let mut ids: Vec<u64> = page1.iter().chain(page2.iter()).map(|h| h.doc_id).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![100, 101, 102],
            "the two pages must partition the in-scope docs with no overlap"
        );
    }
}
