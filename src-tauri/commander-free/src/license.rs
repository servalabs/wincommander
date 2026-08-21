use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const OFFLINE_GRACE_SECONDS: u64 = 7 * 24 * 60 * 60;
#[cfg_attr(feature = "portable", allow(dead_code))] // portable edition has no trial
#[cfg(test)]
const TRIAL_DURATION_SECONDS: u64 = 16 * 24 * 60 * 60; // 16 days
                                                       // Mainline ServaLabs verification material. The Ed25519 public key is public
                                                       // by design and is embedded in every release binary; only its matching private
                                                       // key remains in Cloudflare secrets. Build-time env vars still take precedence
                                                       // for OEM builds and planned key rotations.
const DEFAULT_LICENSE_API_BASE: &str = "https://wincommander-licensing.servalabs.com";
const DEFAULT_LICENSE_PUBLIC_KEY_B64: &str = "Z4ulYtrFLpOZYVLGpNo_PlegZsitTmJx2JwfyIqSJpY";

// White-label: the app identifier sent to the licence worker in
// activate/refresh/deactivate. Defaults to the ServaLabs identifier; an OEM
// build overrides it at compile time via the WINCMD_APP_ID env var (mirrors the
// WINCMD_LICENSE_* embedding in get_config). Build-time only — a release build
// bakes one fixed value; runtime/env cannot change it. The licence worker the
// build talks to must be configured with the same APP_ID.
const APP_ID: &str = match option_env!("WINCMD_APP_ID") {
    Some(v) => v,
    None => "com.servalabs.wincommander",
};
#[cfg(debug_assertions)]
static DOTENV_LOADED: OnceLock<()> = OnceLock::new();

// Cached once per process — spawning powershell.exe per call was wasteful.
static IS_PORTABLE: OnceLock<bool> = OnceLock::new();

// Device hash is stable for the process lifetime; cache it so repeated license
// checks don't re-spawn the WMI/CIM probes.
static DEVICE_HASH: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignedTokenEnvelope {
    payload: String,
    signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedLicense {
    token: SignedTokenEnvelope,
    last_verified_at: u64,
    #[serde(default)]
    seats_used: Option<u32>,
    #[serde(default)]
    seat_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LicenseClaims {
    #[serde(default)]
    license_id: Option<String>,
    device_hash: String,
    // Default: tokens minted before the `plan` claim existed carry no such
    // field; a missing-field deserialize error would fail-closed and lock out
    // existing paid customers. Absent `plan` == "" (not "trial"), which the
    // trial-aware status/entitlement logic treats as a normal licence.
    #[serde(default)]
    plan: String,
    /// Permanent local-software rights, independent of hosted-service terms.
    #[serde(default)]
    base_features: Vec<String>,
    /// Time-limited hosted or specialist rights (Investigator, Fleet, Netwall).
    #[serde(default)]
    service_features: Vec<String>,
    #[serde(default)]
    service_exp: Option<u64>,
    /// Legacy combined vector. New claims use the split vectors above.
    #[serde(default)]
    features: Vec<String>,
    iat: u64,
    exp: u64,
    pub license_exp: Option<u64>,
    iss: Option<String>,
    sub: Option<String>,
}

#[derive(Debug, Serialize)]
struct LicenseStatus {
    configured: bool,
    licensed: bool,
    valid: bool,
    reason: Option<String>,
    plan: Option<String>,
    features: Vec<String>,
    /// Permanent local-software rights from the signed claim.
    base_features: Vec<String>,
    /// Term services that are active at the time this status is built.
    active_service_features: Vec<String>,
    /// Product entitlement end, if the product itself is time-limited.
    entitlement_expires_at: Option<u64>,
    /// Hosted/specialist service end, independent of retained Pro rights.
    service_expires_at: Option<u64>,
    expires_at: Option<u64>,
    last_verified_at: Option<u64>,
    grace_until: Option<u64>,
    device_hash: String,
    seats_used: Option<u32>,
    seat_limit: Option<u32>,
    /// True when running as Windows To Go (portable OS from USB drive).
    is_portable: bool,
    /// Whether a free trial is currently active.
    trial_active: bool,
    /// Unix timestamp when the trial expires (if active).
    trial_expires_at: Option<u64>,
    /// True when no trial has ever been started on this device (trial is still claimable).
    trial_available: bool,
    /// True when a trial was started but has since expired. Features still work (soft expiry).
    trial_expired: bool,
}

#[derive(Debug, Deserialize)]
struct WorkerTokenResponse {
    ok: bool,
    payload: Option<String>,
    signature: Option<String>,
    error: Option<String>,
    #[serde(default)]
    entitlement_revoked: bool,
    #[serde(rename = "seatsUsed")]
    seats_used: Option<u32>,
    #[serde(rename = "seatLimit")]
    seat_limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct WorkerTrialStatusResponse {
    ok: bool,
    available: Option<bool>,
}

fn worker_rejection(payload: &WorkerTokenResponse, fallback: &str) -> (bool, String) {
    if payload.entitlement_revoked {
        return (true, "License entitlement has been revoked.".to_string());
    }
    (
        false,
        payload
            .error
            .clone()
            .unwrap_or_else(|| fallback.to_string()),
    )
}

#[derive(Debug, Serialize)]
struct WorkerActivateRequest<'a> {
    #[serde(rename = "licenseKey")]
    license_key: &'a str,
    #[serde(rename = "deviceHash")]
    device_hash: &'a str,
    #[serde(rename = "appId")]
    app_id: &'a str,
    #[serde(rename = "appVersion")]
    app_version: &'a str,
    #[serde(rename = "isPortable")]
    is_portable: bool,
}

#[derive(Debug, Serialize)]
struct WorkerRefreshRequest<'a> {
    payload: &'a str,
    signature: &'a str,
    #[serde(rename = "deviceHash")]
    device_hash: &'a str,
    #[serde(rename = "appId")]
    app_id: &'a str,
    #[serde(rename = "appVersion")]
    app_version: &'a str,
    #[serde(rename = "isPortable")]
    is_portable: bool,
}

#[derive(Debug, Serialize)]
struct WorkerDeactivateRequest<'a> {
    payload: &'a str,
    signature: &'a str,
    #[serde(rename = "deviceHash")]
    device_hash: &'a str,
    #[serde(rename = "appId")]
    app_id: &'a str,
}

#[cfg_attr(feature = "portable", allow(dead_code))] // portable edition has no trial
#[derive(Debug, Serialize)]
struct WorkerTrialRequest<'a> {
    #[serde(rename = "deviceHash")]
    device_hash: &'a str,
    #[serde(rename = "appId")]
    app_id: &'a str,
    #[serde(rename = "appVersion")]
    app_version: &'a str,
    #[serde(rename = "isPortable")]
    is_portable: bool,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

