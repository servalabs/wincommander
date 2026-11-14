/// Holds the created stage handle and an independent reader while the caller
/// verifies. Reopening the path each time detects observed replacement. Root
/// authorization and activation fencing remain production requirements.
struct StagedFile {
    path: PathBuf,
    written: std::fs::File,
    _reader: std::fs::File,
}

fn open_stage_reader(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    if std::fs::symlink_metadata(path).map_err(|_| "cannot inspect settings stage")?.file_type().is_symlink() {
        return Err("linked settings stage refused".into());
    }
    options.open(path).map_err(|_| "cannot reopen settings stage".into())
}

impl StagedFile {
    fn verify(&self, candidate: &[u8], old_material: &[u8; 32], sections: &[(String, String)]) -> Result<(), String> {
        use std::io::Read;
        let limit = MAX_TOTAL * 2 + 65536;
        let readback = open_stage_reader(&self.path)?;
        if !same_stage_file(&self.written, &readback)? { return Err("settings stage identity changed".into()); }
        let metadata = readback.metadata().map_err(|_| "cannot inspect settings stage")?;
        if !metadata.is_file() || metadata.len() > limit as u64 { return Err("invalid settings stage".into()); }
        let mut actual = Vec::new();
        readback.take(limit as u64 + 1).read_to_end(&mut actual).map_err(|_| "cannot read settings stage")?;
        if actual.len() > limit || actual != candidate { return Err("settings stage readback mismatch".into()); }
        verify_source(&actual, old_material, sections)
    }
}

/// Reopen an existing unactivated stage after restart, without modifying it.
/// The caller must hold and authenticate the complete source snapshot and
/// authorize the containing directory before considering any activation.
fn recover_stage(path: &Path, old_material: &[u8; 32], sections: &[(String, String)]) -> Result<(StagedFile, Vec<u8>), String> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
        options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    if std::fs::symlink_metadata(path).map_err(|_| "cannot inspect settings stage")?.file_type().is_symlink() {
        return Err("linked settings stage refused".into());
    }
    let written = options.open(path).map_err(|_| "cannot open settings stage for recovery")?;
    let reader = open_stage_reader(path)?;
    if !same_stage_file(&written, &reader)? { return Err("settings stage identity changed".into()); }
    let limit = MAX_TOTAL * 2 + 65536;
    let metadata = reader.metadata().map_err(|_| "cannot inspect settings stage")?;
    if !metadata.is_file() || metadata.len() > limit as u64 { return Err("invalid settings stage".into()); }
    let mut candidate = Vec::new();
    (&reader).take(limit as u64 + 1).read_to_end(&mut candidate).map_err(|_| "cannot read settings stage")?;
    if candidate.len() > limit { return Err("invalid settings stage".into()); }
    verify_source(&candidate, old_material, sections)?;
    let guard = StagedFile { path: path.to_path_buf(), written, _reader: reader };
    guard.verify(&candidate, old_material, sections)?;
    Ok((guard, candidate))
}

/// Create a new stage without overwrite and hold it through readback. A failed
/// attempt may leave an unactivated file that must be inspected, not activated.
fn stage_new_with_hook(
    path: &Path,
    candidate: &[u8],
    old_material: &[u8; 32],
    sections: &[(String, String)],
    after_write: &impl Fn(&Path, &mut std::fs::File) -> Result<(), String>,
) -> Result<StagedFile, String> {
    use std::io::Write;
    let limit = MAX_TOTAL * 2 + 65536;
    if candidate.len() > limit { return Err("settings generation too large".into()); }
    verify_source(candidate, old_material, sections)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    let mut written = options.open(path).map_err(|_| "settings stage already exists or is unavailable")?;
    written.write_all(candidate).and_then(|_| written.sync_all())
        .map_err(|_| "settings stage write failed; inspect destination")?;
    after_write(path, &mut written)?;
    let reader = open_stage_reader(path)?;
    if !same_stage_file(&written, &reader)? { return Err("settings stage identity changed".into()); }
    let guard = StagedFile { path: path.to_path_buf(), written, _reader: reader };
    guard.verify(candidate, old_material, sections)?;
    Ok(guard)
}

