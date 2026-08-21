// SPDX-License-Identifier: AGPL-3.0-or-later
//! Query-filter-syntax parser: pulls `ext:`, `after:`, `before:`, and `size:`
//! tokens out of a raw search query, leaving everything else as free text.
//!
//! This is a search box, not a command line: an unrecognized or malformed
//! filter-shaped token (bad date, bad unit, empty value) is left in the text
//! query verbatim — never dropped, never an error. A later phase feeds the
//! returned `QueryFilters` into the tantivy query path.

/// Structured filters pulled out of a raw query.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueryFilters {
    /// Lowercased, no leading dots.
    pub exts: Vec<String>,
    /// Unix seconds, inclusive lower bound on mtime.
    pub after: Option<u64>,
    /// Unix seconds, exclusive upper bound on mtime.
    pub before: Option<u64>,
    /// Bytes, inclusive lower bound on size.
    pub size_min: Option<u64>,
    /// Bytes, exclusive upper bound on size.
    pub size_max: Option<u64>,
}

impl QueryFilters {
    pub fn is_empty(&self) -> bool {
        self.exts.is_empty()
            && self.after.is_none()
            && self.before.is_none()
            && self.size_min.is_none()
            && self.size_max.is_none()
    }
}

/// Split `raw` into (remaining text query, filters).
///
/// `after:` is start-inclusive of the named period, `before:` is
/// start-exclusive — both anchor to the START instant of the year/month/day
/// named, so `before:2026` excludes all of 2026 (< 2026-01-01T00:00:00Z)
/// while `after:2026` includes it (>= 2026-01-01T00:00:00Z).
pub fn parse_filters(raw: &str) -> (String, QueryFilters) {
    let mut filters = QueryFilters::default();
    let mut remaining: Vec<&str> = Vec::new();

    for token in raw.split_whitespace() {
        if let Some(rest) = strip_key(token, "ext:") {
            match parse_ext(rest) {
                Some(mut exts) => filters.exts.append(&mut exts),
                None => remaining.push(token),
            }
        } else if let Some(rest) = strip_key(token, "after:") {
            match parse_date(rest) {
                Some(secs) => filters.after = Some(secs),
                None => remaining.push(token),
            }
        } else if let Some(rest) = strip_key(token, "before:") {
            match parse_date(rest) {
                Some(secs) => filters.before = Some(secs),
                None => remaining.push(token),
            }
        } else if let Some(rest) = strip_key(token, "size:") {
            match parse_size(rest) {
                Some((min, max)) => {
                    if min.is_some() {
                        filters.size_min = min;
                    }
                    if max.is_some() {
                        filters.size_max = max;
                    }
                }
                None => remaining.push(token),
            }
        } else {
            remaining.push(token);
        }
    }

    (remaining.join(" "), filters)
}

/// Case-insensitive `key:` prefix strip; returns the (still-cased) value.
/// Uses `get()` rather than `split_at()` — a non-matching token may not have
/// a char boundary at `key.len()` and `split_at` would panic on it.
fn strip_key<'a>(token: &'a str, key: &str) -> Option<&'a str> {
    let head = token.get(..key.len())?;
    if head.eq_ignore_ascii_case(key) {
        token.get(key.len()..)
    } else {
        None
    }
}

fn parse_ext(value: &str) -> Option<Vec<String>> {
    if value.is_empty() {
        return None;
    }
    let mut exts = Vec::new();
    for part in value.split(',') {
        let trimmed = part.trim_start_matches('.');
        if trimmed.is_empty() {
            return None;
        }
        exts.push(trimmed.to_ascii_lowercase());
    }
    Some(exts)
}

/// Accepts `YYYY`, `YYYY-MM`, `YYYY-MM-DD`; missing month/day default to 1.
fn parse_date(value: &str) -> Option<u64> {
    let mut parts = value.splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = match parts.next() {
        Some(s) => s.parse().ok()?,
        None => 1,
    };
    let d: u32 = match parts.next() {
        Some(s) => s.parse().ok()?,
        None => 1,
    };
    if !(1..=12).contains(&m) {
        return None;
    }
    if d < 1 || d > days_in_month(y, m) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    // Reject pre-epoch dates rather than let the cast wrap to a huge u64.
    u64::try_from(days).ok().map(|d| d * 86_400)
}

fn is_leap_year(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0, // caller already validated m in 1..=12
    }
}

/// Howard Hinnant's days-from-civil algorithm (public domain), returning days
/// since the Unix epoch (1970-01-01), possibly negative for earlier dates.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (i64::from(m) + 9) % 12; // Mar=0 .. Jan=10, Feb=11
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

enum SizeOp {
    Gt,
    Ge,
    Lt,
    Le,
}

/// Returns `(size_min, size_max)` — exactly one side is `Some`.
fn parse_size(value: &str) -> Option<(Option<u64>, Option<u64>)> {
    let (op, rest) = if let Some(r) = value.strip_prefix(">=") {
        (SizeOp::Ge, r)
    } else if let Some(r) = value.strip_prefix("<=") {
        (SizeOp::Le, r)
    } else if let Some(r) = value.strip_prefix('>') {
        (SizeOp::Gt, r)
    } else {
        let r = value.strip_prefix('<')?;
        (SizeOp::Lt, r)
    };
    let bytes = parse_bytes(rest)?.round() as u64;
    match op {
        // '>'/'<=' shift by one byte to keep the stored bound's inclusive
        // (min) / exclusive (max) meaning consistent for both operators.
        SizeOp::Ge => Some((Some(bytes), None)),
        SizeOp::Gt => Some((Some(bytes.checked_add(1)?), None)),
        SizeOp::Le => Some((None, Some(bytes.checked_add(1)?))),
        SizeOp::Lt => Some((None, Some(bytes))),
    }
}

