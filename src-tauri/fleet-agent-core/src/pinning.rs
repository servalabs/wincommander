//! Optional TLS server-certificate pinning by SPKI SHA-256 hash.
//!
//! Gated behind `feature = "transport"` (it builds the `reqwest::Client`).
//!
//! # Why SPKI-hash, not a leaf-cert PEM
//!
//! A PEM-embedded leaf certificate carries the issuer/CN/SAN strings of the
//! real fleet server in plaintext inside the binary — those read as hostname
//! literals and can trip an AV-hygiene "no URL/hostname in the Free binary"
//! strings scan. A SHA-256 hash of the DER-encoded SubjectPublicKeyInfo is 32
//! opaque bytes (rendered here as a hex string for config transport) with no
//! recoverable hostname, issuer, or CN — it cannot trip that kind of scan.
//! (In this workspace only the `types`-only `fleet-agent-core` slice, which
//! never includes this module, is linked by any Free binary; this module is
//! transport-only and Pro/TuxCommander-only. The hex-string design is kept
//! anyway because it's the strictly safer shape and costs nothing.)
//!
//! # Behavior
//!
//! - Pin absent/unset (`None`) → the client uses ordinary WebPKI TLS
//!   verification (reqwest's default `rustls` backend). Dev/test/first-run
//!   against a server with a normal CA-issued or self-signed-but-unpinned
//!   cert keeps working exactly as before this change.
//! - Pin present → the client REJECTS any server whose leaf certificate's
//!   SPKI SHA-256 does not equal the pinned hash, regardless of chain/CA
//!   validity (this is intentional — pinning exists precisely to survive a
//!   compromised or coerced CA). Hostname/expiry checks are NOT performed by
//!   the pinned verifier; the pin itself is the trust anchor.
//!
//! # Configuration
//!
//! `FleetConfig::from_env` reads an optional `{prefix}_CMD_PIN_SPKI` var: one
//! or more comma-separated 64-character lowercase-or-uppercase hex strings
//! (each a SHA-256 of a server leaf certificate's SubjectPublicKeyInfo,
//! DER-encoded). The connection is accepted if the presented cert's SPKI
//! matches ANY pin in the set. Absent, empty, or containing no valid entries
//! → pinning is not enabled (falls back to default TLS verification; a
//! malformed entry is logged and skipped, never a hard failure that would
//! prevent the client from being constructed).
//!
//! # Rotation overlap
//!
//! A single pin turns a routine server cert rotation into a fleet-wide
//! control-channel outage: every already-enrolled device rejects the new
//! cert until it is manually re-pinned. Configuring a SET (current pin +
//! the upcoming "next" pin) lets an operator roll the server cert with an
//! overlap window — old devices keep matching the retiring pin, already-
//! updated devices match the new one — then drop the retired pin from the
//! set once the rotation is complete fleet-wide.

use std::fmt;
use std::sync::Arc;

use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use sha2::{Digest, Sha256};

/// A 32-byte SHA-256 hash of a server leaf certificate's DER-encoded SPKI.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SpkiPin(pub [u8; 32]);

impl fmt::Debug for SpkiPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SpkiPin({})", hex::encode(self.0))
    }
}

impl SpkiPin {
    /// Parse a 64-char hex string into a pin. Returns `None` on any malformed
    /// input (wrong length, non-hex chars) — callers treat this the same as
    /// "no pin configured" rather than a hard error.
    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s.trim()).ok()?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(Self(arr))
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

/// A non-empty set of accepted SPKI pins — a primary plus any number of
/// retired/next pins kept during a rotation-overlap window. A connection is
/// accepted if the presented certificate's SPKI SHA-256 matches ANY member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpkiPinSet(Vec<SpkiPin>);

impl SpkiPinSet {
    /// Build a set from at least one pin. Returns `None` if `pins` is empty —
    /// callers treat that the same as "no pin configured".
    pub fn new(pins: Vec<SpkiPin>) -> Option<Self> {
        if pins.is_empty() {
            None
        } else {
            Some(Self(pins))
        }
    }

