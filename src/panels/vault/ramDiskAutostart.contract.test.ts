import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/vault/RamDisksSection.tsx", "utf8");
const startupSource = readFileSync("src/components/BackgroundPollers.tsx", "utf8");

test("enabling RAM-disk autostart requires a saved user-selected specification", () => {
  expect(source).toContain("Do not silently save the 256 MB fallback");
  expect(source).toContain("setAutostartConfigOpen(true)");
  expect(source).toContain("if (await saveAutostart())");
  expect(source).not.toContain("void saveAutostart({ enabled: next })");
});

test("startup never converts a missing saved size into a 256 MB disk", () => {
  expect(startupSource).toContain("RAM disk autostart needs a saved size");
  expect(startupSource).toContain("configuredSizeMB < MIN_RAM_DISK_SIZE_MB");
  expect(startupSource).toContain("const sizeMB = normalizeRamDiskSizeMB(configuredSizeMB)");
});

test("saving an enabled startup specification mounts it immediately", () => {
  expect(source).toContain("const mountSavedAutostartSpec = useCallback");
  expect(source).toContain("await mountSavedAutostartSpec(next)");
  expect(source).toContain("Startup RAM disk saved and mounted at");
});
