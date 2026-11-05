// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gated cutover protocol; no runtime provider is wired.
//! The caller must hold exclusive generation ownership and a trusted/pinned
//! root throughout every operation. The anchor provider must authenticate,
//! authorize and durably publish records independently of the selected file.
//! Restoring both an old valid anchor and an old valid file still requires an
//! external anti-rollback policy. This module never deletes legacy files.
#![allow(dead_code)]
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", deny_unknown_fields)]
pub(super) enum AnchorState {
    Legacy,
    Preparing { next: String },
    Active { digest: String },
    Updating { prior: String, next: String },
}
/// A real implementation must authenticate the record and restrict its
/// independent storage to the authorized system/user identity. Absence is an
/// error: an installed legacy state must itself be explicitly provisioned.
pub(super) trait Anchor {
    fn read(&self) -> Result<Option<AnchorState>, String>;
    fn write(&self, value: &AnchorState) -> Result<(), String>;
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectedRecord {
    format: String,
    scope: super::settings_rotation_trial::StoreScope,
    anchor: AnchorState,
}
/// File-backed draft provider. The root must be independently authorized and
/// pinned; it is not a trusted hardware/registry rollback anchor. Never deploy
/// solely on the basis of this provider or silently recreate a missing anchor.
pub(super) struct ProtectedFileAnchor {
    path: PathBuf,
    scope: super::settings_rotation_trial::StoreScope,
}
impl ProtectedFileAnchor {
    pub(super) fn new(root: &Path, scope: super::settings_rotation_trial::StoreScope) -> Self {
        Self { path: root.join(".settings-cutover-anchor.v1"), scope }
    }
    fn encode(&self, state: &AnchorState) -> Result<Vec<u8>, String> {
        let record = ProtectedRecord { format: "wincommander-settings-cutover-anchor-v1".into(), scope: self.scope, anchor: state.clone() };
        let plain = Zeroizing::new(serde_json::to_vec(&record).map_err(|_| "cannot encode settings cutover anchor")?);
        let protected = self.scope.protect(&plain)?;
        let reopened = Zeroizing::new(self.scope.unprotect(&protected)?);
        if reopened.as_slice() != plain.as_slice() || protected.len() > 65_536 || protected.is_empty() {
            return Err("settings cutover anchor protection failed".into());
        }
        Ok(protected)
    }
    /// Explicit install-time initialization only. A missing anchor must never be
    /// recreated as Legacy during normal startup or recovery.
    pub(super) fn provision_legacy(&self) -> Result<(), String> {
        let bytes = self.encode(&AnchorState::Legacy)?;
        if !crate::datastore_io::publish_new(&self.path, &bytes)? { return Err("settings cutover anchor already exists".into()); }
        if self.read()? != Some(AnchorState::Legacy) { return Err("settings cutover anchor initialization did not verify".into()); }
        Ok(())
    }
}
impl Anchor for ProtectedFileAnchor {
    fn read(&self) -> Result<Option<AnchorState>, String> {
        let Some(bytes) = super::settings_rotation_trial::read_generation_file(&self.path, 65_536)? else { return Ok(None); };
        let plain = Zeroizing::new(self.scope.unprotect(&bytes)?);
        if plain.len() > 65_536 { return Err("settings cutover anchor too large".into()); }
        let record: ProtectedRecord = serde_json::from_slice(&plain).map_err(|_| "invalid settings cutover anchor")?;
        if record.format != "wincommander-settings-cutover-anchor-v1" || record.scope != self.scope { return Err("invalid settings cutover anchor scope".into()); }
        Ok(Some(record.anchor))
    }
    fn write(&self, state: &AnchorState) -> Result<(), String> {
        if self.read()?.is_none() { return Err("settings cutover anchor is missing".into()); }
        let bytes = self.encode(state)?;
        crate::datastore_io::atomic_write(&self.path, &bytes)?;
        if self.read()?.as_ref() != Some(state) { return Err("settings cutover anchor publication did not verify".into()); }
        Ok(())
    }
}

pub(super) fn digest(bytes: &[u8]) -> String { B64.encode(Sha256::digest(bytes)) }
fn set_anchor(anchor: &impl Anchor, state: AnchorState) -> Result<(), String> {
    anchor.write(&state)?;
    if anchor.read()?.as_ref() != Some(&state) { return Err("settings cutover anchor readback mismatch".into()); }
    Ok(())
}

/// The selected file is separate from the old store. This helper does not
/// itself establish directory ACLs or the identity of the pinned root.
pub(super) struct SelectedFile { path: PathBuf }
impl SelectedFile {
    pub(super) fn new(root: &Path) -> Self { Self { path: root.join(".settings-generation.v1") } }
    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        super::settings_rotation_trial::read_generation_file(&self.path, 16 * 1024 * 1024 * 2 + 65_536)
    }
    fn create(&self, bytes: &[u8]) -> Result<(), String> {
        if !crate::datastore_io::publish_new(&self.path, bytes)? { return Err("selected settings already exist".into()); }
        Ok(())
    }
    fn replace(&self, bytes: &[u8]) -> Result<(), String> { crate::datastore_io::atomic_write(&self.path, bytes) }
}

