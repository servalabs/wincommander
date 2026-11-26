// SPDX-License-Identifier: AGPL-3.0-or-later
//! Letter repair uses durable authority, never an encrypted-device prefix alone.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Category {
    RealWindowsVolume,
    ActiveWincommanderMount,
    StaleWincommanderReservation,
    DuplicateSavedEntry,
    SystemOrPolicyReservedLetter,
    ExternalOrUnknownOwner,
    StaleDriveMapping,
}

fn category(
    letter: char,
    saved: usize,
    policy: bool,
    windows_volume: bool,
    mounts: usize,
    stale_owned: bool,
) -> Category {
    if letter <= 'C' || policy {
        return Category::SystemOrPolicyReservedLetter;
    }
    if saved > 1 {
        return Category::DuplicateSavedEntry;
    }
    if windows_volume {
        return Category::RealWindowsVolume;
    }
    if saved == 1 {
        return Category::SystemOrPolicyReservedLetter;
    }
    if mounts > 0 && !stale_owned {
        return Category::ActiveWincommanderMount;
    }
    if mounts == 1 && stale_owned {
        return Category::StaleWincommanderReservation;
    }
    Category::ExternalOrUnknownOwner
}

impl VaultMountBroker {
    pub(crate) fn cleanup_owned_letters_locked(
        &self,
        store: &VaultAccessStore,
        sid: &str,
        session: u32,
        logon: (u32, i32),
        saved: &[String],
        caller_is_administrator: bool,
    ) -> Result<serde_json::Value, &'static str> {
        let mut result = self.cleanup_owned_letters_with(
            store,
            sid,
            session,
            logon,
            saved,
            crate::vault_drive_letters::is_real_global_volume,
            |mount, letter| {
                if mount.presentation == VaultPresentation::Machine {
                    matches!(
                        crate::vault_drive_letters::cleanup_global_encrypted_link(
                            letter,
                            mount.internal_drive
                        ),
                        Ok(
                            crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Absent
                                | crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Removed
                        )
                    )
                } else if mount.authentication_id == Some(logon) {
                    matches!(
                        crate::vault_drive_letters::cleanup_owned_caller_link(
                            logon,
                            letter,
                            mount.internal_drive
                        ),
                        Ok(
                            crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Absent
                                | crate::vault_drive_letters::GlobalEncryptedLinkCleanup::Removed
                        )
                    )
                } else {
                    (self.private_alias_cleanup)(mount) == Ok(true)
                }
            },
            |letters| self.release_stale_letters_locked(letters, Some(sid)),
            caller_is_administrator.then_some(crate::vault_access::account_label_for_sid),
        )?;
        let Some(reports) = result["letters"].as_array_mut() else {
            return Ok(result);
        };
        let mapping_letters = reports
            .iter()
            .filter_map(external_letter)
            .collect::<HashSet<_>>();
        let mappings = if mapping_letters.is_empty() {
            HashMap::new()
        } else {
            crate::vault_drive_letters::session_drive_mappings(logon, sid, session, &mapping_letters)
                .unwrap_or_default()
        };
        let assignments = store.reservation_assignments(sid, caller_is_administrator);
        for report in reports {
            if let Some(mapping) = external_letter(report).and_then(|letter| mappings.get(&letter)) {
                report["windows_mapping"] = serde_json::json!(mapping);
            }
            if report["vault_policy_reservation"] == true {
                let assignment = report["letter"]
                    .as_str()
                    .and_then(|letter| assignments.get(letter));
                if let Some((is_caller, name)) = assignment {
                    report["vault_policy_owner_is_caller"] = serde_json::json!(is_caller);
                    if let Some(name) = name {
                        report["vault_policy_owner"] = serde_json::json!(name);
                    }
                }
            }
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn cleanup_owned_letters_with(
        &self,
        store: &VaultAccessStore,
        sid: &str,
        session: u32,
        logon: (u32, i32),
        saved: &[String],
        real_volume: impl Fn(char) -> bool,
        mut cleanup_alias: impl FnMut(&ActiveMount, char) -> bool,
        release_dead_links: impl FnOnce(&HashSet<char>) -> HashMap<char, LetterRelease>,
        account_name: Option<fn(&str) -> Option<String>>,
    ) -> Result<serde_json::Value, &'static str> {
        const UNKNOWN: &str = "vault_drive_letter_cleanup_failed";
        if self.recovery.lock().map_or(true, |state| {
            state.registry_untrusted
                || !state.persistence_pending.is_empty()
                || !state.removal_pending.is_empty()
        }) {
            return Err(UNKNOWN);
        }
        let snapshot = self.snapshot().map_err(|_| UNKNOWN)?;
        let occupied = (self.drive_letter_probe)().map_err(|_| UNKNOWN)?;
        let policy = store.reserved_letters(None).map_err(|_| UNKNOWN)?;
        let active = self.active.lock().map_err(|_| UNKNOWN)?.clone();
        let mapped = occupied
            .iter()
            .filter(|letter| letter.len() == 1)
            .filter_map(|letter| letter.chars().next())
            .filter(|letter| ('D'..='Z').contains(letter))
            .collect::<HashSet<_>>();
        let mut releases = if mapped.is_empty() {
            HashMap::new()
        } else {
            release_dead_links(&mapped)
        };
        let remaining = (self.drive_letter_probe)().map_err(|_| UNKNOWN)?;
        let mut reports = Vec::new();
        let mut released = 0;
        // The picker and cleanup support D–Z; do not report A–C as if they
        // were occupied when this operation never attempts to release them.
        for letter in 'D'..='Z' {
            let key = letter.to_string();
            let saved_count = saved.iter().filter(|entry| **entry == key).count();
            let mounts = active
                .iter()
                .filter(|(_, mount)| {
                    mount
                        .drive_letter
                        .trim_end_matches(':')
                        .eq_ignore_ascii_case(&key)
                })
                .collect::<Vec<_>>();
            if !occupied.contains(&key)
                && saved_count == 0
                && !policy.contains(&key)
                && mounts.is_empty()
            {
                continue;
            }
            let own = mounts.first().filter(|(_, mount)| {
                mounts.len() == 1
                    // A signed-out owner's closed Vault no longer needs that owner to clean it up.
                    && (mount.caller_sid == sid || self.owner_logon_ended(mount))
                    && mount.engine_mount_identity.is_some()
                    && !snapshot.contains_key(&mount.internal_drive)
                    && !requires_syncthing_pause(mount)
                    && ((mount.authentication_id == Some(logon)
                        && same_mount_owner(mount, session, sid, Some(logon)))
                        || self.owner_logon_ended(mount))
            });
            let windows_volume = real_volume(letter);
            let mut kind = category(
                letter,
                saved_count,
                policy.contains(&key),
                windows_volume,
                mounts.len(),
                own.is_some(),
            );
            let mut was_released = false;
            if kind == Category::StaleWincommanderReservation {
                let (id, mount) = own.ok_or(UNKNOWN)?;
                // Reused slots and changed inventories retain the journal and alias.
                if self.snapshot().map_err(|_| UNKNOWN)? == snapshot {
                    let cleaned = cleanup_alias(mount, letter);
                    if cleaned
                        && self.snapshot().map_err(|_| UNKNOWN)? == snapshot
                        && !(self.drive_letter_probe)()
                            .map_err(|_| UNKNOWN)?
                            .contains(&key)
                    {
                        was_released = self.clear_retained_mount(store, id);
                        if was_released {
                            released += 1;
                        }
                    } else {
                        kind = Category::ExternalOrUnknownOwner;
                    }
                } else {
                    kind = Category::ExternalOrUnknownOwner;
                }
            }
            let mut holder = None;
            if kind == Category::ExternalOrUnknownOwner && mounts.is_empty() {
                if let Some(release) = releases.remove(&letter) {
                    if release.released() > 0 && !remaining.contains(&key) {
                        kind = Category::StaleDriveMapping;
                        was_released = true;
                        released += 1;
                    }
                    holder = release
                        .reported()
                        .map(|holder| holder_report(holder, sid, account_name));
                }
            }
            let mut report = serde_json::json!({ "letter": key, "category": kind, "released": was_released,
                "saved_entry": saved_count > 0, "vault_policy_reservation": policy.contains(&key) });
            if let Some(holder) = holder {
                report["holder"] = holder;
            }
            if kind == Category::ActiveWincommanderMount
                && matches!(mounts.as_slice(), [(_, mount)]
                    if mount.personal
                        && mount.presentation == VaultPresentation::PerUser
                        && mount.caller_sid != sid
                        && !mount.cleanup_required
                        && mount.engine_mount_identity.as_ref().is_some_and(|identity| {
                            snapshot.get(&mount.internal_drive) == Some(identity)
                        }))
            {
                report["other_account_personal_vault"] = serde_json::json!(true);
            }
            reports.push(report);
        }
        // The returned availability is another observation, never the old query cache.
        let mut unavailable = self
            .unavailable_letters_locked(store, None)
            .map_err(|_| UNKNOWN)?;
        unavailable.extend_from_slice(saved);
        unavailable.sort();
        unavailable.dedup();
        Ok(
            serde_json::json!({ "released": released, "letters": reports, "unavailable_letters": unavailable }),
        )
    }
}

