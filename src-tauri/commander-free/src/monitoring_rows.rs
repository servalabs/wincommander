// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{json, Value};

pub(crate) fn unavailable_row(
    id: &str,
    label: &str,
    group: &str,
    requires_pro: bool,
    cadence_secs: Option<u64>,
    capabilities: &[&str],
    error_code: &str,
) -> Value {
    json!({
        "id": id,
        "label": label,
        "group": group,
        "requiresPro": requires_pro,
        "running": false,
        "state": "unavailable",
        "health": "unavailable",
        "recentCount": Value::Null,
        "activeCount": Value::Null,
        "lastActivityAt": Value::Null,
        "startedAt": Value::Null,
        "cadenceSecs": cadence_secs,
        "errorCode": error_code,
        "capabilities": capabilities,
    })
}

pub(crate) fn locked_row(
    id: &str,
    label: &str,
    group: &str,
    cadence_secs: Option<u64>,
    capability_string: &str,
) -> Value {
    json!({
        "id": id,
        "label": label,
        "group": group,
        "requiresPro": true,
        "running": false,
        "state": "locked",
        "health": "locked",
        "recentCount": Value::Null,
        "activeCount": Value::Null,
        "lastActivityAt": Value::Null,
        "startedAt": Value::Null,
        "cadenceSecs": cadence_secs,
        "errorCode": "entitlement_required",
        "capabilities": capability_string
            .split(',')
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn local_row(
    id: &str,
    label: &str,
    group: &str,
    requires_pro: bool,
    locked: bool,
    cadence_secs: Option<u64>,
    capabilities: &[&str],
    status: Result<Value, String>,
    recent_count: Option<u64>,
    active_count: Option<u64>,
    health_issue: bool,
    error_code: Option<&str>,
) -> Value {
    if locked {
        return locked_row(id, label, group, cadence_secs, &capabilities.join(","));
    }
    let Ok(value) = status else {
        return unavailable_row(
            id,
            label,
            group,
            requires_pro,
            cadence_secs,
            capabilities,
            "status_unavailable",
        );
    };
    if !value.is_object() {
        return unavailable_row(
            id,
            label,
            group,
            requires_pro,
            cadence_secs,
            capabilities,
            "invalid_status",
        );
    }
    let is_running = value
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let health = if health_issue { "degraded" } else { "healthy" };
    let state = if health_issue {
        "degraded"
    } else if is_running {
        "active"
    } else {
        "idle"
    };
    json!({
        "id": id,
        "label": label,
        "group": group,
        "requiresPro": requires_pro,
        "running": is_running,
        "state": state,
        "health": health,
        "recentCount": value.get("recentCount").and_then(Value::as_u64).or(recent_count),
        "activeCount": value.get("activeCount").and_then(Value::as_u64).or(active_count),
        "lastActivityAt": value.get("lastTick").and_then(Value::as_str),
        "startedAt": value.get("startedAt").and_then(Value::as_str),
        "cadenceSecs": cadence_secs,
        "errorCode": if health_issue { error_code } else { None },
        "capabilities": capabilities,
    })
}

pub(crate) fn summarize(monitors: &[Value]) -> Value {
    let running = monitors
        .iter()
        .filter(|monitor| monitor.get("running").and_then(Value::as_bool) == Some(true))
        .count();
    let mut active = 0u64;
    let mut alerts = 0u64;
    let mut degraded = 0u64;
    let mut unavailable = 0u64;
    let mut locked = 0u64;
    for monitor in monitors {
        match monitor.get("state").and_then(Value::as_str) {
            Some("active") => active += 1,
            Some("alert") => alerts += 1,
            Some("degraded" | "stale") => degraded += 1,
            Some("unavailable") => unavailable += 1,
            Some("locked") => locked += 1,
            _ => {}
        }
    }
    json!({
        "total": monitors.len(),
        "running": running,
        "active": active,
        "alerts": alerts,
        "degraded": degraded,
        "unavailable": unavailable,
        "locked": locked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_rows_never_report_running() {
        let row = locked_row("test", "Test", "test", Some(5), "history");
        assert_eq!(row["state"], "locked");
        assert_eq!(row["running"], false);
        assert_eq!(row["errorCode"], "entitlement_required");
    }

    #[test]
    fn summary_keeps_locked_and_unavailable_distinct() {
        let rows = vec![
            json!({ "state": "active", "running": true }),
            json!({ "state": "degraded", "running": false }),
            json!({ "state": "unavailable", "running": false }),
            json!({ "state": "locked", "running": false }),
        ];
        let summary = summarize(&rows);
        assert_eq!(summary["running"], 1);
        assert_eq!(summary["active"], 1);
        assert_eq!(summary["degraded"], 1);
        assert_eq!(summary["unavailable"], 1);
        assert_eq!(summary["locked"], 1);
    }

    #[test]
    fn local_rows_redact_status_details_and_surface_health_without_running() {
        let row = local_row(
            "shield",
            "Shield",
            "physical",
            false,
            false,
            Some(3),
            &["camera"],
            Ok(json!({
                "running": false,
                "cameraAvailable": false,
                "cameraMessage": "C:\\Users\\Alice\\private-camera-name",
            })),
            None,
            None,
            true,
            Some("camera_unavailable"),
        );
        assert_eq!(row["state"], "degraded");
        assert_eq!(row["errorCode"], "camera_unavailable");
        assert!(!serde_json::to_string(&row)
            .unwrap()
            .contains("private-camera-name"));
    }
}
