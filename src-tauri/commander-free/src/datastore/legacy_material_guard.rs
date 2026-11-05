// SPDX-License-Identifier: AGPL-3.0-or-later
//! Preserve legacy readability while requiring protected keys for new writes.

pub(super) fn protect_legacy_material(
    material: &[u8; 32], require_protected: bool,
    protect: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
    unprotect: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
    persist: impl FnOnce(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let migrated = (|| {
        let protected = protect(material)?;
        if protected.len() <= material.len() || unprotect(&protected)?.as_slice() != material {
            return Err("Protected settings key could not be verified".to_string());
        }
        persist(&protected)
    })();
    if require_protected {
        migrated.map_err(|_| "Settings key protection is unavailable; existing data preserved and changes were not saved.".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::protect_legacy_material;

    #[test]
    fn failed_protection_allows_read_but_refuses_write() {
        let key = [0x42; 32];
        for must_protect in [false, true] {
            let result = protect_legacy_material(&key, must_protect, |_| Err("failed".into()),
                |_| panic!("nothing to decrypt"), |_| panic!("must not persist"));
            assert_eq!(result.is_err(), must_protect);
            let result = protect_legacy_material(&key, must_protect, |_| Ok(vec![9; 64]),
                |_| Ok(vec![7; 32]), |_| panic!("mismatched key must not be persisted"));
            assert_eq!(result.is_err(), must_protect);
            let result = protect_legacy_material(&key, must_protect, |_| Ok(vec![9; 64]),
                |_| Ok(key.to_vec()), |_| Err("destination is locked".into()));
            assert_eq!(result.is_err(), must_protect);
        }
    }
}
