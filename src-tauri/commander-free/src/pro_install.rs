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

use std::{collections::BTreeMap, path::PathBuf};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// A Pro binary is deliberately machine-scoped: it lives under ProgramData and
// is shared by every interactive Windows user.  A standard token may read and
// run that verified artifact, but it must never replace it.  The foreground
// application therefore hands the narrow, validated update request to a
// second copy of this signed executable through Windows UAC.
const MACHINE_PRO_UPDATE_FLAG: &str = "--machine-pro-update";
const MACHINE_PRO_UPDATE_JOB_FLAG: &str = "--machine-pro-job";
const MACHINE_PRO_UPDATE_URL_FLAG: &str = "--machine-pro-url";
const MACHINE_PRO_UPDATE_SHA256_FLAG: &str = "--machine-pro-sha256";
const MACHINE_PRO_UPDATE_VERSION_FLAG: &str = "--machine-pro-version";
const MACHINE_PRO_UPDATE_DEFENDER_CONSENT_FLAG: &str = "--machine-pro-defender-consent";
const MACHINE_PRO_UPDATE_TIMEOUT_MS: u32 = 310_000;

#[derive(Debug, Clone)]
struct MachineProUpdateRequest {
    job_id: String,
    download_url: String,
    expected_sha256: String,
    consent_defender_exclusion: bool,
    pro_version: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MachineProUpdateOutcome {
    ok: bool,
    result: Option<serde_json::Value>,
    error: Option<String>,
}

// F-1: every outbound URL accepted from the frontend (manifest fetch +
// binary download) must point at this host. Any other host is rejected
// in Rust so a compromised webview / XSS / devtools call cannot pivot
// install_pro_binary into "download arbitrary EXE from attacker URL,
// run it under the user's session". See ref/security-audit-report.md F-1.
//
// This must stay in lock-step with src/hooks/useProInstall.ts's
// PRO_MANIFEST_BASE. It governs only Pro manifests and binaries; the Free
// Tauri updater has its own minisign-verified R2-backed endpoint.
//
// White-label: an OEM build overrides the pinned host at compile time via the
// WINCMD_UPDATE_HOST env var. This does NOT relax F-1 — the binary still pins to
// exactly ONE host that a compromised webview cannot influence; only *which*
// host is baked in changes. An OEM build MUST set the matching
// PRO_MANIFEST_BASE so the two remain in lock-step.
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

fn is_managed_program_files_install(
    executable: &std::path::Path,
    program_files: &std::path::Path,
) -> bool {
    let Some(parent) = executable.parent() else {
        return false;
    };
    parent
        .to_string_lossy()
        .eq_ignore_ascii_case(&program_files.join("WinCommander").to_string_lossy())
}

fn select_pro_binary(
    install: Option<PathBuf>,
    sibling: Option<PathBuf>,
    legacy_install: Option<PathBuf>,
    prefer_managed_install: bool,
) -> Option<PathBuf> {
    if prefer_managed_install {
        install.or(sibling).or(legacy_install)
    } else {
        sibling.or(install).or(legacy_install)
    }
}

/// Installed releases must not run a leftover sidecar beside the Free EXE.
/// Portable Fleet kits intentionally remain side-by-side and therefore retain
/// the sibling-first behavior.
fn should_prefer_managed_install() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let Some(program_files) = std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .map(PathBuf::from)
    else {
        return false;
    };
    is_managed_program_files_install(&executable, &program_files)
}

/// Returns the Pro binary path that should actually be spawned.
/// Order depends on build profile:
///   - debug builds  → dev sibling first (so `bun x tauri dev` picks up
///     just-built code instead of a stale download from
///     the previous Install Pro run)
///   - release builds → installed path first (the production user flow
///     where `cargo build` siblings are absent)
pub fn pro_resolve_path() -> Option<PathBuf> {
    migrate_legacy_disabled_marker();
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
    select_pro_binary(
        install,
        dev,
        legacy_install,
        should_prefer_managed_install(),
    )
}

#[cfg(test)]
mod path_selection_tests {
    use super::{is_managed_program_files_install, select_pro_binary};
    use std::path::{Path, PathBuf};

