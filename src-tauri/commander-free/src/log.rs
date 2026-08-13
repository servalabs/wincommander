use std::collections::HashMap;
use std::collections::HashSet;
#[cfg(not(test))]
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// Atomic flag keeping the current `app.loggingEnabled` state.
/// Initialised to `true`; synced from the encrypted settings store by
/// `settings::sync_logging_flag()` on every settings read or write so
/// the plaintext `settings.json` fallback is no longer needed.
pub(crate) static LOGGING_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

/// Minimum level that gets persisted. Levels below this are silently dropped.
/// warn/error/danger always pass regardless.
///
/// Quiet by default — set `WINCMD_LOG=debug` (or `=info`) for full verbosity.
/// Hierarchy (lowest → highest): debug < info < warn < error/danger.
static LOG_MIN_LEVEL: std::sync::OnceLock<u8> = std::sync::OnceLock::new();

/// Within this window, identical records are coalesced into a count instead of
/// writing a new line each time — stops the same error from burying the log.
const DEDUP_WINDOW_SECS: i64 = 60;

struct AggEntry {
    body: String, // full original formatted body incl. stack trace
    count: u32,
    first: chrono::DateTime<chrono::Local>,
    last: chrono::DateTime<chrono::Local>,
}

// Mutex::new is const; HashMap::new is NOT const, so wrap in Option and lazily init.
static DEDUP: Mutex<Option<HashMap<String, AggEntry>>> = Mutex::new(None);

/// Collapse variable numeric tokens so similar messages share a dedup key.
/// Strategy: replace digit-only tokens (word-boundary separated) with `#`.
/// Short standalone integers (1-2 digits surrounded by whitespace/end) are
/// preserved so `code 12` keeps `12`; numeric tokens embedded in dotted/colon
/// notation (IP addresses, ports within an IP) are collapsed because they vary
/// per-host. Additionally any run of 4+ consecutive digits is always collapsed.
fn normalize_key(level: &str, source: &str, message: &str) -> String {
    // Tokenize on whitespace; within each token collapse numeric sub-parts that
    // are embedded in network-address punctuation (dots, colons, slashes).
    let mut out = String::with_capacity(message.len());
    for (i, token) in message.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        // If the token looks like a network address (digits + dots/colons/slashes)
        // replace it entirely with `#`.
        let is_net_addr = token
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ':' || c == '/')
            && token.contains('.')
            && token.chars().any(|c| c.is_ascii_digit());
        if is_net_addr {
            out.push('#');
            continue;
        }
        // Otherwise, collapse any run of 4+ consecutive digits in the token.
        let mut digit_run = 0usize;
        for ch in token.chars() {
            if ch.is_ascii_digit() {
                digit_run += 1;
                if digit_run <= 3 {
                    out.push(ch);
                } else if digit_run == 4 {
                    out.push('#');
                }
            } else {
                digit_run = 0;
                out.push(ch);
            }
        }
    }
    format!("{}|{}|{}", level.to_ascii_lowercase(), source, out)
}

/// 0=debug, 1=info, 2=warn, 3=error/danger
fn min_level() -> u8 {
    *LOG_MIN_LEVEL.get_or_init(|| {
        match std::env::var("WINCMD_LOG")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "debug" => 0,
            "info" => 1,
            // default: warn — routine info/debug lines are dropped
            _ => 2,
        }
    })
}

/// Returns true if `level` should be recorded given the current threshold.
/// warn / error / danger always pass (numeric >= 2).
fn level_passes(level: &str) -> bool {
    let l = level.to_ascii_lowercase();
    let rank: u8 = match l.as_str() {
        "debug" => 0,
        "info" => 1,
        "warn" => 2,
        _ => 3, // error, danger, unknown → always record
    };
    rank >= min_level()
}

