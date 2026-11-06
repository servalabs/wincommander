/// Read-only startup decision; callers must not fall back to legacy when this
/// reports recovery or an error. The Windows lab route uses the same decision.
#[derive(Debug, PartialEq)]
pub(super) enum StartupView {
    Legacy,
    Selected { sections: BTreeMap<String, Value>, fresh: bool },
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallSourceProof { Fresh, Legacy(String), Preparing(String), Active(String) }
impl InstallSourceProof {
    fn encode(&self) -> String {
        match self {
            Self::Fresh => format!("F{}", "0".repeat(64)),
            Self::Legacy(digest) => format!("L{digest}"),
            Self::Preparing(digest) => format!("P{digest}"),
            Self::Active(digest) => format!("A{digest}"),
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        if value.len() != 65 || !value.is_ascii() { return Err("Invalid settings source proof; retry setup.".into()); }
        let (kind, digest) = value.split_at(1);
        if !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Err("Invalid settings source proof; retry setup.".into());
        }
        match kind {
            "F" if digest.bytes().all(|byte| byte == b'0') => Ok(Self::Fresh),
            "L" => Ok(Self::Legacy(digest.into())),
            "P" => Ok(Self::Preparing(digest.into())),
            "A" => Ok(Self::Active(digest.into())),
            _ => Err("Invalid settings source proof; retry setup.".into()),
        }
    }
}
fn verify_legacy_source_proof(initial: &Option<AnchorState>, expected: &InstallSourceProof, generation: &Generation) -> Result<(), String> {
    let digest = legacy_source_proof(generation)?;
    let valid = match (initial, expected) {
        (None | Some(AnchorState::Legacy), InstallSourceProof::Legacy(value)) => value == &digest,
        (Some(AnchorState::Preparing { .. }), InstallSourceProof::Preparing(value)) => value == &digest,
        _ => false,
    };
    if valid { Ok(()) } else { Err("Settings source changed since preflight; existing data was preserved. Retry setup.".into()) }
}
fn verify_active_source_proof(expected: &InstallSourceProof, selected: &[u8]) -> Result<(), String> {
    if *expected == InstallSourceProof::Active(hex::encode(Sha256::digest(selected))) { Ok(()) }
    else { Err("Settings state changed since preflight; retry setup.".into()) }
}
fn verify_fresh_pending(root: &Path, state: &AnchorState) -> Result<(), String> {
    super::settings_generation_bootstrap::ensure_fresh_contents(root)?;
    let stage = read_generation_file(&root.join(".settings-cutover-candidate.v1"), MAX_TOTAL * 2 + 65_536)?;
    let selected = read_generation_file(&root.join(".settings-generation.v1"), MAX_TOTAL * 2 + 65_536)?;
    match state {
        AnchorState::Legacy => {
            if selected.is_some() { return Err("Unexpected selected settings require recovery.".into()); }
            if let Some(candidate) = stage { verify_fresh_generation(root, StoreScope::Machine, &candidate)?; }
        },
        AnchorState::Preparing { next } => {
            let candidate = stage.ok_or("Prepared fresh settings candidate is missing")?;
            verify_fresh_generation(root, StoreScope::Machine, &candidate)?;
            if cutover::digest(&candidate) != *next || selected.as_ref().is_some_and(|bytes| bytes != &candidate) {
                return Err("Prepared fresh settings do not match the recovery journal.".into());
            }
        },
        _ => return Err("Fresh settings state requires recovery.".into()),
    }
    Ok(())
}
pub(super) fn inspect_startup_once(root: &Path, scope: StoreScope) -> Result<StartupView, String> {
    ReadOnlySelectedGeneration::acquire(root, scope)?.inspect_startup()
}
/// The paired installer lab cannot start the desktop on legacy or an
/// incomplete generation. Retrying setup remains the recovery route.
pub(super) fn require_selected_startup(view: StartupView, scope: StoreScope, validate_settings: impl FnOnce(&Value) -> bool) -> Result<(), String> {
    match view {
        StartupView::Selected { sections, fresh: true } if scope == StoreScope::Machine => {
            let settings = matching_section(&sections, "settings").and_then(|name| sections.get(name));
            let f6 = matching_section(&sections, "f6-keystore").and_then(|name| sections.get(name));
            if settings.is_some_and(validate_settings) && f6.is_some_and(valid_f6) {
                Ok(())
            } else {
                Err("Protected settings setup is incomplete; retry setup. Existing data was preserved.".into())
            }
        },
        StartupView::Selected { .. } => Ok(()),
        _ => Err("Protected settings setup needs to finish; retry setup.".into()),
    }
}

/// Open and release shared ownership for this read only. It cannot be cached by
/// callers, avoiding long-lived reader locks that would block policy writers.
pub(super) fn read_selected_once(root: &Path, scope: StoreScope) -> Result<BTreeMap<String, Value>, String> {
    ReadOnlySelectedGeneration::acquire(root, scope)?.read_all()
}

/// Prepare a reusable encrypted candidate before recording the cutover.
/// Existing stage bytes are never overwritten; source verification detects a
/// stale or incomplete stage. The caller holds exclusive ownership and pins.
fn stage_and_select<A: Anchor>(
    guard: &super::settings_generation_ownership::SettingsGenerationLock,
    anchor: &A,
    root: &Path,
    prepare: impl FnOnce() -> Result<Vec<u8>, String>,
    verify: &impl Fn(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    guard.require_exclusive().map_err(|e| e.to_string())?;
    let stage_path = root.join(".settings-cutover-candidate.v1");
    let state = anchor.read()?.ok_or("settings cutover anchor is missing")?;
    let candidate = match state {
        AnchorState::Legacy => match read_generation_file(&stage_path, MAX_TOTAL * 2 + 65_536)? {
            Some(existing) => existing,
            None => {
                let candidate = prepare()?;
                verify(&candidate)?;
                if !crate::datastore_io::publish_new(&stage_path, &candidate)? {
                    return Err("settings cutover candidate already exists; retry recovery".into());
                }
                let observed = read_generation_file(&stage_path, MAX_TOTAL * 2 + 65_536)?
                    .ok_or("settings cutover candidate is missing")?;
                if observed != candidate { return Err("settings cutover candidate did not verify".into()); }
                observed
            }
        },
        AnchorState::Preparing { .. } => read_generation_file(&stage_path, MAX_TOTAL * 2 + 65_536)?
            .ok_or("prepared settings candidate is missing; restore it before recovery")?,
        _ => return Err("settings cutover is not awaiting initial selection".into()),
    };
    verify(&candidate)?;
    cutover::select_initial(guard, anchor, &SelectedFile::new(root), &candidate, verify)
}

/// Explicit lab-only migration entry point. No application or installer route
/// invokes this function. Root ACL, old-writer exclusion and independent anchor
/// authorization remain mandatory external prerequisites.
#[cfg(windows)]
pub(super) fn migrate_explicitly() -> Result<(), String> {
    migrate_explicitly_with_proof(None)
}
#[cfg(windows)]
fn migrate_explicitly_with_proof(expected: Option<&InstallSourceProof>) -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let _maintenance_pins = if crate::paths::current_user_install_uses_local_datastore() { None }
        else { Some(source_snapshot::pin_machine_maintenance()?) };
    let (root, _pins, scope) = experimental_runtime_root()?;
    match scope {
        StoreScope::Machine => super::settings_generation_acl_windows::provision_or_verify(&root),
        StoreScope::CurrentUser => super::settings_generation_acl_windows::provision_user_or_verify(&root),
    }.map_err(|_| "cannot provision authorized settings lock; existing data preserved")?;
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    let initial = anchor.read()?;
    match initial {
        Some(AnchorState::Active { .. }) if expected.is_none() => { owner.read_reconciled()?; return Ok(()); },
        Some(AnchorState::Active { .. }) => return Err("Settings state changed since preflight; retry setup.".into()),
        Some(AnchorState::Legacy | AnchorState::Preparing { .. }) | None => {},
        Some(AnchorState::Updating { .. }) => return Err("settings update requires authorized recovery before migration".into()),
    }
    let snapshot = match scope {
        StoreScope::Machine => source_snapshot::Snapshot::capture_machine()?,
        StoreScope::CurrentUser => source_snapshot::Snapshot::capture_user()?,
    };
    let held = std::cell::RefCell::new(snapshot);
    let verify_expected = |bytes: &[u8]| -> Result<(), String> {
        let Some(expected) = expected else { return Ok(()); };
        verify_legacy_source_proof(&initial, expected, &open_generation(bytes)?)
    };
    if initial.is_none() {
        // Authenticate every original, including F6, before provisioning the
        // first marker. This assumes the first rollout has never selected a
        // prior generation; losing all old cutover artifacts is undetectable.
        let probe = held.borrow_mut().prepare()?;
        held.borrow_mut().verify_generation(&probe)?;
        verify_expected(&probe)?;
        cutover::provision_first_migration_with_stage(&owner._ownership, &anchor, &root, &|stage| {
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            held.borrow_mut().verify_generation(stage.unwrap_or(&probe))?;
            verify_expected(stage.unwrap_or(&probe))
        })?;
    }
    stage_and_select(&owner._ownership, &anchor, &root,
        || held.borrow_mut().prepare(),
        &|bytes| {
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            held.borrow_mut().verify_generation(bytes)?;
            verify_expected(bytes)
        })
}

// An interrupted post-selection write is safe to resolve under the same
// exclusive lock and authenticated journal used by the installer itself.
// Initial selection still requires a freshly verified legacy snapshot.
fn reconcile_stage_owner(owner: &LockedSelectedGeneration) -> Result<bool, String> {
    let root = owner.path.parent().ok_or("settings root unavailable")?;
    let anchor = ProtectedFileAnchor::new(root, owner.scope);
    match anchor.read()? {
        Some(AnchorState::Active { .. } | AnchorState::Updating { .. }) => {
            owner.read_reconciled()?;
            Ok(true)
        },
        Some(AnchorState::Legacy | AnchorState::Preparing { .. }) | None => Ok(false),
    }
}

fn stage_root_exists(root: &Path, scope: StoreScope) -> Result<(), String> {
    match std::fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && scope == StoreScope::Machine => Err(unverified_identity()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err("Original user settings root is missing; no migration was performed.".into()),
        Err(_) => Err("Cannot inspect settings root; existing data preserved.".into()),
        Ok(_) => Ok(()),
    }
}
fn stage_keyless_store(root: &Path, scope: StoreScope) -> Result<(), String> {
    let store = root.join("store");
    match std::fs::read_dir(&store) {
        Ok(mut entries) => {
            if entries.next().is_none() { return if scope == StoreScope::Machine { Err(unverified_identity()) } else { Ok(()) }; }
            Err("Settings key is missing for existing store; existing data preserved.".into())
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => if scope == StoreScope::Machine { Err(unverified_identity()) } else { Ok(()) },
        Err(_) => Err("Settings key is missing or store cannot be inspected; existing data preserved.".into()),
    }
}

// Do not leave an unselected candidate behind when setup needs an identity
// choice. The old application may keep writing its store after the user says
// No; a candidate published before that choice would become stale on retry.
fn publish_stage_after_identity_check(stage: &Path, prepared: &[u8]) -> Result<Vec<u8>, String> {
    verify_f6_creation_authority(&open_generation(prepared)?)?;
    if !crate::datastore_io::publish_new(stage, prepared)? { return Err("Settings staging changed; retry with originals preserved.".into()); }
    let observed = read_generation_file(stage, MAX_TOTAL * 2 + 65_536)?.ok_or("Settings stage unavailable")?;
    if observed != prepared { return Err("Settings stage readback failed; originals preserved.".into()); }
    Ok(observed)
}

/// Preflight the machine source without publishing a new candidate. Proof is
/// only meaningful while it is retained by the same installer invocation.
#[cfg(windows)]
pub(super) fn stage_machine_with_proof() -> Result<(String, u8), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let _maintenance_pins = source_snapshot::pin_machine_maintenance()?;
    let root = source_snapshot::known_root(StoreScope::Machine)?;
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((InstallSourceProof::Fresh.encode(), 2)),
        Err(_) => return Err("Cannot inspect settings root; existing data preserved.".into()),
        Ok(_) => {},
    }
    let (root, _pins, scope) = experimental_runtime_root_for(StoreScope::Machine)?;
    let key = read_generation_file(&root.join(".install.material"), 65_536)?;
    let anchor_bytes = read_generation_file(&root.join(".settings-cutover-anchor.v1"), 65_536)?;
    if key.is_none() && anchor_bytes.is_none() {
        // Fresh proof is not allowed to hide an orphan staged or selected file.
        for name in [".settings-cutover-candidate.v1", ".settings-generation.v1"] {
            if read_generation_file(&root.join(name), MAX_TOTAL * 2 + 65_536)?.is_some() {
                return Err("Existing settings artifacts require recovery; originals preserved.".into());
            }
        }
        super::settings_generation_bootstrap::ensure_fresh_contents(&root)?;
        return Ok((InstallSourceProof::Fresh.encode(), 2));
    }
    super::settings_generation_acl_windows::provision_or_verify(&root)
        .map_err(|_| "Cannot provision settings lock; existing data preserved.")?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    let state = anchor.read()?;
    if matches!(state, Some(AnchorState::Active { .. } | AnchorState::Updating { .. })) {
        // Authenticated interrupted-update recovery is the sole preflight
        // mutation; its journal only admits the recorded prior/next bytes.
        let bytes = owner.read_reconciled()?;
        let selected = open_generation(&bytes)?;
        return Ok((InstallSourceProof::Active(hex::encode(Sha256::digest(&bytes))).encode(),
            machine_identity_consent_code(&selected)));
    }
    if key.is_none() {
        let pending = state.as_ref().ok_or("Settings anchor disappeared; recovery required")?;
        verify_fresh_pending(&root, pending)?;
        return Ok((InstallSourceProof::Fresh.encode(), 2));
    }
    let mut snapshot = source_snapshot::Snapshot::capture_machine()?;
    let stage_path = root.join(".settings-cutover-candidate.v1");
    let selected_path = root.join(".settings-generation.v1");
    let selected = read_generation_file(&selected_path, MAX_TOTAL * 2 + 65_536)?;
    if !matches!(state, Some(AnchorState::Preparing { .. })) && selected.is_some() {
        return Err("Unexpected selected settings require recovery; originals preserved.".into());
    }
    let candidate = match read_generation_file(&stage_path, MAX_TOTAL * 2 + 65_536)? {
        Some(bytes) => bytes,
        None if matches!(state, Some(AnchorState::Preparing { .. })) => return Err("Prepared settings candidate is missing; recovery required.".into()),
        None => snapshot.prepare()?,
    };
    if let Some(AnchorState::Preparing { next }) = &state {
        if cutover::digest(&candidate) != *next { return Err("Prepared settings candidate does not match its journal.".into()); }
        if selected.as_ref().is_some_and(|bytes| bytes != &candidate) {
            return Err("Selected settings differ from the prepared candidate; recovery required.".into());
        }
    }
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    snapshot.verify_generation(&candidate)?;
    let generation = open_generation(&candidate)?;
    let digest = legacy_source_proof(&generation)?;
    let proof = if matches!(state, Some(AnchorState::Preparing { .. })) { InstallSourceProof::Preparing(digest) }
        else { InstallSourceProof::Legacy(digest) };
    Ok((proof.encode(), machine_identity_consent_code(&generation)))
}
/// Pre-replacement staging only. It never provisions a cutover anchor or
/// selects a generation, so the original installation remains authoritative.
/// User staging additionally requires an expected SID checked by the caller.
/// Installer handoff and reliable identity capture are not yet implemented.
#[cfg(windows)]
pub(super) fn stage_existing_explicitly(expected_scope: StoreScope) -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let actual_scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    if expected_scope == StoreScope::Machine && actual_scope != expected_scope {
        return Err("Settings installation scope does not match the running application; existing data preserved.".into());
    }
    let _maintenance_pins = if expected_scope == StoreScope::Machine { Some(source_snapshot::pin_machine_maintenance()?) } else { None };
    let root = source_snapshot::known_root(expected_scope)?;
    stage_root_exists(&root, expected_scope)?;
    let (root, _pins, scope) = experimental_runtime_root_for(expected_scope)?;
    let _installed_binary = if scope == StoreScope::CurrentUser {
        Some(source_snapshot::pin_user_installed_binary(&root)?)
    } else { None };
    let key_name = match scope { StoreScope::Machine => ".install.material", StoreScope::CurrentUser => ".user-store.material" };
    let key = read_generation_file(&root.join(key_name), 65_536)?;
    let existing_anchor = read_generation_file(&root.join(".settings-cutover-anchor.v1"), 65_536)?;
    if key.is_none() && existing_anchor.is_none() {
        return stage_keyless_store(&root, scope);
    }
    match scope {
        StoreScope::Machine => super::settings_generation_acl_windows::provision_or_verify(&root),
        StoreScope::CurrentUser => super::settings_generation_acl_windows::provision_user_or_verify(&root),
    }.map_err(|_| "Cannot provision settings lock; existing data preserved.")?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    if reconcile_stage_owner(&owner)? { return verify_machine_f6_creation_authority(&owner); }
    let mut snapshot = match scope {
        StoreScope::Machine => source_snapshot::Snapshot::capture_machine()?,
        StoreScope::CurrentUser => source_snapshot::Snapshot::capture_user_from_verified_helper()?,
    };
    let stage = root.join(".settings-cutover-candidate.v1");
    let candidate = match read_generation_file(&stage, MAX_TOTAL * 2 + 65_536)? {
        Some(existing) => existing,
        None => {
            let prepared = snapshot.prepare()?;
            snapshot.verify_generation(&prepared)?;
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            publish_stage_after_identity_check(&stage, &prepared)?
        },
    };
    if let Some(AnchorState::Preparing { next }) = anchor.read()? {
        if cutover::digest(&candidate) != next { return Err("Settings stage does not match pending recovery; originals preserved.".into()); }
    }
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    snapshot.verify_generation(&candidate)?;
    verify_f6_creation_authority(&open_generation(&candidate)?)
}