    #[test]
    fn managed_program_files_location_is_recognised() {
        assert!(is_managed_program_files_install(
            Path::new(r"C:\Program Files\WinCommander\wincommander-free.exe"),
            Path::new(r"C:\Program Files"),
        ));
        assert!(!is_managed_program_files_install(
            Path::new(r"C:\FleetKit\wincommander-free.exe"),
            Path::new(r"C:\Program Files"),
        ));
    }

    #[test]
    fn installed_release_prefers_managed_pro_over_a_stale_sibling() {
        let managed = PathBuf::from(r"C:\ProgramData\WinCommander\bin\wincommander-pro.exe");
        let sibling = PathBuf::from(r"C:\Program Files\WinCommander\wincommander-pro.exe");
        assert_eq!(
            select_pro_binary(Some(managed.clone()), Some(sibling), None, true),
            Some(managed),
        );
    }

    #[test]
    fn portable_fleet_kit_keeps_its_side_by_side_pro_binary() {
        let managed = PathBuf::from(r"C:\ProgramData\WinCommander\bin\wincommander-pro.exe");
        let sibling = PathBuf::from(r"C:\FleetKit\wincommander-pro.exe");
        assert_eq!(
            select_pro_binary(Some(managed), Some(sibling.clone()), None, false),
            Some(sibling),
        );
    }
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
    crate::paths::machine_state_file("wincommander-pro.disabled")
}

fn legacy_pro_disabled_marker_path() -> Result<PathBuf, String> {
    crate::paths::legacy_user_state_file("wincommander-pro.disabled")
}

fn migrate_legacy_disabled_marker() {
    let Ok(_lock) = crate::paths::acquire_machine_state_lock("pro-disabled-marker") else {
        return;
    };
    let Ok(machine_marker) = pro_disabled_marker_path() else {
        return;
    };
    if machine_marker.exists() {
        return;
    }
    let Ok(legacy_marker) = legacy_pro_disabled_marker_path() else {
        return;
    };
    if !legacy_marker.exists() {
        return;
    }
    // The marker represents an explicit elevated uninstall. Keep the old copy
    // unless its replacement in ProgramData succeeds, so a failed migration
    // cannot unexpectedly reactivate Pro for the user who disabled it.
    if crate::paths::atomic_write_machine_state(&machine_marker, b"disabled\n").is_ok() {
        let _ = std::fs::remove_file(legacy_marker);
    }
}

fn write_disabled_marker() -> Result<PathBuf, String> {
    let _lock = crate::paths::acquire_machine_state_lock("pro-disabled-marker")?;
    let marker = pro_disabled_marker_path()?;
    crate::paths::atomic_write_machine_state(&marker, b"disabled\n")?;
    Ok(marker)
}

fn clear_disabled_markers() {
    let Ok(_lock) = crate::paths::acquire_machine_state_lock("pro-disabled-marker") else {
        return;
    };
    if let Ok(marker) = pro_disabled_marker_path() {
        let _ = std::fs::remove_file(marker);
    }
    if let Ok(marker) = legacy_pro_disabled_marker_path() {
        let _ = std::fs::remove_file(marker);
    }
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
             $remaining=Get-CimInstance Win32_Process -Filter \"Name = 'wincommander-pro.exe'\" -ErrorAction SilentlyContinue | \
               Where-Object {{ $_.ExecutablePath -and ($_.ExecutablePath -ieq $target) }}; \
             if (@($remaining).Count -gt 0) {{ throw ('verified Pro process still running: ' + (@($remaining | ForEach-Object ProcessId) -join ',')) }}; \
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

/// Replace the shared version/hash record without leaving a partially-written
/// JSON file for another Windows session to observe.
fn atomic_replace_shared_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let parent = path
        .parent()
        .ok_or_else(|| "disk:shared metadata has no parent directory".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "disk:shared metadata has no file name".to_string())?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    {
        let mut file = std::fs::File::create(&temporary)
            .map_err(|error| format!("disk:metadata temp create: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("disk:metadata temp write: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("disk:metadata temp sync: {error}"))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            let _ = std::fs::remove_file(&temporary);
            return Err("disk:metadata atomic replace failed".to_string());
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&temporary, path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("disk:metadata atomic replace: {error}")
        })?;
    }
    Ok(())
}

