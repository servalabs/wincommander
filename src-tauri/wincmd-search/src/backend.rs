use crate::error::Result;
use crate::types::{ContentHit, ContentQuery, Finding};

/// Extension point for vector/semantic search (P2+).
pub trait SemanticBackend: Send + Sync {
    fn search(&self, query: &ContentQuery, limit: usize) -> Result<Vec<ContentHit>>;
}

/// Extension point for forensic pattern scanning (P2+).
pub trait ForensicBackend: Send + Sync {
    fn scan(&self, path: &std::path::Path) -> Result<Vec<Finding>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Finding;

    /// Minimal stub that satisfies SemanticBackend — confirms trait is object-safe.
    struct NullSemantic;
    impl SemanticBackend for NullSemantic {
        fn search(&self, _query: &ContentQuery, _limit: usize) -> Result<Vec<ContentHit>> {
            Ok(vec![])
        }
    }

    /// Minimal stub that satisfies ForensicBackend.
    struct NullForensic;
    impl ForensicBackend for NullForensic {
        fn scan(&self, _path: &std::path::Path) -> Result<Vec<Finding>> {
            Ok(vec![])
        }
    }

    fn make_query() -> ContentQuery {
        ContentQuery {
            terms: "test".into(),
            roots: vec![],
            limit: 10,
            offset: 0,
            keyword_only: true,
        }
    }

    #[test]
    fn semantic_backend_returns_empty_for_stub() {
        let b: Box<dyn SemanticBackend> = Box::new(NullSemantic);
        let q = make_query();
        let results = b.search(&q, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn forensic_backend_returns_empty_for_stub() {
        let b: Box<dyn ForensicBackend> = Box::new(NullForensic);
        let findings = b.scan(std::path::Path::new("C:\\foo.txt")).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn semantic_backend_is_dyn_dispatchable() {
        // Confirms the trait is object-safe by coercing to &dyn.
        let b = NullSemantic;
        let _: &dyn SemanticBackend = &b;
    }

    #[test]
    fn forensic_backend_is_dyn_dispatchable() {
        let b = NullForensic;
        let _: &dyn ForensicBackend = &b;
    }
}
