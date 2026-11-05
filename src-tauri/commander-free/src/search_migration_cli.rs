// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental original-user installer phases. These derive every path from
//! the user's Windows Known Folder; callers cannot supply a filesystem path.
use std::{io, path::Path};
use wincmd_search::{SearchEngine, encrypted_search_fence::SearchOperationFence};

#[derive(Debug, PartialEq, Eq)]
enum State { Fresh, NeedsRebuild, NeedsQuarantine, Quarantined }
fn present(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
fn classify(parent: &Path, old: &Path, selection: &Path) -> Result<State, String> {
    let old_exists = present(old).map_err(|_| "Cannot inspect the old search index.")?;
    let selection_exists = present(selection).map_err(|_| "Cannot inspect the encrypted search selection.")?;
    match (old_exists, selection_exists) {
        (true, true) => {
            if present(&selection.join("candidate-proof.v1")).map_err(|_| "Cannot inspect search selection proof.")? {
                if SearchEngine::experimental_scope_only(old).map_err(|e| e.to_string())? { Ok(State::Quarantined) } else { Ok(State::NeedsQuarantine) }
            }
            else { Ok(State::NeedsRebuild) }
        },
        (false, true) => Ok(State::Quarantined),
        (true, false) => Ok(State::NeedsRebuild),
        (false, false) => {
            // Missing selector is fresh only if no evidence of an earlier
            // encrypted generation or quarantined plaintext backup remains.
            for entry in std::fs::read_dir(parent).map_err(|_| "Cannot inspect search migration state.")? {
                let name = entry.map_err(|_| "Cannot inspect search migration state.")?.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".wincommander-encrypted-") || name.starts_with(".wincommander-legacy-index-backup-") {
                    return Err("Incomplete search migration needs recovery; existing files were retained.".into());
                }
            }
            Ok(State::Fresh)
        },
    }
}

