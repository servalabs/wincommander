/// Read-only seam for a pre-provisioned shared lock. No automatic repair is
/// permitted under a shared lock. Root/file ACLs and old-version fencing are
/// prerequisites. Only the explicitly opted-in Windows lab route uses it.
struct ReadOnlySelectedGeneration {
    _ownership: super::settings_generation_ownership::SettingsGenerationLock,
    #[cfg(windows)]
    _root_pins: Vec<std::fs::File>,
    path: PathBuf,
    authority: PathBuf,
    scope: StoreScope,
}
impl ReadOnlySelectedGeneration {
    fn acquire(root: &Path, scope: StoreScope) -> Result<Self, String> {
        #[cfg(windows)]
        let root_pins = source_snapshot::pin_directories(root)?;
        let ownership = match scope {
            StoreScope::Machine => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_machine(root, true),
            StoreScope::CurrentUser => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_user(root, true),
        }.map_err(|error| format!("settings generation shared ownership unavailable: {error}"))?;
        Ok(Self { _ownership: ownership, #[cfg(windows)] _root_pins: root_pins,
            path: root.join(".settings-generation.v1"), authority: root.join(".settings-cutover-anchor.v1"), scope })
    }
    fn try_open(root: &Path, scope: StoreScope) -> Result<Self, String> {
        let reader = Self::acquire(root, scope)?;
        reader.read_all()?;
        Ok(reader)
    }
    fn inspect_startup(&self) -> Result<StartupView, String> {
        inspect_generation_files(&self._ownership, &self.path, &self.authority, self.scope)
    }
    fn read_all(&self) -> Result<BTreeMap<String, Value>, String> {
        match self.inspect_startup()? {
            StartupView::Selected(sections) => Ok(sections),
            StartupView::Legacy => Err("settings generation has not been selected".into()),
            StartupView::RecoveryRequired => Err("settings generation requires authorized recovery".into()),
        }
    }
}

fn verify_scoped_generation(scope: StoreScope, bytes: &[u8]) -> Result<(), String> {
    if open_generation(bytes).map_err(|error| scope.access_error(error))?.scope != scope {
        return Err("settings generation scope mismatch".into());
    }
    Ok(())
}
fn inspect_generation_files(guard: &super::settings_generation_ownership::SettingsGenerationLock, path: &Path, authority_path: &Path, scope: StoreScope) -> Result<StartupView, String> {
    let root = authority_path.parent().ok_or("settings root unavailable")?;
    let anchor = ProtectedFileAnchor::new(root, scope);
    let file = SelectedFile::new(root);
    match anchor.read()? {
        Some(AnchorState::Legacy) => {
            if read_generation_file(path, MAX_TOTAL * 2 + 65_536)?.is_some() { return Err("unexpected selected settings before cutover".into()); }
            Ok(StartupView::Legacy)
        },
        Some(AnchorState::Active { .. }) => {
            let bytes = cutover::read_active(guard, &anchor, &file, &|value| verify_scoped_generation(scope, value))?;
            Ok(StartupView::Selected(open_generation(&bytes).map_err(|error| scope.access_error(error))?.sections))
        },
        Some(AnchorState::Preparing { .. } | AnchorState::Updating { .. }) => Ok(StartupView::RecoveryRequired),
        None => Err("settings cutover anchor is missing; existing data preserved".into()),
    }
}

/// Keep shared ownership through a legacy read or selected-generation read.
pub(super) fn read_with_legacy(root: &Path, scope: StoreScope, section: &str, legacy: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    let owner = ReadOnlySelectedGeneration::acquire(root, scope)?;
    match owner.inspect_startup()? {
        StartupView::Legacy => legacy(),
        StartupView::Selected(sections) => Ok(selected_section(sections, section)),
        StartupView::RecoveryRequired => {
            // Never upgrade in place: release shared ownership before asking
            // for exclusive repair, then re-read the anchor under that guard.
            drop(owner);
            let exclusive = LockedSelectedGeneration::acquire_for_detailed(root, scope)
                .map_err(|error| error.message().to_string())?;
            let anchor = ProtectedFileAnchor::new(root, scope);
            match anchor.read()? {
                Some(AnchorState::Updating { .. } | AnchorState::Active { .. }) => {
                    let sections = exclusive.read_all()?;
                    Ok(selected_section(sections, section))
                },
                _ => Err("Settings migration requires authorized recovery from the original source. Existing data was preserved.".into()),
            }
        },
    }
}
fn selected_section(mut sections: BTreeMap<String, Value>, section: &str) -> Value {
    let name = matching_section(&sections, section).map(str::to_owned);
    // Missing F6 is safe only because the authenticated generation has an
    // explicit never-initialized state; whole-anchor rollback remains open.
    name.and_then(|name| sections.remove(&name)).unwrap_or_else(|| Value::Object(Default::default()))
}