/// Under machine install authority, initialize F6 only when the authenticated
/// generation proves it has never existed. This permits standard-user reads
/// without granting users the machine settings write lock.
fn verify_machine_f6_creation_authority(owner: &LockedSelectedGeneration) -> Result<(), String> {
    let generation = open_generation(&owner.read_reconciled()?)?;
    verify_f6_creation_authority(&generation)
}
fn unverified_identity() -> String {
    "SETTINGS_IDENTITY_UNVERIFIED: The existing device signing identity could not be verified. Existing data was preserved. Automatic restoration is not available; a new identity requires explicit confirmation.".into()
}
fn verify_f6_creation_authority(generation: &Generation) -> Result<(), String> {
    if generation.scope != StoreScope::Machine { return Ok(()); }
    if matching_section(&generation.sections, "f6-keystore").is_none() {
        return Err(unverified_identity());
    }
    Ok(())
}
fn machine_identity_consent_code(generation: &Generation) -> u8 {
    if verify_f6_creation_authority(generation).is_err() { 2 } else { 3 }
}

/// Explicit, offline administrator action. The caller must explain that a
/// newly generated identity cannot recover a previous per-user identity.
#[cfg(windows)]
pub(super) fn create_new_machine_identity_explicitly() -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    if crate::paths::current_user_install_uses_local_datastore() {
        return Err("Machine identity recovery requires a machine installation.".into());
    }
    let _maintenance = source_snapshot::pin_machine_maintenance()?;
    let (root, _pins, scope) = experimental_runtime_root_for(StoreScope::Machine)?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    owner.create_new_f6_confirmed(|| {
        let (signing, _) = wincmd_shared::wipe_auth::generate_provisioning_keypair();
        let seed = zeroize::Zeroizing::new(wincmd_shared::wipe_auth::signing_key_to_bytes(&signing));
        Ok(serde_json::json!({"signing_seed_hex": hex::encode(&*seed)}))
    }).map(|_| ())
}

