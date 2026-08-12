// SPDX-License-Identifier: AGPL-3.0-or-later
// src-tauri/commander-free/src/paste_monitor.rs
//
// ═══════════════════════════════════════════════════════════════════════
// PASTE MONITOR — credential-pattern clipboard watcher (F-1)
// ═══════════════════════════════════════════════════════════════════════
//
// Background task that polls the Windows clipboard every ~750ms. On
// content change, runs a fixed set of credential-format regexes
// (categorised) and emits the `paste-monitor-detected` Tauri event +
// a custom out-of-app alert on match.
//
// Privacy guarantees:
//   - Clipboard CONTENT never crosses the IPC boundary. The event
//     carries only the matched pattern's display name.
//   - SHA-256 of the clipboard content is held only in memory for
//     change-detection between polls; nothing persists to disk.
//   - The watcher never reads non-text clipboard formats (images, files).
//   - Recent-detections ring buffer holds pattern names + timestamps
//     only — no clipboard content.
//
// User-controllable surface (progressive disclosure on the frontend):
//   - Master ON/OFF toggle (already in Privacy panel).
//   - Per-category enable/disable — 6 categories so the user can
//     silence pattern groups they don't care about without picking
//     individual regexes.
//   - Snooze — temporary mute for 15 / 60 minutes when the user is
//     legitimately handling credentials.
//   - Recent detections — last 10 in memory, surfaces "caught N this
//     session" feedback.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

// ── Categories ──────────────────────────────────────────────────────
//
// 6 buckets chosen to match user mental models, not pattern provenance.
// Adding a new pattern = pick the closest category; don't add a 7th
// category just because the new pattern doesn't fit perfectly.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // PersonalData has no regex pattern — credit cards
                    // are detected procedurally via looks_like_credit_card,
                    // gated by `EnabledCategories.personal_data`.
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
    /// Credit cards.
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

impl Category {
    /// Two-tier severity: "warning" for credential-leak patterns
    /// (you copied YOUR secret, don't paste it in the wrong place);
    /// "danger" for malicious-command patterns (you copied SOMEONE
    /// ELSE'S payload — don't paste this anywhere).
    fn severity(self) -> &'static str {
        match self {
            // Bidi-override and zero-width-in-code are near-certainly
            // hostile; mixed-script-host is high-signal phishing. All
            // warrant the loud "danger" severity rather than "warning"
            // so the toast copy reads with urgency.
            Category::UnicodeAnomaly => "danger",
            Category::MaliciousCommand => "danger",
            _ => "warning",
        }
    }
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

// ── Pattern set ─────────────────────────────────────────────────────

struct PatternDef {
    name: &'static str,
    category: Category,
    re: Regex,
}