/// Write `data` to `path` atomically: write to a sibling temp file,
/// fsync it, then rename over the target. This prevents the license
/// cache and trial record from being torn / corrupted if the app
/// crashes or the host loses power mid-write — the prior plain
/// `fs::write` call could leave a half-written file that the next
/// load would refuse to parse, locking the user out of their
/// licence/trial state until they re-activated.
fn atomic_write(path: &PathBuf, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| format!("Path has no parent directory: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("Path has no file name: {}", path.display()))?;
    let tmp_path = parent.join(format!(".{}.tmp", file_name));

    {
        let mut tmp = fs::File::create(&tmp_path)
            .map_err(|e| format!("Failed to open temp file {}: {}", tmp_path.display(), e))?;
        tmp.write_all(data)
            .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
        tmp.sync_all()
            .map_err(|e| format!("Failed to fsync temp file {}: {}", tmp_path.display(), e))?;
    }

    fs::rename(&tmp_path, path).map_err(|e| {
        // Clean up the temp file if rename failed so we don't leak it.
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to atomically replace {}: {}", path.display(), e)
    })
}

// ── Windows To Go (WTG) detection ────────────────────────────────────
// Windows sets HKLM\...\Control\PortableOperatingSystem = 1 on every WTG
// boot. This is the authoritative flag; Get-Disk BusType=USB can misfire on
// USB-docked SSDs. Result is cached for the process lifetime via IS_PORTABLE.
#[cfg(windows)]
fn is_portable_os() -> bool {
    *IS_PORTABLE.get_or_init(|| {
        let mut cmd = Command::new("powershell");
        cmd.creation_flags(CREATE_NO_WINDOW);
        let output = cmd
            .args([
                "-NoProfile",
                "-Command",
                "(Get-ItemProperty -Path \
                 'HKLM:\\SYSTEM\\CurrentControlSet\\Control' \
                 -Name PortableOperatingSystem \
                 -ErrorAction SilentlyContinue).PortableOperatingSystem -eq 1",
            ])
            .output();
        matches!(output, Ok(out) if
            String::from_utf8_lossy(&out.stdout).trim() == "True")
    })
}

#[cfg(not(windows))]
fn is_portable_os() -> bool {
    false
}

/// Return the volume serial number of the system drive (e.g. `C:`) as an
/// 8-char uppercase hex string. Used as the stable HWID for WTG installs.
#[cfg(windows)]
fn get_system_drive_serial() -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
    let drive = std::env::var("SYSTEMDRIVE").unwrap_or_else(|_| "C:".to_string());
    let root: Vec<u16> = format!("{}\\\0", drive).encode_utf16().collect();
    let mut serial: u32 = 0;
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 {
        Some(format!("{:08X}", serial))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn get_system_drive_serial() -> Option<String> {
    None
}

/// Backend gate — refuses if the caller doesn't have a paid entitlement.
/// Used by run_backend_script before dispatching any command tagged
/// `tier: "paid"` in the command tier map. Returns Err with a UI-safe
/// message that the frontend can surface (the LockedToggle / Paywall
/// already prevents this from happening through the normal UI; this
/// is the defence-in-depth backstop).
pub fn require_paid(feature: &str) -> Result<(), String> {
    if has_paid_entitlement() {
        Ok(())
    } else {
        Err(format!(
            "WinCommander Pro entitlement required for: {}. Activate a license or start the 16-day free trial.",
            feature
        ))
    }
}

/// Reads and verifies the current entitlement vector for this device.
/// Returns the features the device is entitled to (e.g. ["paid"] or
/// ["paid", "advanced"]) or an empty vec if no valid licence/trial.
///
/// Returns ["paid"] for an active 16-day trial — the trial is a worker-signed
/// token (plan "trial", features ["paid"]) cached exactly like a paid
/// licence, so it unlocks every paid feature via the SAME verification path
/// a paid licence uses. Returns the JWT's `features` claim for a valid paid
/// licence. Returns [] when the cached licence/trial token can't be
/// verified, the device hash no longer matches, or the token has expired
/// beyond the offline grace window — this includes a lapsed trial, which no
/// longer grants anything once its token expires.
pub fn current_features() -> Vec<String> {
    current_features_raw()
}

/// The cached licence token is the SOLE source of entitlement truth here —
/// for BOTH a paid licence and a server-issued 16-day trial (see
/// `start_trial()`, which stores the worker-signed trial token in the SAME
/// `license_cache.json` a real licence uses). The Pro sidecar
/// (`entitlement.rs`) reads that identical file, so Free and Pro can never
/// disagree about whether a trial is still active — unlike the old design,
/// where the trial was purely local and Pro had no way to see it at all.
///
/// Older releases short-circuited on a local trial file's mere existence,
/// so a lapsed trial could keep paid features unlocked. That local file has
/// been removed; the signed token's `exp` claim is the only trial gate.
fn current_features_raw() -> Vec<String> {
    let Ok((_, public_key_b64)) = get_config() else {
        return Vec::new();
    };
    let Ok(Some(cached)) = load_cached_license() else {
        return Vec::new();
    };
    let Ok(claims) = parse_and_verify_claims(
        &cached.token.payload,
        &cached.token.signature,
        &public_key_b64,
    ) else {
        return Vec::new();
    };

    if claims.device_hash != current_device_hash() {
        return Vec::new();
    }

    features_from_verified_claims(&claims, cached.last_verified_at, now_unix())
}

/// Pure decision step, split out of `current_features_raw()` so the expiry
/// logic — the exact thing that regressed when this function used to check
/// `trial.json`'s mere existence instead of the token's `exp` — is
/// unit-testable without a real cached-license file or a live device-hash
/// probe. `claims` must already be signature- and device-hash-verified by
/// the caller; this only decides what a claim set that expired at `exp`,
/// last verified against the server at `last_verified_at`, grants at `now`.
fn features_from_verified_claims(
    claims: &LicenseClaims,
    last_verified_at: u64,
    now: u64,
) -> Vec<String> {
    // A trial is never granted a permanent base entitlement. Its signed token
    // remains the complete authority and expires exactly at `exp`.
    if claims.plan == "trial" {
        let trial_features = if claims.service_features.is_empty() {
            &claims.features
        } else {
            &claims.service_features
        };
        return if claims.exp > now {
            trial_features.clone()
        } else {
            Default::default()
        };
    }

    let has_split_features = !claims.base_features.is_empty()
        || !claims.service_features.is_empty()
        || claims.service_exp.is_some();
    if has_split_features {
        let mut features = claims.base_features.clone();
        if claims
            .service_exp
            .is_some_and(|service_exp| service_exp > now)
        {
            features.extend(claims.service_features.iter().cloned());
        }
        return features;
    }

    let in_grace = now <= last_verified_at.saturating_add(OFFLINE_GRACE_SECONDS);
    if claims.exp <= now && !in_grace {
        return Vec::new();
    }

    claims.features.clone()
}

/// True if the device has a valid paid entitlement OR an active 16-day trial.
/// "Paid entitlement" = the licence JWT's `features` vector contains
/// "paid". Used by gates
/// that don't care about specific feature names — just "is this user paid?"
pub fn has_paid_entitlement() -> bool {
    let features = current_features();
    features.iter().any(|f| f == "paid")
}

fn feature_vector_has_service(features: &[String], name: &str) -> bool {
    features.iter().any(|feature| feature == name)
}

/// True only when a currently active signed service claim contains `name`.
/// Permanent normal-Pro fallback (`paid`) never implies hosted/specialist
/// services such as Fleet, Investigator, or Netwall.
pub fn has_service_feature(name: &str) -> bool {
    feature_vector_has_service(&current_features(), name)
}

/// Central backend gate for term-based service capabilities.
pub fn require_service_feature(name: &str) -> Result<(), String> {
    if has_service_feature(name) {
        Ok(())
    } else {
        Err(format!(
            "An active {} service subscription is required.",
            name
        ))
    }
}

// ── Emergency self-destruct licence grace (owner policy 2026-07-01) ───
//
// The emergency wipe (destroy PIN / distress phrase / dead-man switch)
// must protect the user even if their Pro licence lapsed — refusing a
// duress wipe can get someone hurt. But an expired licence is not an
// unlimited free pass either. Policy: grant the full deep wipe ONCE on an
// expired licence, then refuse the Pro-only steps until renewal. A renewal
// (valid entitlement seen here) clears the marker so the one-time grace is
// available again for the NEXT lapse — "once per lapse", not "once ever".

fn emergency_grace_marker_path() -> Option<std::path::PathBuf> {
    crate::paths::machine_data_dir()
        .ok()
        .map(|d| d.join(".emergency_grace_used"))
}

/// True once the one-time expired-licence emergency grace has been consumed.
pub fn emergency_grace_used() -> bool {
    emergency_grace_marker_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

fn mark_emergency_grace_used() {
    if let Some(p) = emergency_grace_marker_path() {
        let _ = std::fs::write(&p, b"1");
    }
}

fn clear_emergency_grace() {
    if let Some(p) = emergency_grace_marker_path() {
        let _ = std::fs::remove_file(&p);
    }
}

/// Licence posture for the emergency destruct cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmergencyLicense {
    /// Valid paid entitlement (or trial soft-pass) — run everything.
    Licensed,
    /// Expired paid licence, first emergency since expiry — one-time grace granted.
    GraceGranted,
    /// Expired paid licence, grace already consumed — refuse the Pro-only steps.
    GraceExhausted,
    /// Pro sidecar not installed — the deep steps cannot run at all.
    ProMissing,
}

/// Decide (and, on the grace path, CONSUME) the emergency-wipe licence grace.
/// Call ONCE at the start of a cascade. A valid entitlement clears the marker
/// so the grace resets per lapse.
pub fn emergency_license_gate() -> EmergencyLicense {
    if !crate::pro_install::pro_is_installed() {
        return EmergencyLicense::ProMissing;
    }
    if has_paid_entitlement() {
        clear_emergency_grace();
        return EmergencyLicense::Licensed;
    }
    if emergency_grace_used() {
        EmergencyLicense::GraceExhausted
    } else {
        mark_emergency_grace_used();
        EmergencyLicense::GraceGranted
    }
}

/// True if the device is entitled to a specific named feature. The ordinary
/// "paid" entitlement grants paid features, except the investigator-only
/// "advanced" feature, which must be explicit in the signed feature vector.
#[allow(dead_code)]
pub fn has_entitlement(name: &str) -> bool {
    has_entitlement_in_features(&current_features(), name)
}

/// Feature matching for a verified entitlement vector. Keep specialist
/// services explicit: ordinary Pro's broad `paid` claim must not become a
/// download or launch credential for Investigator.
fn has_entitlement_in_features(features: &[String], name: &str) -> bool {
    let is_term_service = matches!(name, "advanced" | "fleet" | "netwall");
    features
        .iter()
        .any(|f| f == name || (!is_term_service && f == "paid"))
}

/// True when advanced mode is BOTH licence-eligible AND
/// the operator has explicitly opted in via the Settings diagnostics toggle.
///
/// When true, the rest of the binary must behave as an analysis tool:
///   - The dispatch layer (`run_backend_script`) refuses every Clear-* /
///     Erase-* / Remove-* PowerShell command (A1-c).
///   - The case-session gate (A2) must be open before any artifact read.
///   - The hash-chained audit log (A3) is active and stamps every
///     operation with the active licence's Ed25519 fingerprint.
///   - The Pro sidecar is not invoked at all (A1-e) — Pro features are
///     clear-shaped and would taint evidence.
///
/// The two conditions are:
///
/// 1. **Eligibility** — licence features array contains the literal
///    "advanced" claim (NOT granted by the ordinary "paid" feature;
///    the 16-day trial gives "paid" but never advanced).
///
/// 2. **Consent** — `settings.identity.advancedToolsEnabled == true`,
///    the same Settings diagnostics toggle that surfaces the Investigator panel
///    in the sidebar. Without explicit consent, the kill-switch stays
///    off so the operator's day-to-day commands (RAM disk dismount,
///    cache clears, etc.) keep working. Turning the toggle off
///    deactivates the advanced gate immediately — no app restart.
///
/// Earlier versions checked only condition 1, which meant any licence
/// carrying the "advanced" claim permanently armed the kill-switch
/// with no way for the user to step out of advanced mode without
/// switching licences. The toggle now gates both panel visibility AND
/// the dispatch refusal, making the UX coherent.
pub fn is_advanced_mode() -> bool {
    // Eligibility first — if the licence doesn't qualify, no toggle in
    // the world arms advanced mode. Cheap check; short-circuits early.
    let eligible = current_features().iter().any(|f| f == "advanced");
    if !eligible {
        return false;
    }

    // Consent — the user has flipped the Settings diagnostics toggle. Reading
    // settings is cached after first hit (see settings::read_settings),
    // so the per-dispatch cost is a HashMap lookup on the hot path.
    // On read failure we default to OFF: keeping the user functional
    // is preferred over silently arming a kill-switch they can't see.
    // The toggle is in `ideal.identity` — that's the user-desired state
    // section the Settings UI patches. `current` holds observed system
    // state, which doesn't apply here.
    match crate::settings::read_settings() {
        Ok(s) => s.ideal.identity.advanced_tools_enabled.unwrap_or(false),
        Err(_) => false,
    }
}

fn decode_b64_any(input: &str) -> Result<Vec<u8>, String> {
    general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .or_else(|_| general_purpose::URL_SAFE.decode(input))
        .or_else(|_| general_purpose::STANDARD.decode(input))
        .map_err(|e| format!("Failed to decode base64 value: {}", e))
}

fn pick_non_empty(values: &[&'static str]) -> &'static str {
    values
        .iter()
        .copied()
        .find(|v| !v.trim().is_empty())
        .unwrap_or("")
}

// F-2: dotenv loading + env-var override are confined to debug builds.
// In release builds, the licence API base and verification public key
// are sourced exclusively from the build-time `option_env!()` embed
// (see get_config). This prevents the "drop a .env next to the EXE to
// re-key licence verification" bypass — see ref/security-audit-report.md.
#[cfg(debug_assertions)]
fn ensure_dotenv_loaded() {
    let _ = DOTENV_LOADED.get_or_init(|| {
        let _ = dotenvy::from_filename(".env");
        let _ = dotenvy::from_filename("../.env");
    });
}

#[cfg(debug_assertions)]
fn runtime_or_build(var_name: &str, build_fallback: &'static str) -> String {
    ensure_dotenv_loaded();
    std::env::var(var_name).unwrap_or_else(|_| build_fallback.to_string())
}

// Release builds: ignore the runtime environment entirely for licence
// config. The build-time embed is the single source of truth.
#[cfg(not(debug_assertions))]
fn runtime_or_build(_var_name: &str, build_fallback: &'static str) -> String {
    build_fallback.to_string()
}

fn get_config() -> Result<(String, String), String> {
    // Do not silently point an OEM app id at the ServaLabs tenant. OEM builds
    // remain fail-closed unless they provide their own endpoint and public key.
    let mainline_api_default = if APP_ID == "com.servalabs.wincommander" {
        DEFAULT_LICENSE_API_BASE
    } else {
        ""
    };
    let mainline_key_default = if APP_ID == "com.servalabs.wincommander" {
        DEFAULT_LICENSE_PUBLIC_KEY_B64
    } else {
        ""
    };
    let build_license_api_base = pick_non_empty(&[
        option_env!("WINCMD_LICENSE_API_BASE").unwrap_or(""),
        option_env!("LICENSE_API_BASE").unwrap_or(""),
        mainline_api_default,
    ]);
    let build_license_public_key_b64 = pick_non_empty(&[
        option_env!("WINCMD_LICENSE_PUBLIC_KEY").unwrap_or(""),
        option_env!("LICENSE_PUBLIC_KEY_B64").unwrap_or(""),
        mainline_key_default,
    ]);

    let runtime_license_api_base =
        runtime_or_build("WINCMD_LICENSE_API_BASE", build_license_api_base);
    let runtime_license_public_key_b64 =
        runtime_or_build("WINCMD_LICENSE_PUBLIC_KEY", build_license_public_key_b64);
    let license_api_base = runtime_license_api_base.trim().to_string();
    let license_public_key_b64 = runtime_license_public_key_b64.trim().to_string();

    if license_api_base.trim().is_empty() || license_public_key_b64.trim().is_empty() {
        return Err(
            "Licensing is not configured. Set WINCMD_LICENSE_API_BASE and WINCMD_LICENSE_PUBLIC_KEY (or LICENSE_PUBLIC_KEY_B64) at build time."
                .to_string(),
        );
    }
    Ok((
        license_api_base.trim_end_matches('/').to_string(),
        license_public_key_b64,
    ))
}

pub(crate) fn license_api_base() -> Result<String, String> {
    get_config().map(|(base, _)| base)
}

#[cfg(not(feature = "portable"))]
async fn trial_available_from_worker(api_base: &str, device_hash: &str) -> Option<bool> {
    let client = crate::net::doh_http_client().ok()?;
    let response = client
        .post(format!("{}/trial/status", api_base))
        .json(&WorkerTrialRequest {
            device_hash,
            app_id: APP_ID,
            app_version: env!("CARGO_PKG_VERSION"),
            is_portable: false,
        })
        .send()
        .await
        .ok()?;
    let body = response.json::<WorkerTrialStatusResponse>().await.ok()?;
    body.ok.then_some(body.available.unwrap_or(false))
}

fn license_file_path() -> Result<PathBuf, String> {
    // Per-MACHINE cache (%ProgramData%\WinCommander) so ONE activation covers
    // every Windows account on the device. Writes still require the existing
    // machine-data ACL; standard-user sessions are not elevated by default.
    // Was per-user %APPDATA% — that gave each account its own (un)licensed state.
    let mut path = crate::paths::machine_data_dir()?;
    path.push("license_cache.json");
    Ok(path)
}

fn load_cached_license() -> Result<Option<CachedLicense>, String> {
    let path = license_file_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read cached license token: {}", e))?;
    let parsed: CachedLicense =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid cached license format: {}", e))?;
    Ok(Some(parsed))
}

fn save_cached_license(cached: &CachedLicense) -> Result<(), String> {
    let path = license_file_path()?;
    let data = serde_json::to_string_pretty(cached)
        .map_err(|e| format!("Failed to encode cached license token: {}", e))?;
    atomic_write(&path, data.as_bytes())
}

fn clear_cached_license_inner() -> Result<(), String> {
    let path = license_file_path()?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|e| format!("Failed to clear cached license token: {}", e))?;
    }
    Ok(())
}

#[cfg(windows)]
fn get_motherboard_uuid() -> String {
    // wmic.exe is removed by default on Windows 11 / ReviOS, so use the CIM
    // cmdlets (backed by the WMI service, which stays present) instead — an
    // empty serial here would collapse every machine to the same device hash.
    let mut cmd = Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance -ClassName Win32_ComputerSystemProduct).UUID",
        ])
        .output();
    if let Ok(out) = output {
        return String::from_utf8_lossy(&out.stdout).trim().to_string();
    }
    String::new()
}

