// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental device renewal and release messages. No IPC is enabled.
use super::{sign_device_wire, Binding, ConfirmedCredential, PendingCredential, INVALID, RETRY_LATER};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RESPONSE_LIMIT: usize = 4096;
const REQUEST_LIMIT: usize = 16 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnrolmentRequest<'a> {
    app_id: &'static str,
    license_key: &'a str,
    device_hash: &'a str,
    credential_id: String,
    device_secret: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrolmentResponse {
    ok: bool,
    receipt: String,
}

/// The returned body and temporary encoded secret are zeroed when released.
pub(super) fn enrolment_request(
    binding: &Binding<'_>,
    pending: &PendingCredential,
    license_key: &str,
) -> Result<zeroize::Zeroizing<Vec<u8>>, &'static str> {
    let license_key = license_key.trim();
    if license_key.is_empty()
        || license_key.len() > 512
        || binding.device_hash.is_empty()
        || binding.device_hash.len() > 512
        || binding.device_hash.trim() != binding.device_hash
    {
        return Err(INVALID);
    }
    let secret = zeroize::Zeroizing::new(URL_SAFE_NO_PAD.encode(pending.secret()));
    let mut body = zeroize::Zeroizing::new(Vec::new());
    serde_json::to_writer(
        &mut *body,
        &EnrolmentRequest {
            app_id: "com.servalabs.wincommander",
            license_key,
            device_hash: binding.device_hash,
            credential_id: URL_SAFE_NO_PAD.encode(pending.id),
            device_secret: &secret,
        },
    )
    .map_err(|_| INVALID)?;
    Ok(body)
}

pub(super) fn enrolment_receipt(response: &[u8]) -> Result<String, &'static str> {
    if response.is_empty() || response.len() > HTTP_RESPONSE_LIMIT {
        return Err(INVALID);
    }
    let value: EnrolmentResponse = serde_json::from_slice(response).map_err(|_| INVALID)?;
    if !value.ok || value.receipt.is_empty() || value.receipt.len() > 32 * 1024 {
        return Err(INVALID);
    }
    Ok(value.receipt)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChallengeRequest<'a> {
    old_request: String,
    credential_id: &'a str,
    challenge_proof: String,
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredReleaseRequest {
    old_request: String,
    credential_id: String,
    nonce: String,
    request_digest: String,
    expires_at: u64,
    proof: String,
}

impl Drop for StoredReleaseRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.old_request.zeroize();
        self.credential_id.zeroize();
        self.nonce.zeroize();
        self.request_digest.zeroize();
        self.proof.zeroize();
    }
}

/// Authenticate an exact stored request without requiring the challenge to
/// remain live: the server permits receipt retries after challenge expiry.
pub(super) fn verify_stored_release_request(
    credential: &ConfirmedCredential, expected_body: &[u8], request: &[u8],
) -> Result<(), &'static str> {
    use subtle::ConstantTimeEq;
    if expected_body.is_empty() || expected_body.len() > REQUEST_LIMIT || request.is_empty() || request.len() > 32 * 1024 {
        return Err(INVALID);
    }
    let value: StoredReleaseRequest = serde_json::from_slice(request).map_err(|_| INVALID)?;
    let canonical = zeroize::Zeroizing::new(serde_json::to_vec(&value).map_err(|_| INVALID)?);
    if value.expires_at > MAX_SAFE_INTEGER || canonical.as_slice() != request {
        return Err(INVALID);
    }
    let body = zeroize::Zeroizing::new(URL_SAFE_NO_PAD.decode(&value.old_request).map_err(|_| INVALID)?);
    if URL_SAFE_NO_PAD.encode(body.as_slice()) != value.old_request || body.as_slice() != expected_body { return Err(INVALID); }
    let id = fixed(&value.credential_id)?;
    let nonce = fixed(&value.nonce)?;
    let digest = fixed(&value.request_digest)?;
    let proof = fixed(&value.proof)?;
    let actual: [u8; 32] = Sha256::digest(expected_body).into();
    if id != credential.id || !bool::from(actual.ct_eq(&digest)) { return Err(INVALID); }
    let challenge = wincmd_pq_experiment::licence_device_proof::Challenge {
        operation: wincmd_pq_experiment::licence_device_proof::Operation::Deactivate,
        credential_id: &id, nonce: &nonce, request_digest: &digest, expires_at: value.expires_at,
    };
    let expected = wincmd_pq_experiment::licence_device_proof::sign(credential.secret(), &challenge);
    if !bool::from(expected.ct_eq(&proof)) { return Err(INVALID); }
    Ok(())
}

