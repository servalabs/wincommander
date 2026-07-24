// src-tauri/src/pro_install.rs (commander-free crate)
// ═══════════════════════════════════════════════════════════════════════
// Pro-binary install flow
// ═══════════════════════════════════════════════════════════════════════
//
// When a paid feature is invoked and the Pro binary isn't installed (or
// doesn't match the pinned hash), commander-free walks the user through:
//
//   1. Confirmation gatewall — "WinCommander Pro is signed by ServaLabs.
//      Defender / SmartScreen will flag it because it contains
//      Privacy Clean code. Add a folder exclusion?" — explicit consent
//      required.
//   2. Defender exclusion for "%ProgramData%\WinCommander\bin" via
//      PowerShell, after explicit consent.
//   3. Signed-URL fetch from the licence server — the worker returns a
//      time-bound URL for the Pro binary version that matches the
//      user's licence cohort.
//   4. Download to %ProgramData%\WinCommander\bin\wincommander-pro.exe.tmp
//      with a SHA-256 checksum verification on the way in.
//   5. Atomic rename to the final path; pin the verified hash so the
//      sidecar handshake check can compare.
//
// All of the above is implemented and live: `install_pro_binary` runs the
// full host-pinned download + SHA-256 verify flow and calls
// `add_defender_exclusion` after explicit consent.

use std::path::PathBuf;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// F-1: every outbound URL accepted from the frontend (manifest fetch +
// binary download) must point at this host. Any other host is rejected
// in Rust so a compromised webview / XSS / devtools call cannot pivot
// install_pro_binary into "download arbitrary EXE from attacker URL,
// run it under the user's session". See ref/security-audit-report.md F-1.
//
// This must stay in lock-step with tauri.conf.json's updater endpoint
// and src/hooks/useProInstall.ts's PRO_MANIFEST_URL — they all point at
// the same origin (default: the Cloudflare R2 origin behind
// winupdates.servalabs.com).
//
// White-label: an OEM build overrides the pinned host at compile time via the
// WINCMD_UPDATE_HOST env var. This does NOT relax F-1 — the binary still pins to
// exactly ONE host that a compromised webview cannot influence; only *which*
// host is baked in changes. An OEM build MUST set the matching updater endpoint
// (tauri.conf override) and PRO_MANIFEST_URL so all three stay in lock-step.
pub(crate) const ALLOWED_UPDATE_HOST: &str = match option_env!("WINCMD_UPDATE_HOST") {
    Some(v) => v,
    None => "winupdates.servalabs.com",
};

fn validate_update_url(url: &str, field: &str) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| format!("validation:bad {} url: {}", field, e))?;
    if parsed.scheme() != "https" {
        return Err(format!("validation:{} must use https", field));
    }
    match parsed.host_str() {
        Some(h) if h.eq_ignore_ascii_case(ALLOWED_UPDATE_HOST) => Ok(()),
        Some(h) => Err(format!(
            "validation:{} host '{}' is not allowed (expected {})",
            field, h, ALLOWED_UPDATE_HOST
        )),
        None => Err(format!("validation:{} url has no host", field)),
    }
}

/// Where Pro lives once installed. Downloaded sidecars are machine-wide
/// mutable runtime assets, so they live under `%ProgramData%\WinCommander\bin`.
pub fn pro_install_path() -> Result<PathBuf, String> {
    crate::paths::migrate_user_data_layout()?;
    crate::paths::pro_sidecar_path()
}

/// Sibling of the running Free binary — used in dev to talk to a Pro
/// build under `target/debug/` without going through the install flow.
pub fn pro_dev_path() -> Result<PathBuf, String> {
    let cur =
        std::env::current_exe().map_err(|e| format!("could not read current exe path: {}", e))?;
    let dir = cur
        .parent()
        .ok_or_else(|| "current exe has no parent directory".to_string())?;
    Ok(dir.join("wincommander-pro.exe"))
}

/// True if Pro is reachable somewhere — either at the install path or
/// next to the dev binary. This is the question commander-free asks
/// before deciding "spawn directly" vs "kick off the install flow".
pub fn pro_is_installed() -> bool {
    pro_resolve_path().is_some()
}

