// src/advisor.rs
//
// ═══════════════════════════════════════════════════════════════════════
// AI SECURITY ADVISOR — Free-side context assembler (spec 13 / #10)
// ═══════════════════════════════════════════════════════════════════════
//
// Free OWNS the machine's data; Pro only runs the local LLM. This module
// is the thin, FREE context-assembler: it reads signals Free already
// computes (drift report, rolling log tail, active ports, public-IP
// trace), bounds them to a token budget, and hands a compact JSON
// `context` object to the frontend. The FE stringifies it and forwards
// it to the PAID `Llm-Analyze` Pro handler.
//
// This command is FREE (it only reads data Free already exposes) and is
// invoked directly like `get_drift_report` — it is NOT registered as
// paid in `get_command_tier`. Only the model-calling Pro handlers
// (Get-OllamaStatus / Pull-OllamaModel / Llm-Analyze) are paid.
//
// Nothing here reaches the network except the public-IP trace, which is
// the same Cloudflare-trace lookup the rest of the app already performs
// (DoH-aware client). The LLM analysis path itself is 100% localhost
// (Pro → 127.0.0.1:11434).
//
// PORTS NOTE: `Get-NetworkPorts` is an admin-gated PowerShell module, not
// a plain Rust read. Rather than have this free command shell out (which
// would need admin + the encrypted-module path), the frontend passes the
// rows it already fetched via `getNetworkPorts()` into the optional
// `ports` arg. advisor.rs filters + caps them. If `ports` is absent the
// connection context is simply empty and the model is told so.

use serde_json::{json, Value};

/// Whole-context hard cap (~8 KB ≈ 2k tokens) — comfortably inside a 3B
/// model's window while leaving room for the response. Keeps CPU-only
/// latency predictable.
const CONTEXT_BUDGET_BYTES: usize = 8 * 1024;
/// Max drift items embedded before we summarise "+N more".
const MAX_DRIFT_ITEMS: usize = 40;
/// Max connection rows embedded for `explain-connection`.
const MAX_PORT_ROWS: usize = 30;
/// Log tail bounds — last ~150 lines / ~6 KB, whichever is smaller.
const MAX_LOG_LINES: usize = 150;
const MAX_LOG_BYTES: usize = 6 * 1024;
/// Fallback home jurisdiction used only when the OS locale hasn't been
/// probed yet (fresh install, probe not yet run). `isForeignJurisdiction`
/// is a heuristic flag derived from `resolve_home_country()`, not this
/// constant directly; clearly labelled as a heuristic to the model.
const DEFAULT_HOME_COUNTRY: &str = "IN";

/// Trim a drift item to `{path, ideal, current}` (drop `command`, which
/// the model must never echo as something to run).
fn slim_drift_item(item: &Value) -> Value {
    json!({
        "path": item.get("path").cloned().unwrap_or(Value::Null),
        "ideal": item.get("idealValue").cloned().unwrap_or(Value::Null),
        "current": item.get("currentValue").cloned().unwrap_or(Value::Null),
    })
}

/// True for security-relevant drift paths — sorted first within the cap
/// for `suggest-hardening`.
fn is_security_path(path: &str) -> bool {
    path.starts_with("privacy.")
        || path.starts_with("tweaks.security.")
        || path.starts_with("network.")
}

/// Read the drift report and return (slimmed items capped, total count,
/// high-risk count). High-risk = security-relevant path.
fn collect_drift(sort_security_first: bool) -> (Vec<Value>, usize, usize) {
    let drifts: Vec<Value> = crate::settings::get_drift_report()
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let total = drifts.len();
    let high_risk = drifts
        .iter()
        .filter(|d| {
            d.get("path")
                .and_then(Value::as_str)
                .map(is_security_path)
                .unwrap_or(false)
        })
        .count();

    let mut ordered = drifts;
    if sort_security_first {
        ordered.sort_by_key(|d| {
            let sec = d
                .get("path")
                .and_then(Value::as_str)
                .map(is_security_path)
                .unwrap_or(false);
            // false (0) sorts before true (1); we want security first, so
            // invert.
            u8::from(!sec)
        });
    }

    let slim: Vec<Value> = ordered
        .iter()
        .take(MAX_DRIFT_ITEMS)
        .map(slim_drift_item)
        .collect();
    (slim, total, high_risk)
}

