/// Read-only startup decision; callers must not fall back to legacy when this
/// reports recovery or an error. The Windows lab route uses the same decision.
#[derive(Debug, PartialEq)]
pub(super) enum StartupView {
    Legacy,
    Selected(BTreeMap<String, Value>),
    RecoveryRequired,
}
pub(super) fn inspect_startup_once(root: &Path, scope: StoreScope) -> Result<StartupView, String> {
    ReadOnlySelectedGeneration::acquire(root, scope)?.inspect_startup()
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
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
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
        Some(AnchorState::Active { .. }) => { owner.read_reconciled()?; return Ok(()); },
        Some(AnchorState::Legacy | AnchorState::Preparing { .. }) | None => {},
        Some(AnchorState::Updating { .. }) => return Err("settings update requires authorized recovery before migration".into()),
    }
    let snapshot = match scope {
        StoreScope::Machine => source_snapshot::Snapshot::capture_machine()?,
        StoreScope::CurrentUser => source_snapshot::Snapshot::capture_user()?,
    };
    let held = std::cell::RefCell::new(snapshot);
    if initial.is_none() {
        // Authenticate every original, including F6, before provisioning the
        // first marker. This assumes the first rollout has never selected a
        // prior generation; losing all old cutover artifacts is undetectable.
        let probe = held.borrow_mut().prepare()?;
        cutover::provision_first_migration_with_stage(&owner._ownership, &anchor, &root, &|stage| {
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            held.borrow_mut().verify_generation(stage.unwrap_or(&probe))
        })?;
    }
    stage_and_select(&owner._ownership, &anchor, &root,
        || held.borrow_mut().prepare(),
        &|bytes| {
            super::settings_generation_maintenance::verify_known_writers_stopped()?;
            held.borrow_mut().verify_generation(bytes)
        })
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
    let root = source_snapshot::known_root(expected_scope)?;
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && expected_scope == StoreScope::Machine => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err("Original user settings root is missing; no migration was performed.".into()),
        Err(_) => return Err("Cannot inspect settings root; existing data preserved.".into()),
        Ok(_) => {},
    }
    let (root, _pins, scope) = experimental_runtime_root_for(expected_scope)?;
    let _installed_binary = if scope == StoreScope::CurrentUser {
        Some(source_snapshot::pin_user_installed_binary(&root)?)
    } else { None };
    let key_name = match scope { StoreScope::Machine => ".install.material", StoreScope::CurrentUser => ".user-store.material" };
    let key = read_generation_file(&root.join(key_name), 65_536)?;
    if key.is_none() {
        let store = root.join("store");
        match std::fs::read_dir(&store) {
            Ok(mut entries) => {
                if entries.next().is_none() { return Ok(()); }
                return Err("Settings key is missing for existing store; existing data preserved.".into());
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            _ => return Err("Settings key is missing or store cannot be inspected; existing data preserved.".into()),
        }
    }
    match scope {
        StoreScope::Machine => super::settings_generation_acl_windows::provision_or_verify(&root),
        StoreScope::CurrentUser => super::settings_generation_acl_windows::provision_user_or_verify(&root),
    }.map_err(|_| "Cannot provision settings lock; existing data preserved.")?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    match anchor.read()? {
        Some(AnchorState::Active { .. }) => { owner.read_reconciled()?; return Ok(()); },
        Some(AnchorState::Updating { .. }) => return Err("Settings update requires recovery; existing data preserved.".into()),
        Some(AnchorState::Legacy | AnchorState::Preparing { .. }) | None => {},
    }
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
            if !crate::datastore_io::publish_new(&stage, &prepared)? { return Err("Settings staging changed; retry with originals preserved.".into()); }
            let observed = read_generation_file(&stage, MAX_TOTAL * 2 + 65_536)?.ok_or("Settings stage unavailable")?;
            if observed != prepared { return Err("Settings stage readback failed; originals preserved.".into()); }
            observed
        },
    };
    if let Some(AnchorState::Preparing { next }) = anchor.read()? {
        if cutover::digest(&candidate) != next { return Err("Settings stage does not match pending recovery; originals preserved.".into()); }
    }
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    snapshot.verify_generation(&candidate)
}

/// Installer decision under the same maintenance checks. A protected Active
/// generation is reused; an existing legacy key is migrated; otherwise the
/// conservative fresh path rejects any unexpected existing file or directory.
#[cfg(windows)]
pub(super) fn install_migration_explicitly() -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    let root = source_snapshot::known_root(scope)?;
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return provision_verified_fresh_install(),
        Err(_) => return Err("Cannot inspect settings root; existing data was preserved.".into()),
        Ok(_) => {},
    }
    let (root, _pins, scope) = experimental_runtime_root()?;
    let anchor = ProtectedFileAnchor::new(&root, scope);
    if matches!(anchor.read()?, Some(AnchorState::Active { .. } | AnchorState::Updating { .. })) {
        let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
        owner.read_reconciled()?;
        return Ok(());
    }
    let key_name = match scope { StoreScope::Machine => ".install.material", StoreScope::CurrentUser => ".user-store.material" };
    if read_generation_file(&root.join(key_name), 65_536)?.is_some() {
        migrate_explicitly()
    } else {
        provision_verified_fresh_install()
    }
}
