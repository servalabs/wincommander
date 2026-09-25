// SPDX-License-Identifier: AGPL-3.0-or-later
//! Private indexes have no resident readers, watchers, or writers on the volume.

use super::privacy::PrivateShard;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};
use wincmd_search::{
    types::{Chunk, ContentHit, ContentQuery, IndexConfig},
    SearchEngine,
};
use wincmd_volume::{inspect_path, VolumeOperationGuard};

pub(super) fn index_dir(shard: &PrivateShard, device: &str) -> PathBuf {
    let device = hex::encode(Sha256::digest(device.as_bytes()));
    shard
        .volume
        .root
        .join(".wincommander")
        .join("search")
        .join(device)
        .join("fts")
}

fn verify(shard: &PrivateShard) -> Result<(), String> {
    let actual = inspect_path(&shard.volume.root)?;
    if actual.identity != shard.volume.identity || !actual.is_private {
        return Err("The private volume changed or was locked.".into());
    }
    Ok(())
}

fn config(shard: &PrivateShard, device: &str, exclusions: &[String]) -> IndexConfig {
    let mut skip = super::decoy_skip_paths();
    skip.push(shard.volume.root.join(".wincommander"));
    IndexConfig {
        roots: shard.roots.clone(),
        exclusions: exclusions.to_vec(),
        skip_paths: skip,
        max_file_bytes: 50 * 1024 * 1024,
        index_dir: index_dir(shard, device),
    }
}

fn policy(shard: &PrivateShard) -> wincmd_search::PathPolicy {
    let identity = shard.volume.identity.clone();
    Arc::new(move |path| inspect_path(path).is_ok_and(|v| v.is_private && v.identity == identity))
}

fn ensure_directory(shard: &PrivateShard, directory: &Path) -> Result<(), String> {
    verify(shard)?;
    let relative = directory
        .strip_prefix(&shard.volume.root)
        .map_err(|_| "Invalid private index path.")?;
    let mut current = shard.volume.root.clone();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("Invalid private index path.".into());
        }
        current.push(component);
        match std::fs::create_dir(&current) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                return Err(
                    "Cannot create the index inside this volume; no external fallback was used."
                        .into(),
                )
            }
        }
        let actual = inspect_path(&current)?;
        if actual.identity != shard.volume.identity {
            return Err("Private index location escaped its volume.".into());
        }
    }
    Ok(())
}

pub(super) fn reconcile(
    shard: &PrivateShard,
    device: &str,
    exclusions: &[String],
    cancel: &AtomicBool,
) -> Result<(u64, bool), String> {
    let _operation = VolumeOperationGuard::acquire(&shard.volume)?;
    verify(shard)?;
    let config = config(shard, device, exclusions);
    if shard.volume.read_only {
        if !config.index_dir.join("meta.json").is_file() {
            return Err("No private index exists on this read-only volume.".into());
        }
        let engine = SearchEngine::open_existing_with_policy(config, policy(shard))
            .map_err(|_| "The read-only private index is unavailable.")?;
        let count = engine
            .indexed_document_count()
            .map_err(|_| "Cannot read the private index count.")?;
        verify(shard)?;
        return Ok((count, true));
    }
    ensure_directory(shard, &config.index_dir)?;
    let engine = SearchEngine::open_with_policy(config, policy(shard))
        .map_err(|_| "Private index could not be opened.")?;
    let report = engine
        .reconcile_sync(cancel, 128)
        .map_err(|_| "Private index update failed; retry or rebuild it.")?;
    verify(shard)?;
    Ok((report.indexed_docs, report.complete))
}

pub(super) fn search(
    shard: &PrivateShard,
    device: &str,
    exclusions: &[String],
    query: &ContentQuery,
) -> Result<Vec<ContentHit>, String> {
    let _operation = VolumeOperationGuard::acquire(&shard.volume)?;
    verify(shard)?;
    let config = config(shard, device, exclusions);
    if !config.index_dir.join("meta.json").is_file() {
        return Ok(Vec::new());
    }
    if inspect_path(&config.index_dir)?.identity != shard.volume.identity {
        return Err("Private index is unavailable.".into());
    }
    let engine = SearchEngine::open_existing_with_policy(config, policy(shard))
        .map_err(|_| "Private index requires a writable rebuild.")?;
    let hits = engine
        .search_restricted(query)
        .map_err(|_| "Private search failed.")?;
    verify(shard)?;
    Ok(hits)
}

pub(super) fn chunks(
    shard: &PrivateShard,
    device: &str,
    exclusions: &[String],
    id: u64,
) -> Result<Vec<Chunk>, String> {
    let _operation = VolumeOperationGuard::acquire(&shard.volume)?;
    verify(shard)?;
    let config = config(shard, device, exclusions);
    if !config.index_dir.join("meta.json").is_file() {
        return Ok(Vec::new());
    }
    if inspect_path(&config.index_dir)?.identity != shard.volume.identity {
        return Err("Private index is unavailable.".into());
    }
    let engine = SearchEngine::open_existing_with_policy(config, policy(shard))
        .map_err(|_| "Private index requires a writable rebuild.")?;
    let chunks = engine
        .get_chunks_restricted(id)
        .map_err(|_| "Private preview is unavailable.")?;
    verify(shard)?;
    Ok(chunks)
}

pub(super) fn rebuild(shard: &PrivateShard, device: &str) -> Result<(), String> {
    let _operation = VolumeOperationGuard::acquire(&shard.volume)?;
    verify(shard)?;
    if shard.volume.read_only {
        return Err("A read-only private volume cannot be rebuilt.".into());
    }
    let dir = index_dir(shard, device);
    if dir.exists() {
        if inspect_path(&dir)?.identity != shard.volume.identity {
            return Err("Private index location changed.".into());
        }
        std::fs::remove_dir_all(&dir)
            .map_err(|_| "Private index rebuild could not remove the old index.")?;
    }
    verify(shard)
}
