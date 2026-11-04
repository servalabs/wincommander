// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded local transport for service-verified historical contact evidence.
use crate::fleet_machine_policy::FleetMachinePolicy;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use zeroize::Zeroize;

const CHUNK: usize = 1024 * 1024;
const REQUEST_MAX: usize = 2 * 1024 * 1024;
const RESPONSE_MAX: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Begin,
    Append {
        nonce: String,
        stream: String,
        offset: usize,
        data: String,
    },
    Finish {
        nonce: String,
        receipt: String,
    },
    Offer {
        credential: CredentialTransfer,
        connection_token: String,
    },
    Clear {
        token: String,
        connection_token: String,
    },
    ConnectionStart {
        trust: crate::fleet_machine_policy::FleetTrust,
        token: String,
        connection_token: String,
    },
    Get,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunningTransport {
    origins: Vec<String>,
    profile: Option<crate::fleet_machine_policy::TransportProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialTransfer {
    trust: crate::fleet_machine_policy::FleetTrust,
    transport: RunningTransport,
    origin: String,
    secret_b64: String,
}
impl Drop for CredentialTransfer {
    fn drop(&mut self) {
        self.secret_b64.zeroize();
    }
}

struct Transfer {
    nonce: String,
    request: Vec<u8>,
    response: Vec<u8>,
}
fn slot() -> &'static Mutex<Option<Transfer>> {
    static SLOT: OnceLock<Mutex<Option<Transfer>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

pub(crate) fn handle(store: &FleetMachinePolicy, args: Value) -> Result<Value, &'static str> {
    let operation: Operation =
        serde_json::from_value(args).map_err(|_| "invalid_contact_request")?;
    match operation {
        Operation::Begin => {
            let mut transfer = slot().lock().map_err(|_| "contact_unavailable")?;
            *transfer = None;
            let challenge = store.begin_contact()?;
            Ok(json!({"nonce": challenge.nonce}))
        }
        Operation::Append {
            nonce,
            stream,
            offset,
            data,
        } => {
            let mut slot = slot().lock().map_err(|_| "contact_unavailable")?;
            if !store.contact_pending(&nonce) {
                *slot = None;
                return Err("contact_unavailable");
            }
            if data.len() > CHUNK.div_ceil(3) * 4 {
                return Err("invalid_contact_chunk");
            }
            let bytes = STANDARD.decode(data).map_err(|_| "invalid_contact_chunk")?;
            if bytes.is_empty() || bytes.len() > CHUNK {
                return Err("invalid_contact_chunk");
            }
            if slot.is_none() && stream == "request" && offset == 0 {
                *slot = Some(Transfer {
                    nonce: nonce.clone(),
                    request: Vec::new(),
                    response: Vec::new(),
                });
            }
            let transfer = slot
                .as_mut()
                .filter(|t| t.nonce == nonce)
                .ok_or("contact_unavailable")?;
            let (target, limit) = match stream.as_str() {
                "request" if transfer.response.is_empty() => (&mut transfer.request, REQUEST_MAX),
                "response" if !transfer.request.is_empty() => {
                    (&mut transfer.response, RESPONSE_MAX)
                }
                _ => return Err("invalid_contact_chunk"),
            };
            if target.len() != offset
                || target
                    .len()
                    .checked_add(bytes.len())
                    .map_or(true, |n| n > limit)
            {
                return Err("invalid_contact_chunk");
            }
            target.extend_from_slice(&bytes);
            Ok(json!({"accepted": true}))
        }
        Operation::Finish { nonce, receipt } => {
            let mut slot = slot().lock().map_err(|_| "contact_unavailable")?;
            let transfer = slot
                .take()
                .filter(|t| t.nonce == nonce)
                .ok_or("contact_unavailable")?;
            let result =
                store.record_contact(&nonce, &transfer.request, &transfer.response, &receipt)?;
            Ok(json!({"observation": result}))
        }
        Operation::Offer { credential, connection_token } => {
            let secret = zeroize::Zeroizing::new(
                STANDARD
                    .decode(&credential.secret_b64)
                    .map_err(|_| "invalid_fleet_credential")?,
            );
            if secret.len() != 32 || STANDARD.encode(secret.as_slice()) != credential.secret_b64 {
                return Err("invalid_fleet_credential");
            }
            #[cfg(windows)]
            {
                crate::fleet_transport::offer(
                    store,
                    credential.trust.clone(),
                    credential.transport.origins.clone(),
                    credential.transport.profile.clone(),
                    credential.origin.clone(),
                    secret,
                    connection_token,
                )?;
                Ok(json!({"queued":true}))
            }
            #[cfg(not(windows))]
            {
                let _ = (credential, secret, connection_token);
                Err("fleet_credential_unavailable")
            }
        }
        Operation::Clear { token, connection_token } => Ok(json!({"cleared":store.clear_credential(&token, &connection_token)?})),
        Operation::ConnectionStart { trust, token, connection_token } => {
            let (connection_token, token) = store.start_connection(&trust, &token, &connection_token)?;
            Ok(json!({"connection_token":connection_token, "credential_clear_token":token}))
        }
        Operation::Get => {
            let contact = store.contact_state()?;
            #[cfg(windows)]
            let reachability = crate::fleet_transport::status(
                store,
                contact.as_ref().map(|(trust, _, _, _, revision)| (trust, *revision)),
            );
            #[cfg(not(windows))]
            let reachability = json!({"state":"unconfirmed"});
            match contact {
                Some((trust, observation, token, connection_token, _revision)) => Ok(
                    json!({"deviceId": trust.device_id, "observation": observation, "service_reachability":reachability, "credential_clear_token":token,
                        "connection_token":connection_token,
                        "serviceTrust":{"orgId":trust.org_id,"deviceId":trust.device_id,"enrollmentGeneration":trust.enrollment_generation,"signerKey":trust.signer_key}}),
                ),
                None => Ok(
                    json!({"deviceId": null, "observation": null, "service_reachability":reachability, "credential_clear_token":null, "serviceTrust":null, "connection_token":null}),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_size_evidence_fits_the_existing_helper_rate_limit_and_frames() {
        let calls = REQUEST_MAX.div_ceil(CHUNK) + RESPONSE_MAX.div_ceil(CHUNK) + 2;
        assert!(calls <= crate::peer_auth::RATE_LIMIT_MAX_CALLS as usize);
        let request = json!({"operation":"append", "nonce":"ci-00000000000000000000000000000000", "stream":"response", "offset":0, "data":STANDARD.encode(vec![0u8; CHUNK])});
        let frame = wincmd_shared::Envelope::Request(wincmd_shared::Request {
            request_id: 1,
            feature_id: "svc.fleet.contact_observation".into(),
            diagnostic_operation_id: None,
            args: request,
        })
        .sign("test-only-session");
        assert!(
            serde_json::to_vec(&frame).unwrap().len() < wincmd_shared::MAX_PAYLOAD_BYTES as usize
        );
    }

    #[test]
    fn proof_transfer_rejects_an_unattested_http_status() {
        let supplied =
            json!({"operation":"finish", "nonce":"ci-test", "receipt":"test", "status":200});
        assert!(serde_json::from_value::<Operation>(supplied).is_err());
        let absent = json!({"operation":"finish", "nonce":"ci-test", "receipt":"test"});
        assert!(matches!(
            serde_json::from_value::<Operation>(absent),
            Ok(Operation::Finish { .. })
        ));
    }
}
