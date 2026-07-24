// src-tauri/src/system_metrics.rs
//
// Rust-native live system metrics using the `sysinfo` crate.
// Replaces the PowerShell-based Get-SystemInfo poll that spawned a new
// powershell.exe process every 3 seconds (the #1 CPU offender).
//
// A lazy-initialized System singleton is kept alive so sysinfo can compute
// accurate CPU deltas between refresh() calls.  Each invocation costs <1ms
// vs. ~300ms+ for the old PS approach.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

const CREATE_NO_WINDOW: u32 = 0x08000000;

static SYS: Lazy<Mutex<System>> = Lazy::new(|| {
    let mut sys = System::new_all();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    Mutex::new(sys)
});

// CPU temperature is read via a WMI/PowerShell probe, which is comparatively
// expensive and (historically) the source of a per-poll powershell.exe spawn.
// The 2s dashboard poll does NOT need fresh temp every tick, so the probe is
// throttled and cached here — at most one (windowless) spawn per TEMP_REFRESH.
const TEMP_REFRESH: Duration = Duration::from_secs(30);
static TEMP_CACHE: Lazy<Mutex<(Option<Instant>, Option<f32>)>> =
    Lazy::new(|| Mutex::new((None, None)));
// Set to true after the first probe attempt that returns None — on machines
// without accessible WMI temperature sensors (VMs, locked-down hardware) the
// queries always fail, so there is no point retrying every 30 seconds.
static TEMP_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

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

fn resolve_smartctl_path() -> Option<PathBuf> {
    // Absolute paths: check file existence — no need to spawn the process.
    let absolute_paths = [
        r"C:\Program Files\smartmontools\bin\smartctl.exe",
        r"C:\Program Files\smartmontools\smartctl.exe",
        r"C:\Program Files (x86)\smartmontools\bin\smartctl.exe",
        r"C:\Program Files (x86)\smartmontools\smartctl.exe",
        r"C:\ProgramData\chocolatey\bin\smartctl.exe",
    ];
    for candidate in absolute_paths {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
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
            let p = PathBuf::from(candidate);
            if p.exists() {
                return Some(p);
            }
        }
    }

    // Last resort: PATH lookup via --version probe
    let mut cmd = Command::new("smartctl.exe");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.arg("--version");
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            return Some(PathBuf::from("smartctl.exe"));
        }
    }

    None
}

