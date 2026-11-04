// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental device renewal wire messages and fixed-origin transport. No IPC is enabled.
use super::{sign_refresh_wire, ConfirmedCredential, INVALID};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

const RESPONSE_LIMIT: usize = 4096;
const REQUEST_LIMIT: usize = 16 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChallengeRequest<'a> {
    old_request: String,
    credential_id: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChallengeResponse {
    ok: bool,
    credential_id: String,
    nonce: String,
    request_digest: String,
    expires_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenewalRequest<'a> {
    old_request: String,
    credential_id: &'a str,
    nonce: &'a str,
    request_digest: &'a str,
    expires_at: u64,
    proof: String,
}

fn fixed(value: &str) -> Result<[u8; 32], &'static str> {
    if value.len() != 43 {
        return Err(INVALID);
    }
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| INVALID)?;
    let result: [u8; 32] = bytes.try_into().map_err(|_| INVALID)?;
    if URL_SAFE_NO_PAD.encode(result) != value {
        return Err(INVALID);
    }
    Ok(result)
}

/// `body` must already be the service-verified current licence request.
pub(super) fn challenge_request(
    credential: &ConfirmedCredential,
    body: &[u8],
) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > REQUEST_LIMIT {
        return Err(INVALID);
    }
    let id = URL_SAFE_NO_PAD.encode(credential.id);
    serde_json::to_vec(&ChallengeRequest {
        old_request: URL_SAFE_NO_PAD.encode(body),
        credential_id: &id,
    })
    .map_err(|_| INVALID)
}

/// Verify the complete challenge before authenticating it with the device secret.
pub(super) fn renewal_request(
    credential: &ConfirmedCredential,
    body: Vec<u8>,
    response: &[u8],
    now: u64,
) -> Result<Vec<u8>, &'static str> {
    if response.is_empty() || response.len() > RESPONSE_LIMIT {
        return Err(INVALID);
    }
    let challenge: ChallengeResponse = serde_json::from_slice(response).map_err(|_| INVALID)?;
    if !challenge.ok || challenge.expires_at > MAX_SAFE_INTEGER {
        return Err(INVALID);
    }
    let id = fixed(&challenge.credential_id)?;
    let nonce = fixed(&challenge.nonce)?;
    let digest = fixed(&challenge.request_digest)?;
    let proof = sign_refresh_wire(
        credential,
        body,
        &id,
        &nonce,
        &digest,
        challenge.expires_at,
        now,
    )?;
    serde_json::to_vec(&RenewalRequest {
        old_request: URL_SAFE_NO_PAD.encode(&proof.body),
        credential_id: &challenge.credential_id,
        nonce: &challenge.nonce,
        request_digest: &challenge.request_digest,
        expires_at: challenge.expires_at,
        proof: URL_SAFE_NO_PAD.encode(proof.tag),
    })
    .map_err(|_| INVALID)
}

pub(super) const LICENCE_ORIGIN: &str = "https://wincommander-licensing.servalabs.com";
const HTTP_RESPONSE_LIMIT: usize = 64 * 1024;

pub(super) enum Endpoint {
    Challenge,
    Renewal,
}

/// Uses only the fixed licensing origin and never follows redirects.
pub(super) async fn post(endpoint: Endpoint, body: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    let path = match endpoint {
        Endpoint::Challenge => "/experimental/device/challenge",
        Endpoint::Renewal => "/experimental/device/renew",
    };
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| INVALID)?;
    post_to(&client, &format!("{LICENCE_ORIGIN}{path}"), body).await
}

async fn post_to(
    client: &reqwest::Client,
    url: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 64 * 1024 {
        return Err(INVALID);
    }
    let mut response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| INVALID)?;
    if response.status() != reqwest::StatusCode::OK
        || response
            .content_length()
            .is_some_and(|size| size > HTTP_RESPONSE_LIMIT as u64)
    {
        return Err(INVALID);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| INVALID)? {
        if bytes
            .len()
            .checked_add(chunk.len())
            .map_or(true, |size| size > HTTP_RESPONSE_LIMIT)
        {
            return Err(INVALID);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn serve(response: &'static [u8]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let received = socket.read(&mut buf).await.unwrap();
            assert!(received > 0);
            socket.write_all(response).await.unwrap();
        });
        format!("http://{address}/test")
    }

    #[tokio::test]
    async fn transport_bounds_responses_and_rejects_redirects() {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let ok =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{} ").await;
        assert_eq!(post_to(&client, &ok, b"{}".to_vec()).await.unwrap(), b"{}");
        let redirect = serve(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1/\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        assert!(post_to(&client, &redirect, b"{}".to_vec()).await.is_err());
        let oversized =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 65537\r\nConnection: close\r\n\r\n").await;
        assert!(post_to(&client, &oversized, b"{}".to_vec()).await.is_err());
    }
}