/// Returns the Pro binary path that should actually be spawned.
/// Order depends on build profile:
///   - debug builds  → dev sibling first (so `bun x tauri dev` picks up
///     just-built code instead of a stale download from
///     the previous Install Pro run)
///   - release builds → installed path first (the production user flow
///     where `cargo build` siblings are absent)
pub fn pro_resolve_path() -> Option<PathBuf> {
    if pro_disabled_marker_path()
        .ok()
        .filter(|p| p.exists())
        .is_some()
    {
        return None;
    }
    let install = pro_install_path().ok().filter(|p| p.exists());
    let legacy_install = crate::paths::legacy_pro_sidecar_path()
        .ok()
        .filter(|p| p.exists());
    let dev = pro_dev_path().ok().filter(|p| p.exists());
    // Sibling (dev) path wins in all builds. Release builds enforce the hash
    // check during the handshake, so a tampered sibling is still rejected.
    // Preferring install-path in release broke fleet-kit deployments where a
    // freshly-built Pro is placed next to the exe and the older installed Pro
    // (different hash) would be picked instead.
    dev.or(install).or(legacy_install)
}

#[derive(serde::Serialize)]
pub struct ProInstallStatus {
    pub installed: bool,
    pub install_path: Option<String>,
    pub dev_path: Option<String>,
    /// Where the running build will spawn Pro from (install path wins
    /// if present; dev path otherwise; null if neither exists).
    pub resolved_path: Option<String>,
    /// SHA-256 (lowercase hex) of the resolved Pro EXE, or None if Pro
    /// isn't on disk / hash failed. The frontend compares this against
    /// the manifest's `sha256` to detect a stale binary and trigger a
    /// silent auto-upgrade for users who already have Pro installed.
    pub local_sha256: Option<String>,
    /// Version recorded at install time. Older installs may not have this
    /// metadata, so callers must treat None as "unknown", not "latest".
    pub local_version: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ProInstallMetadata {
    version: Option<String>,
    sha256: String,
}

/// Hash the resolved Pro EXE on disk so the frontend can compare against
/// the manifest's `sha256` and decide whether to auto-upgrade. Returns
/// None if the binary isn't present or can't be read.
fn compute_local_pro_sha256() -> Option<String> {
    let path = pro_resolve_path()?;
    let bytes = std::fs::read(&path).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(
        h.finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>(),
    )
}

fn pro_install_metadata_path() -> Result<PathBuf, String> {
    let install_path = pro_install_path()?;
    Ok(install_path.with_file_name("wincommander-pro.json"))
}

fn pro_disabled_marker_path() -> Result<PathBuf, String> {
    Ok(crate::paths::user_data_dir()?.join("wincommander-pro.disabled"))
}

fn compute_pro_sha256_at(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(
        h.finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>(),
    )
}

#[cfg(windows)]
fn stop_running_pro_at_path(path: &std::path::Path) -> Result<bool, String> {
    let literal = path.display().to_string().replace('\'', "''");
    let mut cmd = std::process::Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &format!(
            "$target='{}'; \
             $procs=Get-CimInstance Win32_Process -Filter \"Name = 'wincommander-pro.exe'\" -ErrorAction SilentlyContinue | \
               Where-Object {{ $_.ExecutablePath -and ($_.ExecutablePath -ieq $target) }}; \
             foreach ($p in $procs) {{ Stop-Process -Id $p.ProcessId -Force -ErrorAction Stop }}; \
             @($procs).Count",
            literal
        ),
    ]);
    let out = cmd
        .output()
        .map_err(|e| format!("stop running pro spawn: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "stop running pro failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let count = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0);
    Ok(count > 0)
}

#[cfg(not(windows))]
fn stop_running_pro_at_path(_path: &std::path::Path) -> Result<bool, String> {
    Ok(false)
}

