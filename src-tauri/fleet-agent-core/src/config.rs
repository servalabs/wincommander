//! Fleet client configuration, constructed generically from environment variables.
//!
//! Each platform passes its own env-var prefix (e.g. `"TC_FLEET"` for TuxCommander)
//! so the five underlying vars become `{prefix}_URL`, `{prefix}_ENROLL_TOKEN`,
//! `{prefix}_TOKEN`, `{prefix}_CHECKIN_SECRET`, `{prefix}_CMD_PUBKEY`, plus the two
//! optional interval/skew vars `{prefix}_CHECKIN_INTERVAL_SECS` /
//! `{prefix}_CMD_SKEW_SECS`.
//!
//! # Traffic shaping (LIGHT tier — NOT the deferred obfs4/AmneziaWG tunnel tier)
//!
//! Owner-authorized, config-gated, opt-in-by-default-off-or-conservative knobs
//! that stop the check-in channel's cadence/size from being a fixed fingerprint
//! a network observer could use to distinguish an idle poll from a command
//! delivery, or pin the exact time of an event:
//!
//! - `{prefix}_CHECKIN_JITTER_MIN_FRAC` / `{prefix}_CHECKIN_JITTER_MAX_FRAC`:
//!   each cycle's actual sleep is drawn uniformly from
//!   `[checkin_interval_secs * min_frac, checkin_interval_secs * max_frac]` —
//!   a genuinely randomized interval, not a fixed beacon with a small ±10%
//!   wobble. Defaults `0.5` / `1.5`, bounded so responsiveness is preserved.
//! - `{prefix}_CHECKIN_PADDING_BYTES`: pads the check-in request body (and,
//!   server-side, the response) with an opaque field so the two are a
//!   constant size bucket on the wire — an idle poll and a poll carrying a
//!   command/ack look the same size. `0` disables padding. Default `512`.
//!   **This padding is carried OUTSIDE the HMAC preimage** — see
//!   `CheckinRequest::padding` in `verify.rs`; it can never affect
//!   `compute_checkin_hmac` or `fleet_proto::canonical_command_bytes`.
//! - `{prefix}_CHECKIN_DECOY_ENABLED` / `{prefix}_CHECKIN_DECOY_RATE`: optional
//!   low-rate decoy (cover-traffic) check-ins, indistinguishable on the wire
//!   from a real one (same auth, same padding bucket), sent at randomized
//!   times so real events don't stand out against the background rate.
//!   Default: disabled (`false` / rate `0.0`).

use ed25519_dalek::VerifyingKey;

use crate::verify::decode_verifying_key;

/// Configuration for the fleet check-in client, constructed from env vars.
///
/// Constructed only when ALL five required vars (under `prefix`) are set.
#[derive(Clone)]
pub struct FleetConfig {
    /// Fleet server base URL (e.g. `https://fleet.example.com`). Trailing slash trimmed.
    pub url: String,
    /// Enroll token (used in the enroll request's Authorization header).
    pub enroll_token: String,
    /// Device token (used in the check-in request's Authorization header).
    pub fleet_token: String,
    /// Per-device check-in HMAC secret, taken as the RAW UTF-8 bytes of the env
    /// var value (matching the server's verbatim-bytes storage — NOT hashed,
    /// NOT hex/base64-decoded).
    pub checkin_secret: Vec<u8>,
    /// Pinned command public key (base64 ed25519). Verified at enroll; refused if changed.
    pub cmd_pubkey_b64: String,
    /// Parsed verifying key (derived from `cmd_pubkey_b64`).
    pub cmd_pubkey: VerifyingKey,
    /// Check-in interval in seconds (default: 60). This is the CENTER of the
    /// randomized window — see `checkin_jitter_min_frac`/`checkin_jitter_max_frac`;
    /// the actual per-cycle sleep is drawn from
    /// `[checkin_interval_secs * min_frac, checkin_interval_secs * max_frac]`,
    /// not a fixed value with small wobble.
    pub checkin_interval_secs: u64,
    /// Lower bound fraction of `checkin_interval_secs` for the randomized
    /// per-cycle interval window (traffic-shaping; default `0.5`).
    pub checkin_jitter_min_frac: f64,
    /// Upper bound fraction of `checkin_interval_secs` for the randomized
    /// per-cycle interval window (traffic-shaping; default `1.5`).
    pub checkin_jitter_max_frac: f64,
    /// Target size (bytes) of the opaque padding field added to check-in
    /// requests/responses so an idle poll and a poll carrying a command/ack
    /// are the same size on the wire. `0` disables padding. Default `512`.
    /// Carried OUTSIDE any signed/HMAC'd preimage (see `verify.rs`).
    pub checkin_padding_bytes: usize,
    /// Enable low-rate decoy (cover-traffic) check-ins, indistinguishable on
    /// the wire from a real check-in. Default `false` (off).
    pub checkin_decoy_enabled: bool,
    /// Expected decoys per real check-in interval (e.g. `0.2` = roughly one
    /// decoy every 5 real cycles, at a randomized offset). Only consulted when
    /// `checkin_decoy_enabled` is true. Default `0.2`.
    pub checkin_decoy_rate: f64,
    /// Maximum acceptable clock skew on fleet commands (default: 300 s / 5 min).
    pub max_cmd_skew_secs: i64,
    /// Optional TLS server-certificate pin SET: one or more SHA-256 hashes of
    /// accepted fleet-server leaf certificate SubjectPublicKeyInfo values, as
    /// a comma-separated list of 64-char hex strings, read from
    /// `{prefix}_CMD_PIN_SPKI`. A connection is accepted if the server's SPKI
    /// matches ANY pin in the set — configuring a primary + retired/next pin
    /// lets an operator roll the server cert with an overlap window instead
    /// of a fleet-wide outage. `None` (unset or no entry parses) means
    /// default TLS verification — pinning is opt-in hardening, never
    /// required. See `crate::pinning` for how this is consumed.
    #[cfg(feature = "transport")]
    pub cmd_pin_spki: Option<crate::pinning::SpkiPinSet>,
}