/// Tail the rolling application log — last ~150 lines / ~6 KB. Strips the
/// date portion of each timestamp (keeps the time) to save tokens. Never
/// reads the whole file.
fn collect_log_tail() -> String {
    let path = match crate::paths::user_logs_dir() {
        Ok(dir) => dir.join("wincommander.log"),
        Err(_) => return String::new(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(MAX_LOG_LINES);
    let tail: Vec<String> = lines[start..]
        .iter()
        .map(|line| strip_log_date(line))
        .collect();
    let mut joined = tail.join("\n");

    // Byte cap (keep the most recent bytes — the end of the string).
    if joined.len() > MAX_LOG_BYTES {
        let cut = joined.len() - MAX_LOG_BYTES;
        // Snap to a char boundary so we don't slice mid-UTF-8.
        let mut idx = cut;
        while idx < joined.len() && !joined.is_char_boundary(idx) {
            idx += 1;
        }
        joined = format!("...[earlier lines truncated]\n{}", &joined[idx..]);
    }
    joined
}

/// `[2026-06-01 14:03:22] [INFO] ...` → `[14:03:22] [INFO] ...`. Best
/// effort — leaves the line untouched if the shape doesn't match.
fn strip_log_date(line: &str) -> String {
    // Expect a leading `[YYYY-MM-DD HH:MM:SS]`.
    if let Some(rest) = line.strip_prefix('[') {
        if let Some(space) = rest.find(' ') {
            if let Some(close) = rest.find(']') {
                if space < close && space == 10 {
                    // rest[..10] is the date, rest[11..] is "HH:MM:SS]...".
                    return format!("[{}", &rest[space + 1..]);
                }
            }
        }
    }
    line.to_string()
}

/// Filter raw `getNetworkPorts()` rows to non-loopback / non-local, prefer
/// Established + Listen, cap, and slim to `{proto, remoteAddr, remotePort,
/// state, processName}`.
fn collect_ports(ports: Option<&Value>) -> Vec<Value> {
    let rows = match ports.and_then(|p| p.get("rows")).and_then(Value::as_array) {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut filtered: Vec<&Value> = rows
        .iter()
        .filter(|row| {
            let remote = row.get("remoteAddr").and_then(Value::as_str).unwrap_or("");
            !is_local_addr(remote)
        })
        .collect();

    // Prefer Established + Listen by sorting them first.
    filtered.sort_by_key(|row| {
        let state = row.get("state").and_then(Value::as_str).unwrap_or("");
        match state {
            "Established" => 0u8,
            "Listen" => 1,
            _ => 2,
        }
    });

    filtered
        .into_iter()
        .take(MAX_PORT_ROWS)
        .map(|row| {
            json!({
                "proto": row.get("proto").cloned().unwrap_or(Value::Null),
                "remoteAddr": row.get("remoteAddr").cloned().unwrap_or(Value::Null),
                "remotePort": row.get("remotePort").cloned().unwrap_or(Value::Null),
                "state": row.get("state").cloned().unwrap_or(Value::Null),
                "processName": row.get("processName").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// Parse the 2-letter ISO 3166-1 region subtag from a BCP-47 locale name
/// (e.g. "en-IN" -> `Some("IN")`, "en-US" -> `Some("US")`). Locales whose
/// trailing subtag isn't exactly 2 ASCII letters (UN M49 area codes like
/// "es-419", or a bare language tag with no region) return `None` rather
/// than risk treating a non-country code as one.
fn region_from_locale(locale: &str) -> Option<String> {
    let region = locale.rsplit('-').next()?;
    if region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(region.to_ascii_uppercase())
    } else {
        None
    }
}

/// Resolve the "home" jurisdiction from the OS-reported system locale
/// (`current.device.systemLocale`, populated by the settings-bridge probe
/// via `Get-WinSystemLocale`, e.g. "en-IN") instead of a single hardcoded
/// country. Falls back to `DEFAULT_HOME_COUNTRY` when the locale hasn't
/// been probed yet or doesn't carry a parseable region subtag, so the
/// "foreign jurisdiction" signal stays meaningful for non-Indian customers
/// too.
fn resolve_home_country() -> String {
    crate::settings::get_setting("current.device.systemLocale".to_string())
        .ok()
        .and_then(|v| v.as_str().and_then(region_from_locale))
        .unwrap_or_else(|| DEFAULT_HOME_COUNTRY.to_string())
}

/// Loopback / unspecified / empty addresses we never want the model to
/// reason about as "external connections".
fn is_local_addr(addr: &str) -> bool {
    let a = addr.trim();
    a.is_empty()
        || a == "127.0.0.1"
        || a == "::1"
        || a == "0.0.0.0"
        || a == "::"
        || a == "*"
        || a.starts_with("127.")
        || a.starts_with("::ffff:127.")
        || a.starts_with("fe80:") // link-local
}

/// Fetch the public-IP trace via the same DoH-aware client the rest of
/// the app uses. Best-effort: returns `(ip, country, colo, source)` with
/// `None`s on failure (the advisor copes with missing data).
async fn fetch_ip_trace() -> (Option<String>, Option<String>, Option<String>, String) {
    let client = match crate::net::doh_http_client() {
        Ok(c) => c,
        Err(_) => return (None, None, None, "unavailable".to_string()),
    };
    let raw = client
        .get("https://cloudflare.com/cdn-cgi/trace")
        .send()
        .await
        .ok();
    let text = match raw {
        Some(r) if r.status().is_success() => r.text().await.ok(),
        _ => None,
    };
    let Some(text) = text else {
        return (None, None, None, "unavailable".to_string());
    };

    let (mut ip, mut country, mut colo) = (None, None, None);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim();
            match k {
                "ip" if !v.is_empty() => ip = Some(v.to_string()),
                "loc" if !v.is_empty() => country = Some(v.to_string()),
                "colo" if !v.is_empty() => colo = Some(v.to_string()),
                _ => {}
            }
        }
    }
    (ip, country, colo, "cloudflare-trace".to_string())
}

/// Enforce the whole-context byte budget with a hard truncation marker.
fn enforce_budget(context: Value) -> Value {
    let s = context.to_string();
    if s.len() <= CONTEXT_BUDGET_BYTES {
        return context;
    }
    // Too big even after per-task selection — hand back a string with a
    // truncation marker (Pro embeds the context verbatim, so a JSON
    // string is fine for the model to read).
    let mut idx = CONTEXT_BUDGET_BYTES;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    json!({ "truncated": true, "raw": format!("{}...[truncated]", &s[..idx]) })
}

/// Build the bounded context for a task. FREE command — reads only data
/// Free already exposes. `ports` is the optional rows object the frontend
/// already fetched via `getNetworkPorts()` (see PORTS NOTE above).
#[tauri::command]
pub async fn advisor_build_context(task: String, ports: Option<Value>) -> Result<Value, String> {
    let context = match task.as_str() {
        "explain-risks" => {
            let (items, total, _high) = collect_drift(false);
            let extra = total.saturating_sub(items.len());
            json!({ "drift": { "items": items, "total": total, "omitted": extra } })
        }
        "suggest-hardening" => {
            let (items, total, _high) = collect_drift(true);
            let extra = total.saturating_sub(items.len());
            json!({ "drift": { "items": items, "total": total, "omitted": extra } })
        }
        "summarize-logs" => {
            let tail = collect_log_tail();
            json!({ "logTail": tail })
        }
        "detect-anomalies" => {
            let (items, total, high) = collect_drift(true);
            let extra = total.saturating_sub(items.len());
            let (ip, country, colo, source) = fetch_ip_trace().await;
            let _ = (ip, colo); // ip/colo not needed for anomaly signals
            let home_country = resolve_home_country();
            let is_foreign = country
                .as_deref()
                .map(|c| c != home_country)
                .unwrap_or(false);
            json!({
                "drift": { "items": items, "total": total, "omitted": extra },
                // Recent security-monitor / subsystem events from the evidence
                // ledger so the advisor can triage them alongside drift (A8).
                "recentEvents": crate::evidence::evidence_read(Some(20)).unwrap_or_default(),
                "signals": {
                    "publicIpCountry": country,
                    "ipSource": source,
                    "homeCountry": home_country,
                    "isForeignJurisdiction": is_foreign,
                    "isForeignJurisdictionIsHeuristic": true,
                    "driftCount": total,
                    "highRiskDriftCount": high,
                }
            })
        }
        "explain-connection" => {
            let rows = collect_ports(ports.as_ref());
            let (ip, country, colo, source) = fetch_ip_trace().await;
            json!({
                "ports": rows,
                "ipTrace": { "ip": ip, "country": country, "colo": colo, "source": source }
            })
        }
        other => return Err(format!("unknown advisor task: {}", other)),
    };

    Ok(enforce_budget(context))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_addrs_are_dropped() {
        assert!(is_local_addr("127.0.0.1"));
        assert!(is_local_addr("::1"));
        assert!(is_local_addr("0.0.0.0"));
        assert!(is_local_addr(""));
        assert!(is_local_addr("127.0.0.53"));
        assert!(!is_local_addr("8.8.8.8"));
        assert!(!is_local_addr("140.82.121.4"));
    }

    #[test]
    fn ports_filter_drops_loopback_and_caps() {
        let ports = json!({
            "rows": [
                { "proto": "TCP", "remoteAddr": "127.0.0.1", "remotePort": 5000, "state": "Established", "processName": "loopback" },
                { "proto": "TCP", "remoteAddr": "8.8.8.8", "remotePort": 443, "state": "Established", "processName": "chrome" },
                { "proto": "TCP", "remoteAddr": "0.0.0.0", "remotePort": 135, "state": "Listen", "processName": "svchost" }
            ]
        });
        let rows = collect_ports(Some(&ports));
        // Only the 8.8.8.8 row survives the non-loopback filter.
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("remoteAddr").unwrap(), "8.8.8.8");
    }

    #[test]
    fn budget_truncation_marks_oversized_context() {
        let big = "x".repeat(CONTEXT_BUDGET_BYTES * 2);
        let ctx = json!({ "logTail": big });
        let out = enforce_budget(ctx);
        assert_eq!(out.get("truncated").and_then(Value::as_bool), Some(true));
        assert!(out
            .get("raw")
            .and_then(Value::as_str)
            .unwrap()
            .ends_with("...[truncated]"));
    }

    #[test]
    fn region_from_locale_parses_two_letter_region() {
        assert_eq!(region_from_locale("en-US"), Some("US".to_string()));
        assert_eq!(region_from_locale("en-IN"), Some("IN".to_string()));
        assert_eq!(region_from_locale("fr-CA"), Some("CA".to_string()));
        // Script-tagged locale still ends in a valid 2-letter region.
        assert_eq!(region_from_locale("zh-Hans-CN"), Some("CN".to_string()));
        // Region subtag is case-normalized to uppercase.
        assert_eq!(region_from_locale("en-us"), Some("US".to_string()));
    }

    #[test]
    fn region_from_locale_rejects_unparseable_subtags() {
        // UN M49 numeric area code, not an ISO country.
        assert_eq!(region_from_locale("es-419"), None);
        // No region subtag at all.
        assert_eq!(region_from_locale("plain"), None);
        assert_eq!(region_from_locale(""), None);
    }

    #[test]
    fn strip_log_date_keeps_time() {
        let line = "[2026-06-01 14:03:22] [INFO] something happened";
        assert_eq!(strip_log_date(line), "[14:03:22] [INFO] something happened");
        // Non-matching lines are untouched.
        assert_eq!(strip_log_date("plain line"), "plain line");
    }
}