    /// Parse a comma-separated list of 64-char hex SPKI hashes. Malformed or
    /// blank entries are skipped (logged by the caller); returns `None` if no
    /// entry parses — same "absent" treatment as a single malformed pin.
    pub fn from_hex_list(s: &str) -> Option<Self> {
        let pins: Vec<SpkiPin> = s
            .split(',')
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .filter_map(SpkiPin::from_hex)
            .collect();
        Self::new(pins)
    }

    /// True if `hash` matches any pin in the set (constant-time per-pin compare).
    fn matches(&self, hash: &[u8; 32]) -> bool {
        self.0.iter().any(|pin| hash.ct_eq_bytes(&pin.0))
    }

    /// The pins in this set, in configured order (primary first).
    pub fn pins(&self) -> &[SpkiPin] {
        &self.0
    }
}

/// Compute the SHA-256 of a DER-encoded certificate's SubjectPublicKeyInfo.
fn spki_sha256(cert_der: &CertificateDer<'_>) -> Result<[u8; 32], TlsError> {
    let end_entity = webpki::EndEntityCert::try_from(cert_der)
        .map_err(|_| TlsError::InvalidCertificate(rustls::CertificateError::BadEncoding))?;
    let spki = end_entity.subject_public_key_info();
    let mut hasher = Sha256::new();
    hasher.update(spki.as_ref());
    Ok(hasher.finalize().into())
}

/// A `rustls` server-certificate verifier that accepts a connection if and
/// only if the leaf certificate's SPKI SHA-256 matches ANY pin in the
/// configured set (primary + any retired/next pins kept for a rotation
/// overlap window).
///
/// Deliberately does NOT validate the certificate chain, hostname, or
/// expiry — the pin itself is the trust decision (this is standard
/// certificate/public-key pinning semantics, matching how mobile MDM/cert
/// pinning libraries behave). Accepting ANY pin in the set does not weaken
/// the reject-on-no-match behavior: a cert that matches none of the
/// configured pins is still rejected exactly as a single-pin mismatch was.
#[derive(Debug)]
struct PinnedSpkiVerifier {
    pins: SpkiPinSet,
    supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedSpkiVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let actual = spki_sha256(end_entity)?;
        // Constant-time per-pin compare: the pins are not secret, but there
        // is no reason to leak timing information about a certificate check.
        if self.pins.matches(&actual) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::General(
                "fleet TLS pin mismatch: server SPKI does not match any pinned hash".to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algs.supported_schemes()
    }
}

/// Small local trait so the comparison above reads as constant-time intent
/// without pulling in a `subtle`/`constant_time_eq` dependency for one call.
trait CtEqBytes {
    fn ct_eq_bytes(&self, other: &[u8; 32]) -> bool;
}

impl CtEqBytes for [u8; 32] {
    fn ct_eq_bytes(&self, other: &[u8; 32]) -> bool {
        let mut diff: u8 = 0;
        for i in 0..32 {
            diff |= self[i] ^ other[i];
        }
        diff == 0
    }
}

