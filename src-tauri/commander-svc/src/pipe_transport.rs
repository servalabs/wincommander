// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::Semaphore,
    time::timeout,
};
use wincmd_shared::{Envelope, MAX_PAYLOAD_BYTES};

const MAX_CONNECTIONS: usize = 32;
pub(crate) const MAX_FRAMES_PER_CONNECTION: usize = 128;
const HELLO_MAX_BYTES: u32 = 4 * 1024;
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Concurrent connections one client SID may hold (the global cap still applies).
const MAX_CONNECTIONS_PER_SID: usize = 8;
/// Separate, larger allowance for SYSTEM/administrator callers so a standard
/// user exhausting its own quota can never block them (and vice versa).
const MAX_CONNECTIONS_PER_PRIVILEGED_SID: usize = 16;
/// Total wall-clock lifetime of one connection; callers can reconnect. This
/// bounds a client that trickles one frame per idle window forever.
pub(crate) const MAX_CONNECTION_LIFETIME: Duration = Duration::from_secs(300);

pub(crate) fn connection_slots() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(MAX_CONNECTIONS))
}

/// Per-client-SID concurrent connection accounting.
pub(crate) struct ClientQuota {
    counts: Mutex<HashMap<String, usize>>,
    limit: usize,
    privileged_limit: usize,
}

/// Releases one quota unit when the connection ends.
pub(crate) struct ClientQuotaPermit {
    quota: Arc<ClientQuota>,
    sid: String,
}

impl Drop for ClientQuotaPermit {
    fn drop(&mut self) {
        let mut counts = self.quota.counts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = counts.get_mut(&self.sid) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&self.sid);
            }
        }
    }
}

impl ClientQuota {
    fn new(limit: usize, privileged_limit: usize) -> Arc<Self> {
        Arc::new(Self {
            counts: Mutex::new(HashMap::new()),
            limit,
            privileged_limit,
        })
    }

    /// `None` when `sid` already holds its allowance (or the SID is empty,
    /// which would otherwise let unattributed callers share one bucket).
    pub(crate) fn try_acquire(
        self: &Arc<Self>,
        sid: &str,
        privileged: bool,
    ) -> Option<ClientQuotaPermit> {
        if sid.is_empty() {
            return None;
        }
        let cap = if privileged {
            self.privileged_limit
        } else {
            self.limit
        };
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        let count = counts.entry(sid.to_string()).or_insert(0);
        if *count >= cap {
            return None;
        }
        *count += 1;
        Some(ClientQuotaPermit {
            quota: Arc::clone(self),
            sid: sid.to_string(),
        })
    }
}

/// Process-wide quota shared by every pipe connection task.
pub(crate) fn client_quota() -> &'static Arc<ClientQuota> {
    static QUOTA: OnceLock<Arc<ClientQuota>> = OnceLock::new();
    QUOTA.get_or_init(|| {
        ClientQuota::new(MAX_CONNECTIONS_PER_SID, MAX_CONNECTIONS_PER_PRIVILEGED_SID)
    })
}

/// Time left before `started + max` at `now`; `None` once it has elapsed.
pub(crate) fn remaining_lifetime(
    started: Instant,
    now: Instant,
    max: Duration,
) -> Option<Duration> {
    max.checked_sub(now.saturating_duration_since(started))
        .filter(|left| !left.is_zero())
}

/// Like [`read_frame`] but never waits past `limit` (the connection's
/// remaining lifetime).
pub(crate) async fn read_frame_within<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: Duration,
) -> io::Result<Envelope> {
    read(reader, MAX_PAYLOAD_BYTES, FRAME_TIMEOUT.min(limit)).await
}

// Every transport timeout is terminal. Never retry read_exact on the same
// stream after cancellation, because a partial header/body may be consumed.
async fn read<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: u32,
    deadline: Duration,
) -> io::Result<Envelope> {
    timeout(
        deadline,
        wincmd_shared::read_envelope_limited(reader, limit),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "service frame read timed out"))?
}

pub(crate) async fn read_hello<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Envelope> {
    read(reader, HELLO_MAX_BYTES, HELLO_TIMEOUT).await
}

#[cfg(test)]
pub(crate) async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Envelope> {
    read(reader, MAX_PAYLOAD_BYTES, FRAME_TIMEOUT).await
}

pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    envelope: &Envelope,
) -> io::Result<()> {
    timeout(
        WRITE_TIMEOUT,
        wincmd_shared::write_envelope(writer, envelope),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "service frame write timed out"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncWriteExt};

    #[tokio::test(start_paused = true)]
    async fn silent_client_cannot_hold_a_handshake_open() {
        let (_client, mut server) = duplex(64);
        assert_eq!(
            read_hello(&mut server).await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[tokio::test(start_paused = true)]
    async fn partial_header_and_body_are_terminal_timeouts() {
        for prefix in [vec![8, 0], vec![8, 0, 0, 0, b'{']] {
            let (mut client, mut server) = duplex(64);
            client.write_all(&prefix).await.unwrap();
            assert_eq!(
                read_hello(&mut server).await.unwrap_err().kind(),
                io::ErrorKind::TimedOut
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn oversized_hello_is_rejected_without_waiting_for_its_body() {
        let (mut client, mut server) = duplex(64);
        client.write_u32_le(HELLO_MAX_BYTES + 1).await.unwrap();
        assert_eq!(
            read_hello(&mut server).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test(start_paused = true)]
    async fn idle_post_handshake_connections_expire() {
        let (_client, mut server) = duplex(64);
        assert_eq!(
            read_frame(&mut server).await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_nonreading_client_cannot_block_response_writes_forever() {
        let (_client, mut server) = duplex(1);
        assert_eq!(
            write_frame(&mut server, &Envelope::Bye)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[tokio::test]
    async fn normal_hello_and_signed_requests_keep_the_wire_contract() {
        let (mut client, mut server) = duplex(4096);
        let hello = Envelope::Hello(wincmd_shared::svc::hello_from_ui("test-session"));
        write_frame(&mut client, &hello).await.unwrap();
        assert!(matches!(
            read_hello(&mut server).await.unwrap(),
            Envelope::Hello(_)
        ));
        let request = Envelope::Request(wincmd_shared::Request {
            request_id: 1,
            feature_id: "svc.ping".into(),
            diagnostic_operation_id: None,
            args: serde_json::json!({}),
        })
        .sign("test-session");
        write_frame(&mut client, &request).await.unwrap();
        assert!(matches!(
            read_frame(&mut server)
                .await
                .unwrap()
                .verify_and_unwrap("test-session")
                .unwrap(),
            Envelope::Request(_)
        ));
    }

    #[test]
    fn per_sid_quota_caps_one_user_without_starving_others() {
        let quota = ClientQuota::new(2, 3);
        let a1 = quota.try_acquire("S-1-5-21-1", false).unwrap();
        let _a2 = quota.try_acquire("S-1-5-21-1", false).unwrap();
        assert!(quota.try_acquire("S-1-5-21-1", false).is_none());
        assert!(quota.try_acquire("S-1-5-21-2", false).is_some());
        drop(a1);
        assert!(quota.try_acquire("S-1-5-21-1", false).is_some());
    }

    #[test]
    fn privileged_callers_have_their_own_larger_allowance_and_empty_sid_is_refused() {
        let quota = ClientQuota::new(1, 3);
        let _user = quota.try_acquire("S-1-5-21-1", false).unwrap();
        assert!(quota.try_acquire("S-1-5-21-1", false).is_none());
        let held: Vec<_> = (0..3)
            .map(|_| quota.try_acquire("S-1-5-18", true).unwrap())
            .collect();
        assert!(quota.try_acquire("S-1-5-18", true).is_none());
        assert!(quota.try_acquire("", true).is_none());
        drop(held);
        assert!(quota.counts.lock().unwrap().get("S-1-5-18").is_none());
    }

    #[test]
    fn connection_lifetime_is_bounded() {
        let start = Instant::now();
        let max = Duration::from_secs(300);
        assert_eq!(
            remaining_lifetime(start, start + Duration::from_secs(100), max),
            Some(Duration::from_secs(200))
        );
        assert_eq!(remaining_lifetime(start, start + max, max), None);
        assert_eq!(remaining_lifetime(start, start + max * 2, max), None);
    }

    #[tokio::test(start_paused = true)]
    async fn frame_read_never_outlives_remaining_lifetime() {
        let (_client, mut server) = duplex(64);
        let began = tokio::time::Instant::now();
        let err = read_frame_within(&mut server, Duration::from_secs(3))
            .await
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn connection_limit_rejects_excess_and_recovers_when_a_task_exits() {
        let slots = connection_slots();
        let mut permits = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            permits.push(slots.clone().try_acquire_owned().unwrap());
        }
        assert!(slots.clone().try_acquire_owned().is_err());
        permits.pop();
        assert!(slots.clone().try_acquire_owned().is_ok());
    }
}
