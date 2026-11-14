// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental admission contract for a future paired ordinary search index.
//!
//! This does not implement a protected selector, authenticate an authority,
//! prevent rollback, acquire an OS lease, migrate indexes, or enable search.
//! It must not be used until those responsibilities are supplied independently
//! by Free and Pro, including a shared authority that serializes updates and
//! publication. Legacy indexes and all older generations must be retained.
use std::io;

/// Obtained from independently authenticated, rollback-resistant state; never
/// derive authority solely from the selector or directory being opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeGeneration {
    pub generation: [u8; 16],
    pub scope_digest: [u8; 32],
    pub revision: u64,
}

impl ScopeGeneration {
    fn valid(&self) -> bool {
        self.generation != [0; 16] && self.scope_digest != [0; 32] && self.revision != 0
    }
}

/// The implementation MUST serialize this callback with authority updates,
/// use authenticated same-user state, resist rollback, and fail on missing or
/// untrusted state. Revisions must advance for every scope or generation
/// transition and must never be reused, including after reset or restart.
/// DPAPI or an index-local record alone does not satisfy this.
pub trait ScopeAuthority {
    fn with_current<T>(&self, f: impl FnOnce(&ScopeGeneration) -> io::Result<T>) -> io::Result<T>;
}

fn denied() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "search generation unavailable")
}

/// Keep the caller-provided lease alive while searching and through publication.
/// Acquire must bind the opened index handle to the verified generation using
/// reparse-safe handle containment and obtain the real shared OS lease. The
/// publication callback must be bounded and must not re-enter the
/// authority. No lock here stops an already-running unfenced legacy process.
pub fn search_and_publish<A, L, T, R>(
    authority: &A,
    selected: ScopeGeneration,
    acquire: impl FnOnce(&ScopeGeneration) -> io::Result<L>,
    search: impl FnOnce(&L) -> io::Result<T>,
    publish: impl FnOnce(T) -> io::Result<R>,
) -> io::Result<R>
where
    A: ScopeAuthority,
{
    if !selected.valid() { return Err(denied()); }
    authority.with_current(|current| if current.valid() && *current == selected { Ok(()) } else { Err(denied()) })?;
    let lease = acquire(&selected)?;
    authority.with_current(|current| if current.valid() && *current == selected { Ok(()) } else { Err(denied()) })?;
    let results = search(&lease)?;
    authority.with_current(|current| {
        if !current.valid() || *current != selected { return Err(denied()); }
        publish(results)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};

    struct Authority(Mutex<Option<ScopeGeneration>>);
    impl ScopeAuthority for Authority {
        fn with_current<T>(&self, f: impl FnOnce(&ScopeGeneration) -> io::Result<T>) -> io::Result<T> {
            let state = self.0.lock().map_err(|_| denied())?;
            f(state.as_ref().ok_or_else(denied)?)
        }
    }
    fn generation() -> ScopeGeneration { ScopeGeneration { generation: [1;16], scope_digest: [2;32], revision: 1 } }
    struct Lease<'a>(&'a AtomicBool);
    impl Drop for Lease<'_> { fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); } }

    #[test]
    fn lease_covers_search_and_locked_publication() {
        let authority=Authority(Mutex::new(Some(generation())));
        let dropped=AtomicBool::new(false);
        let answer=search_and_publish(&authority,generation(),|_|Ok(Lease(&dropped)),|_| {
            assert!(!dropped.load(Ordering::SeqCst)); Ok(7)
        },|result| {
            assert!(!dropped.load(Ordering::SeqCst));
            assert!(authority.0.try_lock().is_err());
            Ok(result)
        }).unwrap();
        assert_eq!(answer,7); assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn stale_selection_or_missing_authority_never_opens() {
        let authority=Authority(Mutex::new(None));
        let opened=AtomicBool::new(false);
        let run=|selected| search_and_publish(&authority,selected,|_| { opened.store(true,Ordering::SeqCst); Ok(()) },|_|Ok(()),|_|Ok(()));
        assert!(run(generation()).is_err());
        *authority.0.lock().unwrap()=Some(generation());
        let mut stale=generation();stale.revision+=1;
        assert!(run(stale).is_err());
        stale=generation();stale.scope_digest[0]^=1;
        assert!(run(stale).is_err());
        stale=generation();stale.generation=[0;16];
        assert!(run(stale).is_err());
        assert!(!opened.load(Ordering::SeqCst));
    }

    #[test]
    fn revocation_during_acquire_or_search_never_publishes_and_releases_lease() {
        for during_acquire in [true,false] {
            let authority=Authority(Mutex::new(Some(generation())));
            let dropped=AtomicBool::new(false);let published=AtomicBool::new(false);
            let result=search_and_publish(&authority,generation(),|_| {
                if during_acquire { *authority.0.lock().unwrap()=None; }
                Ok(Lease(&dropped))
            },|_| {
                *authority.0.lock().unwrap()=None; Ok(())
            },|_| { published.store(true,Ordering::SeqCst); Ok(()) });
            assert!(result.is_err()); assert!(dropped.load(Ordering::SeqCst));
            assert!(!published.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn concurrent_scope_update_waits_for_publication() {
        let authority=Authority(Mutex::new(Some(generation())));
        std::thread::scope(|scope| {
            let (started_tx,started_rx)=std::sync::mpsc::channel();
            let (done_tx,done_rx)=std::sync::mpsc::channel();
            let mut worker=None;
            search_and_publish(&authority,generation(),|_|Ok(()),|_|Ok(()),|_| {
                let shared=&authority;
                worker=Some(scope.spawn(move || {
                    started_tx.send(()).unwrap();
                    *shared.0.lock().unwrap()=None;
                    done_tx.send(()).unwrap();
                }));
                started_rx.recv().unwrap();
                assert!(done_rx.try_recv().is_err());
                Ok(())
            }).unwrap();
            worker.unwrap().join().unwrap();
            done_rx.recv().unwrap();
        });
    }
}
