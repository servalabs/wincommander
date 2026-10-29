// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signed machine deployments may arrive from a logged-on RDP session or SYSTEM.

use super::{paths_equal, PeerAuthError, PeerIdentitySnapshot, SessionHelperGate, TrustOrigin};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, time::Instant};
use windows_sys::Win32::System::RemoteDesktop::{
    WTSActive, WTSDisconnected, WTS_CONNECTSTATE_CLASS,
};

#[path = "peer_auth_deployment_acl.rs"]
mod acl;

#[cfg(debug_assertions)]
fn dev_helper_path() -> Result<std::path::PathBuf, PeerAuthError> {
    let root = std::env::var_os("ProgramData").ok_or(PeerAuthError::IdentityUnavailable)?;
    Ok(std::path::PathBuf::from(root)
        .join("WinCommander")
        .join("dev-fleet")
        .join("wincommander-free.exe"))
}

#[cfg(debug_assertions)]
fn dev_pin_digest_matches(candidate: &Path, expected: &Path, actual: &[u8], pin: &str) -> bool {
    paths_equal(candidate, expected)
        && pin.len() == 64
        && pin.bytes().all(|c| c.is_ascii_hexdigit())
        && actual
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            .eq_ignore_ascii_case(pin)
}

#[cfg(debug_assertions)]
fn dev_helper_matches(path: &Path) -> bool {
    let Ok(expected) = dev_helper_path()
        .and_then(|p| std::fs::canonicalize(p).map_err(|_| PeerAuthError::IdentityUnavailable))
    else {
        return false;
    };
    if !paths_equal(path, &expected) || acl::verify_path(&expected).is_err() {
        return false;
    }
    let pin_path = expected.with_file_name("wincommander-free.sha256");
    if acl::verify_component(&pin_path).is_err() {
        return false;
    }
    let Ok(pin) = std::fs::read_to_string(&pin_path) else {
        return false;
    };
    let pin = pin.trim();
    let Ok(mut file) = File::open(&expected) else {
        return false;
    };
    hash(&mut file).is_ok_and(|current| dev_pin_digest_matches(path, &expected, &current, pin))
}

pub(crate) fn deployment_session_is_permitted(
    session_id: u32,
    state: Option<WTS_CONNECTSTATE_CLASS>,
    is_system: bool,
) -> bool {
    // Disconnecting RDP must not revoke an already-authorized machine operation.
    // This does not authorize a logoff/unknown session or create a management pin.
    if session_id == 0 {
        return is_system;
    }
    session_id != u32::MAX && (state == Some(WTSActive) || state == Some(WTSDisconnected))
}

impl SessionHelperGate {
    /// Fleet policy only: admit a protected Free/Pro helper from its actual
    /// active Windows session, including RDP, while retaining path and ACL
    /// checks. The session comes from the captured pipe token, never the UI.
    pub(crate) fn authorize_fleet_policy_relay(
        &self,
        pid: u32,
        verb: &str,
        captured_session: u32,
        active: bool,
    ) -> Result<TrustOrigin, PeerAuthError> {
        let identity = self.probe.identity(pid)?;
        self.authorize_fleet_policy_observation(&identity, verb, captured_session, active, || {
            verify_image(
                &identity.canonical_image_path,
                &identity.canonical_image_path,
            )
        })
    }

    fn authorize_fleet_policy_observation(
        &self,
        identity: &PeerIdentitySnapshot,
        verb: &str,
        captured_session: u32,
        active: bool,
        verify: impl FnOnce() -> Result<(), PeerAuthError>,
    ) -> Result<TrustOrigin, PeerAuthError> {
        if !matches!(verb, "svc.fleet.install_policy" | "svc.fleet.get_policy") {
            return Err(PeerAuthError::PathNotAllowed);
        }
        if !active || captured_session == 0 || identity.session_id != captured_session {
            return Err(PeerAuthError::WrongSession);
        }
        let allowed_name = identity
            .canonical_image_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| {
                n.eq_ignore_ascii_case("wincommander-free.exe")
                    || n.eq_ignore_ascii_case("wincommander-pro.exe")
            });
        let installed_pro = identity
            .canonical_image_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("wincommander-pro.exe"))
            && std::fs::canonicalize(crate::pro_broker::fixed_pro_path())
                .ok()
                .is_some_and(|expected| paths_equal(&identity.canonical_image_path, &expected));
        #[cfg(debug_assertions)]
        let staged_dev = dev_helper_matches(&identity.canonical_image_path);
        #[cfg(not(debug_assertions))]
        let staged_dev = false;
        if !allowed_name
            || (!self.is_allowed_helper_path(&identity.canonical_image_path)
                && !installed_pro
                && !staged_dev)
        {
            return Err(PeerAuthError::PathNotAllowed);
        }
        verify()?;
        self.check_rate_limit(identity, verb, Instant::now())?;
        Ok(TrustOrigin::SessionHelperPinned)
    }

    pub(crate) fn authorize_deployment_relay(
        &self,
        pid: u32,
        verb: &str,
        permitted_session: bool,
    ) -> Result<TrustOrigin, PeerAuthError> {
        let identity = self.probe.identity(pid)?;
        let expected = crate::pro_broker::fixed_pro_path();
        let canonical =
            std::fs::canonicalize(&expected).map_err(|_| PeerAuthError::PathNotAllowed)?;
        self.authorize_deployment_observation(
            &identity,
            verb,
            permitted_session,
            &canonical,
            || verify_image(&identity.canonical_image_path, &expected),
        )
    }

    fn authorize_deployment_observation(
        &self,
        identity: &PeerIdentitySnapshot,
        verb: &str,
        permitted_session: bool,
        expected: &Path,
        verify: impl FnOnce() -> Result<(), PeerAuthError>,
    ) -> Result<TrustOrigin, PeerAuthError> {
        if !matches!(
            verb,
            "svc.managed_deployment.apply" | "svc.managed_deployment.status"
        ) {
            return Err(PeerAuthError::PathNotAllowed);
        }
        if !permitted_session {
            return Err(PeerAuthError::WrongSession);
        }
        if !paths_equal(&identity.canonical_image_path, expected) {
            return Err(PeerAuthError::PathNotAllowed);
        }
        verify()?;
        self.check_rate_limit(identity, verb, Instant::now())?;
        Ok(TrustOrigin::SessionHelperPinned)
    }
}

fn verify_image(actual: &Path, expected: &Path) -> Result<(), PeerAuthError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    // Pin both the executable and its containing directories against unprivileged replacement.
    acl::verify_path(expected)?;
    let mut actual_file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(actual)
        .map_err(|_| PeerAuthError::IdentityUnavailable)?;
    let mut expected_file = File::open(expected).map_err(|_| PeerAuthError::IdentityUnavailable)?;
    if hash(&mut actual_file)? != hash(&mut expected_file)? {
        return Err(PeerAuthError::PathNotAllowed);
    }
    Ok(())
}

fn hash(file: &mut File) -> Result<Vec<u8>, PeerAuthError> {
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|_| PeerAuthError::IdentityUnavailable)?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(digest.finalize().to_vec())
}

#[cfg(test)]
#[path = "peer_auth_deployment_tests.rs"]
mod tests;
