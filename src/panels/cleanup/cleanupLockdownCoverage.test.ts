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

describe("System Cleanup lockdown coverage", () => {
  test("all scanable cleanup categories use the shared details viewer by default", async () => {
    const panel = await Bun.file("src/panels/cleanup/SystemCleanupPanel.tsx").text();

    expect(panel).toContain("Object.fromEntries(");
    expect(panel).toContain(".filter((category) => !category.actionOnly && !!category.getDataKey)");
    expect(panel).toContain("openSharedDetails(category.id)");
  });

  test("completed scans collect every clean card in one final compact section", async () => {
    const grid = await Bun.file("src/panels/cleanup/CleanupCategoryGrid.tsx").text();
    const card = await Bun.file("src/components/cleanup/CleanupTraceCard.tsx").text();

    expect(grid).toContain("const cleanUserCats = userCats.filter(isUserClean)");
    expect(grid).toContain("const totalCleanCards = cleanCats.length + cleanUserCats.length");
    expect(grid).toContain("Clean cards");
    expect(grid).toContain("{cleanUserCats.map((cat) => renderUserCard(cat, true))}");
    expect(card).toContain("height: compact ? 44 : TRACE_CARD_HEIGHT");
  });

  test("every expanded cleanup category is configurable and dispatched through the guarded lockdown path", async () => {
    const frontend = await Bun.file("src/types/lockdownSteps.ts").text();
    const free = await Bun.file("src-tauri/commander-free/src/action_steps.rs").text();
    const pro = await Bun.file("../wincommander-pro/commander-pro/src/handlers.rs").text();

    for (const id of EXPANDED_CATEGORY_IDS) {
      expect(frontend).toContain(`id: "${id}"`);
      expect(free).toContain(`id: "${id}"`);
      expect(pro).toContain(`"${id}"`);
    }
  });
});
