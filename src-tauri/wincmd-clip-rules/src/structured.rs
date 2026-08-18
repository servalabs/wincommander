// SPDX-License-Identifier: AGPL-3.0-or-later
//! Checksum-validated structural matchers — patterns that a regex alone
//! can't verify because the format has an internal check digit.
//!
//! Scope note (recorded for downstream agents): the plan's API sketch
//! names this enum's intent as "Luhn-checked card, mod-97 IBAN, org id
//! formats" — the `...` is illustrative, not a commitment to an
//! unspecified third format. Only the two concretely named checks are
//! implemented: `PaymentCard` (Luhn) and `Iban` (ISO 7064 MOD 97-10). No
//! "org id format" is implemented because none was specified.

use serde::{Deserialize, Serialize};

/// A structurally-validated pattern family — matched by a checksum over
/// candidate digit/alnum runs found in the text, not by a plain regex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum StructuredKind {
    /// A 13–19 digit run (optional space/dash separators) that passes the
    /// Luhn check digit — migrated verbatim from
    /// `commander-free/src/paste_monitor.rs::looks_like_credit_card`
    /// (`Category::PersonalData`, which had no regex in the original
    /// engine).
    PaymentCard,
    /// A 15–34 char alnum run (2 letters + 2 check digits + up to 30
    /// alnum, optional space separators as printed on statements) that
    /// passes the ISO 7064 MOD 97-10 check. New in this crate — the
    /// legacy free-tier engine never had an IBAN detector.
    Iban,
}

pub(crate) fn matches(kind: StructuredKind, text: &str) -> bool {
    match kind {
        StructuredKind::PaymentCard => looks_like_payment_card(text),
        StructuredKind::Iban => looks_like_iban(text),
    }
}

// ── Payment card (Luhn) ──────────────────────────────────────────────
//
// Verbatim port of `paste_monitor.rs::looks_like_credit_card` /
// `paste_monitor.rs::luhn` — kept byte-identical so the free-tier
// migration doesn't change what counts as a credit card number.

fn looks_like_payment_card(text: &str) -> bool {
    let mut candidates: Vec<String> = Vec::with_capacity(8);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && candidates.len() < 32 {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut j = i;
            let mut digit_count = 0usize;
            while j < bytes.len() {
                let b = bytes[j];
                if b.is_ascii_digit() {
                    digit_count += 1;
                    j += 1;
                } else if matches!(b, b' ' | b'-') {
                    j += 1;
                } else {
                    break;
                }
            }
            if (13..=19).contains(&digit_count) {
                let digits: String = text[start..j]
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect();
                candidates.push(digits);
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    candidates.iter().any(|d| luhn(d))
}

fn luhn(digits: &str) -> bool {
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut alt = false;
    for c in digits.chars().rev() {
        let mut n = match c.to_digit(10) {
            Some(n) => n,
            None => return false,
        };
        if alt {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
        alt = !alt;
    }
    sum.is_multiple_of(10)
}

// ── IBAN (ISO 7064 MOD 97-10) ────────────────────────────────────────
//
// Same scan-then-checksum shape as the card matcher above, over alnum
// runs instead of digit runs: find a candidate that's the right length
// and shape (2 letters, 2 digits, then alnum), then verify the check
// digits via mod-97.

fn is_iban_shape(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 15 || bytes.len() > 34 {
        return false;
    }
    bytes[0].is_ascii_alphabetic()
        && bytes[1].is_ascii_alphabetic()
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4..].iter().all(|b| b.is_ascii_alphanumeric())
}

/// `s` must already satisfy `is_iban_shape` (uppercase alnum, no
/// separators). Moves the first 4 chars (country + check digits) to the
/// end, expands each letter to its two-digit position (A=10 .. Z=35), then
/// reduces the resulting decimal string mod 97 digit-by-digit (so it never
/// materializes the full — potentially ~70-digit — number). Valid IBANs
/// reduce to remainder 1.
fn iban_mod97_valid(s: &str) -> bool {
    let rearranged = format!("{}{}", &s[4..], &s[..4]);
    let mut remainder: u32 = 0;
    for c in rearranged.chars() {
        let value = match c.to_digit(10) {
            Some(d) => d,
            None if c.is_ascii_uppercase() => (c as u32) - ('A' as u32) + 10,
            None => return false, // unreachable given is_iban_shape's alphanumeric check
        };
        remainder = if value > 9 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    remainder == 1
}

fn looks_like_iban(text: &str) -> bool {
    let mut candidates: Vec<String> = Vec::with_capacity(8);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && candidates.len() < 32 {
        if bytes[i].is_ascii_alphanumeric() {
            let start = i;
            let mut j = i;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b' ') {
                j += 1;
            }
            let collected: String = text[start..j]
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_uppercase())
                .collect();
            if is_iban_shape(&collected) {
                candidates.push(collected);
            }
            i = j.max(start + 1);
        } else {
            i += 1;
        }
    }
    candidates.iter().any(|c| iban_mod97_valid(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_luhn_card_detected() {
        // Standard Luhn test number.
        assert!(looks_like_payment_card("4532015112830366"));
    }

    #[test]
    fn valid_luhn_card_with_separators_detected() {
        assert!(looks_like_payment_card("card: 4532-0151-1283-0366 exp 12/29"));
    }

    #[test]
    fn invalid_luhn_digits_not_detected() {
        // Same length, last digit changed — fails the check digit.
        assert!(!looks_like_payment_card("4532015112830367"));
    }

    #[test]
    fn short_digit_run_not_detected() {
        assert!(!looks_like_payment_card("12345"));
    }

    #[test]
    fn valid_iban_detected() {
        // ISO 13616 published example IBAN.
        assert!(looks_like_iban("GB82 WEST 1234 5698 7654 32"));
    }

    #[test]
    fn valid_iban_no_spaces_detected() {
        assert!(looks_like_iban("GB82WEST12345698765432"));
    }

    #[test]
    fn invalid_iban_checksum_not_detected() {
        // Last digit flipped — fails mod-97.
        assert!(!looks_like_iban("GB82 WEST 1234 5698 7654 33"));
    }

    #[test]
    fn matches_dispatches_to_the_right_checker() {
        assert!(matches(StructuredKind::PaymentCard, "4532015112830366"));
        assert!(!matches(StructuredKind::Iban, "4532015112830366"));
        assert!(matches(StructuredKind::Iban, "GB82 WEST 1234 5698 7654 32"));
    }
}