#[cfg(windows)]
fn get_primary_disk_serial() -> String {
    // Disk index 0 = the system disk on the vast majority of machines.
    // Get-CimInstance replaces the removed wmic.exe (see get_motherboard_uuid).
    let mut cmd = Command::new("powershell");
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance -ClassName Win32_DiskDrive | Where-Object { $_.Index -eq 0 }).SerialNumber",
        ])
        .output();
    if let Ok(out) = output {
        return String::from_utf8_lossy(&out.stdout).trim().to_string();
    }
    String::new()
}

pub fn current_device_hash() -> String {
    // Memoized — identity is stable for the process lifetime and the Windows
    // probes spawn powershell, too costly to repeat on every license check.
    // Exposed crate-wide so startup_auth can salt the PIN KDF with it.
    DEVICE_HASH
        .get_or_init(|| {
            let mut hasher = Sha256::new();
            hasher.update("wincommander-license-v1");
            if is_portable_os() {
                // Windows To Go: bind the license to the USB drive's volume serial instead
                // of host-machine identifiers so the key roams with the drive.
                hasher.update("|WTG|");
                if let Some(serial) = get_system_drive_serial() {
                    hasher.update(&serial);
                }
            } else {
                #[cfg(windows)]
                {
                    hasher.update("|MB|");
                    hasher.update(get_motherboard_uuid());
                    hasher.update("|DS|");
                    hasher.update(get_primary_disk_serial());
                }
                #[cfg(not(windows))]
                {
                    for key in [
                        "COMPUTERNAME",
                        "USERDOMAIN",
                        "PROCESSOR_IDENTIFIER",
                        "NUMBER_OF_PROCESSORS",
                        "OS",
                    ] {
                        hasher.update("|");
                        hasher.update(std::env::var(key).unwrap_or_default());
                    }
                }
            }
            let digest = hasher.finalize();
            digest.iter().map(|b| format!("{:02x}", b)).collect()
        })
        .clone()
}

