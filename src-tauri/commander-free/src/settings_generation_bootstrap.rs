// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gated fresh-install provisioning; never called by ordinary startup.
//! Callers must prove a fresh installation, authorize and pin root/ancestors,
//! and fence all older writers. A missing anchor is not proof of a fresh install.
#![allow(dead_code)]
use super::{settings_generation_cutover::{Anchor, AnchorState, ProtectedFileAnchor},
    settings_generation_ownership::SettingsGenerationLock, settings_rotation_trial::StoreScope};
use std::path::Path;

fn ensure_empty(root: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(root).map_err(|_| "settings root unavailable")?;
    for entry in entries {
        let entry = entry.map_err(|_| "cannot inspect settings root")?;
        let name = entry.file_name();
        if name == ".settings-generation.lock" { continue; }
        if name == "store" {
            let meta = std::fs::symlink_metadata(entry.path()).map_err(|_| "cannot inspect existing settings store")?;
            if !meta.is_dir() || meta.file_type().is_symlink() { return Err("existing settings store is not a fresh empty directory".into()); }
            #[cfg(windows)] {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err("existing settings store is a linked directory".into());
                }
            }
            if std::fs::read_dir(entry.path()).map_err(|_| "cannot inspect existing settings store")?.next().is_some() {
                return Err("existing settings data found; explicit recovery is required".into());
            }
            continue;
        }
        return Err("existing settings data found; explicit recovery is required".into());
    }
    Ok(())
}

/// Called only after verified fresh-install authorization. No existing key,
/// settings file, candidate or marker is overwritten or removed.
pub(super) fn provision_fresh(
    root: &Path, scope: StoreScope, provision_lock: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    ensure_empty(root)?;
    provision_lock(root)?;
    let guard = match scope {
        StoreScope::Machine => SettingsGenerationLock::try_acquire_machine(root, false),
        StoreScope::CurrentUser => SettingsGenerationLock::try_acquire_user(root, false),
    }.map_err(|_| "cannot obtain authorized settings ownership")?;
    guard.require_exclusive().map_err(|e| e.to_string())?;
    ensure_empty(root)?;
    let anchor = ProtectedFileAnchor::new(root, scope);
    anchor.provision_legacy()?;
    if anchor.read()? != Some(AnchorState::Legacy) { return Err("settings anchor provisioning did not verify".into()); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_lock(root: &Path) -> Result<(), String> {
        std::fs::write(root.join(".settings-generation.lock"), b"").map_err(|e| e.to_string())
    }
    #[test]
    #[cfg_attr(windows, ignore = "requires authorized machine lock ACL; run in Windows acceptance")]
    fn fresh_provision_never_reclassifies_existing_key_or_settings() {
        let existing = tempfile::tempdir().unwrap();
        std::fs::write(existing.path().join(".install.material"), b"old protected key").unwrap();
        assert!(provision_fresh(existing.path(), StoreScope::Machine, fixture_lock).is_err());
        assert_eq!(std::fs::read(existing.path().join(".install.material")).unwrap(), b"old protected key");
        assert!(!existing.path().join(".settings-cutover-anchor.v1").exists());
        let fresh = tempfile::tempdir().unwrap();
        provision_fresh(fresh.path(), StoreScope::Machine, fixture_lock).unwrap();
        let anchor = ProtectedFileAnchor::new(fresh.path(), StoreScope::Machine);
        assert_eq!(anchor.read().unwrap(), Some(AnchorState::Legacy));
        assert!(provision_fresh(fresh.path(), StoreScope::Machine, fixture_lock).is_err());
    }
}
