// SPDX-License-Identifier: AGPL-3.0-or-later
//! The migrated free-tier pattern set.
//!
//! Every regex string here is copied VERBATIM from
//! `commander-free/src/paste_monitor.rs::PATTERNS` (byte-identical — this is
//! the "free-tier behaviour must stay byte-identical" requirement), so the
//! free-tier watcher and any custom fleet rule now go through the same
//! `compile()` / `CompiledRuleSet::evaluate` engine. The 3 unicode-anomaly
//! sub-checks (`paste_monitor.rs::check_unicode_anomaly`) are similarly
//! ported verbatim, but as procedural matchers rather than regexes — Rust
//! `regex` has no construct for "mixed-script host" or "RTL-aware bidi
//! override", so those stay hand-written functions dispatched by
//! `BuiltinPattern` instead of compiled patterns.
//!
//! Category → Severity mapping (the original `Category::severity()`
//! two-tier scheme, carried into this crate's four-tier `Severity`):
//!   - `CloudApi`, `AiApi`, `DevTools`, `PaymentComms`, `KeysAndCrypto` →
//!     `Severity::Warn` (was "warning": you copied YOUR OWN secret).
//!   - `MaliciousCommand`, `UnicodeAnomaly` → `Severity::High` (was
//!     "danger": the clipboard likely holds SOMEONE ELSE'S payload, or a
//!     near-certain spoofing indicator).
//!   - `PersonalData` doesn't appear here at all — it moved to
//!     `StructuredKind::PaymentCard` (see `structured.rs`), carrying the
//!     same `Severity::Warn` its `Category::severity()` would have given it
//!     (`PersonalData` falls through `Category::severity()`'s `_ =>
//!     "warning"` arm).
//!
//! `Severity::Info` and `Severity::Critical` are deliberately unused by any
//! builtin — headroom for custom fleet-authored rules above and below the
//! migrated set.

use serde::{Deserialize, Serialize};

/// One of the individually-named patterns migrated from the free-tier
/// `paste_monitor.rs` engine. Each variant is either backed by a fixed
/// regex (`regex_source` returns `Some`) or by a procedural check
/// (`regex_source` returns `None` — the three `Unicode*` variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-codegen", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-codegen", ts(export, export_to = "fleet.ts"))]
#[serde(rename_all = "snake_case")]
pub enum BuiltinPattern {
    // ── Cloud APIs ──────────────────────────────────────────────────
    AwsAccessKey,
    GoogleApiKey,
    SendgridApiKey,
    MailgunApiKey,
    TwilioAccountSid,
    DatabaseUrlWithCredentials,
    // ── AI APIs ─────────────────────────────────────────────────────
    OpenAiProjectKey,
    OpenAiApiKey,
    AnthropicApiKey,
    // ── Developer tools ─────────────────────────────────────────────
    GitHubClassicToken,
    GitHubFineGrainedToken,
    NpmToken,
    // ── Payments & comms ─────────────────────────────────────────────
    StripeLiveSecret,
    StripeLivePublishable,
    SlackToken,
    DiscordBotToken,
    // ── Keys & crypto ────────────────────────────────────────────────
    PrivateKeyPem,
    SshPrivateKeyHeader,
    Jwt,
    BitcoinWifPrivateKey,
    // ── Malicious commands (ClickFix / pastejacking) ────────────────
    PowershellEncodedPayload,
    HiddenPowershellWindow,
    PowershellExecutionPolicyBypass,
    PowershellRemoteDownloadExecute,
    MshtaWebPayload,
    CertutilWebDownload,
    Regsvr32WebPayload,
    BitsadminWebTransfer,
    CurlWgetPipeToShell,
    // ── Unicode anomalies — procedural, see the bottom of this file ─
    UnicodeBidiOverride,
    UnicodeZeroWidthInCode,
    UnicodeConfusableUrlHost,
}

