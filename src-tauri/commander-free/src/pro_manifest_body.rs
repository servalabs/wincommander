// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read release metadata with a hard bound, including chunked responses.
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

pub(super) async fn read(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|length| length > MAX_MANIFEST_BYTES as u64) {
        return Err("parse:Pro manifest exceeds size limit".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("parse:{e}"))? {
        if chunk.len() > MAX_MANIFEST_BYTES.saturating_sub(body.len()) {
            return Err("parse:Pro manifest exceeds size limit".into());
        }
        body.try_reserve(chunk.len()).map_err(|_| "parse:Pro manifest allocation failed".to_string())?;
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    async fn response(body: Vec<u8>) -> reqwest::Response {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request);
            let _ = socket.write_all(&body);
        });
        reqwest::Client::builder().no_proxy().build().unwrap()
            .get(format!("http://{address}/")).send().await.unwrap()
    }

    #[tokio::test]
    async fn bounded_read_accepts_small_json_and_rejects_oversized_streams() {
        let small = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec();
        assert_eq!(read(response(small).await).await.unwrap(), b"{}");
        let advertised = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", MAX_MANIFEST_BYTES + 1).into_bytes();
        assert!(read(response(advertised).await).await.is_err());
        let mut chunked = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", MAX_MANIFEST_BYTES + 1).into_bytes();
        chunked.extend(vec![b'x'; MAX_MANIFEST_BYTES + 1]);
        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
        assert!(read(response(chunked).await).await.is_err());
    }
}
