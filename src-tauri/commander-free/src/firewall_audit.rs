// SPDX-License-Identifier: AGPL-3.0-or-later
//! Conservative, preview-first audit of third-party Windows firewall rules.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[path = "firewall_audit/netsh.rs"]
mod netsh;
#[path = "firewall_audit/rules.rs"]
mod rules;
use rules::{eligible, parse_rules};

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_SELECTION: usize = 300;
static CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Eq, PartialEq)]
struct Rule {
    name: String,
    enabled: bool,
    action: String,
    program: String,
}
#[derive(Clone)]
struct CachedRule {
    rule: Rule,
}
struct Cache {
    created_at: Instant,
    rules: HashMap<String, CachedRule>,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            created_at: Instant::now(),
            rules: HashMap::new(),
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub action: String,
    pub program: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallAudit {
    pub rules: Vec<FirewallRule>,
    pub cancelled: bool,
    pub error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRemediation {
    pub changed: usize,
    pub cancelled: bool,
    pub errors: Vec<String>,
    pub backup_path: Option<String>,
}

#[tauri::command]
pub async fn firewall_audit_preview() -> Result<FirewallAudit, String> {
    ensure_read_allowed()?;
    CANCELLED.store(false, Ordering::Release);
    let (audit, cached) = tokio::task::spawn_blocking(scan_rules)
        .await
        .map_err(|e| format!("firewall audit task failed: {e}"))?;
    let mut guard = cache()
        .lock()
        .map_err(|_| "firewall audit cache lock poisoned".to_string())?;
    guard.created_at = Instant::now();
    guard.rules = cached;
    Ok(audit)
}

#[tauri::command]
pub async fn firewall_audit_remediate(
    rule_ids: Vec<String>,
    action: String,
) -> Result<FirewallRemediation, String> {
    ensure_mutation_allowed()?;
    let action = validate_action(&action)?.to_string();
    if rule_ids.is_empty()
        || rule_ids.len() > MAX_SELECTION
        || rule_ids.iter().any(|id| id.len() > 64)
    {
        return Err("invalid firewall rule selection".into());
    }
    let selected = selected_rules(rule_ids)?;
    CANCELLED.store(false, Ordering::Release);
    tokio::task::spawn_blocking(move || remediate_rules(selected, &action))
        .await
        .map_err(|e| format!("firewall remediation task failed: {e}"))?
}

#[tauri::command]
pub fn firewall_audit_cancel() {
    CANCELLED.store(true, Ordering::Release);
}

fn scan_rules() -> (FirewallAudit, HashMap<String, CachedRule>) {
    match read_rules() {
        Err(error) => (
            FirewallAudit {
                rules: Vec::new(),
                cancelled: false,
                error: Some(error),
            },
            HashMap::new(),
        ),
        Ok(all) => {
            let mut counts = HashMap::<String, usize>::new();
            for rule in &all {
                *counts.entry(rule.name.clone()).or_default() += 1;
            }
            let mut cached = HashMap::new();
            let mut rules = Vec::new();
            for rule in all
                .into_iter()
                .filter(|rule| eligible(rule) && counts.get(&rule.name) == Some(&1))
            {
                if CANCELLED.load(Ordering::Acquire) {
                    break;
                }
                let id = Uuid::new_v4().to_string();
                cached.insert(id.clone(), CachedRule { rule: rule.clone() });
                rules.push(FirewallRule {
                    id,
                    name: rule.name,
                    enabled: rule.enabled,
                    action: rule.action,
                    program: rule.program,
                });
            }
            (
                FirewallAudit {
                    rules,
                    cancelled: CANCELLED.load(Ordering::Acquire),
                    error: None,
                },
                cached,
            )
        }
    }
}

fn selected_rules(ids: Vec<String>) -> Result<Vec<CachedRule>, String> {
    let guard = cache()
        .lock()
        .map_err(|_| "firewall audit cache lock poisoned".to_string())?;
    if guard.created_at.elapsed() > CACHE_TTL {
        return Err("firewall audit expired; scan again before remediation".into());
    }
    ids.into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|id| {
            guard
                .rules
                .get(&id)
                .cloned()
                .ok_or_else(|| "firewall selection is stale or invalid; scan again".into())
        })
        .collect()
}

fn remediate_rules(selected: Vec<CachedRule>, action: &str) -> Result<FirewallRemediation, String> {
    let live = read_rules()?;
    let mut counts = HashMap::<String, usize>::new();
    for rule in &live {
        *counts.entry(rule.name.clone()).or_default() += 1;
    }
    let mut result = FirewallRemediation {
        changed: 0,
        cancelled: false,
        errors: Vec::new(),
        backup_path: export_backup().ok(),
    };
    for cached in selected {
        if CANCELLED.load(Ordering::Acquire) {
            result.cancelled = true;
            break;
        }
        let live_rule = live.iter().find(|rule| rule.name == cached.rule.name);
        if live_rule != Some(&cached.rule)
            || counts.get(&cached.rule.name) != Some(&1)
            || !eligible(&cached.rule)
        {
            result.errors.push(format!(
                "{}: rule changed after preview; scan again",
                cached.rule.name
            ));
            continue;
        }
        let args = match action {
            "enable" => vec![
                "advfirewall".into(),
                "firewall".into(),
                "set".into(),
                "rule".into(),
                format!("name={}", cached.rule.name),
                "new".into(),
                "enable=yes".into(),
            ],
            "disable" => vec![
                "advfirewall".into(),
                "firewall".into(),
                "set".into(),
                "rule".into(),
                format!("name={}", cached.rule.name),
                "new".into(),
                "enable=no".into(),
            ],
            "remove" => vec![
                "advfirewall".into(),
                "firewall".into(),
                "delete".into(),
                "rule".into(),
                format!("name={}", cached.rule.name),
            ],
            _ => unreachable!(),
        };
        match netsh::run_owned(&args) {
            Ok(_) => result.changed += 1,
            Err(error) => result.errors.push(format!("{}: {error}", cached.rule.name)),
        }
    }
    Ok(result)
}

fn read_rules() -> Result<Vec<Rule>, String> {
    let text = netsh::run(&[
        "advfirewall",
        "firewall",
        "show",
        "rule",
        "name=all",
        "verbose",
    ])?;
    let rules = parse_rules(&text);
    if rules.is_empty() && !text.trim().is_empty() {
        return Err("firewall rule output is unsupported on this Windows locale".into());
    }
    Ok(rules)
}

fn export_backup() -> Result<String, String> {
    let path = crate::paths::user_data_dir()?.join(format!(
        "firewall-audit-{}.wfw",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    let owned = path.to_string_lossy().into_owned();
    netsh::run(&["advfirewall", "export", &owned])?;
    Ok(owned)
}

fn validate_action(action: &str) -> Result<&str, String> {
    match action {
        "enable" | "disable" | "remove" => Ok(action),
        _ => Err("invalid firewall remediation action".into()),
    }
}
fn ensure_read_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        Err("Refused: firewall audit is unavailable in Decoy mode.".into())
    } else {
        Ok(())
    }
}
fn ensure_mutation_allowed() -> Result<(), String> {
    ensure_read_allowed()?;
    if crate::license::is_advanced_mode() {
        Err(
            "Refused: investigator mode forbids firewall changes because they alter evidence."
                .into(),
        )
    } else {
        Ok(())
    }
}