impl BuiltinPattern {
    /// The fixed regex source for this pattern, or `None` for the three
    /// procedural `Unicode*` variants. Case sensitivity is baked into the
    /// pattern text itself (several malicious-command patterns embed an
    /// inline `(?i)`) — callers compile this source with
    /// `case_sensitive: true` (i.e. no builder-level override) so the
    /// inline flags are the only source of truth, matching the original
    /// engine exactly.
    pub(crate) fn regex_source(self) -> Option<&'static str> {
        use BuiltinPattern::*;
        Some(match self {
            AwsAccessKey => r"\bAKIA[0-9A-Z]{16}\b",
            GoogleApiKey => r"\bAIza[0-9A-Za-z_-]{35}\b",
            SendgridApiKey => r"\bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}\b",
            MailgunApiKey => r"\bkey-[a-f0-9]{32}\b",
            TwilioAccountSid => r"\bAC[0-9a-f]{32}\b",
            DatabaseUrlWithCredentials => {
                r"\b(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp|amqps)://[^\s:/@]+:[^\s@]+@"
            }
            OpenAiProjectKey => r"\bsk-proj-[A-Za-z0-9_-]{40,}\b",
            OpenAiApiKey => r"\bsk-[A-Za-z0-9]{48}\b",
            AnthropicApiKey => r"\bsk-ant-[A-Za-z0-9_-]{20,}\b",
            GitHubClassicToken => r"\bgh[pousr]_[A-Za-z0-9_]{36,255}\b",
            GitHubFineGrainedToken => r"\bgithub_pat_[A-Za-z0-9_]{20,255}\b",
            NpmToken => r"\bnpm_[A-Za-z0-9]{36}\b",
            StripeLiveSecret => r"\bsk_live_[A-Za-z0-9]{24,}\b",
            StripeLivePublishable => r"\bpk_live_[A-Za-z0-9]{24,}\b",
            SlackToken => r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
            DiscordBotToken => r"\b[A-Za-z0-9_-]{24}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,38}\b",
            PrivateKeyPem => {
                r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY( BLOCK)?-----"
            }
            SshPrivateKeyHeader => r"-----BEGIN OPENSSH PRIVATE KEY-----",
            Jwt => r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b",
            BitcoinWifPrivateKey => r"\b[5KL][1-9A-HJ-NP-Za-km-z]{50,51}\b",
            PowershellEncodedPayload => {
                r"(?i)\bpowershell(?:\.exe)?[^\n]*\s-(?:e|en|enc|encodedcommand)\b"
            }
            HiddenPowershellWindow => {
                r"(?i)\bpowershell(?:\.exe)?[^\n]*\s-(?:w|windowstyle)\s+hidden\b"
            }
            PowershellExecutionPolicyBypass => {
                r"(?i)\bpowershell(?:\.exe)?[^\n]*\s-(?:exp?|executionpolicy)\s+bypass\b"
            }
            PowershellRemoteDownloadExecute => {
                r"(?i)\b(?:iex|invoke-expression)\b[^\n]*\b(?:irm|iwr|invoke-restmethod|invoke-webrequest|downloadstring|downloadfile|new-object\s+net\.webclient)\b"
            }
            MshtaWebPayload => r"(?i)\bmshta(?:\.exe)?\s+https?://",
            CertutilWebDownload => r"(?i)\bcertutil(?:\.exe)?\s+(?:-urlcache|-decode)\b",
            Regsvr32WebPayload => r"(?i)\bregsvr32(?:\.exe)?\s+[^\n]*?/i:https?://",
            BitsadminWebTransfer => r"(?i)\bbitsadmin(?:\.exe)?\s+/transfer\b",
            CurlWgetPipeToShell => {
                r"(?i)\b(?:curl|wget|iwr|irm|invoke-webrequest|invoke-restmethod)\b[^\n|]+\|\s*(?:sh|bash|zsh|ksh|fish|iex|invoke-expression|cmd|powershell)\b"
            }
            UnicodeBidiOverride | UnicodeZeroWidthInCode | UnicodeConfusableUrlHost => return None,
        })
    }

    /// The severity this builtin carries by default — the console may let
    /// an admin override severity per-rule (that's a `Rule.severity` field
    /// choice, not something this crate enforces), but this is what a
    /// freshly-added builtin rule should start at. See the module-level
    /// doc comment for the full category→severity mapping this implements.
    pub fn default_severity(self) -> crate::Severity {
        use crate::Severity;
        use BuiltinPattern::*;
        match self {
            AwsAccessKey
            | GoogleApiKey
            | SendgridApiKey
            | MailgunApiKey
            | TwilioAccountSid
            | DatabaseUrlWithCredentials
            | OpenAiProjectKey
            | OpenAiApiKey
            | AnthropicApiKey
            | GitHubClassicToken
            | GitHubFineGrainedToken
            | NpmToken
            | StripeLiveSecret
            | StripeLivePublishable
            | SlackToken
            | DiscordBotToken
            | PrivateKeyPem
            | SshPrivateKeyHeader
            | Jwt
            | BitcoinWifPrivateKey => Severity::Warn,
            PowershellEncodedPayload
            | HiddenPowershellWindow
            | PowershellExecutionPolicyBypass
            | PowershellRemoteDownloadExecute
            | MshtaWebPayload
            | CertutilWebDownload
            | Regsvr32WebPayload
            | BitsadminWebTransfer
            | CurlWgetPipeToShell
            | UnicodeBidiOverride
            | UnicodeZeroWidthInCode
            | UnicodeConfusableUrlHost => Severity::High,
        }
    }

    /// The migrated pattern's human-readable name — identical to the
    /// original `PatternDef.name` / `check_unicode_anomaly`'s returned
    /// label. A static, crate-owned label (never clipboard content), so
    /// surfacing it to a user or the console does not violate the
    /// content-free rule.
    pub fn display_name(self) -> &'static str {
        use BuiltinPattern::*;
        match self {
            AwsAccessKey => "AWS Access Key",
            GoogleApiKey => "Google API Key",
            SendgridApiKey => "SendGrid API Key",
            MailgunApiKey => "Mailgun API Key",
            TwilioAccountSid => "Twilio Account SID",
            DatabaseUrlWithCredentials => "Database URL with credentials",
            OpenAiProjectKey => "OpenAI Project Key",
            OpenAiApiKey => "OpenAI API Key",
            AnthropicApiKey => "Anthropic API Key",
            GitHubClassicToken => "GitHub Classic Personal Token",
            GitHubFineGrainedToken => "GitHub Fine-Grained Token",
            NpmToken => "NPM Token",
            StripeLiveSecret => "Stripe Live Secret",
            StripeLivePublishable => "Stripe Live Publishable",
            SlackToken => "Slack Token",
            DiscordBotToken => "Discord Bot Token",
            PrivateKeyPem => "Private Key (PEM)",
            SshPrivateKeyHeader => "SSH Private Key Header",
            Jwt => "JWT",
            BitcoinWifPrivateKey => "Bitcoin WIF Private Key",
            PowershellEncodedPayload => "PowerShell encoded payload",
            HiddenPowershellWindow => "Hidden PowerShell window",
            PowershellExecutionPolicyBypass => "PowerShell ExecutionPolicy bypass",
            PowershellRemoteDownloadExecute => "PowerShell remote download + execute",
            MshtaWebPayload => "mshta web payload",
            CertutilWebDownload => "certutil web download",
            Regsvr32WebPayload => "regsvr32 web payload",
            BitsadminWebTransfer => "bitsadmin web transfer",
            CurlWgetPipeToShell => "curl/wget pipe to shell",
            UnicodeBidiOverride => "Bidi Override (U+202D/E)",
            UnicodeZeroWidthInCode => "Zero-Width Chars in Code",
            UnicodeConfusableUrlHost => "Confusable URL Host (mixed scripts)",
        }
    }
}