/// Explicit first migration only. The caller must provide an independently
/// verified current source and enforce maintenance for all ordinary writers.
/// Marker absence cannot detect restoration/deletion of a previous cutover;
/// this is acceptable only under the documented first-rollout assumption.
pub(super) fn provision_first_migration(
    guard: &super::settings_generation_ownership::SettingsGenerationLock,
    anchor: &ProtectedFileAnchor, root: &Path, verify_source_and_maintenance: &impl Fn() -> Result<(), String>,
) -> Result<(), String> {
    guard.require_exclusive().map_err(|e| e.to_string())?;
    if anchor.read()?.is_some() { return Err("settings cutover anchor already exists".into()); }
    for name in [".settings-cutover-candidate.v1", ".settings-generation.v1"] {
        if super::settings_rotation_trial::read_generation_file(&root.join(name), 16 * 1024 * 1024 * 2 + 65_536)?.is_some() {
            return Err("existing settings cutover artifacts require recovery".into());
        }
    }
    verify_source_and_maintenance()?;
    anchor.provision_legacy()?;
    verify_source_and_maintenance()?;
    Ok(())
}

/// Start or resume initial selection. A crash after writing Preparing cannot
/// fall back to legacy; retry requires the exact candidate and a fresh source
/// verification under the same ownership/fencing rules as initial selection.
pub(super) fn select_initial(
    _guard: &super::settings_generation_ownership::SettingsGenerationLock,
    anchor: &impl Anchor, file: &SelectedFile, candidate: &[u8],
    verify_source: &impl Fn(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    _guard.require_exclusive().map_err(|e| e.to_string())?;
    verify_source(candidate)?;
    let wanted = digest(candidate);
    match anchor.read()? {
        Some(AnchorState::Legacy) => {
            if file.read()?.is_some() { return Err("unexpected selected settings before cutover".into()); }
            set_anchor(anchor, AnchorState::Preparing { next: wanted.clone() })?;
        },
        Some(AnchorState::Preparing { next }) if next == wanted => {},
        _ => return Err("settings cutover anchor is unavailable or inconsistent".into()),
    }
    verify_source(candidate)?;
    match file.read()? {
        None => file.create(candidate)?,
        Some(existing) if existing == candidate => {},
        Some(_) => return Err("selected settings differ from prepared cutover".into()),
    }
    if file.read()?.as_deref() != Some(candidate) { return Err("selected settings publication did not verify".into()); }
    verify_source(candidate)?;
    set_anchor(anchor, AnchorState::Active { digest: wanted })
}

/// Open the selected generation only when the separately trusted anchor is
/// active and matches authenticated bytes. Pending/absent state never falls
/// back to the legacy store.
pub(super) fn read_active(_guard: &super::settings_generation_ownership::SettingsGenerationLock, anchor: &impl Anchor, file: &SelectedFile, verify: &impl Fn(&[u8]) -> Result<(), String>) -> Result<Vec<u8>, String> {
    let Some(AnchorState::Active { digest: expected }) = anchor.read()? else { return Err("settings cutover is not active".into()); };
    let bytes = file.read()?.ok_or("selected settings are missing")?;
    if digest(&bytes) != expected { return Err("selected settings do not match cutover anchor".into()); }
    verify(&bytes)?;
    Ok(bytes)
}

/// Atomically publish a new generation after journaling its old/new digests.
/// A failed operation remains Updating and must be explicitly recovered.
pub(super) fn update(
    _guard: &super::settings_generation_ownership::SettingsGenerationLock,
    anchor: &impl Anchor, file: &SelectedFile, candidate: &[u8], verify: &impl Fn(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    _guard.require_exclusive().map_err(|e| e.to_string())?;
    let old = read_active(_guard, anchor, file, verify)?;
    verify(candidate)?;
    let prior = digest(&old); let next = digest(candidate);
    if prior == next { return Ok(()); }
    set_anchor(anchor, AnchorState::Updating { prior, next: next.clone() })?;
    file.replace(candidate)?;
    let observed = file.read()?.ok_or("selected settings missing after update")?;
    if observed != candidate { return Err("settings update readback mismatch".into()); }
    verify(&observed)?;
    set_anchor(anchor, AnchorState::Active { digest: next })
}

pub(super) fn recover_update(
    _guard: &super::settings_generation_ownership::SettingsGenerationLock,
    anchor: &impl Anchor, file: &SelectedFile, verify: &impl Fn(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    _guard.require_exclusive().map_err(|e| e.to_string())?;
    let Some(AnchorState::Updating { prior, next }) = anchor.read()? else { return Err("no interrupted settings update".into()); };
    let bytes = file.read()?.ok_or("interrupted settings generation is missing")?;
    let current = digest(&bytes);
    if current != prior && current != next { return Err("interrupted settings generation does not match anchor".into()); }
    verify(&bytes)?;
    set_anchor(anchor, AnchorState::Active { digest: current })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct TestAnchor(RefCell<Option<AnchorState>>);
    impl Anchor for TestAnchor {
        fn read(&self) -> Result<Option<AnchorState>, String> { Ok(self.0.borrow().clone()) }
        fn write(&self, value: &AnchorState) -> Result<(), String> { *self.0.borrow_mut() = Some(value.clone()); Ok(()) }
    }
    fn fixture() -> (tempfile::TempDir, super::super::settings_generation_ownership::SettingsGenerationLock) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".settings-generation.lock"), b"").unwrap();
        let guard = super::super::settings_generation_ownership::SettingsGenerationLock::try_acquire(root.path()).unwrap();
        (root, guard)
    }
    fn verify(bytes: &[u8]) -> Result<(), String> { if bytes.starts_with(b"valid:") { Ok(()) } else { Err("invalid generation".into()) } }
    #[test]
    fn initial_cutover_keeps_legacy_and_recovers_only_verified_candidate() {
        let (root, guard) = fixture();
        let legacy = root.path().join("f6-keystore.dat");
        std::fs::write(&legacy, b"original encrypted signing key").unwrap();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Legacy)));
        select_initial(&guard, &anchor, &file, b"valid:signing-key", &verify).unwrap();
        assert_eq!(read_active(&guard, &anchor, &file, &verify).unwrap(), b"valid:signing-key");
        assert_eq!(std::fs::read(&legacy).unwrap(), b"original encrypted signing key");
        std::fs::remove_file(&file.path).unwrap();
        assert!(read_active(&guard, &anchor, &file, &verify).is_err());
        *anchor.0.borrow_mut() = None;
        assert!(select_initial(&guard, &anchor, &file, b"valid:other", &verify).is_err());
    }
    #[test]
    fn pending_initial_requires_fresh_source_and_exact_candidate() {
        let (root, guard) = fixture();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Preparing { next: digest(b"valid:original") })));
        assert!(read_active(&guard, &anchor, &file, &verify).is_err());
        assert!(select_initial(&guard, &anchor, &file, b"valid:other", &verify).is_err());
        assert!(select_initial(&guard, &anchor, &file, b"valid:original", &|_| Err("source changed".into())).is_err());
        assert!(file.read().unwrap().is_none());
        select_initial(&guard, &anchor, &file, b"valid:original", &verify).unwrap();
    }
    #[test]
    fn interrupted_update_only_recovers_matching_valid_old_or_new() {
        let (root, guard) = fixture();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Legacy)));
        select_initial(&guard, &anchor, &file, b"valid:old", &verify).unwrap();
        let state = AnchorState::Updating { prior: digest(b"valid:old"), next: digest(b"valid:new") };
        *anchor.0.borrow_mut() = Some(state.clone());
        recover_update(&guard, &anchor, &file, &verify).unwrap();
        assert_eq!(read_active(&guard, &anchor, &file, &verify).unwrap(), b"valid:old");
        *anchor.0.borrow_mut() = Some(state.clone());
        file.replace(b"valid:new").unwrap();
        recover_update(&guard, &anchor, &file, &verify).unwrap();
        assert_eq!(read_active(&guard, &anchor, &file, &verify).unwrap(), b"valid:new");
        *anchor.0.borrow_mut() = Some(state);
        file.replace(b"valid:other").unwrap();
        assert!(recover_update(&guard, &anchor, &file, &verify).is_err());
        assert!(read_active(&guard, &anchor, &file, &verify).is_err());
    }
    #[test]
    fn file_anchor_survives_reopen_and_missing_or_corrupt_fails_closed() {
        let (root, guard) = fixture();
        let scope = super::super::settings_rotation_trial::StoreScope::Machine;
        let anchor = ProtectedFileAnchor::new(root.path(), scope);
        let file = SelectedFile::new(root.path());
        assert!(select_initial(&guard, &anchor, &file, b"valid:key", &verify).is_err());
        anchor.provision_legacy().unwrap();
        assert!(anchor.provision_legacy().is_err());
        select_initial(&guard, &anchor, &file, b"valid:key", &verify).unwrap();
        let reopened = ProtectedFileAnchor::new(root.path(), scope);
        assert_eq!(read_active(&guard, &reopened, &file, &verify).unwrap(), b"valid:key");
        std::fs::write(&anchor.path, b"corrupt").unwrap();
        assert!(read_active(&guard, &reopened, &file, &verify).is_err());
        assert!(reopened.provision_legacy().is_err());
        std::fs::remove_file(&anchor.path).unwrap();
        assert!(read_active(&guard, &reopened, &file, &verify).is_err());
        assert!(reopened.write(&AnchorState::Legacy).is_err());
    }

    #[test]
    fn shared_guard_cannot_start_or_repair_cutover() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".settings-generation.lock"), b"").unwrap();
        let shared = super::super::settings_generation_ownership::SettingsGenerationLock::try_acquire_shared(root.path()).unwrap();
        let file = SelectedFile::new(root.path());
        let anchor = TestAnchor(RefCell::new(Some(AnchorState::Legacy)));
        assert!(select_initial(&shared, &anchor, &file, b"valid:one", &verify).is_err());
        assert!(file.read().unwrap().is_none());
        assert_eq!(anchor.read().unwrap(), Some(AnchorState::Legacy));
    }

    #[test]
    fn first_migration_preserves_existing_key_and_rejects_ambiguous_artifacts() {
        let (root, guard) = fixture();
        let key = root.path().join("f6-keystore.dat");
        std::fs::write(&key, b"existing key").unwrap();
        let anchor = ProtectedFileAnchor::new(root.path(), super::super::settings_rotation_trial::StoreScope::Machine);
        assert!(provision_first_migration(&guard, &anchor, root.path(), &|| Err("writer running".into())).is_err());
        assert!(anchor.read().unwrap().is_none());
        let stage = root.path().join(".settings-cutover-candidate.v1");
        std::fs::write(&stage, b"ambiguous prior state").unwrap();
        assert!(provision_first_migration(&guard, &anchor, root.path(), &|| Ok(())).is_err());
        assert_eq!(std::fs::read(&stage).unwrap(), b"ambiguous prior state");
        std::fs::remove_file(stage).unwrap();
        provision_first_migration(&guard, &anchor, root.path(), &|| Ok(())).unwrap();
        assert_eq!(anchor.read().unwrap(), Some(AnchorState::Legacy));
        assert_eq!(std::fs::read(key).unwrap(), b"existing key");
    }

}
