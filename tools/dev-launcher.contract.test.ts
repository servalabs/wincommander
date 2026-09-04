import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as {
  scripts: Record<string, string>;
};
const devLauncher = readFileSync("tools/dev.ps1", "utf8");
const combinedLauncher = readFileSync("tools/dev-all.ts", "utf8");

describe("desktop development launchers", () => {
  test("synchronizes the Vault service before starting the normal desktop", () => {
    expect(packageJson.scripts["dev:tauri"]).toContain("tools/dev.ps1");
    expect(devLauncher).toContain('sync-dev-service.ps1');
    expect(devLauncher.indexOf('sync-dev-service.ps1')).toBeLessThan(
      devLauncher.indexOf('x tauri dev'),
    );
  });

  test("routes the combined developer launcher through the normal desktop launcher", () => {
    expect(combinedLauncher).toContain('"run", "dev:tauri"');
    expect(combinedLauncher).not.toContain('"x", "tauri", "dev"');
  });
});