pub fn set_logging_enabled_flag(enabled: bool) {
    let was_enabled = LOGGING_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
    LOGGING_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
    // KT: logging OFF→ON bug — the flag is stored atomically but the log dir may
    // have been cleaned while logging was off (purge, manual delete). Re-enabling
    // without a forced write attempt left callers silent: log_message_src's early
    // `Err(_) => return` on paths::user_logs_dir() silently dropped every message.
    // Force-creating the dir here and writing a "logging enabled" sentinel confirms
    // the path is live before the first real message arrives.
    if enabled && !was_enabled {
        log_message_src("info", LOG_SRC_CORE, "[Logging] logging enabled");
    }
}

fn logging_enabled() -> bool {
    LOGGING_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Trim log records older than `keep_days` from `wincommander.log`.
/// Handles both the new `L:YYYY-MM-DD:` encrypted format and the legacy
/// `[YYYY-MM-DD` plaintext format. Only rewrites when lines are removed.
pub fn purge_old_log_records(log_file: &Path, keep_days: u64) {
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(keep_days as i64);

    let content = match std::fs::read_to_string(log_file) {
        Ok(c) => c,
        Err(_) => return,
    };

    let total = content.lines().count();
    let kept: Vec<&str> = content
        .lines()
        .filter(|line| {
            // Encrypted format: L:YYYY-MM-DD:...
            if let Some(rest) = line.strip_prefix("L:") {
                if rest.len() >= 10 {
                    if let Ok(date) = chrono::NaiveDate::parse_from_str(&rest[..10], "%Y-%m-%d") {
                        return date >= cutoff;
                    }
                }
                return true;
            }
            // Legacy plaintext: [YYYY-MM-DD ...
            if let Some(rest) = line.strip_prefix('[') {
                if rest.len() >= 10 {
                    if let Ok(date) = chrono::NaiveDate::parse_from_str(&rest[..10], "%Y-%m-%d") {
                        return date >= cutoff;
                    }
                }
            }
            true
        })
        .collect();

    if kept.len() < total {
        let new_content = kept.join("\n") + if kept.is_empty() { "" } else { "\n" };
        let _ = std::fs::write(log_file, new_content.as_bytes());
    }
}

/// Re-encrypt any plaintext log lines left from before the encryption
/// migration. Called once at startup before purge_old_log_records in release
/// builds. Debug builds deliberately use the inverse migration below so a
/// local Tauri dev session can be inspected without app-side decryption.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn migrate_plaintext_logs(log_file: &Path) {
    let content = match std::fs::read_to_string(log_file) {
        Ok(c) => c,
        Err(_) => return,
    };
    // Short-circuit if every non-empty line is already encrypted.
    if !content
        .lines()
        .any(|l| !l.is_empty() && !l.starts_with("L:"))
    {
        return;
    }

    let new_lines: Vec<String> = content
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            if line.starts_with("L:") {
                return line.to_string();
            }
            // Legacy: [YYYY-MM-DD HH:MM:SS] [LEVEL] message
            if let Some(rest) = line.strip_prefix('[') {
                if rest.len() > 11
                    && chrono::NaiveDate::parse_from_str(&rest[..10], "%Y-%m-%d").is_ok()
                {
                    let date = &rest[..10];
                    // body becomes [HH:MM:SS] [LEVEL] message
                    let body = format!("[{}", &rest[11..]);
                    return crate::datastore::log_encrypt_line(date, &body);
                }
            }
            // Unknown format: encrypt today-dated
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            crate::datastore::log_encrypt_line(&date, line)
        })
        .collect();

    let new_content = new_lines.join("\n") + "\n";
    let _ = std::fs::write(log_file, new_content.as_bytes());
}

