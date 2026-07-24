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