static PATTERNS: Lazy<Vec<PatternDef>> = Lazy::new(|| {
    let mk = |name: &'static str, cat: Category, pat: &str| PatternDef {
        name,
        category: cat,
        re: Regex::new(pat).expect("paste_monitor: invalid regex"),
    };
    vec![
        // ── Cloud APIs ──────────────────────────────────────────────
        mk(
            "AWS Access Key",
            Category::CloudApi,
            r"\bAKIA[0-9A-Z]{16}\b",
        ),
        mk(
            "Google API Key",
            Category::CloudApi,
            r"\bAIza[0-9A-Za-z_-]{35}\b",
        ),
        mk(
            "SendGrid API Key",
            Category::CloudApi,
            r"\bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}\b",
        ),
        mk(
            "Mailgun API Key",
            Category::CloudApi,
            r"\bkey-[a-f0-9]{32}\b",
        ),
        mk(
            "Twilio Account SID",
            Category::CloudApi,
            r"\bAC[0-9a-f]{32}\b",
        ),
        mk(
            "Database URL with credentials",
            Category::CloudApi,
            r"\b(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp|amqps)://[^\s:/@]+:[^\s@]+@",
        ),
        // ── AI APIs ─────────────────────────────────────────────────
        // OpenAI keys ship in two formats: legacy `sk-` + 48 alphanum,
        // and the newer project-scoped `sk-proj-` + ~120 alphanum.
        // sk-proj must come first so the legacy regex doesn't shadow it.
        mk(
            "OpenAI Project Key",
            Category::AiApi,
            r"\bsk-proj-[A-Za-z0-9_-]{40,}\b",
        ),
        mk("OpenAI API Key", Category::AiApi, r"\bsk-[A-Za-z0-9]{48}\b"),
        mk(
            "Anthropic API Key",
            Category::AiApi,
            r"\bsk-ant-[A-Za-z0-9_-]{20,}\b",
        ),
        // ── Developer Tools ─────────────────────────────────────────
        mk(
            "GitHub Classic Personal Token",
            Category::DevTools,
            r"\bgh[pousr]_[A-Za-z0-9_]{36,255}\b",
        ),
        mk(
            "GitHub Fine-Grained Token",
            Category::DevTools,
            r"\bgithub_pat_[A-Za-z0-9_]{20,255}\b",
        ),
        mk("NPM Token", Category::DevTools, r"\bnpm_[A-Za-z0-9]{36}\b"),
        // ── Payments & Comms ────────────────────────────────────────
        mk(
            "Stripe Live Secret",
            Category::PaymentComms,
            r"\bsk_live_[A-Za-z0-9]{24,}\b",
        ),
        mk(
            "Stripe Live Publishable",
            Category::PaymentComms,
            r"\bpk_live_[A-Za-z0-9]{24,}\b",
        ),
        mk(
            "Slack Token",
            Category::PaymentComms,
            r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
        ),
        mk(
            "Discord Bot Token",
            Category::PaymentComms,
            r"\b[A-Za-z0-9_-]{24}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,38}\b",
        ),
        // ── Keys & Crypto ───────────────────────────────────────────
        mk(
            "Private Key (PEM)",
            Category::KeysAndCrypto,
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY( BLOCK)?-----",
        ),
        mk(
            "SSH Private Key Header",
            Category::KeysAndCrypto,
            r"-----BEGIN OPENSSH PRIVATE KEY-----",
        ),
        mk(
            "JWT",
            Category::KeysAndCrypto,
            r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b",
        ),
        // Bitcoin WIF private key: starts with 5 (uncompressed) or K/L
        // (compressed); Base58Check, no 0/O/I/l. 51 or 52 chars total.
        mk(
            "Bitcoin WIF Private Key",
            Category::KeysAndCrypto,
            r"\b[5KL][1-9A-HJ-NP-Za-km-z]{50,51}\b",
        ),
        // ── Personal Data ───────────────────────────────────────────
        // Credit cards aren't here — handled separately because Luhn
        // doesn't fit cleanly into the regex pipeline. See check_patterns.

        // ── Malicious Commands (ClickFix / pastejacking defence) ────
        //
        // These patterns are the actual TTPs that "verify-you're-human"
        // and fake-CAPTCHA pages tell users to paste into Win+R or
        // PowerShell. False-positive rate is near zero — almost no
        // legitimate workflow puts these strings on the clipboard.
        //
        // Case-insensitive on the case-significant tokens (`(?i)`)
        // because attacker copy varies wildly: `POWERSHELL`, `PowerShell`,
        // `pOwErShElL` all in the wild.

        // Encoded PowerShell payload — `-enc <base64>`. The classic
        // ClickFix pattern: legit-looking command, but the actual
        // payload is base64'd to hide what it does.
        mk(
            "PowerShell encoded payload",
            Category::MaliciousCommand,
            r"(?i)\bpowershell(?:\.exe)?[^\n]*\s-(?:e|en|enc|encodedcommand)\b",
        ),
        // Hidden-window PowerShell — common with -enc to keep the
        // window invisible while malware runs.
        mk(
            "Hidden PowerShell window",
            Category::MaliciousCommand,
            r"(?i)\bpowershell(?:\.exe)?[^\n]*\s-(?:w|windowstyle)\s+hidden\b",
        ),
        // PowerShell execution-policy bypass — almost always used to
        // run untrusted scripts.
        mk(
            "PowerShell ExecutionPolicy bypass",
            Category::MaliciousCommand,
            r"(?i)\bpowershell(?:\.exe)?[^\n]*\s-(?:exp?|executionpolicy)\s+bypass\b",
        ),
        // Invoke-Expression of remote download — `iex (irm http://...)`,
        // `iex (iwr http://...)`, `iex (New-Object Net.WebClient).DownloadString`.
        // The malicious-payload-from-web pattern, full stop.
        mk(
            "PowerShell remote download + execute",
            Category::MaliciousCommand,
            r"(?i)\b(?:iex|invoke-expression)\b[^\n]*\b(?:irm|iwr|invoke-restmethod|invoke-webrequest|downloadstring|downloadfile|new-object\s+net\.webclient)\b",
        ),
        // mshta executing remote content — Microsoft HTML Application
        // host, abused as a LOLBin to run remote .hta scripts.
        mk(
            "mshta web payload",
            Category::MaliciousCommand,
            r"(?i)\bmshta(?:\.exe)?\s+https?://",
        ),
        // certutil downloading a file — the canonical "abuse a Windows
        // signed binary to fetch malware" trick.
        mk(
            "certutil web download",
            Category::MaliciousCommand,
            r"(?i)\bcertutil(?:\.exe)?\s+(?:-urlcache|-decode)\b",
        ),
        // regsvr32 with /i:http — fetches and executes a remote
        // Squiblydoo-style scriptlet.
        mk(
            "regsvr32 web payload",
            Category::MaliciousCommand,
            r"(?i)\bregsvr32(?:\.exe)?\s+[^\n]*?/i:https?://",
        ),
        // bitsadmin transferring from web — abuses Windows BITS to
        // fetch payloads.
        mk(
            "bitsadmin web transfer",
            Category::MaliciousCommand,
            r"(?i)\bbitsadmin(?:\.exe)?\s+/transfer\b",
        ),
        // curl/wget piped to shell — the *nix and cross-shell variant.
        mk(
            "curl/wget pipe to shell",
            Category::MaliciousCommand,
            r"(?i)\b(?:curl|wget|iwr|irm|invoke-webrequest|invoke-restmethod)\b[^\n|]+\|\s*(?:sh|bash|zsh|ksh|fish|iex|invoke-expression|cmd|powershell)\b",
        ),
    ]
});

