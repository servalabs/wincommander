import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";

const root = decodeURIComponent(new URL("../../../", import.meta.url).pathname)
  .replace(/^\/([A-Za-z]:)/, "$1");
const sourcePath = `${root}/src-tauri/commander-free/src/usb_guard.rs`;
const source = readFileSync(sourcePath, "utf8");

describe("USB Free/Pro security boundary", () => {
  test("Free retains only the neutral consumer timeline implementation", () => {
    for (const retired of [
      "usb_auto_sandbox.rs",
      "usb_hid_guard.rs",
      "usb_metering.rs",
      "usb_monitor.rs",
      "usb_policy.rs",
    ]) {
      expect(existsSync(`${root}/src-tauri/commander-free/src/${retired}`)).toBe(false);
    }
    expect(source).toContain("async fn basic_snapshot");
    expect(source).not.toContain("SetWindowsHookEx");
    expect(source).not.toContain("DiskReadBytesPersec");
    expect(source).not.toContain("score = 50");
  });

  test("advanced starts are paid but expiry can always stop them", () => {
    expect(source).toContain('dispatch_paid("start_usb_metering"');
    expect(source).toContain('dispatch_paid("start_usb_hid_guard"');
    expect(source).toContain('dispatch_paid("start_usb_autosandbox"');
    expect(source).toContain('"reconcile_usb_guard"');
    expect(source).toContain('dispatch_cleanup("stop_usb_metering")');
    expect(source).toContain('dispatch_cleanup("stop_usb_hid_guard")');
    expect(source).toContain('dispatch_cleanup("stop_usb_autosandbox")');
  });
});
