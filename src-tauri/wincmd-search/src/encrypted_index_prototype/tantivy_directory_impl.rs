impl Directory for TrialDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let source = self.inner.open_read_pinned(path)?;
        let file = ProtectedFile::open(
            source,
            self.key.key(),
            name(path).map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?,
        )
        .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
        let len = file.len;
        Ok(Arc::new(TrialHandle {
            file: Mutex::new(file),
            len,
            _key_owner: Arc::clone(&self.key),
        }))
    }
    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        self.inner.delete_pinned(path)
    }
    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        self.inner.exists(path)
    }
    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let path_name = name(path).map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?.to_owned();
        let sink = self.inner.open_write_pinned(path)?;
        let writer = TrialWriter::new(sink, Arc::clone(&self.key), path_name).map_err(|e| OpenWriteError::wrap_io_error(e, path.into()))?;
        Ok(BufWriter::new(Box::new(writer)))
    }
    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        let bytes = self.inner.atomic_read_bounded(path, MAX_ATOMIC_CIPHERTEXT)?;
        let mut f = ProtectedFile::open(
            io::Cursor::new(bytes),
            self.key.key(),
            name(path).map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?,
        )
        .map_err(|e| OpenReadError::wrap_io_error(e, path.into()))?;
        if f.len > MAX_ATOMIC_PLAINTEXT {
            return Err(OpenReadError::wrap_io_error(
                io::Error::other("index manifest exceeds limit"),
                path.into(),
            ));
        }
        f.read(0..f.len).map_err(|e| OpenReadError::wrap_io_error(e, path.into()))
    }
    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        if data.len() > MAX_ATOMIC_PLAINTEXT {
            return Err(io::Error::other("index manifest exceeds limit"));
        }
        let mut encrypted = Vec::new();
        encrypt(&mut encrypted, self.key.key(), name(path)?, data)?;
        // Windows can briefly refuse atomic replacement while a shared
        // reader has the old manifest open. Preserve a permanent failure.
        #[cfg(windows)]
        for delay in [2, 4, 8, 16, 32, 64, 128] {
            match self.inner.atomic_write_pinned(path, &encrypted) {
                Err(e) if e.raw_os_error() == Some(5) => std::thread::sleep(std::time::Duration::from_millis(delay)),
                result => return result,
            }
        }
        self.inner.atomic_write_pinned(path, &encrypted)
    }
    fn sync_directory(&self) -> io::Result<()> {
        self.inner.sync_directory()
    }
    fn acquire_lock(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
        self.inner.acquire_lock_pinned(lock)
    }
    fn watch(&self, callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        self.inner.watch(callback)
    }
}
