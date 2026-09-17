import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as { scripts: Record<string, string> };
const baseConfig = JSON.parse(readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8")) as {
  bundle: {
    resources: string[];
    targets: string | string[];
    windows: { nsis: { installMode: string; installerHooks?: string } };
  };
};
const releaseTool = readFileSync("tools/build-tauri-release.ts", "utf8");
const manifest = readFileSync("src-tauri/commander-free/app.manifest", "utf8");
const buildScript = readFileSync("src-tauri/commander-free/build.rs", "utf8");
const releaseWorkflow = readFileSync(".github/workflows/release.yml", "utf8");

describe("Free per-user release packaging", () => {
  test("uses the standard-user NSIS installation mode without machine hooks", () => {
    expect(baseConfig.bundle.targets).toBe("nsis");
    expect(baseConfig.bundle.windows.nsis.installMode).toBe("currentUser");
    expect(baseConfig.bundle.windows.nsis.installerHooks).toBeUndefined();
  });

  test("does not bundle or register a service or kernel driver during normal setup", () => {
    expect(baseConfig.bundle.resources).not.toContain("resources/wincommander-svc.exe");
    expect(releaseTool).not.toContain("commander-svc");
    expect(releaseTool).not.toContain("wincommander-svc.exe");
    expect(releaseTool).toContain('const contextShredResource = "resources/wincommander-context-shred.exe"');
    expect(releaseTool).toContain("copyFileSync(contextShredBuildPath, stagedContextShredPath)");
    expect(releaseTool).toContain('config.bundle.targets = ["nsis"]');
    expect(packageJson.scripts["build:free:release-installer"]).toContain("bun run tools/build-tauri-release.ts");
  });

  test("keeps the desktop process and bundled Explorer helper at the caller's privilege", () => {
    expect(manifest).toContain('requestedExecutionLevel level="asInvoker"');
    expect(manifest).not.toContain("highestAvailable");
    expect(manifest).not.toContain("requireAdministrator");
    expect(buildScript).toContain('const AS_INVOKER_LEVEL: &str = r#"level="asInvoker""#;');

    const helperManifest = readFileSync("src-tauri/commander-context-shred/app.manifest", "utf8");
    expect(helperManifest).toContain('requestedExecutionLevel level="asInvoker"');
    expect(helperManifest).not.toContain("highestAvailable");
    expect(helperManifest).not.toContain("requireAdministrator");
  });

  test("runs the release setup and uninstaller without RunAs and rejects machine-service side effects", () => {
    expect(releaseWorkflow).not.toContain("Verify Free setup installs and removes WinCommanderSvc");
    expect(releaseWorkflow).toContain("Verify Free setup installs and removes only the current-user installation");
    expect(releaseWorkflow).toContain('Join-Path $env:LOCALAPPDATA "WinCommander\\wincommander-free.exe"');
    expect(releaseWorkflow).toContain('Join-Path $env:LOCALAPPDATA "WinCommander\\uninstall.exe"');
    expect(releaseWorkflow).toContain("The Free setup unexpectedly created WinCommanderSvc.");

    const verification = releaseWorkflow.slice(
      releaseWorkflow.indexOf("Verify Free setup installs and removes only the current-user installation"),
      releaseWorkflow.indexOf("Verify bundled shared media"),
    );
    expect(verification).not.toContain("-Verb RunAs");
    expect(verification).not.toContain("$env:ProgramFiles");
  });
});