#[cfg(windows)]
fn same_stage_file(first: &std::fs::File, second: &std::fs::File) -> Result<bool, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT};
    let mut a = BY_HANDLE_FILE_INFORMATION::default();
    let mut b = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: both handles and output structs remain valid throughout the calls.
    if unsafe { GetFileInformationByHandle(first.as_raw_handle(), &mut a) } == 0
        || unsafe { GetFileInformationByHandle(second.as_raw_handle(), &mut b) } == 0 {
        return Err("cannot verify settings stage identity".into());
    }
    Ok(a.nNumberOfLinks == 1 && b.nNumberOfLinks == 1
        && a.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
        && b.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
        && a.dwVolumeSerialNumber == b.dwVolumeSerialNumber
        && a.nFileIndexHigh == b.nFileIndexHigh && a.nFileIndexLow == b.nFileIndexLow)
}

#[cfg(unix)]
fn same_stage_file(first: &std::fs::File, second: &std::fs::File) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    let a = first.metadata().map_err(|_| "cannot verify settings stage identity")?;
    let b = second.metadata().map_err(|_| "cannot verify settings stage identity")?;
    Ok(a.nlink() == 1 && b.nlink() == 1 && a.dev() == b.dev() && a.ino() == b.ino())
}

/// A single file contains both protected key material and all encrypted
/// sections. Publication must be atomic; an ambiguous error requires readback.
fn publish(
    path: &Path,
    candidate: &[u8],
    old_material: &[u8; 32],
    sections: &[(String, String)],
    persist: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    verify_source(candidate, old_material, sections)?;
    persist(path, candidate)
}

/// Unwired selected-generation store. Callers must authorize the root, fence
/// old binaries, and provide a trusted anti-rollback anchor before integration.
pub(super) struct LockedSelectedGeneration {
    _ownership: super::settings_generation_ownership::SettingsGenerationLock,
    #[cfg(windows)]
    _root_pins: Vec<std::fs::File>,
    path: PathBuf,
    authority: PathBuf,
    scope: StoreScope,
}

// No missing/error conflation. Windows holds a no-follow, no-write/delete
// handle for each read; the owner also holds directory pins for its lifetime.
pub(super) fn read_generation_file(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
        options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(not(windows))]
    if path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) { return Err("linked settings file refused".into()); }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("settings generation file unavailable".into()),
    };
    let metadata = file.metadata().map_err(|_| "cannot inspect settings file")?;
    if !metadata.is_file() || metadata.len() > limit as u64 { return Err("invalid settings file".into()); }
    #[cfg(windows)] {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT};
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file handle and output structure remain valid for the call.
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.nNumberOfLinks != 1 { return Err("invalid settings file".into()); }
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|_| "cannot read settings file")?;
    if bytes.len() > limit { return Err("settings file too large".into()); }
    Ok(Some(bytes))
}

#[derive(Debug, PartialEq, Eq)]
enum OwnerAcquireError { Busy, Unavailable }
impl OwnerAcquireError {
    fn message(&self) -> &'static str {
        match self {
            Self::Busy => "Settings are busy. Wait a moment and try again; existing data was preserved.",
            Self::Unavailable => "Settings recovery requires an authorized administrator or the owning Windows account. Existing data was preserved.",
        }
    }
    fn from_io(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::WouldBlock { Self::Busy } else { Self::Unavailable }
    }
}
impl LockedSelectedGeneration {
    fn acquire(root: &Path) -> Result<Self, String> { Self::acquire_for(root, StoreScope::Machine) }
    fn acquire_for(root: &Path, scope: StoreScope) -> Result<Self, String> {
        Self::acquire_for_detailed(root, scope).map_err(|e| e.message().to_string())
    }
    fn acquire_for_detailed(root: &Path, scope: StoreScope) -> Result<Self, OwnerAcquireError> {
        #[cfg(windows)]
        let root_pins = source_snapshot::pin_directories(root).map_err(|_| OwnerAcquireError::Unavailable)?;
        let ownership = match scope {
            StoreScope::Machine => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_machine(root, false),
            StoreScope::CurrentUser => super::settings_generation_ownership::SettingsGenerationLock::try_acquire_user(root, false),
        }.map_err(OwnerAcquireError::from_io)?;
        Ok(Self { _ownership: ownership, #[cfg(windows)] _root_pins: root_pins,
            path: root.join(".settings-generation.v1"), authority: root.join(".settings-cutover-anchor.v1"), scope })
    }

    pub(super) fn try_open(root: &Path) -> Result<Self, String> { Self::try_open_for(root, StoreScope::Machine) }
    pub(super) fn try_open_for(root: &Path, scope: StoreScope) -> Result<Self, String> {
        let selected = Self::acquire_for(root, scope)?;
        selected.read_reconciled()?;
        Ok(selected)
    }