fn write_pro_install_metadata(version: Option<String>, sha256: &str) -> Result<(), String> {
    let path = pro_install_metadata_path()?;
    let metadata = ProInstallMetadata {
        version,
        sha256: sha256.to_ascii_lowercase(),
    };
    let bytes =
        serde_json::to_vec_pretty(&metadata).map_err(|e| format!("disk:metadata encode: {}", e))?;
    atomic_replace_shared_file(&path, &bytes)
}

fn remove_file_if_present(
    path: PathBuf,
    removed: &mut Vec<String>,
    missing: &mut Vec<String>,
) -> Result<(), String> {
    let label = path.display().to_string();
    if !path.exists() {
        missing.push(label);
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| format!("disk:remove {}: {}", label, e))?;
    removed.push(label);
    Ok(())
}

#[tauri::command]
pub async fn delete_pro_binary() -> Result<serde_json::Value, String> {
    crate::paths::migrate_user_data_layout()?;
    // Disable first. If an antivirus product or an already-running process keeps
    // a file locked, Pro must still remain off rather than being rediscovered on
    // the next launch.
    let disabled_marker =
        write_disabled_marker().map_err(|e| format!("disk:disable pro marker: {}", e))?;
    crate::sidecar::close_pro_session().await;
    let mut removed = Vec::new();
    let mut missing = Vec::new();
    let mut warnings = Vec::new();

    let install_path = pro_install_path()?;
    for path in [
        install_path.with_extension("exe.tmp"),
        install_path.with_file_name("wincommander-pro.json"),
    ] {
        if let Err(error) = remove_file_if_present(path, &mut removed, &mut missing) {
            warnings.push(error);
        }
    }
    if !install_path.exists() {
        missing.push(install_path.display().to_string());
    } else if let Err(error) = remove_existing_pro_binary(&install_path).await {
        warnings.push(error);
    } else if install_path.exists() {
        warnings.push(format!(
            "disk:remove {}: file still exists",
            install_path.display()
        ));
    } else {
        removed.push(install_path.display().to_string());
    }

    if let Ok(legacy_path) = crate::paths::legacy_pro_sidecar_path() {
        let legacy_metadata = legacy_path.with_file_name("wincommander-pro.json");
        if let Err(error) = remove_file_if_present(legacy_metadata, &mut removed, &mut missing) {
            warnings.push(error);
        }
        if !legacy_path.exists() {
            missing.push(legacy_path.display().to_string());
        } else if let Err(error) = remove_existing_pro_binary(&legacy_path).await {
            warnings.push(error);
        } else if legacy_path.exists() {
            warnings.push(format!(
                "disk:remove {}: file still exists",
                legacy_path.display()
            ));
        } else {
            removed.push(legacy_path.display().to_string());
        }
    }

    Ok(serde_json::json!({
        "ok": true,
        "disabled": true,
        "fully_removed": warnings.is_empty(),
        "removed": removed,
        "missing": missing,
        "warnings": warnings,
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

#[cfg(windows)]
fn defender_exclusion_already_set() -> bool {
    let pro_bin = pro_install_path()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.display().to_string()))
        .unwrap_or_default();
    defender_pref_field("ExclusionPath -join '|'")
        .map(|joined| {
            joined
                .split('|')
                .any(|path| path.eq_ignore_ascii_case(&pro_bin))
        })
        .unwrap_or(false)
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
        let exclusion_already_set = defender_exclusion_already_set();
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

fn validate_machine_pro_update_request(request: &MachineProUpdateRequest) -> Result<(), String> {
    if uuid::Uuid::parse_str(&request.job_id).is_err() {
        return Err("invalid machine Pro update job id".to_string());
    }
    if request.download_url.len() > 4_096 {
        return Err("validation:download url is too long".to_string());
    }
    if request.expected_sha256.len() != 64
        || !request
            .expected_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(
            "validation:expected_sha256 must be a 64-character hexadecimal value".to_string(),
        );
    }
    validate_update_url(&request.download_url, "download_url")?;
    if let Some(version) = &request.pro_version {
        if version.len() > 128 {
            return Err("validation:Pro version is too long".to_string());
        }
        check_pro_version_not_newer(version)?;
    }
    Ok(())
}

fn machine_pro_update_result_path(job_id: &str) -> Result<PathBuf, String> {
    let job = uuid::Uuid::parse_str(job_id)
        .map_err(|_| "invalid machine Pro update job id".to_string())?;
    Ok(crate::paths::machine_data_dir()?.join(format!(".pro-update-{job}.json")))
}

fn write_machine_pro_update_outcome(
    job_id: &str,
    outcome: &MachineProUpdateOutcome,
) -> Result<(), String> {
    let path = machine_pro_update_result_path(job_id)?;
    let encoded = serde_json::to_vec(outcome)
        .map_err(|error| format!("machine update result encode: {error}"))?;
    std::fs::write(&path, encoded).map_err(|error| format!("machine update result write: {error}"))
}

fn read_machine_pro_update_outcome(job_id: &str) -> Result<serde_json::Value, String> {
    let path = machine_pro_update_result_path(job_id)?;
    let encoded = std::fs::read(&path)
        .map_err(|error| format!("elevation:could not read elevated Pro update result: {error}"))?;
    let outcome = serde_json::from_slice::<MachineProUpdateOutcome>(&encoded).map_err(|error| {
        format!("elevation:elevated Pro update returned an invalid result: {error}")
    })?;
    if outcome.ok {
        outcome
            .result
            .ok_or_else(|| "elevation:elevated Pro update returned no result".to_string())
    } else {
        Err(format!(
            "{}",
            outcome
                .error
                .unwrap_or_else(|| "elevation:elevated Pro update failed".to_string())
        ))
    }
}

fn parse_machine_pro_update_request(
    args: &[String],
) -> Result<Option<MachineProUpdateRequest>, String> {
    if args.first().map(String::as_str) != Some(MACHINE_PRO_UPDATE_FLAG) {
        return Ok(None);
    }
    if args.len() < 3 || args.len() % 2 == 0 {
        return Err("machine Pro update arguments are incomplete".to_string());
    }

    let mut values = BTreeMap::new();
    for pair in args[1..].chunks_exact(2) {
        let key = &pair[0];
        let value = &pair[1];
        if !matches!(
            key.as_str(),
            MACHINE_PRO_UPDATE_JOB_FLAG
                | MACHINE_PRO_UPDATE_URL_FLAG
                | MACHINE_PRO_UPDATE_SHA256_FLAG
                | MACHINE_PRO_UPDATE_VERSION_FLAG
                | MACHINE_PRO_UPDATE_DEFENDER_CONSENT_FLAG
        ) || values.insert(key.as_str(), value.as_str()).is_some()
        {
            return Err("machine Pro update arguments are invalid".to_string());
        }
    }

    let required = |flag: &str| {
        values
            .get(flag)
            .map(|value| (*value).to_string())
            .ok_or_else(|| "machine Pro update arguments are incomplete".to_string())
    };
    let consent = match required(MACHINE_PRO_UPDATE_DEFENDER_CONSENT_FLAG)?.as_str() {
        "0" => false,
        "1" => true,
        _ => return Err("machine Pro update consent argument is invalid".to_string()),
    };
    let request = MachineProUpdateRequest {
        job_id: required(MACHINE_PRO_UPDATE_JOB_FLAG)?,
        download_url: required(MACHINE_PRO_UPDATE_URL_FLAG)?,
        expected_sha256: required(MACHINE_PRO_UPDATE_SHA256_FLAG)?,
        consent_defender_exclusion: consent,
        pro_version: values
            .get(MACHINE_PRO_UPDATE_VERSION_FLAG)
            .map(|value| (*value).to_string())
            .filter(|value| !value.is_empty()),
    };
    validate_machine_pro_update_request(&request)?;
    Ok(Some(request))
}

/// Called by the GUI executable before Tauri starts.  The helper has no
/// webview, no IPC listener, and accepts only the fixed update argument set
/// constructed below.  Its sole capability is replacing the verified shared
/// Pro artifact after Windows has issued an elevated token.
pub fn run_machine_pro_update_if_requested(args: &[String]) -> Option<i32> {
    let request = match parse_machine_pro_update_request(args) {
        Ok(Some(request)) => request,
        Ok(None) => return None,
        Err(error) => {
            crate::log_message(
                "error",
                &format!("[ProInstall] invalid machine update helper invocation: {error}"),
            );
            return Some(1);
        }
    };

    #[cfg(windows)]
    if !crate::startup_elevation::is_current_process_elevated() {
        return Some(1);
    }

    let outcome = match tauri::async_runtime::block_on(install_pro_binary_machine(
        request.download_url.clone(),
        request.expected_sha256.clone(),
        request.consent_defender_exclusion,
        request.pro_version.clone(),
    )) {
        Ok(result) => MachineProUpdateOutcome {
            ok: true,
            result: Some(result),
            error: None,
        },
        Err(error) => MachineProUpdateOutcome {
            ok: false,
            result: None,
            error: Some(error),
        },
    };
    let exit_code = if outcome.ok { 0 } else { 1 };
    if let Err(error) = write_machine_pro_update_outcome(&request.job_id, &outcome) {
        crate::log_message(
            "error",
            &format!("[ProInstall] could not write machine update outcome: {error}"),
        );
        return Some(1);
    }
    Some(exit_code)
}

fn quote_windows_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return argument.to_string();
    }
    let mut quoted = String::from("\"");
    let mut slashes = 0usize;
    for character in argument.chars() {
        match character {
            '\\' => slashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(slashes.saturating_mul(2).saturating_add(1)));
                quoted.push('"');
                slashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(slashes));
                quoted.push(character);
                slashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(slashes.saturating_mul(2)));
    quoted.push('"');
    quoted
}

