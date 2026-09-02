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
const BROKER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
#[cfg(windows)]
const MAX_SIGNED_NOTIFICATIONS: usize = 32;
#[cfg(windows)]
const REQUEST_ID: u64 = 1;

#[cfg(windows)]
pub async fn vault_call(
    target_session_id: u32,
    caller_sid: &str,
    caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
    caller_authentication_id: Option<(u32, i32)>,
    presentation: wincmd_shared::vault_access::VaultPresentation,
    feature_id: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};
    use tokio::time::{timeout_at, Instant};
    use wincmd_shared::{
        read_envelope, write_envelope, Envelope, Hello, Request, PROTOCOL_VERSION,
    };

    let deadline = Instant::now() + BROKER_TIMEOUT;
    let pipe_name = random_pipe_name();
    let session_token = random_session_token();
    let mut pipe = ServerOptions::new()
        .pipe_mode(PipeMode::Byte)
        .first_pipe_instance(true)
        .create(&pipe_name)
        .map_err(|_| "broker_unavailable")?;
    let process = spawn_pro_for_presentation(
        target_session_id,
        caller_sid,
        caller_token,
        caller_authentication_id,
        presentation,
        feature_id,
        &pipe_name,
        &session_token,
    )?;
    let result = async {
        timeout_at(deadline, pipe.connect())
            .await
            .map_err(|_| "broker_timeout")?
            .map_err(|_| "broker_io")?;
        let hello = Envelope::Hello(Hello {
            protocol_version: PROTOCOL_VERSION.into(),
            session_token: session_token.clone(),
            binary_hash: None,
            free_version: None,
            pro_version: None,
        });
        timeout_at(deadline, write_envelope(&mut pipe, &hello))
            .await
            .map_err(|_| "broker_timeout")?
            .map_err(|_| "broker_io")?;
        let ack = timeout_at(deadline, read_envelope(&mut pipe))
            .await
            .map_err(|_| "broker_timeout")?
            .map_err(|_| "broker_io")?;
        let Envelope::Hello(Hello {
            protocol_version,
            session_token: echoed,
            binary_hash: Some(hash),
            ..
        }) = ack
        else {
            return Err("broker_handshake");
        };
        if protocol_version != PROTOCOL_VERSION
            || echoed != session_token
            || !hash_matches_fixed_pro(&hash)
        {
            return Err("broker_handshake");
        }
        let mut request = Envelope::Request(Request {
            request_id: REQUEST_ID,
            feature_id: feature_id.into(),
            args,
        })
        .sign(&session_token);
        let write_result = timeout_at(deadline, write_envelope(&mut pipe, &request))
            .await
            .map_err(|_| "broker_timeout")
            .and_then(|result| result.map_err(|_| "broker_io"));
        zeroize_broker_request(&mut request);
        write_result?;
        let mut notification_count = 0;
        let result = loop {
            let reply = timeout_at(deadline, read_envelope(&mut pipe))
                .await
                .map_err(|_| "broker_timeout")?
                .map_err(|_| "broker_io")?;
            match process_broker_reply(reply, &session_token, REQUEST_ID, &mut notification_count)?
            {
                BrokerReply::Notification => {}
                BrokerReply::Finished(result) => break result,
            }
        };
        let _ = timeout_at(deadline, write_envelope(&mut pipe, &Envelope::Bye)).await;
        result
    }
    .await;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
    let waited = unsafe { WaitForSingleObject(process, 5_000) };
    if result.is_err() || waited != WAIT_OBJECT_0 {
        unsafe {
            TerminateProcess(process, 1);
        }
    }
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(process);
    }
    result.map_err(str::to_string)
}

#[cfg(windows)]
fn zeroize_broker_request(request: &mut wincmd_shared::Envelope) {
    use zeroize::Zeroize;
    match request {
        wincmd_shared::Envelope::Signed(signed) => signed.inner.zeroize(),
        wincmd_shared::Envelope::Request(request) => zeroize_json(&mut request.args),
        _ => {}
    }
}

#[cfg(windows)]
fn zeroize_json(value: &mut serde_json::Value) {
    use zeroize::Zeroize;
    match value {
        serde_json::Value::String(text) => text.zeroize(),
        serde_json::Value::Array(values) => values.iter_mut().for_each(zeroize_json),
        serde_json::Value::Object(values) => values.values_mut().for_each(zeroize_json),
        _ => {}
    }
}

