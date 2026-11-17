// SPDX-License-Identifier: AGPL-3.0-or-later
//! Remembers the SHA-256 of a protected image so the large executable is not
//! re-read on every Fleet policy call.
//!
//! The digest is reused only while the file's identity is unchanged: same
//! path, file ID (volume + index), byte length, last-write time, NTFS change
//! time and security-descriptor fingerprint. The caller still performs the
//! full access-control verification on every call; this cache only skips
//! re-reading file bytes that have not been written (a writer changes the
//! length, write time or change time; a permission change alters the change
//! time and the fingerprint; each forces a rehash).

use crate::peer_auth::PeerAuthError;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};

const MAX_ENTRIES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageIdentity {
    pub(crate) path: PathBuf,
    pub(crate) volume: u64,
    pub(crate) file_index: u64,
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    pub(crate) change_time: i64,
    /// SHA-256 over the owner and DACL of the opened file.
    pub(crate) acl: [u8; 32],
}

#[derive(Default)]
pub(crate) struct DigestCache {
    entries: Vec<(ImageIdentity, Vec<u8>)>,
}

impl DigestCache {
    pub(crate) fn lookup(&self, identity: &ImageIdentity) -> Option<Vec<u8>> {
        self.entries
            .iter()
            .find(|(known, _)| known == identity)
            .map(|(_, digest)| digest.clone())
    }

    pub(crate) fn insert(&mut self, identity: ImageIdentity, digest: Vec<u8>) {
        // A changed identity for a known path replaces its stale entry.
        self.entries.retain(|(known, _)| known.path != identity.path);
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push((identity, digest));
    }

    /// Return the remembered digest for exactly this identity, or compute,
    /// remember and return it. A failed computation is never remembered.
    #[cfg(test)]
    pub(crate) fn get_or_compute(
        &mut self,
        identity: ImageIdentity,
        compute: impl FnOnce() -> Result<Vec<u8>, PeerAuthError>,
    ) -> Result<Vec<u8>, PeerAuthError> {
        if let Some(digest) = self.lookup(&identity) {
            return Ok(digest);
        }
        let digest = compute()?;
        self.insert(identity, digest.clone());
        Ok(digest)
    }
}

struct Shared {
    cache: DigestCache,
    /// One gate per image path: concurrent callers for the same image wait
    /// for the first hash instead of repeating it, while the map lock itself
    /// is never held during hashing.
    gates: HashMap<PathBuf, Arc<Mutex<()>>>,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

fn with_shared<T>(action: impl FnOnce(&mut Shared) -> T) -> Option<T> {
    let mut guard = SHARED.lock().ok()?;
    let shared = guard.get_or_insert_with(|| Shared {
        cache: DigestCache::default(),
        gates: HashMap::new(),
    });
    Some(action(shared))
}

/// Process-wide lookup used by the pipe authorization path.
pub(crate) fn cached_digest(
    identity: ImageIdentity,
    compute: impl FnOnce() -> Result<Vec<u8>, PeerAuthError>,
) -> Result<Vec<u8>, PeerAuthError> {
    let Some(gate) = with_shared(|shared| {
        if shared.gates.len() > 4 * MAX_ENTRIES {
            shared.gates.clear();
        }
        Arc::clone(shared.gates.entry(identity.path.clone()).or_default())
    }) else {
        // A poisoned cache is never trusted: hash directly.
        return compute();
    };
    let Ok(_serialized) = gate.lock() else {
        return compute();
    };
    if let Some(Some(digest)) = with_shared(|shared| shared.cache.lookup(&identity)) {
        return Ok(digest);
    }
    let digest = compute()?;
    with_shared(|shared| shared.cache.insert(identity, digest.clone()));
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, time::Duration};