#[cfg(windows)]
fn launch_elevated_machine_pro_update(
    request: MachineProUpdateRequest,
) -> Result<serde_json::Value, String> {
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct ShellExecuteInfoW {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut std::ffi::c_void,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show: i32,
        h_inst_app: *mut std::ffi::c_void,
        lp_id_list: *mut std::ffi::c_void,
        lp_class: *const u16,
        hkey_class: *mut std::ffi::c_void,
        dw_hot_key: u32,
        h_icon_or_monitor: *mut std::ffi::c_void,
        h_process: *mut std::ffi::c_void,
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    }

    validate_machine_pro_update_request(&request)?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("elevation:could not resolve WinCommander executable: {error}"))?;
    let mut arguments = vec![
        MACHINE_PRO_UPDATE_FLAG.to_string(),
        MACHINE_PRO_UPDATE_JOB_FLAG.to_string(),
        request.job_id.clone(),
        MACHINE_PRO_UPDATE_URL_FLAG.to_string(),
        request.download_url.clone(),
        MACHINE_PRO_UPDATE_SHA256_FLAG.to_string(),
        request.expected_sha256.clone(),
        MACHINE_PRO_UPDATE_DEFENDER_CONSENT_FLAG.to_string(),
        if request.consent_defender_exclusion {
            "1"
        } else {
            "0"
        }
        .to_string(),
    ];
    if let Some(version) = &request.pro_version {
        arguments.push(MACHINE_PRO_UPDATE_VERSION_FLAG.to_string());
        arguments.push(version.clone());
    }
    let parameters = arguments
        .iter()
        .map(|argument| quote_windows_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let executable: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let parameters: Vec<u16> = parameters.encode_utf16().chain(Some(0)).collect();
    let mut info = ShellExecuteInfoW {
        cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
        f_mask: 0x0000_0040, // SEE_MASK_NOCLOSEPROCESS
        hwnd: std::ptr::null_mut(),
        lp_verb: verb.as_ptr(),
        lp_file: executable.as_ptr(),
        lp_parameters: parameters.as_ptr(),
        lp_directory: std::ptr::null(),
        n_show: 0, // SW_HIDE: this is a headless updater helper, not another UI.
        h_inst_app: std::ptr::null_mut(),
        lp_id_list: std::ptr::null_mut(),
        lp_class: std::ptr::null(),
        hkey_class: std::ptr::null_mut(),
        dw_hot_key: 0,
        h_icon_or_monitor: std::ptr::null_mut(),
        h_process: std::ptr::null_mut(),
    };
    if unsafe { ShellExecuteExW(&mut info) } == 0 || info.h_process.is_null() {
        return Err(format!(
            "elevation:Administrator permission was not granted for the shared Pro update: {}",
            std::io::Error::last_os_error()
        ));
    }

    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, WaitForSingleObject},
    };
    const WAIT_OBJECT_0: u32 = 0;
    let wait = unsafe { WaitForSingleObject(info.h_process, MACHINE_PRO_UPDATE_TIMEOUT_MS) };
    if wait != WAIT_OBJECT_0 {
        unsafe { CloseHandle(info.h_process) };
        return Err(
            "elevation:The elevated shared Pro update did not finish within five minutes."
                .to_string(),
        );
    }
    let mut exit_code = 1u32;
    let read_exit_code = unsafe { GetExitCodeProcess(info.h_process, &mut exit_code) } != 0;
    unsafe { CloseHandle(info.h_process) };
    if !read_exit_code || exit_code != 0 {
        return read_machine_pro_update_outcome(&request.job_id).map_err(|error| {
            if error.starts_with("elevation:") {
                error
            } else {
                format!("elevation:{error}")
            }
        });
    }
    read_machine_pro_update_outcome(&request.job_id)
}