/// Recovery cannot depend on the original interactive user still being logged
/// on. This path has no renderer input and names only a bounded engine slot;
/// it launches the authenticated Pro sidecar as SYSTEM to close that slot.
#[cfg(windows)]
pub async fn vault_recovery_dismount(internal_drive: u8) -> Result<serde_json::Value, String> {
    if internal_drive > 25 {
        return Err("broker_rejected".to_string());
    }
    vault_call(
        0,
        "S-1-5-18",
        None,
        None,
        wincmd_shared::vault_access::VaultPresentation::Machine,
        "vault.broker.dismount",
        serde_json::json!({ "internal_drive": internal_drive }),
    )
    .await
}

#[cfg(windows)]
enum BrokerReply {
    Notification,
    Finished(Result<serde_json::Value, &'static str>),
}

#[cfg(windows)]
fn process_broker_reply(
    reply: wincmd_shared::Envelope,
    session_token: &str,
    request_id: u64,
    notification_count: &mut usize,
) -> Result<BrokerReply, &'static str> {
    use wincmd_shared::Envelope;

    match reply
        .verify_and_unwrap(session_token)
        .map_err(|_| "broker_hmac")?
    {
        Envelope::Notification(_) => {
            *notification_count = notification_count.checked_add(1).ok_or("broker_rejected")?;
            if *notification_count > MAX_SIGNED_NOTIFICATIONS {
                return Err("broker_rejected");
            }
            Ok(BrokerReply::Notification)
        }
        Envelope::Response(response) if response.request_id == request_id => {
            Ok(BrokerReply::Finished(Ok(response.result)))
        }
        Envelope::Error(error) if error.request_id == request_id => {
            let error = match error.kind.as_str() {
                "vault_acl_readback_failed" => "vault_acl_readback_failed",
                "vault_acl_apply_failed" => "vault_acl_apply_failed",
                "vault_engine_unlock_failed" => "vault_engine_unlock_failed",
                "vault_engine_drive_letter_unavailable" => "vault_engine_drive_letter_unavailable",
                "vault_engine_mount_failed" => "vault_engine_mount_failed",
                _ => "broker_rejected",
            };
            Ok(BrokerReply::Finished(Err(error)))
        }
        Envelope::Response(_)
        | Envelope::Error(_)
        | Envelope::Hello(_)
        | Envelope::Request(_)
        | Envelope::Bye
        | Envelope::Signed(_) => Err("broker_rejected"),
    }
}

#[cfg(windows)]
fn fixed_pro_path() -> std::path::PathBuf {
    std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"C:\ProgramData".into())
        .join("WinCommander")
        .join("bin")
        .join("wincommander-pro.exe")
}

#[cfg(windows)]
fn hash_matches_fixed_pro(reported: &str) -> bool {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::sync::{Mutex, OnceLock};

    #[derive(Clone, PartialEq, Eq)]
    struct FileIdentity {
        len: u64,
        modified: Option<std::time::SystemTime>,
    }
    static HASH_CACHE: OnceLock<Mutex<Option<(FileIdentity, String)>>> = OnceLock::new();

    let path = fixed_pro_path();
    let Ok(metadata) = std::fs::metadata(&path) else {
        return false;
    };
    let identity = FileIdentity {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    };
    let cache = HASH_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(cache) = cache.lock() {
        if let Some((cached_identity, hash)) = cache.as_ref() {
            if cached_identity == &identity {
                return hash.eq_ignore_ascii_case(reported);
            }
        }
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(count) => hash.update(&buf[..count]),
            Err(_) => return false,
        }
    }
    let actual = bytes_to_hex(&hash.finalize());
    if let Ok(mut cache) = cache.lock() {
        *cache = Some((identity, actual.clone()));
    }
    actual.eq_ignore_ascii_case(reported)
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn spawn_pro_for_presentation(
    session_id: u32,
    caller_sid: &str,
    caller_token: Option<windows_sys::Win32::Foundation::HANDLE>,
    caller_authentication_id: Option<(u32, i32)>,
    presentation: wincmd_shared::vault_access::VaultPresentation,
    feature_id: &str,
    pipe: &str,
    token: &str,
) -> Result<windows_sys::Win32::Foundation::HANDLE, &'static str> {
    use wincmd_shared::vault_access::VaultPresentation;

    match presentation {
        // Machine mounts stay in the SYSTEM service context. Their Mount
        // Manager presentation must not depend on a user's desktop token.
        VaultPresentation::Machine => spawn_pro_as_service(pipe, token),
        // Per-user mounts use only the logon token Windows associates with the
        // authenticated pipe client. A WTS session/SID match is insufficient:
        // the per-user drive link is scoped to the caller token's DOS-device
        // namespace, so a substituted session token can attest a mount the
        // client cannot see.
        VaultPresentation::PerUser => match caller_token {
            Some(caller_token) => spawn_pro_as_authenticated_user_token(
                caller_token,
                caller_authentication_id,
                session_id,
                caller_sid,
                pipe,
                token,
            ),
            // Lifecycle cleanup has no live pipe client to duplicate. It uses
            // an explicit internal slot and may retain the bounded WTS path.
            None if per_user_wts_fallback_allowed(feature_id) => {
                spawn_pro_as_session_user(session_id, caller_sid, pipe, token)
            }
            None => Err("broker_rejected"),
        },
    }
}

