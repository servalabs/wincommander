import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const titleBar = readFileSync("src/components/TitleBar.tsx", "utf8");
const desktopRuntime = readFileSync("src-tauri/commander-free/src/lib.rs", "utf8");
const displayLabelCommand = desktopRuntime.slice(
  desktopRuntime.indexOf("fn set_app_display_label"),
  desktopRuntime.indexOf("fn set_capture_protection"),
);

describe("standard-user desktop startup", () => {
  test("does not request UAC to update cosmetic labels", () => {
    expect(titleBar).toContain("invoke('set_app_display_label'");
    expect(displayLabelCommand).toContain('std::env::var("APPDATA")');
    expect(displayLabelCommand).not.toContain('std::env::var("ProgramData")');
    expect(displayLabelCommand).not.toContain("HKLM:");
    expect(displayLabelCommand).not.toContain("Verb RunAs");
    expect(desktopRuntime).not.toContain("elevate_display_label");
  });
});
