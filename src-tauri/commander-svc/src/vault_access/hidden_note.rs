// SPDX-License-Identifier: AGPL-3.0-or-later
//! Containers known to hold a hidden part, keyed by file identity (volume
//! serial plus NTFS file ID). Once a Vault is noted its decoy password alone
//! changes nothing. No verb returns this set; the path is kept only so a note
//! can be pruned or diagnosed.

use super::*;

const HIDDEN_PARTS_FILE: &str = "vault-hidden-parts-v1.json";
/// Bounds the file far above any real machine; the oldest note makes room.
pub(super) const MAX_HIDDEN_PART_NOTES: usize = 4096;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct HiddenPartNotes {
    #[serde(default)]
    notes: Vec<HiddenPartNote>,
    /// An unreadable file is never overwritten, and every check on it fails closed.
    #[serde(skip)]
    unreadable: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HiddenPartNote {
    container_identity: String,
    container_path: String,
}

impl HiddenPartNotes {
    /// The notes with `identity`'s path renamed to `file_name`; `None` when nothing changes.
    pub(super) fn moved(&self, identity: &str, file_name: &str) -> Result<Option<Self>, VaultError> {
        if self.unreadable {
            return Err(VaultError::Persistence);
        }
        let mut moved = self.clone();
        let Some(note) = moved.notes.iter_mut().find(|note| note.container_identity == identity) else {
            return Ok(None);
        };
        note.container_path = rename_records::renamed_path(&note.container_path, file_name);
        Ok(Some(moved))
    }
}

impl VaultAccessStore {
    pub(super) fn hidden_parts_path(&self) -> PathBuf {
        self.personal_path.with_file_name(HIDDEN_PARTS_FILE)
    }

    pub(super) fn load_hidden_parts_at_startup(&self) {
        let unreadable = HiddenPartNotes { notes: Vec::new(), unreadable: true };
        let notes = match self.fs.read(&self.hidden_parts_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(unreadable),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HiddenPartNotes::default(),
            Err(_) => unreadable,
        };
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .hidden_parts = notes;
    }

    pub(crate) fn has_hidden_part(&self, container_identity: &str) -> Result<bool, VaultError> {
        let state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if state.hidden_parts.unreadable {
            return Err(VaultError::Persistence);
        }
        Ok(state
            .hidden_parts
            .notes
            .iter()
            .any(|note| note.container_identity == container_identity))
    }

    /// Records that `target` has a hidden part; a failed save keeps the previous set.
    pub(crate) fn note_hidden_part(&self, target: &PasswordChangeTarget) -> Result<(), VaultError> {
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if state.hidden_parts.unreadable {
            return Err(VaultError::Persistence);
        }
        let mut updated = state.hidden_parts.clone();
        match updated
            .notes
            .iter_mut()
            .find(|note| note.container_identity == target.container_identity)
        {
            Some(note) if note.container_path == target.container_path => return Ok(()),
            Some(note) => note.container_path = target.container_path.clone(),
            None => {
                if updated.notes.len() >= MAX_HIDDEN_PART_NOTES {
                    updated.notes.remove(0);
                }
                updated.notes.push(HiddenPartNote {
                    container_identity: target.container_identity.clone(),
                    container_path: target.container_path.clone(),
                });
            }
        }
        self.save_hidden_parts(&updated)?;
        state.hidden_parts = updated;
        Ok(())
    }

    pub(super) fn save_hidden_parts(&self, notes: &HiddenPartNotes) -> Result<(), VaultError> {
        let bytes = serde_json::to_vec(notes).map_err(|_| VaultError::Persistence)?;
        self.fs
            .atomic_write(&self.hidden_parts_path(), &bytes)
            .map_err(|_| VaultError::Persistence)
    }
}