/// Read an existing F6 key with shared ownership. Only authenticated absence
/// permits the caller to attempt exclusive creation; that caller must re-read
/// under exclusive ownership before generating anything.
pub(super) fn read_f6_or_create(
    root: &Path,
    scope: StoreScope,
    read_legacy: impl FnOnce() -> Result<Value, String>,
    create_exclusive: impl FnOnce() -> Result<Value, String>,
) -> Result<Value, String> {
    let value = read_with_legacy(root, scope, "f6-keystore", read_legacy)?;
    if valid_f6(&value) { return Ok(value); }
    if value != Value::Object(Default::default()) {
        return Err("device signing key is invalid; existing data preserved".into());
    }
    create_exclusive()
}

/// Keep exclusive ownership through either a legacy save or a selected write.
pub(super) fn write_with_legacy(root: &Path, scope: StoreScope, section: &str, value: &Value, legacy: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let owner = LockedSelectedGeneration::acquire_for(root, scope)?;
    match inspect_generation_files(&owner._ownership, &owner.path, &owner.authority, scope)? {
        StartupView::Legacy => legacy(),
        StartupView::Selected(_) => owner.write_section(section, value.clone()),
        StartupView::RecoveryRequired => Err("settings generation requires authorized recovery".into()),
    }
}

/// Holds exclusive ownership across settings preflight and the final write.
/// Root/lock ACL and old-version fencing remain activation prerequisites.
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(super) struct RuntimeWriteGuard {
    owner: LockedSelectedGeneration,
    _root_pins: Vec<std::fs::File>,
    selected: bool,
}
#[cfg(all(windows, feature = "experimental-settings-generation"))]
impl RuntimeWriteGuard {
    pub(super) fn selected(&self) -> bool { self.selected }
    pub(super) fn get_or_create_f6(&self, create: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
        if !self.selected { return Err("settings generation is not selected".into()); }
        self.owner.get_or_create_f6(create)
    }
    pub(super) fn write(&self, section: &str, value: &Value) -> Result<(), String> {
        if !self.selected { return Err("settings generation is not selected".into()); }
        self.owner.write_section(section, value.clone())
    }
}
#[cfg(all(windows, feature = "experimental-settings-generation"))]
pub(super) fn begin_experimental_write() -> Result<RuntimeWriteGuard, String> {
    let (root, pins, scope) = experimental_runtime_root()?;
    let owner = LockedSelectedGeneration::acquire_for(&root, scope)?;
    let selected = match inspect_generation_files(&owner._ownership, &owner.path, &owner.authority, owner.scope)? {
        StartupView::Legacy => false,
        StartupView::Selected(_) => true,
        StartupView::RecoveryRequired => {
            let anchor = ProtectedFileAnchor::new(&root, scope);
            if matches!(anchor.read()?, Some(AnchorState::Updating { .. })) {
                owner.read_reconciled()?;
                true
            } else {
                return Err("Settings migration requires authorized recovery from the original source. Existing data was preserved.".into());
            }
        },
    };
    Ok(RuntimeWriteGuard { owner, _root_pins: pins, selected })
}

/// Resolve and pin the selected store root for the explicit lab route.
/// ACL and cross-user/noninteractive behavior remain unvalidated on Windows.
#[cfg(windows)]
pub(super) fn experimental_runtime_root() -> Result<(PathBuf, Vec<std::fs::File>, StoreScope), String> {
    let scope = if crate::paths::current_user_install_uses_local_datastore() { StoreScope::CurrentUser } else { StoreScope::Machine };
    experimental_runtime_root_for(scope)
}

