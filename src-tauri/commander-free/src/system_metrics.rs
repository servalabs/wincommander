// SPDX-License-Identifier: AGPL-3.0-or-later
// Hardware probes retain one worker each; a stalled device cannot block the UI.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

#[path = "metrics_child.rs"]
mod metrics_child;
#[path = "metrics_probe.rs"]
mod metrics_probe;
use metrics_probe::Probe;

const CREATE_NO_WINDOW: u32 = 0x08000000;

static SYS: Lazy<Mutex<Option<System>>> = Lazy::new(|| Mutex::new(None));

// CPU temperature is read via a WMI/PowerShell probe, which is comparatively
// expensive and (historically) the source of a per-poll powershell.exe spawn.
// The 2s dashboard poll does NOT need fresh temp every tick, so the probe is
// throttled and cached here — at most one (windowless) spawn per TEMP_REFRESH.
const TEMP_REFRESH: Duration = Duration::from_secs(30);
static TEMP_CACHE: Lazy<Probe<Option<f32>>> = Lazy::new(Probe::new);

// Mount enumeration can block on disconnected network-backed volumes.
const DISK_SNAPSHOT_REFRESH: Duration = Duration::from_secs(5);
static DISK_SNAPSHOT: Lazy<Probe<Vec<DiskMetric>>> = Lazy::new(Probe::new);
static CPU_SNAPSHOT: Lazy<Probe<CpuMetrics>> = Lazy::new(Probe::new);
static SMART_SNAPSHOT: Lazy<Probe<DriveSmartHealthResult>> = Lazy::new(Probe::new);
static PROCESS_SNAPSHOT: Lazy<Probe<Vec<ProcessMetric>>> = Lazy::new(Probe::new);
static WIPE_SNAPSHOT: Lazy<Probe<Vec<WipeDriveEntry>>> = Lazy::new(Probe::new);
const CPU_REFRESH: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct CpuMetrics {
    cpu_usage: f32,
    ram_usage_percent: f32,
    ram_used_gb: f64,
    ram_total_gb: f64,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskMetric {
    pub name: String,
    pub total_gb: f64,
    pub free_gb: f64,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiveMetrics {
    pub cpu_usage: f32,
    pub cpu_temp: Option<f32>,
    pub ram_usage_percent: f32,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub disks: Vec<DiskMetric>,
    pub cpu_temp_status: &'static str,
    pub cpu_temp_age_ms: Option<u64>,
    pub disks_status: &'static str,
    pub disks_age_ms: Option<u64>,
}

fn collect_disk_metrics() -> Vec<DiskMetric> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .map(|disk: &sysinfo::Disk| DiskMetric {
            name: disk.mount_point().to_string_lossy().to_string(),
            total_gb: disk.total_space() as f64 / 1_073_741_824.0,
            free_gb: disk.available_space() as f64 / 1_073_741_824.0,
        })
        .collect()
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriveSmartHealth {
    pub drive_letter: String,
    pub health_percent: Option<u8>,
    pub passed: Option<bool>,
    pub source: String,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriveSmartHealthResult {
    pub smartctl_available: bool,
    pub drives: Vec<DriveSmartHealth>,
}

fn resolve_smartctl_path(deadline: Instant) -> Result<Option<PathBuf>, String> {
    // Absolute paths: check file existence — no need to spawn the process.
    let absolute_paths = [
        r"C:\Program Files\smartmontools\bin\smartctl.exe",
        r"C:\Program Files\smartmontools\smartctl.exe",
        r"C:\Program Files (x86)\smartmontools\bin\smartctl.exe",
        r"C:\Program Files (x86)\smartmontools\smartctl.exe",
        r"C:\ProgramData\chocolatey\bin\smartctl.exe",
    ];
    for candidate in absolute_paths {
        ensure_probe_budget(deadline)?;
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Ok(Some(p));
        }
    }

    // WinGet user-scoped links / packages
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let winget_candidates = [
            format!(r"{}\Microsoft\WinGet\Links\smartctl.exe", local_app_data),
            format!(
                r"{}\Microsoft\WinGet\Packages\smartmontools.smartmontools\bin\smartctl.exe",
                local_app_data
            ),
        ];
        for candidate in &winget_candidates {
            ensure_probe_budget(deadline)?;
            let p = PathBuf::from(candidate);
            if p.exists() {
                return Ok(Some(p));
            }
        }
    }

    // Last resort: PATH lookup via --version probe
    let mut cmd = Command::new("smartctl.exe");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.arg("--version");
    match metrics_child::output_until(&mut cmd, deadline) {
        Ok(out) if out.status.success() => return Ok(Some(PathBuf::from("smartctl.exe"))),
        Ok(_) => {}
        Err(error) if error == "Hardware probe could not start" => {}
        Err(error) => return Err(error),
    }

    Ok(None)
}

fn run_powershell_json(script: &str) -> Result<Value, String> {
    run_powershell_json_until(script, Instant::now() + Duration::from_secs(5))
}

fn run_powershell_json_until(script: &str, deadline: Instant) -> Result<Value, String> {
    let mut cmd = Command::new("powershell");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    let output = metrics_child::output_until(&mut cmd, deadline)?;

    if !output.status.success() {
        return Err("Hardware probe failed".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Err("Hardware probe returned no observation".into());
    }

    serde_json::from_str::<Value>(&stdout)
        .map_err(|_| "Hardware probe returned invalid data".into())
}

fn parse_health_from_smartctl_json(v: &Value) -> Option<u8> {
    if let Some(used) = v
        .get("nvme_smart_health_information_log")
        .and_then(|x| x.get("percentage_used"))
        .and_then(Value::as_f64)
    {
        let health = (100.0 - used).clamp(0.0, 100.0);
        return Some(health.round() as u8);
    }

    let attr_names = [
        "Percentage_Used",
        "Percent_Lifetime_Remain",
        "Media_Wearout_Indicator",
        "Wear_Leveling_Count",
        "SSD_Life_Left",
        "PercentLifeRemaining",
    ];

    if let Some(table) = v
        .get("ata_smart_attributes")
        .and_then(|x| x.get("table"))
        .and_then(Value::as_array)
    {
        for item in table {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            if !attr_names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                continue;
            }

            if let Some(value) = item.get("value").and_then(Value::as_u64) {
                return Some((value.min(100)) as u8);
            }

            if let Some(raw_val) = item
                .get("raw")
                .and_then(|x| x.get("value"))
                .and_then(Value::as_u64)
            {
                if raw_val <= 100 {
                    return Some(raw_val as u8);
                }
            }
        }
    }

    None
}

fn parse_passed_from_smartctl_json(v: &Value) -> Option<bool> {
    v.get("smart_status")
        .and_then(|x| x.get("passed"))
        .and_then(Value::as_bool)
}

fn extract_drive_map(deadline: Instant) -> Result<HashMap<String, u64>, String> {
    // Returns { "C:": 0, "D:": 1 } where value is the physical DiskNumber.
    let script = r#"
      $rows = Get-Partition -ErrorAction SilentlyContinue |
        Where-Object { $_.DriveLetter } |
        Select-Object @{Name='drive';Expression={"$($_.DriveLetter):"}}, DiskNumber
      $rows | ConvertTo-Json -Compress
    "#;

    let mut map = HashMap::new();
    let v = run_powershell_json_until(script, deadline)?;

    let items: Vec<Value> = match v {
        Value::Array(arr) => arr,
        Value::Object(_) => vec![v],
        _ => return Err("Hardware probe returned invalid drive inventory".into()),
    };

    for item in items {
        let Some(drive) = item.get("drive").and_then(Value::as_str) else {
            continue;
        };
        let Some(disk_number) = item.get("DiskNumber").and_then(Value::as_u64) else {
            continue;
        };
        map.insert(drive.to_ascii_uppercase(), disk_number);
    }

    Ok(map)
}

#[tauri::command]
pub async fn get_drive_smart_health() -> Result<DriveSmartHealthResult, String> {
    SMART_SNAPSHOT.refresh(Duration::from_secs(30), collect_drive_smart_health);
    SMART_SNAPSHOT.wait(Duration::from_secs(20)).await
}

fn collect_drive_smart_health() -> Result<DriveSmartHealthResult, String> {
    // One budget includes partition discovery, executable discovery, and every disk.
    let deadline = Instant::now() + Duration::from_secs(15);
    let drive_map = extract_drive_map(deadline)?;
    if drive_map.is_empty() {
        return Ok(DriveSmartHealthResult {
            smartctl_available: false,
            drives: vec![],
        });
    }

    let Some(smartctl_path) = resolve_smartctl_path(deadline)? else {
        return Ok(DriveSmartHealthResult {
            smartctl_available: false,
            drives: drive_map
                .keys()
                .map(|drive| DriveSmartHealth {
                    drive_letter: drive.clone(),
                    health_percent: None,
                    passed: None,
                    source: "unavailable".to_string(),
                })
                .collect(),
        });
    };

    let mut disk_health_cache: HashMap<u64, (Option<u8>, Option<bool>)> = HashMap::new();
    for disk_number in drive_map.values() {
        if disk_health_cache.contains_key(disk_number) {
            continue;
        }
        ensure_probe_budget(deadline)?;

        let device = format!("/dev/pd{}", disk_number);
        let mut cmd = Command::new(&smartctl_path);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);

        cmd.args(["-j", "-A", "-H", &device]);
        let output = metrics_child::output_until(&mut cmd, deadline);

        let (health_percent, passed) = match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                match serde_json::from_str::<Value>(&stdout) {
                    Ok(v) => (
                        parse_health_from_smartctl_json(&v),
                        parse_passed_from_smartctl_json(&v),
                    ),
                    Err(e) => {
                        crate::log_message(
                            "error",
                            &format!("[Metrics] Failed to parse smartctl JSON: {}", e),
                        );
                        (None, None)
                    }
                }
            }
            Ok(out) => {
                let _ = out;
                crate::log_message(
                    "warn",
                    "[Metrics] smartctl did not return a successful observation",
                );
                (None, None)
            }
            Err(error) => return Err(error),
        };

        disk_health_cache.insert(*disk_number, (health_percent, passed));
    }

    let mut drives = drive_map
        .into_iter()
        .map(|(drive, disk_number)| {
            let (health_percent, passed) = disk_health_cache
                .get(&disk_number)
                .cloned()
                .unwrap_or((None, None));
            DriveSmartHealth {
                drive_letter: drive,
                health_percent,
                passed,
                source: "smartctl".to_string(),
            }
        })
        .collect::<Vec<_>>();

    drives.sort_by(|a, b| a.drive_letter.cmp(&b.drive_letter));

    Ok(DriveSmartHealthResult {
        smartctl_available: true,
        drives,
    })
}

