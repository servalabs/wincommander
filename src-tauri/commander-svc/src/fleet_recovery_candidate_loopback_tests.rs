// SPDX-License-Identifier: AGPL-3.0-or-later
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn v2_candidate_proposal_is_signed_bound_and_never_treats_http_success_as_approval() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let signer = SigningKey::from_bytes(&[11; 32]);
    let trust = FleetTrust {
        org_id: "org-a".into(),
        device_id: "0480b2ca-f4c3-4599-81fc-4876f73e70fa".into(),
        enrollment_generation: "22222222-2222-4222-8222-222222222222".into(),
        signer_key: STANDARD.encode(signer.verifying_key().as_bytes()),
    };
    let retirement = "11111111-1111-4111-8111-111111111111";
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    let issued = now - 2;
    let preimage = serde_json::to_vec(&(
        "fleet.windows.admin-retirement.v1",
        &trust.org_id,
        &trust.device_id,
        &trust.enrollment_generation,
        retirement,
        issued,
        &trust.signer_key,
    ))
    .unwrap();
    let receipt = serde_json::to_vec(&serde_json::json!({
        "org_id":trust.org_id, "old_device_id":trust.device_id, "old_generation":trust.enrollment_generation,
        "retirement_id":retirement, "issued_at":issued, "signer_key":trust.signer_key,
        "signature":STANDARD.encode(signer.sign(&preimage).to_bytes())
    })).unwrap();
    let expected_trust = trust.clone();
    let verifier = tokio::spawn(async move {
        let mut observed_keys = Vec::new();
        for status in ["403 Forbidden", "200 OK"] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let (body_start, body_len) = loop {
                let mut chunk = [0u8; 4096];
                let read = stream.read(&mut chunk).await.unwrap();
                assert!(read > 0 && request.len() + read <= 65536);
                request.extend_from_slice(&chunk[..read]);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&request[..end]).unwrap();
                    assert!(headers.starts_with("POST /v1/windows/enroll-v2 HTTP/1.1\r\n"));
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while request.len() < body_start + body_len {
                let mut chunk = [0u8; 4096];
                let read = stream.read(&mut chunk).await.unwrap();
                assert!(read > 0 && request.len() + read <= 65536);
                request.extend_from_slice(&chunk[..read]);
            }
            let mut body: serde_json::Value =
                serde_json::from_slice(&request[body_start..body_start + body_len]).unwrap();
            assert_eq!(body["device_id"], expected_trust.device_id);
            assert_eq!(body["device_kind"], "wincommander");
            assert_eq!(body["device_hash"], "recovery-candidate");
            assert_eq!(body["enrollment_intent"], "explicit_connect");
            assert!(body["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|cap| cap.as_str() == Some(fleet_agent_core::HMAC_BODY_V2_CAPABILITY)));
            assert_eq!(
                body["previous_generation"],
                expected_trust.enrollment_generation
            );
            assert_eq!(body["recovery_retirement_id"], retirement);
            assert_eq!(body["enrollment_server_key_b64"], expected_trust.signer_key);
            assert_eq!(body["enrollment_proof_version"], 2);
            assert!(
                uuid::Uuid::parse_str(body["shield_progress_challenge"].as_str().unwrap())
                    .is_ok_and(|id| !id.is_nil() && id.get_version_num() == 4)
            );
            assert!(
                uuid::Uuid::parse_str(body["enrollment_nonce"].as_str().unwrap())
                    .is_ok_and(|id| !id.is_nil() && id.get_version_num() == 4)
            );
            assert!((now..=now + 300).contains(&body["enrollment_ts"].as_i64().unwrap()));
            let public = body["outcome_pubkey_b64"].as_str().unwrap().to_owned();
            observed_keys.push(public.clone());
            let key = STANDARD.decode(public).unwrap();
            let key: [u8; 32] = key.try_into().unwrap();
            let signature = STANDARD
                .decode(body["enrollment_signature_b64"].as_str().unwrap())
                .unwrap();
            body.as_object_mut()
                .unwrap()
                .remove("enrollment_signature_b64");
            let mut signed =
                b"WinCommander Fleet enrollment proof v2\nPOST\n/v1/windows/enroll-v2\n".to_vec();
            signed.extend(fleet_proto::canonical_json_bytes(&body));
            let signature = ed25519_dalek::Signature::from_slice(&signature).unwrap();
            assert!(ed25519_dalek::VerifyingKey::from_bytes(&key)
                .unwrap()
                .verify_strict(&signed, &signature)
                .is_ok());
            stream
                .write_all(
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
        }
        observed_keys
    });
    let fs = Fs::default();
    let dir = PathBuf::from("candidate-loopback-protected");
    let state = State {
        record: Some(Record {
            trust: trust.clone(),
            hybrid_trust: None,
            hybrid_epoch_floor: 0,
            hybrid_credential: None,
            hybrid_initial_pending: false,
            hybrid_unconfirmed: false,
            policy: None,
            audit: Vec::new(),
            contact: None,
            origins: vec![base],
            transport_profile: Some(TransportProfile {
                spki_pins: String::new(),
                environment_fingerprint: crate::fleet_runtime_env::fingerprint().unwrap(),
            }),
            credential: None,
            credential_revision: 0,
            connection_revision: 0,
            cleared: None,
            runtime_profile_changed: false,
            service_enabled: true,
            recovery_commit: None,
            operator_key: None,
        }),
        pending: None,
        contact_challenge: None,
        hybrid_enrollment: None,
        hybrid_claim: None,
    };
    fs.atomic_write(
        &dir.join("fleet-machine-policy.json"),
        &serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let store = FleetMachinePolicy::open(Box::new(fs.clone()), dir.clone()).unwrap();
    let protector = Protector;
    let candidate =
        crate::fleet_recovery_candidate::CandidateStore::open(&fs, dir, &protector).unwrap();
    let (public, conflict) =
        crate::fleet_recovery_transport::submit_candidate(&store, &candidate, &receipt, || true)
            .await
            .unwrap();
    assert!(!conflict);
    assert_eq!(STANDARD.decode(&public).unwrap().len(), 32);
    assert!(crate::fleet_recovery_transport::submit_candidate(
        &store,
        &candidate,
        &receipt,
        || true
    )
    .await
    .is_err());
    let seen = tokio::time::timeout(std::time::Duration::from_secs(5), verifier)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(seen, vec![public.clone(), public]);
}
