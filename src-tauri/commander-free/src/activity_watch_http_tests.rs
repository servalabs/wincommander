// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn server(response: String) -> (Url, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0u8; 4096];
        let size = socket.read(&mut bytes).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&bytes[..size]).into_owned()
    });
    (
        Url::parse(&format!("http://{address}/api/0/buckets/")).unwrap(),
        task,
    )
}

#[test]
fn canonical_url_keeps_legitimate_bucket_queries_on_the_fixed_origin() {
    let url =
        api_url("/api/0/buckets/test/events?limit=10&start=2026-09-19T00%3A00%3A00Z").unwrap();
    assert_eq!(url.origin().ascii_serialization(), ORIGIN);
    assert!(url.query().unwrap().contains("limit=10"));
}

#[test]
fn plain_encoded_and_nested_traversal_cannot_escape_the_api() {
    for path in [
        "/api/0/../../outside",
        "/api/0/%2e%2e/%2e%2e/outside",
        "/api/0/%252e%252e/outside",
        "/api/0/a%2fb",
        "/api/0/a%5cb",
        "/api/0/\\evil",
        "//example.invalid/api/0/",
        "/api/0/a#fragment",
        "/api/0/a@host",
        "/api/0/a
",
        "http://example.invalid/api/0/",
    ] {
        assert!(api_url(path).is_err(), "{path:?}");
    }
    assert!(api_url(&format!("/api/0/{}", "a".repeat(4096))).is_err());
}

#[tokio::test]
async fn valid_local_json_uses_a_read_only_get() {
    let (url, task) = server(
        "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}".into(),
    )
    .await;
    assert_eq!(
        fetch_json(&client().unwrap(), url, 64).await.unwrap(),
        serde_json::json!({"ok":true})
    );
    assert!(task
        .await
        .unwrap()
        .starts_with("GET /api/0/buckets/ HTTP/1.1"));
}

#[tokio::test]
async fn redirects_never_reach_the_destination_listener() {
    let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", destination.local_addr().unwrap());
    let (url, task) = server(redirect).await;
    assert!(fetch_json(&client().unwrap(), url, 64)
        .await
        .unwrap_err()
        .contains("302"));
    task.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), destination.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn actual_chunked_length_is_bounded_without_a_content_length_header() {
    let (url, task) = server("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10\r\n0123456789abcdef\r\n0\r\n\r\n".into()).await;
    assert!(fetch_json(&client().unwrap(), url, 8)
        .await
        .unwrap_err()
        .contains("oversized"));
    task.await.unwrap();
}

#[tokio::test]
async fn declared_oversize_and_invalid_json_are_rejected() {
    for response in [
        "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot json",
    ] {
        let (url, task) = server(response.into()).await;
        assert!(fetch_json(&client().unwrap(), url, 16).await.is_err());
        task.await.unwrap();
    }
}