fn ensure_probe_budget(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("Hardware probe timed out".into())
    } else {
        Ok(())
    }
}

/// Top N processes by CPU usage, aggregated by name.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMetric {
    pub name: String,
    pub cpu_usage: f32,
    pub ram_mb: f64,
}

/// Returns the top CPU-consuming processes, names de-duped and aggregated.
///
/// Hot-path optimization: only CPU + memory are refreshed (the two fields
/// we actually consume). The previous `ProcessRefreshKind::everything()`
/// also refreshed cmdline / cwd / env / disk-usage / user — none of which
/// we read here. On a busy host that touches 300+ processes, this cuts
/// the per-call cost roughly 4-6×.
#[tauri::command]
pub async fn get_top_processes(limit: usize) -> Result<Vec<ProcessMetric>, String> {
    PROCESS_SNAPSHOT.refresh(Duration::from_secs(2), || Ok(collect_top_processes()));
    let mut processes = PROCESS_SNAPSHOT.wait(Duration::from_secs(2)).await?;
    processes.truncate(limit.clamp(1, 20));
    Ok(processes)
}

fn collect_top_processes() -> Vec<ProcessMetric> {
    static PROCESSES: Lazy<Mutex<Option<System>>> = Lazy::new(|| Mutex::new(None));
    // Single-flight ownership lets the worker retain deltas without locking during OS calls.
    let mut sys = PROCESSES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_default();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );

    let logical_cores = std::thread::available_parallelism().map_or(1, |count| count.get()) as f32;
    let mut agg: HashMap<String, ProcessMetric> = HashMap::new();
    for process in sys.processes().values() {
        let cpu = process.cpu_usage() / logical_cores;
        if cpu < 0.2 {
            continue;
        }
        let raw_name = process.name().to_string_lossy().to_string();
        let name = raw_name
            .trim_end_matches(".exe")
            .trim_end_matches(".EXE")
            .to_string();
        let entry = agg.entry(name.clone()).or_insert(ProcessMetric {
            name: name.clone(),
            cpu_usage: 0.0,
            ram_mb: 0.0,
        });
        entry.cpu_usage += cpu;
        entry.ram_mb += process.memory() as f64 / 1_048_576.0;
    }

    let mut processes: Vec<ProcessMetric> = agg.into_values().collect();
    processes.sort_by(|a, b| {
        b.cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    processes.truncate(20);
    *PROCESSES.lock().unwrap_or_else(|e| e.into_inner()) = Some(sys);
    processes
}

/// Query CPU temperature via WMI across multiple sources.
/// Returns actual CPU package/DTS temperature, not chipset/zone temps.
/// NOTE: Some systems (esp. desktops) don't expose CPU DTS via WMI —
/// third-party tools like LibreHardwareMonitor may be needed for full temp support.
fn get_cpu_temp_from_wmi() -> Result<Option<f32>, String> {
    // Try Win32_TemperatureProbe first (works on systems with proper sensor exposure)
    if let Some(temp) = query_wmi_temperature_probe()? {
        return Ok(Some(temp));
    }

    // Try CIM_TemperatureSensor as fallback
    if let Some(temp) = query_wmi_cim_temperature()? {
        return Ok(Some(temp));
    }

    // Note: MSAcpi_ThermalZoneTemperature / ThermalZoneInformation are NOT returned
    // because they often report chipset/zone temps (e.g. 28°C) instead of CPU package temp.
    // If you need accurate CPU temps, install LibreHardwareMonitor or Open Hardware Monitor.

    Ok(None)
}

/// Query Win32_TemperatureProbe for CPU temperatures.
/// Handles Intel Core, AMD Ryzen, package sensors, and other CPU-related probes.
fn query_wmi_temperature_probe() -> Result<Option<f32>, String> {
    let mut cmd = Command::new("powershell");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(
            r#"$temps = @();
               Get-WmiObject Win32_TemperatureProbe -ErrorAction SilentlyContinue |
               Where-Object { $_.CurrentReading -gt 0 } |
               ForEach-Object {
                   $name = $_.Name -replace '\s+', '';
                   $desc = $_.Description -replace '\s+', '';
                   $reading = [int]($_.CurrentReading / 10);
                   if ($name -match '(CPU|Package|Core|Die|Junction|Socket|Processor)' -or
                       $desc -match '(CPU|Package|Core|Die|Junction|Socket|Processor|Thermal)') {
                       $temps += $reading;
                   }
               };
               if ($temps.Count -gt 0) { [math]::Max($temps) } else { $null }"#,
        );
    let output = metrics_child::output(&mut cmd)?;

    if !output.status.success() {
        return Err("Temperature probe failed".into());
    }

    let stdout = String::from_utf8(output.stdout).map_err(|_| "Invalid temperature output")?;
    Ok(parse_temp_output(&stdout))
}

/// Query CIM_TemperatureSensor for CPU temps.
fn query_wmi_cim_temperature() -> Result<Option<f32>, String> {
    let mut cmd = Command::new("powershell");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(
            r#"$temps = @();
               Get-WmiObject CIM_TemperatureSensor -ErrorAction SilentlyContinue |
               Where-Object { $_.CurrentReading -gt 0 } |
               ForEach-Object {
                   $name = $_.Name -replace '\s+', '';
                   if ($name -match '(CPU|Package|Core|Die|Junction|Processor)') {
                       $reading = [int]($_.CurrentReading / 10);
                       $temps += $reading;
                   }
               };
               if ($temps.Count -gt 0) { [math]::Max($temps) } else { $null }"#,
        );
    let output = metrics_child::output(&mut cmd)?;

    if !output.status.success() {
        return Err("Temperature probe failed".into());
    }

    let stdout = String::from_utf8(output.stdout).map_err(|_| "Invalid temperature output")?;
    Ok(parse_temp_output(&stdout))
}

/// Parse PowerShell output and validate temperature is reasonable.
/// Valid range: 25°C (below ambient) to 110°C (below most thermal junction limits).
fn parse_temp_output(output: &str) -> Option<f32> {
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed == "$null" {
        return None;
    }

    trimmed
        .parse::<f32>()
        .ok()
        .filter(|&t| (20.0..=120.0).contains(&t))
}

// ── Wipe Drive List ──────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WipeDriveEntry {
    pub letter: String,
    pub label: String,
    pub free_gb: f64,
    pub total_gb: f64,
    pub media_type: String,
    pub bus_type: String,
    pub is_removable: bool,
    pub is_system: bool,
}

