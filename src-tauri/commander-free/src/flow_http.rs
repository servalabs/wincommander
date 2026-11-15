// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded HTTP action transport. Authorization and dry-run checks live in the caller.
use std::{collections::HashMap, time::Duration};

pub(super) async fn send(
    method: &str,
    url: &str,
    headers: Option<&HashMap<String, String>>,
    body: Option<&str>,
) -> Result<(), String> {
    send_with_timeout(method, url, headers, body, Duration::from_secs(30)).await
}

async fn send_with_timeout(
    method: &str,
    url: &str,
    headers: Option<&HashMap<String, String>>,
    body: Option<&str>,
    timeout: Duration,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "HTTP client initialization failed".to_string())?;
    let mut request = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        _ => return Err(format!("Unknown HTTP method: {method}")),
    };
    if let Some(headers) = headers {
        for (key, value) in headers { request = request.header(key.as_str(), value.as_str()); }
    }
    if let Some(body) = body { request = request.body(body.to_owned()); }
    let response = request.send().await
        .map_err(|_| "HTTP request failed; remote outcome may be unknown".to_string())?;
    http_action_outcome(response.status().as_u16())
}

fn http_action_outcome(status: u16) -> Result<(), String> {
    if (200..300).contains(&status) { Ok(()) }
    else { Err(format!("HTTP action returned status {status}; completion was not acknowledged")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener, thread};

    #[test]
    fn only_success_status_is_an_acknowledgement() {
        for status in [200, 201, 204, 299] { assert!(http_action_outcome(status).is_ok()); }
        for status in [100, 301, 307, 400, 401, 403, 429, 500, 503] { assert!(http_action_outcome(status).is_err()); }
    }

    fn server(status: u16, delay: Duration) -> (String, thread::JoinHandle<(String, bool)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0 && request.len() + read <= 4096);
                request.extend_from_slice(&chunk[..read]);
            }
            if request.starts_with(b"POST ") {
                while !request.windows(12).any(|bytes| bytes == b"fixture body") {
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0 && request.len() + read <= 4096);
                    request.extend_from_slice(&chunk[..read]);
                }
            }
            thread::sleep(delay);
            let response = format!("HTTP/1.1 {status} fixture\r\nContent-Length: 0\r\nLocation: http://{address}/redirected\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(response.as_bytes());
            listener.set_nonblocking(true).unwrap();
            thread::sleep(Duration::from_millis(100));
            let redirected = listener.accept().is_ok();
            (String::from_utf8_lossy(&request).into_owned(), redirected)
        });
        (format!("http://{address}/fixture"), worker)
    }

    #[tokio::test]
    async fn sends_request_and_accepts_success() {
        let (url, worker) = server(204, Duration::ZERO);
        let headers = HashMap::from([("x-fixture".into(), "value".into())]);
        assert!(send("POST", &url, Some(&headers), Some("fixture body")).await.is_ok());
        let (request, redirected) = worker.join().unwrap();
        assert!(request.starts_with("POST /fixture HTTP/1.1\r\n"));
        assert!(request.to_ascii_lowercase().contains("x-fixture: value\r\n"));
        assert!(request.contains("fixture body"));
        assert!(!redirected);
    }

    #[tokio::test]
    async fn rejects_redirect_and_server_failure_without_following() {
        for code in [301, 302, 307, 500] {
            let (url, worker) = server(code, Duration::ZERO);
            let error = send("GET", &url, None, None).await.unwrap_err();
            assert!(error.contains(&format!("status {code}")));
            assert!(!worker.join().unwrap().1);
        }
    }

    #[tokio::test]
    async fn timeout_does_not_disclose_the_url() {
        let (url, worker) = server(200, Duration::from_millis(250));
        let error = send_with_timeout("GET", &url, None, None, Duration::from_millis(50)).await.unwrap_err();
        assert!(error.contains("outcome may be unknown"));
        assert!(!error.contains(&url));
        worker.join().unwrap();
    }
}