/// Build a `reqwest::Client` honoring an optional SET of accepted SPKI pins.
///
/// - `pins`: `Some` enables pinning (rejects any server whose SPKI SHA-256
///   matches none of the pins in the set); `None` builds a normal client
///   with default TLS verification (today's behavior, unchanged). A set
///   with more than one pin is how an operator runs a cert-rotation overlap
///   window (primary + retired/next) without a fleet-wide outage.
///
/// Returns `Err` only on an internal rustls config-build failure (should not
/// happen with the fixed protocol-version defaults used here); callers that
/// want a hard guarantee of "never fails to construct" can fall back to
/// `reqwest::Client::new()` on `Err`, matching pre-pinning behavior.
pub fn build_client(pins: Option<SpkiPinSet>) -> Result<reqwest::Client, String> {
    let Some(pins) = pins else {
        return Ok(reqwest::Client::new());
    };

    let provider = Arc::new(aws_lc_rs::default_provider());
    let supported_algs = rustls::crypto::WebPkiSupportedAlgorithms {
        all: provider.signature_verification_algorithms.all,
        mapping: provider.signature_verification_algorithms.mapping,
    };
    let verifier = Arc::new(PinnedSpkiVerifier {
        pins,
        supported_algs,
    });

    let tls_config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("fleet TLS pin: protocol version setup failed: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .build()
        .map_err(|e| format!("fleet TLS pin: client build failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{generate_simple_self_signed, CertifiedKey};

    /// Generate a self-signed cert for `name` and return (DER cert, SPKI SHA-256 hash).
    fn self_signed(name: &str) -> (CertificateDer<'static>, [u8; 32]) {
        let CertifiedKey { cert, .. } =
            generate_simple_self_signed(vec![name.to_string()]).expect("generate cert");
        let der = CertificateDer::from(cert.der().to_vec());
        let hash = spki_sha256(&der).expect("spki hash");
        (der, hash)
    }

    fn verifier_for(pin: [u8; 32]) -> PinnedSpkiVerifier {
        verifier_for_set(vec![pin])
    }

    fn verifier_for_set(pins: Vec<[u8; 32]>) -> PinnedSpkiVerifier {
        let provider = aws_lc_rs::default_provider();
        let supported_algs = rustls::crypto::WebPkiSupportedAlgorithms {
            all: provider.signature_verification_algorithms.all,
            mapping: provider.signature_verification_algorithms.mapping,
        };
        PinnedSpkiVerifier {
            pins: SpkiPinSet::new(pins.into_iter().map(SpkiPin).collect()).expect("non-empty"),
            supported_algs,
        }
    }

    fn server_name() -> ServerName<'static> {
        ServerName::try_from("fleet.internal.example").unwrap()
    }

    #[test]
    fn accepts_cert_matching_the_pinned_spki() {
        let (der, hash) = self_signed("fleet.internal.example");
        let verifier = verifier_for(hash);
        let result = verifier.verify_server_cert(&der, &[], &server_name(), &[], UnixTime::now());
        assert!(
            result.is_ok(),
            "matching SPKI pin must be accepted: {result:?}"
        );
    }

    #[test]
    fn rejects_cert_with_different_spki_than_pinned() {
        let (der_x, _hash_x) = self_signed("attacker.example");
        let (_der_y, hash_y) = self_signed("fleet.internal.example");
        // Pin Y's hash, present X's cert — must be rejected (MITM simulation:
        // a different keypair/cert presented where Y was expected).
        assert_ne!(
            spki_sha256(&der_x).unwrap(),
            hash_y,
            "test fixture sanity: the two generated certs must have different SPKI"
        );
        let verifier = verifier_for(hash_y);
        let result = verifier.verify_server_cert(&der_x, &[], &server_name(), &[], UnixTime::now());
        assert!(
            result.is_err(),
            "SPKI mismatch (simulated MITM) must be rejected"
        );
    }

    // ── Rotation-overlap: a set of pins (primary + retired/next) ──────────

    #[test]
    fn accepts_cert_matching_a_retired_pin_in_the_set() {
        // Simulate a cert-rotation overlap window: the set holds the new
        // ("primary") pin plus the still-valid retiring ("old") pin. A
        // server presenting the OLD cert (not yet rotated on this device,
        // or a device that hasn't picked up the new cert yet) must still be
        // accepted — this is the whole point of the overlap window.
        let (der_old, hash_old) = self_signed("fleet.internal.example");
        let (_der_new, hash_new) = self_signed("fleet.internal.example");
        assert_ne!(hash_old, hash_new, "fixture sanity: distinct cert keys");

        let verifier = verifier_for_set(vec![hash_new, hash_old]);
        let result =
            verifier.verify_server_cert(&der_old, &[], &server_name(), &[], UnixTime::now());
        assert!(
            result.is_ok(),
            "a cert matching the RETIRED pin in a multi-pin set must be accepted: {result:?}"
        );
    }

    #[test]
    fn accepts_cert_matching_the_new_pin_in_the_set() {
        // Same overlap window, but the server has already rotated to the
        // new cert — a device holding {new, old} must also accept it.
        let (_der_old, hash_old) = self_signed("fleet.internal.example");
        let (der_new, hash_new) = self_signed("fleet.internal.example");
        assert_ne!(hash_old, hash_new, "fixture sanity: distinct cert keys");

        let verifier = verifier_for_set(vec![hash_new, hash_old]);
        let result =
            verifier.verify_server_cert(&der_new, &[], &server_name(), &[], UnixTime::now());
        assert!(
            result.is_ok(),
            "a cert matching the NEW pin in a multi-pin set must be accepted: {result:?}"
        );
    }

    #[test]
    fn rejects_cert_matching_no_pin_in_the_set() {
        // A cert that matches NEITHER the primary NOR the retired pin (e.g.
        // an attacker/MITM cert) must still be rejected — a pin set must
        // never widen acceptance beyond its configured members.
        let (der_attacker, hash_attacker) = self_signed("attacker.example");
        let (_der_new, hash_new) = self_signed("fleet.internal.example");
        let (_der_old, hash_old) = self_signed("fleet.internal.example");
        assert_ne!(hash_attacker, hash_new);
        assert_ne!(hash_attacker, hash_old);

        let verifier = verifier_for_set(vec![hash_new, hash_old]);
        let result =
            verifier.verify_server_cert(&der_attacker, &[], &server_name(), &[], UnixTime::now());
        assert!(
            result.is_err(),
            "a cert matching no pin in the set must be rejected: {result:?}"
        );
    }

    #[test]
    fn spki_pin_set_from_hex_list_parses_multiple_comma_separated_pins() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let set = SpkiPinSet::from_hex_list(&format!("{a}, {b}")).expect("must parse");
        assert_eq!(set.pins().len(), 2);
        assert_eq!(set.pins()[0].to_hex(), a);
        assert_eq!(set.pins()[1].to_hex(), b);
    }

    #[test]
    fn spki_pin_set_from_hex_list_skips_malformed_entries_but_keeps_valid_ones() {
        let a = "a".repeat(64);
        let set = SpkiPinSet::from_hex_list(&format!("{a},not-hex,")).expect("must parse");
        assert_eq!(set.pins().len(), 1);
        assert_eq!(set.pins()[0].to_hex(), a);
    }

    #[test]
    fn spki_pin_set_from_hex_list_none_when_all_entries_malformed() {
        assert!(SpkiPinSet::from_hex_list("not-hex,also-bad").is_none());
        assert!(SpkiPinSet::from_hex_list("").is_none());
        assert!(SpkiPinSet::from_hex_list(",,").is_none());
    }

    #[test]
    fn spki_pin_set_new_rejects_empty_vec() {
        assert!(SpkiPinSet::new(vec![]).is_none());
    }

    #[test]
    fn spki_pin_hex_roundtrips() {
        let pin = SpkiPin([0x11u8; 32]);
        let hex_str = pin.to_hex();
        assert_eq!(hex_str.len(), 64);
        let parsed = SpkiPin::from_hex(&hex_str).expect("parse back");
        assert_eq!(parsed, pin);
    }

    #[test]
    fn spki_pin_from_hex_rejects_malformed_input() {
        assert!(SpkiPin::from_hex("not-hex").is_none());
        assert!(SpkiPin::from_hex("deadbeef").is_none()); // too short
        assert!(SpkiPin::from_hex("").is_none());
    }

    #[test]
    fn build_client_without_pin_succeeds() {
        // None → falls back to reqwest::Client::new(); must not error.
        let client = build_client(None);
        assert!(client.is_ok());
    }

    #[test]
    fn build_client_with_pin_succeeds() {
        let (_der, hash) = self_signed("fleet.internal.example");
        let client = build_client(SpkiPinSet::new(vec![SpkiPin(hash)]));
        assert!(client.is_ok(), "pinned client must build: {client:?}");
    }

    #[test]
    fn build_client_with_multi_pin_set_succeeds() {
        let (_der_new, hash_new) = self_signed("fleet.internal.example");
        let (_der_old, hash_old) = self_signed("fleet.internal.example");
        let client = build_client(SpkiPinSet::new(vec![SpkiPin(hash_new), SpkiPin(hash_old)]));
        assert!(
            client.is_ok(),
            "client with a rotation-overlap pin set must build: {client:?}"
        );
    }
}
