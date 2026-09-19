// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded read-only transport for the fixed ActivityWatch loopback API.
use reqwest::{Client, Url};
use serde_json::Value;
use std::time::Duration;

const ORIGIN: &str = "http://127.0.0.1:5600";
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

fn api_url(path: &str) -> Result<Url, String> {
    if !path.starts_with("/api/0/")
        || path.len() > 4096
        || path.contains(['\\', '#', '@'])
        || path.contains("//")
        || path.chars().any(char::is_control)
    {
        return Err("Invalid ActivityWatch API path".into());
    }
    let raw_path = path.split('?').next().unwrap_or(path).to_ascii_lowercase();
    if ["%2e", "%2f", "%5c", "%25"]
        .iter()
        .any(|escape| raw_path.contains(escape))
    {
        return Err("Encoded ActivityWatch path traversal is not allowed".into());
    }
    let url = Url::parse(&format!("{ORIGIN}{path}"))
        .map_err(|_| "Invalid ActivityWatch API path".to_string())?;
    if url.origin().ascii_serialization() != ORIGIN || !url.path().starts_with("/api/0/") {
        return Err("ActivityWatch request must stay in its local API namespace".into());
    }
    Ok(url)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|_| "ActivityWatch client setup failed".into())
}

pub(super) async fn request(path: &str) -> Result<Value, String> {
    let url = api_url(path)?;
    fetch_json(&client()?, url, MAX_RESPONSE_BYTES).await
}

async fn fetch_json(client: &Client, url: Url, maximum: usize) -> Result<Value, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "ActivityWatch is not running.".to_string())?;
    if !response.status().is_success() {
        return Err(format!("ActivityWatch returned HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err("ActivityWatch returned an oversized response.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "ActivityWatch response could not be read.".to_string())?
    {
        if chunk.len() > maximum.saturating_sub(bytes.len()) {
            return Err("ActivityWatch returned an oversized response.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "ActivityWatch returned invalid JSON.".into())
}

#[cfg(test)]
#[path = "activity_watch_http_tests.rs"]
mod tests;
