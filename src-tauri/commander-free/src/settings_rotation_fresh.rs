// Fresh installs have no legacy key or sections to migrate. The marker and
// candidate still go through the same interrupted-selection protocol.
fn prepare_fresh_generation(scope: StoreScope) -> Result<Vec<u8>, String> {
    seal_generation(&Generation { scope, f6_initialized: false, sections: BTreeMap::new(),
        source_sections: BTreeMap::new(), source_key_digest: source_digest(b"wincommander-fresh-install-v1") })
}
fn verify_fresh_generation(root: &Path, scope: StoreScope, candidate: &[u8]) -> Result<(), String> {
    super::settings_generation_bootstrap::ensure_fresh_contents(root)?;
    let generation = open_generation(candidate).map_err(|error| scope.access_error(error))?;
    if generation.scope != scope || generation.f6_initialized || !generation.sections.is_empty()
        || !generation.source_sections.is_empty() || generation.source_key_digest != source_digest(b"wincommander-fresh-install-v1") {
        return Err("fresh settings candidate does not match an empty source".into());
    }
    Ok(())
}
fn select_fresh_generation(
    guard: &super::settings_generation_ownership::SettingsGenerationLock,
    root: &Path, scope: StoreScope, verify_maintenance: &impl Fn() -> Result<(), String>,
) -> Result<(), String> {
    guard.require_exclusive().map_err(|e| e.to_string())?;
    verify_maintenance()?;
    let anchor = ProtectedFileAnchor::new(root, scope);
    if matches!(anchor.read()?, Some(AnchorState::Active { .. })) {
        return cutover::read_active(guard, &anchor, &SelectedFile::new(root), &|bytes| verify_scoped_generation(scope, bytes)).map(|_| ());
    }
    stage_and_select(guard, &anchor, root, || prepare_fresh_generation(scope),
        &|bytes| { verify_maintenance()?; verify_fresh_generation(root, scope, bytes) })
}

#[cfg(windows)]
pub(super) fn pin_root_for_acl(root: &Path) -> Result<Vec<std::fs::File>, String> {
    source_snapshot::pin_directories(root)
}

/// Create or verify a fresh install's data root while holding both the known
/// and configured parent paths pinned. Its DACL is verified before any store
/// operation. Fresh-install proof and old-process fencing remain external.
#[cfg(windows)]
fn prepare_fresh_root() -> Result<(PathBuf, Vec<std::fs::File>, StoreScope), String> {
    let scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    let root = source_snapshot::known_root(scope)?;
    let mut pins = source_snapshot::pin_matching_parents(&root, scope)?;
    pins.extend(super::settings_generation_acl_windows::create_or_verify_root(&root, scope == StoreScope::CurrentUser)
        .map_err(|_| "cannot create or verify authorized settings root")?);
    Ok((root, pins, scope))
}

/// Explicit installer-lab entry for a verified fresh install only. Never
/// invoked automatically; the caller must establish fresh-install authority
/// and prevent old executable launches before calling it.
#[cfg(windows)]
pub(super) fn provision_verified_fresh_install() -> Result<(), String> {
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    let (root, _pins, scope) = prepare_fresh_root()?;
    super::settings_generation_maintenance::verify_known_writers_stopped()?;
    super::settings_generation_bootstrap::provision_fresh(&root, scope, |path| {
        match scope {
            StoreScope::Machine => super::settings_generation_acl_windows::provision_or_verify(path),
            StoreScope::CurrentUser => super::settings_generation_acl_windows::provision_user_or_verify(path),
        }.map_err(|_| "cannot provision authorized settings lock".into())
    })?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    select_fresh_generation(&owner._ownership, &root, scope, &|| super::settings_generation_maintenance::verify_known_writers_stopped())
}

/// Trial reader/writer for an already selected generation. The caller must
/// serialize all readers and writers and keep trusted selection state; neither
/// condition is supplied by this experiment. Never falls back to the legacy
/// store if the selected generation is unreadable or corrupt.
fn update_selected_generation(
    path: &Path,
    section: &str,
    value: Value,
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let input = read_generation_file(path, MAX_TOTAL * 2 + 65_536)?.ok_or("selected settings generation unavailable")?;
    let candidate = updated_generation(&input, section, value)?;
    persist(path, &candidate)
}

fn updated_generation(input: &[u8], section: &str, value: Value) -> Result<Vec<u8>, String> {
    let mut generation = open_generation(input)?;
    if section.is_empty() || section.len() > 128 { return Err("invalid settings section".into()); }
    validate_section_name(section)?;
    let existing = matching_section(&generation.sections, section).map(str::to_owned);
    if existing.is_none() && generation.sections.len() >= MAX_SECTIONS { return Err("excessive settings sections".into()); }
    if section.eq_ignore_ascii_case("f6-keystore") {
        if !valid_f6(&value) { return Err("device signing key is empty or invalid; existing key preserved".into()); }
        if let Some(name) = existing.as_ref() {
            let old_seed = generation.sections[name]["signing_seed_hex"].as_str().ok_or("device signing key is invalid")?;
            let new_seed = value["signing_seed_hex"].as_str().ok_or("device signing key is invalid")?;
            if !old_seed.eq_ignore_ascii_case(new_seed) { return Err("replacing an existing device signing key is not allowed".into()); }
        }
        generation.f6_initialized = true;
    }
    generation.sections.insert(existing.unwrap_or_else(|| section.to_owned()), value);
    let candidate = seal_generation(&generation)?;
    let reopened = open_generation(&candidate)?;
    if reopened.sections != generation.sections { return Err("settings generation readback mismatch".into()); }
    Ok(candidate)
}

/// Test-only single-file generation selection. The caller must hold a complete,
/// authorized source snapshot and exclusive migration ownership through this
/// call. Existing legacy files are never renamed or deleted. No production
/// reader or writer selects this file yet.
fn select_initial_generation(
    path: &Path,
    candidate: &[u8],
    old_material: &[u8; 32],
    sections: &[(String, String)],
    create: &impl Fn(&Path, &[u8]) -> Result<bool, String>,
) -> Result<(), String> {
    use std::io::Read;
    verify_source(candidate, old_material, sections)?;
    let publication = create(path, candidate);
    let file = std::fs::File::open(path).map_err(|_| "selected settings generation unavailable; legacy preserved")?;
    let mut observed = Vec::new();
    file.take((MAX_TOTAL * 2 + 65537) as u64).read_to_end(&mut observed)
        .map_err(|_| "cannot read selected settings generation; legacy preserved")?;
    if observed.len() > MAX_TOTAL * 2 + 65536 { return Err("selected settings generation too large; legacy preserved".into()); }
    // If our publish succeeded, or its result is ambiguous, only our exact
    // bytes can establish that it committed. An existing other winner must
    // independently authenticate against the exact guarded original values.
    if !matches!(publication, Ok(false)) && observed != candidate {
        return Err("settings generation publication ambiguous; legacy preserved".into());
    }
    verify_source(&observed, old_material, sections)?;
    Ok(())
}