/// This is only called for a durably confirmed release, never on a network response alone.
pub(super) fn release_acknowledgement_request(
    credential: &ConfirmedCredential, expected_body: &[u8], request: &[u8],
) -> Result<zeroize::Zeroizing<Vec<u8>>, &'static str> {
    use zeroize::Zeroize;
    verify_stored_release_request(credential, expected_body, request)?;
    let mut value: StoredReleaseRequest = serde_json::from_slice(request).map_err(|_| INVALID)?;
    let id = fixed(&value.credential_id)?;
    let nonce = fixed(&value.nonce)?;
    let digest = fixed(&value.request_digest)?;
    let challenge = wincmd_pq_experiment::licence_device_proof::Challenge {
        operation: wincmd_pq_experiment::licence_device_proof::Operation::Deactivate,
        credential_id: &id, nonce: &nonce, request_digest: &digest, expires_at: value.expires_at,
    };
    let ack = wincmd_pq_experiment::licence_device_proof::sign_release_acknowledgement(credential.secret(), &challenge).map_err(|_| INVALID)?;
    value.proof.zeroize();
    value.proof = URL_SAFE_NO_PAD.encode(ack);
    Ok(zeroize::Zeroizing::new(serde_json::to_vec(&value).map_err(|_| INVALID)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseResponse { ok: bool }

/// Do not clear the local licence unless a complete success response is read.
pub(super) fn confirmed_release(response: &[u8]) -> Result<(), &'static str> {
    if response.is_empty() || response.len() > RESPONSE_LIMIT { return Err(INVALID); }
    let value: ReleaseResponse = serde_json::from_slice(response).map_err(|_| INVALID)?;
    if !value.ok { return Err(INVALID); }
    Ok(())
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
    challenge_request_for(credential, body, false)
}

pub(super) fn release_challenge_request(
    credential: &ConfirmedCredential,
    body: &[u8],
) -> Result<Vec<u8>, &'static str> {
    challenge_request_for(credential, body, true)
}

fn challenge_request_for(credential: &ConfirmedCredential, body: &[u8], release: bool) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > REQUEST_LIMIT {
        return Err(INVALID);
    }
    let id = URL_SAFE_NO_PAD.encode(credential.id);
    let digest: [u8; 32] = Sha256::digest(body).into();
    let proof = if release {
        wincmd_pq_experiment::licence_device_proof::sign_release_challenge_request(credential.secret(), &credential.id, &digest)
    } else {
        wincmd_pq_experiment::licence_device_proof::sign_challenge_request(credential.secret(), &credential.id, &digest)
    };
    serde_json::to_vec(&ChallengeRequest {
        old_request: URL_SAFE_NO_PAD.encode(body),
        credential_id: &id,
        challenge_proof: URL_SAFE_NO_PAD.encode(proof),
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
    device_request(credential, body, response, now, wincmd_pq_experiment::licence_device_proof::Operation::Refresh)
}

pub(super) fn release_request(
    credential: &ConfirmedCredential, body: Vec<u8>, response: &[u8], now: u64,
) -> Result<Vec<u8>, &'static str> {
    device_request(credential, body, response, now, wincmd_pq_experiment::licence_device_proof::Operation::Deactivate)
}