/// Another account's name is included only when `account_name` is given (administrators).
fn holder_report(
    holder: &LetterHolder,
    caller_sid: &str,
    account_name: Option<fn(&str) -> Option<String>>,
) -> serde_json::Value {
    let mut report = serde_json::json!({
        "kind": holder.kind,
        "scope": if holder.logon.is_some() { "session" } else { "machine" },
        "logon_state": holder.state,
        "device_present": holder.device_present,
    });
    if let Some(owner) = &holder.owner_sid {
        let is_caller = owner == caller_sid;
        report["owner_is_caller"] = serde_json::json!(is_caller);
        let name = account_name
            .filter(|_| !is_caller)
            .and_then(|lookup| lookup(owner))
            .filter(|name| {
                !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
            });
        if let Some(name) = name {
            report["owner"] = serde_json::json!(name);
        }
    }
    report
}

fn external_letter(report: &serde_json::Value) -> Option<char> {
    if report["category"] != "external_or_unknown_owner" {
        return None;
    }
    report["letter"].as_str()?.chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_classifies_protected_real_duplicate_active_external_and_stale_letters() {
        assert_eq!(
            category('C', 0, false, false, 1, true),
            Category::SystemOrPolicyReservedLetter
        );
        assert_eq!(
            category('Q', 0, true, false, 1, true),
            Category::SystemOrPolicyReservedLetter
        );
        assert_eq!(
            category('Q', 2, false, false, 1, true),
            Category::DuplicateSavedEntry
        );
        assert_eq!(
            category('Q', 0, false, true, 1, true),
            Category::RealWindowsVolume
        );
        assert_eq!(
            category('Q', 1, false, false, 1, true),
            Category::SystemOrPolicyReservedLetter
        );
        assert_eq!(
            category('Q', 0, false, false, 1, false),
            Category::ActiveWincommanderMount
        );
        assert_eq!(
            category('Q', 0, false, false, 0, false),
            Category::ExternalOrUnknownOwner
        );
        assert_eq!(
            category('Q', 0, false, false, 1, true),
            Category::StaleWincommanderReservation
        );
        assert_ne!(
            category('Q', 0, false, false, 2, true),
            Category::StaleWincommanderReservation
        );
    }
}
