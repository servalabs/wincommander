// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounds transport work without cancelling an in-progress service operation.

use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;
use wincmd_shared::{read_envelope_with_limit, write_envelope, Envelope, MAX_PAYLOAD_BYTES};

pub(crate) const MAX_CONNECTIONS: usize = 32;
pub(crate) const MAX_FRAMES_PER_CONNECTION: usize = 128;
const MAX_HELLO_BYTES: u32 = 4096;
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn connection_slots() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(MAX_CONNECTIONS))
}

pub(crate) async fn read_hello<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Envelope> {
    read_with_deadline(reader, MAX_HELLO_BYTES, HELLO_TIMEOUT).await
}

pub(crate) async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Envelope> {
    read_with_deadline(reader, MAX_PAYLOAD_BYTES, FRAME_TIMEOUT).await
}

async fn read_with_deadline<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: u32,
    deadline: Duration,
) -> io::Result<Envelope> {
    tokio::time::timeout(deadline, read_envelope_with_limit(reader, limit))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "service frame read timed out"))?
}

pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Envelope,
) -> io::Result<()> {
    write_with_deadline(writer, frame, WRITE_TIMEOUT).await
}

async fn write_with_deadline<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Envelope,
    deadline: Duration,
) -> io::Result<()> {
    tokio::time::timeout(deadline, write_envelope(writer, frame))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "service frame write timed out"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn stalled_partial_header_and_body_have_a_deadline() {
        for prefix in [vec![1, 0], 100u32.to_le_bytes().to_vec()] {
            let (mut peer, mut reader) = tokio::io::duplex(256);
            peer.write_all(&prefix).await.unwrap();
            let error = read_with_deadline(&mut reader, 4096, Duration::from_millis(25))
                .await
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        }
    }

    #[tokio::test]
    async fn a_peer_that_stops_reading_cannot_hold_the_writer_forever() {
        let (_peer, mut writer) = tokio::io::duplex(1);
        let error = write_with_deadline(&mut writer, &Envelope::Bye, Duration::from_millis(25))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn hello_limit_is_checked_before_body_read() {
        let (mut peer, mut reader) = tokio::io::duplex(16);
        peer.write_u32_le(MAX_HELLO_BYTES + 1).await.unwrap();
        let error = read_hello(&mut reader).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn admission_is_bounded_and_released_on_connection_task_abort() {
        let slots = connection_slots();
        let all = Arc::clone(&slots)
            .try_acquire_many_owned(MAX_CONNECTIONS as u32)
            .unwrap();
        assert!(Arc::clone(&slots).try_acquire_owned().is_err());
        let task = tokio::spawn(async move {
            let _all = all;
            std::future::pending::<()>().await;
        });
        task.abort();
        let _ = task.await;
        assert_eq!(slots.available_permits(), MAX_CONNECTIONS);
        assert!(Arc::clone(&slots).try_acquire_owned().is_ok());
    }

    #[tokio::test]
    async fn valid_hello_and_closed_peer_keep_their_normal_results() {
        let (mut peer, mut reader) = tokio::io::duplex(4096);
        write_frame(
            &mut peer,
            &Envelope::Hello(wincmd_shared::svc::hello_from_ui("test")),
        )
        .await
        .unwrap();
        assert!(matches!(
            read_hello(&mut reader).await.unwrap(),
            Envelope::Hello(_)
        ));
        drop(peer);
        assert_eq!(
            read_frame(&mut reader).await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
