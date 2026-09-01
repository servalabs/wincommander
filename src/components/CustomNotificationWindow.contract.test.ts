import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Privacy Shield notification presentation", () => {
  test("raises the alert only after its local renderer has shown it", async () => {
    const [renderer, native] = await Promise.all([
      Bun.file("src/components/CustomNotificationWindow.tsx").text(),
      Bun.file("src-tauri/commander-free/src/native_notify.rs").text(),
    ]);

    const showIndex = renderer.indexOf("windowRef.show()");
    const presentIndex = renderer.indexOf('invoke("present_notification_window")');
    expect(showIndex).toBeGreaterThan(-1);
    expect(presentIndex).toBeGreaterThan(showIndex);
    expect(renderer).toContain("requestAnimationFrame");
    expect(native).toContain("pub fn present_notification_window");
    expect(native).toContain("SWP_SHOWWINDOW");
    expect(native).toContain("SWP_NOACTIVATE");
  });
});
