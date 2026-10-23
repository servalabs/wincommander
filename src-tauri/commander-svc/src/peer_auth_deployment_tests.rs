// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::peer_auth::{PeerAuthProbe, RATE_LIMIT_MAX_CALLS};
use std::{path::PathBuf, sync::Arc};

const IMAGE: &str = r"C:\ProgramData\WinCommander\bin\wincommander-pro.exe";
const APPLY: &str = "svc.managed_deployment.apply";

struct Probe;
impl PeerAuthProbe for Probe {
    fn identity(&self, _: u32) -> Result<PeerIdentitySnapshot, PeerAuthError> {
        unreachable!()
    }
    fn active_interactive_session(&self) -> Result<u32, PeerAuthError> {
        Ok(1)
    }
}
fn gate() -> SessionHelperGate {
    SessionHelperGate::with_allowed_root(
        Arc::new(Probe),
        Some(PathBuf::from(r"C:\ProgramData\WinCommander\service")),
    )
}
fn identity(session_id: u32) -> PeerIdentitySnapshot {
    PeerIdentitySnapshot {
        session_id,
        canonical_image_path: IMAGE.into(),
    }
}

#[test]
fn signed_machine_relay_survives_rdp_disconnect_but_rejects_logged_off_or_unknown_sessions() {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSActive, WTSConnected, WTSDisconnected, WTSDown, WTSIdle, WTSListen, WTSReset,
    };
    for state in [WTSActive, WTSDisconnected] {
        assert!(deployment_session_is_permitted(2, Some(state), false));
        assert_eq!(
            gate().authorize_deployment_observation(
                &identity(2),
                APPLY,
                deployment_session_is_permitted(2, Some(state), false),
                Path::new(IMAGE),
                || Ok(())
            ),
            Ok(TrustOrigin::SessionHelperPinned)
        );
    }
    for state in [
        None,
        Some(WTSConnected),
        Some(WTSDown),
        Some(WTSIdle),
        Some(WTSListen),
        Some(WTSReset),
    ] {
        assert!(!deployment_session_is_permitted(2, state, false));
        assert_eq!(
            gate().authorize_deployment_observation(
                &identity(2),
                APPLY,
                deployment_session_is_permitted(2, state, false),
                Path::new(IMAGE),
                || Ok(())
            ),
            Err(PeerAuthError::WrongSession)
        );
    }
    assert!(deployment_session_is_permitted(0, None, true));
    assert!(!deployment_session_is_permitted(0, Some(WTSActive), false));
    assert!(!deployment_session_is_permitted(
        u32::MAX,
        Some(WTSActive),
        true
    ));
}

#[test]
fn managed_relay_accepts_verified_connected_rdp_or_system_without_changing_console_gate() {
    for session in [0, 2] {
        let gate = gate();
        assert_eq!(
            gate.authorize_deployment_observation(
                &identity(session),
                APPLY,
                true,
                Path::new(IMAGE),
                || Ok(())
            ),
            Ok(TrustOrigin::SessionHelperPinned)
        );
    }
    assert_eq!(
        gate().authorize_deployment_observation(
            &identity(2),
            APPLY,
            false,
            Path::new(IMAGE),
            || Ok(())
        ),
        Err(PeerAuthError::WrongSession)
    );
}

#[test]
fn managed_relay_rejects_foreign_paths_verbs_and_unverified_image_protection() {
    let gate = gate();
    let mut foreign = identity(2);
    foreign.canonical_image_path = r"C:\Users\user\wincommander-pro.exe".into();
    assert_eq!(
        gate.authorize_deployment_observation(&foreign, APPLY, true, Path::new(IMAGE), || panic!(
            "foreign image must fail before inspection"
        )),
        Err(PeerAuthError::PathNotAllowed)
    );
    for verb in [
        "svc.managed_deployment.bind",
        "svc.dispatch",
        "svc.clipboard.report_event",
        "svc.managed_deployment.apply.extra",
    ] {
        assert_eq!(
            gate.authorize_deployment_observation(
                &identity(2),
                verb,
                true,
                Path::new(IMAGE),
                || Ok(())
            ),
            Err(PeerAuthError::PathNotAllowed)
        );
    }
    assert_eq!(
        gate.authorize_deployment_observation(&identity(2), APPLY, true, Path::new(IMAGE), || Err(
            PeerAuthError::IdentityUnavailable
        )),
        Err(PeerAuthError::IdentityUnavailable)
    );
}

#[test]
fn managed_relay_keeps_per_session_path_verb_rate_limit() {
    let gate = gate();
    for _ in 0..RATE_LIMIT_MAX_CALLS {
        assert!(gate
            .authorize_deployment_observation(
                &identity(2),
                APPLY,
                true,
                Path::new(IMAGE),
                || Ok(())
            )
            .is_ok());
    }
    assert_eq!(
        gate.authorize_deployment_observation(&identity(2), APPLY, true, Path::new(IMAGE), || Ok(
            ()
        )),
        Err(PeerAuthError::RateLimited)
    );
    assert!(gate
        .authorize_deployment_observation(&identity(3), APPLY, true, Path::new(IMAGE), || Ok(()))
        .is_ok());
}
