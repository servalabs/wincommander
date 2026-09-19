import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Clipboard clear-on-lock preference", () => {
  test("persists the user preference and keeps runtime sync wired", async () => {
    const [panel, app, hook, settings, scopes] = await Promise.all([
      Bun.file("src/panels/privacy/PasteMonitorSection.tsx").text(),
      Bun.file("src/App.tsx").text(),
      Bun.file("src/hooks/usePasteMonitor.ts").text(),
      Bun.file("src-tauri/commander-free/src/settings.rs").text(),
      Bun.file("src-tauri/commander-free/src/settings_scope.rs").text(),
    ]);

    expect(panel).toContain("pasteMonitorAutoClearOnLock: e.currentTarget.checked");
    expect(app).toContain("pasteMonitorAutoClearOnLock");
    expect(hook).toContain('invoke("set_paste_monitor_auto_clear_on_lock", { enabled: autoClearOnLock })');
    expect(settings).toContain("pub paste_monitor_auto_clear_on_lock: Option<bool>");
    expect(scopes).toContain('path: "privacy.clipboard.pasteMonitorAutoClearOnLock"');
    expect(scopes).toContain("scope: SettingsScope::User");
  });
});
