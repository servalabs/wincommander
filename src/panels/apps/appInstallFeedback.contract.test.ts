import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const installer = readFileSync("src/panels/apps/components/AppInstallerPanel.tsx", "utf8");
const toastHelpers = readFileSync("src/utils/toast.ts", "utf8");
const notificationsMenu = readFileSync("src/components/NotificationsMenu.tsx", "utf8");

describe("app install feedback", () => {
  test("reports queued, starting, in-progress, success, and error states", () => {
    expect(installer).toContain("Queued installation for");
    expect(installer).toContain("Preparing to install");
    expect(installer).toContain("Installing ${packageName");
    expect(installer).toContain("installed successfully.");
    expect(installer).toContain("Could not install");
    expect(installer).toContain('activeInstall.status === "queued"');
  });

  test("puts package status in the notification feed and shows active state on the app card", () => {
    expect(installer).toContain('kind: "notification"');
    expect(installer).toContain('role="status"');
    expect(installer).toContain('"Queued · waiting to start" : "Installing…"');
    expect(toastHelpers).toContain('pushNotification("info", message, undefined, opts?.kind, opts?.operationId)');
    expect(notificationsMenu).toContain("const notificationCount = notifs.length + runningTasks.length;");
    expect(notificationsMenu).toContain("[...alertNotifs, ...opsNotifs].map((notification) => (");
  });
});
