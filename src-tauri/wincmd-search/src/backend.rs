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
