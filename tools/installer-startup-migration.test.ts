import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test.skipIf(process.platform !== "win32")("upgrade cleans each profile's expandable startup routes without touching foreign launches", () => {
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "tools/test-installer-startup-migration.ps1"], { encoding: "utf8", windowsHide: true });
  expect(result.stderr.trim()).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("PASS: profile startup cleanup");
  expect(result.stdout).toContain("PASS: complete upgrade replaces admin and standard profile routes and preserves data");
  expect(result.stdout).toContain("PASS: both legacy install layouts preserve disabled tasks for every profile");
  expect(result.stdout).toContain("PASS: maintenance migration cannot resurrect startup");
});

const nsis = join(process.env.LOCALAPPDATA ?? "", "tauri", "NSIS", "makensis.exe");
test.skipIf(process.platform !== "win32" || !existsSync(nsis))("installer diagnostics append each stage without overwriting earlier migration evidence", () => {
  const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");
  const macro = hooks.match(/!macro WC_WRITE_LIFECYCLE_DIAGNOSTIC stage exit detail\r?\n[\s\S]*?!macroend/)![0];
  const fixture = mkdtempSync(join(tmpdir(), "wc-installer-log-"));
  try {
    const source = join(fixture, "fixture.nsi");
    const executable = join(fixture, "fixture.exe");
    // This user-level harness executes only the extracted logging macro. It
    // never runs setup, installs an app, or changes startup/service state.
    writeFileSync(source, `Unicode true\nName "Log fixture"\nOutFile "${executable}"\nRequestExecutionLevel user\nSilentInstall silent\n!define WC_LIFECYCLE_DIAGNOSTIC_LOG "$INSTDIR\\installer-lifecycle.log"\n${macro}\nSection\nStrCpy $INSTDIR "${fixture}"\n!insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "startup-migration" "0" "first long result"\n!insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "final" "0" "ok"\nSectionEnd\n`);
    const compiled = spawnSync(nsis, ["/V2", source], { encoding: "utf8", windowsHide: true });
    expect(compiled.status).toBe(0);
    const result = spawnSync(executable, [], { encoding: "utf8", windowsHide: true });
    expect(result.status).toBe(0);
    expect(readFileSync(join(fixture, "installer-lifecycle.log"), "utf8").split(/\r?\n/).filter(Boolean)).toEqual([
      "stage=startup-migration exit=0 detail=first long result",
      "stage=final exit=0 detail=ok",
    ]);
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});

test("manual reinstall and updater preserve legacy OFF using the same reconciliation invocation", () => {
  const hooks = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");
  const macro = hooks.match(/!macro WC_CONFIGURE_ELEVATED_LAUNCHERS\r?\n([\s\S]*?)!macroend/)![1];
  const calls = macro.split(/\r?\n/).filter(line => line.includes("nsExec::ExecToStack"));
  expect(calls.length).toBeGreaterThan(0);
  expect(calls.every(line => line.includes("-PreserveAutostartPreference"))).toBe(true);
});
