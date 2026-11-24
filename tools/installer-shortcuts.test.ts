import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";

const installer = readFileSync("src-tauri/commander-free/nsis/installer.nsi", "utf8").replace(/\r\n/g, "\n");
const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");

test("setup keeps its shortcut creation points and ordering", () => {
  // Install creates the Start Menu entry, then the Desktop one for silent and
  // passive runs, then records both as offered, all before POSTINSTALL.
  const calls = ["Call CreateOrUpdateStartMenuShortcut", "Call CreateOrUpdateDesktopShortcut", '"StartMenuShortcutOffered" 1', '"DesktopShortcutOffered" 1', "NSIS_HOOK_POSTINSTALL"].map((needle) => installer.indexOf(needle, installer.indexOf("Section Install")));
  expect(calls.every((index) => index > 0)).toBe(true);
  expect([...calls].sort((a, b) => a - b)).toEqual(calls);
  // The marker must never be read after the install section wrote it.
  expect(installer).toContain('ReadRegStr $R4 SHCTX "${UNINSTKEY}" "$R1"');
  // Update mode alone must not suppress a shortcut any more.
  expect(installer).not.toMatch(/\$\{If\} \$UpdateMode = 1\r?\n\s+\$\{OrIf\} \$NoShortcutMode = 1/);
  // The parked names are those the service moves a hidden shortcut to.
  const store = readFileSync("src-tauri/commander-svc/src/shared_shortcuts_store.rs", "utf8");
  expect(store).toContain('backup: root.join("desktop.lnk")');
  expect(store).toContain('backup: root.join("start.lnk")');
  expect(store).toContain('join("WinCommanderSharedShortcuts")');
  expect(installer).toContain('\\WinCommanderSharedShortcuts\\$R2');
});

test("uninstall removes the shortcuts only when it is not an update", () => {
  const body = installer.slice(installer.indexOf("Section Uninstall"));
  const removal = body.slice(body.indexOf("; Remove shortcuts if not updating"));
  expect(removal.startsWith("; Remove shortcuts if not updating\n  ${If} $UpdateMode <> 1")).toBe(true);
  expect(removal.indexOf('Delete "$SMPROGRAMS\\$AppStartMenuFolder\\${PRODUCTNAME}.lnk"')).toBeGreaterThan(0);
  expect(removal.indexOf('Delete "$DESKTOP\\${PRODUCTNAME}.lnk"')).toBeGreaterThan(0);
  // The hooks never delete a shortcut, so nothing else removes them on update.
  expect(hooks).not.toMatch(/Delete[^\n]*\.lnk/);
});

const nsisCandidates = [join(process.env["ProgramFiles(x86)"] ?? "", "NSIS", "makensis.exe"), join(process.env.LOCALAPPDATA ?? "", "tauri", "NSIS", "makensis.exe")];
const nsis = nsisCandidates.find((candidate) => existsSync(candidate));

