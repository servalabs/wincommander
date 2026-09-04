// SPDX-License-Identifier: AGPL-3.0-or-later
// commander-free/src/lockdown_steps.rs
// ═══════════════════════════════════════════════════════════════════════
// Lockdown step registry — UI metadata only (P2: command strings moved to Pro)
// ═══════════════════════════════════════════════════════════════════════
//
// A-4 module 2 evaluation: this file stays in Free.
//
// It is a pure-data registry consumed by `full_lockdown` in
// backend.rs and mirrored to TypeScript at `src/types/lockdownSteps.ts`
// so the Privacy panel can render checkboxes against the same step IDs.
// The IDs ("amcache", "ntuser_traces" etc.) are stable settings keys.
//
// P2 change: the `command` field (which held PowerShell function names
// like "Clear-EventLogs") has been removed from Free. The orchestrator
// now dispatches each step to the Pro sidecar by step ID; Pro holds the
// ID → PS command mapping. This keeps privacy-clean PS command strings
// out of the Free binary's string table entirely.
//
// Bottom line: this file is now purely UI-facing configuration metadata
// with no executable command strings.  The strings-grep CI gate (A-5)
// verifies the PS command tokens are absent from the Free binary.
//
// Every step the universal lockdown orchestrator (`full_lockdown`
// in backend.rs) can run is declared here, exactly once. The frontend
// mirrors this list via `src/types/lockdownSteps.ts` so the Privacy
// panel's Self-Destruct configuration section can render checkboxes
// against the same step IDs.
//
// Stable IDs are load-bearing — they're used as keys in
// `privacy.selfDestruct.steps` (see settings::SelfDestructSettings).
// Renaming an ID is a settings migration; never do it.
//
// `command` is the backend command dispatched via `run_backend_script`.
// Two sentinel values are special-cased in the orchestrator:
//   - `__system_cleaner__` invokes run_bleachbit_clean directly
//     (Rust-native helper; not a PowerShell command).
//   - `__app_removal__` triggers Phase 2 (uninstaller + secure erase of
//     %APPDATA%, registry footprint, app exit). It runs at the very end
//     and does not return — the spawned PowerShell script outlives the
//     app process.
//
// `default_enabled` controls behaviour for users who haven't opened the
// settings UI yet. Most steps default ON to preserve the existing
// "panic = full lockdown" behaviour from before customisation existed.
// The three slow Privacy Clean deep erasers (cipher /w, SSD TRIM, virtual-
// memory purge) default OFF because they take minutes-to-hours and
// shouldn't surprise a user who triggered a panic for time-critical
// reasons. App removal also defaults ON to preserve current behaviour;
// users who want "erase traces but keep the app installed" must opt out
// explicitly.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestructGroup {
    /// run_bleachbit_clean — system-wide caches/logs cleaner.
    SystemCleaner,
    /// Standard privacy cleaners (USB, DNS, RDP, event logs, etc.).
    /// Most are paid commands at the run_backend_script layer.
    PrivacyTraces,
    /// Deep deep trace analysis clearers (Amcache, NTUSER, Recall, etc.). All paid.
    DeepDfir,
    /// Irreversible deep erasers (cipher /w, SSD TRIM, virtual memory).
    /// Slow and irreversible; default off.
    PrivacyClean,
    /// App removal (uninstaller + APPDATA erase + registry cleanup).
    /// Single step; ID is `include_app`.
    AppRemoval,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct DestructStepDef {
    /// Stable ID — used as the key in `privacy.selfDestruct.steps`.
    /// snake_case; NEVER rename after release without a migration.
    /// The orchestrator dispatches by ID to the Pro sidecar; the two
    /// sentinel IDs handled locally are "system_cleaner" (BleachBit)
    /// and "include_app" (uninstaller, Phase 3).
    pub id: &'static str,
    /// Human-readable label. Also emitted as the `lockdown-step` event
    /// `label` field; the frontend matches per-row promises against it.
    pub label: &'static str,
    /// `group` is unused inside Rust (the orchestrator iterates the
    /// flat list); it exists for the TS mirror to drive UI grouping.
    /// `#[allow(dead_code)]` on the struct silences the lint.
    pub group: DestructGroup,
    /// Whether this step runs by default if the user hasn't customised
    /// the settings yet. true preserves pre-customisation behaviour.
    pub default_enabled: bool,
}