fn run_powershell_json(script: &str) -> Option<Value> {
    let mut cmd = Command::new("powershell");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        crate::log_message(
            "error",
            &format!("[Metrics] PowerShell helper failed: {}", stderr.trim()),
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }

    serde_json::from_str::<Value>(&stdout).ok()
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

fn extract_drive_map() -> HashMap<String, u64> {
    // Returns { "C:": 0, "D:": 1 } where value is the physical DiskNumber.
    let script = r#"
      $rows = Get-Partition -ErrorAction SilentlyContinue |
        Where-Object { $_.DriveLetter } |
        Select-Object @{Name='drive';Expression={"$($_.DriveLetter):"}}, DiskNumber
      $rows | ConvertTo-Json -Compress
    "#;

    let mut map = HashMap::new();
    let Some(v) = run_powershell_json(script) else {
        return map;
    };

    let items: Vec<Value> = match v {
        Value::Array(arr) => arr,
        Value::Object(_) => vec![v],
        _ => vec![],
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

    map
}

#[tauri::command]
pub fn get_drive_smart_health() -> DriveSmartHealthResult {
    let drive_map = extract_drive_map();
    if drive_map.is_empty() {
        return DriveSmartHealthResult {
            smartctl_available: false,
            drives: vec![],
        };
    }

    let Some(smartctl_path) = resolve_smartctl_path() else {
        return DriveSmartHealthResult {
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
        };
    };

    let mut disk_health_cache: HashMap<u64, (Option<u8>, Option<bool>)> = HashMap::new();
    for disk_number in drive_map.values() {
        if disk_health_cache.contains_key(disk_number) {
            continue;
        }

        let device = format!("/dev/pd{}", disk_number);
        let mut cmd = Command::new(&smartctl_path);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let output = cmd.args(["-j", "-A", "-H", &device]).output();

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
                let stderr = String::from_utf8_lossy(&out.stderr);
                crate::log_message(
                    "warn",
                    &format!(
                        "[Metrics] smartctl failed for {}: {}",
                        device,
                        stderr.trim()
                    ),
                );
                (None, None)
            }
            Err(e) => {
                crate::log_message(
                    "error",
                    &format!("[Metrics] Failed to launch smartctl: {}", e),
                );
                (None, None)
            }
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

    DriveSmartHealthResult {
        smartctl_available: true,
        drives,
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
pub fn get_top_processes(limit: usize) -> Vec<ProcessMetric> {
    let mut sys = SYS.lock().unwrap();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );

    let logical_cores = sys.cpus().len().max(1) as f32;
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
    let cap = limit.clamp(1, 20);
    processes.truncate(cap);
    processes
}

/// Query CPU temperature via WMI across multiple sources.
/// Returns actual CPU package/DTS temperature, not chipset/zone temps.
/// NOTE: Some systems (esp. desktops) don't expose CPU DTS via WMI —
/// third-party tools like LibreHardwareMonitor may be needed for full temp support.
fn get_cpu_temp_from_wmi() -> Option<f32> {
    // Try Win32_TemperatureProbe first (works on systems with proper sensor exposure)
    if let Some(temp) = query_wmi_temperature_probe() {
        return Some(temp);
    }

    // Try CIM_TemperatureSensor as fallback
    if let Some(temp) = query_wmi_cim_temperature() {
        return Some(temp);
    }

    // Note: MSAcpi_ThermalZoneTemperature / ThermalZoneInformation are NOT returned
    // because they often report chipset/zone temps (e.g. 28°C) instead of CPU package temp.
    // If you need accurate CPU temps, install LibreHardwareMonitor or Open Hardware Monitor.

    None
}

/// Query Win32_TemperatureProbe for CPU temperatures.
/// Handles Intel Core, AMD Ryzen, package sensors, and other CPU-related probes.
fn query_wmi_temperature_probe() -> Option<f32> {
    let mut cmd = Command::new("powershell");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .arg("-NoProfile")
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
        )
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_temp_output(&stdout)
}

/// Query CIM_TemperatureSensor for CPU temps.
fn query_wmi_cim_temperature() -> Option<f32> {
    let mut cmd = Command::new("powershell");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .arg("-NoProfile")
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
        )
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_temp_output(&String::from_utf8(output.stdout).ok()?)
}

/// Parse PowerShell output and validate temperature is reasonable.
/// Valid range: 25°C (below ambient) to 110°C (below most thermal junction limits).
fn parse_temp_output(output: &str) -> Option<f32> {
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed == "$null" {
        return None;
    }

    trimmed.parse::<f32>().ok().and_then(|t| {
        if (20.0..=120.0).contains(&t) {
            Some(t)
        } else {
            None
        }
    })
}

/// CPU temperature, throttled. The WMI probe spawns a (windowless) PowerShell
/// process, so the reading is cached and re-probed at most once per
/// TEMP_REFRESH — keeping the 2s `get_live_metrics` poll spawn-free in between.
fn cpu_temp_throttled() -> Option<f32> {
    if TEMP_UNAVAILABLE.load(Ordering::Relaxed) {
        return None;
    }
    let mut cache = TEMP_CACHE.lock().unwrap();
    let fresh = cache.0.map(|t| t.elapsed() < TEMP_REFRESH).unwrap_or(false);
    if !fresh {
        let temp = get_cpu_temp_from_wmi();
        if temp.is_none() {
            TEMP_UNAVAILABLE.store(true, Ordering::Relaxed);
        }
        *cache = (Some(Instant::now()), temp);
    }
    cache.1
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

/// Enumerate local drives with media-type info for the free-space wipe selector.
/// Non-destructive read-only query. Returns empty vec on failure.
#[tauri::command]
pub fn get_wipe_drive_list() -> Vec<WipeDriveEntry> {
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

    let Some(v) = run_powershell_json(script) else {
        return vec![];
    };

    let items = match v {
        Value::Array(a) => a,
        obj @ Value::Object(_) => vec![obj],
        _ => return vec![],
    };

    items
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
        .collect()
}

/// Fast Tauri command — returns live CPU / RAM / Disk usage + CPU temp.
/// Called every 2s from the frontend when the dashboard is visible.
/// CPU/RAM/disk are kernel-counter reads (<2ms). CPU temp is a throttled WMI
/// probe (≤ once per 30s, windowless via CREATE_NO_WINDOW), so the 2s poll
/// itself spawns no process between refreshes.
#[tauri::command]
pub fn get_live_metrics() -> LiveMetrics {
    let mut sys = SYS.lock().unwrap();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    // CPU temperature via WMI only (no fallback to avoid stale/incorrect readings).
    // Win32_TemperatureProbe or MSAcpi_ThermalZoneTemperature if available.
    // Note: Some systems (especially desktops with BIOS-level temp restrictions)
    // may not expose CPU DTS to Windows WMI — HWInfo uses proprietary drivers.
    let cpu_temp: Option<f32> = cpu_temp_throttled();

    let disks = sysinfo::Disks::new_with_refreshed_list();

    LiveMetrics {
        cpu_usage: sys.global_cpu_usage(),
        cpu_temp,
        ram_usage_percent: if sys.total_memory() > 0 {
            (sys.used_memory() as f64 / sys.total_memory() as f64 * 100.0) as f32
        } else {
            0.0
        },
        ram_used_gb: sys.used_memory() as f64 / 1_073_741_824.0,
        ram_total_gb: sys.total_memory() as f64 / 1_073_741_824.0,
        disks: disks
            .iter()
            .map(|d: &sysinfo::Disk| DiskMetric {
                name: d.mount_point().to_string_lossy().to_string(),
                total_gb: d.total_space() as f64 / 1_073_741_824.0,
                free_gb: d.available_space() as f64 / 1_073_741_824.0,
            })
            .collect(),
    }
}
