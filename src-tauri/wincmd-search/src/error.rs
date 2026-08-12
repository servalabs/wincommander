use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Index error: {0}")]
    Index(#[from] tantivy::TantivyError),

    #[error("Extraction error: {0}")]
    Extract(String),

    /// A file whose extension has no content extractor (.db, .exe, images, …).
    /// The indexer treats this as a SILENT SKIP, never a surfaced error.
    #[error("unsupported extension: {0}")]
    Unsupported(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

impl SearchError {
    /// True for files we deliberately don't content-index (no extractor) — these
    /// are skipped during indexing rather than reported as `last_error`.
    pub fn is_unsupported(&self) -> bool {
        matches!(self, SearchError::Unsupported(_))
    }

    /// True for a single file that failed extraction and should not poison the
    /// global index status. The crawl continues and real engine/index failures
    /// still surface through `last_error`.
    pub fn is_per_file_skip(&self) -> bool {
        matches!(self, SearchError::Unsupported(_) | SearchError::Extract(_))
    }
}

pub type Result<T> = std::result::Result<T, SearchError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn extract_error_message() {
        let e = SearchError::Extract("bad format".into());
        assert!(e.to_string().contains("bad format"));
    }

    #[test]
    fn config_error_message() {
        let e = SearchError::Config("missing index_dir".into());
        assert!(e.to_string().contains("missing index_dir"));
    }

    #[test]
    fn io_error_converts() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let search_err: SearchError = io_err.into();
        assert!(search_err.to_string().contains("I/O error"));
    }

    #[test]
    fn extraction_errors_are_per_file_skips() {
        assert!(SearchError::Unsupported("exe".into()).is_per_file_skip());
        assert!(SearchError::Extract("bad pdf".into()).is_per_file_skip());
        assert!(!SearchError::Config("bad roots".into()).is_per_file_skip());
    }
}
