// SPDX-License-Identifier: AGPL-3.0-or-later
//! Optional service-to-Fleet reachability. It never polls commands or acts on silence.
use crate::fleet_machine_policy::{FleetMachinePolicy, FleetTrust, TransportProfile};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct Status {
    state: &'static str,
    observed_unix_ms: Option<u64>,
    fence: Option<(FleetTrust, u64)>,
}
fn status_slot() -> &'static Mutex<Status> {
    static STATUS: OnceLock<Mutex<Status>> = OnceLock::new();
    STATUS.get_or_init(|| {
        Mutex::new(Status {
            state: "starting",
            observed_unix_ms: None,
            fence: None,
        })
    })
}
fn set_status(
    state: &'static str,
    observed_unix_ms: Option<u64>,
    fence: Option<(FleetTrust, u64)>,
) {
    if let Ok(mut status) = status_slot().lock() {
        *status = Status {
            state,
            observed_unix_ms,
            fence,
        };
    }
}

/// This is an observation of the service loop, never proof of current health.
pub(crate) fn status(store: &FleetMachinePolicy) -> serde_json::Value {
    let Ok(status) = status_slot().lock() else {
        return serde_json::json!({"state":"unconfirmed"});
    };
    if let Some((trust, revision)) = &status.fence {
        if !store.credential_revision_is_current(trust, *revision) {
            return serde_json::json!({"state":"unconfirmed"});
        }
    }
    serde_json::json!({"state":status.state,"observed_unix_ms":status.observed_unix_ms})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Observed,
    Superseded,
    Rejected,
    Unsupported,
    Unverified,
    Unreachable,
}

async fn attempt(
    client: &reqwest::Client,
    trust: &FleetTrust,
    origins: &[String],
    secret: &[u8],
    is_current: impl Fn() -> bool,
) -> Outcome {
    let mut result = Outcome::Unreachable;
    for origin in origins {
        if !is_current() {
            return Outcome::Superseded;
        }
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => match i64::try_from(duration.as_secs()) {
                Ok(value) => value,
                Err(_) => return Outcome::Unverified,
            },
            Err(_) => return Outcome::Unverified,
        };
        let (nonce, request) = match crate::fleet_presence::make_request(trust, origin, secret, now)
        {
            Ok(request) => request,
            Err(_) => return Outcome::Unverified,
        };
        let pending = async {
            let mut response = client
                .post(format!("{origin}{}", crate::fleet_presence::PATH))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(request.clone())
                .send()
                .await
                .map_err(|_| Outcome::Unreachable)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(Outcome::Unsupported);
            }
            if response.status().is_client_error()
                && response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS
            {
                return Err(Outcome::Rejected);
            }
            if response.status() != reqwest::StatusCode::OK {
                return Err(Outcome::Unreachable);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| Outcome::Unreachable)? {
                if body
                    .len()
                    .checked_add(chunk.len())
                    .map_or(true, |size| size > crate::fleet_presence::MAX_RESPONSE)
                {
                    return Err(Outcome::Unverified);
                }
                body.extend_from_slice(&chunk);
            }
            if crate::fleet_presence::verify_response(trust, &nonce, &request, &body) {
                Ok(())
            } else {
                Err(Outcome::Unverified)
            }
        };
        match tokio::time::timeout(Duration::from_secs(30), pending).await {
            Ok(Ok(())) => return Outcome::Observed,
            Ok(Err(error)) => result = error,
            Err(_) => result = Outcome::Unreachable,
        }
    }
    result
}

/// Background operation never waits on the interactive agent and never re-enrolls.
#[cfg(windows)]
pub(crate) async fn run() {
    tokio::spawn(candidate_worker());
    let mut cached: Option<(TransportProfile, reqwest::Client)> = None;
    let mut failures: u32 = 0;
    loop {
        let store = match crate::fleet_machine_policy::system_store() {
            Ok(store) => store,
            Err(_) => {
                set_status("service_store_unavailable", None, None);
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        let snapshot = match store.credential_snapshot(&crate::fleet_credential::MachineDpapi) {
            Ok(Some(value)) => value,
            Ok(None) => {
                set_status(store.credential_missing_reason(), None, None);
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            Err(reason) => {
                set_status(
                    if reason == "network_configuration_differs" {
                        reason
                    } else {
                        "credential_unavailable"
                    },
                    None,
                    None,
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        let fence = Some((snapshot.trust.clone(), snapshot.revision));
        if crate::fleet_runtime_env::fingerprint().as_deref()
            != Some(snapshot.profile.environment_fingerprint.as_str())
        {
            set_status("network_configuration_differs", None, fence);
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        if cached
            .as_ref()
            .map_or(true, |(profile, _)| profile != &snapshot.profile)
        {
            let pins = fleet_agent_core::SpkiPinSet::from_hex_list(&snapshot.profile.spki_pins);
            if !snapshot.profile.spki_pins.trim().is_empty() && pins.is_none() {
                set_status("transport_security_invalid", None, fence);
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            match fleet_agent_core::build_client_without_redirects(pins) {
                Ok(client) => cached = Some((snapshot.profile.clone(), client)),
                Err(_) => {
                    set_status("transport_security_invalid", None, fence);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            }
        }
        let Some((_, client)) = &cached else {
            continue;
        };
        set_status("checking", None, fence.clone());
        let result = attempt(
            client,
            &snapshot.trust,
            &snapshot.origins,
            &snapshot.secret,
            || store.credential_is_current(&snapshot),
        )
        .await;
        if !store.credential_is_current(&snapshot) {
            continue;
        }
        match result {
            Outcome::Superseded => continue,
            Outcome::Observed => {
                failures = 0;
                let observed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|d| u64::try_from(d.as_millis()).ok());
                set_status("observed", observed, fence);
            }
            Outcome::Rejected => {
                failures = failures.saturating_add(1);
                set_status("request_rejected", None, fence);
            }
            Outcome::Unsupported => {
                failures = failures.saturating_add(1);
                set_status("server_unsupported", None, fence);
            }
            Outcome::Unverified => {
                failures = failures.saturating_add(1);
                set_status("proof_unverified", None, fence);
            }
            Outcome::Unreachable => {
                failures = failures.saturating_add(1);
                set_status("network_unavailable", None, fence);
            }
        }
        let extra = failures.saturating_sub(1).min(5);
        let seconds = (10_u64 << extra).min(300);
        let jitter = u64::from(rand::random::<u8>()) % (seconds / 10 + 1);
        tokio::time::sleep(Duration::from_secs(seconds + jitter)).await;
    }
}

#[path = "fleet_transport_candidate.rs"]
mod candidate;
#[cfg(windows)]
use candidate::candidate_worker;

#[cfg(test)]
#[path = "fleet_transport_tests.rs"]
mod tests;
#[cfg(windows)]
pub(crate) use candidate::offer;