/// Debug builds only: rewrite decryptable records as the legacy readable
/// format. This is intentionally compile-time gated — a production/release
/// binary cannot enable plaintext logging through a setting or environment
/// variable. An unreadable record is retained rather than silently discarded.
#[cfg(debug_assertions)]
pub(crate) fn migrate_logs_to_plaintext_for_debug(log_file: &Path) {
    let content = match std::fs::read_to_string(log_file) {
        Ok(content) => content,
        Err(_) => return,
    };
    if !content.lines().any(|line| line.starts_with("L:")) {
        return;
    }
    let lines: Vec<String> = content
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            if let Some((date, body)) = crate::datastore::log_decrypt_line(line) {
                // `body` begins `[HH:MM:SS]`, so joining after its first
                // bracket produces the regular `[YYYY-MM-DD HH:MM:SS]` form
                // understood by the in-app viewer and ordinary text tools.
                return format!("[{} {}", date, body.trim_start_matches('['));
            }
            line.to_string()
        })
        .collect();
    let _ = std::fs::write(log_file, lines.join("\n") + "\n");
}

// Log sources tagged into each record so the in-app Error Center can show
// where a line came from. `core` = Free backend, `ui` = frontend (console +
// window errors), `pro` = the Pro sidecar (drained from its stderr).
// Module-specific sources are used by optional features so their log entries
// can be suppressed when those features are disabled.
pub const LOG_SRC_CORE: &str = "core";
pub const LOG_SRC_UI: &str = "ui";
pub const LOG_SRC_PRO: &str = "pro";
pub const LOG_SRC_INVESTIGATOR: &str = "investigator";
pub const LOG_SRC_FLOWS: &str = "flows";
pub const LOG_SRC_SERVER_APPS: &str = "server-apps";

pub fn log_message(level: &str, message: &str) {
    log_message_src(level, LOG_SRC_CORE, message);
}

/// Append one formatted body line. Debug/Tauri-development binaries keep it
/// plaintext for direct troubleshooting; release binaries use the encrypted
/// on-disk format. This is a compile-time distinction, never a user setting.
fn write_log_line(date: &str, body: &str) {
    #[cfg(test)]
    {
        let _ = (date, body);
    }
    #[cfg(not(test))]
    {
        let log_dir = match crate::paths::user_logs_dir() {
            Ok(d) => d,
            Err(_) => return,
        };
        if !log_dir.exists() && std::fs::create_dir_all(&log_dir).is_err() {
            return;
        }
        let log_file = log_dir.join("wincommander.log");
        #[cfg(debug_assertions)]
        let line = format!("[{} {}", date, body.trim_start_matches('['));
        #[cfg(not(debug_assertions))]
        let line = crate::datastore::log_encrypt_line(date, body);
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
        {
            let _ = file.write_all(format!("{}\n", line).as_bytes());
        }
    }
}

/// Like `log_message` but stamps an explicit `[SRC]` token into the record so
/// the Error Center can attribute it to frontend / core / Pro sidecar.
pub fn log_message_src(level: &str, source: &str, message: &str) {
    if !logging_enabled() || !level_passes(level) {
        return;
    }
    let now = chrono::Local::now();
    let key = normalize_key(level, source, message);
    // Compute the first-occurrence body outside the lock (cheap, no IO).
    let body = format!(
        "[{}] [{}] [{}] {}",
        now.format("%H:%M:%S"),
        level.to_uppercase(),
        source,
        message
    );

    // Collect any expired-aggregate summary while holding the lock, then
    // release the lock before all disk IO (mirrors the first-occurrence path).
    let expired_summary: Option<(String, String)> = {
        let mut guard = DEDUP.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(e) = map.get_mut(&key) {
            if (now - e.first).num_seconds() < DEDUP_WINDOW_SECS {
                e.count += 1;
                e.last = now;
                return; // suppress raw write; summary flushes on expiry/sweeper
            }
            // window expired → snapshot what we need, remove from map, release lock
            // count - 1: the first occurrence already has its own on-disk line;
            // the summary carries only the suppressed-repeat count so the viewer's
            // groupLogRecords total = 1 + (count-1) = count = true fires.
            let summary = if e.count > 1 {
                Some((
                    format!(
                        "[x{} {}-{}] {}",
                        e.count - 1,
                        e.first.format("%H:%M:%S"),
                        e.last.format("%H:%M:%S"),
                        e.body
                    ),
                    e.first.format("%Y-%m-%d").to_string(),
                ))
            } else {
                None
            };
            map.remove(&key);
            summary
        } else {
            None
        }
        // guard drops here — lock released before any disk IO below
    };
    // Write the expired aggregate summary OUTSIDE the lock.
    if let Some((summary, date)) = expired_summary {
        write_log_line(&date, &summary);
    }
    // First occurrence in this window: record it and write immediately (full body kept).
    {
        let mut guard = DEDUP.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            key,
            AggEntry {
                body: body.clone(),
                count: 1,
                first: now,
                last: now,
            },
        );
    } // lock released here, before write_log_line
    write_log_line(&now.format("%Y-%m-%d").to_string(), &body);
}

