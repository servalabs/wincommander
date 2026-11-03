// SPDX-License-Identifier: AGPL-3.0-or-later
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use clipboard_guard_helper::policy::PolicySource;
use serde::Serialize;
use wincmd_clip_rules::{Action, DetectorCategory, Finding, Severity};

#[derive(Clone, Serialize)]
struct CategoryCount {
    category: DetectorCategory,
    count: u32,
    severity: Severity,
    report_to_fleet: bool,
}

#[derive(Clone, Serialize)]
struct ContentEvent {
    fleet_binding: Option<String>,
    event_id: String,
    categories: Vec<CategoryCount>,
}

fn queue() -> &'static Mutex<VecDeque<ContentEvent>> {
    static QUEUE: OnceLock<Mutex<VecDeque<ContentEvent>>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn binding_slot() -> &'static Mutex<Option<String>> {
    static BINDING: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    BINDING.get_or_init(|| Mutex::new(None))
}

pub(super) fn current_binding() -> Option<String> {
    binding_slot().lock().ok()?.clone()
}
pub(super) fn set_binding(binding: Option<String>) {
    if let Ok(mut value) = binding_slot().lock() {
        *value = binding;
    }
}
pub(super) fn policy_binding(policy: &wincmd_shared::fleet::PolicyEnvelope) -> Option<String> {
    let value = serde_json::to_string(&[
        policy.signer_key.as_str(),
        policy.org_id.0.as_str(),
        policy.device_id.0.as_str(),
        policy.enrollment_generation.as_deref()?,
    ])
    .ok()?;
    Some(hex::encode(super::hash_text(&value)))
}

pub(super) fn queue_findings(
    findings: &[(PolicySource, Finding)],
    fleet_binding: Option<String>,
) -> bool {
    if !crate::license::has_paid_entitlement() {
        return false;
    }
    let mut categories = category_counts(findings);
    if fleet_binding.is_none() {
        categories.retain(|category| !category.report_to_fleet);
    }
    if categories.is_empty() {
        return false;
    }
    if let Ok(mut pending) = queue().lock() {
        if pending.len() == 2000 {
            pending.pop_front();
        }
        pending.push_back(ContentEvent {
            fleet_binding,
            event_id: uuid::Uuid::new_v4().to_string(),
            categories,
        });
        return true;
    }
    false
}

fn category_counts(findings: &[(PolicySource, Finding)]) -> Vec<CategoryCount> {
    let mut counts = BTreeMap::<(&str, bool), CategoryCount>::new();
    for (source, finding) in findings {
        let report = *source == PolicySource::Fleet
            && finding
                .actions
                .iter()
                .any(|action| matches!(action, Action::ReportFleet | Action::AlertAdmin));
        let entry = counts
            .entry((finding.category.as_str(), report))
            .or_insert(CategoryCount {
                category: finding.category,
                count: 0,
                severity: finding.severity,
                report_to_fleet: report,
            });
        entry.count = entry.count.saturating_add(finding.count);
        let rank = |severity| match severity {
            Severity::Info => 0,
            Severity::Warn => 1,
            Severity::High => 2,
            Severity::Critical => 3,
        };
        if rank(finding.severity) > rank(entry.severity) {
            entry.severity = finding.severity;
        }
    }
    counts.into_values().collect()
}

pub(super) async fn flush() {
    drain(
        |args| async move {
            crate::sidecar::dispatch_paid_command("fleet_agent_dlp_content", args)
                .await
                .map(|_| ())
                .map_err(|_| ())
        },
        || crate::license::require_paid("clipboard detection counts").is_ok(),
        |seconds| tokio::time::sleep(std::time::Duration::from_secs(seconds)),
    )
    .await;
}