#[cfg(not(windows))]
fn launch_elevated_machine_pro_update(
    _request: MachineProUpdateRequest,
) -> Result<serde_json::Value, String> {
    Err("elevation:shared Pro updates are only available on Windows".to_string())
}

#[tauri::command]
pub async fn install_pro_binary(
    download_url: String,
    expected_sha256: String,
    consent_defender_exclusion: bool,
    pro_version: Option<String>,
) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    if !crate::startup_elevation::is_current_process_elevated() {
        // Do not create a download, touch ProgramData, or attempt to close a
        // shared sidecar from a limited token.  Windows owns the consent or
        // credential decision; declining it leaves the existing artifact and
        // every user's data exactly as it was.
        let request = MachineProUpdateRequest {
            job_id: uuid::Uuid::new_v4().to_string(),
            download_url,
            expected_sha256,
            consent_defender_exclusion,
            pro_version,
        };
        return tokio::task::spawn_blocking(move || launch_elevated_machine_pro_update(request))
            .await
            .map_err(|error| format!("elevation:shared Pro update worker failed: {error}"))?;
    }

    install_pro_binary_machine(
        download_url,
        expected_sha256,
        consent_defender_exclusion,
        pro_version,
    )
    .await
}

async fn install_pro_binary_machine(
    download_url: String,
    expected_sha256: String,
    consent_defender_exclusion: bool,
    pro_version: Option<String>,
) -> Result<serde_json::Value, String> {
    // Errors are stage-prefixed so the frontend dialog can render an
    // actionable message per failure mode:
    //   "entitlement:..."         -- paid build delivery period ended
    //   "consent:..."             -- the consent flag wasn't set
    //   "validation:..."          -- bad sha256 / bad path / etc.
    //   "defender_exclusion:..."  -- Add-MpPreference failed (often
    //                                because Tamper Protection is on)
    //   "download:..."            -- HTTP fetch failed
    //   "sha256_mismatch:..."     -- byte hash didn't match the manifest
    //   "disk:..."                -- tmp create / write / fsync / rename
    crate::license::require_update_entitlement().map_err(|error| format!("entitlement:{error}"))?;

    #[cfg(windows)]
    let exclusion_already_set = defender_exclusion_already_set();
    #[cfg(not(windows))]
    let exclusion_already_set = false;

    let install_path = pro_install_path().map_err(|e| format!("validation:{}", e))?;
    let replacing_existing_install = install_path.exists();
    // Defender exclusion is an optional compatibility choice.  A signed,
    // hash-verified Pro package can be installed without reducing Defender
    // coverage; a later AV quarantine remains visible to the user as an AV
    // event, rather than silently turning into a mandatory security exception.
    let _ = replacing_existing_install;
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

    let parent = install_path
        .parent()
        .ok_or_else(|| "validation:install path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("disk:create install dir: {}", e))?;
    if install_path.exists()
        && compute_pro_sha256_at(&install_path)
            .map(|sha| sha.eq_ignore_ascii_case(&expected_sha256))
            .unwrap_or(false)
    {
        clear_disabled_markers();
        write_pro_install_metadata(pro_version.clone(), &expected_sha256)?;
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

    // 1. Defender exclusion is best-effort and only runs after explicit
    //    opt-in.  Tamper Protection or a managed Defender policy must never
    //    block the signed Pro install itself.
    let mut defender_exclusion_warning = None;
    #[cfg(windows)]
    if !exclusion_already_set && consent_defender_exclusion {
        if let Err(error) = add_defender_exclusion(true) {
            crate::log_message(
                "warn",
                &format!("[ProInstall] optional Defender exclusion was not added: {error}"),
            );
            defender_exclusion_warning = Some(error);
        }
    }

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
    clear_disabled_markers();
    write_pro_install_metadata(pro_version.clone(), &expected_sha256)?;

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
        "defender_exclusion_warning": defender_exclusion_warning,
    }))
}