/// Dispatch for the three procedural (non-regex) builtins. Panics on any
/// regex-backed variant — `compile()` never routes those here (see
/// `BuiltinPattern::regex_source`), so this is an internal-consistency
/// assertion, not a reachable runtime path from external input.
pub(crate) fn matches_procedural(pattern: BuiltinPattern, text: &str) -> bool {
    match pattern {
        BuiltinPattern::UnicodeBidiOverride => bidi_override_outside_rtl(text),
        BuiltinPattern::UnicodeZeroWidthInCode => zero_width_in_code_context(text),
        BuiltinPattern::UnicodeConfusableUrlHost => confusable_url_host(text),
        other => unreachable!(
            "matches_procedural called on a regex-backed BuiltinPattern ({other:?}); compile() routes those through CompiledMatcher::Regex instead"
        ),
    }
}

// ── Unicode-anomaly matchers ──────────────────────────────────────────
//
// Verbatim port of `paste_monitor.rs::check_unicode_anomaly` and its three
// helpers, split into three independently-callable functions (one per
// `BuiltinPattern` variant) instead of one function returning the first of
// three possible names — this crate's `Rule`/`BuiltinPattern` model has no
// concept of "one category, several candidate names", so each sub-check
// becomes its own builtin, exactly as the other seven categories already
// have several named patterns apiece.

