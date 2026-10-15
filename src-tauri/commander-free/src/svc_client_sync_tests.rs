// SPDX-License-Identifier: AGPL-3.0-or-later
#[test]
fn sync_management_covers_one_broker_and_never_retries_the_handshake() {
    let feature = "svc.vault.manage_personal_syncthing";
    assert_eq!(
        request_timeout_for(feature),
        std::time::Duration::from_secs(130)
    );
    assert!(may_still_be_completing(feature));
    #[cfg(windows)]
    assert!(!retries_service_startup_handshake(feature));
    assert_eq!(
        request_timeout_for("svc.vault.enroll_personal_syncthing"),
        SYNCTHING_ENROLLMENT_TIMEOUT
    );
}

#[cfg(windows)]
async fn sync_fixture_reply(
    action: &str,
    delay: std::time::Duration,
    budget: std::time::Duration,
    receipt: &'static str,
) -> (Result<Value, String>, usize) {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::net::windows::named_pipe::ServerOptions;
    let pipe = format!(r"\\.\pipe\wincmd-sync-fixture-{}", uuid::Uuid::new_v4());
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe)
        .unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = requests.clone();
    let action = action.to_owned();
    let expected_action = action.clone();
    let worker = tokio::spawn(async move {
        server.connect().await.unwrap();
        let wincmd_shared::Envelope::Hello(hello) =
            wincmd_shared::read_envelope(&mut server).await.unwrap()
        else {
            panic!("Hello required")
        };
        wincmd_shared::write_envelope(
            &mut server,
            &wincmd_shared::Envelope::Hello(wincmd_shared::svc::hello_from_ui(
                &hello.session_token,
            )),
        )
        .await
        .unwrap();
        let request = wincmd_shared::read_envelope(&mut server)
            .await
            .unwrap()
            .verify_and_unwrap(&hello.session_token)
            .unwrap();
        let wincmd_shared::Envelope::Request(request) = request else {
            panic!("signed request required")
        };
        assert_eq!(request.feature_id, "svc.vault.manage_personal_syncthing");
        assert_eq!(request.args["action"], expected_action);
        observed.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(delay).await;
        let reply = if receipt == "denied" {
            wincmd_shared::Envelope::Error(wincmd_shared::ErrorReply {
                request_id: request.request_id,
                kind: "vault_not_authorized".into(),
                message: "Vault sync access denied".into(),
            })
        } else {
            wincmd_shared::Envelope::Response(wincmd_shared::Response {
                request_id: request.request_id + u64::from(receipt == "wrong_id"),
                result: serde_json::json!({"confirmed":true}),
            })
        };
        let reply = reply.sign(if receipt == "bad_signature" {
            "wrong-fixture-session"
        } else {
            &hello.session_token
        });
        let _ = wincmd_shared::write_envelope(&mut server, &reply).await;
    });
    let result = call_via_with_timeout(
        &pipe,
        "svc.vault.manage_personal_syncthing",
        serde_json::json!({"action":action}),
        budget,
        None,
    )
    .await;
    worker.await.unwrap();
    (result, requests.load(Ordering::SeqCst))
}

#[cfg(windows)]
#[tokio::test]
async fn sync_list_accepts_a_verified_reply_after_the_old_probe_deadline() {
    let (reply, requests) = sync_fixture_reply(
        "list",
        SVC_TRANSPORT_TIMEOUT + std::time::Duration::from_millis(100),
        request_timeout_for("svc.vault.manage_personal_syncthing"),
        "confirmed",
    )
    .await;
    assert_eq!(reply.unwrap()["confirmed"], true);
    assert_eq!(requests, 1);
}

#[cfg(windows)]
#[tokio::test]
async fn sync_mutation_timeout_is_unknown_and_never_replays_the_request() {
    for action in ["rename", "remove"] {
        let (result, requests) = sync_fixture_reply(
            action,
            std::time::Duration::from_millis(40),
            std::time::Duration::from_millis(5),
            "confirmed",
        )
        .await;
        assert_eq!(result.unwrap_err(), "service operation did not confirm before its deadline; refresh vault state before retrying");
        assert_eq!(requests, 1);
    }
}

#[cfg(windows)]
#[tokio::test]
async fn sync_unverified_mutation_receipts_remain_unknown_without_replay() {
    for receipt in ["wrong_id", "bad_signature"] {
        let (reply, requests) = sync_fixture_reply(
            "remove",
            std::time::Duration::ZERO,
            std::time::Duration::from_secs(1),
            receipt,
        )
        .await;
        assert_eq!(reply.unwrap_err(), "vault_operation_unconfirmed");
        assert_eq!(requests, 1);
    }
}

#[cfg(windows)]
#[tokio::test]
async fn sync_matching_authenticated_denial_remains_denied() {
    let (reply, requests) = sync_fixture_reply(
        "rename",
        std::time::Duration::ZERO,
        std::time::Duration::from_secs(1),
        "denied",
    )
    .await;
    let failure = reply.unwrap_err();
    assert!(failure.starts_with("service rejected request: vault_not_authorized"));
    assert!(!failure.contains("vault_operation_unconfirmed"));
    assert_eq!(requests, 1);
}
