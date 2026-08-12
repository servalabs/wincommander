// src/hooks/useSettings.ts
//
// Imperative helpers for unified settings management.
// React components should prefer hooks/queries/useSettingsQuery.ts.

import { invoke } from '@tauri-apps/api/core';
import type { AppSettings, DeviceIdentity, DriftItem, SettingsPatch } from '../types/settings';

// ═══════════════════════════════════════════════════════════════════════
// STANDALONE FUNCTIONS (for use outside React components)
// ═══════════════════════════════════════════════════════════════════════

export async function getSettingsOnce(): Promise<AppSettings> {
  return invoke<AppSettings>('get_settings');
}

export async function getSettingOnce<T = unknown>(path: string): Promise<T> {
  return invoke<T>('get_setting', { path });
}

export async function isSettingLocked(path: string): Promise<boolean> {
  return invoke<boolean>('is_setting_locked', { path });
}

export async function getDeviceIdentity(): Promise<DeviceIdentity> {
  return invoke<DeviceIdentity>('get_device_identity');
}

export async function getSettingsHash(): Promise<string> {
  return invoke<string>('get_settings_hash_cmd');
}

export async function exportSettings(): Promise<string> {
  return invoke<string>('export_settings_cmd');
}

export async function importSettings(json: string): Promise<AppSettings> {
  return invoke<AppSettings>('import_settings_cmd', { json });
}

// Backend file I/O for a user-picked path (via @tauri-apps/plugin-dialog's
// save()/open()) — done in Rust rather than @tauri-apps/plugin-fs because
// that plugin's fs:scope capability is locked to WinCommander's own app
// folders; widening it would let any frontend JS touch arbitrary paths, not
// just this dialog-driven export/import flow.
export async function writeSettingsExportFile(path: string, content: string): Promise<void> {
  return invoke<void>('write_settings_export_file', { path, content });
}

export async function readSettingsImportFile(path: string): Promise<string> {
  return invoke<string>('read_settings_import_file', { path });
}

export async function applyAdminConfig(
  config: SettingsPatch,
  lockedPaths: string[],
  strategy: 'merge' | 'overwrite',
  configVersion: number,
): Promise<AppSettings> {
  return invoke<AppSettings>('apply_admin_config_cmd', {
    config,
    lockedPaths,
    strategy,
    configVersion,
  });
}

export async function patchSettingsOnce(patch: SettingsPatch): Promise<AppSettings> {
  return invoke<AppSettings>('patch_settings_cmd', { patch });
}

export async function getDriftReport(): Promise<DriftItem[]> {
  return invoke<DriftItem[]>('get_drift_report');
}

export async function updateCurrentState(probe: Record<string, unknown>): Promise<AppSettings> {
  return invoke<AppSettings>('update_current_state', { probe });
}