fn device_request(
    credential: &ConfirmedCredential, body: Vec<u8>, response: &[u8], now: u64,
    operation: wincmd_pq_experiment::licence_device_proof::Operation,
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
    let proof = sign_device_wire(
        credential,
        body,
        &id,
        &nonce,
        &digest,
        challenge.expires_at,
        now,
        operation,
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
    ReleaseChallenge,
}

/// Uses only the fixed licensing origin and never follows redirects.
pub(super) async fn post(endpoint: Endpoint, body: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    let path = match endpoint {
        Endpoint::Challenge => "/experimental/device/challenge",
        Endpoint::Renewal => "/experimental/device/renew",
        Endpoint::ReleaseChallenge => "/experimental/device/release-challenge",
    };
    let client = required_hybrid_client()?;
    post_to(&client, &format!("{LICENCE_ORIGIN}{path}"), body).await
}

/// Keep the persisted release request in zeroing memory until the HTTP body is dropped.
pub(super) async fn post_release(body: zeroize::Zeroizing<Vec<u8>>) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 32 * 1024 { return Err(INVALID); }
    let client = required_hybrid_client()?;
    let bytes = bytes::Bytes::from_owner(body);
    post_body(&client, &format!("{LICENCE_ORIGIN}/experimental/device/release"), bytes.into()).await
}

/// Fixed-origin activation transport; the encrypted intent remains pending on
/// every failure, including a timeout after the server may have committed.
pub(super) async fn post_activation(body: zeroize::Zeroizing<Vec<u8>>) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 16 * 1024 { return Err(INVALID); }
    let client = required_hybrid_client()?;
    let bytes = bytes::Bytes::from_owner(body);
    // A distinct route prevents an older Worker from silently ignoring the
    // operation ID and allocating an unrecoverable non-idempotent seat.
    post_body(&client, &format!("{LICENCE_ORIGIN}/license/activate/v2"), bytes.into()).await
}

pub(super) async fn post_activation_ack(body: zeroize::Zeroizing<Vec<u8>>) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 16 * 1024 { return Err(INVALID); }
    let client = required_hybrid_client()?;
    let bytes = bytes::Bytes::from_owner(body);
    post_body(&client, &format!("{LICENCE_ORIGIN}/license/activate/v2/ack"), bytes.into()).await
}

/// Unwired v4 trial transport, fixed to the same hybrid-only licensing origin.
pub(super) async fn post_trial_v4(body: zeroize::Zeroizing<Vec<u8>>) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 16 * 1024 { return Err(INVALID); }
    let client = required_hybrid_client()?;
    let bytes = bytes::Bytes::from_owner(body);
    post_body(&client, &format!("{LICENCE_ORIGIN}/trial/v4"), bytes.into()).await
}

/// Unwired secret-custody transport, restricted to the fixed hybrid origin.
pub(super) async fn post_trial_custody(body: zeroize::Zeroizing<Vec<u8>>) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 16 * 1024 { return Err(INVALID); }
    let client = required_hybrid_client()?;
    let bytes = bytes::Bytes::from_owner(body);
    post_body(&client, &format!("{LICENCE_ORIGIN}/experimental/trial/v4/enrol"), bytes.into()).await
}

/// Proof that this process sent the exact acknowledgement and received a
/// successful, bounded response over the fixed hybrid-only transport.
pub(super) struct AcknowledgementConfirmation([u8; 32]);

impl AcknowledgementConfirmation {
    pub(super) fn matches(&self, request: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        let digest: [u8; 32] = Sha256::digest(request).into();
        bool::from(self.0.ct_eq(&digest))
    }
}

pub(super) async fn post_release_acknowledgement(
    body: zeroize::Zeroizing<Vec<u8>>,
) -> Result<AcknowledgementConfirmation, &'static str> {
    if body.is_empty() || body.len() > 32 * 1024 { return Err(INVALID); }
    let digest: [u8; 32] = Sha256::digest(body.as_slice()).into();
    let client = required_hybrid_client()?;
    let bytes = bytes::Bytes::from_owner(body);
    let response = post_body(&client, &format!("{LICENCE_ORIGIN}/experimental/device/release-ack"), bytes.into()).await?;
    confirmed_release(&response)?;
    Ok(AcknowledgementConfirmation(digest))
}