/// Flush any aggregate whose window closed, appending its `[xN first-last]` summary
/// (only when count>1 — a single hit already wrote its line). Called by the sweeper.
pub fn flush_expired_aggregates() {
    let now = chrono::Local::now();
    let mut guard = DEDUP.lock().unwrap();
    let Some(map) = guard.as_mut() else { return };
    let expired: Vec<String> = map
        .iter()
        .filter(|(_, e)| (now - e.last).num_seconds() >= DEDUP_WINDOW_SECS)
        .map(|(k, _)| k.clone())
        .collect();
    // Collect (summary, date) pairs while holding the lock, then write outside it.
    let to_write: Vec<(String, String)> = expired
        .into_iter()
        .filter_map(|k| {
            let e = map.remove(&k)?;
            if e.count > 1 {
                // count - 1: the first occurrence already has its own on-disk line;
                // the summary carries only the suppressed-repeat count so the viewer's
                // groupLogRecords total = 1 + (count-1) = count = true fires.
                Some((
                    format!(
                        "[x{} {}-{}] {}",
                        e.count - 1,
                        e.first.format("%H:%M:%S"),
                        e.last.format("%H:%M:%S"),
                        e.body
                    ),
                    e.first.format("%Y-%m-%d").to_string(),
                ))
            } else {
                None
            }
        })
        .collect();
    drop(guard);
    for (summary, date) in to_write {
        write_log_line(&date, &summary);
    }
}

/// Spawn the background sweeper (call once from lib.rs setup's deferred task).
pub fn start_log_sweeper() {
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            flush_expired_aggregates();
        }
    });
}

/// Best-effort severity for a raw Pro sidecar stderr line (Approach A -
/// heuristic, since Pro logs unstructured text). Pro keeps its `[pro/<module>]`
/// prefix in the message.
pub(crate) fn infer_pro_level(line: &str) -> &'static str {
    let l = line.to_ascii_lowercase();
    if l.contains("panic") || l.contains("error") {
        "error"
    } else if l.contains("warn") {
        "warn"
    } else {
        "info"
    }
}

