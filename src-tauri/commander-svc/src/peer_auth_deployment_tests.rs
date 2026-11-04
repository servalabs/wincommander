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
fn shared_shortcuts_require_the_pinned_free_image_and_captured_session() {
    let gate = SessionHelperGate::with_allowed_root(
        Arc::new(Probe), Some(PathBuf::from(r"C:\Program Files\WinCommander")),
    );
    let free = PeerIdentitySnapshot {
        session_id: 7,
        canonical_image_path: PathBuf::from(r"C:\Program Files\WinCommander\wincommander-free.exe"),
    };
    assert_eq!(gate.authorize_shared_shortcut_observation(&free, 7, true, || Ok(())),
        Ok(TrustOrigin::SessionHelperPinned));
    assert_eq!(gate.authorize_shared_shortcut_observation(&free, 8, true, || Ok(())),
        Err(PeerAuthError::WrongSession));
    assert_eq!(gate.authorize_shared_shortcut_observation(&free, 7, false, || Ok(())),
        Err(PeerAuthError::WrongSession));
    assert_eq!(gate.authorize_shared_shortcut_observation(&free, 7, true,
        || Err(PeerAuthError::IdentityUnavailable)), Err(PeerAuthError::IdentityUnavailable));
    let mut pro = free.clone();
    pro.canonical_image_path = PathBuf::from(r"C:\Program Files\WinCommander\wincommander-pro.exe");
    assert_eq!(gate.authorize_shared_shortcut_observation(&pro, 7, true,
        || panic!("Pro must not change shared links")), Err(PeerAuthError::PathNotAllowed));
    let mut sibling = free.clone();
    sibling.canonical_image_path = PathBuf::from(r"C:\Program Files\WinCommander\other.exe");
    assert_eq!(gate.authorize_shared_shortcut_observation(&sibling, 7, true,
        || panic!("other installed image must not change shared links")), Err(PeerAuthError::PathNotAllowed));
    let mut foreign = free.clone();
    foreign.canonical_image_path = PathBuf::from(r"C:\Users\Public\wincommander-free.exe");
    assert_eq!(gate.authorize_shared_shortcut_observation(&foreign, 7, true,
        || panic!("untrusted path must be denied")), Err(PeerAuthError::PathNotAllowed));
}

#[test]
fn licence_relay_accepts_active_remote_session_but_rejects_untrusted_peers() {
    use wincmd_shared::svc::{ENROL_HYBRID_LICENSE_VERB, RENEW_HYBRID_LICENSE_VERB};
    let gate = SessionHelperGate::with_allowed_root(Arc::new(Probe),
        Some(PathBuf::from(r"C:\ProgramData\WinCommander\bin")));
    let peer = identity(7); // The console session in this test is 1, not 7.
    for verb in [ENROL_HYBRID_LICENSE_VERB, RENEW_HYBRID_LICENSE_VERB] {
        assert_eq!(gate.authorize_licence_observation(&peer, verb, 7, true, || Ok(())),
            Ok(TrustOrigin::SessionHelperPinned));
        assert_eq!(gate.authorize_licence_observation(&peer, verb, 8, true, || Ok(())),
            Err(PeerAuthError::WrongSession));
        assert_eq!(gate.authorize_licence_observation(&peer, verb, 7, false, || Ok(())),
            Err(PeerAuthError::WrongSession));
        assert_eq!(gate.authorize_licence_observation(&peer, verb, 7, true,
            || Err(PeerAuthError::IdentityUnavailable)), Err(PeerAuthError::IdentityUnavailable));
    }
    let mut wrong = peer.clone();
    wrong.canonical_image_path = r"C:\ProgramData\WinCommander\bin\wincommander-clipboard-guard.exe".into();
    assert_eq!(gate.authorize_licence_observation(&wrong, RENEW_HYBRID_LICENSE_VERB, 7, true,
        || panic!("wrong helper must not be authorized")), Err(PeerAuthError::PathNotAllowed));
    wrong.canonical_image_path = r"C:\Users\Public\wincommander-free.exe".into();
    assert_eq!(gate.authorize_licence_observation(&wrong, RENEW_HYBRID_LICENSE_VERB, 7, true,
        || panic!("untrusted path must not be authorized")), Err(PeerAuthError::PathNotAllowed));
    assert_eq!(gate.authorize_licence_observation(&peer, "svc.unknown", 7, true,
        || panic!("unexpected verb must not be authorized")), Err(PeerAuthError::PathNotAllowed));
}

#[test]
fn licence_relay_rate_limits_each_verb_and_session() {
    use wincmd_shared::svc::RENEW_HYBRID_LICENSE_VERB;
    let gate = SessionHelperGate::with_allowed_root(Arc::new(Probe),
        Some(PathBuf::from(r"C:\ProgramData\WinCommander\bin")));
    for _ in 0..RATE_LIMIT_MAX_CALLS {
        assert!(gate.authorize_licence_observation(&identity(7), RENEW_HYBRID_LICENSE_VERB,
            7, true, || Ok(())).is_ok());
    }
    assert_eq!(gate.authorize_licence_observation(&identity(7), RENEW_HYBRID_LICENSE_VERB,
        7, true, || Ok(())), Err(PeerAuthError::RateLimited));
}

