// SPDX-License-Identifier: AGPL-3.0-or-later
//! Disabled service-originated confirmation for a captured interactive session.
use super::{AuthenticatedPipePeer, BoundRequest, NativeConfirmation, Peer};
use super::super::{peer_has_active_interactive_session, service_peer_identity};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::RemoteDesktop::{WTSQueryUserToken, WTSSendMessageW, WTS_CURRENT_SERVER_HANDLE},
    UI::WindowsAndMessaging::{IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO},
};

// The pipe endpoint, helper security review, and interactive Windows acceptance
// are not complete. This must remain false until all are verified.
const NATIVE_CONFIRMATION_READY: bool = false;
const TIMEOUT_SECONDS: u32 = 60;

pub(super) fn canonical_uuid(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(i, c)| {
        if matches!(i, 8 | 13 | 18 | 23) { c == b'-' }
        else { c.is_ascii_digit() || (b'a'..=b'f').contains(&c) }
    }) && value != "00000000-0000-0000-0000-000000000000"
}

fn message(request: &BoundRequest) -> Result<String, &'static str> {
    let recovery_id = &request.recovery_id;
    if !canonical_uuid(recovery_id) { return Err("invalid recovery request"); }
    let fingerprint: String = request.replacement_fingerprint.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("Approve this Fleet device identity recovery?\n\nRecovery ID: {recovery_id}\nReplacement key SHA-256: {fingerprint}\n\nCompare this fingerprint with the Fleet administrator. A separate Fleet administrator approval is still required."))
}

fn session_matches(peer: &AuthenticatedPipePeer) -> bool {
    if !peer_has_active_interactive_session(peer) { return false; }
    let mut token = std::ptr::null_mut();
    if unsafe { WTSQueryUserToken(peer.session_id(), &mut token) } == 0 || token.is_null() { return false; }
    let matches = service_peer_identity::token_sid(token).as_deref() == Some(peer.caller_sid())
        && service_peer_identity::token_session_id(token) == Some(peer.session_id())
        && service_peer_identity::token_authentication_id(token) == Some(peer.authentication_id());
    unsafe { CloseHandle(token); }
    matches
}

fn is_approved(api_succeeded: bool, response: i32, peer_unchanged: bool) -> bool {
    api_succeeded && response == IDYES && peer_unchanged
}

/// Must run on a blocking worker, never an async service pipe executor.
pub(super) fn confirm(peer: &AuthenticatedPipePeer, request: &BoundRequest) -> Result<NativeConfirmation, &'static str> {
    if !NATIVE_CONFIRMATION_READY { return Err("protected native recovery confirmation is unavailable"); }
    let captured = Peer::capture(peer)?;
    if !session_matches(peer) { return Err("interactive session could not be verified"); }
    let text = message(request)?;
    let title: Vec<u16> = "WinCommander Fleet identity recovery".encode_utf16().chain(std::iter::once(0)).collect();
    let body: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut response = 0;
    let success = unsafe { WTSSendMessageW(WTS_CURRENT_SERVER_HANDLE, captured.session,
        title.as_ptr(), ((title.len() - 1) * 2) as u32, body.as_ptr(), ((body.len() - 1) * 2) as u32,
        MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2, TIMEOUT_SECONDS, &mut response, 1) } != 0;
    let unchanged = Peer::capture(peer).ok().as_ref() == Some(&captured) && session_matches(peer);
    if !is_approved(success, response, unchanged) { return Err("recovery confirmation denied or unavailable"); }
    Ok(NativeConfirmation::Verified { peer: captured, request: request.clone() })
}
