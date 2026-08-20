import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/vault/RamDisksSection.tsx", "utf8");
const startupSource = readFileSync("src/components/BackgroundPollers.tsx", "utf8");
const backendSource = readFileSync("src-tauri/commander-free/scripts/modules/vault/ramdisks.ps1", "utf8");

test("enabling RAM-disk autostart requires a saved user-selected specification", () => {
  expect(source).toContain("Do not silently save the 256 MB fallback");
  expect(source).toContain("setAutostartConfigOpen(true)");
  expect(source).toContain("if (await saveAutostart())");
  expect(source).not.toContain("void saveAutostart({ enabled: next })");
});

test("startup never converts a missing saved size into a 256 MB disk", () => {
  expect(startupSource).toContain("RAM disk autostart needs a saved size");
  expect(startupSource).toContain("const mountRequest = savedRamDiskMountRequest(cfg)");
  expect(startupSource).toContain("if (!mountRequest)");
});

test("saving an enabled startup specification mounts it immediately", () => {
  expect(source).toContain("const mountSavedAutostartSpec = useCallback");
  expect(source).toContain("await mountSavedAutostartSpec(next)");
  expect(source).toContain("const mountRequest = savedRamDiskMountRequest(spec)");
  expect(source).toContain("await createRamDisk(mountRequest)");
  expect(source).toContain("Startup RAM disk saved and mounted at");
});

test("the backend keeps three GB of total RAM outside every mount request", () => {
  expect(backendSource).toContain("$headroomMB = 3072");
  expect(backendSource).toContain("$capMB = [int]($sysRam.totalMB - $headroomMB)");
  expect(backendSource).toContain("$sizeInt -gt $capMB");
});
