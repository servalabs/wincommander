//! Small shared utilities (time formatting). Deliberately dependency-free
//! (no `time`/`chrono` crate) — this crate's dependency footprint is a
//! `types`-feature-gate concern, so we keep even the transport-only helpers
//! minimal.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current UTC time as unix seconds.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Current UTC time as an RFC3339 string (`Z` suffix, second precision), for
/// stamping trigger-source records.
pub fn now_rfc3339() -> String {
    unix_secs_to_rfc3339(now_unix())
}

/// Convert unix seconds to an RFC3339 UTC string (`YYYY-MM-DDTHH:MM:SSZ`).
///
/// Hand-rolled (no `time`/`chrono` dependency): a proleptic-Gregorian civil-date
/// conversion from days-since-epoch. Valid for any post-1970 timestamp, which is
/// the only range this crate ever formats.
pub fn unix_secs_to_rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;

    // Howard Hinnant's days-from-civil / civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_formats_correctly() {
        assert_eq!(unix_secs_to_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_timestamp_formats_correctly() {
        // 2023-11-14T22:13:20Z
        assert_eq!(unix_secs_to_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn now_unix_is_positive_and_recent() {
        let now = now_unix();
        assert!(now > 1_700_000_000, "now_unix should be well after 2023");
    }

    #[test]
    fn now_rfc3339_ends_with_z() {
        assert!(now_rfc3339().ends_with('Z'));
    }
}
