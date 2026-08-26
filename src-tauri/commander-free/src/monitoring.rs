// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Unified, content-free monitor operations view. Free-owned monitors are
// read locally; Pro-owned monitor state is read through the authenticated
// sidecar only when the device has a paid entitlement.

use std::collections::HashSet;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::monitoring_catalog::{append_locked_pro, append_unavailable_pro, PRO_MONITORS};
use crate::monitoring_local::local_monitors;
use crate::monitoring_rows::summarize;

/// Return one bounded operations snapshot for the Monitor tab. It combines
/// Free-owned state with the Pro state without making either side expose raw
/// event data to the other side or to the frontend.
#[tauri::command]
pub async fn get_monitoring_overview(app: AppHandle) -> Result<Value, String> {
    let paid = crate::license::has_paid_entitlement();
    const PRO_OVERVIEW_TIMEOUT: Duration = Duration::from_secs(10);

    // The local and Pro checks do not depend on one another. A slow one must
    // not hold the other result hostage.
    let pro_snapshot = async {
        if !paid {
            return Err("entitlement_required");
        }
        match tokio::time::timeout(
            PRO_OVERVIEW_TIMEOUT,
            crate::sidecar::dispatch_paid_command("monitoring_overview", Value::Null),
        )
        .await
        {
            Ok(Ok(value)) => project_pro_rows(&value),
            Ok(Err(error)) if error.contains("agent session unavailable") => {
                Err("agent_unavailable")
            }
            Ok(Err(_)) => Err("pro_unavailable"),
            Err(_) => Err("snapshot_timeout"),
        }
    };
    let (mut monitors, pro_snapshot) = tokio::join!(local_monitors(app, paid), pro_snapshot);
    let mut pro_state = if paid { "unavailable" } else { "locked" };
    let mut pro_error_code: Option<&str> = if paid {
        Some("pro_unavailable")
    } else {
        Some("entitlement_required")
    };

    if paid {
        match pro_snapshot {
            Ok(rows) => {
                monitors.extend(rows);
                pro_state = "ready";
                pro_error_code = None;
            }
            Err(error_code) => append_unavailable_pro(&mut monitors, error_code),
        }
    } else {
        append_locked_pro(&mut monitors);
    }

    Ok(json!({
        "schemaVersion": 1,
        "scope": "this-device",
        "observedAt": Utc::now().to_rfc3339(),
        "monitors": monitors,
        "summary": summarize(&monitors),
        "pro": {
            "state": pro_state,
            "available": pro_state == "ready",
            "errorCode": pro_error_code,
        },
        "privacy": {
            "contentFree": true,
            "rawEventsIncluded": false,
            "identifiersIncluded": false,
        },
    }))
}

/// Project the signed Pro reply into the narrow DTO that the WebView may
/// receive. Labels, groups and capabilities stay public-owned; this protects
/// the content-free boundary against version skew and accidental additions to
/// private status payloads.
fn project_pro_rows(value: &Value) -> Result<Vec<Value>, &'static str> {
    let Some(rows) = value.get("monitors").and_then(Value::as_array) else {
        return Err("invalid_status");
    };
    if rows.len() != PRO_MONITORS.len() {
        return Err("invalid_status");
    }

    let mut seen = HashSet::with_capacity(rows.len());
    let mut projected = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(id) = row.get("id").and_then(Value::as_str) else {
            return Err("invalid_status");
        };
        let Some((_, label, group, default_cadence, capability_string)) =
            PRO_MONITORS.iter().find(|(known_id, ..)| *known_id == id)
        else {
            return Err("invalid_status");
        };
        if !seen.insert(id) {
            return Err("invalid_status");
        }
        let Some(running) = row.get("running").and_then(Value::as_bool) else {
            return Err("invalid_status");
        };
        let Some(state) = row.get("state").and_then(Value::as_str).filter(|state| {
            matches!(
                *state,
                "active" | "alert" | "degraded" | "stale" | "idle" | "unavailable"
            )
        }) else {
            return Err("invalid_status");
        };
        let Some(health) = row
            .get("health")
            .and_then(Value::as_str)
            .filter(|health| matches!(*health, "healthy" | "degraded" | "stale" | "unavailable"))
        else {
            return Err("invalid_status");
        };
        let cadence_secs = row
            .get("cadenceSecs")
            .and_then(Value::as_u64)
            .filter(|cadence| *cadence <= 86_400)
            .or(*default_cadence);
        let error_code = row.get("errorCode").and_then(Value::as_str).filter(|code| {
            matches!(
                *code,
                "collector_error"
                    | "policy_not_compiled"
                    | "configuration_conflict"
                    | "stale_poll"
                    | "status_unavailable"
                    | "invalid_status"
                    | "evidence_loss"
            )
        });
        let safe_time = |field: &str| {
            row.get(field).and_then(Value::as_str).filter(|time| {
                time.len() <= 64 && chrono::DateTime::parse_from_rfc3339(time).is_ok()
            })
        };
        projected.push(json!({
            "id": id,
            "label": label,
            "group": group,
            "requiresPro": true,
            "running": running,
            "state": state,
            "health": health,
            "recentCount": row.get("recentCount").and_then(Value::as_u64),
            "activeCount": row.get("activeCount").and_then(Value::as_u64),
            "lastActivityAt": safe_time("lastActivityAt"),
            "startedAt": safe_time("startedAt"),
            "cadenceSecs": cadence_secs,
            "errorCode": error_code,
            "capabilities": capability_string.split(',').filter(|item| !item.is_empty()).collect::<Vec<_>>(),
        }));
    }
    Ok(projected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pro_projection_drops_unapproved_fields() {
        let rows: Vec<Value> = PRO_MONITORS
            .iter()
            .map(|(id, _, _, cadence, _)| {
                json!({
                    "id": id,
                    "running": true,
                    "state": "active",
                    "health": "healthy",
                    "cadenceSecs": cadence,
                    "lastActivityAt": "2026-08-26T00:00:00Z",
                    "rawPath": "C:\\Users\\Alice\\secret.txt",
                })
            })
            .collect();
        let projected = project_pro_rows(&json!({ "monitors": rows })).unwrap();
        assert_eq!(projected.len(), PRO_MONITORS.len());
        assert!(!serde_json::to_string(&projected)
            .unwrap()
            .contains("secret.txt"));
    }
}
