// SPDX-License-Identifier: AGPL-3.0-or-later
//! Golden-vector EXPORT for cross-language conformance harnesses.
//!
//! `cargo run -p fleet-proto --example dump_vectors` prints one stable JSON
//! object to stdout containing the hex-encoded bytes of every canonical
//! signing preimage this crate defines, computed from the SAME fixed inputs
//! already pinned by the `#[cfg(test)] mod tests` golden-vector tests in
//! `src/lib.rs` (`golden_epoch_preimage`, `golden_canonical_command_bytes`,
//! `golden_policy_preimage`), plus a
//! check-in HMAC sample using the identical construction already exercised by
//! `SecureSyncPlainJvmSelfTest.testCheckinHmacVector` in secureOS
//! (`apps/secure-sync/test/plain-jvm/`).
//!
//! Consumers: the secureOS framework conformance harness
//! (`build/patches/sync/test/`) treats this fixture as the vector source of
//! truth for cross-checking the independent Java (`CanonicalJson.java`,
//! `Ed25519Verifier.java`, `SignedCommandVerifier.java`) and Kotlin
//! (`CheckinHmac`) reimplementations against this Rust SSOT. A mismatch
//! between a regenerated fixture and what those harnesses expect is a
//! wire-breaking change — see `build/patches/sync/CONTRACT.md` and
//! `docs/fleet-server-wire-contract.md`.
//!
//! This file intentionally does NOT change `src/lib.rs` or `Cargo.toml` —
//! it is a plain `examples/` binary, auto-discovered by cargo, using only
//! `fleet-proto`'s existing public API plus a small dependency-free
//! HMAC-SHA256 implementation (hand-rolled below, mirroring the same
//! "deliberately dependency-free" posture `CanonicalJson.java` documents on
//! the secureOS side — no `hmac`/`sha2` crate is a *direct* dependency of
//! `fleet-proto`, so this file cannot `use` them even though `ed25519-dalek`
//! pulls `sha2` in transitively; Cargo does not expose transitive deps to
//! `examples/` unless they are also direct dependencies).

use fleet_proto::{
    canonical_command_bytes, epoch_preimage, policy_preimage, ActionClass, EpochSigningInput,
    PolicyIntent,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() {
    // ── Epoch preimage — SAME inputs as `golden_epoch_preimage` ───────────
    // version=7, config={"telemetry":false}, locked_paths=["b","a"]
    // (out-of-order; epoch_preimage sorts them), managed=true,
    // target_kind="device", target_id=Some("dev-1").
    let epoch_cfg = serde_json::json!({ "telemetry": false });
    let epoch_input = EpochSigningInput {
        version: 7,
        config: &epoch_cfg,
        locked_paths: &["b".to_string(), "a".to_string()],
        managed: true,
        target_kind: "device",
        target_id: Some("dev-1"),
    };
    let epoch_bytes = epoch_preimage(&epoch_input);

    // ── Command preimage — SAME inputs as `golden_canonical_command_bytes` ─
    // command_id="cmd-abc", device_id="dev-xyz", catalog_id="status.read",
    // action_class=Safe, payload={}, epoch_version=3.
    let command_payload = serde_json::json!({});
    let command_bytes = canonical_command_bytes(
        "cmd-abc",
        "dev-xyz",
        "status.read",
        ActionClass::Safe.as_wire_str(),
        &command_payload,
        3,
    );

    // ── Policy preimage — SAME inputs as `golden_policy_preimage` ─────────
    // version=5, org_id="org-test", device_id="dev-abc", intents supplied
    // out-of-order (policy_preimage sorts by key: "fleet.enabled" <
    // "privacy.telemetry").
    let policy_intents = vec![
        PolicyIntent {
            key: "privacy.telemetry".to_string(),
            value: serde_json::json!(false),
            mode: "hard-lock".to_string(),
            ttl_secs: None,
        },
        PolicyIntent {
            key: "fleet.enabled".to_string(),
            value: serde_json::json!(true),
            mode: "heal".to_string(),
            ttl_secs: Some(3600),
        },
    ];
    let policy_bytes = policy_preimage(5, "org-test", "dev-abc", &policy_intents);

    // ── Check-in HMAC sample — SAME inputs as secureOS's
    // `SecureSyncPlainJvmSelfTest.testCheckinHmacVector`, so the fixture is
    // directly diffable against that existing Java vector:
    //   device_id="device-123", ts=1_710_000_000, nonce="nonce-abc",
    //   checkin_secret="secureos-checkin-secret-v1" (raw UTF-8 bytes).
    // Preimage construction mirrors tc-agent's `compute_checkin_hmac`
    // (`"{device_id}:{ts}:{nonce}"`, HMAC-SHA256, STANDARD base64 output) —
    // see canonical-spec.md / CONTRACT.md §3 "hmac preimage is UNCHANGED".
    let checkin_device_id = "device-123";
    let checkin_ts: i64 = 1_710_000_000;
    let checkin_nonce = "nonce-abc";
    let checkin_secret = b"secureos-checkin-secret-v1";
    let checkin_preimage = format!("{checkin_device_id}:{checkin_ts}:{checkin_nonce}");
    let checkin_hmac_bytes = hmac_sha256(checkin_secret, checkin_preimage.as_bytes());
    let checkin_hmac_b64 = {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        B64.encode(checkin_hmac_bytes)
    };

    // ── Emit a stable, hand-written JSON object (keys in a fixed order —
    // this is a human/CI-readable fixture, not itself a signing preimage, so
    // it does not need to go through `write_canonical`; still emitted with
    // fixed key order and no extraneous whitespace variance run-to-run so a
    // `git diff` on a regenerated fixture is minimal and reviewable). ─────
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"_comment\": \"AUTO-GENERATED by fleet-proto/examples/dump_vectors.rs — do not hand-edit. Vector source of truth: fleet-proto src/lib.rs #[cfg(test)] golden vectors.\",\n");
    out.push_str("  \"epoch_preimage\": {\n");
    out.push_str("    \"inputs\": {\"version\": 7, \"config\": {\"telemetry\": false}, \"locked_paths\": [\"b\", \"a\"], \"managed\": true, \"target_kind\": \"device\", \"target_id\": \"dev-1\"},\n");
    out.push_str(&format!(
        "    \"hex\": {}\n",
        json_escape(&hex(&epoch_bytes))
    ));
    out.push_str("  },\n");
    out.push_str("  \"canonical_command_bytes\": {\n");
    out.push_str("    \"inputs\": {\"command_id\": \"cmd-abc\", \"device_id\": \"dev-xyz\", \"catalog_id\": \"status.read\", \"action_class\": \"safe\", \"payload\": {}, \"epoch_version\": 3},\n");
    out.push_str(&format!(
        "    \"hex\": {}\n",
        json_escape(&hex(&command_bytes))
    ));
    out.push_str("  },\n");
    out.push_str("  \"policy_preimage\": {\n");
    out.push_str("    \"inputs\": {\"version\": 5, \"org_id\": \"org-test\", \"device_id\": \"dev-abc\", \"intents\": [{\"key\": \"privacy.telemetry\", \"value\": false, \"mode\": \"hard-lock\", \"ttl_secs\": null}, {\"key\": \"fleet.enabled\", \"value\": true, \"mode\": \"heal\", \"ttl_secs\": 3600}]},\n");
    out.push_str(&format!(
        "    \"hex\": {}\n",
        json_escape(&hex(&policy_bytes))
    ));
    out.push_str("  },\n");
    out.push_str("  \"checkin_hmac\": {\n");
    out.push_str("    \"inputs\": {\"device_id\": \"device-123\", \"ts\": 1710000000, \"nonce\": \"nonce-abc\", \"checkin_secret_utf8\": \"secureos-checkin-secret-v1\"},\n");
    out.push_str("    \"preimage\": \"device-123:1710000000:nonce-abc\",\n");
    out.push_str(&format!(
        "    \"hex\": {},\n",
        json_escape(&hex(&checkin_hmac_bytes))
    ));
    out.push_str(&format!(
        "    \"base64_standard\": {}\n",
        json_escape(&checkin_hmac_b64)
    ));
    out.push_str("  }\n");
    out.push_str("}\n");

    print!("{out}");
}

