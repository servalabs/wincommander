// SPDX-License-Identifier: AGPL-3.0-or-later
//! Remembers the SHA-256 of a protected image so the large executable is not
//! re-read on every Fleet policy call.
//!
//! The digest is reused only while the file's identity is unchanged: same
//! path, byte length, modification time and security-descriptor fingerprint.
//! The caller still performs the full access-control verification on every
//! call; this cache only skips re-reading file bytes that provably have not
//! been written (any writer changes the length or modification time, and a
//! permission change changes the fingerprint, each of which forces a rehash).

use crate::peer_auth::PeerAuthError;
use std::{path::PathBuf, sync::Mutex, time::SystemTime};

const MAX_ENTRIES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageIdentity {
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    /// SHA-256 over the owner and DACL of the opened file.
    pub(crate) acl: [u8; 32],
}

#[derive(Default)]
pub(crate) struct DigestCache {
    entries: Vec<(ImageIdentity, Vec<u8>)>,
}

impl DigestCache {
    /// Return the remembered digest for exactly this identity, or compute,
    /// remember and return it. A failed computation is never remembered.
    pub(crate) fn get_or_compute(
        &mut self,
        identity: ImageIdentity,
        compute: impl FnOnce() -> Result<Vec<u8>, PeerAuthError>,
    ) -> Result<Vec<u8>, PeerAuthError> {
        if let Some((_, digest)) = self.entries.iter().find(|(known, _)| *known == identity) {
            return Ok(digest.clone());
        }
        let digest = compute()?;
        // A changed identity for a known path replaces its stale entry.
        self.entries.retain(|(known, _)| known.path != identity.path);
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push((identity, digest.clone()));
        Ok(digest)
    }
}

static CACHE: Mutex<DigestCache> = Mutex::new(DigestCache { entries: Vec::new() });

/// Process-wide lookup used by the pipe authorization path.
pub(crate) fn cached_digest(
    identity: ImageIdentity,
    compute: impl FnOnce() -> Result<Vec<u8>, PeerAuthError>,
) -> Result<Vec<u8>, PeerAuthError> {
    match CACHE.lock() {
        Ok(mut cache) => cache.get_or_compute(identity, compute),
        // A poisoned cache is never trusted: hash directly.
        Err(_) => compute(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, time::Duration};

    fn identity() -> ImageIdentity {
        ImageIdentity {
            path: PathBuf::from(r"C:\ProgramData\WinCommander\dev-fleet\wincommander-free.exe"),
            size: 88_000_000,
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
