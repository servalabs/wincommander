// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded bridge from synchronous entitlement gates to the authenticated service.
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

#[cfg_attr(test, derive(Debug))]
struct Job {
    args: serde_json::Value,
    reply: mpsc::SyncSender<bool>,
}

const QUEUE_LIMIT: usize = 8;
const WAIT: Duration = Duration::from_secs(8);
static WORKER: OnceLock<Option<mpsc::SyncSender<Job>>> = OnceLock::new();

fn worker() -> Option<mpsc::SyncSender<Job>> {
    let (sender, receiver) = mpsc::sync_channel::<Job>(QUEUE_LIMIT);
    std::thread::Builder::new()
        .name("license-history".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            while let Ok(job) = receiver.recv() {
                let result = runtime.block_on(crate::svc_client::call(
                    wincmd_shared::svc::VERIFY_LICENSE_CACHE_VERB,
                    job.args,
                ));
                let accepted = verified_reply(result);
                let _ = job.reply.try_send(accepted);
            }
        })
        .ok()?;
    Some(sender)
}

fn verified_reply(result: Result<serde_json::Value, String>) -> bool {
    matches!(result, Ok(value) if value.get("verified") == Some(&serde_json::Value::Bool(true)))
}

fn submit(sender: &mpsc::SyncSender<Job>, args: serde_json::Value, wait: Duration) -> bool {
    let (reply, receiver) = mpsc::sync_channel(1);
    if sender.try_send(Job { args, reply }).is_err() {
        return false;
    }
    receiver.recv_timeout(wait).unwrap_or(false)
}

pub(super) fn verify(args: serde_json::Value) -> bool {
    WORKER
        .get_or_init(worker)
        .as_ref()
        .is_some_and(|sender| submit(sender, args, WAIT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_full_queue_disconnected_worker_and_timeout() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (reply, _) = mpsc::sync_channel(1);
        sender
            .try_send(Job {
                args: serde_json::json!({}),
                reply,
            })
            .unwrap();
        assert!(!submit(
            &sender,
            serde_json::json!({}),
            Duration::from_millis(10)
        ));
        drop(receiver);
        assert!(!submit(
            &sender,
            serde_json::json!({}),
            Duration::from_millis(10)
        ));
        let (sender, _receiver) = mpsc::sync_channel(1);
        assert!(!submit(
            &sender,
            serde_json::json!({}),
            Duration::from_millis(10)
        ));
    }

    #[test]
    fn returns_only_the_worker_decision() {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let worker = std::thread::spawn(move || {
            let job = receiver.recv().unwrap();
            assert_eq!(job.args["payload"], "fixture");
            job.reply.send(true).unwrap();
        });
        assert!(submit(
            &sender,
            serde_json::json!({"payload":"fixture"}),
            Duration::from_secs(1)
        ));
        worker.join().unwrap();
    }

    #[test]
    fn rejects_errors_and_non_boolean_or_missing_confirmation() {
        assert!(!verified_reply(Err("unavailable".into())));
        assert!(!verified_reply(Ok(serde_json::json!({}))));
        assert!(!verified_reply(Ok(serde_json::json!({"verified":"true"}))));
        assert!(!verified_reply(Ok(serde_json::json!({"verified":false}))));
        assert!(verified_reply(Ok(serde_json::json!({"verified":true}))));
    }

    #[tokio::test]
    async fn hybrid_licence_check_uses_the_signed_service_protocol() {
        use tokio::net::windows::named_pipe::ServerOptions;
        use wincmd_shared::{read_envelope, write_envelope, Envelope, Response};
        let name = format!(
            r"\\.\pipe\wincmd-licence-history-test-{}",
            uuid::Uuid::new_v4()
        );
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let worker = tokio::spawn(async move {
            server.connect().await.unwrap();
            let Envelope::Hello(hello) = read_envelope(&mut server).await.unwrap() else {
                panic!("hello required")
            };
            write_envelope(
                &mut server,
                &Envelope::Hello(wincmd_shared::svc::hello_from_ui(&hello.session_token)),
            )
            .await
            .unwrap();
            let Envelope::Request(request) = read_envelope(&mut server)
                .await
                .unwrap()
                .verify_and_unwrap(&hello.session_token)
                .unwrap()
            else {
                panic!("signed request required")
            };
            assert_eq!(
                request.feature_id,
                wincmd_shared::svc::VERIFY_LICENSE_CACHE_VERB
            );
            assert_eq!(request.args["payload"], "fixture");
            let response = Envelope::Response(Response {
                request_id: request.request_id,
                result: serde_json::json!({"verified":true}),
            })
            .sign(&hello.session_token);
            write_envelope(&mut server, &response).await.unwrap();
            assert!(matches!(
                read_envelope(&mut server).await.unwrap(),
                Envelope::Bye
            ));
        });
        let result = crate::svc_client::call_via(
            &name,
            wincmd_shared::svc::VERIFY_LICENSE_CACHE_VERB,
            serde_json::json!({"payload":"fixture"}),
        )
        .await;
        assert!(verified_reply(result));
        worker.await.unwrap();
    }
}