const ZERO_WIDTH: &[char] = &[
    '\u{200B}', // ZWSP
    '\u{200C}', // ZWNJ
    '\u{200D}', // ZWJ
    '\u{FEFF}', // BOM / ZWNBSP
];

const BIDI_OVERRIDE: &[char] = &['\u{202D}', '\u{202E}'];

/// `true` if `c` is in a script block that confuses with basic Latin.
/// Cyrillic basic (U+0400..U+04FF) and Greek basic (U+0370..U+03FF) are the
/// load-bearing cases for phishing URLs.
fn is_confusable_with_latin(c: char) -> bool {
    let n = c as u32;
    (0x0400..=0x04FF).contains(&n) || (0x0370..=0x03FF).contains(&n)
}

/// `true` if `c` is a member of an actual RTL script (Arabic/Hebrew). Used
/// to suppress bidi-override false-positives in genuine RTL text.
fn is_rtl_script(c: char) -> bool {
    let n = c as u32;
    (0x0590..=0x05FF).contains(&n) // Hebrew
        || (0x0600..=0x06FF).contains(&n) // Arabic
        || (0x0750..=0x077F).contains(&n) // Arabic Supplement
        || (0xFB50..=0xFDFF).contains(&n) // Arabic Presentation A
        || (0xFE70..=0xFEFF).contains(&n) // Arabic Presentation B
}

/// Find the FIRST URL host in the text. Bare-bones — no need for the `url`
/// crate; we just want the chars between the scheme `://` and the next
/// `/`, `?`, `#`, or whitespace.
fn first_url_host(text: &str) -> Option<&str> {
    let lower = text.as_bytes();
    let needle_a = b"http://";
    let needle_b = b"https://";
    let pos = (0..lower.len())
        .find(|&i| lower[i..].starts_with(needle_a) || lower[i..].starts_with(needle_b))?;
    let after_scheme = if lower[pos..].starts_with(needle_b) {
        pos + needle_b.len()
    } else {
        pos + needle_a.len()
    };
    let rest = &text[after_scheme..];
    let end = rest
        .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Bidi override (U+202D / U+202E) outside an RTL-script context — almost
/// always hostile filename-spoofing (`report<RLO>gpj.exe` displays as
/// `report...exe.jpg`).
fn bidi_override_outside_rtl(text: &str) -> bool {
    let has_bidi = text.chars().any(|c| BIDI_OVERRIDE.contains(&c));
    if !has_bidi {
        return false;
    }
    !text.chars().any(is_rtl_script)
}

/// Zero-width chars (U+200B/200C/200D/U+FEFF) immediately adjacent to an
/// ASCII alphanumeric — invisible payload injection. Suppresses legitimate
/// ZWJ usage in emoji sequences (the ZWJ sits between two emoji codepoints,
/// not letters/digits).
fn zero_width_in_code_context(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !ZERO_WIDTH.contains(&c) {
            continue;
        }
        let prev_ok = i > 0 && chars[i - 1].is_ascii_alphanumeric();
        let next_ok = i + 1 < chars.len() && chars[i + 1].is_ascii_alphanumeric();
        if prev_ok || next_ok {
            return true;
        }
    }
    false
}