#[test]
fn fleet_policy_relay_requires_matching_active_session_and_protected_helper() {
    let gate = SessionHelperGate::with_allowed_root(
        Arc::new(Probe),
        Some(PathBuf::from(r"C:\ProgramData\WinCommander\bin")),
    );
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &identity(2),
            "svc.fleet.get_policy",
            2,
            true,
            || Ok(())
        ),
        Ok(TrustOrigin::SessionHelperPinned)
    );
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &identity(2),
            "svc.fleet.get_policy",
            3,
            true,
            || Ok(())
        ),
        Err(PeerAuthError::WrongSession)
    );
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &identity(2),
            "svc.fleet.get_policy",
            2,
            false,
            || Ok(())
        ),
        Err(PeerAuthError::WrongSession)
    );
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &identity(2),
            "svc.fleet.replace_trust",
            2,
            true,
            || Ok(())
        ),
        Err(PeerAuthError::PathNotAllowed)
    );
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &identity(2),
            "svc.fleet.get_policy",
            2,
            true,
            || Err(PeerAuthError::PathNotAllowed)
        ),
        Err(PeerAuthError::PathNotAllowed)
    );
    let mut foreign = identity(2);
    foreign.canonical_image_path = r"C:\Users\other\wincommander-pro.exe".into();
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &foreign,
            "svc.fleet.get_policy",
            2,
            true,
            || panic!("foreign binary must be denied")
        ),
        Err(PeerAuthError::PathNotAllowed)
    );
    let mut other_helper = identity(2);
    other_helper.canonical_image_path =
        r"C:\ProgramData\WinCommander\bin\wincommander-clipboard-guard.exe".into();
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &other_helper,
            "svc.fleet.get_policy",
            2,
            true,
            || panic!("wrong helper must be denied")
        ),
        Err(PeerAuthError::PathNotAllowed)
    );
}

#[test]
fn hybrid_trust_read_requires_active_session_and_verified_installed_helper() {
    let gate = SessionHelperGate::with_allowed_root(
        Arc::new(Probe), Some(PathBuf::from(r"C:\ProgramData\WinCommander\bin")),
    );
    let verb = "svc.fleet.read_hybrid_trust";
    assert_eq!(gate.authorize_fleet_policy_observation(&identity(2), verb, 2, true, || Ok(())),
        Ok(TrustOrigin::SessionHelperPinned));
    assert_eq!(gate.authorize_fleet_policy_observation(&identity(2), verb, 3, true, || Ok(())),
        Err(PeerAuthError::WrongSession));
    assert_eq!(gate.authorize_fleet_policy_observation(&identity(2), verb, 2, false, || Ok(())),
        Err(PeerAuthError::WrongSession));
    assert_eq!(gate.authorize_fleet_policy_observation(&identity(2), verb, 2, true, || Err(PeerAuthError::PathNotAllowed)),
        Err(PeerAuthError::PathNotAllowed));
    let mut foreign = identity(2);
    foreign.canonical_image_path = r"C:\Users\other\wincommander-pro.exe".into();
    assert_eq!(gate.authorize_fleet_policy_observation(&foreign, verb, 2, true, || panic!("foreign image must not be verified")),
        Err(PeerAuthError::PathNotAllowed));
}

#[test]
fn fleet_policy_relay_retains_per_session_rate_limit() {
    let gate = SessionHelperGate::with_allowed_root(
        Arc::new(Probe),
        Some(PathBuf::from(r"C:\ProgramData\WinCommander\bin")),
    );
    for _ in 0..RATE_LIMIT_MAX_CALLS {
        assert!(gate
            .authorize_fleet_policy_observation(
                &identity(2),
                "svc.fleet.get_policy",
                2,
                true,
                || Ok(())
            )
            .is_ok());
    }
    assert_eq!(
        gate.authorize_fleet_policy_observation(
            &identity(2),
            "svc.fleet.get_policy",
            2,
            true,
            || Ok(())
        ),
        Err(PeerAuthError::RateLimited)
    );
}

#[cfg(debug_assertions)]
#[test]
fn debug_helper_pin_requires_exact_path_and_digest() {
    let path = Path::new(r"C:\ProgramData\WinCommander\dev-fleet\wincommander-free.exe");
    let digest = [0xabu8; 32];
    let pin = "ab".repeat(32);
    assert!(dev_pin_digest_matches(path, path, &digest, &pin));
    assert!(!dev_pin_digest_matches(
        Path::new(IMAGE),
        path,
        &digest,
        &pin
    ));
    assert!(!dev_pin_digest_matches(
        path,
        path,
        &digest,
        &"cd".repeat(32)
    ));
    assert!(!dev_pin_digest_matches(path, path, &digest, "bad"));
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