    /// Explicit initial selection. Legacy files stay in place and the source
    /// snapshot must be fresh and held until independent anchor publication.
    #[cfg(windows)]
    fn initialize(snapshot: &mut source_snapshot::Snapshot, candidate: &[u8]) -> Result<Self, String> {
        let root = snapshot.root()?;
        let selected = Self::acquire_for(&root, snapshot.scope())?;
        let anchor = ProtectedFileAnchor::new(&root, selected.scope);
        let file = SelectedFile::new(&root);
        let guarded = std::cell::RefCell::new(snapshot);
        cutover::select_initial(&selected._ownership, &anchor, &file, candidate, &|bytes| guarded.borrow_mut().verify_generation(bytes))?;
        Ok(selected)
    }

    /// Interrupted first selection requires a fresh guarded source and the
    /// exact selected candidate; it never falls back to the legacy path.
    #[cfg(windows)]
    fn recover_initial(snapshot: &mut source_snapshot::Snapshot) -> Result<Self, String> {
        let root = snapshot.root()?;
        let candidate = read_generation_file(&root.join(".settings-generation.v1"), MAX_TOTAL * 2 + 65_536)?
            .ok_or("initial selected generation unavailable; retry with original candidate")?;
        Self::initialize(snapshot, &candidate)
    }

    fn read_reconciled(&self) -> Result<Vec<u8>, String> {
        let root = self.path.parent().ok_or("settings root unavailable")?;
        let anchor = ProtectedFileAnchor::new(root, self.scope);
        let file = SelectedFile::new(root);
        let verify = |bytes: &[u8]| verify_scoped_generation(self.scope, bytes);
        if matches!(anchor.read()?, Some(AnchorState::Updating { .. })) {
            cutover::recover_update(&self._ownership, &anchor, &file, &verify)?;
        }
        cutover::read_active(&self._ownership, &anchor, &file, &verify)
    }

    pub(super) fn read_all(&self) -> Result<BTreeMap<String, Value>, String> {
        open(&self.read_reconciled()?)
    }

    /// The exclusive owner covers the read, generation, write and readback.
    fn get_or_create_f6(&self, create: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
        let current = open_generation(&self.read_reconciled()?)?;
        if let Some(name) = matching_section(&current.sections, "f6-keystore") {
            let value = current.sections[name].clone();
            if !valid_f6(&value) { return Err("device signing key is invalid; existing data preserved".into()); }
            return Ok(value);
        }
        if current.f6_initialized { return Err("device signing key is missing; existing data preserved".into()); }
        let created = create()?;
        if !valid_f6(&created) { return Err("new device signing key is invalid".into()); }
        self.write_section("f6-keystore", created.clone())?;
        let committed = open_generation(&self.read_reconciled()?)?;
        let name = matching_section(&committed.sections, "f6-keystore").ok_or("committed device signing key missing")?;
        if committed.sections[name] != created { return Err("committed device signing key did not verify".into()); }
        Ok(created)
    }

    pub(super) fn write_section(&self, section: &str, value: Value) -> Result<(), String> {
        if section.eq_ignore_ascii_case("f6-keystore") { verify_machine_f6_creation_authority(self)?; }
        self.write_section_inner(section, value)
    }

    fn write_section_inner(&self, section: &str, value: Value) -> Result<(), String> {
        let current = self.read_reconciled()?;
        let candidate = updated_generation(&current, section, value)?;
        let root = self.path.parent().ok_or("settings root unavailable")?;
        let anchor = ProtectedFileAnchor::new(root, self.scope);
        let file = SelectedFile::new(root);
        cutover::update(&self._ownership, &anchor, &file, &candidate, &|bytes| verify_scoped_generation(self.scope, bytes))
    }

    /// Explicit offline recovery only: never replace an existing identity.
    fn create_new_f6_confirmed(&self, create: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
        if self.scope != StoreScope::Machine { return Err("Machine identity recovery requires the machine settings store.".into()); }
        let generation = open_generation(&self.read_reconciled()?)?;
        if generation.f6_initialized || matching_section(&generation.sections, "f6-keystore").is_some() {
            return Err("A device signing identity already exists; it was not replaced.".into());
        }
        let value = create()?;
        if !valid_f6(&value) { return Err("New device signing identity is invalid.".into()); }
        self.write_section_inner("f6-keystore", value.clone())?;
        let committed = self.read_all()?;
        let name = matching_section(&committed, "f6-keystore").ok_or("New device signing identity was not committed")?;
        if committed[name] != value { return Err("New device signing identity did not verify".into()); }
        Ok(value)
    }
}