/// All steps the orchestrator can run, in the order they execute.
/// Order matters: System Cleaner runs first (fast, free), privacy
/// cleaners next, deep trace analysis after, slow Privacy Clean deep erasers next,
/// and the app-removal step last (Phase 3 exits the process).
///
/// P2: `command` strings removed. The orchestrator dispatches by ID to the
/// Pro sidecar ("run_destruct_step" feature), which holds the mapping.
/// Two IDs are handled locally in backend.rs:
///   "system_cleaner" → run_bleachbit_clean (BleachBit, Rust-native)
///   "include_app"    → lockdown() (uninstaller + app exit)
pub const DESTRUCT_STEPS: &[DestructStepDef] = &[
    // ── System Cleaner ────────────────────────────────────────────────
    DestructStepDef {
        id: "system_cleaner",
        label: "System Cleaner",
        group: DestructGroup::SystemCleaner,
        default_enabled: true,
    },
    // ── Privacy traces ────────────────────────────────────────────────
    DestructStepDef {
        id: "dismount_volumes",
        label: "Dismount Volumes",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "encryption_keys",
        label: "Clear Encryption Keys",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "usb_history",
        label: "USB History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "dns_cache",
        label: "DNS Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "execution_cache",
        label: "Execution Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "shadow_copies",
        label: "Shadow Copies",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "rdp_history",
        label: "RDP History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "rdp_passwords",
        label: "RDP Passwords",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "srum",
        label: "SRUM Database",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "event_logs",
        label: "Event Logs",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "ntfs_journals",
        label: "NTFS Journals",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    // Feature 2 — Metadata-zone scrub (paid)
    // recyclebin_overwrite: default ON — cheap, safe, always appropriate.
    // logfile_clear: default OFF — best-effort on live system; defers C: to next boot.
    DestructStepDef {
        id: "recyclebin_overwrite",
        label: "Recycle Bin overwrite-before-delete",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "logfile_clear",
        label: "$LogFile / NTFS journal scrub",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    // wifi_profiles: Pro passes Name="" to Remove-WlanProfile (erase all).
    DestructStepDef {
        id: "wifi_profiles",
        label: "Wi-Fi Profiles",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "bluetooth",
        label: "Bluetooth",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "network_drives",
        label: "Network Drives",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "clipboard",
        label: "Clipboard",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "jump_lists",
        label: "Jump Lists",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "recent_files",
        label: "Recent Files",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "powershell_history",
        label: "PowerShell History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    // Browser data is its own toggleable step; System Cleaner always
    // excludes browsers so the two don't double-cover.
    DestructStepDef {
        id: "browser_footprints",
        label: "Browser Data",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "prefetch",
        label: "Prefetch Files",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "shellbags",
        label: "ShellBags",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    DestructStepDef {
        id: "connectivity_history",
        label: "Connectivity History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: true,
    },
    // System Cleanup's expanded trace catalogue. These are opt-in because
    // several remove application state or saved access configuration.
    DestructStepDef {
        id: "wsl_data",
        label: "WSL Data",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "docker_desktop_data",
        label: "Docker Desktop Data",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "virtual_machine_artifacts",
        label: "Virtual Machine Artifacts",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "developer_caches",
        label: "Developer Tool Caches",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "credential_manager",
        label: "Saved Credentials",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "network_wizard_history",
        label: "Network Wizard History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "wer_history",
        label: "Windows Error Reporting History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "inactive_user_protection_metadata",
        label: "Inactive User Protection Metadata",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "sticky_notes",
        label: "Sticky Notes",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "onedrive_metadata",
        label: "OneDrive Sync Metadata",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "spotlight_cache",
        label: "Windows Spotlight Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "font_cache",
        label: "Font Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "legacy_icon_cache",
        label: "Legacy Icon Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "game_captures",
        label: "Game Captures",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "photos_cache",
        label: "Photos Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "xbox_cache",
        label: "Xbox Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "communication_caches",
        label: "Communication App Caches",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "editor_history",
        label: "Editor History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "git_activity",
        label: "Git Activity",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "ssh_state",
        label: "SSH State",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "remote_access_logs",
        label: "Remote Access Logs",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "password_manager_caches",
        label: "Password Manager Caches",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "game_launcher_logs",
        label: "Game Launcher Logs",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "adobe_recent",
        label: "Adobe Recent Files",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "office_temp_files",
        label: "Office Temporary Files",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "firewall_log",
        label: "Firewall Log",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "neighbor_cache",
        label: "Network Neighbor Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "netbios_cache",
        label: "NetBIOS Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "geolocation_cache",
        label: "Geolocation Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "vpn_phonebooks",
        label: "VPN Phonebooks",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "proxy_cache",
        label: "Proxy Cache",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "cloud_placeholders",
        label: "Cloud Sync Placeholders",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "bits_queue",
        label: "BITS Transfer Queue",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    DestructStepDef {
        id: "cellular_history",
        label: "Cellular Connection History",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    // Feature 3: live pagefile zero. Off by default — deferred ClearPageFileAtShutdown
    // (via Enable-RamSpillControl toggle) is the safe path; this is the immediate wipe.
    DestructStepDef {
        id: "pagefile_zero",
        label: "Pagefile Zero (live)",
        group: DestructGroup::PrivacyTraces,
        default_enabled: false,
    },
    // ── Deep trace analysis clearers ──────────────────────────────────
    DestructStepDef {
        id: "amcache",
        label: "Amcache",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "ntuser_traces",
        label: "NTUSER Traces",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "notepad_state",
        label: "Notepad State",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "pca_database",
        label: "PCA Database",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "windows_old",
        label: "Windows.old",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "crash_dumps",
        label: "Crash Dumps",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "sqlite_wal",
        label: "SQLite WAL Files",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "recall",
        label: "Recall Database",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "search_index",
        label: "Search Index",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "print_spooler",
        label: "Print Spooler",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "web_cache",
        label: "Web Cache Database",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "thumbnail_cache",
        label: "Thumbnail & Icon Cache",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "notification_database",
        label: "Notification History",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "branch_cache",
        label: "Peer Distribution Cache",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "event_transcript",
        label: "Diagnostics Timeline",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "activities_timeline",
        label: "Timeline Cache",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "rdp_bitmap_cache",
        label: "Remote Session Cache",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "servicing_logs",
        label: "Servicing Logs",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "device_install_logs",
        label: "Device Install Logs",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "usage_trace_logs",
        label: "Usage Trace Logs",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "defender_history",
        label: "Protection History",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "third_party_security_product_logs",
        label: "Third-Party Security Product Logs",
        group: DestructGroup::DeepDfir,
        default_enabled: false,
    },
    DestructStepDef {
        id: "forensic_tool_artifacts",
        label: "FTK Imager Artifacts",
        group: DestructGroup::DeepDfir,
        default_enabled: false,
    },
    DestructStepDef {
        id: "windows_policy_auth_caches",
        label: "Policy & Authentication Caches",
        group: DestructGroup::DeepDfir,
        default_enabled: false,
    },
    DestructStepDef {
        id: "cortana_wsa_logs",
        label: "Cortana & Android Subsystem Logs",
        group: DestructGroup::DeepDfir,
        default_enabled: false,
    },
    DestructStepDef {
        id: "bitlocker_recovery_temp",
        label: "BitLocker Recovery-Key Temp Files",
        group: DestructGroup::DeepDfir,
        default_enabled: false,
    },
    DestructStepDef {
        id: "app_launch_history",
        label: "App Launch History",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "office_mru",
        label: "Office Document History",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "embedded_web_cache",
        label: "Embedded Browser Cache",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "p2p_update_cache",
        label: "Update Sharing Cache",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "reliability_history",
        label: "Stability History",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "explorer_search_history",
        label: "Explorer Search History",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    DestructStepDef {
        id: "search_personalization",
        label: "Search Personalization Data",
        group: DestructGroup::DeepDfir,
        default_enabled: true,
    },
    // ── Privacy Clean deep erasers (slow; default OFF) ────────────────
    DestructStepDef {
        id: "unallocated_erase",
        label: "Unallocated Space Erase",
        group: DestructGroup::PrivacyClean,
        default_enabled: false,
    },
    DestructStepDef {
        id: "ssd_trim",
        label: "SSD TRIM",
        group: DestructGroup::PrivacyClean,
        default_enabled: false,
    },
    // #13: default ON — disabling hibernation (removes hiberfil.sys, a RAM-on-disk
    // source) + arming ClearPageFileAtShutdown is fast flag-setting, not a slow
    // eraser. The immediate pagefile zero (pagefile_zero) stays opt-in.
    DestructStepDef {
        id: "virtual_memory",
        label: "Virtual Memory Purge",
        group: DestructGroup::PrivacyClean,
        default_enabled: true,
    },
    DestructStepDef {
        id: "configured_folders",
        label: "Configured Folder Shred",
        group: DestructGroup::PrivacyClean,
        default_enabled: true,
    },
    // Feature 5 — real crypto-erase (Irreversible; default OFF — must be opted in)
    DestructStepDef {
        id: "bitlocker_erase",
        label: "BitLocker Key Erase",
        group: DestructGroup::PrivacyClean,
        default_enabled: false,
    },
    DestructStepDef {
        id: "veracrypt_header_destroy",
        label: "VeraCrypt Header Destroy",
        group: DestructGroup::PrivacyClean,
        default_enabled: false,
    },
    // Selective account removal — securely wipes each configured local
    // account's profile then deletes the account (Pro executes; usernames come
    // from settings.privacy.selfDestruct.users_to_remove). Default OFF.
    DestructStepDef {
        id: "remove_users",
        label: "Remove Users & Wipe Data",
        group: DestructGroup::PrivacyClean,
        default_enabled: false,
    },
    // ── App removal (Phase 3; runs last, exits the app) ──────────────
    DestructStepDef {
        id: "include_app",
        label: "Uninstall WinCommander",
        group: DestructGroup::AppRemoval,
        default_enabled: true,
    },
];

/// Look up a step by its stable ID. Returns `None` for unknown IDs
/// (which can happen if a settings file from a newer build is loaded
/// by an older binary — we ignore unknown keys rather than crash).
#[allow(dead_code)]
pub fn lookup(id: &str) -> Option<&'static DestructStepDef> {
    DESTRUCT_STEPS.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::{lookup, DestructGroup};

    #[test]
    fn nyx_security_and_forensic_steps_are_opt_in_deep_dfir_actions() {
        for (id, label) in [
            (
                "third_party_security_product_logs",
                "Third-Party Security Product Logs",
            ),
            ("forensic_tool_artifacts", "FTK Imager Artifacts"),
            (
                "windows_policy_auth_caches",
                "Policy & Authentication Caches",
            ),
            ("cortana_wsa_logs", "Cortana & Android Subsystem Logs"),
            (
                "bitlocker_recovery_temp",
                "BitLocker Recovery-Key Temp Files",
            ),
        ] {
            let step = lookup(id).expect("Nyx-derived step must remain selectable");
            assert_eq!(step.label, label);
            assert_eq!(step.group, DestructGroup::DeepDfir);
            assert!(
                !step.default_enabled,
                "{id} must require an explicit opt-in"
            );
        }
    }
}
