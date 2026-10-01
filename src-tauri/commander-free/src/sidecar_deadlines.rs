use std::{
    fmt,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{io::AsyncWrite, sync::Mutex, time::timeout};
use wincmd_shared::{write_envelope, Envelope};

#[derive(Debug)]
pub(super) enum DispatchFailure {
    NotSent(String),
    OutcomeUnknown(String),
    Rejected(String),
}

impl DispatchFailure {
    pub(super) fn may_retry(&self) -> bool {
        matches!(self, Self::NotSent(_))
    }

    pub(super) fn message(&self) -> &str {
        match self {
            Self::NotSent(message) | Self::OutcomeUnknown(message) | Self::Rejected(message) => {
                message
            }
        }
    }
}

impl fmt::Display for DispatchFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

pub(super) async fn while_open<T>(
    mut closing: tokio::sync::watch::Receiver<bool>,
    future: impl Future<Output = T>,
) -> Option<T> {
    if *closing.borrow() {
        return None;
    }
    tokio::select! {
        biased;
        _ = closing.changed() => None,
        result = future => Some(result),
    }
}

pub(super) async fn admit<T>(
    future: impl Future<Output = T>,
    budget: Duration,
    phase: &str,
) -> Result<T, String> {
    timeout(budget, future)
        .await
        .map_err(|_| format!("Pro {phase} busy; request not sent"))
}

struct TrackedWriter<'a, W> {
    writer: &'a mut W,
    accepted: usize,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for TrackedWriter<'_, W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut *self.writer).poll_write(cx, bytes);
        if let Poll::Ready(Ok(written)) = result {
            self.accepted += written;
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.writer).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.writer).poll_shutdown(cx)
    }
}

// A cancelled partial frame must retire its pipe, never resume or replay it.
pub(super) async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &Mutex<W>,
    envelope: &Envelope,
    budget: Duration,
) -> Result<(), DispatchFailure> {
    let mut writer = admit(writer.lock(), budget, "writer")
        .await
        .map_err(DispatchFailure::NotSent)?;
    let mut tracked = TrackedWriter {
        writer: &mut *writer,
        accepted: 0,
    };
    let message = match timeout(budget, write_envelope(&mut tracked, envelope)).await {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(error)) => format!("write request: {error}"),
        Err(_) => "Pro write timeout".to_string(),
    };
    // Count acknowledgements from the transport, including partial length headers.
    Err(if tracked.accepted == 0 {
        DispatchFailure::NotSent(message)
    } else {
        DispatchFailure::OutcomeUnknown(message)
    })
}

pub(super) async fn shutdown<G, R, F>(grace: G, terminate_and_reap: F, budget: Duration)
where
    G: Future,
    R: Future,
    F: FnOnce() -> R,
{
    let _ = timeout(budget, grace).await;
    let _ = timeout(budget, terminate_and_reap()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tokio::{
        io::{duplex, AsyncReadExt},
        sync::Semaphore,
    };

    const BUDGET: Duration = Duration::from_millis(20);

    #[tokio::test]
    async fn closed_peer_before_write_is_safe_to_retry_on_a_new_session() {
        let (client, peer) = duplex(64);
        drop(peer);
        let error = write_frame(&Mutex::new(client), &Envelope::Bye, BUDGET)
            .await
            .unwrap_err();
        assert!(matches!(error, DispatchFailure::NotSent(_)));
        assert!(error.may_retry());
    }

    struct InterruptedWriter {
        accepted: usize,
        partial: bool,
    }

    impl AsyncWrite for InterruptedWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            if !self.partial {
                return Poll::Pending;
            }
            if self.accepted == 0 && !bytes.is_empty() {
                self.accepted = 1;
                Poll::Ready(Ok(1))
            } else {
                Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
            }
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn write_failures_only_allow_retry_before_any_byte_is_accepted() {
        for partial in [false, true] {
            let writer = Mutex::new(InterruptedWriter {
                accepted: 0,
                partial,
            });
            let error = write_frame(&writer, &Envelope::Bye, BUDGET)
                .await
                .unwrap_err();
            assert_eq!(writer.lock().await.accepted, usize::from(partial));
            assert_eq!(error.may_retry(), !partial);
            assert_eq!(matches!(error, DispatchFailure::OutcomeUnknown(_)), partial);
        }
    }

    #[tokio::test]
    async fn held_writer_times_out_without_sending_a_frame() {
        let (client, mut peer) = duplex(64);
        let writer = Mutex::new(client);
        let held = writer.lock().await;
        let error = write_frame(&writer, &Envelope::Bye, BUDGET)
            .await
            .unwrap_err();
        assert!(error.may_retry());
        drop(held);
        let mut byte = [0];
        assert!(timeout(BUDGET, peer.read(&mut byte)).await.is_err());
    }

    #[tokio::test]
    async fn partial_frame_timeout_is_never_retryable_and_releases_writer() {
        let (client, mut peer) = duplex(1);
        let writer = Mutex::new(client);
        let error = write_frame(&writer, &Envelope::Bye, BUDGET)
            .await
            .unwrap_err();
        assert!(matches!(error, DispatchFailure::OutcomeUnknown(_)));
        assert!(!error.may_retry());
        assert!(writer.try_lock().is_ok());
        let mut byte = [0];
        assert_eq!(peer.read(&mut byte).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn busy_pool_and_agent_admission_do_not_consume_later_capacity() {
        let pool = Semaphore::new(1);
        let held = pool.acquire().await.unwrap();
        assert!(admit(pool.acquire(), BUDGET, "worker").await.is_err());
        drop(held);
        assert!(pool.try_acquire().is_ok());
        let agent = Mutex::new(());
        let held = agent.lock().await;
        assert!(admit(agent.lock(), BUDGET, "agent").await.is_err());
        drop(held);
        assert!(agent.try_lock().is_ok());
    }

    #[tokio::test]
    async fn stalled_grace_and_reap_still_attempt_termination_and_return() {
        let terminated = Arc::new(AtomicBool::new(false));
        let observed = terminated.clone();
        timeout(
            Duration::from_secs(1),
            shutdown(
                std::future::pending::<()>(),
                move || async move {
                    observed.store(true, Ordering::SeqCst);
                    std::future::pending::<()>().await;
                },
                BUDGET,
            ),
        )
        .await
        .unwrap();
        assert!(terminated.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn successful_frame_keeps_the_authenticated_payload_intact() {
        let (client, mut peer) = duplex(4096);
        let writer = Mutex::new(client);
        let envelope = Envelope::Bye.sign("test-session-token");
        write_frame(&writer, &envelope, BUDGET).await.unwrap();
        let read = wincmd_shared::read_envelope(&mut peer).await.unwrap();
        assert!(matches!(
            read.verify_and_unwrap("test-session-token").unwrap(),
            Envelope::Bye
        ));
    }

    #[tokio::test]
    async fn closing_interrupts_active_work_and_releases_its_owned_guard() {
        let (closing, receiver) = tokio::sync::watch::channel(false);
        let agent = Arc::new(Mutex::new(()));
        let owned_agent = agent.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(while_open(receiver, async move {
            let _guard = owned_agent.lock().await;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        }));
        ready.await.unwrap();
        closing.send_replace(true);
        assert!(timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .is_none());
        assert!(agent.try_lock().is_ok());
        assert!(while_open(closing.subscribe(), std::future::ready(()))
            .await
            .is_none());
    }
}