#[cfg(test)]
pub(super) fn test_acknowledgement_confirmation(
    request: &[u8], response: &[u8],
) -> Result<AcknowledgementConfirmation, &'static str> {
    confirmed_release(response)?;
    Ok(AcknowledgementConfirmation(Sha256::digest(request).into()))
}

/// The request buffer is wiped after the HTTP body releases its last reference.
pub(super) async fn post_enrolment(
    body: zeroize::Zeroizing<Vec<u8>>,
) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 64 * 1024 {
        return Err(INVALID);
    }
    let client = required_hybrid_client()?;
    let bytes = bytes::Bytes::from_owner(body);
    post_body(
        &client,
        &format!("{LICENCE_ORIGIN}/experimental/device/enrol"),
        bytes.into(),
    )
    .await
}

fn required_hybrid_provider() -> rustls::crypto::CryptoProvider {
    let mut provider = rustls::crypto::aws_lc_rs::default_provider();
    provider.kx_groups = vec![rustls::crypto::aws_lc_rs::kx_group::X25519MLKEM768];
    provider
}

fn required_hybrid_client() -> Result<reqwest::Client, &'static str> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        required_hybrid_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|_| INVALID)?
    .with_platform_verifier()
    .map_err(|_| INVALID)?
    .with_no_client_auth();
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .use_preconfigured_tls(config)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| INVALID)
}

async fn post_to(
    client: &reqwest::Client,
    url: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() || body.len() > 64 * 1024 {
        return Err(INVALID);
    }
    post_body(client, url, body.into()).await
}