// ── Dependency-free SHA-256 + HMAC-SHA256 ───────────────────────────────
//
// `fleet-proto`'s direct dependency list is deliberately minimal (see
// Cargo.toml: serde/serde_json/base64/ed25519-dalek only) and this file must
// not add a dependency (`sha2`/`hmac` are NOT direct deps — see the module
// doc comment above), so this is a small, standard, textbook SHA-256/HMAC
// implementation used ONLY by this example binary to compute the check-in
// HMAC sample. It is not part of the crate's public API and never ships in
// any production binary — it exists solely so this fixture-generator can
// reproduce `tc-agent::compute_checkin_hmac`'s output without a new crate
// dependency. Verified against the FIPS 180-4 SHA-256 test vector for the
// empty string and against the existing Java HMAC vector in
// `SecureSyncPlainJvmSelfTest.testCheckinHmacVector` (see VERIFY section of
// the accompanying report — both match).

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut block_key = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = sha256(key);
        block_key[..32].copy_from_slice(&hashed);
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= block_key[i];
        opad[i] ^= block_key[i];
    }

    let mut inner_input = ipad.to_vec();
    inner_input.extend_from_slice(message);
    let inner_hash = sha256(&inner_input);

    let mut outer_input = opad.to_vec();
    outer_input.extend_from_slice(&inner_hash);
    sha256(&outer_input)
}

#[cfg(test)]
mod self_check {
    use super::*;

    #[test]
    fn sha256_matches_fips_180_4_empty_string_vector() {
        // FIPS 180-4 / NIST published test vector for SHA-256("").
        let digest = sha256(b"");
        assert_eq!(
            hex(&digest),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_matches_fips_test_vector_abc() {
        // FIPS 180-4 test vector for SHA-256("abc").
        let digest = sha256(b"abc");
        assert_eq!(
            hex(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_test_case_1() {
        // RFC 4231 Test Case 1: key = 0x0b * 20, data = "Hi There".
        let key = [0x0bu8; 20];
        let mac = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            hex(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn checkin_hmac_matches_the_java_plain_jvm_vector() {
        // Must match SecureSyncPlainJvmSelfTest.testCheckinHmacVector exactly:
        // base64 "n/SoxbZ/S5L7ZAPUu3Jic8FxLcDafUU4vwqeGppDcKU=".
        let mac = hmac_sha256(
            b"secureos-checkin-secret-v1",
            b"device-123:1710000000:nonce-abc",
        );
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        assert_eq!(
            B64.encode(mac),
            "n/SoxbZ/S5L7ZAPUu3Jic8FxLcDafUU4vwqeGppDcKU="
        );
    }
}