fn parse_and_verify_claims(
    payload: &str,
    signature_b64: &str,
    public_key_b64: &str,
) -> Result<LicenseClaims, String> {
    let key_bytes = decode_b64_any(public_key_b64)?;
    let signature_bytes = decode_b64_any(signature_b64)?;

    let key_arr: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| {
            "Public key must decode to exactly 32 bytes Ed25519 key material. Minisign-formatted Tauri updater keys are not directly supported for licensing signatures."
                .to_string()
        })?;
    let sig_arr: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Signature must be exactly 64 bytes".to_string())?;

    let verifying_key =
        VerifyingKey::from_bytes(&key_arr).map_err(|e| format!("Invalid public key: {}", e))?;
    let signature = Signature::from_bytes(&sig_arr);

    verifying_key
        .verify(payload.as_bytes(), &signature)
        .map_err(|e| format!("License signature validation failed: {}", e))?;

    serde_json::from_str(payload).map_err(|e| format!("Invalid signed payload: {}", e))
}

fn status_from_cached(
    cached: &CachedLicense,
    claims: &LicenseClaims,
    reason: Option<String>,
) -> LicenseStatus {
    let now = now_unix();
    let features = features_from_verified_claims(claims, cached.last_verified_at, now);
    let valid = !features.is_empty();
    let grace_until = cached
        .last_verified_at
        .saturating_add(OFFLINE_GRACE_SECONDS);

    // Choose the true license expiration for display if available
    let display_exp = claims.license_exp.unwrap_or(claims.exp);

    // A server-issued trial token (see start_trial()) is cached in the SAME
    // license_cache.json a paid licence uses and flows through this exact
    // function — the only marker distinguishing it is the `plan` claim the
    // worker mints as "trial". Surface that as trial_active/trial_expires_at
    // so the UI keeps showing the trial banner/countdown after a restart,
    // instead of rendering it as an indistinguishable ordinary licence.
    let is_trial = claims.plan == "trial";
    let is_term_service = |feature: &str| matches!(feature, "advanced" | "fleet" | "netwall");
    let (base_features, active_service_features) = if is_trial {
        (Vec::new(), features.clone())
    } else {
        (
            features
                .iter()
                .filter(|feature| !is_term_service(feature))
                .cloned()
                .collect(),
            features
                .iter()
                .filter(|feature| is_term_service(feature))
                .cloned()
                .collect(),
        )
    };

    LicenseStatus {
        configured: true,
        licensed: true,
        valid,
        reason,
        plan: Some(claims.plan.clone()),
        features,
        base_features,
        active_service_features,
        entitlement_expires_at: claims.license_exp,
        service_expires_at: claims.service_exp,
        expires_at: Some(display_exp),
        last_verified_at: Some(cached.last_verified_at),
        grace_until: Some(grace_until),
        device_hash: current_device_hash(),
        seats_used: cached.seats_used,
        seat_limit: cached.seat_limit,
        is_portable: is_portable_os(),
        trial_active: is_trial && valid,
        trial_expires_at: if is_trial { Some(display_exp) } else { None },
        trial_available: false,
        trial_expired: is_trial && !valid,
    }
}

