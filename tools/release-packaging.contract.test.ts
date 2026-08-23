import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as { scripts: Record<string, string> };
const baseConfig = JSON.parse(readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8")) as {
  bundle: { resources: string[] };
};
const releaseTool = readFileSync("tools/build-tauri-release.ts", "utf8");
const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8").replace(/\r\n/g, "\n");
const manifest = readFileSync("src-tauri/commander-free/app.manifest", "utf8");
const buildScript = readFileSync("src-tauri/commander-free/build.rs", "utf8");
const releaseWorkflow = readFileSync(".github/workflows/release.yml", "utf8");
const publishTagWorkflow = readFileSync(".github/workflows/publish-release-tag.yml", "utf8");
const prepareReleaseWorkflow = readFileSync(".github/workflows/prepare-release.yml", "utf8");

describe("public service release packaging", () => {
  test("keeps raw Cargo checks independent of a release-only service artifact", () => {
    expect(baseConfig.bundle.resources).not.toContain("../target/release/wincommander-svc.exe");
    expect(releaseTool).toContain('["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-svc", "--release"]');
    expect(releaseTool).toContain('const serviceResource = "resources/wincommander-svc.exe"');
    expect(releaseTool).toContain("copyFileSync(serviceBuildPath, stagedServicePath)");
    expect(releaseTool).toContain('config.bundle.targets = ["nsis"]');
    expect(releaseTool).toContain('config.bundle.resources = [...config.bundle.resources.filter');
    expect(releaseTool).toContain('rmSync(generatedConfigPath, { force: true })');
    expect(releaseTool).toContain('rmSync(stagedServicePath, { force: true })');
    expect(packageJson.scripts["build:free:release-installer"]).toContain("bun run tools/build-tauri-release.ts");
    expect(packageJson.scripts["build:free:release-installer"]).not.toContain("build:pro");
    expect(packageJson.scripts.build).toContain("bun run build:service:release");
  });

  test("builds and publishes the NSIS artifact that exercises the service lifecycle", () => {
    expect(releaseWorkflow).toContain("run: bun run build:free:release-installer");
    expect(releaseWorkflow).not.toContain("run: bun tauri build --config src-tauri/commander-free/tauri.conf.json");
    expect(releaseWorkflow).toContain('bundle/nsis" -Filter "WinCommander*${version}*_x64-setup.exe"');
    expect(releaseWorkflow).toContain("Verify Free setup installs and removes WinCommanderSvc");
    expect(releaseWorkflow).toContain('Join-Path $env:ProgramFiles "WinCommander\\\\wincommander-svc.exe"');
    expect(releaseWorkflow).toContain('Get-CimInstance Win32_Service -Filter "Name=\'WinCommanderSvc\'"');
    expect(releaseWorkflow).toContain('Get-ItemProperty -LiteralPath "Registry::HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\WinCommanderSvc"');
    expect(releaseWorkflow).toContain('$serviceRegistry.Start -ne 2');
    expect(releaseWorkflow).toContain('CIM StartMode=$($service.StartMode)');
    expect(releaseWorkflow).toContain('$running.WaitForStatus("Running", [TimeSpan]::FromSeconds(30))');
    expect(releaseWorkflow).toContain('Join-Path $env:ProgramFiles "WinCommander\\\\uninstall.exe"');
    expect(releaseWorkflow).toContain('Join-Path $env:ProgramFiles "WinCommander\\installer-lifecycle.log"');
    expect(releaseWorkflow).toContain('Get-Content -LiteralPath $diagnosticLog -Raw | Write-Host');
    expect(releaseWorkflow).toContain('Remove-Item -LiteralPath $diagnosticLog -Force');
    expect(releaseWorkflow).toContain('"SETUP_PATH=$($setup.FullName)"');
    expect(releaseWorkflow).toContain('"$remote/free/latest.exe"');
    expect(releaseWorkflow).not.toContain("MSI_PATH=");
    expect(releaseWorkflow).not.toContain("latest.msi");
    expect(publishTagWorkflow).toContain("signed NSIS setup release");
    expect(publishTagWorkflow).not.toContain("signed MSI release");
    expect(prepareReleaseWorkflow).toContain("signed Free NSIS setup workflow");
    expect(prepareReleaseWorkflow).not.toContain("signed Free MSI workflow");
  });

  test("uses a fixed quoted Program Files service path with checked lifecycle commands", () => {
    // The probe-verified NSIS form compiles to `\"\\\"path\\\"\"`, the
    // one argument sc.exe needs for a quoted Program Files ImagePath.
    const scmImagePathArgument = String.raw`"\$\"` + '${WC_SERVICE_EXE}' + String.raw`\$\""`;

    expect(hooks).toContain('!define WC_SERVICE_EXE "${WC_INSTALL_DIR}\\wincommander-svc.exe"');
    expect(hooks).toContain('StrCpy $INSTDIR "${WC_INSTALL_DIR}"');
    expect(hooks).toContain("${If} $R6 == 1");
    expect(hooks).toContain("sc stop WinCommanderSvc");
    expect(hooks).toContain('findstr /C:": 1  STOPPED"');
    expect(hooks).not.toContain("WaitForStatus([System.ServiceProcess.ServiceControllerStatus]::Stopped");
    expect(hooks).not.toContain("Get-Service -Name WinCommanderSvc");
    expect(hooks).not.toContain("$$svc");
    expect(hooks).not.toContain("''WinCommanderSvc''");
    expect(hooks).toContain('reg.exe export "HKLM\\SYSTEM\\CurrentControlSet\\Services\\WinCommanderSvc" "${WC_SERVICE_CONFIG_BACKUP}" /y');
    expect(hooks).toContain('reg.exe import "${WC_SERVICE_CONFIG_BACKUP}"');
    expect(hooks).toContain('!define WC_SERVICE_BACKUP "${WC_INSTALL_DIR}\\wincommander-svc.exe.wc-backup"');
    expect(hooks).toContain('!define WC_LIFECYCLE_DIAGNOSTIC_LOG "${WC_INSTALL_DIR}\\installer-lifecycle.log"');
    expect(hooks).toContain('!macro WC_WRITE_LIFECYCLE_DIAGNOSTIC');
    expect(hooks).toContain('FileWrite $R0 "stage=$R3 exit=$R8 reason=$R4$\\r$\\n"');
    expect(hooks).toContain('StrCpy $R3 "service-start"');
    expect(hooks).toContain('wc_service_rollback:\n    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC');
    expect(hooks).toContain('StrCpy $R4 "WinCommander service payload is missing; the installation was not completed."');
    expect(hooks).toContain('StrCpy $R4 "WinCommander service executable could not be backed up."');
    expect(hooks).toContain('kernel32::CopyFileW(w "${WC_SERVICE_EXE}", w "${WC_SERVICE_BACKUP}", i 0)');
    expect(hooks).toContain('kernel32::CopyFileW(w "${WC_BUNDLED_SERVICE}", w "${WC_SERVICE_EXE}", i 0)');
    expect(hooks).toContain('kernel32::CopyFileW(w "${WC_SERVICE_BACKUP}", w "${WC_SERVICE_EXE}", i 0)');
    expect(hooks).not.toContain('CopyFiles /SILENT "${WC_BUNDLED_SERVICE}"');
    expect(hooks).toContain(`sc.exe create WinCommanderSvc binPath= ${scmImagePathArgument} start= auto obj= LocalSystem`);
    expect(hooks).not.toContain('sc create WinCommanderSvc binPath= """"${WC_SERVICE_EXE}""""');
    expect(hooks).toContain('IfFileExists "${WC_BUNDLED_SERVICE}" wc_service_payload_ready 0');
    expect(hooks).toContain("wc_service_payload_ready:");
    expect(hooks).not.toContain('IfFileExists "${WC_BUNDLED_SERVICE}" +2 0');
    expect(hooks).toContain(`sc.exe config WinCommanderSvc binPath= ${scmImagePathArgument} start= auto obj= LocalSystem`);
    expect(hooks).toContain('${If} $R8 == 1073');
    expect(hooks).toContain('${AndIf} $R6 == 0');
    expect(hooks).toContain('sc.exe create WinCommanderEncVol type= kernel binPath= "\\$\\"${WC_ENCVOL_DRIVER}\\$\\"" start= system');
    expect(hooks).toContain('sc.exe config WinCommanderEncVol type= kernel binPath= "\\$\\"${WC_ENCVOL_DRIVER}\\$\\"" start= system');
    expect(hooks).toContain("sc failure WinCommanderSvc reset= 86400 actions= restart/5000/restart/5000/none/0");
    expect(hooks).toContain("sc start WinCommanderSvc");
    expect(hooks).toContain("sc delete WinCommanderSvc");
    expect(hooks).toContain('Delete "${WC_SERVICE_EXE}"');
  });

  test("preserves third-party encryption drivers and waits before removing its own", () => {
    const uninstall = hooks.indexOf("!macro NSIS_HOOK_PREUNINSTALL");
    const uninstallHooks = hooks.slice(uninstall);
    const stop = uninstallHooks.indexOf("sc stop WinCommanderEncVol");
    const stopped = uninstallHooks.indexOf('sc query WinCommanderEncVol | findstr /C:": 1  STOPPED"');
    const remove = uninstallHooks.indexOf("sc delete WinCommanderEncVol");

    expect(hooks).toContain("sc query WinCommanderEncVol");
    expect(hooks).not.toContain("sc stop veracrypt");
    expect(hooks).not.toContain("sc delete veracrypt");
    expect(uninstall).toBeGreaterThanOrEqual(0);
    expect(stop).toBeGreaterThanOrEqual(0);
    expect(stopped).toBeGreaterThan(stop);
    expect(remove).toBeGreaterThan(stopped);
    expect(hooks).toContain("Restart Windows, then run the uninstaller again.");
  });

  test("repairs only the fixed owned encryption driver on install or update", () => {
    const quotedDriverArgument = String.raw`"\$\"` + '${WC_ENCVOL_DRIVER}' + String.raw`\$\""`;

    expect(hooks).toContain('!define WC_ENCVOL_DRIVER "$PROGRAMDATA\\WinCommander\\bin\\engine\\EncVolKm.sys"');
    expect(hooks).toContain('IfFileExists "${WC_ENCVOL_DRIVER}" wc_encvol_driver_present wc_encvol_driver_ready');
    expect(hooks).toContain("sc query WinCommanderEncVol");
    expect(hooks).toContain('sc qc WinCommanderEncVol | findstr /I /C:"${WC_ENCVOL_DRIVER}"');
    expect(hooks).toContain(`sc.exe create WinCommanderEncVol type= kernel binPath= ${quotedDriverArgument} start= system`);
    expect(hooks).toContain(`sc.exe config WinCommanderEncVol type= kernel binPath= ${quotedDriverArgument} start= system`);
    expect(hooks).toContain("sc start WinCommanderEncVol");
    expect(hooks).toContain("wc_encvol_driver_rollback:");
    expect(hooks).not.toContain("sc create veracrypt");
    expect(hooks).not.toContain("sc config veracrypt");
  });

  test("lets ordinary users run with their own token while debug stays asInvoker", () => {
    expect(manifest).toContain('requestedExecutionLevel level="highestAvailable"');
    expect(manifest).not.toContain("requireAdministrator");
    expect(buildScript).toContain('const HIGHEST_AVAILABLE_LEVEL: &str = r#"level="highestAvailable""#;');
    expect(buildScript).toContain('replacen(HIGHEST_AVAILABLE_LEVEL, r#"level="asInvoker""#, 1)');
    expect(hooks).toContain("-RunLevel Limited");
  });
});
