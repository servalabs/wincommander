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
