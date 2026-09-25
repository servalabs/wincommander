// SPDX-License-Identifier: AGPL-3.0-or-later
//! Route ordinary and private content search into separate storage lifetimes.
mod defaults;
mod global;
mod locks;
mod privacy;
mod private_index;
mod private_jobs;
mod status;

use crate::settings::{read_settings, AppSettings, FileSearchSettings};
use defaults::ensure_initialized;
use locks::IndexReplacementLock;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use wincmd_search::types::{Chunk, ContentHit, ContentQuery, IndexConfig, IndexStatus};

pub fn fts_index_dir() -> Result<PathBuf, String> {
    Ok(crate::paths::user_data_dir()?
        .join("file-search")
        .join("fts"))
}

pub(crate) fn decoy_skip_paths() -> Vec<PathBuf> {
    crate::file_monitor::enrolled_decoy_paths()
}

fn prepare(initialize: bool) -> Result<(AppSettings, privacy::SearchPlan), String> {
    static PREPARE: OnceLock<Mutex<()>> = OnceLock::new();
    let _lock = PREPARE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Search policy is unavailable.")?;
    let mut settings = read_settings()?;
    if initialize {
        settings = ensure_initialized(settings)?;
    }
    privacy::enroll(&mut settings)?;
    let plan = privacy::plan(&settings.app.file_search)?;
    global::publish_scope(
        &fts_index_dir()?,
        &plan.ordinary,
        &settings.app.file_search.exclusions,
        &plan.protected,
    )?;
    Ok((settings, plan))
}

fn build_index_config(fs: &FileSearchSettings, roots: Vec<PathBuf>) -> Result<IndexConfig, String> {
    let mut exclusions = fs.exclusions.clone();
    exclusions.push(".wincommander".into());
    Ok(IndexConfig {
        roots,
        exclusions,
        skip_paths: decoy_skip_paths(),
        max_file_bytes: 50 * 1024 * 1024,
        index_dir: fts_index_dir()?,
    })
}

fn build_content_query(
    terms: String,
    roots: Vec<PathBuf>,
    limit: Option<usize>,
    offset: Option<usize>,
    keyword_only: Option<bool>,
) -> ContentQuery {
    ContentQuery {
        terms,
        roots,
        limit: limit.unwrap_or(50).min(12_000),
        offset: offset.unwrap_or(0).min(10_000),
        keyword_only: keyword_only.unwrap_or(true),
    }
}

fn validate_content_scope_path(scope: &str) -> Result<String, String> {
    let trimmed = scope.trim();
    if trimmed.is_empty() {
        return Err("Search folder is empty.".into());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("Search folder contains control characters.".into());
    }
    Ok(trimmed.to_string())
}

fn resolve_content_roots(
    scope: Option<String>,
    roots: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    let scope = scope.map(|s| validate_content_scope_path(&s)).transpose()?;
    Ok(privacy::scope_roots(scope.as_deref(), &roots))
}

fn unchanged(generation: &str) -> Result<(), String> {
    let (_, current) = prepare(false)?;
    if current.generation != generation {
        return Err("Search volumes changed; retry the search.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn search_content(
    terms: String,
    limit: Option<usize>,
    offset: Option<usize>,
    keyword_only: Option<bool>,
    scope_path: Option<String>,
) -> Result<Vec<ContentHit>, String> {
    tokio::task::spawn_blocking(move || {
        let (settings, plan) = prepare(true)?;
        let fs = &settings.app.file_search;
        let limit = limit.unwrap_or(50).min(2_000);
        let offset = offset.unwrap_or(0).min(10_000);
        let mut hits = Vec::new();
        let roots = resolve_content_roots(scope_path.clone(), plan.ordinary.clone())?;
        if !roots.is_empty() {
            let query = build_content_query(
                terms.clone(),
                roots,
                Some(limit + offset),
                Some(0),
                keyword_only,
            );
            hits = global::with_engine(
                build_index_config(fs, plan.ordinary.clone())?,
                &plan.protected,
                |engine| engine.search_restricted(&query).map_err(|e| e.to_string()),
            )?;
        }
        for shard in &plan.private {
            let roots = resolve_content_roots(scope_path.clone(), shard.roots.clone())?;
            if roots.is_empty() {
                continue;
            }
            let query = build_content_query(
                terms.clone(),
                roots,
                Some(limit + offset),
                Some(0),
                keyword_only,
            );
            hits.extend(private_index::search(
                shard,
                &settings.device_id,
                &fs.exclusions,
                &query,
            )?);
        }
        private_jobs::refresh(&plan, &settings.device_id, &fs.exclusions, false);
        unchanged(&plan.generation)?;
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.path.cmp(&b.path)));
        let mut seen = std::collections::HashSet::new();
        hits.retain(|h| seen.insert(h.path.to_lowercase()));
        Ok(hits.into_iter().skip(offset).take(limit).collect())
    })
    .await
    .map_err(|_| "Content search worker failed.".to_string())?
}

