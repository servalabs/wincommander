// SPDX-License-Identifier: AGPL-3.0-or-later
//! Preview-first ARP cache maintenance using the pinned Windows `arp` utility.

use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArpEntry {
    pub interface: String,
    pub address: String,
    pub physical_address: String,
    pub entry_type: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArpScan {
    pub scan_id: String,
    pub entries: Vec<ArpEntry>,
    pub dynamic_entries: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArpClearResult {
    pub before: usize,
    pub remaining: usize,
    pub cleared: usize,
}

struct CachedScan {
    id: String,
    created_at: Instant,
    dynamic_entries: usize,
}

fn cache() -> &'static Mutex<Option<CachedScan>> {
    static CACHE: OnceLock<Mutex<Option<CachedScan>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

#[tauri::command]
pub async fn arp_cache_scan() -> Result<ArpScan, String> {
    let entries = tokio::task::spawn_blocking(read_arp_cache)
        .await
        .map_err(|error| format!("ARP scan task failed: {error}"))??;
    let dynamic_entries = entries
        .iter()
        .filter(|entry| entry.entry_type.eq_ignore_ascii_case("dynamic"))
        .count();
    let scan_id = Uuid::new_v4().to_string();
    *cache()
        .lock()
        .map_err(|_| "ARP scan cache lock poisoned".to_string())? = Some(CachedScan {
        id: scan_id.clone(),
        created_at: Instant::now(),
        dynamic_entries,
    });
    Ok(ArpScan {
        scan_id,
        entries,
        dynamic_entries,
    })
}

#[tauri::command]
pub async fn arp_cache_clear(scan_id: String) -> Result<ArpClearResult, String> {
    ensure_mutation_allowed()?;
    if scan_id.len() > 64 {
        return Err("invalid ARP scan selection".into());
    }
    let before = {
        let guard = cache()
            .lock()
            .map_err(|_| "ARP scan cache lock poisoned".to_string())?;
        let cached = guard
            .as_ref()
            .ok_or("scan the ARP cache before clearing it")?;
        if cached.created_at.elapsed() > CACHE_TTL || cached.id != scan_id {
            return Err("ARP scan expired or is invalid; scan again".into());
        }
        cached.dynamic_entries
    };
    tokio::task::spawn_blocking(clear_arp_cache)
        .await
        .map_err(|error| format!("ARP clear task failed: {error}"))??;
    let remaining = read_arp_cache()?
        .into_iter()
        .filter(|entry| entry.entry_type.eq_ignore_ascii_case("dynamic"))
        .count();
    if before > 0 && remaining >= before {
        return Err("ARP cache clear did not remove any dynamic entries".into());
    }
    Ok(ArpClearResult {
        before,
        remaining,
        cleared: before.saturating_sub(remaining),
    })
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: ARP maintenance is unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode forbids ARP cache mutation.".into());
    }
    Ok(())
}

#[cfg(windows)]
fn read_arp_cache() -> Result<Vec<ArpEntry>, String> {
    use std::process::Command;
    let output = Command::new("arp.exe")
        .arg("-a")
        .output()
        .map_err(|error| format!("could not run arp.exe: {error}"))?;
    if !output.status.success() {
        return Err("arp.exe could not read the cache".into());
    }
    Ok(parse_arp_output(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(not(windows))]
fn read_arp_cache() -> Result<Vec<ArpEntry>, String> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn clear_arp_cache() -> Result<(), String> {
    use std::process::Command;
    let status = Command::new("arp.exe")
        .args(["-d", "*"])
        .status()
        .map_err(|error| format!("could not run arp.exe: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "arp.exe refused to clear the cache; Administrator may be required".into())
}

#[cfg(not(windows))]
fn clear_arp_cache() -> Result<(), String> {
    Err("ARP cache maintenance is available only on Windows".into())
}

fn parse_arp_output(output: &str) -> Vec<ArpEntry> {
    let mut interface = String::new();
    let mut entries = Vec::new();
    for line in output.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("Interface:") {
            interface = value.split_whitespace().next().unwrap_or_default().into();
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 3 && fields[0].parse::<std::net::IpAddr>().is_ok() {
            entries.push(ArpEntry {
                interface: interface.clone(),
                address: fields[0].into(),
                physical_address: fields[1].into(),
                entry_type: fields[2].into(),
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_structured_arp_rows() {
        let rows = parse_arp_output(
            "Interface: 10.0.0.2 --- 0x6\n  10.0.0.1 aa-bb-cc-dd-ee-ff dynamic\n  invalid row",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].interface, "10.0.0.2");
    }
}
