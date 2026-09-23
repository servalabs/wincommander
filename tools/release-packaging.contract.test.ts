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
        template?: string;
        startMenuFolder?: string;
      };
    };
  };
};
const releaseTool = readFileSync("tools/build-tauri-release.ts", "utf8");
const manifest = readFileSync("src-tauri/commander-free/app.manifest", "utf8");
const buildScript = readFileSync("src-tauri/commander-free/build.rs", "utf8");
const releaseWorkflow = readFileSync(".github/workflows/release.yml", "utf8");
const legacyLaunchMigration = readFileSync(
  "src-tauri/commander-free/nsis/migrate-legacy-user-launches.ps1",
  "utf8",
);
const closeInstalledApp = readFileSync(
  "src-tauri/commander-free/nsis/close-installed-app.ps1",
  "utf8",
);
const elevatedLaunchers = readFileSync(
  "src-tauri/commander-free/nsis/configure-elevated-launchers.ps1",
  "utf8",
);
const proInstaller = readFileSync("src-tauri/commander-free/src/pro_install.rs", "utf8");
const sharedSettingsRepair = readFileSync(
  "src-tauri/commander-free/nsis/repair-shared-settings.ps1",
  "utf8",
);
const sharedSettingsRepairTest = readFileSync("tools/test-shared-settings-repair.ps1", "utf8");