// Runs the real shortcut functions and install-section tail from installer.nsi
// inside a user-level harness. Only the Windows shell lookups are replaced:
// shortcuts go to a temporary folder and the markers to a scratch HKCU key.
// The produced setup is never started and no real shortcut or service is touched.
function runScenario(options: { args: string[]; markers?: boolean; shortcuts?: Array<"start" | "desktop">; parked?: Array<"start" | "desktop"> }) {
  const fixture = mkdtempSync(join(tmpdir(), "wc-installer-shortcuts-"));
  const key = `Software\\WinCommanderInstallerShortcutTest-${randomUUID()}`;
  const reg = (...args: string[]) => spawnSync("reg.exe", args, { encoding: "utf8", windowsHide: true });
  const start = join(fixture, "sm", "ServaLabs", "WinCommander", "WinCommander.lnk");
  const desktop = join(fixture, "desk", "WinCommander.lnk");
  try {
    const functions = installer.slice(installer.indexOf("Function ShouldRestoreShortcut"));
    expect(functions).toContain("Function CreateOrUpdateDesktopShortcut");
    const tail = installer.slice(installer.indexOf("  ; Create start menu shortcut\n"), installer.indexOf("  !ifmacrodef NSIS_HOOK_POSTINSTALL"));
    const shell = (text: string) => text.replaceAll("$SMPROGRAMS", "$INSTDIR\\sm").replaceAll("$DESKTOP", "$INSTDIR\\desk");
    const source = `Unicode true
Name "Shortcut fixture"
OutFile "${join(fixture, "fixture.exe")}"
RequestExecutionLevel user
SilentInstall silent
InstallDir "${fixture}"
!include LogicLib.nsh
!include FileFunc.nsh
!define UNINSTKEY "${key}"
!define PRODUCTNAME "WinCommander"
!define MAINBINARYNAME "wincommander-free"
!define STARTMENUFOLDER "ServaLabs\\WinCommander"
!define BUNDLEID "fixture"
Var PassiveMode
Var UpdateMode
Var NoShortcutMode
Var WixMode
Var OldMainBinaryName
Var AppStartMenuFolder
!macro WC_LOAD_PROGRAMDATA_OR_ABORT stage
  StrCpy $R5 "$INSTDIR\\ProgramData"
!macroend
!macro IsShortcutTarget shortcut target
  StrCpy $3 0
  \${If} \${FileExists} "\${shortcut}"
  \${AndIf} "\${target}" == "$INSTDIR\\wincommander-free.exe"
    StrCpy $3 1
  \${EndIf}
  Push $3
!macroend
!macro SetShortcutTarget shortcut target
!macroend
!macro SetLnkAppUserModelId shortcut
!macroend
!macro MUI_STARTMENU_WRITE_BEGIN id
!macroend
!macro MUI_STARTMENU_WRITE_END
!macroend
${shell(functions)}
Section
  CreateDirectory "$INSTDIR\\desk"
  StrCpy $PassiveMode 0
  StrCpy $WixMode 0
  StrCpy $AppStartMenuFolder "\${STARTMENUFOLDER}"
  \${GetOptions} $CMDLINE "/UPDATE" $0
  \${IfNot} \${Errors}
    StrCpy $UpdateMode 1
  \${EndIf}
  \${GetOptions} $CMDLINE "/NS" $0
  \${IfNot} \${Errors}
    StrCpy $NoShortcutMode 1
  \${EndIf}
  \${GetOptions} $CMDLINE "/OLD" $0
  \${IfNot} \${Errors}
    StrCpy $OldMainBinaryName "wincommander-free.exe"
  \${EndIf}
${shell(tail)}
SectionEnd
`;
    writeFileSync(join(fixture, "fixture.nsi"), source);
    const compiled = spawnSync(nsis!, ["/V2", join(fixture, "fixture.nsi")], { encoding: "utf8", windowsHide: true });
    expect(compiled.stdout + compiled.stderr).not.toMatch(/warning|error/i);
    expect(compiled.status).toBe(0);

    const place = (path: string) => {
      mkdirSync(join(path, ".."), { recursive: true });
      writeFileSync(path, "existing");
    };
    for (const name of options.shortcuts ?? []) place(name === "start" ? start : desktop);
    for (const name of options.parked ?? []) place(join(fixture, "ProgramData", "WinCommanderSharedShortcuts", `${name}.lnk`));
    if (options.markers) {
      for (const name of ["StartMenuShortcutOffered", "DesktopShortcutOffered"]) reg("add", `HKCU\\${key}`, "/v", name, "/t", "REG_DWORD", "/d", "1", "/f");
    }
    const run = spawnSync(join(fixture, "fixture.exe"), ["/S", ...options.args], { encoding: "utf8", windowsHide: true });
    expect(run.status).toBe(0);
    const marker = (name: string) => /0x1\b/.test(reg("query", `HKCU\\${key}`, "/v", name).stdout);
    return {
      start: existsSync(start) ? readFileSync(start, "latin1").slice(0, 8) : null,
      desktop: existsSync(desktop) ? readFileSync(desktop, "latin1").slice(0, 8) : null,
      startOffered: marker("StartMenuShortcutOffered"),
      desktopOffered: marker("DesktopShortcutOffered"),
    };
  } finally {
    reg("delete", `HKCU\\${key}`, "/f");
    rmSync(fixture, { recursive: true, force: true });
  }
}

const harness = process.platform !== "win32" || !nsis;
const real = "L\u0000\u0000\u0000"; // a Windows .lnk begins with header size 0x4c

test.skipIf(harness)("fresh silent install creates both shortcuts and records them", () => {
  const result = runScenario({ args: [] });
  expect(result.start?.startsWith(real)).toBe(true);
  expect(result.desktop?.startsWith(real)).toBe(true);
  expect(result.startOffered && result.desktopOffered).toBe(true);
});

test.skipIf(harness)("/UPDATE keeps shortcuts the user still has and never overwrites them", () => {
  const result = runScenario({ args: ["/UPDATE", "/OLD"], shortcuts: ["start", "desktop"] });
  expect(result.start).toBe("existing");
  expect(result.desktop).toBe("existing");
  expect(result.startOffered && result.desktopOffered).toBe(true);
});

test.skipIf(harness)("/UPDATE over an install that predates the markers restores both missing shortcuts", () => {
  const result = runScenario({ args: ["/UPDATE", "/OLD"] });
  expect(result.start?.startsWith(real)).toBe(true);
  expect(result.desktop?.startsWith(real)).toBe(true);
  expect(result.startOffered && result.desktopOffered).toBe(true);
});

test.skipIf(harness)("/UPDATE leaves a shortcut the user removed after setup offered it", () => {
  const result = runScenario({ args: ["/UPDATE", "/OLD"], markers: true, shortcuts: ["start"] });
  expect(result.start).toBe("existing");
  expect(result.desktop).toBeNull();
});

test.skipIf(harness)("/UPDATE does not resurrect a shortcut that hide mode parked", () => {
  const result = runScenario({ args: ["/UPDATE", "/OLD"], parked: ["start", "desktop"] });
  expect(result.start).toBeNull();
  expect(result.desktop).toBeNull();
});

test.skipIf(harness)("/NS creates nothing but still records both shortcuts as offered", () => {
  const result = runScenario({ args: ["/NS"] });
  expect(result.start).toBeNull();
  expect(result.desktop).toBeNull();
  expect(result.startOffered && result.desktopOffered).toBe(true);
});