#[cfg(windows)]
fn spawn_pro_as_service(
    pipe: &str,
    token: &str,
) -> Result<windows_sys::Win32::Foundation::HANDLE, &'static str> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    let exe = fixed_pro_path();
    if !exe.is_file() {
        return Err("broker_unavailable");
    }
    let mut command = std::process::Command::new(&exe);
    command
        .arg(format!("--core-pipe={pipe}"))
        .arg(format!("--session-token={token}"))
        // The service has no interactive UI. Never attach a child console to
        // the desktop if a Vault recovery helper cannot start.
        .creation_flags(CREATE_NO_WINDOW);
    let child = command.spawn().map_err(|_| "broker_unavailable")?;
    use std::os::windows::io::IntoRawHandle;
    Ok(child.into_raw_handle() as HANDLE)
}

#[cfg(windows)]
fn spawn_pro_as_session_user(
    session_id: u32,
    caller_sid: &str,
    pipe: &str,
    token: &str,
) -> Result<windows_sys::Win32::Foundation::HANDLE, &'static str> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::RemoteDesktop::WTSQueryUserToken;
    if session_id == 0 {
        return Err("broker_unavailable");
    }
    unsafe {
        let mut user_token: HANDLE = std::ptr::null_mut();
        if WTSQueryUserToken(session_id, &mut user_token) == 0 {
            return Err("session_unavailable");
        }
        let result =
            spawn_pro_with_user_token(user_token, None, session_id, caller_sid, pipe, token);
        CloseHandle(user_token);
        result
    }
}

#[cfg(windows)]
fn per_user_wts_fallback_allowed(feature_id: &str) -> bool {
    feature_id == "vault.broker.dismount"
}

#[cfg(windows)]
fn spawn_pro_as_authenticated_user_token(
    client_token: windows_sys::Win32::Foundation::HANDLE,
    expected_authentication_id: Option<(u32, i32)>,
    session_id: u32,
    caller_sid: &str,
    pipe: &str,
    token: &str,
) -> Result<windows_sys::Win32::Foundation::HANDLE, &'static str> {
    if client_token.is_null() || session_id == 0 {
        return Err("broker_rejected");
    }
    spawn_pro_with_user_token(
        client_token,
        expected_authentication_id,
        session_id,
        caller_sid,
        pipe,
        token,
    )
}

