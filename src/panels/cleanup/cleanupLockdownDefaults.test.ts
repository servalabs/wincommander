import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const CLEANUP_EXCLUSION_TO_LOCKDOWN_STEP = {
  wlanProfiles: "wifi_profiles",
  browserFootprints: "browser_footprints",
  notepadState: "notepad_state",
  wslData: "wsl_data",
  dockerDesktopData: "docker_desktop_data",
  virtualMachineArtifacts: "virtual_machine_artifacts",
  developerCaches: "developer_caches",
  credentialManager: "credential_manager",
  sshState: "ssh_state",
  passwordManagerCaches: "password_manager_caches",
} as const;

const NON_TRACE_OR_USER_DATA_STEPS = [
  "system_cleaner",
  "dismount_volumes",
  "encryption_keys",
  "shadow_copies",
  "rdp_history",
  "rdp_passwords",
  "recyclebin_overwrite",
  "clipboard",
  "ntuser_traces",
  "windows_old",
  "crash_dumps",
  "sqlite_wal",
  "recall",
  "print_spooler",
  "web_cache",
  "search_index",
  "thumbnail_cache",
  "branch_cache",
  "p2p_update_cache",
  "notification_database",
  "rdp_bitmap_cache",
  "embedded_web_cache",
  "search_personalization",
  "virtual_memory",
  "configured_folders",
  "include_app",
  "remove_users",
] as const;

function readQuotedValues(source: string, startMarker: string, endMarker: string): string[] {
  const start = source.indexOf(startMarker);
  if (start < 0) throw new Error(`Missing source block: ${startMarker}`);
  const end = source.indexOf(endMarker, start);
  if (end < 0) throw new Error(`Missing source block terminator: ${endMarker}`);
  return Array.from(source.slice(start, end).matchAll(/["']([^"']+)["']/g), (match) => match[1]);
}

describe("System Cleanup and Lockdown default exclusions", () => {
  test("all default-excluded cleanup targets stay off in Lockdown", async () => {
    const cleanupUi = await Bun.file("src/panels/cleanup/useCleanupScan.ts").text();
    const cleanupSettings = await Bun.file("src-tauri/commander-free/src/settings.rs").text();
    const cleanupCategories = await Bun.file("src/panels/cleanup/cleanupCategories.ts").text();
    const lockdownUi = await Bun.file("src/types/lockdownSteps.ts").text();
    const lockdownRust = await Bun.file("src-tauri/commander-free/src/action_steps.rs").text();

    const expectedCleanupIds = Object.keys(CLEANUP_EXCLUSION_TO_LOCKDOWN_STEP);
    const uiCleanupIds = readQuotedValues(
      cleanupUi,
      "const DEFAULT_BULK_CLEAR_EXCLUDES = [",
      "];",
    );
    const rustCleanupIds = readQuotedValues(
      cleanupSettings,
      "fn default_bulk_clear_excludes()",
      ".into_iter()",
    );
    expect(uiCleanupIds).toEqual(expectedCleanupIds);
    expect(rustCleanupIds).toEqual(expectedCleanupIds);

    for (const [cleanupId, lockdownId] of Object.entries(CLEANUP_EXCLUSION_TO_LOCKDOWN_STEP)) {
      expect(cleanupCategories).toContain(`id: '${cleanupId}'`);

      const uiEntry = lockdownUi
        .split(/\r?\n/)
        .find((line) => line.includes(`id: "${lockdownId}"`));
      expect(uiEntry !== undefined).toBe(true);
      expect(uiEntry).toContain("defaultEnabled: false");

      const rustEntryStart = lockdownRust.indexOf(`id: "${lockdownId}",`);
      expect(rustEntryStart).toBeGreaterThan(-1);
      const rustEntryEnd = lockdownRust.indexOf("\n    },", rustEntryStart);
      expect(rustEntryEnd).toBeGreaterThan(rustEntryStart);
      expect(lockdownRust.slice(rustEntryStart, rustEntryEnd)).toContain("default_enabled: false");
    }
  });

  test("sparse Lockdown settings do not uninstall WinCommander or erase its app data", async () => {
    const lockdownUi = await Bun.file("src/types/lockdownSteps.ts").text();
    const lockdownRust = await Bun.file("src-tauri/commander-free/src/action_steps.rs").text();

    const uiEntry = lockdownUi
      .split(/\r?\n/)
      .find((line) => line.includes('id: "include_app"'));
    expect(uiEntry !== undefined).toBe(true);
    expect(uiEntry).toContain("defaultEnabled: false");

    const rustEntryStart = lockdownRust.indexOf('id: "include_app",');
    expect(rustEntryStart).toBeGreaterThan(-1);
    const rustEntryEnd = lockdownRust.indexOf("\n    },", rustEntryStart);
    expect(rustEntryEnd).toBeGreaterThan(rustEntryStart);
    expect(lockdownRust.slice(rustEntryStart, rustEntryEnd)).toContain("default_enabled: false");
  });

  test("Lockdown defaults leave user content and broad cleanup steps off", async () => {
    const lockdownUi = await Bun.file("src/types/lockdownSteps.ts").text();
    const lockdownRust = await Bun.file("src-tauri/commander-free/src/action_steps.rs").text();

    for (const lockdownId of NON_TRACE_OR_USER_DATA_STEPS) {
      const uiEntry = lockdownUi
        .split(/\r?\n/)
        .find((line) => line.includes(`id: "${lockdownId}"`));
      expect(uiEntry !== undefined).toBe(true);
      expect(uiEntry).toContain("defaultEnabled: false");

      const rustEntryStart = lockdownRust.indexOf(`id: "${lockdownId}",`);
      expect(rustEntryStart).toBeGreaterThan(-1);
      const rustEntryEnd = lockdownRust.indexOf("\n    },", rustEntryStart);
      expect(rustEntryEnd).toBeGreaterThan(rustEntryStart);
      expect(lockdownRust.slice(rustEntryStart, rustEntryEnd)).toContain("default_enabled: false");
    }
  });
});
