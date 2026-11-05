// src/hooks/useSettings.ts
//
// Imperative helpers for unified settings management.
// React components should prefer hooks/queries/useSettingsQuery.ts.

import { invoke } from '@tauri-apps/api/core';
import type { AppSettings, DeviceIdentity, DriftItem } from '../types/settings';

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

// File selection and exported bytes are owned by Rust, never renderer input.
export interface SettingsImportFile {
  fileName: string;
  json: string;
  encrypted: boolean;
}

export async function writeSettingsExportFile(password: string): Promise<boolean> {
  return invoke<boolean>('write_settings_export_file', { password });
}

export async function readSettingsImportFile(): Promise<SettingsImportFile | null> {
  return invoke<SettingsImportFile | null>('read_settings_import_file');
}

export async function decryptSettingsImportFile(json: string, password: string): Promise<string> {
  return invoke<string>('decrypt_settings_import_file', { json, password });
}

export async function getDriftReport(): Promise<DriftItem[]> {
  return invoke<DriftItem[]>('get_drift_report');
}
