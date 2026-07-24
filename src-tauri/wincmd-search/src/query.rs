// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hit-merge helper for blending keyword and semantic results.

use crate::types::{ContentHit, DocId, MatchKind};
use std::collections::HashMap;

/// Merge keyword hits and optional semantic hits into a single ranked list.
///
/// Normalises each set to [0, 1], blends scores (0.6 keyword + 0.4 semantic),
/// deduplicates by `DocId`, sorts descending, and truncates to `limit`.
pub fn merge_hits(
    keyword_hits: Vec<ContentHit>,
    semantic_hits: Vec<ContentHit>,
    limit: usize,
) -> Vec<ContentHit> {
    let normalize = |hits: &[ContentHit]| -> HashMap<DocId, f32> {
        let max = hits.iter().map(|h| h.score).fold(0.0_f32, f32::max);
        if max == 0.0 {
            return hits.iter().map(|h| (h.doc_id, 0.0)).collect();
        }
        hits.iter().map(|h| (h.doc_id, h.score / max)).collect()
    };

    let kw_norm = normalize(&keyword_hits);
    let sem_norm = normalize(&semantic_hits);

    let mut merged: HashMap<DocId, ContentHit> = HashMap::new();

    for mut hit in keyword_hits {
        let ks = kw_norm.get(&hit.doc_id).copied().unwrap_or(0.0);
        let ss = sem_norm.get(&hit.doc_id).copied().unwrap_or(0.0);
        hit.score = 0.6 * ks + 0.4 * ss;
        hit.match_kind = if sem_norm.contains_key(&hit.doc_id) {
            MatchKind::Hybrid
        } else {
            MatchKind::Keyword
        };
        merged.insert(hit.doc_id, hit);
    }

    for mut hit in semantic_hits {
        merged.entry(hit.doc_id).or_insert_with(|| {
            let ss = sem_norm.get(&hit.doc_id).copied().unwrap_or(0.0);
            hit.score = 0.4 * ss;
            hit.match_kind = MatchKind::Semantic;
            hit
        });
    }

    let mut out: Vec<ContentHit> = merged.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentHit, MatchKind};

    fn hit(doc_id: u64, score: f32) -> ContentHit {
        ContentHit {
            doc_id,
            path: format!("C:\\docs\\doc{}.txt", doc_id),
            name: format!("doc{}.txt", doc_id),
            ext: "txt".into(),
            mtime: 0,
            size: 0,
            score,
            match_kind: MatchKind::Keyword,
            snippet: String::new(),
            author: String::new(),
            doc_title: String::new(),
            tags: String::new(),
        }
    }

    // ── keyword-only ─────────────────────────────────────────────────────

    #[test]
    fn keyword_only_sorted_desc_and_truncated() {
        let kw = vec![hit(1, 0.5), hit(2, 1.0), hit(3, 0.3)];
        let result = merge_hits(kw, vec![], 2);
        assert_eq!(result.len(), 2);
        // After normalisation doc2 = 0.6, doc1 = 0.3 — top 2 are doc2, doc1.
        assert_eq!(result[0].doc_id, 2);
        assert_eq!(result[1].doc_id, 1);
        assert!(result.iter().all(|h| h.match_kind == MatchKind::Keyword));
    }

    // ── semantic-only ────────────────────────────────────────────────────

    #[test]
    fn semantic_only_has_semantic_kind() {
        let sem = vec![hit(10, 0.9), hit(11, 0.4)];
        let result = merge_hits(vec![], sem, 10);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|h| h.match_kind == MatchKind::Semantic));
        // Top result is doc 10.
        assert_eq!(result[0].doc_id, 10);
    }

    // ── dedup: doc in both → Hybrid ───────────────────────────────────────

    #[test]
    fn overlap_becomes_hybrid() {
        let kw = vec![hit(5, 1.0), hit(6, 0.5)];
        let sem = vec![hit(5, 0.8), hit(7, 0.6)];
        let result = merge_hits(kw, sem, 10);
        // doc 5 is in both → Hybrid; doc 6 keyword-only; doc 7 semantic-only.
        let doc5 = result.iter().find(|h| h.doc_id == 5).unwrap();
        let doc6 = result.iter().find(|h| h.doc_id == 6).unwrap();
        let doc7 = result.iter().find(|h| h.doc_id == 7).unwrap();
        assert_eq!(doc5.match_kind, MatchKind::Hybrid);
        assert_eq!(doc6.match_kind, MatchKind::Keyword);
        assert_eq!(doc7.match_kind, MatchKind::Semantic);
    }

    // ── score ordering ────────────────────────────────────────────────────

    #[test]
    fn result_is_sorted_by_blended_score_desc() {
        let kw = vec![hit(1, 1.0)];
        let sem = vec![hit(1, 1.0), hit(2, 0.1)];
        let result = merge_hits(kw, sem, 10);
        // doc1 gets 0.6+0.4 = 1.0; doc2 gets 0.4*0.1/1.0 = 0.04 → doc1 first.
        assert_eq!(result[0].doc_id, 1);
    }

    // ── limit enforced ───────────────────────────────────────────────────

    #[test]
    fn limit_enforced() {
        let kw = (0..20).map(|i| hit(i, i as f32)).collect::<Vec<_>>();
        let result = merge_hits(kw, vec![], 5);
        assert_eq!(result.len(), 5);
    }

    // ── Hybrid when semantic score is exactly 0.0 ────────────────────────

    #[test]
    fn overlap_with_zero_semantic_score_is_hybrid() {
        // doc 20 is in both keyword and semantic inputs, but its semantic score
        // is 0.0.  Under the old `ss > 0.0` guard it would have been mis-tagged
        // Keyword.  Membership detection must tag it Hybrid regardless.
        let kw = vec![hit(20, 1.0), hit(21, 0.5)];
        // All semantic scores are 0.0 — normalize produces a 0/0 → all map to 0.0.
        let sem = vec![hit(20, 0.0)];
        let result = merge_hits(kw, sem, 10);

        let doc20 = result.iter().find(|h| h.doc_id == 20).unwrap();
        assert_eq!(
            doc20.match_kind,
            MatchKind::Hybrid,
            "doc present in both sets must be Hybrid even when semantic score is 0.0"
        );
        let doc21 = result.iter().find(|h| h.doc_id == 21).unwrap();
        assert_eq!(doc21.match_kind, MatchKind::Keyword);
    }

    // ── empty inputs ─────────────────────────────────────────────────────

    #[test]
    fn both_empty_returns_empty() {
        let result = merge_hits(vec![], vec![], 10);
        assert!(result.is_empty());
    }
}