async fn drain<S, F, E, W, T>(send: S, enabled: E, wait: W)
where
    S: Fn(serde_json::Value) -> F,
    F: std::future::Future<Output = Result<(), ()>>,
    E: Fn() -> bool,
    W: Fn(u64) -> T,
    T: std::future::Future<Output = ()>,
{
    static FLUSHING: AtomicBool = AtomicBool::new(false);
    if FLUSHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let mut backoff = 1u64;
    loop {
        let event = {
            let Ok(pending) = queue().lock() else {
                FLUSHING.store(false, Ordering::SeqCst);
                return;
            };
            let Some(event) = pending.front().cloned() else {
                // Enqueue and releasing ownership share the lock, so an arrival
                // cannot miss both this drain and its own newly spawned drain.
                FLUSHING.store(false, Ordering::SeqCst);
                return;
            };
            event
        };
        if !enabled() {
            if let Ok(mut pending) = queue().lock() {
                pending.clear();
                FLUSHING.store(false, Ordering::SeqCst);
            }
            return;
        }
        let Ok(args) = serde_json::to_value(&event) else {
            FLUSHING.store(false, Ordering::SeqCst);
            return;
        };
        if send(args).await.is_err() {
            wait(backoff).await;
            backoff = (backoff * 2).min(30);
            continue;
        }
        backoff = 1;
        if let Ok(mut pending) = queue().lock() {
            if pending
                .front()
                .is_some_and(|front| front.event_id == event.event_id)
            {
                pending.pop_front();
            }
        }
    }
}

pub(super) fn clear() {
    set_binding(None);
    if let Ok(mut pending) = queue().lock() {
        pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn reporting_binding_changes_across_enrollment_even_when_the_epoch_number_is_reused() {
        let mut policy: wincmd_shared::fleet::PolicyEnvelope = serde_json::from_value(serde_json::json!({
            "org_id":"org-a", "device_id":"device-a", "signer_key":"signer", "signature":"unused",
            "enrollment_generation":"generation-a", "shield_revision":0
        })).unwrap();
        let first = policy_binding(&policy).unwrap();
        let expected = hex::encode(super::super::hash_text(
            r#"["signer","org-a","device-a","generation-a"]"#,
        ));
        assert_eq!(first, expected);
        policy.enrollment_generation = Some("generation-b".into());
        assert_ne!(policy_binding(&policy).unwrap(), first);
    }

    #[test]
    fn many_matching_rules_fit_the_metadata_bound_without_mixing_local_reporting_authority() {
        let finding: Finding = serde_json::from_value(serde_json::json!({
            "ruleId":"a0000000000000000000000000000001", "ruleRevision":1, "detectorId":"custom",
            "category":"national_id", "score":60, "count":1, "reasons":[], "severity":"warn", "actions":["report_fleet"]
        })).unwrap();
        let mut findings = vec![(PolicySource::Local, finding.clone()); 600];
        findings.extend(vec![(PolicySource::Fleet, finding); 600]);
        let counts = category_counts(&findings);
        assert_eq!(counts.len(), 2);
        assert_eq!(
            counts
                .iter()
                .find(|count| !count.report_to_fleet)
                .unwrap()
                .count,
            600
        );
        assert_eq!(
            counts
                .iter()
                .find(|count| count.report_to_fleet)
                .unwrap()
                .count,
            600
        );
    }

    #[tokio::test]
    async fn local_metadata_drains_independently_and_retries_the_same_event_after_an_ipc_outage() {
        let event = ContentEvent {
            fleet_binding: None,
            event_id: "synthetic-event".into(),
            categories: vec![CategoryCount {
                category: DetectorCategory::NationalId,
                count: 2,
                severity: Severity::Warn,
                report_to_fleet: false,
            }],
        };
        queue().lock().unwrap().clear();
        queue().lock().unwrap().push_back(event);
        let calls = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let delays = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let waited = delays.clone();
        drain(
            move |args| {
                let mut calls = recorded.lock().unwrap();
                calls.push(args);
                std::future::ready(if calls.len() == 1 { Err(()) } else { Ok(()) })
            },
            || true,
            move |seconds| {
                waited.lock().unwrap().push(seconds);
                std::future::ready(())
            },
        )
        .await;
        assert!(queue().lock().unwrap().is_empty());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], calls[1]);
        assert_eq!(calls[1]["categories"][0]["count"], 2);
        assert_eq!(calls[1]["categories"][0]["report_to_fleet"], false);
        assert_eq!(*delays.lock().unwrap(), vec![1]);
    }
}
