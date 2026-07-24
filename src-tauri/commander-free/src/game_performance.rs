// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only performance reporting. Game Mode is isolated in the sibling
//! module so its state-changing allowlist stays easy to audit.

mod game_mode;
pub use game_mode::{GameModeOperation, GameModePreview};

use once_cell::sync::Lazy;
use serde::Serialize;
use std::cmp::Ordering;
use std::sync::Mutex;
use sysinfo::{Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};

static PERFORMANCE_SYSTEM: Lazy<Mutex<System>> = Lazy::new(|| Mutex::new(System::new_all()));
static PERFORMANCE_NETWORKS: Lazy<Mutex<Networks>> =
    Lazy::new(|| Mutex::new(Networks::new_with_refreshed_list()));

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceSnapshot {
    pub captured_at: String,
    pub cpu_usage_percent: f32,
    pub logical_cores: usize,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disks: Vec<DiskSnapshot>,
    pub network_interfaces: Vec<NetworkSnapshot>,
    pub top_cpu_processes: Vec<ProcessSnapshot>,
    pub top_memory_processes: Vec<ProcessSnapshot>,
    pub top_disk_processes: Vec<ProcessSnapshot>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSnapshot {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_percent: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    pub name: String,
    pub received_bytes_since_last_refresh: u64,
    pub transmitted_bytes_since_last_refresh: u64,
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub name: String,
    pub cpu_usage_percent: f32,
    pub memory_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_written_bytes: u64,
}

#[tauri::command]
pub async fn game_mode_preview() -> Result<GameModePreview, String> {
    game_mode::game_mode_preview().await
}

#[tauri::command]
pub async fn game_mode_apply() -> Result<GameModeOperation, String> {
    game_mode::game_mode_apply().await
}

#[tauri::command]
pub async fn game_mode_restore() -> Result<GameModeOperation, String> {
    game_mode::game_mode_restore().await
}

#[tauri::command]
pub fn get_performance_snapshot() -> Result<PerformanceSnapshot, String> {
    let mut system = PERFORMANCE_SYSTEM
        .lock()
        .map_err(|_| "performance system lock poisoned".to_string())?;
    system.refresh_cpu_all();
    system.refresh_memory();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage(),
    );

    let logical_cores = system.cpus().len().max(1);
    let processes = system
        .processes()
        .values()
        .map(|process| {
            let disk = process.disk_usage();
            ProcessSnapshot {
                pid: process.pid().as_u32(),
                name: process.name().to_string_lossy().to_string(),
                cpu_usage_percent: process.cpu_usage() / logical_cores as f32,
                memory_bytes: process.memory(),
                disk_read_bytes: disk.total_read_bytes,
                disk_written_bytes: disk.total_written_bytes,
            }
        })
        .collect::<Vec<_>>();
    let top_cpu_processes = top_processes(&processes, |process| process.cpu_usage_percent);
    let top_memory_processes = top_processes(&processes, |process| process.memory_bytes as f32);
    let top_disk_processes = top_processes(&processes, |process| {
        process
            .disk_read_bytes
            .saturating_add(process.disk_written_bytes) as f32
    });
    let disks = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|disk| DiskSnapshot {
            name: disk.name().to_string_lossy().to_string(),
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
            used_percent: used_percent(disk.total_space(), disk.available_space()),
        })
        .collect();
    let mut networks = PERFORMANCE_NETWORKS
        .lock()
        .map_err(|_| "performance network lock poisoned".to_string())?;
    networks.refresh(true);
    let mut network_interfaces = networks
        .iter()
        .map(|(name, network)| NetworkSnapshot {
            name: name.clone(),
            received_bytes_since_last_refresh: network.received(),
            transmitted_bytes_since_last_refresh: network.transmitted(),
            total_received_bytes: network.total_received(),
            total_transmitted_bytes: network.total_transmitted(),
        })
        .collect::<Vec<_>>();
    network_interfaces.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(PerformanceSnapshot {
        captured_at: chrono::Utc::now().to_rfc3339(),
        cpu_usage_percent: system.global_cpu_usage(),
        logical_cores,
        memory_used_bytes: system.used_memory(),
        memory_total_bytes: system.total_memory(),
        disks,
        network_interfaces,
        top_cpu_processes,
        top_memory_processes,
        top_disk_processes,
    })
}

fn top_processes(
    processes: &[ProcessSnapshot],
    score: impl Fn(&ProcessSnapshot) -> f32,
) -> Vec<ProcessSnapshot> {
    let mut ranked = processes.to_vec();
    ranked.sort_by(|left, right| {
        score(right)
            .partial_cmp(&score(left))
            .unwrap_or(Ordering::Equal)
    });
    ranked.truncate(10);
    ranked
}

fn used_percent(total: u64, available: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        ((total - available) as f64 / total as f64 * 100.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_percentage_handles_empty_drives() {
        assert_eq!(used_percent(0, 0), 0.0);
        assert_eq!(used_percent(100, 25), 75.0);
    }
}
