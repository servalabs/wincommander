impl Snapshot {
    /// Machine-only entry point. The inherited machine-data resolver uses the
    /// Windows known-folder API. Production cutover must establish that this
    /// root has the same file identity as the live datastore root; this trial
    /// checks the current process's resolver and pins both roots. Production
    /// must also verify authorized directory ACLs and fail closed when install mode is
    /// uncertain: the existing install-mode test can fall back to machine mode.
    /// These requirements are not implemented by this test-only entry point.
    pub(super) fn capture_machine() -> Result<Self, String> {
        if crate::paths::current_user_install_uses_local_datastore() {
            return Err("machine settings rotation is unavailable for a current-user install".into());
        }
        let root = known_root(StoreScope::Machine)?;
        let root_pins = pin_matching_roots(&root, StoreScope::Machine)?;
        let mut snapshot = Self::capture(&root)?;
        snapshot._directories.extend(root_pins);
        Ok(snapshot)
    }

    /// Current-user install only; never falls back to a machine-store root.
    /// ACL authorization and reliable noninteractive DPAPI remain open.
    pub(super) fn capture_user() -> Result<Self, String> {
        if !crate::paths::current_user_install_uses_local_datastore() {
            return Err("current-user settings rotation is unavailable for this install".into());
        }
        Self::capture_user_from_verified_helper()
    }

    /// Caller verifies the original-user token and pins the installed app before
    /// using a temporary helper outside the installation directory.
    pub(super) fn capture_user_from_verified_helper() -> Result<Self, String> {
        let root = known_root(StoreScope::CurrentUser)?;
        let root_pins = pin_matching_roots(&root, StoreScope::CurrentUser)?;
        let mut snapshot = Self::capture_for(&root, StoreScope::CurrentUser)?;
        snapshot._directories.extend(root_pins);
        Ok(snapshot)
    }

    /// Test fixture / internal seam. Caller must independently authorize root.
    /// The key and every section are read once while write/delete sharing is denied.
    fn capture(root: &Path) -> Result<Self, String> { Self::capture_for(root, StoreScope::Machine) }
    fn capture_for(root: &Path, scope: StoreScope) -> Result<Self, String> {
        let store = root.join("store");
        let pins = pin_directories(&store)?;
        let key_name = match scope { StoreScope::Machine => ".install.material", StoreScope::CurrentUser => ".user-store.material" };
        let mut key_file = open_locked(&root.join(key_name), MAX_KEY)?;
        let raw = Zeroizing::new(read_locked(&mut key_file, MAX_KEY)?);
        let key_digest = Sha256::digest(&raw[..]).into();
        let plain = match scope {
            StoreScope::Machine if raw.len() == 32 => Zeroizing::new(raw.to_vec()),
            StoreScope::Machine => Zeroizing::new(dpapi_unprotect(&raw).map_err(|_| UNAVAILABLE)?),
            StoreScope::CurrentUser => Zeroizing::new(crate::datastore::unprotect_user_store_material(&raw, true).map_err(|_| UNAVAILABLE)?),
        };
        let mut material = Zeroizing::new([0u8; 32]);
        if plain.len() != material.len() { return Err(UNAVAILABLE.into()); }
        material.copy_from_slice(&plain);
        let mut sections = Vec::new();
        let mut total = 0usize;
        for (name, path) in enumerate(&store)? {
            let mut file = open_locked(&path, MAX_TOTAL as u64)?;
            let bytes = read_locked(&mut file, MAX_TOTAL as u64)?;
            total = total.checked_add(bytes.len()).ok_or(UNAVAILABLE)?;
            if total > MAX_TOTAL { return Err(UNAVAILABLE.into()); }
            let digest = Sha256::digest(&bytes).into();
            let encoded = String::from_utf8(bytes).map_err(|_| UNAVAILABLE)?;
            sections.push(Section { name, file, digest, encoded });
        }
        let mut snapshot = Self { store, scope, _directories: pins, key_file, key_digest, material, sections };
        snapshot.verify_unchanged()?;
        Ok(snapshot)
    }

    pub(super) fn scope(&self) -> StoreScope { self.scope }
    pub(super) fn root(&self) -> Result<PathBuf, String> {
        self.store.parent().map(Path::to_path_buf).ok_or_else(|| UNAVAILABLE.into())
    }

    pub(super) fn verify_unchanged(&mut self) -> Result<(), String> {
        let raw_key = Zeroizing::new(read_locked(&mut self.key_file, MAX_KEY)?);
        let observed_key: [u8; 32] = Sha256::digest(&raw_key[..]).into();
        if observed_key != self.key_digest { return Err(UNAVAILABLE.into()); }
        let current = enumerate(&self.store)?;
        if current.len() != self.sections.len()
            || current.iter().zip(&self.sections).any(|((name, _), section)| name != &section.name) {
            return Err(UNAVAILABLE.into());
        }
        for section in &mut self.sections {
            let bytes = read_locked(&mut section.file, MAX_TOTAL as u64)?;
            let observed: [u8; 32] = Sha256::digest(&bytes).into();
            if observed != section.digest { return Err(UNAVAILABLE.into()); }
        }
        Ok(())
    }

    /// Build the complete new generation while the guard remains alive.
    /// A second verification narrows source-change races, but is not a
    /// substitute for deployment-wide fencing or crash-safe activation.
    pub(super) fn prepare(&mut self) -> Result<Vec<u8>, String> {
        self.verify_unchanged()?;
        let sections: Vec<_> = self.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        let output = prepare_for(self.scope, &self.material, &sections)?;
        self.verify_unchanged()?;
        Ok(output)
    }

    /// Compare a reopened generation with a freshly guarded, complete source
    /// snapshot. This detects observed changes, not writers racing afterward.
    pub(super) fn verify_generation(&mut self, generation: &[u8]) -> Result<(), String> {
        self.verify_unchanged()?;
        let sections: Vec<_> = self.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        verify_source_for(self.scope, generation, &self.material, &sections)?;
        self.verify_unchanged()
    }

    /// Stage at the reserved filename under the pinned source root. The
    /// returned value holds the source and stage handles through verification;
    /// it does not activate or authorize a production cutover.
    pub(super) fn stage_unactivated(&mut self) -> Result<GuardedStage<'_>, String> {
        self.stage_unactivated_with_hook(&|_, _| Ok(()))
    }

    /// Inspect a prior stage after restart against the complete current source.
    /// A mismatch or partial write is left untouched for manual investigation.
    pub(super) fn recover_unactivated(&mut self) -> Result<GuardedStage<'_>, String> {
        if self.scope != StoreScope::Machine { return Err("user staged recovery is not available".into()); }
        self.verify_unchanged()?;
        let sections: Vec<_> = self.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        let stage_path = self.store.parent().ok_or(UNAVAILABLE)?.join(STAGED_NAME);
        let (stage, output) = recover_stage(&stage_path, &self.material, &sections)?;
        self.verify_generation(&output)?;
        Ok(GuardedStage { snapshot: self, stage, output })
    }

    fn stage_unactivated_with_hook(
        &mut self,
        after_write: &impl Fn(&Path, &mut File) -> Result<(), String>,
    ) -> Result<GuardedStage<'_>, String> {
        if self.scope != StoreScope::Machine { return Err("user staging is not available".into()); }
        let output = self.prepare()?;
        self.verify_generation(&output)?;
        let sections: Vec<_> = self.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        let stage_path = self.store.parent().ok_or(UNAVAILABLE)?.join(STAGED_NAME);
        let stage = stage_new_with_hook(&stage_path, &output, &self.material, &sections, after_write)?;
        self.verify_generation(&output)?;
        Ok(GuardedStage { snapshot: self, stage, output })
    }
}

/// This borrow keeps the snapshot and stage handles alive together. Dropping
/// it ends that protection; it is not a durable restart or activation token.
pub(super) struct GuardedStage<'a> {
    snapshot: &'a mut Snapshot,
    stage: StagedFile,
    output: Vec<u8>,
}

impl GuardedStage<'_> {
    pub(super) fn verify(&mut self) -> Result<(), String> {
        self.snapshot.verify_generation(&self.output)?;
        let sections: Vec<_> = self.snapshot.sections.iter().map(|s| (s.name.clone(), s.encoded.clone())).collect();
        self.stage.verify(&self.output, &self.snapshot.material, &sections)?;
        self.snapshot.verify_generation(&self.output)
    }
}