#[cfg(test)]
mod machine_update_tests {
    use super::{
        parse_machine_pro_update_request, quote_windows_argument, MachineProUpdateRequest,
        MACHINE_PRO_UPDATE_DEFENDER_CONSENT_FLAG, MACHINE_PRO_UPDATE_FLAG,
        MACHINE_PRO_UPDATE_JOB_FLAG, MACHINE_PRO_UPDATE_SHA256_FLAG, MACHINE_PRO_UPDATE_URL_FLAG,
        MACHINE_PRO_UPDATE_VERSION_FLAG,
    };

    fn valid_args() -> Vec<String> {
        vec![
            MACHINE_PRO_UPDATE_FLAG.into(),
            MACHINE_PRO_UPDATE_JOB_FLAG.into(),
            "c4c52637-6d5a-4f05-9a66-c7091dba2b33".into(),
            MACHINE_PRO_UPDATE_URL_FLAG.into(),
            "https://winupdates.servalabs.com/pro/wincommander-pro.exe".into(),
            MACHINE_PRO_UPDATE_SHA256_FLAG.into(),
            "a".repeat(64),
            MACHINE_PRO_UPDATE_DEFENDER_CONSENT_FLAG.into(),
            "1".into(),
            MACHINE_PRO_UPDATE_VERSION_FLAG.into(),
            "3.6.2".into(),
        ]
    }

