// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use aws_lc_rs::signature::{KeyPair, PqdsaKeyPair, ML_DSA_65_SIGNING};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

#[test]
fn rejects_bad_context_signatures_and_installer() {
    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let trust = Trust {
        ed: ed.verifying_key().to_bytes(),
        pq: pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "ed",
        pq_id: "pq",
        epoch: 7,
    };
    let bytes = b"installer";
    let url = "https://winupdates.servalabs.com/free/v3.7.0/WinCommander_3.7.0_x64-setup.exe";
    let payload = serde_json::to_vec(&serde_json::json!({"format":"wincommander-free-release-v1", "version":"3.7.0", "channel":"stable", "platform":"windows-x86_64", "url":url, "sha256":hex::encode(Sha256::digest(bytes)), "size":bytes.len()})).unwrap();
    let envelope = jws::sign::assemble(
        &payload,
        7,
        &trust.policy(),
        |m| Ok(ed.sign(m).to_bytes().to_vec()),
        |m| {
            let mut s = vec![0; 3309];
            pq.sign(m, &mut s).unwrap();
            Ok(s)
        },
    )
    .unwrap();
    let release = metadata(&envelope, "3.7.0", UpdateChannel::Stable, url, &trust).unwrap();
    assert!(free_release::verify_artifact(&release, bytes).is_ok());
    assert!(free_release::verify_artifact(&release, b"malicious").is_err());
    assert!(metadata(&envelope, "3.7.0", UpdateChannel::Beta, url, &trust).is_err());
    assert!(metadata(
        &envelope,
        "3.7.0",
        UpdateChannel::Stable,
        "https://evil.example/x",
        &trust
    )
    .is_err());
    for i in 0..2 {
        let mut bad: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
        bad["signatures"][i]["signature"] = serde_json::json!("bad");
        assert!(metadata(
            &serde_json::to_vec(&bad).unwrap(),
            "3.7.0",
            UpdateChannel::Stable,
            url,
            &trust
        )
        .is_err());
    }
    assert!(metadata_url("../evil", UpdateChannel::Stable).is_err());
    assert!(metadata_url("3.07.0", UpdateChannel::Beta).is_err());
    assert!(metadata_url("3.7.0", UpdateChannel::Beta)
        .unwrap()
        .ends_with("/hybrid-beta.json"));
    let mut bounded = Vec::new();
    append_bounded(&mut bounded, b"123", 4).unwrap();
    assert!(append_bounded(&mut bounded, b"45", 4).is_err());
    assert_eq!(&bounded, b"123");
}

#[tokio::test]
async fn response_rejects_oversized_stream_even_without_length_header() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n12345")
            .await
            .unwrap();
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
        .unwrap();
    assert!(read_bounded_response(response, 4).await.is_err());
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn staged_release_requires_hybrid_and_tauri_signatures_together() {
    use super::super::launch_tests::{TEST_UPDATER_PUBLIC_KEY, TEST_UPDATE_SIGNATURE};
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri_plugin_updater::UpdaterExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let ed = SigningKey::from_bytes(&[42; 32]);
    let pq = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING).unwrap();
    let trust = Trust {
        ed: ed.verifying_key().to_bytes(),
        pq: pq.public_key().as_ref().try_into().unwrap(),
        ed_id: "ed",
        pq_id: "pq",
        epoch: 7,
    };
    let version = "99.0.0";
    let url = "https://winupdates.servalabs.com/free/v99.0.0/WinCommander_99.0.0_x64-setup.exe";
    let installer = b"MZ";
    let staged_manifest = serde_json::to_vec(&serde_json::json!({"version":version,"platforms":{
        "windows-x86_64":{"url":url,"signature":TEST_UPDATE_SIGNATURE}}}))
    .unwrap();
    let payload = free_release::stage_payload(
        &staged_manifest,
        &installer[..],
        version,
        "stable",
        TEST_UPDATER_PUBLIC_KEY,
    )
    .unwrap();
    let inputs = free_release::prepare_release_from_reader(
        &payload,
        &installer[..],
        version,
        "stable",
        url,
        7,
        &trust.policy(),
    )
    .unwrap();
    let ed_sig = ed.sign(&inputs.ed).to_bytes().to_vec();
    let mut pq_sig = vec![0; 3309];
    pq.sign(&inputs.pq, &mut pq_sig).unwrap();
    let envelope = free_release::finish_release(
        &payload,
        &installer[..],
        version,
        "stable",
        7,
        &trust.policy(),
        (ed_sig, pq_sig),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/manifest.json", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        let manifest =
            serde_json::json!({"version":version,"url":url,"signature":TEST_UPDATE_SIGNATURE})
                .to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", manifest.len(), manifest).as_bytes()).await.unwrap();
    });
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert("updater".into(), serde_json::json!({"pubkey":TEST_UPDATER_PUBLIC_KEY, "requireSignedVersion":true, "dangerousInsecureTransportProtocol":true}));
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let mut update = app
        .updater_builder()
        .endpoints(vec![endpoint.parse().unwrap()])
        .unwrap()
        .no_proxy()
        .build()
        .unwrap()
        .check()
        .await
        .unwrap()
        .unwrap();
    server.await.unwrap();

    let release = metadata(
        &envelope,
        &update.version,
        UpdateChannel::Stable,
        update.download_url.as_str(),
        &trust,
    )
    .unwrap();
    assert!(verify_package(&update, &release, installer).is_ok());
    assert!(verify_package(&update, &release, b"MX").is_err());
    assert!(metadata(
        &envelope,
        &update.version,
        UpdateChannel::Beta,
        update.download_url.as_str(),
        &trust
    )
    .is_err());
    for i in 0..2 {
        let mut bad: serde_json::Value = serde_json::from_slice(&envelope).unwrap();
        bad["signatures"][i]["signature"] = serde_json::json!("invalid");
        assert!(metadata(
            &serde_json::to_vec(&bad).unwrap(),
            &update.version,
            UpdateChannel::Stable,
            update.download_url.as_str(),
            &trust
        )
        .is_err());
    }
    update.signature = "invalid".into();
    assert!(verify_package(&update, &release, installer).is_err());
}