/// Run from the temporary early CLI helper, before normal application startup.
/// The verify phase is read-only and the installer must hold its exclusive lock.
pub fn run(phase: &str) -> Result<String, String> {
    let parent = crate::file_search::runtime_fence::verified_parent()?;
    let old = parent.join("fts");
    let selection = parent.join(".wincommander-encrypted-selection");
    match phase {
        "stage" => {
            let guard = SearchOperationFence::acquire_exclusive(&parent)
                .map_err(|_| "Cannot acquire offline search maintenance lock.")?;
            match classify(&parent, &old, &selection)? {
                State::Fresh => Ok("fresh".into()),
                State::NeedsRebuild => {
                    // No settings or user roots are read in the installer.
                    let config = wincmd_search::types::IndexConfig {
                        roots: Vec::new(), exclusions: Vec::new(), skip_paths: Vec::new(),
                        max_file_bytes: 50 * 1024 * 1024, index_dir: old.clone(),
                    };
                    let policy: wincmd_search::PathPolicy = std::sync::Arc::new(|_| false);
                    SearchEngine::rebuild_offline_and_select(config, policy, &selection, &guard)
                        .map_err(|e| e.to_string())?;
                    SearchEngine::verify_selected_generation(&old, &selection).map_err(|e| e.to_string())?;
                    Ok("staged reconciliation=pending old-cache=retained".into())
                },
                State::NeedsQuarantine => {
                    SearchEngine::verify_selected_generation(&old, &selection).map_err(|e| e.to_string())?;
                    Ok("staged reconciliation=pending old-cache=retained".into())
                },
                State::Quarantined => {
                    SearchEngine::verify_quarantined_selection(&old, &selection).map_err(|e| e.to_string())?;
                    Ok("already-quarantined".into())
                },
            }
        }
        "quarantine" => {
            let guard = SearchOperationFence::acquire_exclusive(&parent)
                .map_err(|_| "Cannot acquire offline search maintenance lock.")?;
            match classify(&parent, &old, &selection)? {
                State::Fresh => Ok("fresh".into()),
                State::NeedsRebuild => {
                    // No settings are read from this temporary executable: it
                    // prepares an empty cache, then the normal app reconciles
                    // using the live user's authoritative privacy plan.
                    let config = wincmd_search::types::IndexConfig {
                        roots: Vec::new(), exclusions: Vec::new(), skip_paths: Vec::new(),
                        max_file_bytes: 50 * 1024 * 1024, index_dir: old.clone(),
                    };
                    let policy: wincmd_search::PathPolicy = std::sync::Arc::new(|_| false);
                    let report = SearchEngine::rebuild_offline_and_select(config, policy, &selection, &guard)
                        .map_err(|e| e.to_string())?;
                    SearchEngine::quarantine_selected_with_fence(&old, &selection, &guard)
                        .map(|_| format!("moved indexed={} omitted={} links={} reconciliation=pending", report.indexed, report.omitted, report.skipped_links)).map_err(|e| e.to_string())
                },
                State::NeedsQuarantine => SearchEngine::quarantine_selected_with_fence(&old, &selection, &guard)
                    .map(|_| "moved".into()).map_err(|e| e.to_string()),
                State::Quarantined => SearchEngine::verify_quarantined_selection(&old, &selection)
                    .map(|_| "already-quarantined".into()).map_err(|e| e.to_string()),
            }
        }
        "restore" => {
            let guard = SearchOperationFence::acquire_exclusive(&parent)
                .map_err(|_| "Cannot acquire offline search maintenance lock.")?;
            SearchEngine::restore_quarantined_with_fence(&old, &selection, &guard)
                .map(|_| "restored".into()).map_err(|e| e.to_string())
        }
        "verify" => match classify(&parent, &old, &selection)? {
            State::Fresh => Ok("fresh".into()),
            State::Quarantined => SearchEngine::verify_quarantined_selection(&old, &selection)
                .map(|_| "verified".into()).map_err(|e| e.to_string()),
            State::NeedsRebuild | State::NeedsQuarantine => Err("The old search index is still present.".into()),
        },
        _ => Err("Invalid search migration phase.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_requires_no_legacy_selection_or_recovery_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("fts");
        let selection = root.path().join(".wincommander-encrypted-selection");
        assert_eq!(classify(root.path(), &old, &selection).unwrap(), State::Fresh);
        std::fs::create_dir(root.path().join(".wincommander-encrypted-interrupted")).unwrap();
        assert!(classify(root.path(), &old, &selection).is_err());
        std::fs::remove_dir(root.path().join(".wincommander-encrypted-interrupted")).unwrap();
        std::fs::create_dir(&old).unwrap();
        assert_eq!(classify(root.path(), &old, &selection).unwrap(), State::NeedsRebuild);
        std::fs::create_dir(&selection).unwrap();
        assert_eq!(classify(root.path(), &old, &selection).unwrap(), State::NeedsRebuild);
        std::fs::write(selection.join("candidate-proof.v1"), b"not-a-valid-proof").unwrap();
        assert_eq!(classify(root.path(), &old, &selection).unwrap(), State::NeedsQuarantine);
        std::fs::remove_dir(&old).unwrap();
        assert_eq!(classify(root.path(), &old, &selection).unwrap(), State::Quarantined);
    }
}

/// Transactional installer entrypoints. The installer persists the random
/// 16-byte token it receives; on restart `inspect` reports pending state.
/// Marking replacement before handing the fence to NSIS is conservative:
/// after that boundary an interrupted installer must roll forward.
pub fn run_transaction(phase: &str, token_hex: &str) -> Result<String, String> {
    let parent = crate::file_search::runtime_fence::verified_parent()?;
    let old = parent.join("fts");
    let selection = parent.join(".wincommander-encrypted-selection");
    if phase == "inspect" {
        if !present(&selection).map_err(|_| "Cannot inspect search selection.")? {
            return Ok("none".into());
        }
        return Ok(match SearchEngine::inspect_search_migration(&selection).map_err(|e| e.to_string())? {
            None => "none".into(),
            Some((token, false)) => format!("intent {}", hex::encode(token)),
            Some((token, true)) => format!("replacement-pending {}", hex::encode(token)),
        });
    }
    let raw = hex::decode(token_hex).map_err(|_| "Invalid search migration transaction.")?;
    let token: [u8; 16] = raw.try_into().map_err(|_| "Invalid search migration transaction.")?;
    if token == [0; 16] { return Err("Invalid search migration transaction.".into()); }
    let guard = SearchOperationFence::acquire_exclusive(&parent)
        .map_err(|_| "Cannot acquire offline search maintenance lock.")?;
    match phase {
        "prepare" => {
            match classify(&parent, &old, &selection)? {
                State::Fresh => return Ok("fresh".into()),
                State::NeedsRebuild => {
                    let config = wincmd_search::types::IndexConfig {
                        roots: Vec::new(), exclusions: Vec::new(), skip_paths: Vec::new(),
                        max_file_bytes: 50 * 1024 * 1024, index_dir: old.clone(),
                    };
                    let policy: wincmd_search::PathPolicy = std::sync::Arc::new(|_| false);
                    SearchEngine::rebuild_offline_and_select(config, policy, &selection, &guard)
                        .map_err(|e| e.to_string())?;
                },
                State::NeedsQuarantine => {},
                State::Quarantined => {
                    match SearchEngine::inspect_search_migration(&selection).map_err(|e| e.to_string())? {
                        Some((existing, true)) if existing == token => {
                            SearchEngine::mark_search_replacement(&old, &selection, token, &guard).map_err(|e| e.to_string())?;
                            return Ok("replacement-pending reconciliation=pending".into());
                        },
                        None => {
                            SearchEngine::verify_quarantined_selection(&old, &selection).map_err(|e| e.to_string())?;
                            return Ok("already-quarantined".into());
                        },
                        _ => {},
                    }
                },
            }
            SearchEngine::begin_search_migration(&old, &selection, token, &guard).map_err(|e| e.to_string())?;
            SearchEngine::mark_search_replacement(&old, &selection, token, &guard).map_err(|e| e.to_string())?;
            Ok("replacement-pending reconciliation=pending".into())
        },
        "abort" => SearchEngine::abort_search_migration(&old, &selection, token, &guard)
            .map(|_| "restored".into()).map_err(|e| e.to_string()),
        "commit" => SearchEngine::commit_search_migration(&old, &selection, token, &guard)
            .map(|_| "committed".into()).map_err(|e| e.to_string()),
        _ => Err("Invalid search migration transaction phase.".into()),
    }
}