impl FleetConfig {
    /// Try to build from environment variables under `{prefix}_*`.
    ///
    /// Returns `None` when any required var is absent (sim-default / not configured).
    pub fn from_env(prefix: &str) -> Option<Self> {
        let var = |suffix: &str| std::env::var(format!("{prefix}_{suffix}"));

        let url = var("URL").ok().filter(|s| !s.is_empty())?;
        let enroll_token = var("ENROLL_TOKEN").ok().filter(|s| !s.is_empty())?;
        let fleet_token = var("TOKEN").ok().filter(|s| !s.is_empty())?;
        // {prefix}_CHECKIN_SECRET: raw bytes for the check-in HMAC.
        // Accepted as a UTF-8 string (the server stores it verbatim in the
        // `checkin_secret` bytea column). We take the raw UTF-8 bytes of the
        // env-var value, matching the server's treatment — do NOT hex/base64-decode.
        let checkin_secret_str = var("CHECKIN_SECRET").ok().filter(|s| !s.is_empty())?;
        let checkin_secret = checkin_secret_str.into_bytes();
        let cmd_pubkey_b64 = var("CMD_PUBKEY").ok().filter(|s| !s.is_empty())?;

        let cmd_pubkey = match decode_verifying_key(&cmd_pubkey_b64) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("{prefix}_CMD_PUBKEY invalid — fleet client not constructed: {e}");
                return None;
            }
        };

        let checkin_interval_secs = var("CHECKIN_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60u64);

        // Randomized check-in window fractions. A malformed or out-of-order
        // pair (min <= 0, or min > max) falls back to the safe default rather
        // than blocking construction or collapsing to a fixed interval.
        const DEFAULT_JITTER_MIN_FRAC: f64 = 0.5;
        const DEFAULT_JITTER_MAX_FRAC: f64 = 1.5;
        let min_frac_raw = var("CHECKIN_JITTER_MIN_FRAC")
            .ok()
            .and_then(|s| s.parse::<f64>().ok());
        let max_frac_raw = var("CHECKIN_JITTER_MAX_FRAC")
            .ok()
            .and_then(|s| s.parse::<f64>().ok());
        let (checkin_jitter_min_frac, checkin_jitter_max_frac) = match (min_frac_raw, max_frac_raw)
        {
            (Some(min), Some(max)) if min > 0.0 && min <= max => (min, max),
            (Some(min), None) if min > 0.0 && min <= DEFAULT_JITTER_MAX_FRAC => {
                (min, DEFAULT_JITTER_MAX_FRAC)
            }
            (None, Some(max)) if max >= DEFAULT_JITTER_MIN_FRAC => (DEFAULT_JITTER_MIN_FRAC, max),
            (None, None) => (DEFAULT_JITTER_MIN_FRAC, DEFAULT_JITTER_MAX_FRAC),
            _ => {
                tracing::warn!(
                    "{prefix}_CHECKIN_JITTER_MIN_FRAC/{prefix}_CHECKIN_JITTER_MAX_FRAC invalid \
                     or out of order — falling back to defaults ({DEFAULT_JITTER_MIN_FRAC}, \
                     {DEFAULT_JITTER_MAX_FRAC})"
                );
                (DEFAULT_JITTER_MIN_FRAC, DEFAULT_JITTER_MAX_FRAC)
            }
        };

        let checkin_padding_bytes = var("CHECKIN_PADDING_BYTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(512usize);

        let checkin_decoy_enabled = var("CHECKIN_DECOY_ENABLED")
            .ok()
            .map(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        let checkin_decoy_rate = var("CHECKIN_DECOY_RATE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|r| r.is_finite() && *r >= 0.0)
            .unwrap_or(0.2);

        let max_cmd_skew_secs = var("CMD_SKEW_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(300i64); // spec: 5 minutes

        // Optional TLS pin set: absent or all-malformed hex is NOT a
        // construction failure — it just means pinning stays off (default
        // TLS verification). Multiple comma-separated pins are how an
        // operator configures a cert-rotation overlap window.
        #[cfg(feature = "transport")]
        let cmd_pin_spki = var("CMD_PIN_SPKI")
            .ok()
            .and_then(|s| crate::pinning::SpkiPinSet::from_hex_list(&s));
        #[cfg(feature = "transport")]
        if var("CMD_PIN_SPKI").is_ok() && cmd_pin_spki.is_none() {
            tracing::warn!(
                "{prefix}_CMD_PIN_SPKI is set but contains no valid 64-char SHA-256 hex \
                 entries — TLS pinning disabled, falling back to default verification"
            );
        }

        Some(Self {
            url: url.trim_end_matches('/').to_string(),
            enroll_token,
            fleet_token,
            checkin_secret,
            cmd_pubkey_b64,
            cmd_pubkey,
            checkin_interval_secs,
            checkin_jitter_min_frac,
            checkin_jitter_max_frac,
            checkin_padding_bytes,
            checkin_decoy_enabled,
            checkin_decoy_rate,
            max_cmd_skew_secs,
            #[cfg(feature = "transport")]
            cmd_pin_spki,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use ed25519_dalek::SigningKey;

    fn set_all(prefix: &str, pubkey_b64: &str) {
        std::env::set_var(format!("{prefix}_URL"), "https://fleet.example.com");
        std::env::set_var(format!("{prefix}_ENROLL_TOKEN"), "enroll-tok");
        std::env::set_var(format!("{prefix}_TOKEN"), "fleet-tok");
        std::env::set_var(
            format!("{prefix}_CHECKIN_SECRET"),
            "a-32-byte-secret-for-this-test!!",
        );
        std::env::set_var(format!("{prefix}_CMD_PUBKEY"), pubkey_b64);
    }

    fn clear_all(prefix: &str) {
        for suffix in [
            "URL",
            "ENROLL_TOKEN",
            "TOKEN",
            "CHECKIN_SECRET",
            "CMD_PUBKEY",
            "CHECKIN_INTERVAL_SECS",
            "CHECKIN_JITTER_MIN_FRAC",
            "CHECKIN_JITTER_MAX_FRAC",
            "CHECKIN_PADDING_BYTES",
            "CHECKIN_DECOY_ENABLED",
            "CHECKIN_DECOY_RATE",
            "CMD_SKEW_SECS",
            "CMD_PIN_SPKI",
        ] {
            std::env::remove_var(format!("{prefix}_{suffix}"));
        }
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_from_env_returns_none_when_vars_absent() {
        clear_all("TAC_TEST_ABSENT");
        assert!(FleetConfig::from_env("TAC_TEST_ABSENT").is_none());
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_default_skew_is_300s() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SKEW";

        set_all(prefix, &pubkey_b64);
        std::env::remove_var(format!("{prefix}_CMD_SKEW_SECS"));

        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert_eq!(
            cfg.max_cmd_skew_secs, 300,
            "default skew must be 300s (5 min per spec)"
        );
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_trims_trailing_slash_from_url() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SLASH";

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_URL"), "https://fleet.example.com/");

        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert_eq!(cfg.url, "https://fleet.example.com");
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_checkin_secret_is_raw_utf8_bytes() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SECRETBYTES";

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CHECKIN_SECRET"), "plain-text-secret");

        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert_eq!(cfg.checkin_secret, b"plain-text-secret".to_vec());
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_pin_spki_absent_by_default() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_PIN_ABSENT";

        set_all(prefix, &pubkey_b64);
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert!(
            cfg.cmd_pin_spki.is_none(),
            "no CMD_PIN_SPKI set — pinning must default to off"
        );
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_pin_spki_parsed_when_set() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_PIN_SET";
        let pin_hex = "a".repeat(64); // valid-shape 32-byte hex

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CMD_PIN_SPKI"), &pin_hex);
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        let pins = cfg
            .cmd_pin_spki
            .expect("a valid CMD_PIN_SPKI hex value must be parsed");
        assert_eq!(pins.pins().len(), 1);
        assert_eq!(pins.pins()[0].to_hex(), pin_hex);
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_pin_spki_parses_rotation_overlap_pin_set() {
        // Two comma-separated pins: the primary + a retired/next pin kept
        // during a cert-rotation overlap window.
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_PIN_ROTATION";
        let pin_a = "a".repeat(64);
        let pin_b = "b".repeat(64);

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CMD_PIN_SPKI"), format!("{pin_a},{pin_b}"));
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        let pins = cfg
            .cmd_pin_spki
            .expect("a comma-separated pin list must be parsed");
        assert_eq!(
            pins.pins().len(),
            2,
            "both pins in the rotation set must be kept"
        );
        assert_eq!(pins.pins()[0].to_hex(), pin_a);
        assert_eq!(pins.pins()[1].to_hex(), pin_b);
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_pin_spki_malformed_falls_back_to_none() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_PIN_BAD";

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CMD_PIN_SPKI"), "not-a-valid-hash");
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        // Construction must still succeed — a malformed pin never blocks the
        // client from being built; it just leaves pinning off.
        let cfg = cfg.expect("config must still be constructed with a malformed pin");
        assert!(cfg.cmd_pin_spki.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn fleet_config_different_prefixes_do_not_collide() {
        let key_a = SigningKey::generate(&mut rand::rngs::OsRng);
        let key_b = SigningKey::generate(&mut rand::rngs::OsRng);
        set_all("TAC_TEST_A", &B64.encode(key_a.verifying_key().to_bytes()));
        // Deliberately leave prefix B unset.
        clear_all("TAC_TEST_B");

        assert!(FleetConfig::from_env("TAC_TEST_A").is_some());
        assert!(FleetConfig::from_env("TAC_TEST_B").is_none());

        clear_all("TAC_TEST_A");
        let _ = key_b;
    }

    // ── Traffic-shaping config: defaults + overrides + fallback ─────────────

    #[test]
    #[serial_test::serial]
    fn traffic_shaping_defaults_are_sane_when_unset() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SHAPE_DEFAULTS";

        set_all(prefix, &pubkey_b64);
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert_eq!(cfg.checkin_jitter_min_frac, 0.5);
        assert_eq!(cfg.checkin_jitter_max_frac, 1.5);
        assert_eq!(cfg.checkin_padding_bytes, 512);
        assert!(!cfg.checkin_decoy_enabled, "decoys must default to off");
        assert_eq!(cfg.checkin_decoy_rate, 0.2);
    }

    #[test]
    #[serial_test::serial]
    fn traffic_shaping_jitter_window_is_configurable() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SHAPE_WINDOW";

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CHECKIN_JITTER_MIN_FRAC"), "0.75");
        std::env::set_var(format!("{prefix}_CHECKIN_JITTER_MAX_FRAC"), "1.25");
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert_eq!(cfg.checkin_jitter_min_frac, 0.75);
        assert_eq!(cfg.checkin_jitter_max_frac, 1.25);
    }

    #[test]
    #[serial_test::serial]
    fn traffic_shaping_jitter_window_falls_back_when_inverted() {
        // min > max is nonsensical — must fall back to the safe default
        // window rather than construct a backwards (or panicking) range.
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SHAPE_INVERTED";

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CHECKIN_JITTER_MIN_FRAC"), "2.0");
        std::env::set_var(format!("{prefix}_CHECKIN_JITTER_MAX_FRAC"), "1.0");
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must still be constructed with an inverted window");
        assert_eq!(cfg.checkin_jitter_min_frac, 0.5);
        assert_eq!(cfg.checkin_jitter_max_frac, 1.5);
    }

    #[test]
    #[serial_test::serial]
    fn traffic_shaping_padding_bytes_configurable_and_zero_disables() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SHAPE_PAD";

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CHECKIN_PADDING_BYTES"), "0");
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert_eq!(cfg.checkin_padding_bytes, 0, "0 must disable padding");
    }

    #[test]
    #[serial_test::serial]
    fn traffic_shaping_decoy_enabled_parses_truthy_strings() {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let pubkey_b64 = B64.encode(key.verifying_key().to_bytes());
        let prefix = "TAC_TEST_SHAPE_DECOY";

        set_all(prefix, &pubkey_b64);
        std::env::set_var(format!("{prefix}_CHECKIN_DECOY_ENABLED"), "true");
        std::env::set_var(format!("{prefix}_CHECKIN_DECOY_RATE"), "0.5");
        let cfg = FleetConfig::from_env(prefix);
        clear_all(prefix);

        let cfg = cfg.expect("config must be constructed");
        assert!(cfg.checkin_decoy_enabled);
        assert_eq!(cfg.checkin_decoy_rate, 0.5);
    }
}
