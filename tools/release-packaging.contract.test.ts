import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as { scripts: Record<string, string> };
const baseConfig = JSON.parse(readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8")) as {
  bundle: { resources: string[] };
};
const releaseTool = readFileSync("tools/build-tauri-release.ts", "utf8");
const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");
const manifest = readFileSync("src-tauri/commander-free/app.manifest", "utf8");
const buildScript = readFileSync("src-tauri/commander-free/build.rs", "utf8");

describe("public service release packaging", () => {
  test("keeps raw Cargo checks independent of a release-only service artifact", () => {
    expect(baseConfig.bundle.resources).not.toContain("../target/release/wincommander-svc.exe");
    expect(releaseTool).toContain('["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-svc", "--release"]');
    expect(releaseTool).toContain('const serviceResource = "../target/release/wincommander-svc.exe"');
    expect(releaseTool).toContain('config.bundle.resources = [...config.bundle.resources.filter');
    expect(releaseTool).toContain('rmSync(generatedConfigPath, { force: true })');
    expect(packageJson.scripts["build:tauri:release"]).toContain("bun run tools/build-tauri-release.ts");
    expect(packageJson.scripts.build).toContain("bun run build:service:release");
  });

  test("uses a fixed quoted Program Files service path with checked lifecycle commands", () => {
    // `sc.exe` needs an argument whose value is the quoted executable path.
    // The outer quote pair is consumed by CreateProcess; the inner pair is
    // retained in SCM's ImagePath so Program Files stays a single executable.
    const scmImagePathArgument = '""""${WC_SERVICE_EXE}""""';
    const expectedImagePath = '"${WC_SERVICE_EXE}"';
    const imagePathPassedToSc = scmImagePathArgument.slice(3, -3);

    expect(hooks).toContain('!define WC_SERVICE_EXE "${WC_INSTALL_DIR}\\wincommander-svc.exe"');
    expect(hooks).toContain('StrCpy $INSTDIR "${WC_INSTALL_DIR}"');
    expect(hooks).toContain("WaitForStatus([System.ServiceProcess.ServiceControllerStatus]::Stopped");
    expect(hooks).toContain('reg.exe export "HKLM\\SYSTEM\\CurrentControlSet\\Services\\WinCommanderSvc" "${WC_SERVICE_CONFIG_BACKUP}" /y');
    expect(hooks).toContain('reg.exe import "${WC_SERVICE_CONFIG_BACKUP}"');
    expect(hooks).toContain('!define WC_SERVICE_BACKUP "${WC_INSTALL_DIR}\\wincommander-svc.exe.wc-backup"');
    expect(hooks).toContain('StrCpy $R4 "WinCommander service payload is missing; the installation was not completed."');
    expect(hooks).toContain('StrCpy $R4 "WinCommander service executable could not be backed up."');
    expect(hooks).toContain('CopyFiles /SILENT "${WC_BUNDLED_SERVICE}" "${WC_INSTALL_DIR}"');
    expect(imagePathPassedToSc).toBe(expectedImagePath);
    expect(hooks).toContain(`sc create WinCommanderSvc binPath= ${scmImagePathArgument} start= auto obj= LocalSystem`);
    expect(hooks).toContain(`sc config WinCommanderSvc binPath= ${scmImagePathArgument} start= auto obj= LocalSystem`);
    expect(hooks).toContain("sc failure WinCommanderSvc reset= 86400 actions= restart/5000/restart/5000/none/0");
    expect(hooks).toContain("sc start WinCommanderSvc");
    expect(hooks).toContain("sc delete WinCommanderSvc");
    expect(hooks).toContain('Delete "${WC_SERVICE_EXE}"');
  });

  test("lets ordinary users run with their own token while debug stays asInvoker", () => {
    expect(manifest).toContain('requestedExecutionLevel level="highestAvailable"');
    expect(manifest).not.toContain("requireAdministrator");
    expect(buildScript).toContain('const HIGHEST_AVAILABLE_LEVEL: &str = r#"level="highestAvailable""#;');
    expect(buildScript).toContain('replacen(HIGHEST_AVAILABLE_LEVEL, r#"level="asInvoker""#, 1)');
    expect(hooks).toContain("-RunLevel Limited");
  });
});
