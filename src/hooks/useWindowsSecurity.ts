import { invoke } from "@tauri-apps/api/core";

const WINDOWS_SECURITY_SETTINGS_URI = "ms-settings:windowsdefender";

/** Open the fixed Windows Security settings page through Tauri's scoped opener permission. */
export function openWindowsSecuritySettings(): Promise<void> {
  return invoke<void>("plugin:opener|open_url", { url: WINDOWS_SECURITY_SETTINGS_URI });
}