#[cfg(windows)]
fn experimental_runtime_root_for(scope: StoreScope) -> Result<(PathBuf, Vec<std::fs::File>, StoreScope), String> {
    let root = source_snapshot::known_root(scope)?;
    let pins = source_snapshot::pin_matching_roots(&root, scope)?;
    let root_handle = pins.last().ok_or("settings root unavailable")?;
    match scope {
        StoreScope::Machine => super::settings_generation_acl_windows::verify_machine_root_handle(root_handle),
        StoreScope::CurrentUser => super::settings_generation_acl_windows::verify_current_user_root_handle(root_handle),
    }.map_err(|_| "settings root permissions are not authorized")?;
    Ok((root, pins, scope))
}

/// Prepare pinned handles in the installer process. The installer must acquire
/// the exclusive byte-range lock itself before doing any protected work.
#[cfg(windows)]
pub(super) fn prepare_installer_search_fence(pid: u32) -> Result<(), String> {
    let root = source_snapshot::known_root(StoreScope::CurrentUser)?;
    let mut root_pins = source_snapshot::pin_matching_parents(&root, StoreScope::CurrentUser)?;
    match std::fs::create_dir(&root) {
        Ok(()) => {},
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(_) => return Err("Cannot prepare user search root.".into()),
    }
    root_pins.extend(source_snapshot::pin_matching_roots(&root, StoreScope::CurrentUser)?);
    duplicate_search_fence_handles(&root.join("file-search"), pid, root_pins)
}

#[cfg(windows)]
fn duplicate_search_fence_handles(parent_dir: &std::path::Path, pid: u32, mut pins: Vec<std::fs::File>) -> Result<(), String> {
    use std::{fs::OpenOptions, io::Write, os::windows::{fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle}};
    use windows_sys::Win32::{Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, WAIT_TIMEOUT},
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle},
        System::Threading::{GetCurrentProcess, OpenProcess, WaitForSingleObject, PROCESS_DUP_HANDLE, PROCESS_SYNCHRONIZE}};
    struct Process(*mut std::ffi::c_void);
    impl Drop for Process { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    if pid == 0 { return Err("Installer process is unavailable.".into()); }
    let parent = Process(unsafe { OpenProcess(PROCESS_DUP_HANDLE | PROCESS_SYNCHRONIZE, 0, pid) });
    if parent.0.is_null() { return Err("Cannot verify installer process lifetime.".into()); }
    if unsafe { WaitForSingleObject(parent.0, 0) } != WAIT_TIMEOUT { return Err("Installer already exited.".into()); }
    match std::fs::create_dir(parent_dir) {
        Ok(()) => {},
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(_) => return Err("Cannot prepare search maintenance directory.".into()),
    }
    pins.extend(source_snapshot::pin_directories(parent_dir)?);
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(parent_dir.join(".encrypted-search-maintenance.lock")).map_err(|_| "Cannot open search maintenance fence.")?;
    let metadata = file.metadata().map_err(|_| "Cannot inspect search maintenance fence.")?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 || info.nNumberOfLinks != 1 {
        return Err("Search maintenance fence is not a safe file.".into());
    }
    // Duplicated pin handles intentionally live until the installer exits. They
    // prevent ancestor replacement while it owns the lock. A failure makes the
    // installer abort, which closes any handles already duplicated into it.
    let current = unsafe { GetCurrentProcess() };
    for pin in &pins {
        let mut duplicate = std::ptr::null_mut();
        if unsafe { DuplicateHandle(current, pin.as_raw_handle(), parent.0, &mut duplicate, 0, 0, DUPLICATE_SAME_ACCESS) } == 0 {
            return Err("Cannot pin search directory in installer.".into());
        }
    }
    let mut duplicate = std::ptr::null_mut();
    if unsafe { DuplicateHandle(current, file.as_raw_handle(), parent.0, &mut duplicate, 0, 0, DUPLICATE_SAME_ACCESS) } == 0 {
        return Err("Cannot transfer search fence to installer.".into());
    }
    if unsafe { WaitForSingleObject(parent.0, 0) } != WAIT_TIMEOUT { return Err("Installer exited before maintenance began.".into()); }
    println!("WINCOMMANDER_SEARCH_FENCE_HANDLE_V1:{}", duplicate as usize);
    std::io::stdout().flush().map_err(|_| "Cannot confirm search maintenance handle.")?;
    Ok(())
}