/// Bare number = bytes; `b`/`kb`/`mb`/`gb` suffix (case-insensitive, binary
/// 1024 multiples). Longer suffixes are checked first so `kb` isn't
/// mistaken for a bare `b` suffix.
fn parse_bytes(value: &str) -> Option<f64> {
    let lower = value.to_ascii_lowercase();
    let (num_str, mult) = if let Some(n) = lower.strip_suffix("gb") {
        (n, 1024f64.powi(3))
    } else if let Some(n) = lower.strip_suffix("mb") {
        (n, 1024f64.powi(2))
    } else if let Some(n) = lower.strip_suffix("kb") {
        (n, 1024f64)
    } else if let Some(n) = lower.strip_suffix('b') {
        (n, 1.0)
    } else {
        (lower.as_str(), 1.0)
    };
    let n: f64 = num_str.parse().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    Some(n * mult)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── passthrough ──────────────────────────────────────────────────────

    #[test]
    fn plain_query_passes_through_unchanged() {
        let (text, filters) = parse_filters("budget report 2025");
        assert_eq!(text, "budget report 2025");
        assert!(filters.is_empty());
    }

    // ── ext ──────────────────────────────────────────────────────────────

    #[test]
    fn ext_single() {
        let (text, filters) = parse_filters("ext:pdf");
        assert_eq!(text, "");
        assert_eq!(filters.exts, vec!["pdf"]);
    }

    #[test]
    fn ext_comma_list_and_repeated_tokens_accumulate_lowercased_and_stripped() {
        let (text, filters) = parse_filters("ext:PDF,.docx ext:txt");
        assert_eq!(text, "");
        assert_eq!(filters.exts, vec!["pdf", "docx", "txt"]);
    }

    #[test]
    fn ext_empty_value_stays_in_text() {
        let (text, filters) = parse_filters("ext:");
        assert_eq!(text, "ext:");
        assert!(filters.exts.is_empty());
    }

    // ── dates ────────────────────────────────────────────────────────────

    #[test]
    fn after_year_only() {
        let (text, filters) = parse_filters("after:1970");
        assert_eq!(text, "");
        assert_eq!(filters.after, Some(0));
    }

    #[test]
    fn after_date_vectors() {
        // Independently derived via Hinnant's days-from-civil:
        // 2026-01-01 is 20454 days after epoch (56y incl. 14 leap years:
        // 1972..=2024 step 4) -> 20454 * 86400 = 1_767_225_600.
        assert_eq!(parse_date("2026"), Some(1_767_225_600));
        // + Jan(31)+Feb(28)+Mar(31)+Apr(30)+May(31)+Jun(30) = 181 days.
        assert_eq!(parse_date("2026-07"), Some(1_782_864_000));
        // + 8 more days to reach the 9th.
        assert_eq!(parse_date("2026-07-09"), Some(1_783_555_200));
    }

    #[test]
    fn before_is_start_exclusive_of_named_period() {
        let (_, filters) = parse_filters("before:2026");
        assert_eq!(filters.before, Some(1_767_225_600));
    }

    #[test]
    fn leap_year_feb29_accepted_2024_rejected_2025() {
        assert!(parse_date("2024-02-29").is_some());
        assert!(parse_date("2025-02-29").is_none());
    }

    #[test]
    fn malformed_date_stays_in_text() {
        let (text, filters) = parse_filters("after:soon");
        assert_eq!(text, "after:soon");
        assert!(filters.after.is_none());
    }

    #[test]
    fn uppercase_key_parses() {
        let (text, filters) = parse_filters("AFTER:2026");
        assert_eq!(text, "");
        assert_eq!(filters.after, Some(1_767_225_600));
    }

    // ── size ─────────────────────────────────────────────────────────────

    #[test]
    fn size_gt_mb_is_min_inclusive_plus_one() {
        let (_, filters) = parse_filters("size:>10mb");
        assert_eq!(filters.size_min, Some(10 * 1024 * 1024 + 1));
        assert!(filters.size_max.is_none());
    }

    #[test]
    fn size_ge_mb_is_min_exact() {
        let (_, filters) = parse_filters("size:>=10mb");
        assert_eq!(filters.size_min, Some(10 * 1024 * 1024));
    }

    #[test]
    fn size_lt_gb_decimal_is_max_exact() {
        let (_, filters) = parse_filters("size:<1.5gb");
        assert_eq!(filters.size_max, Some(1_610_612_736));
    }

    #[test]
    fn size_le_bare_bytes_is_max_plus_one() {
        let (_, filters) = parse_filters("size:<=2048");
        assert_eq!(filters.size_max, Some(2049));
    }

    #[test]
    fn size_gt_bare_bytes() {
        let (_, filters) = parse_filters("size:>1000");
        assert_eq!(filters.size_min, Some(1001));
    }

    #[test]
    fn size_bad_unit_stays_in_text() {
        let (text, filters) = parse_filters("size:>10xb");
        assert_eq!(text, "size:>10xb");
        assert!(filters.size_min.is_none());
    }

    // ── combined + ordering ──────────────────────────────────────────────

    #[test]
    fn combined_filters_and_remaining_text() {
        let (text, filters) = parse_filters("budget ext:pdf after:2026-01 size:>10mb");
        assert_eq!(text, "budget");
        assert_eq!(filters.exts, vec!["pdf"]);
        // 2026-01-01, not 2026-07 — see after_date_vectors for the derivation.
        assert_eq!(filters.after, Some(1_767_225_600));
        assert_eq!(filters.size_min, Some(10 * 1024 * 1024 + 1));
    }

    #[test]
    fn remaining_text_preserves_token_order() {
        let (text, _) = parse_filters("alpha ext:pdf beta");
        assert_eq!(text, "alpha beta");
    }
}
