import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const EXPANDED_CATEGORY_IDS = [
  "wsl_data", "docker_desktop_data", "virtual_machine_artifacts", "developer_caches",
  "credential_manager", "network_wizard_history", "wer_history", "inactive_user_protection_metadata",
  "sticky_notes", "onedrive_metadata", "spotlight_cache", "font_cache", "legacy_icon_cache",
  "game_captures", "photos_cache", "xbox_cache", "communication_caches", "editor_history",
  "git_activity", "ssh_state", "remote_access_logs", "password_manager_caches", "game_launcher_logs",
  "adobe_recent", "office_temp_files", "firewall_log", "neighbor_cache", "netbios_cache",
  "geolocation_cache", "vpn_phonebooks", "proxy_cache", "cloud_placeholders", "bits_queue",
  "cellular_history",
];

const FORENSIC_TRACE_LOCKDOWN_IDS = [
  "web_cache", "thumbnail_cache", "notification_database", "branch_cache",
  "event_transcript", "activities_timeline", "rdp_bitmap_cache", "servicing_logs",
  "device_install_logs", "usage_trace_logs", "defender_history", "third_party_security_product_logs", "forensic_tool_artifacts", "windows_policy_auth_caches", "cortana_wsa_logs", "bitlocker_recovery_temp", "nyx_application_logs", "app_launch_history",
  "office_mru", "embedded_web_cache", "p2p_update_cache", "reliability_history",
  "explorer_search_history", "search_personalization",
];

const FORENSIC_TRACE_SCHEDULE_IDS = [
  "webCache", "thumbnailDb", "notificationDb", "branchCache", "eventTranscript",
  "activitiesTimeline", "rdpBitmapCache", "servicingLogs", "deviceInstallLogs",
  "usageTraceLogs", "defenderHistory", "appLaunchHistory", "officeMru",
  "embeddedWebCache", "p2pUpdateCache", "reliabilityHistory",
  "explorerSearchHistory", "searchPersonalization",
];

async function readProCleanupSteps() {
  return Bun.file("../wincommander-pro/commander-pro/src/handlers/cleanup_steps.rs").text();
}

describe("System Cleanup lockdown coverage", () => {
  test("all scanable cleanup categories use the shared details viewer by default", async () => {
    const panel = await Bun.file("src/panels/cleanup/SystemCleanupPanel.tsx").text();

    expect(panel).toContain("Object.fromEntries(");
    expect(panel).toContain(".filter((category) => !category.actionOnly && !!category.getDataKey)");
    expect(panel).toContain("openSharedDetails(category.id)");
  });

  test("completed scans pack every four clean cards into the next grid slot", async () => {
    const grid = await Bun.file("src/panels/cleanup/CleanupCategoryGrid.tsx").text();
    const card = await Bun.file("src/components/cleanup/CleanupTraceCard.tsx").text();

    expect(grid).toContain("const cleanCardPacks = packCleanCards(");
    expect(grid).toContain("orderedScanCategories.flatMap");
    expect(grid).toContain('data-cleanup-clean-pack="true"');
    expect(grid).not.toContain("cleanCardsOpen");
    expect(card).toContain('height: compact ? "100%" : TRACE_CARD_HEIGHT');
  });

  test("every expanded cleanup category is configurable and dispatched through the guarded lockdown path", async () => {
    const frontend = await Bun.file("src/types/lockdownSteps.ts").text();
    const free = await Bun.file("src-tauri/commander-free/src/action_steps.rs").text();
    const pro = await readProCleanupSteps();

    for (const id of EXPANDED_CATEGORY_IDS) {
      expect(frontend).toContain(`id: "${id}"`);
      expect(free).toContain(`id: "${id}"`);
      expect(pro).toContain(`"${id}"`);
    }
  });

  test("all high-value forensic trace stores are exposed in lockdown and the scheduler", async () => {
    const frontend = await Bun.file("src/types/lockdownSteps.ts").text();
    const free = await Bun.file("src-tauri/commander-free/src/action_steps.rs").text();
    const pro = await readProCleanupSteps();
    const categories = await Bun.file("src/panels/cleanup/cleanupCategories.ts").text();
    const scheduler = await Bun.file("src-tauri/wincmd-shared/scripts/auto-erase.ps1").text();

    expect(frontend).toContain('id: "system_cleaner"');
    for (const id of FORENSIC_TRACE_LOCKDOWN_IDS) {
      expect(frontend).toContain(`id: "${id}"`);
      expect(free).toContain(`id: "${id}"`);
      expect(pro).toContain(`"${id}"`);
    }
    for (const id of FORENSIC_TRACE_SCHEDULE_IDS) {
      expect(categories).toContain(`'${id}'`);
      expect(scheduler).toContain(`'${id}'`);
    }
  });

  test("scheduled wipes execute real hardened payloads and overwrite the Recycle Bin before unlinking", async () => {
    const pro = await Bun.file("../wincommander-pro/commander-pro/src/handlers.rs").text();
    const scheduler = await Bun.file("src-tauri/wincmd-shared/scripts/auto-erase.ps1").text();

    expect(pro).not.toContain("-NonInteractive -Command exit");
    expect(pro).toContain('cmd @ ("Set-AutoEraseSchedule"');
    expect(pro).toContain('| "Set-MultiUserAutoEraseSchedule"');
    expect(pro).toContain('| "Remove-MultiUserAutoEraseSchedule"');
    expect(scheduler).toContain("function Set-AutoEraseDirectoryAcl");
    expect(scheduler).toContain("WinCommander\\auto-erase\\scripts");
    expect(scheduler).toContain('SetAccessRuleProtection($true, $false)');
    expect(scheduler).toContain('$scope = if ($RunAsSystem) { \'system\' } else { "user-$TargetUser" }');

    const recycleStart = scheduler.indexOf("'recycleBin'");
    const recycleEnd = scheduler.indexOf("# Deep trace analysis categories", recycleStart);
    const recycleBody = scheduler.slice(recycleStart, recycleEnd);
    expect(recycleStart).toBeGreaterThan(-1);
    expect(recycleBody.indexOf("Erase-OneFile")).toBeGreaterThan(-1);
    expect(recycleBody.indexOf("Erase-OneFile") < recycleBody.indexOf("Clear-RecycleBin")).toBe(true);
  });
});
