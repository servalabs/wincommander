import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("screen-capture protection contract", () => {
  test("limits the supported exclusion to the designated application window", async () => {
    const backend = await Bun.file("src-tauri/commander-free/src/lib.rs").text();
    const nativeReconciler = await Bun.file("src-tauri/commander-free/src/capture_protection.rs").text();
    const panel = await Bun.file("src/panels/privacy/ScreenCaptureSection.tsx").text();

    expect(backend).toContain('scope: "wincommander-main-window"');
    expect(backend).toContain("capture_protection::apply(hwnd, enabled)");
    expect(nativeReconciler).toContain("GetWindowDisplayAffinity");
    expect(nativeReconciler).toContain("SetWindowDisplayAffinity");
    expect(nativeReconciler).toContain("observed == desired");
    expect(backend).toContain("privileged capture, remote desktop, cameras");
    expect(panel).toContain("Remote");
    expect(panel).toContain("can bypass it");
    expect(panel).not.toContain("strong\n          guarantee");
  });
});
