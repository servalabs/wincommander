// SPDX-License-Identifier: AGPL-3.0-or-later
//! Letter repair uses durable authority, never an encrypted-device prefix alone.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Category { RealWindowsVolume, ActiveWincommanderMount, StaleWincommanderReservation, DuplicateSavedEntry, SystemOrPolicyReservedLetter, ExternalOrUnknownOwner }

fn category(letter: char, saved: usize, policy: bool, windows_volume: bool, mounts: usize, stale_owned: bool) -> Category {
    if letter <= 'C' || policy { return Category::SystemOrPolicyReservedLetter; }
    if saved > 1 { return Category::DuplicateSavedEntry; }
    if windows_volume { return Category::RealWindowsVolume; }
    if saved == 1 { return Category::SystemOrPolicyReservedLetter; }
    if mounts > 0 && !stale_owned { return Category::ActiveWincommanderMount; }
    if mounts == 1 && stale_owned { return Category::StaleWincommanderReservation; }
    Category::ExternalOrUnknownOwner
}

impl VaultMountBroker {
    pub(crate) fn cleanup_owned_letters_locked(
        &self, store: &VaultAccessStore, sid: &str, session: u32, logon: (u32, i32), saved: &[String],
    ) -> Result<serde_json::Value, &'static str> {
        self.cleanup_owned_letters_with(store, sid, session, logon, saved,
            crate::vault_drive_letters::is_real_global_volume, |mount, letter| {
                if mount.presentation == VaultPresentation::Machine {
                    matches!(crate::vault_drive_letters::cleanup_global_encrypted_link(letter, mount.internal_drive),
                        Ok(crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Absent | crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Removed))
                } else if mount.authentication_id == Some(logon) {
                    matches!(crate::vault_drive_letters::cleanup_owned_caller_link(logon, letter, mount.internal_drive),
                        Ok(crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Absent | crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Removed))
                } else { (self.private_alias_cleanup)(mount) == Ok(true) }
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn cleanup_owned_letters_with(
        &self, store: &VaultAccessStore, sid: &str, session: u32, logon: (u32, i32), saved: &[String],
        real_volume: impl Fn(char) -> bool, mut cleanup_alias: impl FnMut(&ActiveMount, char) -> bool,
    ) -> Result<serde_json::Value, &'static str> {
        const UNKNOWN: &str = "vault_drive_letter_cleanup_failed";
        if self.recovery.lock().map_or(true, |state| state.registry_untrusted
            || !state.persistence_pending.is_empty() || !state.removal_pending.is_empty()) { return Err(UNKNOWN); }
        let snapshot = self.snapshot().map_err(|_| UNKNOWN)?;
        let occupied = (self.drive_letter_probe)().map_err(|_| UNKNOWN)?;
        let policy = store.reserved_letters(None).map_err(|_| UNKNOWN)?;
        let active = self.active.lock().map_err(|_| UNKNOWN)?.clone();
        let mut reports = Vec::new();
        let mut released = 0;
        for letter in 'A'..='Z' {
            let key = letter.to_string();
            let saved_count = saved.iter().filter(|entry| **entry == key).count();
            let mounts = active.iter().filter(|(_, mount)| mount.drive_letter.trim_end_matches(':').eq_ignore_ascii_case(&key)).collect::<Vec<_>>();
            if letter > 'C' && !occupied.contains(&key) && saved_count == 0 && !policy.contains(&key) && mounts.is_empty() { continue; }
            let own = mounts.first().filter(|(_, mount)| mounts.len() == 1
                && mount.caller_sid == sid && mount.engine_mount_identity.is_some()
                && !snapshot.contains_key(&mount.internal_drive)
                && !requires_syncthing_pause(mount)
                && ((mount.authentication_id == Some(logon) && same_mount_owner(mount, session, sid, Some(logon))) || self.owner_logon_ended(mount)));
            let windows_volume = real_volume(letter);
            let mut kind = category(letter, saved_count, policy.contains(&key), windows_volume, mounts.len(), own.is_some());
            let mut was_released = false;
            if kind == Category::StaleWincommanderReservation {
                let (id, mount) = own.ok_or(UNKNOWN)?;
                // Reused slots and changed inventories retain the journal and alias.
                if self.snapshot().map_err(|_| UNKNOWN)? == snapshot {
                    let cleaned = cleanup_alias(mount, letter);
                    if cleaned && self.snapshot().map_err(|_| UNKNOWN)? == snapshot
                        && !(self.drive_letter_probe)().map_err(|_| UNKNOWN)?.contains(&key) {
                        was_released = self.clear_retained_mount(store, id);
                        if was_released { released += 1; }
                    } else { kind = Category::ExternalOrUnknownOwner; }
                } else { kind = Category::ExternalOrUnknownOwner; }
            }
            reports.push(serde_json::json!({ "letter": key, "category": kind, "released": was_released,
                "saved_entry": saved_count > 0 }));
        }
        // The returned availability is another observation, never the old query cache.
        let mut unavailable = self.unavailable_letters_locked(store, None).map_err(|_| UNKNOWN)?;
        unavailable.extend_from_slice(saved);
        unavailable.sort(); unavailable.dedup();
        Ok(serde_json::json!({ "released": released, "letters": reports, "unavailable_letters": unavailable }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_classifies_protected_real_duplicate_active_external_and_stale_letters() {
        assert_eq!(category('C', 0, false, false, 1, true), Category::SystemOrPolicyReservedLetter);
        assert_eq!(category('Q', 0, true, false, 1, true), Category::SystemOrPolicyReservedLetter);
        assert_eq!(category('Q', 2, false, false, 1, true), Category::DuplicateSavedEntry);
        assert_eq!(category('Q', 0, false, true, 1, true), Category::RealWindowsVolume);
        assert_eq!(category('Q', 1, false, false, 1, true), Category::SystemOrPolicyReservedLetter);
        assert_eq!(category('Q', 0, false, false, 1, false), Category::ActiveWincommanderMount);
        assert_eq!(category('Q', 0, false, false, 0, false), Category::ExternalOrUnknownOwner);
        assert_eq!(category('Q', 0, false, false, 1, true), Category::StaleWincommanderReservation);
        assert_ne!(category('Q', 0, false, false, 2, true), Category::StaleWincommanderReservation);
    }
}