describe("Free machine-wide release packaging", () => {
  test("uses a shared Program Files install and a product-specific Start Menu location", () => {
    expect(baseConfig.bundle.targets).toBe("nsis");
    expect(baseConfig.bundle.windows.nsis.installMode).toBe("perMachine");
    expect(baseConfig.bundle.windows.nsis.installerHooks).toBe("nsis/hooks.nsh");
    expect(baseConfig.bundle.windows.nsis.template).toBe("nsis/installer.nsi");
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
    expect(hooks).toContain("WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT");
    expect(hooks).toContain("close-installed-app.ps1");
    expect(hooks).toContain('!insertmacro WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT "preinstall"');
    expect(hooks).toContain('!insertmacro WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT "uninstall"');
    expect(hooks).toContain("Win32_Process.ExecutablePath matching rather than a broad `/IM` kill");
    expect(closeInstalledApp).toContain("Win32_Process");
    expect(closeInstalledApp).toContain("$process.ExecutablePath");
    expect(closeInstalledApp).toContain("taskkill.exe\" /PID $processId");
    expect(closeInstalledApp).toContain("/PID $processId /F");
    expect(closeInstalledApp).toContain("Normal close request for WinCommander process ID $processId was refused; waiting before forced shutdown");
    expect(closeInstalledApp).toContain("Forced close request for WinCommander process ID $processId");
    expect(closeInstalledApp).not.toContain(" /IM ");
    expect(proInstaller).toContain("Name = 'wincommander-pro.exe'");
    expect(proInstaller).toContain("$_.ExecutablePath -and ($_.ExecutablePath -ieq $target)");
    expect(proInstaller).toContain("verified Pro process still running");
    expect(proInstaller).toContain("atomic_replace_shared_file");
    expect(proInstaller).toContain("MACHINE_PRO_UPDATE_FLAG");
    expect(proInstaller).toContain("ShellExecuteExW");
    expect(proInstaller).toContain("if !crate::startup_elevation::is_current_process_elevated()");
    expect(hooks).toContain("configure-elevated-launchers.ps1");
    expect(hooks).toContain("WinCommander Elevated Launcher");
    expect(hooks).toContain("WinCommander Elevated Autostart");
    expect(elevatedLaunchers).toContain("$manualTaskName = 'WinCommander Elevated Launcher'");
    expect(elevatedLaunchers).toContain("$autostartTaskName = 'WinCommander Autostart'");
    expect(elevatedLaunchers).toContain("$obsoleteElevatedAutostartTaskName = 'WinCommander Elevated Autostart'");
    expect(elevatedLaunchers).toContain("Register-LogonRouterTask");
    expect(elevatedLaunchers).toContain("-Argument '--autostart'");
    expect(elevatedLaunchers).not.toContain("--elevated-relaunch --autostart");
    expect(hooks).toContain("WC_SERVICE_STOP_TIMEOUT_SECONDS 135");
    expect(hooks).toContain("SCM's STATE, CHECKPOINT and WAIT_HINT");
    expect(hooks).toContain('!insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT "uninstall" "un"');
    expect(hooks).toContain('!insertmacro WC_DELETE_OWNED_SERVICE_OR_ABORT "uninstall"');
    expect(hooks).toContain("sc.exe delete ${WC_SERVICE_NAME}");
    expect(hooks).toContain("installer-lifecycle.log");
    expect(hooks).toContain('nsExec::ExecToStack \'sc.exe query ${WC_SERVICE_NAME}\'');
    expect(hooks).not.toContain("cmd.exe /c sc query ${WC_SERVICE_NAME} ^| findstr");
    expect(hooks).toContain("WC_LEGACY_LAUNCH_MIGRATION");
    expect(hooks).toContain("WC_REPAIR_SHARED_MACHINE_DATA_ACL_OR_ABORT");
    expect(hooks).toContain("repair-shared-settings.ps1");
    expect(hooks).toContain("shared-settings-repair");
    expect(hooks).not.toContain("/A /R /D Y");
    expect(hooks).not.toContain("/T /C");
    expect(elevatedLaunchers).toContain("-MultipleInstances Parallel");
    expect(elevatedLaunchers).not.toContain("-MultipleInstances IgnoreNew");
    expect(hooks).toContain('!insertmacro WC_REPAIR_SHARED_MACHINE_DATA_ACL_OR_ABORT "postinstall"');
    expect(hooks).toContain("NSIS_HOOK_PREINSTALL");
    expect(hooks).toContain("WinCommander-license_cache.upgrade-backup.json");
    expect(hooks).toContain("wincommander-migrate-legacy-user-launches.ps1");
    expect(hooks).toContain("-SharedExecutable");
    expect(legacyLaunchMigration).toContain("ProfileList");
    expect(legacyLaunchMigration).toContain("wincommander-free.exe");
    expect(legacyLaunchMigration).toContain("WinCommander legacy launch migration");
    expect(legacyLaunchMigration).toContain("'resources', 'scripts'");
    expect(legacyLaunchMigration).toContain("file-search");
    expect(legacyLaunchMigration).toContain("HKEY_USERS");
    expect(legacyLaunchMigration).toContain("Join-Path $profile.Path 'Desktop'");
    expect(legacyLaunchMigration).not.toContain("Remove-Item -LiteralPath $legacyRoot");
    expect(hooks).not.toContain("WC_PRO_PAYLOAD");
    expect(hooks).not.toContain("WC_PRO_EXE");
    expect(hooks).toContain('net.exe localgroup "WinCommander Vault Policy Administrators" /add');
    expect(hooks).toContain('cmd.exe /c net.exe localgroup "WinCommander Vault Policy Administrators" "%USERNAME%" /add');
    expect(hooks).not.toContain('Abort "WinCommander could not create the Vault Policy Administrators group."');
    expect(hooks).not.toContain('Abort "WinCommander could not grant Vault policy administration to the installing account."');
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

  test("repairs shared settings ACLs without PowerShell Security cmdlets or wrapper-only properties", () => {
    expect(sharedSettingsRepair).toContain("Add-Type -AssemblyName System.IO.FileSystem.AccessControl");
    expect(sharedSettingsRepair).toContain("FileSystemAclExtensions]::SetAccessControl");
    expect(sharedSettingsRepair).toContain("$entry.SetAccessControl");
    expect(sharedSettingsRepair).toContain("$acl = Get-EntrySecurity $Path $Directory");
    expect(sharedSettingsRepair).not.toContain("$acl.SetOwner");
    expect(sharedSettingsRepair).toContain("Shared settings ownership verification failed.");
    expect(sharedSettingsRepair).not.toMatch(/^\s*(Get|Set)-Acl\b/m);
    expect(sharedSettingsRepairTest).toContain("GetSecurityDescriptorSddlForm");
    expect(sharedSettingsRepairTest).not.toContain(".Sddl");
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
    expect(hooks).toContain('${GetOptions} $CMDLINE "/UPDATE" $R7');
    expect(hooks).toContain("An update must retain");
    expect(hooks).toContain('ReadEnvStr $R5 "ProgramData"');
    expect(hooks).toContain('RMDir /r "$R5\\WinCommander"');
    expect(hooks.indexOf('${GetOptions} $CMDLINE "/UPDATE" $R7')).toBeLessThan(
      hooks.indexOf('RMDir /r "$R5\\WinCommander"'),
    );
    expect(hooks).toContain('license_cache.json');
    expect(hooks).toContain('icacls.exe "$R5\\WinCommander" /inheritance:r');
    expect(hooks).not.toContain('IfFileExists "$PROGRAMDATA\\');
    expect(hooks).not.toContain('IfFileExists "$COMMONAPPDATA\\');
    expect(hooks).toContain('RMDir /r "$LOCALAPPDATA\\WinCommander"');
  });

  test("manual NSIS setup replaces the installed app in place before any old uninstaller can run", () => {
    const template = readFileSync("src-tauri/commander-free/nsis/installer.nsi", "utf8");
    const reinstall = template.slice(template.indexOf("Function PageReinstall"), template.indexOf("FunctionEnd", template.indexOf("Function PageReinstall")));
    expect(reinstall).toMatch(/\$WixMode != 1\r?\n\s+!if "\$\{ALLOWDOWNGRADES\}" == "true"\r?\n\s+Abort/);
    expect(reinstall).toMatch(/!else\r?\n\s+\$\{If\} \$R0 != -1\r?\n\s+Abort/);
    expect(reinstall.indexOf("${If} $WixMode != 1")).toBeLessThan(reinstall.indexOf("${If} $R0 = 0"));
    expect(template).toContain('Page custom PageReinstall PageLeaveReinstall');
    expect(template).toContain('ExecWait \'$R1\' $0');
    expect(template).toContain('Function un.onInit');
    expect(template).toContain('Section Uninstall');
    const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");
    expect(hooks).toContain('!define WC_REPAIR_VAULT_DRIVER_ACCESS "${__FILEDIR__}\\..\\..\\..\\tools\\repair-vault-driver-access.ps1"');
    expect(hooks).toContain('IfFileExists "$R5\\WinCommander\\bin\\engine" 0 wc_no_driver_access_repair');
    expect(hooks.indexOf('"vault-driver-access-repair"')).toBeLessThan(hooks.indexOf('sc.exe start ${WC_SERVICE_NAME}'));
  });
});
