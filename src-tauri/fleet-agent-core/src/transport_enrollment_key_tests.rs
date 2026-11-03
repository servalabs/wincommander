// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use ed25519_dalek::SigningKey;
use std::io::{BufRead, Read, Write};

#[tokio::test]
async fn weak_enrollment_key_does_not_partially_pin_and_valid_retry_succeeds() {
    let operator = SigningKey::from_bytes(&[31; 32]);
    let server = SigningKey::from_bytes(&[32; 32]);
    let operator_key = B64.encode(operator.verifying_key().as_bytes());
    let server_key = B64.encode(server.verifying_key().as_bytes());
    let mut identity = [0u8; 32];
    identity[0] = 1;
    let weak_key = B64.encode(identity);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let op_for_server = operator_key.clone();
    let valid_for_server = server_key.clone();
    let host = std::thread::spawn(move || {
        for key in [weak_key, valid_for_server] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(&mut socket);
            let mut line = String::new();
            let mut length = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let response =
                serde_json::json!({"device_id":"device", "command_pubkey_b64":op_for_server,
                "server_signing_key_b64":key, "checkin_secret_b64":null})
                .to_string();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        }
    });
    let config = FleetConfig {
        url,
        enroll_token: "token".into(),
        fleet_token: "token".into(),
        checkin_secret: vec![1; 32],
        cmd_pubkey_b64: operator_key.clone(),
        cmd_pubkey: operator.verifying_key(),
        checkin_interval_secs: 60,
        checkin_jitter_min_frac: 0.5,
        checkin_jitter_max_frac: 1.5,
        checkin_padding_bytes: 0,
        checkin_decoy_enabled: false,
        checkin_decoy_rate: 0.0,
        max_cmd_skew_secs: 300,
        cmd_pin_spki: None,
    };
    let state = crate::state::new_shared_fleet_state();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    assert!(enroll(&client, &config, &state, "linux", "test")
        .await
        .is_err());
    {
        let state = state.lock().unwrap();
        assert!(state.device_id.is_none());
        assert!(state.pinned_pubkey_b64.is_none());
        assert!(state.pinned_server_key_b64.is_none());
    }
    assert_eq!(
        enroll(&client, &config, &state, "linux", "test")
            .await
            .unwrap(),
        "device"
    );
    let state = state.lock().unwrap();
    assert_eq!(
        state.pinned_pubkey_b64.as_deref(),
        Some(operator_key.as_str())
    );
    assert_eq!(
        state.pinned_server_key_b64.as_deref(),
        Some(server_key.as_str())
    );
    drop(state);
    host.join().unwrap();
}
