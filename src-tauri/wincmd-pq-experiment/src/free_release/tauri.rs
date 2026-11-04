// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{MAX_INSTALLER, SHA256};
use std::io::Read;

pub(super) fn verify(
    mut artifact: impl Read,
    tauri_public_key: &str,
    signature_text: &str,
    version: &str,
) -> Result<(u64, String), &'static str> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let decode = |encoded: &str| -> Result<String, &'static str> {
        if encoded.len() > 8192 {
            return Err("Tauri signature or public key is too large");
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| "invalid Tauri signature encoding")?;
        String::from_utf8(bytes).map_err(|_| "invalid Tauri signature text")
    };
    let public = minisign_verify::PublicKey::decode(&decode(tauri_public_key)?)
        .map_err(|_| "invalid Tauri public key")?;
    let signature = minisign_verify::Signature::decode(&decode(signature_text)?)
        .map_err(|_| "invalid Tauri signature")?;
    let mut verifier = public
        .verify_stream(&signature)
        .map_err(|_| "unsupported or untrusted Tauri signature")?;
    let mut sha = aws_lc_rs::digest::Context::new(&SHA256);
    let mut buffer = [0u8; 65536];
    let mut size = 0u64;
    loop {
        let count = artifact
            .read(&mut buffer)
            .map_err(|_| "cannot read Free installer")?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or("Free installer is too large")?;
        if size > MAX_INSTALLER {
            return Err("Free installer is too large");
        }
        sha.update(&buffer[..count]);
        verifier.update(&buffer[..count]);
    }
    verifier
        .finalize()
        .map_err(|_| "Tauri signature does not match installer")?;
    let signed_version = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .ok_or("Tauri signed version is missing")?;
    if signed_version.strip_prefix('v').unwrap_or(signed_version) != version {
        return Err("Tauri signed version does not match release");
    }
    let sha256: String = sha
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((size, sha256))
}