#[cfg(windows)]
fn spawn_pro_with_user_token(
    user_token: windows_sys::Win32::Foundation::HANDLE,
    expected_authentication_id: Option<(u32, i32)>,
    session_id: u32,
    caller_sid: &str,
    pipe: &str,
    token: &str,
) -> Result<windows_sys::Win32::Foundation::HANDLE, &'static str> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        DuplicateTokenEx, SecurityImpersonation, TokenPrimary, TOKEN_ASSIGN_PRIMARY,
        TOKEN_DUPLICATE, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Environment::{
        CreateEnvironmentBlock, DestroyEnvironmentBlock,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    let exe = fixed_pro_path();
    if !exe.is_file() {
        return Err("broker_unavailable");
    }
    if !token_matches_authenticated_client(user_token, session_id, caller_sid) {
        return Err("broker_rejected");
    }
    let source_authentication_id = token_authentication_id(user_token).ok_or("broker_rejected")?;
    if expected_authentication_id.is_some()
        && !same_authentication_id(Some(source_authentication_id), expected_authentication_id)
    {
        return Err("broker_rejected");
    }
    let command = format!(
        "\"{}\" --core-pipe={} --session-token={}",
        exe.display(),
        pipe,
        token
    );
    let mut command_wide: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let mut desktop_wide: Vec<u16> = "winsta0\\default".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut primary_token: HANDLE = std::ptr::null_mut();
        if DuplicateTokenEx(
            user_token,
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            std::ptr::null_mut(),
            SecurityImpersonation,
            TokenPrimary,
            &mut primary_token,
        ) == 0
        {
            return Err("broker_unavailable");
        }
        let primary_authentication_id = token_authentication_id(primary_token);
        if !same_authentication_id(primary_authentication_id, Some(source_authentication_id)) {
            CloseHandle(primary_token);
            return Err("broker_rejected");
        }
        let mut environment = std::ptr::null_mut();
        if CreateEnvironmentBlock(&mut environment, primary_token, 0) == 0 {
            CloseHandle(primary_token);
            return Err("broker_unavailable");
        }
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            lpDesktop: desktop_wide.as_mut_ptr(),
            ..std::mem::zeroed()
        };
        let mut process = PROCESS_INFORMATION::default();
        let ok = CreateProcessAsUserW(
            primary_token,
            std::ptr::null(),
            command_wide.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment,
            std::ptr::null(),
            &startup,
            &mut process,
        );
        DestroyEnvironmentBlock(environment);
        CloseHandle(primary_token);
        if ok == 0 {
            return Err("broker_unavailable");
        }
        CloseHandle(process.hThread);
        Ok(process.hProcess)
    }
}

#[cfg(windows)]
fn same_authentication_id(left: Option<(u32, i32)>, right: Option<(u32, i32)>) -> bool {
    left.is_some() && left == right
}

#[cfg(windows)]
fn token_matches_authenticated_client(
    token: windows_sys::Win32::Foundation::HANDLE,
    session_id: u32,
    caller_sid: &str,
) -> bool {
    use windows_sys::Win32::Security::{GetTokenInformation, TokenSessionId};

    let mut actual_session = 0u32;
    let mut returned = 0u32;
    unsafe {
        GetTokenInformation(
            token,
            TokenSessionId,
            &mut actual_session as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        ) != 0
            && actual_session == session_id
            && session_token_sid(token)
                .as_deref()
                .is_some_and(|sid| sid == caller_sid)
    }
}

#[cfg(windows)]
fn token_authentication_id(token: windows_sys::Win32::Foundation::HANDLE) -> Option<(u32, i32)> {
    use windows_sys::Win32::Security::{GetTokenInformation, TokenStatistics, TOKEN_STATISTICS};

    let mut statistics = TOKEN_STATISTICS::default();
    let mut returned = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenStatistics,
            &mut statistics as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_STATISTICS>() as u32,
            &mut returned,
        ) != 0
    };
    ok.then_some((
        statistics.AuthenticationId.LowPart,
        statistics.AuthenticationId.HighPart,
    ))
}

