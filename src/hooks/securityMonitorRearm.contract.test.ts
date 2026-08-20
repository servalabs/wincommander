import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const root = decodeURIComponent(new URL("../../", import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, "$1");
const source = (path: string) => readFileSync(`${root}/${path}`, "utf8");

describe("security monitor startup contracts", () => {
  test("ransomware applies policy and folders before arming", async () => {
    const hook = source("src/hooks/useRansomwareMonitor.ts");
    const config = hook.indexOf('await invoke("set_ransomware_config"');
    const folders = hook.indexOf('await invoke("set_ransomware_watch_dirs"');
    const arm = hook.indexOf('await invoke(enabled ? "start_ransomware_monitor"');
    expect(config).toBeGreaterThan(-1);
    expect(folders).toBeGreaterThan(config);
    expect(arm).toBeGreaterThan(folders);
    expect(hook).toContain("attempt < 3");
  });

  test("decoy read auditing follows the persisted setting", async () => {
    const hook = source("src/hooks/useDecoyMonitor.ts");
    expect(hook).toContain('{ enabled: readAuditEnabled }');
    expect(hook).not.toContain('{ enabled: true }');
  });

  test("USB dependent guards rearm only after the attach monitor", async () => {
    const app = source("src/App.tsx");
    const blockStart = app.indexOf("// USB monitor arm state is persisted");
    const blockEnd = app.indexOf("// Wi-Fi Guard retains", blockStart);
    const block = app.slice(blockStart, blockEnd);
    expect(block.indexOf('await invoke("start_usb_monitor")')).toBeGreaterThan(-1);
    expect(block.indexOf('"start_usb_hid_guard"')).toBeGreaterThan(
      block.indexOf('await invoke("start_usb_monitor")'),
    );
    expect(block.indexOf('"start_usb_autosandbox"')).toBeGreaterThan(
      block.indexOf('"start_usb_hid_guard"'),
    );
  });

  test("saved screen-capture detection is reconciled at app startup", async () => {
    const app = source("src/App.tsx");
    expect(app).toContain('? "start_screen_capture_watch"');
    expect(app).toContain("Screen-capture detection could not re-arm");
  });
});
