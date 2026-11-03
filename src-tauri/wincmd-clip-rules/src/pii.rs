// SPDX-License-Identifier: AGPL-3.0-or-later
// Recognizer patterns/validators adapted from Microsoft Presidio (MIT); see NOTICE.
use crate::base58_decode;
use crate::iban::iban_length;
use crate::DetectorCategory as Category;
pub(crate) struct PiiDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub pattern: &'static str,
    pub category: Category,
    pub context: &'static [&'static str],
    pub base_score: u8,
}
pub(crate) const PII: &[PiiDefinition] = &[
    PiiDefinition {
        id: "payment_card",
        name: "Payment card (Luhn)",
        pattern: r"\b[0-9](?:[ -]?[0-9]){12,18}\b",
        category: Category::PaymentCard,
        context: &["card", "visa", "mastercard", "credit", "amex"],
        base_score: 60,
    },
    PiiDefinition {
        id: "iban",
        name: "IBAN (mod-97)",
        pattern: r"\b[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]){11,30}\b",
        category: Category::BankAccount,
        context: &["iban", "bank", "account", "transfer"],
        base_score: 60,
    },
    PiiDefinition {
        id: "us_ssn",
        name: "US Social Security number",
        pattern: r"\b[0-9]{3}-[0-9]{2}-[0-9]{4}\b",
        category: Category::NationalId,
        context: &["ssn", "social", "security"],
        base_score: 40,
    },
    PiiDefinition {
        id: "us_itin",
        name: "US Individual Taxpayer number",
        pattern: r"\b9[0-9]{2}-[0-9]{2}-[0-9]{4}\b",
        category: Category::TaxId,
        context: &["itin", "taxpayer", "tax"],
        base_score: 40,
    },
    PiiDefinition {
        id: "us_aba_routing",
        name: "US ABA routing number",
        pattern: r"\b[0-9]{9}\b",
        category: Category::BankAccount,
        context: &["aba", "routing", "bank"],
        base_score: 20,
    },
    PiiDefinition {
        id: "uk_nino",
        name: "UK National Insurance number",
        pattern: r"(?i)\b[A-CEGHJ-PR-TW-Z][A-CEGHJ-NPR-TW-Z] ?[0-9]{2} ?[0-9]{2} ?[0-9]{2} ?[A-D]\b",
        category: Category::NationalId,
        context: &["nino", "insurance", "national"],
        base_score: 40,
    },
    PiiDefinition {
        id: "uk_nhs",
        name: "UK NHS number (mod-11)",
        pattern: r"\b[0-9]{3} ?[0-9]{3} ?[0-9]{4}\b",
        category: Category::Health,
        context: &["nhs", "health", "patient"],
        base_score: 20,
    },
    PiiDefinition {
        id: "email",
        name: "Email address",
        pattern: r"(?i)\b[A-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Z0-9](?:[A-Z0-9-]{0,61}[A-Z0-9])?(?:\.[A-Z0-9](?:[A-Z0-9-]{0,61}[A-Z0-9])?)+\b",
        category: Category::ContactInfo,
        context: &[],
        base_score: 60,
    },
    PiiDefinition {
        id: "es_dni",
        name: "Spain DNI / NIF (mod-23)",
        pattern: r"(?i)\b[0-9]{8}-?[A-Z]\b",
        category: Category::NationalId,
        context: &["dni", "nif", "identidad", "identificación"],
        base_score: 40,
    },
    PiiDefinition {
        id: "es_nie",
        name: "Spain NIE (mod-23)",
        pattern: r"(?i)\b[XYZ][0-9]{7}-?[A-Z]\b",
        category: Category::NationalId,
        context: &["nie", "extranjero", "identificación"],
        base_score: 40,
    },
    PiiDefinition {
        id: "phone",
        name: "International phone number",
        pattern: r"\+[1-9][0-9](?:[ -]?[0-9]){6,13}\b",
        category: Category::ContactInfo,
        context: &["phone", "mobile", "tel", "call"],
        base_score: 40,
    },
    PiiDefinition {
        id: "crypto_wallet",
        name: "Bitcoin wallet (Base58Check)",
        pattern: r"\b[13][1-9A-HJ-NP-Za-km-z]{25,34}\b",
        category: Category::Crypto,
        context: &["bitcoin", "wallet", "btc"],
        base_score: 60,
    },
];

