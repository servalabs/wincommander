// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only migration sequence, with no real storage or production switch.
//! The caller owns an exclusive cross-process lease for this entire operation.
use std::io;

#[derive(Debug, PartialEq, Eq)]
enum Outcome { Published, AlreadyPublished, InspectPublication }
trait MigrationBackend {
    // True only for an authenticated marker bound to this index and key;
    // malformed, inaccessible, or mismatched markers are errors. The check
    // must run under the same lease used by all old and new writers.
    fn publication_exists(&mut self) -> io::Result<bool>;
    fn protected_key_ready(&mut self) -> io::Result<bool>;
    // Must bind the exact source and each change while the lease is held.
    fn source_revision(&mut self) -> io::Result<[u8; 32]>;
    // Build only in a separate destination; never touch the old index.
    fn build_candidate(&mut self, source: &[u8; 32]) -> io::Result<()>;
    // A real reader must authenticate and query the built candidate.
    fn verify_candidate(&mut self, source: &[u8; 32]) -> io::Result<bool>;
    // Durable, authenticated, atomic publication. Any error may have committed.
    fn publish(&mut self, source: &[u8; 32]) -> io::Result<()>;
}
fn rejected(msg: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, msg) }
fn migrate(backend: &mut impl MigrationBackend) -> io::Result<Outcome> {
    // This seam cannot itself prove the lease, storage durability or that the
    // publication marker authenticates freshness. Real integration must.
    if backend.publication_exists()? { return Ok(Outcome::AlreadyPublished); }
    if !backend.protected_key_ready()? { return Err(rejected("protected key unavailable")); }
    let before = backend.source_revision()?;
    backend.build_candidate(&before)?;
    if !backend.verify_candidate(&before)? { return Err(rejected("candidate verification failed")); }
    if backend.source_revision()? != before { return Err(rejected("source changed during rebuild")); }
    // Do not infer rollback when a publication operation reports an error.
    match backend.publish(&before) {
        Ok(()) => Ok(Outcome::Published),
        Err(_) => Ok(Outcome::InspectPublication),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        published: bool, key: bool, bad_candidate: bool, changed: bool,
        build_fails: bool, bad_marker: bool, publish_fails: bool, publish_before_error: bool,
        reads: u8, built: bool, old_present: bool,
    }
    impl MigrationBackend for Fake {
        fn publication_exists(&mut self) -> io::Result<bool> { if self.bad_marker { return Err(rejected("invalid marker")); } Ok(self.published) }
        fn protected_key_ready(&mut self) -> io::Result<bool> { Ok(self.key) }
        fn source_revision(&mut self) -> io::Result<[u8; 32]> {
            self.reads += 1; Ok(if self.changed && self.reads > 1 { [2; 32] } else { [1; 32] })
        }
        fn build_candidate(&mut self, _: &[u8; 32]) -> io::Result<()> {
            self.built = true;
            if self.build_fails { return Err(rejected("build failed")); } Ok(())
        }
        fn verify_candidate(&mut self, _: &[u8; 32]) -> io::Result<bool> { Ok(!self.bad_candidate) }
        fn publish(&mut self, _: &[u8; 32]) -> io::Result<()> {
            if self.publish_before_error || !self.publish_fails { self.published = true; }
            if self.publish_fails { return Err(rejected("unknown commit")); } Ok(())
        }
    }
    fn ready() -> Fake { Fake { key: true, old_present: true, ..Default::default() } }
    #[test]
    fn verified_candidate_can_be_published_but_old_data_is_retained() {
        let mut store = ready();
        assert_eq!(migrate(&mut store).unwrap(), Outcome::Published);
        assert!(store.published && store.built && store.old_present);
        assert_eq!(migrate(&mut store).unwrap(), Outcome::AlreadyPublished);
    }
    #[test]
    fn missing_key_failed_build_bad_candidate_or_source_change_never_publish() {
        for scenario in 0..5 {
            let mut store = ready();
            match scenario { 0 => store.key = false, 1 => store.build_fails = true,
                2 => store.bad_candidate = true, 3 => store.changed = true, _ => store.bad_marker = true }
            assert!(migrate(&mut store).is_err());
            assert!(!store.published && store.old_present);
        }
    }
    #[test]
    fn failed_publication_requires_inspection_even_if_it_already_committed() {
        for committed in [false, true] {
            let mut store = ready(); store.publish_fails = true; store.publish_before_error = committed;
            assert_eq!(migrate(&mut store).unwrap(), Outcome::InspectPublication);
            assert_eq!(store.published, committed);
            assert!(store.old_present);
        }
    }
}
