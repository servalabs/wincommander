use serde::{Deserialize, Serialize};

/// Stable 64-bit identity for a file: FNV-1a hash of canonicalized path.
pub type DocId = u64;

/// Serde helpers: serialize/deserialize a u64 as a JSON string so JS never
/// loses precision on 64-bit IDs (JS numbers are f64 — safe only up to 2^53).
pub(crate) mod u64_str {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        s.parse::<u64>().map_err(serde::de::Error::custom)
    }
}

/// Which field a chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkField {
    Title,
    Body,
}

/// A single indexable text chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    #[serde(with = "u64_str")]
    pub doc_id: DocId,
    pub field: ChunkField,
    pub ordinal: u32,
    pub text: String,
}

/// Lightweight file metadata collected during crawl / watch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub doc_id: DocId,
    pub path: std::path::PathBuf,
    pub name: String,
    pub ext: String,
    pub mtime: u64, // UNIX seconds
    pub size: u64,  // bytes
}

/// Document-internal properties (author/title/keywords) extracted alongside body text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocProps {
    pub author: String,
    pub doc_title: String,
    /// Keywords + subject, space-joined.
    pub tags: String,
}

/// Text extracted from a file before chunking.
#[derive(Debug, Clone)]
pub struct ExtractedDoc {
    pub meta: FileMeta,
    pub title: String,
    pub body: String,
    pub props: DocProps,
}

/// Parameters for a content search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentQuery {
    pub terms: String,
    /// Reserved for P2 root-scoped query filtering.  Set from config but not
    /// applied by `search_keyword` today — all indexed roots are searched.
    pub roots: Vec<std::path::PathBuf>,
    pub limit: usize,
    pub offset: usize,
    /// BM25-only (no semantic blend) when true.
    pub keyword_only: bool,
}

/// How a document matched the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchKind {
    Keyword,
    Semantic,
    Hybrid,
    /// Query is a substring of the file name (no token boundary exposed it).
    NameSubstring,
}

/// One result row returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentHit {
    #[serde(with = "u64_str")]
    pub doc_id: DocId,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub mtime: u64,
    pub size: u64,
    pub score: f32,
    pub match_kind: MatchKind,
    /// HTML-escaped snippet with `<mark>` highlights.
    pub snippet: String,
    pub author: String,
    pub doc_title: String,
    pub tags: String,
}

/// Snapshot of indexer state, polled by the frontend.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStatus {
    pub indexed_docs: u64,
    pub pending_docs: u64,
    pub is_indexing: bool,
    pub last_error: Option<String>,
    pub index_size_bytes: u64,
}

/// Persisted configuration for the search engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    pub roots: Vec<std::path::PathBuf>,
    pub exclusions: Vec<String>, // glob patterns
    pub skip_paths: Vec<std::path::PathBuf>,
    pub max_file_bytes: u64,
    pub index_dir: std::path::PathBuf,
}

/// A forensic finding attached to a hit (P2+).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub doc_id: DocId,
    pub kind: String,
    pub description: String,
    pub severity: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_field_serializes_as_string() {
        let title = serde_json::to_string(&ChunkField::Title).unwrap();
        let body = serde_json::to_string(&ChunkField::Body).unwrap();
        assert_eq!(title, "\"Title\"");
        assert_eq!(body, "\"Body\"");
    }

    #[test]
    fn match_kind_roundtrips() {
        for kind in [
            MatchKind::Keyword,
            MatchKind::Semantic,
            MatchKind::Hybrid,
            MatchKind::NameSubstring,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let parsed: MatchKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn content_hit_roundtrips() {
        let hit = ContentHit {
            doc_id: 42,
            path: "C:\\Users\\test\\doc.txt".into(),
            name: "doc.txt".into(),
            ext: "txt".into(),
            mtime: 1_700_000_000,
            size: 1024,
            score: 0.95,
            match_kind: MatchKind::Keyword,
            snippet: "hello <mark>world</mark>".into(),
            author: String::new(),
            doc_title: String::new(),
            tags: String::new(),
        };
        let json = serde_json::to_string(&hit).unwrap();
        // doc_id must cross the wire as a JSON string, not a number.
        assert!(
            json.contains("\"doc_id\":\"42\""),
            "doc_id must serialize as a quoted string; got: {json}"
        );
        let parsed: ContentHit = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.doc_id, hit.doc_id);
        assert_eq!(parsed.match_kind, MatchKind::Keyword);
        assert_eq!(parsed.snippet, hit.snippet);
    }

    #[test]
    fn chunk_doc_id_serializes_as_string() {
        let c = Chunk {
            doc_id: 99,
            field: ChunkField::Body,
            ordinal: 0,
            text: "hello".into(),
        };
        let json = serde_json::to_string(&c).unwrap();
        // doc_id must be a quoted string, not a bare number.
        assert!(
            json.contains("\"doc_id\":\"99\""),
            "Chunk doc_id must serialize as a quoted string; got: {json}"
        );
        let parsed: Chunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.doc_id, 99);
    }

    #[test]
    fn index_status_default_is_idle() {
        let status = IndexStatus::default();
        assert!(!status.is_indexing);
        assert_eq!(status.indexed_docs, 0);
        assert!(status.last_error.is_none());
    }

    #[test]
    fn index_status_roundtrips() {
        let status = IndexStatus {
            indexed_docs: 100,
            pending_docs: 5,
            is_indexing: true,
            last_error: Some("disk full".into()),
            index_size_bytes: 1_048_576,
        };
        let json = serde_json::to_string(&status).unwrap();
        let parsed: IndexStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.indexed_docs, 100);
        assert_eq!(parsed.last_error.as_deref(), Some("disk full"));
    }

    #[test]
    fn chunk_construct() {
        let c = Chunk {
            doc_id: 1,
            field: ChunkField::Body,
            ordinal: 0,
            text: "hello world".into(),
        };
        assert_eq!(c.ordinal, 0);
        assert_eq!(c.field, ChunkField::Body);
    }

    #[test]
    fn file_meta_construct() {
        let m = FileMeta {
            doc_id: 99,
            path: std::path::PathBuf::from("C:\\foo\\bar.pdf"),
            name: "bar.pdf".into(),
            ext: "pdf".into(),
            mtime: 0,
            size: 512,
        };
        assert_eq!(m.ext, "pdf");
    }
}
