// SPDX-License-Identifier: AGPL-3.0-or-later
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::Semaphore,
    time::timeout,
};
use wincmd_shared::{Envelope, MAX_PAYLOAD_BYTES};

const MAX_CONNECTIONS: usize = 32;
const HELLO_MAX_BYTES: u32 = 16 * 1024;
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn connection_slots() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(MAX_CONNECTIONS))
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