// `ensure_license_gate()` was removed in Phase 4 of the tier-split rollout.
// The Free binary launches without a license; per-feature paywall is enforced
// at the call site via license::require_paid (paid commands) and
// license::has_entitlement (granular checks). See the rollout plan at
// `ref/roadmap.md` (Part 1 — Active Rollout Plan).

#[tauri::command]
pub async fn get_license_status() -> Result<serde_json::Value, String> {
    get_license_status_inner().await
}

async fn get_license_status_inner() -> Result<serde_json::Value, String> {
    crate::log_message("debug", "[License] Checking license status...");
    let device_hash = current_device_hash();
    let config = get_config();
    if config.is_err() {
        let status = LicenseStatus {
            configured: false,
            licensed: false,
            valid: false,
            reason: Some(
                "No active license. Activate a license to unlock paid features.".to_string(),
            ),
            plan: None,
            features: vec![],
            base_features: vec![],
            active_service_features: vec![],
            entitlement_expires_at: None,
            service_expires_at: None,
            expires_at: None,
            last_verified_at: None,
            grace_until: None,
            device_hash,
            seats_used: None,
            seat_limit: None,
            is_portable: is_portable_os(),
            trial_active: false,
            trial_expires_at: None,
            trial_available: false,
            trial_expired: false,
        };
        return serde_json::to_value(status).map_err(|e| e.to_string());
    }

    let (api_base, public_key_b64) = config?;
    let cached = load_cached_license()?;
    let Some(cached) = cached else {
        #[cfg(feature = "portable")]
        let trial_available = false;
        #[cfg(not(feature = "portable"))]
        let trial_available = trial_available_from_worker(&api_base, &device_hash)
            .await
            .unwrap_or(false);
        let status = LicenseStatus {
            configured: true,
            licensed: false,
            valid: false,
            reason: Some("No active license.".to_string()),
            plan: None,
            features: vec![],
            base_features: vec![],
            active_service_features: vec![],
            entitlement_expires_at: None,
            service_expires_at: None,
            expires_at: None,
            last_verified_at: None,
            grace_until: None,
            device_hash,
            seats_used: None,
            seat_limit: None,
            is_portable: is_portable_os(),
            trial_active: false,
            trial_expires_at: None,
            trial_available,
            trial_expired: false,
        };
        return serde_json::to_value(status).map_err(|e| e.to_string());
    };

    let claims = match parse_and_verify_claims(
        &cached.token.payload,
        &cached.token.signature,
        &public_key_b64,
    ) {
        Ok(c) => c,
        Err(e) => {
            // Token signature doesn't verify with the current build key.
            // Do NOT erase the cache here — the token may still be usable after a key fix.
            // Return a non-valid status so the UI shows the problem without destroying the token.
            let status = LicenseStatus {
                configured: true,
                licensed: true,
                valid: false,
                reason: Some(format!(
                    "Token verification failed (key mismatch): {}. Use Refresh to recover or re-activate.",
                    e
                )),
                plan: None,
                features: vec![],
                base_features: vec![],
                active_service_features: vec![],
                entitlement_expires_at: None,
                service_expires_at: None,
                expires_at: None,
                last_verified_at: None,
                grace_until: None,
                device_hash,
                seats_used: None,
                seat_limit: None,
                is_portable: is_portable_os(),
                trial_active: false,
                trial_expires_at: None,
                trial_available: false,
                trial_expired: false,
            };
            return serde_json::to_value(status).map_err(|e| e.to_string());
        }
    };
    let mut status = status_from_cached(&cached, &claims, None);
    status.device_hash = device_hash;

    let now = now_unix();
    if !status.valid {
        if let Some(grace_until) = status.grace_until {
            if now > grace_until {
                status.reason =
                    Some("License token expired and offline grace window ended.".to_string());
            } else {
                status.reason =
                    Some("License expired; running within offline grace window.".to_string());
            }
        }
    }

    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn activate_license(license_key: String) -> Result<serde_json::Value, String> {
    let (api_base, public_key_b64) = get_config()?;
    let trimmed = license_key.trim();
    if trimmed.is_empty() {
        return Err("License key is required.".to_string());
    }

    let device_hash = current_device_hash();
    // DoH-aware client routes name resolution through Cloudflare so an
    // ISP-level DNS block on the license host (same problem we hit for
    // the update host) doesn't kill activation.
    let client = crate::net::doh_http_client()?;

    let response = client
        .post(format!("{}/license/activate", api_base))
        .json(&WorkerActivateRequest {
            license_key: trimmed,
            device_hash: &device_hash,
            app_id: APP_ID,
            app_version: env!("CARGO_PKG_VERSION"),
            is_portable: is_portable_os(),
        })
        .send()
        .await
        .map_err(|e| {
            crate::log_message(
                "error",
                &format!("[License] Activation request failed: {}", e),
            );
            format!("License server request failed: {}", e)
        })?;

    let payload: WorkerTokenResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid license server response: {}", e))?;

    if !payload.ok {
        let (clear_cache, message) =
            worker_rejection(&payload, "License server rejected activation.");
        if clear_cache {
            let _ = clear_cached_license_inner();
        }
        return Err(message);
    }

    let signed_payload = payload
        .payload
        .ok_or_else(|| "License server did not return payload.".to_string())?;
    let signature = payload
        .signature
        .ok_or_else(|| "License server did not return signature.".to_string())?;

    let claims = parse_and_verify_claims(&signed_payload, &signature, &public_key_b64)?;
    if claims.device_hash != device_hash {
        return Err("Activation token device hash mismatch.".to_string());
    }

    let cached = CachedLicense {
        token: SignedTokenEnvelope {
            payload: signed_payload,
            signature,
        },
        last_verified_at: now_unix(),
        seats_used: payload.seats_used,
        seat_limit: payload.seat_limit,
    };
    save_cached_license(&cached)?;

    let status = status_from_cached(&cached, &claims, Some("License activated.".to_string()));
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refresh_license() -> Result<serde_json::Value, String> {
    let (api_base, public_key_b64) = get_config()?;
    let cached =
        load_cached_license()?.ok_or_else(|| "No license token to refresh.".to_string())?;
    let device_hash = current_device_hash();

    let client = crate::net::doh_http_client()?;

    let response = client
        .post(format!("{}/license/refresh", api_base))
        .json(&WorkerRefreshRequest {
            payload: &cached.token.payload,
            signature: &cached.token.signature,
            device_hash: &device_hash,
            app_id: APP_ID,
            app_version: env!("CARGO_PKG_VERSION"),
            is_portable: is_portable_os(),
        })
        .send()
        .await
        .map_err(|e| format!("License refresh request failed: {}", e))?;

    let payload: WorkerTokenResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid refresh response: {}", e))?;

    if !payload.ok {
        let (clear_cache, message) = worker_rejection(&payload, "License refresh rejected.");
        if clear_cache {
            let _ = clear_cached_license_inner();
            return Err(message);
        }
        // Server rejected the refresh (e.g. key mismatch, signature issue, server error).
        // Do NOT clear the local cache — the locally-cached token may still be valid
        // for offline use within the grace window. Just surface the error.
        return Err(format!("Server refresh failed: {}", message));
    }

    let signed_payload = payload
        .payload
        .ok_or_else(|| "Refresh response missing payload.".to_string())?;
    let signature = payload
        .signature
        .ok_or_else(|| "Refresh response missing signature.".to_string())?;

    let claims = parse_and_verify_claims(&signed_payload, &signature, &public_key_b64)?;
    if claims.device_hash != device_hash {
        return Err("Refreshed token does not match this device.".to_string());
    }

    let updated = CachedLicense {
        token: SignedTokenEnvelope {
            payload: signed_payload,
            signature,
        },
        last_verified_at: now_unix(),
        seats_used: payload.seats_used,
        seat_limit: payload.seat_limit,
    };
    save_cached_license(&updated)?;

    let status = status_from_cached(&updated, &claims, Some("License refreshed.".to_string()));
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_license_cache() -> Result<(), String> {
    clear_cached_license_inner()
}

/// Start the one-time 16-day free trial for this device.
/// Fails if a trial was already started or if a paid license is already active.
///
/// SECURITY NOTE (was audit F-5, Medium — superseded 2026-07-10).
/// Trial state used to live ENTIRELY in `trial.json`, purely local, no
/// server contact. That made the trial unverifiable by the Pro sidecar
/// (which never trusts anything Free writes locally, per the C5 hardening
/// in `entitlement.rs`) — Pro refused every paid command during an
/// otherwise-"active" trial, and separately, a deleted `trial.json` let a
/// user re-claim the trial indefinitely.
///
/// Both are fixed the same way: the trial is now a real Ed25519-signed
/// token minted by `POST {WINCMD_LICENSE_API_BASE}/trial`, identical in
/// shape to a paid licence token (`plan: "trial"`, `features: ["paid"]`,
/// `exp` = now + 16 days), cached in the SAME `license_cache.json` a real
/// licence uses. Pro verifies it with zero Pro-side special-casing. The
/// worker enforces ONCE-PER-DEVICE server-side (a `device_trials` D1 row
/// keyed by `device_hash`) — deleting the local cache no longer re-claims
/// a trial, only the worker's row does, and that can't be deleted by the
/// client.
///
/// PRIVACY NOTE: this makes trial-start no longer fully offline — it now
/// sends this device's `device_hash` to the licence worker, the same
/// beacon every paid activation already sends. Before this change,
/// starting a trial made zero network calls.
#[tauri::command]
pub async fn start_trial() -> Result<serde_json::Value, String> {
    // Portable edition ships no trial — only a purchased key unlocks Pro, so the
    // most-copyable build has nothing to farm.
    #[cfg(feature = "portable")]
    return Err(
        "The free trial isn't available in this edition — activate a license key.".to_string(),
    );

    #[cfg(not(feature = "portable"))]
    {
        // Local fast-path check: refuses immediately if this device already
        // has ANY cached token (paid licence, or a trial token from a prior
        // successful call here) — avoids a pointless round trip for the
        // common case. This is NOT the security boundary: it reads a file
        // Free itself wrote, so it can be bypassed by deleting the cache.
        // The worker's `device_trials` table (checked server-side on every
        // /trial call below) is what actually enforces once-per-device.
        if load_cached_license()?.is_some() {
            return Err(
                "A license is already active on this device, or the free trial has already \
                 been used."
                    .to_string(),
            );
        }

        let (api_base, public_key_b64) = get_config()?;
        let device_hash = current_device_hash();
        let client = crate::net::doh_http_client()?;

        let response = client
            .post(format!("{}/trial", api_base))
            .json(&WorkerTrialRequest {
                device_hash: &device_hash,
                app_id: APP_ID,
                app_version: env!("CARGO_PKG_VERSION"),
                is_portable: is_portable_os(),
            })
            .send()
            .await
            .map_err(|e| {
                crate::log_message("error", &format!("[License] Trial request failed: {}", e));
                format!("Trial server request failed: {}", e)
            })?;

        let payload: WorkerTokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Invalid trial server response: {}", e))?;

        if !payload.ok {
            return Err(payload
                .error
                .unwrap_or_else(|| "Trial server rejected the request.".to_string()));
        }

        let signed_payload = payload
            .payload
            .ok_or_else(|| "Trial server did not return a token payload.".to_string())?;
        let signature = payload
            .signature
            .ok_or_else(|| "Trial server did not return a signature.".to_string())?;

        let claims = parse_and_verify_claims(&signed_payload, &signature, &public_key_b64)?;
        if claims.device_hash != device_hash {
            return Err("Trial token device hash mismatch.".to_string());
        }

        let cached = CachedLicense {
            token: SignedTokenEnvelope {
                payload: signed_payload,
                signature,
            },
            last_verified_at: now_unix(),
            seats_used: None,
            seat_limit: None,
        };
        save_cached_license(&cached)?;

        let status = status_from_cached(&cached, &claims, Some("Free trial started.".to_string()));
        serde_json::to_value(status).map_err(|e| e.to_string())
    }
}

/// Internal async helper — can be called from other modules (e.g. lockdown).
/// Contacts the server to release the seat, then removes the local cache.
/// If the server rejects (e.g. key mismatch), we still clear the local cache so
/// deactivation always succeeds locally; the stale seat on the server will be
/// reclaimed automatically by the idle-seat cleanup job.
pub async fn deactivate_license_internal() -> Result<(), String> {
    let (api_base, _public_key_b64) = get_config()?;
    let cached =
        load_cached_license()?.ok_or_else(|| "No license token to deactivate.".to_string())?;
    let device_hash = current_device_hash();

    let client = crate::net::doh_http_client()?;

    // Best-effort server call — we always clear locally regardless of outcome.
    let server_result = async {
        let response = client
            .post(format!("{}/license/deactivate", api_base))
            .json(&WorkerDeactivateRequest {
                payload: &cached.token.payload,
                signature: &cached.token.signature,
                device_hash: &device_hash,
                app_id: APP_ID,
            })
            .send()
            .await
            .map_err(|e| format!("Deactivation request failed: {}", e))?;

        let payload: WorkerTokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Invalid deactivation response: {}", e))?;

        if !payload.ok {
            return Err(payload
                .error
                .unwrap_or_else(|| "Deactivation rejected by server.".to_string()));
        }
        Ok(())
    }
    .await;

    // Always clear the local cache — seat will be reclaimed server-side if not already freed.
    clear_cached_license_inner()?;

    // Surface server errors as warnings (non-fatal since local cache is cleared).
    if let Err(e) = server_result {
        eprintln!(
            "[license] Server deactivation warning (local cache cleared anyway): {}",
            e
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn deactivate_license() -> Result<(), String> {
    deactivate_license_internal().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn ordinary_pro_cannot_satisfy_investigator_entitlement() {
        let ordinary_pro = vec!["paid".to_string()];
        assert!(!has_entitlement_in_features(&ordinary_pro, "advanced"));
        assert!(has_entitlement_in_features(&ordinary_pro, "paid"));
        assert!(has_entitlement_in_features(
            &["paid".to_string(), "advanced".to_string()],
            "advanced"
        ));
    }
    use ed25519_dalek::SigningKey;
    use rand_core::OsRng;

    /// Test fixture: a throwaway Ed25519 keypair generated fresh per test.
    /// This is NOT the production licence-worker key — it exists purely so
    /// we can exercise `parse_and_verify_claims`'s pure verification logic
    /// (signature check, key decode, device-hash binding) without any real
    /// private key material.
    fn test_keypair() -> (SigningKey, String) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key_b64 = URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes());
        (signing_key, public_key_b64)
    }

    #[test]
    fn mainline_license_defaults_are_well_formed() {
        let url = reqwest::Url::parse(DEFAULT_LICENSE_API_BASE).expect("valid licensing URL");
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("wincommander-licensing.servalabs.com"));
        assert_eq!(
            decode_b64_any(DEFAULT_LICENSE_PUBLIC_KEY_B64)
                .expect("valid public key")
                .len(),
            32
        );
    }

    fn claims_json(device_hash: &str) -> String {
        // LicenseClaims has no #[serde(rename_all = "camelCase")] — field
        // names must match the Rust snake_case identifiers exactly.
        serde_json::json!({
            "license_id": "test-id",
            "device_hash": device_hash,
            "plan": "pro",
            "features": ["paid"],
            "iat": 1_000_000_u64,
            "exp": 9_999_999_999_u64,
            "license_exp": serde_json::Value::Null,
            "iss": serde_json::Value::Null,
            "sub": serde_json::Value::Null,
        })
        .to_string()
    }

    fn sign_payload(signing_key: &SigningKey, payload: &str) -> String {
        use ed25519_dalek::Signer;
        let sig = signing_key.sign(payload.as_bytes());
        URL_SAFE_NO_PAD.encode(sig.to_bytes())
    }

    fn worker_response(entitlement_revoked: bool, error: Option<&str>) -> WorkerTokenResponse {
        WorkerTokenResponse {
            ok: false,
            payload: Some("forged payload".to_string()),
            signature: Some("forged signature".to_string()),
            error: error.map(str::to_string),
            entitlement_revoked,
            seats_used: Some(99),
            seat_limit: Some(0),
        }
    }

    #[test]
    fn every_worker_rejection_has_only_non_destructive_license_outcomes() {
        for entitlement_revoked in [false, true] {
            for error in [None, Some("arbitrary server error"), Some("lockdown")] {
                let response = worker_response(entitlement_revoked, error);
                let (clear_cache, message) = worker_rejection(&response, "fallback");

                assert_eq!(clear_cache, entitlement_revoked);
                if entitlement_revoked {
                    assert_eq!(message, "License entitlement has been revoked.");
                } else {
                    assert_eq!(message, error.unwrap_or("fallback"));
                }
            }
        }
    }

    #[test]
    fn licensing_frontend_has_no_path_to_lockdown() {
        let gate_source = include_str!("../../../src/components/LicenseGate.tsx");
        let backend_source = include_str!("../../../src/hooks/useBackend.ts");
        let payments_source = include_str!("payments.rs");

        assert!(
            !gate_source.contains("selfDestructApp")
                && !gate_source.contains("__KILL_SWITCH__")
                && !backend_source.contains("selfDestructApp")
                && !backend_source.contains("invoke<void>(\"lockdown\")")
                && !payments_source.contains("lockdown")
                && !payments_source.contains("uninstall")
                && !payments_source.contains("erase")
                && !payments_source.contains("shutdown"),
            "licensing and payment responses must only affect entitlements; they must never lock down or destroy a device"
        );
    }

    /// A cached license payload whose bytes were altered after signing (e.g. a
    /// tampered on-disk cache, or an attacker flipping the `plan`/`features`
    /// field in the JSON) must fail Ed25519 signature verification. If this
    /// ever passed, an attacker could hand-edit the cached license file to
    /// grant themselves arbitrary features.
    #[test]
    fn tampered_license_payload_fails_signature_verification() {
        let (signing_key, public_key_b64) = test_keypair();
        let payload = claims_json("device-abc");
        let signature = sign_payload(&signing_key, &payload);

        // Flip a single character in the signed payload after signing —
        // simulates an on-disk edit of the cached license JSON.
        let tampered_payload = payload.replacen("\"pro\"", "\"enterprise\"", 1);
        assert_ne!(
            tampered_payload, payload,
            "fixture must actually mutate the payload"
        );

        let result = parse_and_verify_claims(&tampered_payload, &signature, &public_key_b64);

        assert!(
            result.is_err(),
            "a tampered payload must fail signature verification, not silently parse"
        );
    }

    /// A signature altered after being produced (e.g. corrupted cache, or an
    /// attacker's forged signature bytes) must fail verification against the
    /// original payload.
    #[test]
    fn tampered_license_signature_fails_verification() {
        let (signing_key, public_key_b64) = test_keypair();
        let payload = claims_json("device-abc");
        let mut signature = sign_payload(&signing_key, &payload);

        // Corrupt the signature by mutating its first base64 character.
        let first = signature.chars().next().unwrap();
        let replacement = if first == 'A' { 'B' } else { 'A' };
        signature.replace_range(0..1, &replacement.to_string());

        let result = parse_and_verify_claims(&payload, &signature, &public_key_b64);

        assert!(
            result.is_err(),
            "a corrupted signature must fail verification against the original payload"
        );
    }

    /// An entitlement payload signed by a DIFFERENT key than the one pinned
    /// in the build (option_env!/WINCMD_LICENSE_PUBLIC_KEY) must not be
    /// trusted — a valid-looking JSON payload with a plausible signature but
    /// the wrong signer must be rejected exactly like a missing signature.
    /// This is the core anti-forgery property: only the pinned key's
    /// signature grants trust, never the payload content alone.
    #[test]
    fn entitlement_signed_by_wrong_key_is_not_trusted() {
        let (signing_key, _public_key_b64) = test_keypair();
        let (_other_signing_key, other_public_key_b64) = test_keypair();
        let payload = claims_json("device-abc");
        let signature = sign_payload(&signing_key, &payload);

        // Verify against a DIFFERENT public key than the one that actually signed it.
        let result = parse_and_verify_claims(&payload, &signature, &other_public_key_b64);

        assert!(
            result.is_err(),
            "a payload signed by key A must not verify against key B's public key"
        );
    }

    /// A correctly signed, correctly keyed payload DOES verify — this is the
    /// control case proving the two failure tests above fail for the right
    /// reason (bad signature/wrong key), not because the harness is broken.
    #[test]
    fn correctly_signed_entitlement_verifies_and_round_trips_claims() {
        let (signing_key, public_key_b64) = test_keypair();
        let payload = claims_json("device-abc");
        let signature = sign_payload(&signing_key, &payload);

        let claims = parse_and_verify_claims(&payload, &signature, &public_key_b64)
            .expect("a correctly signed payload verified with the matching key must succeed");

        assert_eq!(claims.device_hash, "device-abc");
        assert_eq!(claims.plan, "pro");
        assert_eq!(claims.features, vec!["paid".to_string()]);
    }

    /// A license bound to one device_hash must not validate for a different
    /// device_hash. `parse_and_verify_claims` verifies the signature but does
    /// NOT itself check device binding — every call site (current_features_raw,
    /// activate_license, refresh_license) does `claims.device_hash != device_hash
    /// => reject`. This test replays that exact guard against a genuinely
    /// signature-valid claim minted for different hardware, proving a stolen/
    /// copied cache file (valid signature, wrong machine) is still rejected.
    #[test]
    fn license_bound_to_one_device_hash_does_not_validate_on_another() {
        let (signing_key, public_key_b64) = test_keypair();
        let payload = claims_json("device-hash-for-machine-A");
        let signature = sign_payload(&signing_key, &payload);

        let claims = parse_and_verify_claims(&payload, &signature, &public_key_b64)
            .expect("signature is valid; only the device binding is under test");

        let this_machine_device_hash = "device-hash-for-machine-B";
        let accepted_on_this_machine = claims.device_hash == this_machine_device_hash;

        assert!(
            !accepted_on_this_machine,
            "a signature-valid license minted for machine A must not be accepted as valid on machine B"
        );
    }

    /// A public key that isn't exactly 32 bytes (e.g. corrupted build-time
    /// embed, or a minisign-formatted key pasted in by mistake) must be
    /// rejected with an error rather than panicking or silently truncating.
    #[test]
    fn malformed_public_key_is_rejected_not_trusted() {
        let (signing_key, _public_key_b64) = test_keypair();
        let payload = claims_json("device-abc");
        let signature = sign_payload(&signing_key, &payload);

        let bogus_key_b64 = URL_SAFE_NO_PAD.encode(b"too-short-key");
        let result = parse_and_verify_claims(&payload, &signature, &bogus_key_b64);

        assert!(
            result.is_err(),
            "a public key that doesn't decode to exactly 32 bytes must be rejected"
        );
    }

    /// A signature that isn't exactly 64 bytes must be rejected rather than
    /// panicking.
    #[test]
    fn malformed_signature_is_rejected_not_trusted() {
        let (_signing_key, public_key_b64) = test_keypair();
        let payload = claims_json("device-abc");
        let bogus_sig_b64 = URL_SAFE_NO_PAD.encode(b"too-short-signature");

        let result = parse_and_verify_claims(&payload, &bogus_sig_b64, &public_key_b64);

        assert!(
            result.is_err(),
            "a signature that doesn't decode to exactly 64 bytes must be rejected"
        );
    }

    // ── Trial-expiry regression (2026-07-10) ─────────────────────────────
    // current_features_raw() used to short-circuit on `load_trial().is_some()`
    // — the trial FILE'S mere existence, not its expiry — so a lapsed trial
    // kept every paid feature unlocked forever. The fix routes the
    // trial through the SAME verified-claims path a paid licence uses
    // (features_from_verified_claims), so it naturally stops granting
    // anything once the token's own `exp` claim (plus offline grace) has
    // passed. These tests exercise that pure decision function directly —
    // no real cached-license file or device-hash probe needed.

    fn trial_claims(exp: u64) -> LicenseClaims {
        LicenseClaims {
            license_id: Some("trial-device-x".to_string()),
            device_hash: "device-x".to_string(),
            plan: "trial".to_string(),
            base_features: vec![],
            service_features: vec!["paid".to_string()],
            service_exp: Some(exp),
            features: vec!["paid".to_string()],
            iat: exp.saturating_sub(TRIAL_DURATION_SECONDS),
            exp,
            license_exp: Some(exp),
            iss: Some("wincommander-license-worker".to_string()),
            sub: Some("com.servalabs.wincommander".to_string()),
        }
    }

    #[test]
    fn expired_trial_token_past_offline_grace_grants_no_features() {
        let exp = 2_000_000_u64;
        let claims = trial_claims(exp);
        // Last verified right at issuance; "now" is long past both exp AND
        // the 7-day offline grace window measured from last_verified_at.
        let last_verified_at = claims.iat;
        let now = last_verified_at + OFFLINE_GRACE_SECONDS + 1_000_000;

        let features = features_from_verified_claims(&claims, last_verified_at, now);

        assert!(
            features.is_empty(),
            "an expired trial token outside the offline grace window must grant NO \
             features — the old bug returned [\"all\"] forever regardless of expiry \
             because it checked trial.json's mere existence instead of the token's exp"
        );
    }

    #[test]
    fn expired_trial_token_never_uses_offline_grace() {
        let exp = 2_000_000_u64;
        let claims = trial_claims(exp);
        let last_verified_at = exp - 10; // verified shortly before expiry
        let now = exp + 100; // just past exp, well within the 7-day grace

        let features = features_from_verified_claims(&claims, last_verified_at, now);

        assert!(
            features.is_empty(),
            "a trial token must stop granting features exactly at its signed expiry"
        );
    }

    #[test]
    fn unexpired_trial_token_grants_all() {
        let exp = 2_000_000_u64;
        let claims = trial_claims(exp);
        let last_verified_at = claims.iat;
        let now = claims.iat + 1; // well before exp

        let features = features_from_verified_claims(&claims, last_verified_at, now);

        assert_eq!(features, vec!["paid".to_string()]);
    }

    #[test]
    fn permanent_paid_base_survives_an_expired_service_token() {
        let claims = LicenseClaims {
            license_id: Some("pro-lifetime".to_string()),
            device_hash: "device-x".to_string(),
            plan: "pro_lifetime".to_string(),
            base_features: vec!["paid".to_string()],
            service_features: vec![],
            service_exp: None,
            features: vec![],
            iat: 1,
            exp: 2,
            license_exp: None,
            iss: None,
            sub: None,
        };
        assert_eq!(
            features_from_verified_claims(&claims, 2, 9_999_999),
            vec!["paid".to_string()]
        );
    }

    #[test]
    fn expired_investigator_service_falls_back_to_normal_pro() {
        let claims = LicenseClaims {
            license_id: Some("investigator".to_string()),
            device_hash: "device-x".to_string(),
            plan: "investigator".to_string(),
            base_features: vec!["paid".to_string()],
            service_features: vec!["advanced".to_string(), "netwall".to_string()],
            service_exp: Some(2_000),
            features: vec![],
            iat: 1,
            exp: 2,
            license_exp: Some(2_000),
            iss: None,
            sub: None,
        };
        assert_eq!(
            features_from_verified_claims(&claims, 2, 2_001),
            vec!["paid".to_string()]
        );
    }

    #[test]
    fn permanent_paid_fallback_does_not_grant_fleet_service() {
        let fallback = vec!["paid".to_string()];
        assert!(!feature_vector_has_service(&fallback, "fleet"));

        let active_fleet = vec!["paid".to_string(), "fleet".to_string()];
        assert!(feature_vector_has_service(&active_fleet, "fleet"));
    }
}