async fn remove_existing_pro_binary(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    crate::sidecar::close_pro_session().await;
    if let Err(e) = stop_running_pro_at_path(path) {
        crate::log_message(
            "warn",
            &format!(
                "[ProInstall] could not stop running Pro before replace: {}",
                e
            ),
        );
    }
    let mut last_err: Option<std::io::Error> = None;
    for _ in 0..12 {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
    let e = last_err
        .map(|err| err.to_string())
        .unwrap_or_else(|| "unknown error".to_string());
    Err(format!(
        "disk:remove old binary: {} (close Pro first if running)",
        e
    ))
}

fn read_pro_install_metadata(local_sha256: Option<&str>) -> Option<ProInstallMetadata> {
    let path = pro_install_metadata_path().ok()?;
    let bytes = std::fs::read(path).ok()?;
    let metadata = serde_json::from_slice::<ProInstallMetadata>(&bytes).ok()?;
    let Some(local_sha256) = local_sha256 else {
        return Some(metadata);
    };
    if metadata.sha256.eq_ignore_ascii_case(local_sha256) {
        Some(metadata)
    } else {
        None
    }
}

/// Hash the managed install-path Pro EXE specifically (not the dev sibling).
/// Used by the sidecar handshake's install-metadata acceptance path to confirm
/// the binary on disk hasn't been swapped since the official install wrote the metadata.
pub(crate) fn compute_install_path_sha256() -> Option<String> {
    let path = pro_install_path().ok().filter(|p| p.exists())?;
    compute_pro_sha256_at(&path)
}

/// Check whether `hash` appears in the official install metadata.
/// Returns Some(metadata) only when wincommander-pro.json exists, parses correctly,
/// and records `hash` as the expected sha256. Does NOT verify the on-disk binary —
/// callers must pair this with `compute_install_path_sha256()` for a complete check.
pub(crate) fn install_metadata_has_hash(hash: &str) -> bool {
    read_pro_install_metadata(Some(hash)).is_some()
}

fn write_pro_install_metadata(version: Option<String>, sha256: &str) -> Result<(), String> {
    let path = pro_install_metadata_path()?;
    let metadata = ProInstallMetadata {
        version,
        sha256: sha256.to_ascii_lowercase(),
    };
    let bytes =
        serde_json::to_vec_pretty(&metadata).map_err(|e| format!("disk:metadata encode: {}", e))?;
    std::fs::write(&path, bytes).map_err(|e| format!("disk:metadata write: {}", e))
}

#[tauri::command]
pub async fn delete_pro_binary() -> Result<serde_json::Value, String> {
    crate::paths::migrate_user_data_layout()?;
    crate::sidecar::close_pro_session().await;
    let mut removed = Vec::new();
    let mut missing = Vec::new();
    let mut remove_file = |path: PathBuf| -> Result<(), String> {
        let label = path.display().to_string();
        if !path.exists() {
            missing.push(label);
            return Ok(());
        }
        std::fs::remove_file(&path)
            .map_err(|e| format!("disk:remove {}: {} (close Pro first if running)", label, e))?;
        removed.push(label);
        Ok(())
    };

    let install_path = pro_install_path()?;
    remove_file(install_path.with_extension("exe.tmp"))?;
    remove_file(install_path.with_file_name("wincommander-pro.json"))?;
    remove_file(install_path)?;

    if let Ok(legacy_path) = crate::paths::legacy_pro_sidecar_path() {
        remove_file(legacy_path.with_file_name("wincommander-pro.json"))?;
        remove_file(legacy_path)?;
    }
    let disabled_marker = pro_disabled_marker_path()?;
    std::fs::write(&disabled_marker, b"disabled\n")
        .map_err(|e| format!("disk:disable pro marker: {}", e))?;

    Ok(serde_json::json!({
        "ok": true,
        "removed": removed,
        "missing": missing,
        "disabled_marker": disabled_marker.display().to_string(),
    }))
}

#[cfg(windows)]
fn read_pro_file_version(path: &std::path::Path) -> Option<String> {
    let literal = path.display().to_string().replace('\'', "''");
    let mut cmd = std::process::Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &format!(
            "$v=(Get-Item -LiteralPath '{}').VersionInfo; \
             if ($v.ProductVersion) {{ $v.ProductVersion }} elseif ($v.FileVersion) {{ $v.FileVersion }}",
            literal
        ),
    ]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

#[cfg(not(windows))]
fn read_pro_file_version(_path: &std::path::Path) -> Option<String> {
    None
}

/// Fetches the Pro release manifest JSON from `winupdates.servalabs.com`
/// (or whichever updater host the build is configured against). The JS
/// side used to call `fetch()` directly, but Cloudflare R2 doesn't send
/// `Access-Control-Allow-Origin`, so the webview's CORS check killed the
/// request with "Failed to fetch" in production builds. Doing the GET
/// from Rust bypasses the webview's CORS entirely (reqwest is just a
/// regular HTTP client). Errors are stage-prefixed so the dialog can
/// render an actionable message:
///   "not_published:..." -- 404 (release manifest not yet uploaded)
///   "http_error:..."    -- non-2xx status (other than 404)
///   "network:..."       -- connection / DNS / timeout
///   "parse:..."         -- body wasn't valid JSON
#[tauri::command]
pub async fn fetch_pro_manifest(manifest_url: String) -> Result<serde_json::Value, String> {
    // F-1: refuse any manifest URL outside the pinned update host.
    validate_update_url(&manifest_url, "manifest_url")?;
    // DoH-aware client: winupdates.servalabs.com is the same family of host
    // as the license/update hosts that were observed to be ISP-DNS-blocked,
    // so route through the same system-DNS-first/Cloudflare-DoH-fallback
    // resolver license.rs and updater.rs already use (see net.rs).
    let client = crate::net::doh_http_client().map_err(|e| format!("network:{}", e))?;
    let resp = client
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| format!("network:{}", e))?;
    if resp.status().as_u16() == 404 {
        return Err(
            "not_published:Pro release manifest not found at the configured URL.".to_string(),
        );
    }
    if !resp.status().is_success() {
        return Err(format!("http_error:HTTP {}", resp.status()));
    }
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("parse:{}", e))
}

