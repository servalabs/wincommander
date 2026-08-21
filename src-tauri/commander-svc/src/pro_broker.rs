// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pro sidecar broker owned by the SYSTEM service.

#![allow(dead_code)]

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct HashAcceptance {
    current: Option<String>,
    previous: Option<String>,
    install_metadata_hash: Option<String>,
    install_path_hash: Option<String>,
}

fn random_session_token() -> String {
    let mut buf = [0u8; 32];
    fill_random(&mut buf);
    bytes_to_hex(&buf)
}

fn random_pipe_name() -> String {
    let mut buf = [0u8; 8];
    fill_random(&mut buf);
    format!(r"\\.\pipe\wincmd-pro-{}", bytes_to_hex(&buf))
}

fn fill_random(buf: &mut [u8]) {
    use rand::rngs::OsRng;
    use rand::RngCore;

    OsRng.fill_bytes(buf);
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }

    out
}

fn verify_pro_binary_hash_release(actual: &str, accepted: &HashAcceptance) -> Result<(), String> {
    let actual = actual.trim();

    if actual.is_empty() {
        return Err(
            "Pro did not report a binary hash in Hello ack (handshake refused)".to_string(),
        );
    }

    if accepted_hash_matches(&accepted.current, actual)
        || accepted_hash_matches(&accepted.previous, actual)
    {
        return Ok(());
    }

    if accepted_hash_matches(&accepted.install_metadata_hash, actual)
        && accepted_hash_matches(&accepted.install_path_hash, actual)
    {
        return Ok(());
    }

    Err(format!(
        "Pro binary hash {} is not in the accepted set - refuse handshake. \
         If you just updated Pro, reinstall it from Settings > Pro.",
        actual
    ))
}

fn accepted_hash_matches(expected: &Option<String>, actual: &str) -> bool {
    expected
        .as_deref()
        .map(|hash| !hash.trim().is_empty() && hash.trim().eq_ignore_ascii_case(actual))
        .unwrap_or(false)
}

// The vault engine is intentionally reached only through a service-created,
// one-shot Envelope pipe.  There is no public `--vault-broker-stdin` mode.
#[cfg(windows)]
pub async fn vault_call(
    target_session_id: u32,
    feature_id: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};
    use tokio::time::{timeout, Duration};
    use wincmd_shared::{read_envelope, write_envelope, Envelope, Hello, Request, PROTOCOL_VERSION};

    const BROKER_TIMEOUT: Duration = Duration::from_secs(120);
    let pipe_name = random_pipe_name();
    let session_token = random_session_token();
    let mut pipe = unsafe {
        ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(true)
            .create(&pipe_name)
            .map_err(|_| "broker_unavailable")?
    };
    let process = spawn_pro_in_target_session(target_session_id, &pipe_name, &session_token)?;
    let result = async {
        timeout(BROKER_TIMEOUT, pipe.connect()).await.map_err(|_| "broker_timeout")?.map_err(|_| "broker_io")?;
        let hello = Envelope::Hello(Hello {
            protocol_version: PROTOCOL_VERSION.into(),
            session_token: session_token.clone(),
            binary_hash: None,
            free_version: None,
            pro_version: None,
        });
        timeout(BROKER_TIMEOUT, write_envelope(&mut pipe, &hello)).await.map_err(|_| "broker_timeout")?.map_err(|_| "broker_io")?;
        let ack = timeout(BROKER_TIMEOUT, read_envelope(&mut pipe)).await.map_err(|_| "broker_timeout")?.map_err(|_| "broker_io")?;
        let Envelope::Hello(Hello { protocol_version, session_token: echoed, binary_hash: Some(hash), .. }) = ack else { return Err("broker_handshake"); };
        if protocol_version != PROTOCOL_VERSION || echoed != session_token || !hash_matches_fixed_pro(&hash) { return Err("broker_handshake"); }
        let request = Envelope::Request(Request { request_id: 1, feature_id: feature_id.into(), args }).sign(&session_token);
        timeout(BROKER_TIMEOUT, write_envelope(&mut pipe, &request)).await.map_err(|_| "broker_timeout")?.map_err(|_| "broker_io")?;
        let reply = timeout(BROKER_TIMEOUT, read_envelope(&mut pipe)).await.map_err(|_| "broker_timeout")?.map_err(|_| "broker_io")?;
        let result = match reply.verify_and_unwrap(&session_token).map_err(|_| "broker_hmac")? {
            Envelope::Response(response) if response.request_id == 1 => Ok(response.result),
            Envelope::Error(error) if error.kind == "vault_acl_readback_failed" => Err("vault_acl_readback_failed"),
            Envelope::Error(error) if error.kind == "vault_acl_apply_failed" => Err("vault_acl_apply_failed"),
            _ => Err("broker_rejected"),
        };
        let _ = write_envelope(&mut pipe, &Envelope::Bye).await;
        result
    }.await;
    use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    let waited = unsafe { WaitForSingleObject(process, 5_000) };
    if result.is_err() || waited != WAIT_OBJECT_0 { unsafe { TerminateProcess(process, 1); } }
    unsafe { windows_sys::Win32::Foundation::CloseHandle(process); }
    result.map_err(str::to_string)
}

