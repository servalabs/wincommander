import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as { scripts: Record<string, string> };
const baseConfig = JSON.parse(readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8")) as {
  bundle: {
    resources: string[];
    targets: string | string[];
    windows: {
      nsis: {
        installMode: string;
        installerHooks?: string;
        startMenuFolder?: string;
      };
    };
  };
};
const releaseTool = readFileSync("tools/build-tauri-release.ts", "utf8");
const manifest = readFileSync("src-tauri/commander-free/app.manifest", "utf8");
const buildScript = readFileSync("src-tauri/commander-free/build.rs", "utf8");
const releaseWorkflow = readFileSync(".github/workflows/release.yml", "utf8");

describe("Free machine-wide release packaging", () => {
  test("uses a shared Program Files install and a product-specific Start Menu location", () => {
    expect(baseConfig.bundle.targets).toBe("nsis");
    expect(baseConfig.bundle.windows.nsis.installMode).toBe("perMachine");
    expect(baseConfig.bundle.windows.nsis.installerHooks).toBe("nsis/hooks.nsh");
    // The app lives in Program Files, so every Windows account gets one safe,
    // shared executable. A product folder avoids a collision with root-level
    // shortcuts left by earlier releases.
    expect(baseConfig.bundle.windows.nsis.startMenuFolder).toBe("ServaLabs\\WinCommander");
  });

  test("bundles its machine service but leaves the entitled Pro helper to its verified runtime installer", () => {
    expect(baseConfig.bundle.resources).not.toContain("resources/wincommander-svc.exe");
    expect(baseConfig.bundle.resources).not.toContain("resources/wincommander-pro.exe");
    expect(baseConfig.bundle.resources).not.toContain("resources/EncVolKm.sys");
    expect(releaseTool).toContain("commander-svc");
    expect(releaseTool).toContain("wincommander-svc.exe");
    expect(releaseTool).not.toContain("commander-pro");
    expect(releaseTool).not.toContain("wincommander-pro.exe");
    expect(releaseTool).not.toContain('run(["bun", "run", "hash-pro"], "WinCommander Pro service-helper hash")');
    expect(releaseTool).not.toContain("EncVolKm.sys");
    expect(releaseTool).toContain('const contextShredResource = "resources/wincommander-context-shred.exe"');
    expect(releaseTool).toContain("copyFileSync(contextShredBuildPath, stagedContextShredPath)");
    expect(releaseTool).toContain('config.bundle.targets = ["nsis"]');
    expect(releaseTool).toContain('CARGO_PROFILE_RELEASE_LTO: "false"');
    expect(packageJson.scripts["build:free:release-installer"]).toContain("bun run tools/build-tauri-release.ts");

    const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");
    expect(hooks).toContain("sc.exe create ${WC_SERVICE_NAME}");
    expect(hooks).toContain("WC_STOP_OWNED_SERVICE_OR_ABORT");
    expect(hooks).toContain("installer-lifecycle.log");
    expect(hooks).toContain('nsExec::ExecToStack \'sc.exe query ${WC_SERVICE_NAME}\'');
    expect(hooks).not.toContain("cmd.exe /c sc query ${WC_SERVICE_NAME} ^| findstr");
    expect(hooks).toContain('ReadEnvStr $R6 "LOCALAPPDATA"');
    expect(hooks).toContain('RMDir /r "$R6\\WinCommander"');
    expect(hooks).not.toContain("WC_PRO_PAYLOAD");
    expect(hooks).not.toContain("WC_PRO_EXE");
    expect(hooks).toContain('net.exe localgroup "WinCommander Vault Policy Administrators" /add');
    expect(hooks).toContain('cmd.exe /c net.exe localgroup "WinCommander Vault Policy Administrators" "%USERNAME%" /add');
    expect(hooks).not.toContain("sc.exe delete WinCommanderEncVol");
    expect(hooks).not.toContain("sc.exe delete VeraCrypt");
  });

  test("builds the Free installer without a private Pro workspace or token", () => {
    expect(releaseWorkflow).not.toContain("Checkout required private Pro sidecar source");
    expect(releaseWorkflow).not.toContain("WINCOMMANDER_PRO_READ_TOKEN");
    expect(releaseWorkflow).not.toContain("servalabs/wincommander-pro");
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

  test("runs a machine-wide release setup and uninstaller with service lifecycle and cleanup that preserves the license", () => {
    expect(releaseWorkflow).toContain("Verify Free setup installs and removes the machine-wide installation");
    expect(releaseWorkflow).toContain('Join-Path $env:ProgramFiles "WinCommander\\wincommander-free.exe"');
    expect(releaseWorkflow).toContain('Join-Path $env:ProgramFiles "WinCommander\\uninstall.exe"');
    expect(releaseWorkflow).toContain("The Free setup did not create and start WinCommanderSvc.");
    expect(releaseWorkflow).toContain("The NSIS uninstaller did not remove WinCommanderSvc.");

    const verification = releaseWorkflow.slice(
      releaseWorkflow.indexOf("Verify Free setup installs and removes the machine-wide installation"),
      releaseWorkflow.indexOf("Verify bundled shared media"),
    );
    expect(verification).toContain("-Verb RunAs");
    expect(verification).toContain("NSIS lifecycle diagnostic:");
    expect(verification).not.toContain("$env:LOCALAPPDATA");

    const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");
    expect(hooks).toContain('RMDir /r "$PROGRAMDATA\\WinCommander"');
    expect(hooks).toContain('license_cache.json');
    expect(hooks).toContain('icacls.exe "$PROGRAMDATA\\WinCommander" /inheritance:r');
    expect(hooks).toContain('RMDir /r "$LOCALAPPDATA\\WinCommander"');
  });
});
