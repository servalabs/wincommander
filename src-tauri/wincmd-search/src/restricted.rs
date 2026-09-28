// SPDX-License-Identifier: AGPL-3.0-or-later
use std::path::{Component, Path};

use crate::{
    crawler::build_globset,
    error::Result,
    index::path_in_roots,
    types::{Chunk, ContentHit, ContentQuery, DocId},
    SearchEngine,
};

/// Reject links and Windows reparse points in every existing path component.
pub(crate) fn safe_path(path: &Path) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    path.ancestors().all(|ancestor| {
        std::fs::symlink_metadata(ancestor)
            .map(|meta| {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return false;
                    }
                }
                !meta.file_type().is_symlink()
            })
            .unwrap_or(false)
    })
}

impl SearchEngine {
    pub(crate) fn permits_path(&self, path: &Path) -> bool {
        !self.config.roots.is_empty()
            && path_in_roots(&path.to_string_lossy(), &self.config.roots)
            && !path_in_roots(
                &path.to_string_lossy(),
                std::slice::from_ref(&self.config.index_dir),
            )
            && (self.config.skip_paths.is_empty()
                || !path_in_roots(&path.to_string_lossy(), &self.config.skip_paths))
            && build_globset(&self.config.exclusions)
                .map(|globs| !path.components().any(|c| globs.is_match(c.as_os_str())))
                .unwrap_or(false)
            && safe_path(path)
            && (self.path_policy)(path)
    }

    /// Search only currently configured, accessible roots. Empty configuration denies all.
    /// Query roots can narrow this scope, never widen it.
    pub fn search_restricted(&self, query: &ContentQuery) -> Result<Vec<ContentHit>> {
        if self.config.roots.is_empty() || query.limit == 0 {
            return Ok(vec![]);
        }
        let mut scoped = query.clone();
        scoped.roots = if query.roots.is_empty() {
            self.config.roots.clone()
        } else {
            self.config
                .roots
                .iter()
                .flat_map(|allowed| {
                    query.roots.iter().filter_map(move |requested| {
                        if path_in_roots(
                            &requested.to_string_lossy(),
                            std::slice::from_ref(allowed),
                        ) {
                            Some(requested.clone())
                        } else if path_in_roots(
                            &allowed.to_string_lossy(),
                            std::slice::from_ref(requested),
                        ) {
                            Some(allowed.clone())
                        } else {
                            None
                        }
                    })
                })
                .collect()
        };
        if scoped.roots.is_empty() {
            return Ok(vec![]);
        }
        // Stored paths retain their old mount letter. Current scopes apply after relocation.
        if self.path_mapper.is_some() {
            scoped.roots.clear();
        }
        scoped.offset = 0;
        scoped.limit = query
            .limit
            .saturating_add(query.offset)
            .saturating_mul(8)
            .min(100_000);
        loop {
            let hits = self.search(&scoped)?;
            let fetched = hits.len();
            let mut mapped_paths = std::collections::HashSet::new();
            let permitted: Vec<_> = hits
                .into_iter()
                .filter_map(|mut hit| {
                    if let Some(mapper) = &self.path_mapper {
                        hit.path = mapper(Path::new(&hit.path))?.to_string_lossy().into_owned();
                    }
                    Some(hit)
                })
                .filter(|hit| {
                    path_in_roots(&hit.path, &query.roots)
                        && self.permits_path(Path::new(&hit.path))
                        && (self.path_mapper.is_none()
                            || mapped_paths.insert(hit.path.to_lowercase().replace('/', "\\")))
                })
                .collect();
            if permitted.len() >= query.limit.saturating_add(query.offset)
                || fetched < scoped.limit
                || scoped.limit >= 100_000
            {
                return Ok(permitted
                    .into_iter()
                    .skip(query.offset)
                    .take(query.limit)
                    .collect());
            }
            // Removed or inaccessible rows must not hide lower-ranked permitted results.
            scoped.limit = scoped.limit.saturating_mul(2).min(100_000);
        }
    }

    /// Retrieve stored content only for a file still permitted by the current policy.
    pub fn get_chunks_restricted(&self, doc_id: DocId) -> Result<Vec<Chunk>> {
        if self.config.roots.is_empty() {
            return Ok(vec![]);
        }
        self.get_chunks_inner(doc_id, true)
    }
}