#[cfg(windows)]
fn fixed_pro_path() -> std::path::PathBuf {
    std::env::var_os("ProgramData").map(std::path::PathBuf::from).unwrap_or_else(|| r"C:\ProgramData".into()).join("WinCommander").join("bin").join("wincommander-pro.exe")
}

#[cfg(windows)]
fn hash_matches_fixed_pro(reported: &str) -> bool {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(fixed_pro_path()) else { return false; };
    let mut hash = Sha256::new(); let mut buf = [0u8; 65536];
    loop { match file.read(&mut buf) { Ok(0) => break, Ok(count) => hash.update(&buf[..count]), Err(_) => return false } }
    bytes_to_hex(&hash.finalize()).eq_ignore_ascii_case(reported)
}

#[cfg(windows)]
fn spawn_pro_in_target_session(session_id: u32, pipe: &str, token: &str) -> Result<windows_sys::Win32::Foundation::HANDLE, &'static str> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{DuplicateTokenEx, SetTokenInformation, SecurityImpersonation, TokenPrimary, TokenSessionId, TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{CreateProcessAsUserW, GetCurrentProcess, OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW, CREATE_NO_WINDOW};
    let exe = fixed_pro_path();
    if !exe.is_file() { return Err("broker_unavailable"); }
    let command = format!("\"{}\" --core-pipe={} --session-token={}", exe.display(), pipe, token);
    let mut command_wide: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    unsafe {
        if session_id == 0 {
            let child = std::process::Command::new(&exe).arg(format!("--core-pipe={pipe}")).arg(format!("--session-token={token}")).spawn().map_err(|_| "broker_unavailable")?;
            use std::os::windows::io::IntoRawHandle;
            return Ok(child.into_raw_handle() as HANDLE);
        }
        let mut service_token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_SESSIONID, &mut service_token) == 0 { return Err("broker_unavailable"); }
        let mut session_token: HANDLE = std::ptr::null_mut();
        if DuplicateTokenEx(service_token, TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_SESSIONID, std::ptr::null_mut(), SecurityImpersonation, TokenPrimary, &mut session_token) == 0 { CloseHandle(service_token); return Err("broker_unavailable"); }
        CloseHandle(service_token);
        if SetTokenInformation(session_token, TokenSessionId, &session_id as *const _ as *const _, std::mem::size_of::<u32>() as u32) == 0 { CloseHandle(session_token); return Err("session_unavailable"); }
        let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..std::mem::zeroed() };
        let mut process = PROCESS_INFORMATION::default();
        let ok = CreateProcessAsUserW(session_token, std::ptr::null(), command_wide.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), 0, CREATE_NO_WINDOW, std::ptr::null_mut(), std::ptr::null(), &startup, &mut process);
        CloseHandle(session_token);
        if ok == 0 { return Err("broker_unavailable"); }
        CloseHandle(process.hThread);
        Ok(process.hProcess)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_session_token_is_32_bytes_hex_encoded() {
        let token = random_session_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn random_pipe_name_uses_per_session_wincmd_pro_namespace() {
        let pipe = random_pipe_name();
        assert!(pipe.starts_with(r"\\.\pipe\wincmd-pro-"));
        assert_eq!(pipe.len(), r"\\.\pipe\wincmd-pro-".len() + 16);
    }

    #[test]
    fn release_hash_verifier_refuses_empty_hash() {
        assert_eq!(
            verify_pro_binary_hash_release("", &HashAcceptance::default()),
            Err("Pro did not report a binary hash in Hello ack (handshake refused)".to_string())
        );
    }

    #[test]
    fn release_hash_verifier_accepts_current_or_previous_pin() {
        let pins = HashAcceptance {
            current: Some(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ),
            previous: Some(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            ),
            install_metadata_hash: None,
            install_path_hash: None,
        };

        assert!(verify_pro_binary_hash_release(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            &pins
        )
        .is_ok());
        assert!(verify_pro_binary_hash_release(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            &pins
        )
        .is_ok());
    }

    #[test]
    fn release_hash_verifier_requires_metadata_and_disk_hash_to_match() {
        let actual = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let mut pins = HashAcceptance {
            current: None,
            previous: None,
            install_metadata_hash: Some(actual.to_string()),
            install_path_hash: None,
        };
        assert!(verify_pro_binary_hash_release(actual, &pins).is_err());

        pins.install_path_hash = Some(actual.to_string());
        assert!(verify_pro_binary_hash_release(actual, &pins).is_ok());
    }
}