    fn identity() -> ImageIdentity {
        ImageIdentity {
            path: PathBuf::from(r"C:\ProgramData\WinCommander\dev-fleet\wincommander-free.exe"),
            volume: 3,
            file_index: 42,
            size: 88_000_000,
            change_time: 500,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000)),
            acl: [7; 32],
        }
    }

    fn digest(cache: &mut DigestCache, identity: ImageIdentity, runs: &Cell<u32>, value: u8) -> Vec<u8> {
        cache
            .get_or_compute(identity, || {
                runs.set(runs.get() + 1);
                Ok(vec![value; 32])
            })
            .unwrap()
    }

    #[test]
    fn an_unchanged_image_is_hashed_once() {
        let (mut cache, runs) = (DigestCache::default(), Cell::new(0));
        assert_eq!(digest(&mut cache, identity(), &runs, 1), vec![1; 32]);
        // The second call is served from the cache even if hashing would now differ.
        assert_eq!(digest(&mut cache, identity(), &runs, 2), vec![1; 32]);
        assert_eq!(runs.get(), 1);
    }

    #[test]
    fn a_changed_size_modification_time_or_acl_forces_a_rehash() {
        let (mut cache, runs) = (DigestCache::default(), Cell::new(0));
        digest(&mut cache, identity(), &runs, 1);
        let mut resized = identity();
        resized.size += 1;
        assert_eq!(digest(&mut cache, resized, &runs, 2), vec![2; 32]);
        let mut touched = identity();
        touched.modified = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(2_000));
        assert_eq!(digest(&mut cache, touched, &runs, 3), vec![3; 32]);
        let mut reprotected = identity();
        reprotected.acl = [9; 32];
        assert_eq!(digest(&mut cache, reprotected, &runs, 4), vec![4; 32]);
        assert_eq!(runs.get(), 4);
        // The superseded identities are gone: going back rehashes again.
        assert_eq!(digest(&mut cache, identity(), &runs, 5), vec![5; 32]);
        assert_eq!(runs.get(), 5);
    }

    #[test]
    fn a_replaced_file_or_new_change_time_forces_a_rehash() {
        let (mut cache, runs) = (DigestCache::default(), Cell::new(0));
        digest(&mut cache, identity(), &runs, 1);
        // Same path, contents "restored" to the same size and write time, but
        // a different file object (index) or a later NTFS change time.
        let mut swapped = identity();
        swapped.file_index += 1;
        assert_eq!(digest(&mut cache, swapped, &runs, 2), vec![2; 32]);
        let mut retimed = identity();
        retimed.change_time += 1;
        assert_eq!(digest(&mut cache, retimed, &runs, 3), vec![3; 32]);
        let mut other_volume = identity();
        other_volume.volume += 1;
        assert_eq!(digest(&mut cache, other_volume, &runs, 4), vec![4; 32]);
        assert_eq!(runs.get(), 4);
    }

    #[test]
    fn concurrent_callers_for_one_image_hash_it_once() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        let runs = Arc::new(AtomicU32::new(0));
        let mut each = identity();
        each.path = PathBuf::from(r"C:\concurrent-image.exe");
        let workers: Vec<_> = (0..6)
            .map(|_| {
                let (runs, each) = (Arc::clone(&runs), each.clone());
                std::thread::spawn(move || {
                    cached_digest(each, || {
                        runs.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        Ok(vec![9; 32])
                    })
                    .unwrap()
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(worker.join().unwrap(), vec![9; 32]);
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_hash_is_not_remembered_and_paths_are_independent() {
        let mut cache = DigestCache::default();
        let failed = cache.get_or_compute(identity(), || Err(PeerAuthError::IdentityUnavailable));
        assert_eq!(failed, Err(PeerAuthError::IdentityUnavailable));
        let runs = Cell::new(0);
        assert_eq!(digest(&mut cache, identity(), &runs, 1), vec![1; 32]);
        let mut other = identity();
        other.path = PathBuf::from(r"C:\ProgramData\WinCommander\bin\wincommander-pro.exe");
        assert_eq!(digest(&mut cache, other.clone(), &runs, 2), vec![2; 32]);
        assert_eq!(digest(&mut cache, identity(), &runs, 3), vec![1; 32]);
        assert_eq!(digest(&mut cache, other, &runs, 3), vec![2; 32]);
        assert_eq!(runs.get(), 2);
    }

    #[test]
    fn the_cache_is_bounded() {
        let (mut cache, runs) = (DigestCache::default(), Cell::new(0));
        for index in 0..(MAX_ENTRIES as u32 + 3) {
            let mut each = identity();
            each.path = PathBuf::from(format!(r"C:\image-{index}.exe"));
            digest(&mut cache, each, &runs, 1);
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
    }
}