#[tauri::command]
pub async fn content_index_status() -> Result<IndexStatus, String> {
    tokio::task::spawn_blocking(move || {
        let (settings, plan) = prepare(true)?;
        let fs = &settings.app.file_search;
        let mut status = global::with_engine(
            build_index_config(fs, plan.ordinary.clone())?,
            &plan.protected,
            |engine| Ok(engine.status()),
        )?;
        private_jobs::refresh(&plan, &settings.device_id, &fs.exclusions, false);
        for shard in &plan.private {
            let (state, _, count) = private_jobs::state(shard);
            status.indexed_docs += count;
            status.is_indexing |= state == "indexing";
        }
        Ok(status)
    })
    .await
    .map_err(|_| "Index status worker failed.".to_string())?
}

#[tauri::command]
pub async fn content_privacy_status() -> Result<status::PrivacyStatus, String> {
    tokio::task::spawn_blocking(status::snapshot)
        .await
        .map_err(|_| "Search privacy status failed.".to_string())?
}

#[tauri::command]
pub async fn content_index_configure(
    roots: Vec<PathBuf>,
    exclusions: Vec<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _replacement_lock = IndexReplacementLock::acquire()?;
        wincmd_search::crawler::build_globset(&exclusions).map_err(|e| e.to_string())?;
        if roots.len() > 64 || exclusions.len() > 256 {
            return Err("Too many indexed folders or exclusions.".into());
        }
        for root in &roots {
            if !root.is_absolute() {
                return Err("Indexed folders must use absolute paths.".into());
            }
        }
        private_jobs::cancel_all();
        global::stop()?;
        crate::settings::patch_settings(serde_json::json!({"app": {"fileSearch": {
            "roots": roots, "exclusions": exclusions, "initialized": true
        }}}))?;
        let (settings, plan) = prepare(false)?;
        let fs = &settings.app.file_search;
        global::with_engine(
            build_index_config(fs, plan.ordinary.clone())?,
            &plan.protected,
            |_| Ok(()),
        )?;
        private_jobs::refresh(&plan, &settings.device_id, &fs.exclusions, true);
        Ok(())
    })
    .await
    .map_err(|_| "Index configuration worker failed.".to_string())?
}

#[tauri::command]
pub async fn content_rescan() -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _replacement_lock = IndexReplacementLock::acquire()?;
        global::stop()?;
        let (settings, plan) = prepare(true)?;
        let fs = &settings.app.file_search;
        global::with_engine(
            build_index_config(fs, plan.ordinary.clone())?,
            &plan.protected,
            |_| Ok(()),
        )?;
        private_jobs::refresh(&plan, &settings.device_id, &fs.exclusions, true);
        Ok(())
    })
    .await
    .map_err(|_| "Index rescan worker failed.".to_string())?
}

#[tauri::command]
pub async fn content_reindex() -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _replacement_lock = IndexReplacementLock::acquire()?;
        private_jobs::cancel_all();
        global::stop()?;
        let (settings, plan) = prepare(true)?;
        let dir = fts_index_dir()?;
        global::remove(&dir)?;
        for shard in &plan.private {
            private_index::rebuild(shard, &settings.device_id)?;
        }
        let fs = &settings.app.file_search;
        global::with_engine(
            build_index_config(fs, plan.ordinary.clone())?,
            &plan.protected,
            |_| Ok(()),
        )?;
        private_jobs::refresh(&plan, &settings.device_id, &fs.exclusions, true);
        Ok(())
    })
    .await
    .map_err(|_| "Index rebuild worker failed.".to_string())?
}

#[tauri::command]
pub async fn content_get_doc(doc_id: String) -> Result<Vec<Chunk>, String> {
    tokio::task::spawn_blocking(move || {
        let id: u64 = doc_id.parse().map_err(|_| "Invalid document identity.")?;
        let (settings, plan) = prepare(false)?;
        let fs = &settings.app.file_search;
        let mut chunks = Vec::new();
        if !plan.ordinary.is_empty() {
            chunks = global::with_engine(
                build_index_config(fs, plan.ordinary.clone())?,
                &plan.protected,
                |engine| engine.get_chunks_restricted(id).map_err(|e| e.to_string()),
            )?;
        }
        if chunks.is_empty() {
            for shard in &plan.private {
                chunks = private_index::chunks(shard, &settings.device_id, &fs.exclusions, id)?;
                if !chunks.is_empty() {
                    break;
                }
            }
        }
        unchanged(&plan.generation)?;
        Ok(chunks)
    })
    .await
    .map_err(|_| "Content preview worker failed.".to_string())?
}

pub(crate) fn protected_search_policy() -> Result<(Vec<PathBuf>, String), String> {
    let (_, plan) = prepare(false)?;
    Ok((plan.protected, plan.generation))
}

#[cfg(test)]
include!("file_search/tests.rs");
