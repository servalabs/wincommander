// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental original-user installer phases. These derive every path from
//! the user's Windows Known Folder; callers cannot supply a filesystem path.
use wincmd_search::{SearchEngine, encrypted_search_fence::SearchOperationFence};

/// Run from the temporary early CLI helper, before normal application startup.
/// The verify phase is read-only and the installer must hold its exclusive lock.
pub fn run(phase: &str) -> Result<(), String> {
    let parent = crate::file_search::runtime_fence::verified_parent()?;
    let old = parent.join("fts");
    let selection = parent.join(".wincommander-encrypted-selection");
    match phase {
        "quarantine" => {
            let guard = SearchOperationFence::acquire_exclusive(&parent)
                .map_err(|_| "Cannot acquire offline search maintenance lock.")?;
            SearchEngine::quarantine_selected_with_fence(&old, &selection, &guard)
                .map(|_| ()).map_err(|e| e.to_string())
        }
        "verify" => SearchEngine::verify_quarantined_selection(&old, &selection)
            .map_err(|e| e.to_string()),
        _ => Err("Invalid search migration phase.".into()),
    }
}