    #[test]
    fn machine_update_helper_accepts_only_the_validated_fixed_argument_set() {
        let request = parse_machine_pro_update_request(&valid_args())
            .expect("valid helper arguments")
            .expect("helper invocation");
        assert_eq!(request.pro_version.as_deref(), Some("3.6.2"));
        assert!(request.consent_defender_exclusion);

        let mut unknown = valid_args();
        unknown.extend(["--unexpected".into(), "value".into()]);
        assert!(parse_machine_pro_update_request(&unknown).is_err());

        let mut duplicate = valid_args();
        duplicate.extend([
            MACHINE_PRO_UPDATE_JOB_FLAG.into(),
            "c4c52637-6d5a-4f05-9a66-c7091dba2b33".into(),
        ]);
        assert!(parse_machine_pro_update_request(&duplicate).is_err());
    }

    #[test]
    fn machine_update_helper_rejects_non_pinned_url_before_any_machine_write() {
        let mut args = valid_args();
        let url_index = args
            .iter()
            .position(|argument| argument == MACHINE_PRO_UPDATE_URL_FLAG)
            .unwrap()
            + 1;
        args[url_index] = "https://example.invalid/wincommander-pro.exe".into();
        assert!(parse_machine_pro_update_request(&args).is_err());
    }

    #[test]
    fn uac_helper_arguments_quote_paths_without_shell_interpretation() {
        assert_eq!(
            quote_windows_argument("--machine-pro-update"),
            "--machine-pro-update"
        );
        assert_eq!(
            quote_windows_argument(r#"C:\Path With Spaces\pro.exe"#),
            r#""C:\Path With Spaces\pro.exe""#,
        );
    }

    #[test]
    fn request_shape_keeps_the_machine_update_inputs_explicit() {
        let request = MachineProUpdateRequest {
            job_id: "c4c52637-6d5a-4f05-9a66-c7091dba2b33".into(),
            download_url: "https://winupdates.servalabs.com/pro/wincommander-pro.exe".into(),
            expected_sha256: "b".repeat(64),
            consent_defender_exclusion: false,
            pro_version: None,
        };
        assert!(!request.consent_defender_exclusion);
        assert!(request.pro_version.is_none());
    }
}