fn provision_machine_f6_owned(owner: &LockedSelectedGeneration, create: impl FnOnce() -> Result<Value, String>) -> Result<(), String> {
    if owner.scope != StoreScope::Machine { return Ok(()); }
    verify_machine_f6_creation_authority(owner)?;
    owner.get_or_create_f6(create).map(|_| ())
}

// Only an authenticated generation created from a verified fresh source may
// receive defaults. Migrated or existing sections are never replaced.
fn provision_fresh_machine_settings_owned(owner: &LockedSelectedGeneration, create: impl FnOnce() -> Result<Value, String>) -> Result<(), String> {
    if owner.scope != StoreScope::Machine { return Ok(()); }
    let generation = open_generation(&owner.read_reconciled()?)?;
    if generation.source_key_digest != source_digest(b"wincommander-fresh-install-v1") || !generation.source_sections.is_empty() {
        return Ok(());
    }
    if matching_section(&generation.sections, "settings").is_some() { return Ok(()); }
    let value = create()?;
    if !value.as_object().is_some_and(|object| !object.is_empty()) {
        return Err("Fresh machine settings are invalid; existing data preserved.".into());
    }
    owner.write_section("settings", value.clone())?;
    let committed = owner.read_all()?;
    let name = matching_section(&committed, "settings").ok_or("Fresh machine settings were not committed")?;
    if committed[name] != value { return Err("Fresh machine settings did not verify".into()); }
    Ok(())
}

