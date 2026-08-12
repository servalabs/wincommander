# IPC command catalog

A reference for the two IPC channels in WinCommander, focused on the **Tauri command catalog**.

WinCommander has two IPC boundaries:

1. **Frontend ↔ Backend (Tauri IPC)** — the React UI invokes Rust `#[tauri::command]` handlers in `wincommander-free.exe`. Cataloged below.
2. **commander-free ↔ commander-pro (Windows named pipe)** — Free spawns the Pro sidecar on demand for paid commands.

The **wire format, handshake, signing, and trust model** for both channels are documented in [ARCHITECTURE.md — Free ↔ Pro IPC](../ARCHITECTURE.md#free--pro-ipc) and the [Data flow](../ARCHITECTURE.md#data-flow) section. This document does not repeat them; it is the command-catalog companion. The wire-format source of truth is [`wincmd-shared/src/lib.rs`](../src-tauri/wincmd-shared/src/lib.rs).

Command registration lives in [`src-tauri/commander-free/src/lib.rs`](../src-tauri/commander-free/src/lib.rs) (the `tauri::generate_handler!` block), plus the data-driven tier entries registered at startup by `backend::register_p2_commands` / `register_p3_commands` / `register_file_search_commands`.

## How a command is routed

Every UI-driven toggle funnels through `run_backend_script`, which decides — by `get_command_tier` — whether the work runs in-process (Free) or is forwarded to the Pro sidecar (Paid). The dedicated commands below are direct Tauri handlers; many of them call `license::require_paid` internally and dispatch to Pro over the signed pipe. See [ARCHITECTURE.md — Data flow](../ARCHITECTURE.md#data-flow) for the full guard order (evidence-integrity kill-switch → shield quota → tier gate → module gate).

## Tauri command catalog (frontend ↔ backend)

Tier column: **Free** runs in-process in `commander-free`; **Paid** is gated by `require_paid` and (for execution-bearing work) dispatched to the Pro sidecar. Detector/monitor commands marked Paid gate their start/configure verbs but generally leave read/clear ungated.

### Settings & convergence

| Command | Returns | Purpose |
|---------|---------|---------|
| `get_settings` | `AppSettings` | Full settings object. |
| `get_setting` | Value | Single setting by dot-path. |
| `set_settings` | — | Replace the full settings object. |
| `patch_settings_cmd` | — | Deep-merge a partial update; nulls in `ideal` mean "no preference". |
| `get_settings_hash_cmd` | String | SHA-256 of settings, for drift detection. |
| `is_setting_locked` | Boolean | Whether a path is admin/policy locked. |
| `apply_admin_config_cmd` | — | Apply an admin/fleet policy config. |
| `export_settings_cmd` | JSON string | Serialize settings for backup. |
| `import_settings_cmd` | — | Restore settings from a backup JSON string. |
| `get_drift_report` | `DriftItem[]` | Ideal-vs-current diffs (convergence engine). |
| `update_current_state` | — | Write probe results into `current`. |
| `get_device_identity` | `DeviceIdentity` | Device metadata for registration. |
| `set_decoy_mode` | — | Toggle the runtime `DECOY_MODE` write-refusal guard. |
| `get_managed_policy` | `ManagedPolicy` | Read-only GPO/ADMX managed-policy overlay (HKLM). |

### Backend dispatch & shell

| Command | Tier | Purpose |
|---------|------|---------|
| `run_backend_script` | Free/Paid | Central toggle dispatcher — decrypts + runs a PS module, or forwards to Pro if the command is paid. |
| `search_everything` | Free | Filename search across drives. |
| `get_file_icon_data` | Free | Resolve a file's icon as image data for the UI. |
| `open_path` | Free | Open a path in Explorer / the default handler. |
| `is_path_dir` | Free | Whether a path is a directory. |
| `toggle_context_menu` / `get_context_menu_status` | Free | Add/remove + query the WinCommander shell context-menu entry. |
| `toggle_scrub_context_menu` / `get_scrub_context_menu_status` | Free | Add/remove + query the "Share Safely" scrub context-menu entry. |
| `run_bleachbit_clean` | Free | Run a BleachBit-backed clean. |

### Lockdown, identity & app window

| Command | Tier | Purpose |
|---------|------|---------|
| `lockdown` | Free | Single-step lockdown action. |
| `full_lockdown` | Paid | Full destruct cascade; dispatches by stable step ID (`run_destruct_step`) to Pro. |
| `lock_to_calculator` | Free | Enter calculator cover immediately (dashboard lock button). |
| `set_capture_protection` | Paid | Apply `WDA_EXCLUDEFROMCAPTURE` to WinCommander's own window. |
| `apply_wincommander_hide_mode` | Free | Hide/show the app window + tray and persist the hidden-mode flag. |
| `wincommander_hidden_status` | Free | Report the actual hidden state (flag-file truth). |
| `set_app_display_label` | Free | Rewrite the Windows DisplayName + Start Menu shortcut (Free/Pro edition label). |
| `update_panic_hotkey` | Free | Register/update the global lockdown hotkey. |
| `update_search_hotkey` | Free | Register/update the global search-overlay hotkey. |
| `update_hide_hotkey` | Free | Register/update the global hide/peek hotkey. |
| `update_tray_shield_label` | Free | Sync the tray "Enable/Disable Privacy Shield" label. |
| `exit_app` | Free | Quit the application. |

### Startup PIN gate (Calculator cover)

| Command | Tier | Purpose |
|---------|------|---------|
| `verify_startup_pin` | Paid | Verify a PIN and resolve real/decoy/destroy mode. |
| `startup_pin_is_configured` | Paid | Whether a Real PIN is set. |
| `register_startup_pin` | Paid | Set/replace a real/decoy/destroy PIN. |
| `set_startup_pin_enabled` | Paid | Enable/disable the PIN gate. |
| `clear_startup_pin` | Paid | Remove a configured PIN. |
| `enter_calculator_mode` / `exit_calculator_mode` | Paid | Resize + retitle the window to/from the calculator cover. |

### License & entitlement

| Command | Purpose |
|---------|---------|
| `get_license_status` | Current entitlement (licensed / trial / grace / free). |
| `get_license_api_base` | Resolved license-server base URL. |
| `activate_license` | Activate with a license key (device-bound). |
| `refresh_license` | Re-verify / refresh the cached JWT. |
| `deactivate_license` | Release this device's activation. |
| `clear_license_cache` | Drop the local license cache. |
| `start_trial` / `clear_trial` | Begin / clear the client-side trial record. |

### Pro sidecar & install

| Command | Purpose |
|---------|---------|
| `test_pro_handshake` | Spawn Pro and verify the handshake (diagnostic). |
| `test_pro_dispatch` | Round-trip a request to Pro (diagnostic). |
| `get_pro_install_status` | Whether the Pro EXE is installed + its path. |
| `install_pro_binary` | Pinned-host download + SHA-256 verify + atomic install of Pro. |
| `fetch_pro_manifest` | Fetch the Pro release manifest. |
| `delete_pro_binary` | Remove the installed Pro EXE. |
| `get_defender_status` | Microsoft Defender status (gates the Pro install UX). |

### Updates

| Command | Purpose |
|---------|---------|
| `app_check_for_updates_doh` | DoH-fronted update check (defeats DNS blocks; minisign-verified downstream). |
| `app_install_update_doh` | DoH-fronted download + install of the available update. |
| `app_install_staged_update` | Install a previously staged update artifact. |
| `get_public_ip_trace` | Cloudflare-trace public-IP lookup (ipify fallback). |

### Flow engine — v2 (Pro-backed, `flow_bridge.rs`, current)

Paid (`require_paid("flows")` on every command). Rules persist to `settings.app.proFlows`
(separate store from the legacy `app.flows[]` below, so the two engines never double-fire the
same trigger). Full architecture: [docs/flows.md](flows.md).

| Command | Returns | Purpose |
|---------|---------|---------|
| `flow_list_rules` | `Value[]` | List the raw `flow-core::Rule` JSON. |
| `flow_save_rule` | `Value` | Create/update a rule by `id`; persists then re-syncs the whole set to Pro (`Flow-Sync-Rules`). Refuses a fleet-locked rule or a policy-locked device. |
| `flow_delete_rule` | `Value` | Delete by id; same fleet/policy-lock refusal. |
| `flow_set_enabled` | `Value` | Toggle a rule on/off; same refusal. |
| `flow_fire_now` | — | Manual test trigger — currently re-syncs to Pro and emits a `flow-fired-manually` UI event; true per-rule manual dispatch through the Pro engine is not yet wired. |

There is no v2 equivalent of `get_flow_executions` — the v2 execution log is a frontend-only
in-memory event stream (`flow-executed`/`flow-log` events consumed by `useFlowsV2.ts`), not a
backend-buffered command.

### Flow engine — legacy (`flow_engine.rs`, 4 pre-seeded system flows only)

Free tier. Still owns `settings.app.flows[]` and the four built-in system flows
(`contingency`/`panic-hotkey`/`lid-guard`/`usb-guard`) — not the surface for new user
automations (that's v2, above).

| Command | Returns | Purpose |
|---------|---------|---------|
| `get_flows` | `Flow[]` | Load all saved flows from settings. |
| `save_flow` | — | Create/update a flow; validates + restarts listeners. |
| `delete_flow` | — | Remove a user flow (system flows reject). |
| `toggle_flow` | — | Enable/disable a flow's background listener. |
| `fire_flow` | — | Manually invoke a flow (skips trigger); `dryRun` simulates — destructive actions become no-ops, NotifyAction fires with a `[DRY RUN]` prefix, DelayAction still waits. |
| `get_flow_executions` | `FlowExecution[]` | Last 50 execution records from the in-memory ring buffer. |
| `reload_flows` | — | Tear down + re-arm all listeners; recovers a stuck listener. |
| `list_backend_commands` | `string[]` | Names of every callable backend command (CommandPicker intersects against curated `FLOW_COMMANDS`). |
| `list_usb_devices` | JSON | Snapshot of attached USB devices for the USBTrigger config UI. |
| `preflight_validate_flow` | `PreflightReport` | Same validation as `save_flow`, without persisting; `{errors, warnings, info}`. |
| `export_flow_bundle` | JSON string | Ed25519-sign selected flows with the device key (system flows rejected). |
| `verify_flow_bundle` | `FlowBundleVerification` | Parse + signature-verify a pasted bundle; never throws on bad signature. |
| `import_flow_bundle` | number | Verify-then-import; fresh ids, `enabled:false`; hard-fails on bad signature. |
| `get_flow_signer_pubkey` | string (base64) | This device's Ed25519 verifying key (lazily generated). |
| `probe_flow_capabilities` | `CapabilityReport` | Re-probe every subsystem a Flow depends on; cheap, do not cache. |
| `get_flow_health` | `FlowHealth[]` | Per-flow readiness snapshot (listener armed, last status, preflight counts). |
| `start_file_watch_triggers` / `stop_file_watch_triggers` | — | Arm/disarm the F-6 file-watch lockdown trigger. |

### Disk space analyzer

| Command | Returns | Purpose |
|---------|---------|---------|
| `run_disk_scan` | `ScanMeta` | Run a disk-usage scan of a root path; returns totals. |
| `get_disk_children` | `DiskNode[]` | Children of a directory from the in-memory scan map. |
| `get_large_disk_items` | `LargeDiskItem[]` | Largest files/dirs above a size threshold (filterable). |
| `disk_delete_item` | — | Delete a path surfaced by the analyzer. |

### File-content search (FTS, Free)

| Command | Purpose |
|---------|---------|
| `search_content` | Full-text search over the tantivy BM25 content index. |
| `content_index_status` | Index health / progress. |
| `content_index_configure` | Set indexed roots + exclusions. |
| `content_rescan` | Incremental re-crawl for new/changed/missed files, without wiping the index (search stays live). |
| `content_reindex` | Rebuild the content index from scratch (wipes first; also clears removed files). |
| `content_get_doc` | Fetch an indexed document by `doc_id`. |

### System metrics & live monitoring

| Command | Purpose |
|---------|---------|
| `get_live_metrics` | Live CPU/memory/network sample (Rust-native). |
| `get_top_processes` | Top processes by resource use. |
| `get_drive_smart_health` | Per-drive SMART health. |
| `metric_alerts_set_config` / `metric_alerts_get_config` | Per-metric alerting config (Paid setter). |
| `get_recent_downloads` | Recent items from the Downloads folder watcher. |

### Network & RDP

| Command | Tier | Purpose |
|---------|------|---------|
| `internet_kill_switch_set` / `internet_kill_switch_get` | Free | In-process firewall internet block toggle + state. |
| `vpn_kill_switch_arm` / `vpn_kill_switch_status` | Free | Arm + query the VPN-drop kill switch (Tailscale/ProtonVPN watchdog). |
| `get_ping_block_status` / `set_ping_block` | Paid | ICMP/ping block quick-toggle. |
| `connect_rdp` / `set_rdp_credentials` | Free | Launch an RDP session / stash credentials. |
| `kill_mstsc_processes` | Free | Terminate active `mstsc` RDP client processes. |
| `kill_privacy_shield_process` | Free | Terminate the Privacy Shield child process. |
| `get_system_idle_seconds` | Free | Seconds since last user input (idle detection). |

### Privacy Shield quota (Free tier, 15 min/day)

| Command | Purpose |
|---------|---------|
| `get_shield_quota` | Remaining daily Privacy Shield minutes. |
| `consume_shield_minutes` | Decrement the daily counter. |
| `reset_shield_quota` | Reset the counter. |

### Anti-coercion monitors

| Command group | Tier | Purpose |
|---------------|------|---------|
| `start_paste_monitor` … `cancel_paste_monitor_snooze` (F-1) | Paid | Clipboard credential watcher — start/stop/status, category + snooze + recent config, crypto-swap and auto-clear/auto-clear-on-lock toggles. |
| `start_decoy_monitor` … `enable_last_access_tracking` (F-2) | Paid | Decoy-file sentinel — start/stop/status, enroll/remove/list/delete decoys, drop standard decoys, recent log, last-access tracking. |
| `start_ransomware_monitor` … `set_ransomware_watch_dirs` (F-3) | Paid | Mass-modify (ransomware) detector — start/stop/status, config, recent, watched/extra dirs. |
| `start_lockdown_words` … `test_fire_lockdown_words` (F-5) | Paid | Lockdown-word keyboard trigger — start/stop/status, register/set/list words, test-fire. |
| `register_distress_phrase` / `set_distress_phrases` / `list_distress_phrases` / `check_distress_phrase` | Paid | Distress-phrase registration + match check (keyboard-hook + palette). |

### Threat & anomaly detectors

| Command group | Tier | Purpose |
|---------------|------|---------|
| `start_network_honeypot` … `remove_network_honeypot_custom_port` | Paid | Internal-recon honeypot — start/stop/status, recent, bind-all-interfaces, per-port enable + custom ports. |
| `start_wifi_guard` … `add_wifi_guard_ssid` | Paid | Rogue-AP / Wi-Fi guard — start/stop/status, recent, known-AP list + add SSID. |
| `start_remote_access_monitor` … `set_remote_access_tool_enabled` | Paid | Remote-session monitor — start/stop/status, recent, tool list + enable. |
| `start_screen_capture_watch` … `clear_recent_screen_capture` | Paid | Screen-capture watch — start/stop/status, recent. |
| `get_driver_health` / `start_driver_watch` / `stop_driver_watch` / `driver_watch_status` / `get_vulnerable_drivers` / `open_device_manager` | Paid (Device Manager Free) | Driver-health scan/watch + BYOVD/loldrivers check; `open_device_manager` is Free. |
| `start_auth_anomaly_monitor` … `clear_auth_anomaly_recent` | Paid | Login/auth-anomaly monitor (4625/4720/4624+4778/off-hours) — start/stop/status, recent. |

### Disposable isolation (VM / Sandbox)

| Command | Purpose |
|---------|---------|
| `vm_capabilities` | Hyper-V / Windows Sandbox availability. |
| `vm_enable_feature` | Enables the server-selected Hyper-V or Windows Sandbox feature without forcing an immediate restart. |
| `vm_list` | List managed disposable VMs. |
| `vm_create` / `vm_start` / `vm_stop` / `vm_destroy` | Lifecycle of a disposable Hyper-V VM. |
| `sandbox_launch` / `sandbox_close` | Launch/close a Windows Sandbox instance. |

All Paid, dispatched to Pro's `vm_sandbox.rs`.

### USB control suite

| Command group | Tier | Purpose |
|---------------|------|---------|
| `start_usb_monitor` … `set_usb_monitor_notify` (U-A) | Paid | Device attach/detach timeline. |
| `start_usb_metering` … `set_usb_metering_config` (U-B) | Paid | Data-transfer metering. |
| `start_usb_hid_guard` … `clear_usb_hid_alerts` (U-C) | Paid | BadUSB / HID-injection guard (detection only). |
| `usb_device_trust_score` | Free | Read-only 0-100 score combining identity stability, vendor signal, HID alerts, quarantine history, and transfer volume. |
| `block_usb_device` / `allow_usb_device` / `set_usb_volume_readonly` / `quarantine_usb_device` (U-D/E) | Paid | Trust-policy enforcement (dispatched to Pro). |
| `start_usb_autosandbox` … `clear_usb_autosandbox_recent` (U-F) | Free decision layer | Auto-sandbox / quarantine orchestration — start/stop/status, config, recent. |

### Session Assurance (insider-risk / attention)

| Command | Purpose |
|---------|---------|
| `start_session_monitor` / `stop_session_monitor` / `session_monitor_status` | Lifecycle of the attention monitor (Paid, Pro collector). |
| `get_session_score` | Current session-assurance score. |
| `get_active_alerts` | Active alerts. |
| `get_consent_status` / `request_consent` / `revoke_consent` | Deny-by-default consent gate. |

### Argus collectors (aggregate-only, Paid)

All Argus collectors enforce the privacy invariant: window titles, exe paths, URLs, filenames, printer/document/user names are used locally but never serialized onto the wire or sent to the fleet — only aggregate scalars leave the device.

| Command group | Purpose |
|---------------|---------|
| `argus_app_usage_start` / `_stop` / `_status` / `_recent` | App-usage / idle productivity collector. |
| `argus_print_usb_start` / `_stop` / `_status` / `_recent` | Print + removable-media collector. |
| `argus_tamper_start` / `_stop` / `_status` / `_recent` | Tamper / evasion collector. |
| `argus_dlp_start` / `_stop` / `_status` / `_recent` | DLP-lite / exfil collector. |

### Canary tokens (Paid)

| Command | Purpose |
|---------|---------|
| `generate_canary` / `list_canaries` / `delete_canary` | Manage canary-token artifacts (`.docx`/`.url`). |
| `start_canary_listener` / `stop_canary_listener` / `canary_listener_status` | TCP beacon listener lifecycle. |
| `get_canary_recent` / `clear_canary_recent` | Recent canary hits. |

### Evidence (record + WORM vault)

| Command | Tier | Purpose |
|---------|------|---------|
| `evidence_record` / `evidence_read` / `evidence_clear` | Free | Append/read/clear the local evidence record (read/clear no-op in decoy mode). |
| `export_evidence_vault` | Investigator | WORM export — SHA-256 hash-chain + Ed25519 bundle signature (Pro). |
| `verify_evidence_vault` | Free | Re-walk the chain + verify the bundle signature (ungated). |
| `export_evidence_affidavit` | Investigator | One-page PDF affidavit of the bundle (Pro). |

### Continuity (inactivity timer)

| Command | Purpose |
|---------|---------|
| `get_dead_mans_switch_config` | Persisted config (enabled, thresholdDays, flowId, lastActivity/Fired). |
| `set_dead_mans_switch_config` | Persist config; clamps `thresholdDays` to [1, 365]. |
| `reset_dead_mans_switch_timer` | "I'm alive" — stamp `lastActivityAt = now` (also called at startup). |
| `clear_dead_mans_switch_fired` | Operator-only re-arm after a trip (never automatic). |

### Metadata scrubber & print audit

| Command | Tier | Purpose |
|---------|------|---------|
| `get_metadata_scrubber_status` | Free | Whether ExifTool is installed + version + path. |
| `scrub_metadata_paths` | Free | "Share Safely" metadata scrubber — delegates to ExifTool across ~140 formats; output to `<dir>/_scrubbed/`. |
| `get_print_audit_status` | Free | PrintService/Operational channel state. |
| `set_print_audit_enabled` | Free | Flip the print-audit channel via `wevtutil` (admin). |
| `get_print_audit_log` | Free | Recent Event-307 print-job records via `Get-WinEvent`. |

### Runtime Visibility Manager

| Command | Purpose |
|---------|---------|
| `scan_runtimes` | Discover WinCommander runtime artifacts. |
| `enumerate_services` / `enumerate_scheduled_tasks` | Enumerate related services / tasks. |
| `runtime_visibility_state` | Current visibility state. |
| `hide_runtime` / `hide_runtime_list` / `restore_runtime` / `restore_all_runtimes` | Hide/restore individual or all runtimes. |
| `set_global_runtime_visibility` | Global show/hide. |

### Fleet agent onboarding (Paid)

| Command | Purpose |
|---------|---------|
| `fleet_connect` / `fleet_disconnect` / `fleet_status` | Enroll/leave/query the fleet server (persists `app.fleet`, dispatches to Pro). |
| `fleet_apply_pending_epoch` | Apply a pending config epoch. |
| `fleet_update_posture_snapshot` | Push a posture snapshot. |
| `fleet_request_unenroll` | Request device unenrollment. |

### AI Security Advisor & appearance

| Command | Purpose |
|---------|---------|
| `advisor_build_context` | Assemble local context for the AI Security Advisor (Free). |
| `decoy_mode_set` / `decoy_mode_get` | Appearance / rebrand mode (Paid setter). |
| `appearance` rebrand is distinct from `set_decoy_mode` (the write-refusal guard). |

### Logging, notifications & autostart

| Command | Purpose |
|---------|---------|
| `write_log_record` / `get_log_records` / `clear_log_records` | Encrypted in-app log (read/clear return empty / no-op in decoy mode). |
| `show_native_test_notification` / `show_test_notification_kind` | Native toast test paths. |
| `dismiss_notification_toast` / `dismiss_notification_toast_id` | Dismiss a toast. |
| `show_rdp_idle_warning_native` | Native RDP-idle warning toast. |
| `ensure_autostart_task` / `remove_autostart_task` / `update_autostart_task_identity` | Manage the per-machine autostart scheduled task. |
| `ensure_attend_watch_task` / `remove_attend_watch_task` | Manage the SYSTEM attend-watch task (RDP idle → vault dismount). |
| `is_dev_build` | Dev-build sentinel (always present; `false` in release). |

### Server apps (native webviews)

| Command | Purpose |
|---------|---------|
| `open_server_app` | Open a self-hosted server app in a native webview. |
| `resize_server_app` | Resize an open server-app webview. |
| `hide_all_server_apps` / `close_server_app` / `close_all_server_apps` | Hide/close server-app webviews. |

## Free ↔ Pro named-pipe dispatch (paid commands)

When `run_backend_script` (or a `require_paid` handler) determines a command is paid, `commander-free` spawns `wincommander-pro.exe` and forwards the call over a per-spawn Windows named pipe, then returns Pro's response verbatim. The handshake, length-prefixed JSON envelopes, HMAC-SHA256 per-frame signing, pinned binary-hash trust, and notification re-emission are all detailed in:

- [ARCHITECTURE.md — Free ↔ Pro IPC](../ARCHITECTURE.md#free--pro-ipc) — transport, framing, handshake, signing, trust model.
- [`wincmd-shared/src/lib.rs`](../src-tauri/wincmd-shared/src/lib.rs) — wire-format source of truth (`Envelope`, `read_envelope`/`write_envelope`, `sign_body`/`verify_body`).
- [`commander-free/src/sidecar.rs`](../src-tauri/commander-free/src/sidecar.rs) — Free-side broker (`ProSession`, `dispatch_paid_command`).

Paid commands are dispatched on the Pro side by `feature_id` in `commander-pro`'s handlers; the Free-side wrappers in the catalog above are the thin `require_paid` + dispatch stubs.

## See also

- [Architecture](../ARCHITECTURE.md) — binary model, data flow, and the Free ↔ Pro wire format.
- [Features](../FEATURES.md) — capability inventory with entry points.
- [Settings reference](settings-reference.md) — the settings schema these commands read and write.
- [Flows](flows.md) — the Flow engine triggers/conditions/actions that drive many of these commands.
- [Open core](../OPEN_CORE.md) — Free vs Pro tier rationale.
