// src/types/lockdownSteps.ts
// ═══════════════════════════════════════════════════════════════════════
// Lockdown step registry — TS mirror of Rust DESTRUCT_STEPS
// ═══════════════════════════════════════════════════════════════════════
//
// Mirrors `src-tauri/commander-free/src/lockdown_steps.rs`. Stable
// IDs and labels MUST stay in sync with the Rust side — they're the
// keys in `privacy.selfDestruct.steps` (settings) and the `label`
// values emitted by Rust's `lockdown-step` Tauri events.
//
// Frontend uses this list to:
//   1. Render the configuration UI in the Privacy panel
//      (LockdownConfigSection) — checkboxes grouped by category.
//   2. Build the operation-overlay row list when the sidebar's
//      Lockdown button fires (RightSidebar.fireSelfDestruct).
//
// If you add a step on the Rust side, add it here too. CI doesn't
// catch the divergence today; a settings round-trip will silently
// drop unknown keys (forwards-compat) so an out-of-sync TS only
// causes UI gaps, not data corruption.

import { clearCommand, commandId, invokeCommand } from "../lib/commandIds";

export type DestructGroup =
  | "systemCleaner"
  | "privacyTraces"
  | "deepDfir"
  | "privacyClean"
  | "appRemoval";

export interface DestructStepDef {
  /** Stable settings key — matches Rust DestructStepDef.id. */
  id: string;
  label: string;
  /** Backend command sent to run_backend_script, or a sentinel:
   *  "__system_cleaner__" / "__app_removal__". */
  command: string;
  group: DestructGroup;
  /** What runs if the user hasn't toggled this setting yet. */
  defaultEnabled: boolean;
}