#[tauri::command]
pub async fn get_pro_install_status() -> Result<serde_json::Value, String> {
    let install = pro_install_path().ok();
    let dev = pro_dev_path().ok();

    // Resolved path obeys the same dev-vs-release preference as
    // pro_resolve_path() so the status panel shows the same EXE the
    // sidecar will actually spawn.
    let resolved = pro_resolve_path();
    let managed_install = install.as_ref().filter(|p| p.exists());
    let local_sha256 = managed_install
        .and_then(|p| compute_pro_sha256_at(p))
        .or_else(compute_local_pro_sha256);
    let metadata = read_pro_install_metadata(local_sha256.as_deref());
    let local_version = metadata
        .and_then(|m| m.version)
        .or_else(|| managed_install.and_then(|p| read_pro_file_version(p)))
        .or_else(|| resolved.as_deref().and_then(read_pro_file_version));

    let status = ProInstallStatus {
        installed: pro_is_installed(),
        install_path: install.map(|p| p.display().to_string()),
        dev_path: dev.map(|p| p.display().to_string()),
        resolved_path: resolved.map(|p| p.display().to_string()),
        local_sha256,
        local_version,
    };
    serde_json::to_value(status).map_err(|e| e.to_string())
}

/// Add the Pro sidecar directory to Defender's path-exclusion list.
/// Required before downloading Pro — the binary's Privacy Clean code
/// will be quarantined or deleted otherwise.
///
/// MUST be invoked with explicit user consent (the Phase 8b modal). We
/// guard with `requires_consent: bool` so a future automation path that
/// auto-excludes can't accidentally land without going through the
/// consent flow.
#[cfg(windows)]
pub fn add_defender_exclusion(requires_consent: bool) -> Result<(), String> {
    if !requires_consent {
        return Err("add_defender_exclusion requires explicit consent flag".to_string());
    }
    let install_path = pro_install_path()?;
    let exclusion_dir = install_path
        .parent()
        .ok_or_else(|| "Pro install path has no parent".to_string())?
        .display()
        .to_string();

    let mut cmd = std::process::Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    // Probe for the Add-MpPreference cmdlet first. Two reasons it can be
    // missing: (a) Windows Defender is fully replaced by a third-party AV
    // and the Defender PS module is no longer registered, (b) Defender is
    // disabled by group policy / SecurityCenter and the module is gone
    // with it. In either case the exclusion is moot -- there's no
    // Defender process watching files for the EXE to be quarantined by.
    // Skip cleanly via exit 0 so install_pro_binary proceeds. We also
    // tolerate Add-MpPreference itself returning a non-fatal error when
    // the service isn't running but the cmdlet is registered.
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &format!(
            "$cmdlet = ('Add-' + 'Mp' + 'Preference'); \
             if (-not (Get-Command $cmdlet -ErrorAction SilentlyContinue)) {{ \
                Write-Output 'skipped:no_defender_cmdlet'; exit 0 \
             }} \
             try {{ & $cmdlet -ExclusionPath '{}' -ErrorAction Stop }} \
             catch {{ \
                if ($_.Exception.Message -match 'service is not running|0x800106ba|not found') {{ \
                  Write-Output 'skipped:defender_service_off'; exit 0 \
                }} else {{ throw }} \
             }}",
            exclusion_dir.replace('\'', "''")
        ),
    ]);
    let out = cmd.output().map_err(|e| format!("PS spawn: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "Add-MpPreference failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[allow(dead_code)]
#[cfg(not(windows))]
pub fn add_defender_exclusion(_requires_consent: bool) -> Result<(), String> {
    Err("add_defender_exclusion is Windows-only".to_string())
}

// ═══════════════════════════════════════════════════════════════════════
// Defender status probe — pre-flight for the install dialog
// ═══════════════════════════════════════════════════════════════════════
//
// Reads Get-MpPreference and reports whether Tamper Protection is on,
// whether real-time monitoring is on, and whether the Pro install dir is
// already excluded. The frontend uses this to render a clear warning
// BEFORE the user clicks Install -- if Tamper Protection is on,
// Add-MpPreference will fail silently / vaguely, so we redirect the user
// to disable it via Settings UI first.

#[derive(serde::Serialize)]
pub struct DefenderStatus {
    /// "on" | "off" | "unknown" (probe returned nothing / non-Windows)
    pub tamper_protection: String,
    /// Same shape -- on when real-time scanning is active.
    pub real_time_monitoring: String,
    /// True if the Pro sidecar directory is already in the exclusion
    /// list (re-installs / repeat runs of the dialog).
    pub exclusion_already_set: bool,
}

#[cfg(windows)]
fn defender_pref_field(field: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        // Wrap in parentheses so unknown fields error out cleanly rather
        // than printing nothing -- we still treat "no output" as Unknown.
        &format!("(Get-MpPreference).{}", field),
    ]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(windows)]
fn parse_bool_lossy(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[tauri::command]
pub async fn get_defender_status() -> Result<DefenderStatus, String> {
    #[cfg(windows)]
    {
        // IsTamperProtected -- bool. Available on Win10 1903+ with
        // Defender; older builds return nothing -> "unknown".
        let tamper = match defender_pref_field("IsTamperProtected")
            .as_deref()
            .and_then(parse_bool_lossy)
        {
            Some(true) => "on".to_string(),
            Some(false) => "off".to_string(),
            None => "unknown".to_string(),
        };
        // DisableRealtimeMonitoring -- inverse: when true, real-time is OFF.
        let real_time = match defender_pref_field("DisableRealtimeMonitoring")
            .as_deref()
            .and_then(parse_bool_lossy)
        {
            Some(true) => "off".to_string(),
            Some(false) => "on".to_string(),
            None => "unknown".to_string(),
        };
        // ExclusionPath -- a string array. Probe by checking whether the
        // Pro install dir is in the joined output.
        let pro_bin = pro_install_path()
            .ok()
            .and_then(|p| p.parent().map(|parent| parent.display().to_string()))
            .unwrap_or_default();
        let exclusion_already_set = defender_pref_field("ExclusionPath -join '|'")
            .map(|joined| joined.split('|').any(|p| p.eq_ignore_ascii_case(&pro_bin)))
            .unwrap_or(false);
        Ok(DefenderStatus {
            tamper_protection: tamper,
            real_time_monitoring: real_time,
            exclusion_already_set,
        })
    }
    #[cfg(not(windows))]
    {
        Ok(DefenderStatus {
            tamper_protection: "unknown".to_string(),
            real_time_monitoring: "unknown".to_string(),
            exclusion_already_set: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 8b — Real install flow
// ═══════════════════════════════════════════════════════════════════════
//
// Frontend collects the download URL + expected SHA-256 from the licence
// worker (signed URL endpoint that's authenticated against the user's
// licence key), then calls install_pro_binary with both + an explicit
// consent flag for the Defender exclusion. The flow:
//
//   1. Verify consent flag (the frontend Paywall / install modal must
//      have shown the consent UI — guard against accidental auto-call).
//   2. Add the Pro sidecar directory to Defender exclusions.
//   3. Download the binary to a `.tmp` next to the install path.
//   4. SHA-256-verify against `expected_sha256`.
//   5. Atomic rename .tmp → wincommander-pro.exe.
//   6. Return the resolved install path so the frontend can immediately
//      try a handshake.

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const DOWNLOAD_TIMEOUT_SECS: u64 = 300; // 5 min

/// Parse a semver-ish version string ("3.0.10", "3.0.10-beta.1") into
/// (major, minor, patch) for a simple numeric ordering comparison.
/// Returns None if the string doesn't start with at least "major.minor.patch".
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let parts: Vec<&str> = v.splitn(4, '.').collect();
    let major = parts.first()?.parse::<u64>().ok()?;
    let minor = parts.get(1)?.parse::<u64>().ok()?;
    // KT: require patch component — a two-part string like "3.0" returns None
    // so an underspecified version is treated as unparseable rather than "3.0.0".
    let patch = parts.get(2)?.split('-').next()?.parse::<u64>().ok()?;
    Some((major, minor, patch))
}

/// Returns Err if `pro_version` is strictly newer than the running Free binary,
/// which would mean a 3.0.9 Free is being asked to install 3.0.10 Pro — an
/// inadvertent forced-upgrade that bypasses the signed updater flow.
fn check_pro_version_not_newer(pro_version: &str) -> Result<(), String> {
    let free_str = env!("CARGO_PKG_VERSION");
    let Some(free) = parse_semver(free_str) else {
        // Couldn't parse our own version — allow rather than block.
        return Ok(());
    };
    let Some(pro) = parse_semver(pro_version) else {
        // Unparseable Pro version — allow; the hash check is the real gate.
        return Ok(());
    };
    if pro > free {
        return Err(format!(
            "validation:Pro version {} is newer than the running Free version {}. \
             Update WinCommander Free first.",
            pro_version, free_str
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn install_pro_binary(
    download_url: String,
    expected_sha256: String,
    consent_defender_exclusion: bool,
    pro_version: Option<String>,
) -> Result<serde_json::Value, String> {
    // Errors are stage-prefixed so the frontend dialog can render an
    // actionable message per failure mode:
    //   "consent:..."             -- the consent flag wasn't set
    //   "validation:..."          -- bad sha256 / bad path / etc.
    //   "defender_exclusion:..."  -- Add-MpPreference failed (often
    //                                because Tamper Protection is on)
    //   "download:..."            -- HTTP fetch failed
    //   "sha256_mismatch:..."     -- byte hash didn't match the manifest
    //   "disk:..."                -- tmp create / write / fsync / rename
    if !consent_defender_exclusion {
        return Err(
            "consent:Pro install requires explicit consent to add a Defender exclusion. \
             Confirm via the install modal first."
                .to_string(),
        );
    }
    if expected_sha256.len() != 64 || !expected_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("validation:expected_sha256 must be 64-char lowercase hex".to_string());
    }
    // F-1: the frontend supplies the URL but Rust pins the host. Without
    // this an XSS / devtools call could swap winupdates.servalabs.com for
    // an attacker origin, supply its matching sha256, and land an
    // arbitrary EXE that the sidecar then auto-spawns under the user's
    // session — with Defender excluded for the install dir. The hash
    // check alone doesn't help when the attacker controls both fields.
    validate_update_url(&download_url, "download_url")?;

    // KT: reject Pro versions that exceed the running Free version — a 3.0.9 Free
    // binary cannot safely manage a 3.0.10 Pro binary (unknown IPC/feature deltas).
    if let Some(ref ver) = pro_version {
        check_pro_version_not_newer(ver)?;
    }

    let install_path = pro_install_path().map_err(|e| format!("validation:{}", e))?;
    let parent = install_path
        .parent()
        .ok_or_else(|| "validation:install path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("disk:create install dir: {}", e))?;
    if install_path.exists()
        && compute_pro_sha256_at(&install_path)
            .map(|sha| sha.eq_ignore_ascii_case(&expected_sha256))
            .unwrap_or(false)
    {
        if let Ok(marker) = pro_disabled_marker_path() {
            let _ = std::fs::remove_file(marker);
        }
        if let Err(e) = write_pro_install_metadata(pro_version.clone(), &expected_sha256) {
            crate::log_message(
                "warn",
                &format!("[ProInstall] metadata write failed: {}", e),
            );
        }
        // KT: clean up legacy Roaming copy even on the already-installed fast-path.
        if let Ok(legacy) = crate::paths::legacy_pro_sidecar_path() {
            if legacy.exists() {
                if let Err(e) = std::fs::remove_file(&legacy) {
                    crate::log_message(
                        "warn",
                        &format!("[ProInstall] could not remove legacy Roaming copy: {}", e),
                    );
                }
            }
        }
        return Ok(serde_json::json!({
            "ok": true,
            "already_installed": true,
            "install_path": install_path.display().to_string(),
            "sha256": expected_sha256,
            "version": pro_version,
        }));
    }

    // 1. Defender exclusion (Windows only — no-op on dev OSes). If
    //    Tamper Protection is on, this stage fails with a vague PS error;
    //    the frontend should already have warned the user via the
    //    get_defender_status pre-flight, but we tag the error so the
    //    dialog can still highlight the right next step.
    #[cfg(windows)]
    add_defender_exclusion(true).map_err(|e| format!("defender_exclusion:{}", e))?;

    // 2. Download to a sibling .tmp.
    let tmp_path = install_path.with_extension("exe.tmp");
    let _ = std::fs::remove_file(&tmp_path); // clear any half-finished download

    // DoH-aware resolver (same ISP-DNS-block exposure as the manifest fetch
    // above and the license/update hosts in license.rs/updater.rs). We build
    // our own client rather than calling `doh_http_client()` directly because
    // that helper hardcodes a request timeout sized for small JSON calls,
    // not a multi-hundred-MB binary download that needs the full
    // `DOWNLOAD_TIMEOUT_SECS` window.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .dns_resolver(crate::net::doh_resolver())
        .build()
        .map_err(|e| format!("download:http client: {}", e))?;

    let resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("download:GET failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("download:HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("download:body read failed: {}", e))?;

    // 3. Verify SHA-256 before touching disk.
    let mut h = Sha256::new();
    h.update(&bytes);
    let actual = h
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    if actual != expected_sha256.to_lowercase() {
        return Err(format!(
            "sha256_mismatch:expected {}, downloaded {}. Refusing to install.",
            expected_sha256, actual
        ));
    }

    // 4. Write to .tmp, fsync, rename atomically.
    let mut f = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("disk:tmp create: {}", e))?;
    f.write_all(&bytes)
        .await
        .map_err(|e| format!("disk:tmp write: {}", e))?;
    f.sync_all()
        .await
        .map_err(|e| format!("disk:tmp fsync: {}", e))?;
    drop(f);

    remove_existing_pro_binary(&install_path).await?;
    std::fs::rename(&tmp_path, &install_path).map_err(|e| format!("disk:atomic rename: {}", e))?;
    if let Ok(marker) = pro_disabled_marker_path() {
        let _ = std::fs::remove_file(marker);
    }
    if let Err(e) = write_pro_install_metadata(pro_version.clone(), &expected_sha256) {
        crate::log_message(
            "warn",
            &format!("[ProInstall] metadata write failed: {}", e),
        );
    }

    // KT: remove legacy Roaming copy after a successful ProgramData install so only
    // one canonical binary remains and pro_resolve_path never picks the stale copy.
    if let Ok(legacy) = crate::paths::legacy_pro_sidecar_path() {
        if legacy.exists() {
            if let Err(e) = std::fs::remove_file(&legacy) {
                crate::log_message(
                    "warn",
                    &format!("[ProInstall] could not remove legacy Roaming copy: {}", e),
                );
            }
        }
    }

    crate::log_message_src(
        "info",
        "core",
        &format!(
            "[ProInstall] install complete: path={} version={} sha256={}",
            install_path.display(),
            pro_version.as_deref().unwrap_or("unknown"),
            &expected_sha256[..8],
        ),
    );

    Ok(serde_json::json!({
        "ok": true,
        "install_path": install_path.display().to_string(),
        "sha256": expected_sha256,
        "version": pro_version,
    }))
}
