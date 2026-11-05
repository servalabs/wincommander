// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unwired protected authority and restart decision for settings generations.
//! Caller must hold the generation lock, authorize/pin storage, and independently
//! validate any proposed generation. Protected records do not prevent rollback
//! of both the record and generation: that needs an external trusted anchor.
#![allow(dead_code)]
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const FORMAT: &str = "wincommander-settings-authority-v1";
const MAX_RECORD: usize = 65_536;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record { format: String, state: State }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum State {
    Stable { digest: String },
    Pending { prior: Option<String>, next: String },
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Recovery { Legacy, Selected, InterruptedInitial }

fn digest(bytes: &[u8]) -> String { B64.encode(Sha256::digest(bytes)) }
fn valid_digest(s: &str) -> bool { B64.decode(s).is_ok_and(|d| d.len() == 32) }
fn parse(bytes: &[u8], unprotect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>) -> Result<Record, String> {
    if bytes.is_empty() || bytes.len() > MAX_RECORD { return Err("invalid settings authority".into()); }
    let plain = Zeroizing::new(unprotect(bytes)?);
    let record: Record = serde_json::from_slice(&plain).map_err(|_| "invalid settings authority")?;
    let valid = match &record.state {
        State::Stable { digest } => valid_digest(digest),
        State::Pending { prior, next } => valid_digest(next) && prior.as_ref().is_none_or(|v| valid_digest(v) && v != next),
    };
    if record.format != FORMAT || !valid { return Err("invalid settings authority".into()); }
    Ok(record)
}
fn encode(state: State, protect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>, unprotect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let record = Record { format: FORMAT.into(), state };
    let plain = Zeroizing::new(serde_json::to_vec(&record).map_err(|_| "cannot encode settings authority")?);
    let wrapped = protect(&plain)?;
    if parse(&wrapped, unprotect)? != record { return Err("settings authority protection failed readback".into()); }
    Ok(wrapped)
}

/// Read-only classification. Pending records require a separate exclusive
/// repair; observing one never grants permission to write or to fall back.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Inspection { Legacy, Stable, RepairRequired, InitialVerificationRequired }
pub(super) fn inspect(
    protected: Option<&[u8]>, selected: Option<&[u8]>,
    unprotect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Inspection, String> {
    let Some(raw) = protected else {
        return if selected.is_none() { Ok(Inspection::Legacy) } else { Err("selected settings lack authority".into()) };
    };
    let current = selected.map(digest);
    match parse(raw, unprotect)?.state {
        State::Stable { digest } if current.as_ref() == Some(&digest) => Ok(Inspection::Stable),
        State::Stable { .. } => Err("selected settings do not match authority".into()),
        State::Pending { prior: None, next } if current.is_none() || current.as_ref() == Some(&next) => Ok(Inspection::InitialVerificationRequired),
        State::Pending { prior: Some(prior), next } if current.as_ref() == Some(&prior) || current.as_ref() == Some(&next) => Ok(Inspection::RepairRequired),
        State::Pending { .. } => Err("interrupted settings generation cannot be reconciled".into()),
    }
}

/// Reconcile an already-read protected authority and bounded selected bytes.
/// Writes only the authority through caller-supplied atomic publication, whose
/// result must be bytes reopened independently from disk. Caller must retain
/// exclusive ownership across read, verification and publication.
pub(super) fn recover(
    protected: Option<&[u8]>, selected: Option<&[u8]>, initial_snapshot_verified: bool,
    protect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>,
    unprotect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>,
    write_authority: &impl Fn(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Recovery, String> {
    let Some(raw) = protected else {
        return if selected.is_none() { Ok(Recovery::Legacy) } else { Err("selected settings lack authority".into()) };
    };
    let record = parse(raw, unprotect)?;
    let current = selected.map(digest);
    match record.state {
        State::Stable { digest } if current.as_ref() == Some(&digest) => Ok(Recovery::Selected),
        State::Stable { .. } => Err("selected settings do not match authority".into()),
        State::Pending { prior: None, next } if current.as_ref() == Some(&next) && !initial_snapshot_verified => {
            Err("initial settings selection requires a fresh guarded source snapshot".into())
        },
        State::Pending { prior: _, next } if current.as_ref() == Some(&next) => {
            let wrapped = encode(State::Stable { digest: next }, protect, unprotect)?;
            let observed = write_authority(&wrapped)?;
            if observed != wrapped || parse(&observed, unprotect)? != parse(&wrapped, unprotect)? {
                return Err("settings authority publication did not verify".into());
            }
            Ok(Recovery::Selected)
        },
        State::Pending { prior: Some(prior), .. } if current.as_ref() == Some(&prior) => {
            let wrapped = encode(State::Stable { digest: prior }, protect, unprotect)?;
            let observed = write_authority(&wrapped)?;
            if observed != wrapped || parse(&observed, unprotect)? != parse(&wrapped, unprotect)? {
                return Err("settings authority publication did not verify".into());
            }
            Ok(Recovery::Selected)
        },
        State::Pending { prior: None, .. } if current.is_none() => Ok(Recovery::InterruptedInitial),
        State::Pending { .. } => Err("interrupted settings generation cannot be reconciled".into()),
    }
}

/// Make an authority for a planned transition; publish it before publishing
/// the generation. `prior` is the exact previously authenticated generation.
pub(super) fn pending(prior: Option<&[u8]>, next: &[u8], protect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>, unprotect: &impl Fn(&[u8]) -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let state = State::Pending { prior: prior.map(digest), next: digest(next) };
    encode(state, protect, unprotect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    fn identity(input: &[u8]) -> Result<Vec<u8>, String> { Ok(input.to_vec()) }
    #[test]
    fn recovery_commits_new_or_restores_old_and_rejects_unknown_bytes() {
        let marker = pending(Some(b"old"), b"new", &identity, &identity).unwrap();
        assert_eq!(inspect(Some(&marker), Some(b"old"), &identity).unwrap(), Inspection::RepairRequired);
        assert_eq!(inspect(Some(&marker), Some(b"new"), &identity).unwrap(), Inspection::RepairRequired);
        assert!(inspect(Some(&marker), Some(b"other"), &identity).is_err());
        let written = RefCell::new(Vec::new());
        let persist = |b: &[u8]| { *written.borrow_mut() = b.to_vec(); Ok(b.to_vec()) };
        assert_eq!(recover(Some(&marker), Some(b"new"), false, &identity, &identity, &persist).unwrap(), Recovery::Selected);
        assert_eq!(recover(Some(&written.borrow()), Some(b"new"), false, &identity, &identity, &persist).unwrap(), Recovery::Selected);
        assert!(recover(Some(&written.borrow()), Some(b"old"), false, &identity, &identity, &persist).is_err());
        assert_eq!(recover(Some(&marker), Some(b"old"), false, &identity, &identity, &persist).unwrap(), Recovery::Selected);
        assert!(recover(Some(&marker), Some(b"other"), false, &identity, &identity, &persist).is_err());
        assert!(recover(Some(&marker), None, false, &identity, &identity, &persist).is_err());
    }
    #[test]
    fn initial_interruption_never_selects_absent_or_unauthorized_data() {
        let marker = pending(None, b"new", &identity, &identity).unwrap();
        assert_eq!(inspect(Some(&marker), Some(b"new"), &identity).unwrap(), Inspection::InitialVerificationRequired);
        assert_eq!(inspect(Some(&marker), None, &identity).unwrap(), Inspection::InitialVerificationRequired);
        let persist = |b: &[u8]| Ok(b.to_vec());
        assert_eq!(recover(None, None, false, &identity, &identity, &persist).unwrap(), Recovery::Legacy);
        assert!(recover(None, Some(b"new"), false, &identity, &identity, &persist).is_err());
        assert_eq!(recover(Some(&marker), None, false, &identity, &identity, &persist).unwrap(), Recovery::InterruptedInitial);
        assert!(recover(Some(b"bad"), Some(b"new"), false, &identity, &identity, &persist).is_err());
        assert!(recover(Some(&marker), Some(b"new"), false, &identity, &identity, &persist).is_err());
        assert_eq!(recover(Some(&marker), Some(b"new"), true, &identity, &identity, &persist).unwrap(), Recovery::Selected);
        assert!(recover(Some(&marker), Some(b"new"), true, &identity, &identity, &|_| Err("disk failed".into())).is_err());
        assert!(recover(Some(&marker), Some(b"new"), true, &identity, &identity, &|_| Ok(b"wrong readback".to_vec())).is_err());
    }
}