async fn post_body(
    client: &reqwest::Client,
    url: &str,
    body: reqwest::Body,
) -> Result<Vec<u8>, &'static str> {
    let mut response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| INVALID)?;
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(RETRY_LATER);
    }
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

    #[test]
    fn challenge_request_matches_worker_interop_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/rust-device-challenge.json")).unwrap();
        let id: [u8; 32] = URL_SAFE_NO_PAD.decode(fixture["credentialId"].as_str().unwrap()).unwrap().try_into().unwrap();
        let secret: [u8; 32] = URL_SAFE_NO_PAD.decode(fixture["testSecret"].as_str().unwrap()).unwrap().try_into().unwrap();
        let body = URL_SAFE_NO_PAD.decode(fixture["oldRequest"].as_str().unwrap()).unwrap();
        let credential = ConfirmedCredential { id, secret: zeroize::Zeroizing::new(secret) };
        let request: serde_json::Value = serde_json::from_slice(&challenge_request(&credential, &body).unwrap()).unwrap();
        for field in ["credentialId", "oldRequest", "challengeProof"] {
            assert_eq!(request[field], fixture[field]);
        }
    }

    #[test]
    fn release_wire_is_action_bound_and_validates_the_challenge() {
        let credential = ConfirmedCredential { id: [7; 32], secret: zeroize::Zeroizing::new([9; 32]) };
        let body = b"verified licence request".to_vec();
        let digest: [u8; 32] = Sha256::digest(&body).into();
        let start: serde_json::Value = serde_json::from_slice(&release_challenge_request(&credential, &body).unwrap()).unwrap();
        assert_eq!(start["challengeProof"], URL_SAFE_NO_PAD.encode(
            wincmd_pq_experiment::licence_device_proof::sign_release_challenge_request(credential.secret(), &credential.id, &digest)));
        let refresh: serde_json::Value = serde_json::from_slice(&challenge_request(&credential, &body).unwrap()).unwrap();
        assert_ne!(start["challengeProof"], refresh["challengeProof"]);
        let nonce = [8; 32];
        let response = serde_json::json!({ "ok": true, "credentialId": URL_SAFE_NO_PAD.encode(credential.id),
            "nonce": URL_SAFE_NO_PAD.encode(nonce), "requestDigest": URL_SAFE_NO_PAD.encode(digest), "expiresAt": 1060 });
        let encoded = serde_json::to_vec(&response).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&release_request(&credential, body.clone(), &encoded, 1000).unwrap()).unwrap();
        let challenge = wincmd_pq_experiment::licence_device_proof::Challenge {
            operation: wincmd_pq_experiment::licence_device_proof::Operation::Deactivate,
            credential_id: &credential.id, nonce: &nonce, request_digest: &digest, expires_at: 1060,
        };
        assert_eq!(request["proof"], URL_SAFE_NO_PAD.encode(wincmd_pq_experiment::licence_device_proof::sign(credential.secret(), &challenge)));
        assert!(release_request(&credential, b"other".to_vec(), &encoded, 1000).is_err());
        assert!(release_request(&credential, body.clone(), &encoded, 1060).is_err());
        let mut wrong_device = response.clone();
        wrong_device["credentialId"] = serde_json::json!(URL_SAFE_NO_PAD.encode([6; 32]));
        assert!(release_request(&credential, body.clone(), &serde_json::to_vec(&wrong_device).unwrap(), 1000).is_err());
        let mut bad_nonce = response.clone();
        bad_nonce["nonce"] = serde_json::json!("invalid");
        assert!(release_request(&credential, body.clone(), &serde_json::to_vec(&bad_nonce).unwrap(), 1000).is_err());
        let mut unexpected = response.clone();
        unexpected["extra"] = serde_json::json!(1);
        assert!(release_request(&credential, body.clone(), &serde_json::to_vec(&unexpected).unwrap(), 1000).is_err());
        assert!(confirmed_release(br#"{"ok":true}"#).is_ok());
        for invalid in [br#"{"ok":false}"#.as_slice(), br#"{"ok":true,"extra":1}"#, b"{}"] {
            assert!(confirmed_release(invalid).is_err());
        }
    }

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
    async fn renewal_tls_requires_hybrid_exchange_and_trusted_certificate() {
        use rustls::pki_types::PrivatePkcs8KeyDer;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::time::Duration;

        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let server = |group| {
            let mut provider = rustls::crypto::aws_lc_rs::default_provider();
            provider.kx_groups = vec![group];
            Arc::new(
                rustls::ServerConfig::builder_with_provider(Arc::new(provider))
                    .with_protocol_versions(&[&rustls::version::TLS13])
                    .unwrap()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![cert.der().clone()],
                        PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
                    )
                    .unwrap(),
            )
        };
        let trusted = || {
            let mut roots = rustls::RootCertStore::empty();
            roots.add(cert.der().clone()).unwrap();
            let config =
                rustls::ClientConfig::builder_with_provider(Arc::new(required_hybrid_provider()))
                    .with_protocol_versions(&[&rustls::version::TLS13])
                    .unwrap()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
            reqwest::Client::builder()
                .no_proxy()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .use_preconfigured_tls(config)
                .build()
                .unwrap()
        };
        async fn attempt(server: Arc<rustls::ServerConfig>, client: reqwest::Client) -> bool {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let task = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(_) => return,
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut tls = rustls::StreamOwned::new(
                    rustls::ServerConnection::new(server).unwrap(),
                    socket,
                );
                let mut request = [0u8; 1024];
                if tls.read(&mut request).is_ok() {
                    let _ = tls.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    );
                    let _ = tls.flush();
                }
            });
            let ok = post_to(&client, &format!("https://{address}/"), b"{}".to_vec())
                .await
                .is_ok();
            task.join().unwrap();
            ok
        }
        let hybrid = server(rustls::crypto::aws_lc_rs::kx_group::X25519MLKEM768);
        let classical = server(rustls::crypto::aws_lc_rs::kx_group::X25519);
        assert!(attempt(hybrid.clone(), trusted()).await);
        assert!(!attempt(classical, trusted()).await);
        assert!(!attempt(hybrid, required_hybrid_client().unwrap()).await);
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
        let limited = serve(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        assert_eq!(post_to(&client, &limited, b"{}".to_vec()).await.unwrap_err(), RETRY_LATER);
        let unavailable = serve(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        assert_eq!(post_to(&client, &unavailable, b"{}".to_vec()).await.unwrap_err(), INVALID);
    }
}