#[cfg(windows)]
fn provision_machine_f6_after_selection(root: &Path, scope: StoreScope, create: impl FnOnce() -> Result<Value, String>) -> Result<(), String> {
    if scope != StoreScope::Machine { return Ok(()); }
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let owner = LockedSelectedGeneration::acquire_for(root, scope)?;
    provision_machine_f6_owned(&owner, create)?;
    provision_fresh_machine_settings_owned(&owner, crate::settings::fresh_machine_settings)
}

/// Installer decision under the same maintenance checks. A protected Active
/// generation is reused; an existing legacy key is migrated; otherwise the
/// conservative fresh path rejects any unexpected existing file or directory.
#[cfg(windows)]
pub(super) fn install_migration_explicitly() -> Result<(), String> {
    install_migration_with_proof(None)
}
#[cfg(windows)]
pub(super) fn install_machine_with_proof(proof: &str) -> Result<(), String> {
    let expected = InstallSourceProof::parse(proof)?;
    install_migration_with_proof(Some(&expected))
}
#[cfg(windows)]
fn install_migration_with_proof(expected: Option<&InstallSourceProof>) -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    if expected.is_some() && scope != StoreScope::Machine { return Err("Settings installation scope changed; existing data preserved.".into()); }
    let _maintenance_pins = if scope == StoreScope::Machine { Some(source_snapshot::pin_machine_maintenance()?) } else { None };
    let initial_root = source_snapshot::known_root(scope)?;
    match std::fs::symlink_metadata(&initial_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if expected.is_some_and(|proof| *proof != InstallSourceProof::Fresh) { return Err("Settings source changed since preflight; retry setup.".into()); }
            provision_verified_fresh_install()?
        },
        Err(_) => return Err("Cannot inspect settings root; existing data was preserved.".into()),
        Ok(_) => {
            let (root, _pins, scope) = experimental_runtime_root()?;
            let anchor = ProtectedFileAnchor::new(&root, scope);
            if matches!(anchor.read()?, Some(AnchorState::Active { .. } | AnchorState::Updating { .. })) {
                let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
                let selected = owner.read_reconciled()?;
                if let Some(proof) = expected {
                    verify_active_source_proof(proof, &selected)?;
                }
            } else {
                let key_name = match scope { StoreScope::Machine => ".install.material", StoreScope::CurrentUser => ".user-store.material" };
                if read_generation_file(&root.join(key_name), 65_536)?.is_some() {
                    migrate_explicitly_with_proof(expected)?;
                } else {
                    if expected.is_some_and(|proof| *proof != InstallSourceProof::Fresh) { return Err("Settings source changed since preflight; retry setup.".into()); }
                    provision_verified_fresh_install()?;
                }
            }
        },
    }
    // Fresh provisioning can create the root after the first maintenance pin
    // was acquired. Re-pin it before the remaining F6/settings writes.
    let _final_maintenance_pins = if scope == StoreScope::Machine { Some(source_snapshot::pin_machine_maintenance()?) } else { None };
    let (root, _pins, scope) = experimental_runtime_root()?;
    provision_machine_f6_after_selection(&root, scope, || {
        let (signing, _) = wincmd_shared::wipe_auth::generate_provisioning_keypair();
        let seed = zeroize::Zeroizing::new(wincmd_shared::wipe_auth::signing_key_to_bytes(&signing));
        Ok(serde_json::json!({"signing_seed_hex": hex::encode(&*seed)}))
    })
}