/// Mixed-script URL host — Cyrillic/Greek codepoints in an otherwise-Latin
/// host (`pаypal.com` with a Cyrillic 'а').
fn confusable_url_host(text: &str) -> bool {
    let Some(host) = first_url_host(text) else {
        return false;
    };
    let has_latin = host.chars().any(|c| c.is_ascii_alphabetic());
    let has_confusable = host.chars().any(is_confusable_with_latin);
    has_latin && has_confusable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_regex_backed_variant_compiles() {
        // Not a size/lookaround check (compile.rs owns that) — just a
        // sanity net so a typo in one of the 29 hand-copied regex literals
        // above fails fast in THIS module's tests rather than surfacing as
        // a mysterious compile() error with no obvious cause.
        for pattern in ALL_BUILTINS {
            if let Some(src) = pattern.regex_source() {
                assert!(
                    regex::Regex::new(src).is_ok(),
                    "{pattern:?} regex source fails to compile: {src}"
                );
            }
        }
    }

    #[test]
    fn procedural_variants_have_no_regex_source() {
        assert_eq!(BuiltinPattern::UnicodeBidiOverride.regex_source(), None);
        assert_eq!(BuiltinPattern::UnicodeZeroWidthInCode.regex_source(), None);
        assert_eq!(
            BuiltinPattern::UnicodeConfusableUrlHost.regex_source(),
            None
        );
    }

    #[test]
    fn aws_key_pattern_matches() {
        let re = regex::Regex::new(BuiltinPattern::AwsAccessKey.regex_source().unwrap()).unwrap();
        assert!(re.is_match("AKIAABCDEFGHIJKLMNOP"));
    }

    #[test]
    fn confusable_host_detected() {
        let text = "Click https://p\u{0430}ypal.com/login to verify";
        assert!(confusable_url_host(text));
    }

    #[test]
    fn clean_url_not_confusable() {
        assert!(!confusable_url_host(
            "Visit https://example.com/path for details"
        ));
    }

    #[test]
    fn bidi_override_outside_rtl_detected() {
        assert!(bidi_override_outside_rtl("report\u{202E}gpj.exe"));
    }

    #[test]
    fn bidi_override_in_rtl_context_not_detected() {
        assert!(!bidi_override_outside_rtl("اَلْعَرَبِيَّةُ\u{202E}"));
    }

    #[test]
    fn zero_width_in_code_context_detected() {
        assert!(zero_width_in_code_context(
            "let api\u{200B}Key = 'aws-real-secret'"
        ));
    }

    #[test]
    fn zero_width_in_emoji_sequence_not_detected() {
        assert!(!zero_width_in_code_context("👨\u{200D}👩"));
    }

    /// Every variant, for exhaustive per-variant sanity checks above.
    /// Kept in the test module only — production code never needs to
    /// enumerate all builtins (compile() operates on whatever `Rule`s the
    /// caller supplies).
    const ALL_BUILTINS: &[BuiltinPattern] = &[
        BuiltinPattern::AwsAccessKey,
        BuiltinPattern::GoogleApiKey,
        BuiltinPattern::SendgridApiKey,
        BuiltinPattern::MailgunApiKey,
        BuiltinPattern::TwilioAccountSid,
        BuiltinPattern::DatabaseUrlWithCredentials,
        BuiltinPattern::OpenAiProjectKey,
        BuiltinPattern::OpenAiApiKey,
        BuiltinPattern::AnthropicApiKey,
        BuiltinPattern::GitHubClassicToken,
        BuiltinPattern::GitHubFineGrainedToken,
        BuiltinPattern::NpmToken,
        BuiltinPattern::StripeLiveSecret,
        BuiltinPattern::StripeLivePublishable,
        BuiltinPattern::SlackToken,
        BuiltinPattern::DiscordBotToken,
        BuiltinPattern::PrivateKeyPem,
        BuiltinPattern::SshPrivateKeyHeader,
        BuiltinPattern::Jwt,
        BuiltinPattern::BitcoinWifPrivateKey,
        BuiltinPattern::PowershellEncodedPayload,
        BuiltinPattern::HiddenPowershellWindow,
        BuiltinPattern::PowershellExecutionPolicyBypass,
        BuiltinPattern::PowershellRemoteDownloadExecute,
        BuiltinPattern::MshtaWebPayload,
        BuiltinPattern::CertutilWebDownload,
        BuiltinPattern::Regsvr32WebPayload,
        BuiltinPattern::BitsadminWebTransfer,
        BuiltinPattern::CurlWgetPipeToShell,
        BuiltinPattern::UnicodeBidiOverride,
        BuiltinPattern::UnicodeZeroWidthInCode,
        BuiltinPattern::UnicodeConfusableUrlHost,
    ];
}