pub(crate) fn valid(id: &str, candidate: &str) -> bool {
    let digits: Vec<u32> = candidate
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| u32::from(b - b'0'))
        .collect();
    match id {
        "payment_card" => {
            (13..=19).contains(&digits.len())
                && digits.iter().any(|d| *d != 0)
                && digits
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, d)| {
                        if i % 2 == 1 {
                            let doubled = d * 2;
                            if doubled > 9 {
                                doubled - 9
                            } else {
                                doubled
                            }
                        } else {
                            *d
                        }
                    })
                    .sum::<u32>()
                    % 10
                    == 0
        }
        "iban" => {
            let compact: Vec<_> = candidate
                .bytes()
                .filter(u8::is_ascii_alphanumeric)
                .map(|b| b.to_ascii_uppercase())
                .collect();
            if !(15..=34).contains(&compact.len())
                || !iban_length(&compact[..2]).is_some_and(|length| length == compact.len())
            {
                return false;
            }
            let mut remainder = 0u32;
            for b in compact[4..].iter().chain(&compact[..4]) {
                remainder = if b.is_ascii_digit() {
                    (remainder * 10 + u32::from(b - b'0')) % 97
                } else {
                    (remainder * 100 + u32::from(b - b'A' + 10)) % 97
                };
            }
            remainder == 1
        }
        "us_ssn" => {
            digits.len() == 9
                && !(digits[0] == 0 && digits[1] == 0 && digits[2] == 0)
                && digits[0] != 9
                && !(digits[0] == 6 && digits[1] == 6 && digits[2] == 6)
                && digits[3..5].iter().any(|d| *d != 0)
                && digits[5..].iter().any(|d| *d != 0)
        }
        "us_itin" => {
            if digits.len() != 9 || digits[0] != 9 {
                return false;
            }
            let middle = digits[3] * 10 + digits[4];
            ((50..=65).contains(&middle)
                || (70..=88).contains(&middle)
                || (90..=92).contains(&middle)
                || (94..=99).contains(&middle))
                && digits[5..].iter().any(|d| *d != 0)
        }
        "us_aba_routing" => {
            if digits.len() != 9 || digits.iter().all(|d| *d == 0) {
                return false;
            }
            let prefix = digits[0] * 10 + digits[1];
            ((1..=12).contains(&prefix)
                || (21..=32).contains(&prefix)
                || (61..=72).contains(&prefix)
                || prefix == 80)
                && digits
                    .iter()
                    .zip([3, 7, 1, 3, 7, 1, 3, 7, 1])
                    .map(|(d, w)| d * w)
                    .sum::<u32>()
                    % 10
                    == 0
        }
        "uk_nino" => !["BG", "GB", "KN", "NK", "NT", "TN", "ZZ"]
            .iter()
            .any(|prefix| candidate.to_ascii_uppercase().starts_with(prefix)),
        "uk_nhs" => {
            if digits.len() != 10 || digits.iter().all(|d| *d == 0) {
                return false;
            }
            let check = (11
                - digits[..9]
                    .iter()
                    .zip((2..=10).rev())
                    .map(|(d, w)| d * w)
                    .sum::<u32>()
                    % 11)
                % 11;
            check != 10 && check == digits[9]
        }
        "es_dni" | "es_nie" => valid_spanish_id(id, candidate),
        "crypto_wallet" => base58_decode(candidate).is_some_and(|bytes| {
            use sha2::{Digest, Sha256};
            bytes.len() == 25
                && (bytes[0] == 0 || bytes[0] == 5)
                && Sha256::digest(Sha256::digest(&bytes[..21]))[..4] == bytes[21..]
        }),
        _ => true,
    }
}
pub(crate) fn checksum(id: &str) -> bool {
    matches!(
        id,
        "payment_card"
            | "iban"
            | "us_aba_routing"
            | "uk_nhs"
            | "crypto_wallet"
            | "es_dni"
            | "es_nie"
    )
}

fn valid_spanish_id(id: &str, candidate: &str) -> bool {
    let compact: Vec<_> = candidate
        .bytes()
        .filter(|b| *b != b'-')
        .map(|b| b.to_ascii_uppercase())
        .collect();
    if compact.len() != 9 || !compact[8].is_ascii_uppercase() {
        return false;
    }
    let (prefix, digits) = if id == "es_nie" {
        let prefix = match compact[0] {
            b'X' => 0,
            b'Y' => 1,
            b'Z' => 2,
            _ => return false,
        };
        (prefix, &compact[1..8])
    } else {
        (0, &compact[..8])
    };
    if !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let number = digits
        .iter()
        .fold(prefix, |n, b| n * 10 + u32::from(b - b'0'));
    b"TRWAGMYFPDXBNJZSQVHLCKE"[number as usize % 23] == compact[8]
}