export const DESTRUCT_STEPS: readonly DestructStepDef[] = [
  // System cleaner


  // Privacy traces
  { id: "dismount_volumes",   label: "Dismount Volumes",      command: commandId("Dismount-", "All", "Encryption", "Volumes"), group: "privacyTraces", defaultEnabled: true },
  { id: "encryption_keys",    label: "Clear Encryption Keys",  command: clearCommand("EncryptionKeys"),          group: "privacyTraces", defaultEnabled: true },
  { id: "usb_history",        label: "USB History",           command: clearCommand("USBDeviceHistory"),        group: "privacyTraces", defaultEnabled: true },
  { id: "dns_cache",          label: "DNS Cache",             command: "Clear-DnsCache",                group: "privacyTraces", defaultEnabled: true },
  { id: "execution_cache",    label: "Execution Cache",       command: clearCommand("ExecutionCache"),          group: "privacyTraces", defaultEnabled: true },
  { id: "shadow_copies",      label: "Shadow Copies",         command: clearCommand("ShadowCopies"),            group: "privacyTraces", defaultEnabled: true },
  { id: "rdp_history",        label: "RDP History",           command: clearCommand("RDPHistory"),              group: "privacyTraces", defaultEnabled: true },
  { id: "rdp_passwords",      label: "RDP Passwords",         command: clearCommand("RDPPasswords"),            group: "privacyTraces", defaultEnabled: true },
  { id: "srum",               label: "Resource Usage History", command: clearCommand("SRUM"),                    group: "privacyTraces", defaultEnabled: true },
  { id: "event_logs",         label: "Event Logs",            command: clearCommand("EventLogs"),               group: "privacyTraces", defaultEnabled: true },
  { id: "ntfs_journals",      label: "NTFS Journals",         command: clearCommand("NTFSJournals"),            group: "privacyTraces", defaultEnabled: true },
  // Feature 2 — Metadata-zone scrub (paid)
  // recyclebin_overwrite: default ON — cheap, safe single-pass RNG $R overwrite before delete.
  // logfile_clear: default OFF — best-effort chkdsk /L resize; defers C: to next boot.
  { id: "recyclebin_overwrite", label: "Recycle Bin overwrite-before-delete", command: clearCommand("RecycleBinMetadata"), group: "privacyTraces", defaultEnabled: true },
  { id: "logfile_clear",        label: "$LogFile / NTFS journal scrub",       command: clearCommand("NTFSLogFile"),        group: "privacyTraces", defaultEnabled: false },
  { id: "wifi_profiles",      label: "Wi-Fi Profiles",        command: commandId("Remove-", "Wlan", "Profile"),            group: "privacyTraces", defaultEnabled: false },
  { id: "bluetooth",          label: "Bluetooth",             command: clearCommand("BluetoothDevices"),        group: "privacyTraces", defaultEnabled: false },
  { id: "network_drives",     label: "Network Drives",        command: clearCommand("NetworkDrives"),           group: "privacyTraces", defaultEnabled: false },
  { id: "clipboard",          label: "Clipboard",             command: clearCommand("Clipboard"),               group: "privacyTraces", defaultEnabled: true },
  { id: "remove_schedules",   label: "Auto-clean Schedules",  command: "__remove_schedules__",         group: "privacyTraces", defaultEnabled: false },
  { id: "jump_lists",         label: "Jump Lists",            command: clearCommand("JumpLists"),               group: "privacyTraces", defaultEnabled: true },
  { id: "recent_files",       label: "Recent Files",          command: clearCommand("RecentFiles"),             group: "privacyTraces", defaultEnabled: true },
  { id: "powershell_history", label: "Command History",        command: "Clear-PowerShellHistory",                           group: "privacyTraces", defaultEnabled: true },
  // Browser data is its own toggleable step. The System Cleaner step
  // is pinned to skip browsers in the orchestrator (so the two never
  // double-cover) — turn this on to actually erase browser cache /
  // cookies / history / downloads.
  { id: "browser_footprints", label: "Browser Data",          command: clearCommand("BrowserFootprints"),       group: "privacyTraces", defaultEnabled: false },
  { id: "prefetch",           label: "Prefetch Files",        command: clearCommand("Prefetch"),                group: "privacyTraces", defaultEnabled: true },
  { id: "shellbags",          label: "ShellBags",             command: clearCommand("ShellBags"),               group: "privacyTraces", defaultEnabled: true },
  { id: "connectivity_history", label: "Connectivity History",command: clearCommand("ConnectivityHistory"),     group: "privacyTraces", defaultEnabled: true },
  // Expanded System Cleanup catalogue — opt-in because these can remove app
  // state, saved access settings, or local user content.
  { id: "wsl_data", label: "WSL Data", command: clearCommand("WSLData"), group: "privacyTraces", defaultEnabled: false },
  { id: "docker_desktop_data", label: "Docker Desktop Data", command: clearCommand("DockerDesktopData"), group: "privacyTraces", defaultEnabled: false },
  { id: "virtual_machine_artifacts", label: "Virtual Machine Artifacts", command: clearCommand("VirtualMachineArtifacts"), group: "privacyTraces", defaultEnabled: false },
  { id: "developer_caches", label: "Developer Tool Caches", command: clearCommand("DeveloperCaches"), group: "privacyTraces", defaultEnabled: false },
  { id: "credential_manager", label: "Saved Credentials", command: clearCommand("CredentialManager"), group: "privacyTraces", defaultEnabled: false },
  { id: "network_wizard_history", label: "Network Wizard History", command: clearCommand("NetworkWizardHistory"), group: "privacyTraces", defaultEnabled: false },
  { id: "wer_history", label: "Windows Error Reporting History", command: clearCommand("WERHistory"), group: "privacyTraces", defaultEnabled: false },
  { id: "inactive_user_protection_metadata", label: "Inactive User Protection Metadata", command: clearCommand("InactiveUserProtectionMetadata"), group: "privacyTraces", defaultEnabled: false },
  { id: "sticky_notes", label: "Sticky Notes", command: clearCommand("StickyNotes"), group: "privacyTraces", defaultEnabled: false },
  { id: "onedrive_metadata", label: "OneDrive Sync Metadata", command: clearCommand("OneDriveMetadata"), group: "privacyTraces", defaultEnabled: false },
  { id: "spotlight_cache", label: "Windows Spotlight Cache", command: clearCommand("SpotlightCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "font_cache", label: "Font Cache", command: clearCommand("FontCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "legacy_icon_cache", label: "Legacy Icon Cache", command: clearCommand("LegacyIconCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "game_captures", label: "Game Captures", command: clearCommand("GameCaptures"), group: "privacyTraces", defaultEnabled: false },
  { id: "photos_cache", label: "Photos Cache", command: clearCommand("PhotosCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "xbox_cache", label: "Xbox Cache", command: clearCommand("XboxCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "communication_caches", label: "Communication App Caches", command: clearCommand("CommunicationCaches"), group: "privacyTraces", defaultEnabled: false },
  { id: "editor_history", label: "Editor History", command: clearCommand("EditorHistory"), group: "privacyTraces", defaultEnabled: false },
  { id: "git_activity", label: "Git Activity", command: clearCommand("GitActivity"), group: "privacyTraces", defaultEnabled: false },
  { id: "ssh_state", label: "SSH State", command: clearCommand("SSHState"), group: "privacyTraces", defaultEnabled: false },
  { id: "remote_access_logs", label: "Remote Access Logs", command: clearCommand("RemoteAccessLogs"), group: "privacyTraces", defaultEnabled: false },
  { id: "password_manager_caches", label: "Password Manager Caches", command: clearCommand("PasswordManagerCaches"), group: "privacyTraces", defaultEnabled: false },
  { id: "game_launcher_logs", label: "Game Launcher Logs", command: clearCommand("GameLauncherLogs"), group: "privacyTraces", defaultEnabled: false },
  { id: "adobe_recent", label: "Adobe Recent Files", command: clearCommand("AdobeRecent"), group: "privacyTraces", defaultEnabled: false },
  { id: "office_temp_files", label: "Office Temporary Files", command: clearCommand("OfficeTempFiles"), group: "privacyTraces", defaultEnabled: false },
  { id: "firewall_log", label: "Firewall Log", command: clearCommand("FirewallLog"), group: "privacyTraces", defaultEnabled: false },
  { id: "neighbor_cache", label: "Network Neighbor Cache", command: clearCommand("NeighborCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "netbios_cache", label: "NetBIOS Cache", command: clearCommand("NetBIOSCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "geolocation_cache", label: "Geolocation Cache", command: clearCommand("GeolocationCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "vpn_phonebooks", label: "VPN Phonebooks", command: clearCommand("VPNPhonebooks"), group: "privacyTraces", defaultEnabled: false },
  { id: "proxy_cache", label: "Proxy Cache", command: clearCommand("ProxyCache"), group: "privacyTraces", defaultEnabled: false },
  { id: "cloud_placeholders", label: "Cloud Sync Placeholders", command: clearCommand("CloudPlaceholders"), group: "privacyTraces", defaultEnabled: false },
  { id: "bits_queue", label: "BITS Transfer Queue", command: clearCommand("BITSQueue"), group: "privacyTraces", defaultEnabled: false },
  { id: "cellular_history", label: "Cellular Connection History", command: clearCommand("CellularHistory"), group: "privacyTraces", defaultEnabled: false },
  // Feature 3: live pagefile zero. Off by default — deferred ClearPageFileAtShutdown
  // (via the ramSpillControl toggle) is the safe path; this step zeroes immediately.
  { id: "pagefile_zero",        label: "Pagefile Zero (live)",  command: invokeCommand("PagefileZero"),          group: "privacyTraces", defaultEnabled: false },

  // Deep DFIR
  { id: "amcache",       label: "Amcache",          command: clearCommand("Amcache"),          group: "deepDfir", defaultEnabled: true },
  { id: "ntuser_traces", label: "User Activity Traces", command: clearCommand("NTUserTraces"),     group: "deepDfir", defaultEnabled: true },
  { id: "notepad_state", label: "Notepad State",    command: clearCommand("NotepadState"),     group: "deepDfir", defaultEnabled: true },
  { id: "pca_database",  label: "PCA Database",     command: clearCommand("PCADatabase"),      group: "deepDfir", defaultEnabled: true },
  { id: "windows_old",   label: "Windows.old",      command: clearCommand("WindowsOld"),       group: "deepDfir", defaultEnabled: true },
  { id: "crash_dumps",   label: "Crash Dumps",      command: invokeCommand("CrashDumpErase"),   group: "deepDfir", defaultEnabled: true },
  { id: "sqlite_wal",    label: "SQLite WAL Files", command: invokeCommand("SQLiteWALKiller"), group: "deepDfir", defaultEnabled: true },
  { id: "recall",        label: "Recall Database",  command: clearCommand("RecallDatabase"),   group: "deepDfir", defaultEnabled: true },
  { id: "search_index",  label: "Search Index",     command: clearCommand("SearchIndex"),      group: "deepDfir", defaultEnabled: true },
  { id: "print_spooler", label: "Print Spooler",    command: clearCommand("PrintSpooler"),     group: "deepDfir", defaultEnabled: true },

  // Privacy Clean deep erasers (default OFF — slow)
  { id: "unallocated_erase", label: "Free Space Cleanup",      command: invokeCommand("UnallocatedSpaceErase"), group: "privacyClean", defaultEnabled: false },
  { id: "ssd_trim",         label: "SSD TRIM",               command: invokeCommand("SSDTrim"),              group: "privacyClean", defaultEnabled: false },
  { id: "virtual_memory",   label: "Virtual Memory Purge",   command: invokeCommand("VirtualMemoryPurge"),   group: "privacyClean", defaultEnabled: true },

  // Feature 5 — real crypto-erase (IRREVERSIBLE; default OFF — explicit opt-in required).
  // These steps destroy the encryption master key; the drive becomes permanently unreadable.
  // NOT included in any preset. Must be enabled individually in self-destruct settings.
  { id: "bitlocker_erase",         label: "BitLocker Key Erase",      command: clearCommand("BitLockerKeyProtectors"),  group: "privacyClean", defaultEnabled: false },
  { id: "veracrypt_header_destroy", label: "VeraCrypt Header Destroy",  command: commandId("Destroy-", "VeraCrypt", "Header"),       group: "privacyClean", defaultEnabled: false },

  // Selective account removal (IRREVERSIBLE; default OFF). Pro securely wipes
  // each selected local account's profile then deletes the account. Usernames
  // come from selfDestruct.usersToRemove; sentinel command keeps the removal
  // command string out of the Free JS bundle (dispatch is by step id in Rust).
  { id: "remove_users", label: "Remove Users & Wipe Data", command: "__remove_users__", group: "privacyClean", defaultEnabled: false },

  // App removal
  { id: "include_app", label: "Uninstall WinCommander", command: "__app_removal__", group: "appRemoval", defaultEnabled: false },
] as const;

export const DESTRUCT_GROUP_LABELS: Record<DestructGroup, string> = {
  systemCleaner: "System Cleaner",
  privacyTraces: "Privacy Traces",
  deepDfir: "Deep Trace",
  privacyClean: "Deep Clean",
  appRemoval: "On Completion",
};

/** Per-step descriptions for the configuration UI. Plain English,
 *  non-jargon where possible — these surface in the SelfDestruct
 *  config section as the secondary line under each row's label, and
 *  are the only place a non-expert can find out what e.g.
 *  "Amcache" actually means without leaving the app. */
export const DESTRUCT_STEP_DESCRIPTIONS: Record<string, string> = {
  // System cleaner
  system_cleaner: "Clean system caches, logs, and temporary files.",

  // Privacy traces
  dismount_volumes:    "Force-unmount any open BitLocker or encrypted volumes.",
  encryption_keys:     "Evicts mounted-volume master keys from memory by dismounting every encrypted volume. NOT a crypto-erase — container data stays intact and re-mountable with the password. Use 'VeraCrypt Header Destroy' for irreversible key destruction.",
  usb_history:         "Connected USB device log (registry + setupapi traces).",
  dns_cache:           "Local DNS resolver cache.",
  execution_cache:     "ShimCache + UserAssist + AppCompat execution traces.",
  shadow_copies:       "Volume Shadow Copies (system restore points).",
  rdp_history:         "Recent RDP connections and saved server addresses.",
  rdp_passwords:       "Saved RDP credentials in the Windows Credential Manager.",
  srum:                "System Resource Usage Monitor — per-app CPU + network history.",
  event_logs:          "Windows Application + System + Security event logs.",
  ntfs_journals:       "NTFS USN journal — file-change history per drive.",
  recyclebin_overwrite: "Single durable RNG-overwrite pass of $R data files (and $I metadata) in the Recycle Bin before deletion, so carvers cannot recover the content.",
  logfile_clear:        "Best-effort resize of NTFS $LogFile (redo/undo log) to its minimum via chkdsk /L. The system volume (C:) is deferred to next boot; a live mounted volume cannot be fully zeroed without offline dismount.",
  wifi_profiles:       "Saved Wi-Fi networks and their stored passwords.",
  bluetooth:           "Paired Bluetooth devices and pairing history.",
  network_drives:      "Mapped network shares and recent UNC paths.",
  clipboard:           "System clipboard (current contents + clipboard history).",
  remove_schedules:    "Remove configured auto-clean scheduled tasks (per-category).",
  jump_lists:          "App jump lists — recent files in taskbar / start menu.",
  recent_files:        "Shell Recent folder (Windows-key + R history, etc.).",
  powershell_history:  "Terminal command history and session logs.",
  browser_footprints:  "Browser cache, cookies, history, and downloaded-files list.",
  prefetch:            "Prefetch files (.pf) — execution timing data.",
  shellbags:           "Folder access history stored in the registry.",
  connectivity_history: "General network connectivity traces.",
  pagefile_zero:        "Attempts to zero the active pagefile immediately via a raw volume handle (requires admin + SE_MANAGE_VOLUME_NAME). Best-effort — the safe deferred path is the RAM-Spill Control toggle (ClearPageFileAtShutdown=1).",

  // Deep DFIR
  amcache:        "Amcache.hve — execution timestamps and binary hashes.",
  ntuser_traces:  "RunMRU, TypedPaths, OpenSaveMRU, Recent Apps.",
  notepad_state:  "Unsaved Notepad tab content + .bin hash files.",
  pca_database:   "Program Compatibility Assistant logs.",
  windows_old:    "Windows.old — full copy of the pre-upgrade user profile left after a Windows feature update.",
  crash_dumps:    "WER reports, MEMORY.DMP, minidump files.",
  sqlite_wal:     "Stale .wal / .shm files in user APPDATA.",
  recall:         "Microsoft Recall + ConnectedDevices databases.",
  search_index:   "Windows Search Index (Windows.edb) content.",
  print_spooler:  "Document images sitting in the print spool queue.",

  // Privacy Clean deep erasers (slow / destructive)
  unallocated_erase: "Overwrites unused disk space so deleted files cannot be recovered. May take 30+ minutes on large drives.",
  ssd_trim:         "Optimize-Volume -ReTrim — TRIMs unmapped SSD blocks. Irreversible.",
  virtual_memory:   "Disable hibernation and clear the pagefile on next boot.",

  // Feature 5 — real crypto-erase (IRREVERSIBLE)
  bitlocker_erase:
    "IRREVERSIBLE. Removes ALL BitLocker key protectors from the OS drive (default C:). " +
    "The drive data remains AES-encrypted but is unreadable without a protector. " +
    "WARNING: if any RecoveryPassword or ExternalKey protector was escrowed to AAD or Active Directory " +
    "BEFORE this erase, the volume is still recoverable via that escrow copy. " +
    "Always inspect the 'escrow_warning' field in the result before claiming crypto-erase succeeded.",
  veracrypt_header_destroy:
    "IRREVERSIBLE. Overwrites the first and last 131 072 bytes of the container file with " +
    "cryptographic-RNG data, destroying the primary header, backup header, and both hidden-volume " +
    "header regions simultaneously. No rescue disk or backup header survives. " +
    "Requires the container path to be supplied in the step args.",
  remove_users:
    "IRREVERSIBLE. For each account selected above: securely overwrites its " +
    "user-profile folder (single durable RNG-overwrite pass), then deletes the profile and the local " +
    "account. Built-in Windows accounts, the current account, and any account " +
    "currently signed in are always skipped. No accounts selected by default.",

  // App removal
  include_app: "Uninstall the app, permanently delete its data, and remove its system entries.",
};

/** Resolve enabled state for a step given the user's sparse override map. */
export function isStepEnabled(
  def: DestructStepDef,
  steps: Record<string, boolean> | undefined,
): boolean {
  if (!steps) return def.defaultEnabled;
  const v = steps[def.id];
  return v === undefined ? def.defaultEnabled : v;
}