// ── Credit-card Luhn check (Category::PersonalData) ─────────────────

fn looks_like_credit_card(text: &str) -> bool {
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
/// On each poll tick the watcher checks this — if deadline reached AND
/// current clipboard hash still matches, it clears the clipboard.
#[allow(clippy::type_complexity)]
static PENDING_CLEAR: Lazy<Mutex<Option<(Instant, [u8; 32])>>> = Lazy::new(|| Mutex::new(None));

fn clear_clipboard() -> bool {
    // Setting an empty Unicode string is the cross-app-compat way to
    // "clear" — `clipboard_win::empty()` works too but some apps poll
    // the clipboard expecting SOME format to be present.
    clipboard_win::set_clipboard_string("").is_ok()
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
        WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
        WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
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

// ── Helpers ─────────────────────────────────────────────────────────

fn poll_clipboard_text() -> Option<String> {
    use clipboard_win::{formats, get_clipboard};
    // Single-attempt read. Previous retry loop slept up to 150ms total
    // via std::thread::sleep on a tokio worker — that blocks the runtime
    // and the next tick re-reads anyway. Transient clipboard-locked
    // failures are tolerable; we'll catch the content on the next poll.
    get_clipboard::<String, _>(formats::Unicode).ok()
}

fn hash_text(s: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    h.finalize().into()
}

fn check_patterns(text: &str, enabled: &EnabledCategories) -> Option<(&'static str, Category)> {
    if text.len() < 13 {
        return None;
    }
    for p in PATTERNS.iter() {
        if !enabled.allows(p.category) {
            continue;
        }
        if p.re.is_match(text) {
            return Some((p.name, p.category));
        }
    }
    if enabled.personal_data && looks_like_credit_card(text) {
        return Some(("Credit Card Number", Category::PersonalData));
    }
    if enabled.unicode {
        if let Some(name) = check_unicode_anomaly(text) {
            return Some((name, Category::UnicodeAnomaly));
        }
    }
    None
}

// ── Unicode-anomaly matchers ────────────────────────────────────────
//
// Three sub-matchers, ordered by signal strength:
//
//   1. Bidi override (U+202D / U+202E) outside an RTL-script context.
//      Almost-always hostile — `filename.txt<RLO>gpj.exe` style spoofing,
//      where the embedded U+202E reverses the display so the file looks
//      like `filename.txt.exe.jpg` to the operator. We DON'T inline the
//      real codepoint in source here because rustc refuses to compile
//      comments containing bidi-control chars (rightly).
//   2. Zero-width chars (U+200B/200C/200D/U+FEFF) inside what looks like
//      code/identifier text. Used for invisible payload injection.
//   3. Mixed-script URL host — Cyrillic / Greek codepoints in an
//      otherwise-Latin host (`pаypal.com` with Cyrillic 'а').
//
// Order matters: bidi-override doesn't depend on length or context, so
// it's cheapest and fires first. Mixed-script needs a URL parse, so
// it's last.

const ZERO_WIDTH: &[char] = &[
    '\u{200B}', // ZWSP
    '\u{200C}', // ZWNJ
    '\u{200D}', // ZWJ
    '\u{FEFF}', // BOM / ZWNBSP
];

const BIDI_OVERRIDE: &[char] = &['\u{202D}', '\u{202E}'];

/// `true` if `c` is in a script block that confuses with basic Latin.
/// Cyrillic basic (U+0400..U+04FF) and Greek basic (U+0370..U+03FF) are
/// the load-bearing cases for phishing URLs.
fn is_confusable_with_latin(c: char) -> bool {
    let n = c as u32;
    (0x0400..=0x04FF).contains(&n) || (0x0370..=0x03FF).contains(&n)
}

/// `true` if `c` is a member of an actual RTL script (Arabic/Hebrew).
/// Used to suppress bidi-override false-positives in genuine RTL text.
fn is_rtl_script(c: char) -> bool {
    let n = c as u32;
    (0x0590..=0x05FF).contains(&n) // Hebrew
        || (0x0600..=0x06FF).contains(&n) // Arabic
        || (0x0750..=0x077F).contains(&n) // Arabic Supplement
        || (0xFB50..=0xFDFF).contains(&n) // Arabic Presentation A
        || (0xFE70..=0xFEFF).contains(&n) // Arabic Presentation B
}

/// Find the FIRST URL host in the text. Returns `None` if no URL.
/// Bare-bones — no need for the `url` crate; we just want chars between
/// the scheme `://` and the next `/`, `?`, `#`, or whitespace.
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

/// Returns the matched-pattern name (stable string for the event) if a
/// unicode anomaly is detected; `None` otherwise.
fn check_unicode_anomaly(text: &str) -> Option<&'static str> {
    // 1) Bidi override outside RTL context.
    let has_bidi = text.chars().any(|c| BIDI_OVERRIDE.contains(&c));
    if has_bidi {
        let has_rtl_text = text.chars().any(is_rtl_script);
        if !has_rtl_text {
            return Some("Bidi Override (U+202D/E)");
        }
    }

    // 2) Zero-width chars in code-like context. "Code-like" = the
    //    char IMMEDIATELY before or after is ASCII alphanumeric. This
    //    suppresses legitimate ZWJ usage in emoji sequences (the ZWJ
    //    sits between two emoji codepoints, not letters/digits).
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !ZERO_WIDTH.contains(&c) {
            continue;
        }
        let prev_ok = i > 0 && chars[i - 1].is_ascii_alphanumeric();
        let next_ok = i + 1 < chars.len() && chars[i + 1].is_ascii_alphanumeric();
        if prev_ok || next_ok {
            return Some("Zero-Width Chars in Code");
        }
    }

    // 3) Mixed-script URL host.
    if let Some(host) = first_url_host(text) {
        let has_latin = host.chars().any(|c| c.is_ascii_alphabetic());
        let has_confusable = host.chars().any(is_confusable_with_latin);
        if has_latin && has_confusable {
            return Some("Confusable URL Host (mixed scripts)");
        }
    }

    None
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

// ── Tauri command surface ───────────────────────────────────────────

#[tauri::command]
pub async fn start_paste_monitor(app: AppHandle) -> Result<(), String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(()); // already running, idempotent
    }
    crate::log_message("debug", "[PasteMonitor] watcher started");

    tauri::async_runtime::spawn(async move {
        let mut last_hash: Option<[u8; 32]> = None;
        // 2s poll matches typical paste-attack reaction window without
        // burning a tokio worker every 750ms on a hot loop that
        // regex-scans + SHA-256s the clipboard every tick.
        let mut interval = tokio::time::interval(Duration::from_millis(2000));
        interval.tick().await; // discard first immediate tick
        let mut was_locked = false;

        while RUNNING.load(Ordering::SeqCst) {
            interval.tick().await;

            // Auto-clear on lock: fires only on the unlocked→locked
            // transition so a station that was already locked at startup
            // does not trigger a spurious clear.
            if let Some(locked) = workstation_is_locked() {
                if locked
                    && !was_locked
                    && AUTO_CLEAR_ON_LOCK.load(Ordering::SeqCst)
                    && clear_clipboard()
                {
                    crate::log_message(
                        "info",
                        "[PasteMonitor] auto-clear on lock — clipboard erased",
                    );
                    last_hash = Some(hash_text(""));
                }
                was_locked = locked;
            }

            // Auto-clear: if a clear is pending and its deadline has passed,
            // erase the clipboard but only if its content hasn't changed since
            // the detection (user didn't deliberately copy something else).
            // Runs even while snoozed — snooze suppresses DETECTION, not the
            // protective erase of an already-detected payload.
            {
                let pending = *PENDING_CLEAR.lock().unwrap();
                if let Some(pending) = pending {
                    if Instant::now() >= pending.0 {
                        let current_hash = poll_clipboard_text().map(|t| hash_text(&t));
                        if should_auto_clear(pending, Instant::now(), current_hash)
                            && clear_clipboard()
                        {
                            crate::log_message(
                                "info",
                                "[PasteMonitor] auto-clear fired — clipboard erased",
                            );
                            last_hash = Some(hash_text(""));
                        }
                        *PENDING_CLEAR.lock().unwrap() = None;
                    }
                }
            }

            if is_snoozed() {
                continue;
            }

            let text = match poll_clipboard_text() {
                Some(t) => t,
                None => continue,
            };
            let h = hash_text(&text);
            if last_hash == Some(h) {
                continue;
            }
            last_hash = Some(h);

            let enabled = *ENABLED_CATEGORIES.lock().unwrap();

            // ── Crypto-swap detection (runs BEFORE category patterns so
            //    clipboard-hijack signatures aren't masked by an unrelated
            //    secret accidentally landing on the clipboard).
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

                // Schedule auto-clear if enabled — crypto-swap warrants
                // aggressive clearing because the attacker's address
                // sitting on the clipboard is the active threat.
                if AUTO_CLEAR_ENABLED.load(Ordering::SeqCst) {
                    let secs = AUTO_CLEAR_SECONDS.load(Ordering::SeqCst) as u64;
                    *PENDING_CLEAR.lock().unwrap() = Some(schedule_clear(Instant::now(), secs, h));
                }

                // Don't ALSO run pattern check — the swap is the primary signal.
                continue;
            }

            if let Some((pattern, category)) = check_patterns(&text, &enabled) {
                let severity = category.severity();
                let payload = DetectionEvent {
                    pattern: pattern.to_string(),
                    severity: severity.to_string(),
                    detected_at: chrono::Utc::now().to_rfc3339(),
                };
                let _ = app.emit("paste-monitor-detected", &payload);

                // Schedule auto-clear if enabled.
                if AUTO_CLEAR_ENABLED.load(Ordering::SeqCst) {
                    let secs = AUTO_CLEAR_SECONDS.load(Ordering::SeqCst) as u64;
                    *PENDING_CLEAR.lock().unwrap() = Some(schedule_clear(Instant::now(), secs, h));
                }

                // Different toast copy for danger severity. Credential
                // leaks: "be careful where you paste". Malicious commands:
                // "do NOT paste this anywhere — it's likely malware."
                // The danger phrasing has to be unambiguous because the
                // user is being actively socially-engineered at this
                // exact moment ("just press Win+R and paste this to
                // verify you're human").
                let is_powershell = pattern.to_ascii_lowercase().contains("powershell")
                    || pattern.to_ascii_lowercase().contains("encodedcommand")
                    || pattern.to_ascii_lowercase().contains("executionpolicy")
                    || pattern.to_ascii_lowercase().contains("pwsh");
                let (title, body) = if severity == "danger" && is_powershell {
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
                };
                if let Err(e) = crate::native_notify::show_native_notification(&app, title, &body) {
                    crate::log_message(
                        "warn",
                        &format!("[PasteMonitor] notification failed: {}", e),
                    );
                }

                push_recent(payload);
                crate::log_message(
                    "info",
                    &format!("[PasteMonitor] detected ({}): {}", severity, pattern),
                );
            }
        }
        crate::log_message("debug", "[PasteMonitor] watcher stopped");
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_paste_monitor() -> Result<(), String> {
    RUNNING.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn paste_monitor_status() -> Result<bool, String> {
    Ok(RUNNING.load(Ordering::SeqCst))
}

/// Update which pattern categories the watcher fires for. Frontend calls
/// this on app boot (after settings load) and on every category toggle.
/// Settings.json is the persistent layer; this mutex is the runtime
/// authority that the watcher reads from.
#[tauri::command]
pub async fn set_paste_monitor_categories(categories: EnabledCategories) -> Result<(), String> {
    *ENABLED_CATEGORIES.lock().unwrap() = categories;
    Ok(())
}

#[tauri::command]
pub async fn get_paste_monitor_categories() -> Result<EnabledCategories, String> {
    Ok(*ENABLED_CATEGORIES.lock().unwrap())
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
// TESTS — unicode-anomaly detection
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confusable_host_detected() {
        // Cyrillic 'а' (U+0430) in an otherwise-Latin paypal.com.
        let text = "Click https://p\u{0430}ypal.com/login to verify";
        assert_eq!(
            check_unicode_anomaly(text),
            Some("Confusable URL Host (mixed scripts)")
        );
    }

    #[test]
    fn clean_url_passes() {
        let text = "Visit https://example.com/path for details";
        assert_eq!(check_unicode_anomaly(text), None);
    }

    #[test]
    fn bidi_override_outside_rtl_detected() {
        // U+202E flips display order — classic filename-spoof trick.
        let text = "report\u{202E}gpj.exe";
        assert_eq!(
            check_unicode_anomaly(text),
            Some("Bidi Override (U+202D/E)")
        );
    }

    #[test]
    fn bidi_override_in_rtl_context_passes() {
        // Real Arabic text with bidi marker shouldn't fire.
        let text = "اَلْعَرَبِيَّةُ\u{202E}";
        assert_eq!(check_unicode_anomaly(text), None);
    }

    #[test]
    fn zero_width_in_code_context_detected() {
        // ZWSP between two letters — invisible payload injection.
        let text = "let api\u{200B}Key = 'aws-real-secret'";
        assert_eq!(
            check_unicode_anomaly(text),
            Some("Zero-Width Chars in Code")
        );
    }

    #[test]
    fn zero_width_in_emoji_sequence_passes() {
        // ZWJ legitimately joining two emoji codepoints. The chars on
        // either side aren't ASCII-alphanumeric so we should NOT fire.
        let text = "👨\u{200D}👩";
        assert_eq!(check_unicode_anomaly(text), None);
    }

    #[test]
    fn first_url_host_picks_first() {
        assert_eq!(
            first_url_host("see https://a.example.com/path and https://b.example.com"),
            Some("a.example.com")
        );
        assert_eq!(first_url_host("no url here"), None);
        assert_eq!(first_url_host("http://host.test?q=1"), Some("host.test"));
    }

    #[test]
    fn category_enable_gates_unicode_check() {
        let mut cats = EnabledCategories {
            unicode: false,
            ..Default::default()
        };
        let text = "https://p\u{0430}ypal.com";
        assert!(check_patterns(text, &cats).is_none());

        cats.unicode = true;
        assert!(matches!(
            check_patterns(text, &cats),
            Some((_, Category::UnicodeAnomaly))
        ));
    }

    #[test]
    fn github_pat_variants_are_detected() {
        let cats = EnabledCategories::default();
        let classic = format!("ghu_{}", "A".repeat(36));
        assert_eq!(
            check_patterns(&classic, &cats),
            Some(("GitHub Classic Personal Token", Category::DevTools))
        );

        let fine_grained = format!("github_pat_{}", "A".repeat(70));
        assert_eq!(
            check_patterns(&fine_grained, &cats),
            Some(("GitHub Fine-Grained Token", Category::DevTools))
        );
    }

    #[test]
    fn plain_pat_word_does_not_fire() {
        let cats = EnabledCategories::default();
        assert!(check_patterns("pat", &cats).is_none());
        assert!(check_patterns("personal access token", &cats).is_none());
    }

    // ═══════════════════════════════════════════════════════════════
    // Crypto-address extraction (pure matcher) + swap window/debounce
    // (decide_crypto_swap with injected Instant — no real sleeps).
    // All addresses below are the clearly-fake T1 verification vectors.
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
    // schedule_clear) — deterministic, no real sleeps.
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
}
