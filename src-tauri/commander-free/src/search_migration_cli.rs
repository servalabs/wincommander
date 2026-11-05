// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental original-user installer phases. These derive every path from
//! the user's Windows Known Folder; callers cannot supply a filesystem path.
use std::{io, path::Path};
use wincmd_search::{SearchEngine, encrypted_search_fence::SearchOperationFence};

#[derive(Debug, PartialEq, Eq)]
enum State { Fresh, NeedsQuarantine, Quarantined }
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
        (true, true) => Ok(State::NeedsQuarantine),
        (false, true) => Ok(State::Quarantined),
        (true, false) => Err("The encrypted search selection is missing; the old index was retained.".into()),
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
pub fn run(phase: &str) -> Result<(), String> {
    let parent = if phase == "quarantine" {
        crate::file_search::runtime_fence::prepare_installer_parent()?
    } else {
        crate::file_search::runtime_fence::verified_parent()?
    };
    let old = parent.join("fts");
    let selection = parent.join(".wincommander-encrypted-selection");
    match phase {
        "quarantine" => {
            let guard = SearchOperationFence::acquire_exclusive(&parent)
                .map_err(|_| "Cannot acquire offline search maintenance lock.")?;
            match classify(&parent, &old, &selection)? {
                State::Fresh => Ok(()),
                State::NeedsQuarantine => SearchEngine::quarantine_selected_with_fence(&old, &selection, &guard)
                    .map(|_| ()).map_err(|e| e.to_string()),
                State::Quarantined => SearchEngine::verify_quarantined_selection(&old, &selection)
                    .map_err(|e| e.to_string()),
            }
        }
        "verify" => match classify(&parent, &old, &selection)? {
            State::Fresh => Ok(()),
            State::Quarantined => SearchEngine::verify_quarantined_selection(&old, &selection)
                .map_err(|e| e.to_string()),
            State::NeedsQuarantine => Err("The old search index is still present.".into()),
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
        assert!(classify(root.path(), &old, &selection).is_err());
        std::fs::create_dir(&selection).unwrap();
        assert_eq!(classify(root.path(), &old, &selection).unwrap(), State::NeedsQuarantine);
        std::fs::remove_dir(&old).unwrap();
        assert_eq!(classify(root.path(), &old, &selection).unwrap(), State::Quarantined);
    }
}