#[tauri::command]
pub(crate) fn write_log_record(level: String, message: String) {
    log_message_src(&level, LOG_SRC_UI, &message);
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogRecord {
    date: String,
    timestamp: String,
    level: String,
    source: String,
    message: String,
    #[serde(default)]
    occurrences: u32,
    #[serde(default)]
    first_seen: String,
    #[serde(default)]
    last_seen: String,
}

/// Split an optional `[SRC]` token off the front of the message. Only a
/// recognised source value (`ui`/`core`/`pro`) is consumed - this avoids
/// mistaking a message that merely starts with a bracket (e.g. "[Window Error]
/// ..." or a Pro "[pro/module] ..." prefix) for the source tag. Legacy records,
/// written before source tagging, fall through to `core`.
fn split_source_token(rest: &str) -> (String, &str) {
    let rest = rest.trim_start();
    if let Some(inner) = rest.strip_prefix('[') {
        if let Some((tok, after)) = inner.split_once(']') {
            let tok = tok.trim();
            if matches!(
                tok,
                LOG_SRC_UI
                    | LOG_SRC_CORE
                    | LOG_SRC_PRO
                    | LOG_SRC_INVESTIGATOR
                    | LOG_SRC_FLOWS
                    | LOG_SRC_SERVER_APPS
            ) {
                return (tok.to_string(), after.trim_start());
            }
        }
    }
    (LOG_SRC_CORE.to_string(), rest)
}

/// Try to strip a `[xN HH:MM:SS-HH:MM:SS] ` summary prefix from `msg`.
/// Returns `(occurrences, first_ts, last_ts, stripped_msg)` on match.
fn parse_summary_prefix(msg: &str) -> Option<(u32, &str, &str, &str)> {
    // prefix form: [xN HH:MM:SS-HH:MM:SS] rest
    let inner = msg.strip_prefix('[')?.strip_prefix('x')?;
    let (count_str, rest) = inner.split_once(' ')?;
    let count: u32 = count_str.parse().ok()?;
    let (range, after_bracket) = rest.split_once(']')?;
    let (first_ts, last_ts) = range.split_once('-')?;
    let first_ts = first_ts.trim();
    let last_ts = last_ts.trim();
    // Guard: both tokens must be strict HH:MM:SS so real messages like
    // "[x86 out-of-memory detected]" aren't mis-parsed as summary headers.
    let valid_ts = |s: &str| {
        s.len() == 8
            && s.as_bytes().get(2) == Some(&b':')
            && s.as_bytes().get(5) == Some(&b':')
            && s.bytes()
                .enumerate()
                .all(|(i, b)| i == 2 || i == 5 || b.is_ascii_digit())
    };
    if !valid_ts(first_ts) || !valid_ts(last_ts) {
        return None;
    }
    Some((count, first_ts, last_ts, after_bracket.trim_start()))
}

fn parse_log_body(date: &str, body: &str) -> Option<LogRecord> {
    // body format: [HH:MM:SS] [LEVEL] [SRC]? message
    // message may start with a `[xN HH:MM:SS-HH:MM:SS] ` summary prefix
    let inner = body.strip_prefix('[')?;
    let (ts, rest) = inner.split_once(']')?;
    let rest = rest.trim_start();
    let inner2 = rest.strip_prefix('[')?;
    let (level, rest2) = inner2.split_once(']')?;
    let (source, raw_msg) = split_source_token(rest2);
    let raw_msg = raw_msg.trim_start();
    let ts_str = ts.trim().to_string();
    let date_str = date.to_string();

    // Check for dedup summary prefix
    if let Some((count, first_ts, last_ts, stripped)) = parse_summary_prefix(raw_msg) {
        return Some(LogRecord {
            date: date_str.clone(),
            timestamp: ts_str,
            level: level.trim().to_string(),
            source,
            message: stripped.to_string(),
            occurrences: count,
            first_seen: format!("{} {}", date_str, first_ts),
            last_seen: format!("{} {}", date_str, last_ts),
        });
    }

    Some(LogRecord {
        date: date_str.clone(),
        timestamp: ts_str.clone(),
        level: level.trim().to_string(),
        source,
        message: raw_msg.to_string(),
        occurrences: 1,
        first_seen: format!("{} {}", date_str, ts_str),
        last_seen: format!("{} {}", date_str, ts_str),
    })
}

fn parse_log_body_legacy(line: &str) -> Option<LogRecord> {
    // legacy format: [YYYY-MM-DD HH:MM:SS] [LEVEL] [SRC]? message
    let inner = line.strip_prefix('[')?;
    let (ts_full, rest) = inner.split_once(']')?;
    let (date, time) = ts_full.split_once(' ')?;
    let rest = rest.trim_start();
    let inner2 = rest.strip_prefix('[')?;
    let (level, rest2) = inner2.split_once(']')?;
    let (source, raw_msg) = split_source_token(rest2);
    let raw_msg = raw_msg.trim_start();
    let date_str = date.trim().to_string();
    let time_str = time.trim().to_string();

    // Check for dedup summary prefix
    if let Some((count, first_ts, last_ts, stripped)) = parse_summary_prefix(raw_msg) {
        return Some(LogRecord {
            date: date_str.clone(),
            timestamp: time_str,
            level: level.trim().to_string(),
            source,
            message: stripped.to_string(),
            occurrences: count,
            first_seen: format!("{} {}", date_str, first_ts),
            last_seen: format!("{} {}", date_str, last_ts),
        });
    }

    Some(LogRecord {
        date: date_str.clone(),
        timestamp: time_str.clone(),
        level: level.trim().to_string(),
        source,
        message: raw_msg.to_string(),
        occurrences: 1,
        first_seen: format!("{} {}", date_str, time_str),
        last_seen: format!("{} {}", date_str, time_str),
    })
}

fn parse_log_line(line: &str) -> Option<LogRecord> {
    if line.starts_with("L:") {
        let (date, body) = crate::datastore::log_decrypt_line(line)?;
        parse_log_body(&date, &body)
    } else if line.starts_with('[') {
        parse_log_body_legacy(line)
    } else {
        None
    }
}

fn normalise_levels(levels: Option<&[String]>) -> Option<HashSet<String>> {
    let set: HashSet<String> = levels?
        .iter()
        .filter_map(|level| {
            let level = level.trim();
            if level.is_empty() {
                None
            } else {
                Some(level.to_ascii_uppercase())
            }
        })
        .collect();
    if set.is_empty() {
        None
    } else {
        Some(set)
    }
}

fn record_matches_levels(record: &LogRecord, levels: Option<&HashSet<String>>) -> bool {
    levels
        .map(|accepted| accepted.contains(&record.level.to_ascii_uppercase()))
        .unwrap_or(true)
}

fn is_synthetic_f6_test_failure(message: &str) -> bool {
    message.contains("simulated BitLocker erase failure")
        || message.contains("simulated VeraCrypt destroy failure")
}

fn is_f6_test_cluster_companion(message: &str) -> bool {
    message == "[ProReader] dropping unverifiable frame: envelope was not signed"
        || message
            == "[F6-Orch] BitLocker erase escrow_warning (stage-1 continues): key escrowed to AAD"
}

fn record_cluster_key(record: &LogRecord) -> (String, String) {
    (record.date.clone(), record.timestamp.clone())
}

/// Return log records from newest to oldest, filtering by level before applying
/// the limit so noisy info lines cannot bury warnings/errors off the read window.
pub(crate) fn read_log_records_from_content(
    content: &str,
    limit: Option<usize>,
    levels: Option<&[String]>,
) -> Vec<LogRecord> {
    let max = limit.unwrap_or(usize::MAX);
    if max == 0 {
        return vec![];
    }

    let levels = normalise_levels(levels);
    let mut parsed = Vec::new();
    for line in content.lines().rev().filter(|l| !l.is_empty()) {
        let Some(record) = parse_log_line(line) else {
            continue;
        };
        if !record_matches_levels(&record, levels.as_ref()) {
            continue;
        }
        parsed.push(record);
    }

    let synthetic_f6_keys: HashSet<(String, String)> = parsed
        .iter()
        .filter(|record| is_synthetic_f6_test_failure(&record.message))
        .map(record_cluster_key)
        .collect();

    parsed
        .into_iter()
        .filter(|record| {
            if !synthetic_f6_keys.contains(&record_cluster_key(record)) {
                return true;
            }
            !is_synthetic_f6_test_failure(&record.message)
                && !is_f6_test_cluster_companion(&record.message)
        })
        .take(max)
        .collect()
}

/// Return the last `limit` decrypted log records, newest first.
/// Records from disabled features (investigator off, flows off, server-apps
/// hidden) are filtered out so borrowed-mode observers never see traces of
/// features the owner has turned off.
#[tauri::command]
pub(crate) async fn get_log_records(
    limit: Option<usize>,
    levels: Option<Vec<String>>,
) -> Result<Vec<LogRecord>, String> {
    // A decoy session must never surface the real diagnostic log (deniability):
    // the Error Center reads this directly, bypassing the frontend null-gate.
    if crate::settings::is_decoy_mode() {
        return Ok(vec![]);
    }
    if !logging_enabled() {
        return Ok(vec![]);
    }
    let log_dir = crate::paths::user_logs_dir()?;
    let log_file = log_dir.join("wincommander.log");
    if !log_file.exists() {
        return Ok(vec![]);
    }

    // Compute the set of sources to suppress based on which optional features
    // are currently disabled. Records tagged with a suppressed source are hidden
    // from the viewer — they're still written to disk so nothing is lost, but
    // they don't surface when the feature is turned off.
    let mut blocked: HashSet<&'static str> = HashSet::new();
    if let Ok(s) = crate::settings::read_settings() {
        if !s.ideal.identity.advanced_tools_enabled.unwrap_or(false) {
            blocked.insert(LOG_SRC_INVESTIGATOR);
        }
        if !s.ideal.identity.flows_enabled.unwrap_or(false) {
            blocked.insert(LOG_SRC_FLOWS);
        }
        if s.ideal.identity.hide_server_apps.unwrap_or(false) {
            blocked.insert(LOG_SRC_SERVER_APPS);
        }
    }

    let content =
        std::fs::read_to_string(&log_file).map_err(|e| format!("Failed to read log: {e}"))?;

    let mut records = read_log_records_from_content(&content, limit, levels.as_deref());
    if !blocked.is_empty() {
        records.retain(|r| !blocked.contains(r.source.as_str()));
    }
    Ok(records)
}

/// Wipe the log file content (keeps the file, clears its contents).
#[tauri::command]
pub(crate) fn clear_log_records() -> Result<(), String> {
    // No-op in a decoy session - same anti-coercion backstop as settings writes.
    if crate::settings::is_decoy_mode() {
        return Ok(());
    }
    let log_dir = crate::paths::user_logs_dir()?;
    let log_file = log_dir.join("wincommander.log");
    let result = std::fs::write(&log_file, b"").map_err(|e| format!("Failed to clear log: {e}"));
    // Tamper hook: best-effort, spawn so we never block or panic.
    // Forwards a generic event label only — no AV-flagged strings here.
    // Pro side (tamper_monitor::record_event) validates and enqueues.
    {
        let hook_args = serde_json::json!({ "signal": "log_cleared" });
        tauri::async_runtime::spawn(async move {
            crate::argus::record_tamper_event_hook(hook_args).await;
        });
    }
    result
}

#[cfg(test)]
mod dedup_tests {
    use super::*;
    #[test]
    fn normalize_collapses_long_digit_runs_not_short() {
        let a = normalize_key("error", "core", "conn failed to 10.0.0.5:443");
        let b = normalize_key("error", "core", "conn failed to 10.0.0.9:443");
        assert_eq!(a, b, "IPs should collapse to the same key");
        let c = normalize_key("error", "core", "code 12");
        assert!(c.contains("12"), "short digit runs preserved");
    }
}

#[cfg(test)]
mod log_record_tests {
    use super::*;

    #[test]
    fn test_harness_log_messages_do_not_write_to_user_log() {
        let temp = tempfile::tempdir().unwrap();
        let old_local_app_data = std::env::var_os("LOCALAPPDATA");
        std::env::set_var("LOCALAPPDATA", temp.path());
        LOGGING_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);

        log_message_src(
            "warn",
            LOG_SRC_CORE,
            "[TestHarness] this record must stay out of the user log",
        );

        if let Some(old) = old_local_app_data {
            std::env::set_var("LOCALAPPDATA", old);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }

        let log_file = temp
            .path()
            .join("WinCommander")
            .join("logs")
            .join("wincommander.log");
        assert!(
            !log_file.exists(),
            "Rust tests must not append records to the app Error Center log"
        );
    }

    #[test]
    fn parse_body_with_source_token() {
        // Pro line: a [pro] source token precedes the message's own [pro/module] prefix.
        let r = parse_log_body(
            "2026-06-16",
            "[12:00:01] [ERROR] [pro] [pro/wifi_guard] netsh failed",
        )
        .unwrap();
        assert_eq!(r.timestamp, "12:00:01");
        assert_eq!(r.level, "ERROR");
        assert_eq!(r.source, "pro");
        assert_eq!(r.message, "[pro/wifi_guard] netsh failed");
    }

    #[test]
    fn parse_body_message_starting_with_non_source_bracket_is_core() {
        // No [SRC] token; the message itself opens with a bracket that is NOT a
        // recognised source - must not be mistaken for the source tag.
        let r = parse_log_body("2026-06-16", "[12:00:02] [WARN] [Window Error] boom").unwrap();
        assert_eq!(r.level, "WARN");
        assert_eq!(r.source, "core");
        assert_eq!(r.message, "[Window Error] boom");
    }

    #[test]
    fn parse_body_ui_source() {
        let r = parse_log_body("2026-06-16", "[12:00:03] [INFO] [ui] app started").unwrap();
        assert_eq!(r.source, "ui");
        assert_eq!(r.message, "app started");
    }

    #[test]
    fn legacy_full_timestamp_defaults_core() {
        let r = parse_log_body_legacy("[2026-06-16 12:00:04] [INFO] hello world").unwrap();
        assert_eq!(r.date, "2026-06-16");
        assert_eq!(r.timestamp, "12:00:04");
        assert_eq!(r.source, "core");
        assert_eq!(r.message, "hello world");
    }

    #[test]
    fn pro_level_inference() {
        assert_eq!(
            infer_pro_level("thread 'main' panicked at src/main.rs:1"),
            "error"
        );
        assert_eq!(infer_pro_level("[pro] error: pipe closed"), "error");
        assert_eq!(
            infer_pro_level("[pro/wifi_guard] WARN adapter busy"),
            "warn"
        );
        assert_eq!(infer_pro_level("[pro/wifi_guard] detector started"), "info");
    }

    #[test]
    fn level_filter_is_applied_before_limit() {
        let mut content = String::new();
        content.push_str("[2026-06-16 12:00:00] [ERROR] [pro] real failure\n");
        for i in 0..500 {
            content.push_str(&format!(
                "[2026-06-16 12:{:02}:{:02}] [INFO] [pro] dispatch {}\n",
                1 + (i / 60),
                i % 60,
                i
            ));
        }

        let levels = vec!["error".to_string(), "warn".to_string()];
        let records = read_log_records_from_content(&content, Some(500), Some(&levels));

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, "ERROR");
        assert_eq!(records[0].message, "real failure");
    }

    #[test]
    fn synthetic_test_clusters_are_hidden_without_hiding_real_security_warnings() {
        let content = "\
[2026-07-05 13:39:40] [WARN] [core] [ProReader] dropping unverifiable frame: envelope was not signed
[2026-07-05 13:39:40] [WARN] [core] [F6-Orch] BitLocker erase escrow_warning (stage-1 continues): key escrowed to AAD
[2026-07-05 13:39:40] [ERROR] [core] [F6-Orch] stage-1 FAILED at BitLocker erase — aborting (no reboot): simulated BitLocker erase failure
[2026-07-05 14:00:00] [WARN] [core] [ProReader] dropping unverifiable frame: envelope was not signed
[2026-07-05 14:01:00] [WARN] [core] [F6-Orch] BitLocker erase escrow_warning (stage-1 continues): key escrowed to AAD
[2026-07-05 14:02:00] [ERROR] [core] real failure
";

        let levels = vec!["error".to_string(), "warn".to_string()];
        let records = read_log_records_from_content(content, Some(10), Some(&levels));
        let messages: Vec<&str> = records
            .iter()
            .map(|record| record.message.as_str())
            .collect();

        assert_eq!(messages.len(), 3);
        assert!(messages.contains(&"real failure"));
        assert!(
            messages.contains(&"[ProReader] dropping unverifiable frame: envelope was not signed")
        );
        assert!(messages.contains(
            &"[F6-Orch] BitLocker erase escrow_warning (stage-1 continues): key escrowed to AAD"
        ));
        assert!(messages
            .iter()
            .all(|message| !message.contains("simulated BitLocker erase failure")));
    }
}