#[cfg(windows)]
fn session_token_sid(token: windows_sys::Win32::Foundation::HANDLE) -> Option<String> {
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_USER};

    let mut size = 0u32;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size) };
    let mut buffer = vec![0u8; size as usize];
    let ok = size != 0
        && unsafe {
            GetTokenInformation(token, TokenUser, buffer.as_mut_ptr() as _, size, &mut size)
        } != 0;
    let sid = if ok {
        unsafe {
            crate::vault_access::sid_to_string(
                (buffer.as_ptr() as *const TOKEN_USER).read().User.Sid,
            )
        }
    } else {
        None
    };
    use zeroize::Zeroize;
    buffer.zeroize();
    sid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn signed_notification(token: &str) -> wincmd_shared::Envelope {
        wincmd_shared::Envelope::Notification(wincmd_shared::Notification {
            event: "vault-progress".to_string(),
            payload: serde_json::json!({"stage":"mount"}),
        })
        .sign(token)
    }

    #[cfg(windows)]
    #[test]
    fn signed_notification_before_matching_response_is_ignored() {
        let token = "broker-test-token";
        let mut notifications = 0;
        assert!(matches!(
            process_broker_reply(
                signed_notification(token),
                token,
                REQUEST_ID,
                &mut notifications
            ),
            Ok(BrokerReply::Notification)
        ));
        assert_eq!(notifications, 1);

        let response = wincmd_shared::Envelope::Response(wincmd_shared::Response {
            request_id: REQUEST_ID,
            result: serde_json::json!({"mounted":true}),
        })
        .sign(token);
        match process_broker_reply(response, token, REQUEST_ID, &mut notifications) {
            Ok(BrokerReply::Finished(Ok(result))) => {
                assert_eq!(result, serde_json::json!({"mounted":true}));
            }
            _ => panic!("matching response was not accepted after notification"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn broker_rejects_more_than_the_notification_bound() {
        let token = "broker-test-token";
        let mut notifications = 0;
        for _ in 0..MAX_SIGNED_NOTIFICATIONS {
            assert!(matches!(
                process_broker_reply(
                    signed_notification(token),
                    token,
                    REQUEST_ID,
                    &mut notifications
                ),
                Ok(BrokerReply::Notification)
            ));
        }
        assert!(matches!(
            process_broker_reply(
                signed_notification(token),
                token,
                REQUEST_ID,
                &mut notifications
            ),
            Err("broker_rejected")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn broker_rejects_signed_response_or_error_with_the_wrong_request_id() {
        let token = "broker-test-token";
        let mut notifications = 0;
        let response = wincmd_shared::Envelope::Response(wincmd_shared::Response {
            request_id: REQUEST_ID + 1,
            result: serde_json::Value::Null,
        })
        .sign(token);
        assert!(matches!(
            process_broker_reply(response, token, REQUEST_ID, &mut notifications),
            Err("broker_rejected")
        ));

        let error = wincmd_shared::Envelope::Error(wincmd_shared::ErrorReply {
            request_id: REQUEST_ID + 1,
            kind: "vault_acl_readback_failed".to_string(),
            message: "wrong request".to_string(),
        })
        .sign(token);
        assert!(matches!(
            process_broker_reply(error, token, REQUEST_ID, &mut notifications),
            Err("broker_rejected")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn broker_preserves_personal_vault_native_terminal_codes() {
        let token = "broker-test-token";
        for expected in [
            "vault_engine_unlock_failed",
            "vault_engine_drive_letter_unavailable",
            "vault_engine_mount_failed",
        ] {
            let reply = wincmd_shared::Envelope::Error(wincmd_shared::ErrorReply {
                request_id: REQUEST_ID,
                kind: expected.to_string(),
                message: "bounded failure".to_string(),
            })
            .sign(token);
            let mut notifications = 0;
            assert!(matches!(
                process_broker_reply(reply, token, REQUEST_ID, &mut notifications),
                Ok(BrokerReply::Finished(Err(actual))) if actual == expected
            ));
        }
    }

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

    #[cfg(windows)]
    #[test]
    fn per_user_mount_never_falls_back_to_a_wts_token() {
        assert!(!per_user_wts_fallback_allowed("vault.broker.mount"));
        assert!(!per_user_wts_fallback_allowed("vault.broker.unknown"));
        assert!(per_user_wts_fallback_allowed("vault.broker.dismount"));
    }

    #[cfg(windows)]
    #[test]
    fn duplicated_peer_token_must_keep_its_exact_logon_identity() {
        assert!(same_authentication_id(Some((42, -7)), Some((42, -7))));
        assert!(!same_authentication_id(Some((42, -7)), Some((43, -7))));
        assert!(!same_authentication_id(Some((42, -7)), Some((42, -6))));
        assert!(!same_authentication_id(Some((42, -7)), None));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn recovery_dismount_rejects_out_of_range_slots_before_launching_pro() {
        assert_eq!(
            vault_recovery_dismount(26).await,
            Err("broker_rejected".to_string())
        );
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
