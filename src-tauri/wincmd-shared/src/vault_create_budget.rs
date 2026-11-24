// SPDX-License-Identifier: AGPL-3.0-or-later
//! One formula for how long a Vault creation may run. The service and Pro
//! helper wait `service_budget`; the desktop app waits `desktop_timeout`, which
//! is always longer so the service reports its own timeout first.
use std::time::Duration;

const BASE_SECS: u64 = 120;
const MAX_SECS: u64 = 12 * 60 * 60;
const DESKTOP_MARGIN_SECS: u64 = 120;

/// 120 s plus 1 s per 10 MiB of outer size (a floor of about 10 MiB/s), at most 12 h.
pub fn service_budget(size_mib: u64) -> Duration {
    Duration::from_secs(
        BASE_SECS
            .saturating_add(size_mib.div_ceil(10))
            .clamp(BASE_SECS, MAX_SECS),
    )
}

pub fn desktop_timeout(size_mib: u64) -> Duration {
    service_budget(size_mib) + Duration::from_secs(DESKTOP_MARGIN_SECS)
}

/// Outer size in MiB from a create request. A hidden size must never drive the
/// wait, and a device target takes its size from the partition.
pub fn size_mib_from_args(args: &serde_json::Value) -> u64 {
    fn number(args: &serde_json::Value, key: &str) -> u64 {
        match args.get(key) {
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
            Some(serde_json::Value::String(s)) => s.trim().parse().unwrap_or(0),
            _ => 0,
        }
    }
    [number(args, "HostSizeMB"), number(args, "SizeMB")]
        .into_iter()
        .find(|mib| *mib > 0)
        .unwrap_or_else(|| number(args, "DeviceSizeBytes").div_ceil(1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_scales_with_size_within_the_contract_bounds() {
        assert_eq!(service_budget(51_200), Duration::from_secs(5_240));
        assert_eq!(desktop_timeout(51_200), Duration::from_secs(5_360));
        assert_eq!(service_budget(500), Duration::from_secs(170));
        assert_eq!(desktop_timeout(500), Duration::from_secs(290));
        for tiny in [0, 1, 10] {
            assert_eq!(service_budget(tiny), Duration::from_secs(if tiny == 0 { 120 } else { 121 }));
            assert!(desktop_timeout(tiny) >= Duration::from_secs(240));
        }
        assert_eq!(service_budget(u64::MAX), Duration::from_secs(43_200));
        assert_eq!(desktop_timeout(u64::MAX), Duration::from_secs(43_320));
    }

    #[test]
    fn desktop_always_outlasts_the_service() {
        for mib in [0, 1, 500, 51_200, 10_000_000, u64::MAX] {
            assert_eq!(desktop_timeout(mib) - service_budget(mib), Duration::from_secs(120));
        }
    }

    #[test]
    fn size_comes_from_the_outer_volume_only() {
        assert_eq!(size_mib_from_args(&serde_json::json!({"HostSizeMB": 500, "SecondSizeMB": 51200})), 500);
        assert_eq!(size_mib_from_args(&serde_json::json!({"HostSizeMB": 64, "SizeMB": 32})), 64);
        assert_eq!(size_mib_from_args(&serde_json::json!({"SizeMB": "500"})), 500);
        assert_eq!(
            size_mib_from_args(&serde_json::json!({"SizeMB": 0, "DeviceSizeBytes": 500u64 * 1024 * 1024})),
            500
        );
    }

    #[test]
    fn unreadable_size_is_zero() {
        for args in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({"SizeMB": "not a number"}),
            serde_json::json!({"SizeMB": -5}),
        ] {
            assert_eq!(size_mib_from_args(&args), 0);
        }
    }
}
