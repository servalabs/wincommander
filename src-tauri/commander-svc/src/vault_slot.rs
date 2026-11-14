// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reject ambiguous driver-slot mappings before selecting or mutating a mount.

pub(super) fn unique_entry<'a>(entries: impl Iterator<Item = (&'a str, u8)>, slot: u8) -> Result<Option<&'a str>, ()> {
    let mut matches = entries.filter(|(_, candidate)| *candidate == slot);
    let first = matches.next().map(|(entry, _)| entry);
    if matches.next().is_some() { return Err(()); }
    Ok(first)
}

pub(super) fn all_unique(entries: impl Iterator<Item = u8>) -> bool {
    let mut seen = std::collections::HashSet::new();
    entries.into_iter().all(|slot| seen.insert(slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_unique_and_ambiguous_slots_are_distinct() {
        let rows = [("personal", 12), ("managed", 13)];
        assert_eq!(unique_entry(rows.iter().copied(), 9), Ok(None));
        assert_eq!(unique_entry(rows.iter().copied(), 12), Ok(Some("personal")));
        assert!(all_unique(rows.iter().map(|(_, slot)| *slot)));
        let collision = [("personal", 12), ("managed", 12)];
        assert_eq!(unique_entry(collision.iter().copied(), 12), Err(()));
        assert!(!all_unique(collision.iter().map(|(_, slot)| *slot)));
    }
}