/// Enumerate local drives for the wipe selector; failure is not an empty inventory.
#[tauri::command]
pub async fn get_wipe_drive_list() -> Result<Vec<WipeDriveEntry>, String> {
    WIPE_SNAPSHOT.refresh(Duration::from_secs(5), collect_wipe_drive_list);
    WIPE_SNAPSHOT.wait(Duration::from_secs(7)).await
}

fn collect_wipe_drive_list() -> Result<Vec<WipeDriveEntry>, String> {
    // Get-PhysicalDisk gives MediaType/BusType; Get-PSDrive gives free/used space.
    // $ErrorActionPreference = SilentlyContinue so USB/SD drives with no matching
    // PhysicalDisk entry don't abort the entire loop.
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$out = @()
foreach ($drv in (Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Name.Length -eq 1 })) {
    $letter = $drv.Name
    $vol    = Get-Volume -DriveLetter $letter
    $part   = Get-Partition -DriveLetter $letter
    $mediaType = 'Unknown'; $busType = ''; $isRemovable = $false
    if ($part) {
        $disk = Get-Disk -Number $part.DiskNumber
        if ($disk) {
            $phys = Get-PhysicalDisk | Where-Object { $_.DeviceId -eq [string]$disk.Number }
            if ($phys) {
                $mediaType   = if ($phys.MediaType) { $phys.MediaType } else { 'Unknown' }
                $busType     = if ($phys.BusType)   { $phys.BusType }   else { '' }
                $isRemovable = [bool]($phys.BusType -in @('USB','SDIO','MMC'))
            }
        }
    }
    $freeGB  = [Math]::Round($drv.Free  / 1GB, 1)
    $totalGB = [Math]::Round(($drv.Used + $drv.Free) / 1GB, 1)
    $out += [PSCustomObject]@{
        letter      = $letter
        label       = if ($vol -and $vol.FileSystemLabel) { $vol.FileSystemLabel } else { '' }
        freeGB      = $freeGB
        totalGB     = $totalGB
        mediaType   = $mediaType
        busType     = $busType
        isRemovable = $isRemovable
        isSystem    = [bool]($letter -eq $env:SystemDrive.Substring(0,1))
    }
}
$out | ConvertTo-Json -Depth 2 -Compress
"#;

    let v = run_powershell_json(script)?;

    let items = match v {
        Value::Array(a) => a,
        obj @ Value::Object(_) => vec![obj],
        _ => return Err("Hardware probe returned invalid drive inventory".into()),
    };

    Ok(items
        .into_iter()
        .filter_map(|item| {
            let letter = item.get("letter").and_then(Value::as_str)?.to_string();
            Some(WipeDriveEntry {
                letter,
                label: item
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                free_gb: item.get("freeGB").and_then(Value::as_f64).unwrap_or(0.0),
                total_gb: item.get("totalGB").and_then(Value::as_f64).unwrap_or(0.0),
                media_type: item
                    .get("mediaType")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown")
                    .to_string(),
                bus_type: item
                    .get("busType")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                is_removable: item
                    .get("isRemovable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                is_system: item
                    .get("isSystem")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect())
}

/// Slow hardware observations never gate CPU/RAM reads or the window event loop.
#[tauri::command]
pub async fn get_live_metrics() -> Result<LiveMetrics, String> {
    CPU_SNAPSHOT.refresh(CPU_REFRESH, || Ok(collect_cpu_metrics()));
    DISK_SNAPSHOT.refresh(DISK_SNAPSHOT_REFRESH, || Ok(collect_disk_metrics()));
    TEMP_CACHE.refresh(TEMP_REFRESH, get_cpu_temp_from_wmi);
    let cpu = CPU_SNAPSHOT.wait(Duration::from_millis(500)).await?;
    let disks = DISK_SNAPSHOT.snapshot(DISK_SNAPSHOT_REFRESH);
    let temperature = TEMP_CACHE.snapshot(TEMP_REFRESH);
    let cpu_temp = temperature.value.flatten();
    Ok(LiveMetrics {
        cpu_usage: cpu.cpu_usage,
        cpu_temp,
        ram_usage_percent: cpu.ram_usage_percent,
        ram_used_gb: cpu.ram_used_gb,
        ram_total_gb: cpu.ram_total_gb,
        disks: disks.value.unwrap_or_default(),
        cpu_temp_status: if temperature.status == "live" && cpu_temp.is_none() {
            "unavailable"
        } else {
            temperature.status
        },
        cpu_temp_age_ms: temperature.age_ms,
        disks_status: disks.status,
        disks_age_ms: disks.age_ms,
    })
}

fn collect_cpu_metrics() -> CpuMetrics {
    let mut sys = SYS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_default();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let metrics = CpuMetrics {
        cpu_usage: sys.global_cpu_usage(),
        ram_usage_percent: if sys.total_memory() > 0 {
            (sys.used_memory() as f64 / sys.total_memory() as f64 * 100.0) as f32
        } else {
            0.0
        },
        ram_used_gb: sys.used_memory() as f64 / 1_073_741_824.0,
        ram_total_gb: sys.total_memory() as f64 / 1_073_741_824.0,
    };
    *SYS.lock().unwrap_or_else(|e| e.into_inner()) = Some(sys);
    metrics
}
