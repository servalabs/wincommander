import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

test("Dashboard primes its live DNS readout on mount", () => {
  const sourcePath = new URL("./index.tsx", import.meta.url).pathname.replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
  const source = readFileSync(sourcePath, "utf8");

  expect(source).toContain("void refreshNetwork(true);");
  expect(source).toContain("}, [refreshNetwork]);");
});

test("Fix All uses the all-users scope chosen in Settings, without a dashboard control", () => {
  const sourcePath = new URL("./index.tsx", import.meta.url).pathname.replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
  const source = readFileSync(sourcePath, "utf8");

  expect(source).toContain("applyFixAllMachineWide === true");
  expect(source).toContain("MachineWide: machineWide");
  expect(source).toContain("if (machineWide && needsElevation)");
  expect(source).not.toContain("dashboard-fix-all-scope");
  expect(source).not.toContain("handleApplyFixAllMachineWideChange");
});
