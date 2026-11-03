// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bound and hash a Pro download without loading the full executable in memory.
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub(super) async fn download_verified(
    mut response: reqwest::Response,
    path: &Path,
    expected_sha256: &str,
    maximum_bytes: u64,
) -> Result<(), String> {
    if !response.status().is_success() {
        return Err(format!("download:HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > maximum_bytes)
    {
        return Err("download:Pro executable exceeds size limit".into());
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|e| format!("disk:tmp create: {e}"))?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("download:body read failed: {e}"))?
    {
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or("download:Pro executable exceeds size limit")?;
        if total > maximum_bytes {
            return Err("download:Pro executable exceeds size limit".into());
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("disk:tmp write: {e}"))?;
    }
    let actual = hex::encode(digest.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(format!(
            "sha256_mismatch:expected {expected_sha256}, downloaded {actual}. Refusing to install."
        ));
    }
    file.sync_all()
        .await
        .map_err(|e| format!("disk:tmp fsync: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::update_replace::StagedFile;
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    async fn response(wire: &'static [u8]) -> reqwest::Response {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request);
            let _ = socket.write_all(wire);
        });
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap()
    }
    fn stage() -> StagedFile {
        StagedFile::new(
            &std::env::temp_dir().join(format!("pro-download-{}.exe", uuid::Uuid::new_v4())),
        )
    }
    #[tokio::test]
    async fn streamed_download_verifies_hash_and_preserves_existing_install() {
        let hash = hex::encode(Sha256::digest(b"payload"));
        let target =
            std::env::temp_dir().join(format!("pro-existing-{}.exe", uuid::Uuid::new_v4()));
        std::fs::write(&target, b"installed").unwrap();
        let tmp = stage();
        let resp =
            response(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload")
                .await;
        download_verified(resp, &tmp.0, &hash, 100).await.unwrap();
        assert_eq!(std::fs::read(&tmp.0).unwrap(), b"payload");
        assert_eq!(
            super::super::compute_pro_sha256_at(&tmp.0).as_deref(),
            Some(hash.as_str())
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"installed");
        let temporary = tmp.0.clone();
        drop(tmp);
        assert!(!temporary.exists());
        std::fs::remove_file(target).unwrap();
    }
    #[tokio::test]
    async fn oversized_and_tampered_responses_leave_no_staged_file() {
        let cases: [(&'static [u8], u64); 4] = [
            (b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload", 6),
            (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n7\r\npayload\r\n0\r\n\r\n", 6),
            (b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload", 10),
            (b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\npayload", 30),
        ];
        for (wire, limit) in cases {
            let tmp = stage();
            let path = tmp.0.clone();
            assert!(
                download_verified(response(wire).await, &tmp.0, &"a".repeat(64), limit)
                    .await
                    .is_err()
            );
            drop(tmp);
            assert!(!path.exists());
        }
    }
    #[tokio::test]
    async fn refuses_http_error_before_creating_a_file() {
        let tmp = stage();
        let resp =
            response(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        assert!(download_verified(resp, &tmp.0, &"a".repeat(64), 100)
            .await
            .is_err());
        assert!(!tmp.0.exists());
    }
}
